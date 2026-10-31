//! Blackjack's Misdirection: holographic copies of the units at the point that take orders and
//! fire, deal no damage, vanish after their time or at the first hit, and to the enemy are units
//! of the real def like any other.

use g2_rules::generals::BLACKJACK;
use g2_rules::ids::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::defs::PowerKind;
use g2_sim::world::{Event, INPUT_DELAY};
use g2_sim::{CommandKind, EntityId, Faction, MatchSetup, Order, PlayerId, PlayerSetup, Reject, Vec2, World};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn setup() -> MatchSetup {
    let player = |faction, general| PlayerSetup {
        faction,
        starting_credits: 100_000,
        ai: None,
        general,
        perks: Vec::new(),
        rank: 1,
        team: 0,
    };
    // A town house beside the APA squads' spot, for the garrison check.
    let mut map = crater_ridge();
    map.civilians = vec![(Vec2::from_int(125, 135), g2_sim::Fx::from_int(3))];
    MatchSetup { map, seed: 1, players: vec![player(Faction::Apa, Some(BLACKJACK)), player(Faction::Eu, None)] }
}

fn step_until(w: &mut World, limit: u32, what: &str, mut done: impl FnMut(&World) -> bool) {
    for _ in 0..limit {
        if done(w) {
            return;
        }
        w.step();
    }
    panic!("{what}: not within {limit} ticks");
}

fn order(w: &mut World, who: PlayerId, cmd: CommandKind) -> Vec<Reject> {
    w.submit(who, cmd);
    let mut out = Vec::new();
    for _ in 0..=INPUT_DELAY {
        w.step();
        out.extend(w.events().iter().filter_map(|e| match e {
            Event::Rejected { player, reason } if *player == who => Some(*reason),
            _ => None,
        }));
    }
    out
}

fn build(w: &mut World, who: PlayerId, def: g2_sim::DefId, pos: Vec2) -> EntityId {
    let builder = w.entities().find(|e| e.owner == Some(who) && w.rules.def(e.def).is_constructor()).unwrap().id;
    let before: Vec<_> = w.entities().filter(|e| e.def == def).map(|e| e.id).collect();
    w.submit(who, CommandKind::Build { builder, def, pos });
    let new = |w: &World| w.entities().find(|e| e.def == def && !before.contains(&e.id)).map(|e| e.id);
    step_until(w, INPUT_DELAY + 2, "foundation", |w| new(w).is_some());
    let site = new(w).unwrap();
    step_until(w, 30 * 120, "construction", |w| w.entity(site).is_some_and(|s| s.is_complete()));
    site
}

fn train_at(w: &mut World, who: PlayerId, producer: EntityId, def: g2_sim::DefId, at: Vec2) -> EntityId {
    let before: Vec<_> = w.entities().filter(|e| e.owner == Some(who) && e.def == def).map(|e| e.id).collect();
    w.submit(who, CommandKind::Train { producer, def });
    let new = |w: &World| {
        w.entities().find(|e| e.owner == Some(who) && e.def == def && !before.contains(&e.id)).map(|e| e.id)
    };
    step_until(w, 30 * 120, "training", |w| new(w).is_some());
    let u = new(w).unwrap();
    w.submit(who, CommandKind::Move { units: vec![u], target: at });
    for _ in 0..=INPUT_DELAY {
        w.step();
    }
    step_until(w, 30 * 90, "arrival", |w| w.entity(u).is_none_or(|e| e.order == Order::Idle));
    u
}

fn decoys(w: &World) -> Vec<EntityId> {
    w.entities().filter(|e| e.decoy_until.is_some()).map(|e| e.id).collect()
}

/// Three Urban Assault squads at a spot and an EU Recon Squad waiting at `ENEMY`, then
/// Misdirection ready and aimed at the squads: one copy each.
fn misdirect(spot: Vec2) -> (World, Vec<EntityId>, Vec<EntityId>, EntityId) {
    let mut w = World::new(&setup(), rules());
    let eu = build(&mut w, P1, EU_BARRACKS, Vec2::from_int(290, 250));
    let enemy = train_at(&mut w, P1, eu, EU_RECON, ENEMY);
    w.submit(P1, CommandKind::Stop { units: vec![enemy] });
    let barracks = build(&mut w, P0, APA_BARRACKS, Vec2::from_int(110, 70));
    let squads: Vec<EntityId> = (0..3).map(|_| train_at(&mut w, P0, barracks, APA_URBAN_ASSAULT, spot)).collect();
    let ready = rules().general(BLACKJACK).power.unwrap().first_ticks;
    while w.tick() < ready {
        w.step();
    }
    assert_eq!(order(&mut w, P0, CommandKind::Power { slot: 0, at: spot }), vec![]);
    let copies = decoys(&w);
    (w, squads, copies, enemy)
}

/// Where the EU squad waits: out of the APA squads' sight at `SPOT`.
const ENEMY: Vec2 = Vec2::from_int(210, 180);
const SPOT: Vec2 = Vec2::from_int(140, 120);

#[test]
fn misdirection_copies_each_unit_at_the_point() {
    assert!(matches!(rules().general(BLACKJACK).power.unwrap().kind, PowerKind::Decoys { .. }));
    let spot = SPOT;
    let (w, squads, copies, _) = misdirect(spot);
    assert_eq!(copies.len(), squads.len(), "one copy per unit in reach");
    for &c in &copies {
        let e = w.entity(c).unwrap();
        assert_eq!(e.def, APA_URBAN_ASSAULT, "a copy is a unit of the real def");
        assert_eq!(e.owner, Some(P0));
        assert!(e.pos.distance(spot) < g2_sim::Fx::from_int(25));
    }
    // Copies take orders.
    let mut w = w;
    let away = Vec2::from_int(170, 120);
    assert_eq!(order(&mut w, P0, CommandKind::Move { units: copies.clone(), target: away }), vec![]);
    let before = w.entity(copies[0]).unwrap().pos;
    for _ in 0..60 {
        w.step();
    }
    assert!(w.entity(copies[0]).unwrap().pos.distance(before) > g2_sim::Fx::from_int(3), "a copy walks");
    // Nothing to copy: refused.
    let mut w2 = World::new(&setup(), rules());
    let ready = rules().general(BLACKJACK).power.unwrap().first_ticks;
    while w2.tick() < ready {
        w2.step();
    }
    assert_eq!(
        order(&mut w2, P0, CommandKind::Power { slot: 0, at: Vec2::from_int(150, 150) }),
        vec![Reject::InvalidTarget]
    );
}

#[test]
fn decoys_vanish_after_their_time() {
    let (mut w, squads, copies, _) = misdirect(SPOT);
    let PowerKind::Decoys { ticks, .. } = rules().general(BLACKJACK).power.unwrap().kind else { unreachable!() };
    let until = w.entity(copies[0]).unwrap().decoy_until.unwrap();
    assert!(until <= w.tick() + ticks);
    while w.tick() + 1 < until {
        w.step();
        assert!(copies.iter().all(|&c| w.entity(c).is_some()), "still there at {}", w.tick());
    }
    w.step();
    w.step();
    assert!(copies.iter().all(|&c| w.entity(c).is_none()), "gone once their time is up");
    assert!(squads.iter().all(|&s| w.entity(s).is_some()), "the real units stay");
    assert_eq!(w.stats(P0).units_lost, 0, "a hologram fading is no loss");
}

#[test]
fn decoys_deal_no_damage_and_vanish_at_the_first_hit() {
    let (mut w, squads, copies, target) = misdirect(SPOT);
    // The real squads walk home, out of the fight; the copies attack-move into the EU squad.
    w.submit(P0, CommandKind::Move { units: squads.clone(), target: Vec2::from_int(80, 60) });
    let full = w.entity(target).unwrap().hp;
    let mut fired = false;
    let mut first_hit_on_copy = None;
    w.submit(P0, CommandKind::AttackMove { units: copies.clone(), target: ENEMY });
    for _ in 0..30 * 18 {
        w.step();
        for e in w.events() {
            if let Event::Hit { shooter, target: t, damage, .. } = *e {
                if copies.contains(&shooter) && t == target {
                    fired = true;
                    assert_eq!(damage, 0, "a hologram's shot deals nothing");
                }
                if copies.contains(&t) && first_hit_on_copy.is_none() {
                    first_hit_on_copy = Some(t);
                }
            }
        }
        if fired && first_hit_on_copy.is_some() {
            break;
        }
    }
    assert!(fired, "the copies fired");
    assert_eq!(w.entity(target).unwrap().hp, full, "the EU squad took no damage");
    let hit = first_hit_on_copy.expect("the EU squad shot back");
    w.step();
    assert!(w.entity(hit).is_none(), "one hit and it is gone");
    assert_eq!(w.stats(P0).units_lost, 0);
    assert_eq!(w.entity(target).unwrap().experience, 0, "no experience for shooting a hologram");
}

/// To the enemy a copy is a unit of the real def, seen and attackable like it: nothing in the
/// public view of it differs except `decoy_until`, which only the owner's client reads.
#[test]
fn decoys_look_like_the_real_unit_to_the_enemy() {
    let (mut w, squads, copies, scout) = misdirect(SPOT);
    // The EU squad walks up until it sees the APA group.
    w.submit(P1, CommandKind::Move { units: vec![scout], target: Vec2::from_int(165, 140) });
    step_until(&mut w, 30 * 15, "in sight", |w| w.sees(P1, w.entity(copies[0]).unwrap()));
    let (real, copy) = (w.entity(squads[0]).unwrap(), w.entity(copies[0]).unwrap());
    assert_eq!(real.def, copy.def);
    assert_eq!(real.owner, copy.owner);
    assert_eq!(w.max_hp(real), w.max_hp(copy));
    assert_eq!(real.hp, copy.hp);
    assert_eq!(w.hidden(real), w.hidden(copy));
    assert_eq!(order(&mut w, P1, CommandKind::Attack { units: vec![scout], target: copies[0] }), vec![]);
}

#[test]
fn copies_cant_garrison() {
    let (mut w, squads, copies, _) = misdirect(SPOT);
    let house = w.entities().find(|e| w.rules.def(e.def).garrison.is_some() && e.owner.is_none()).unwrap().id;
    let mut all = copies.clone();
    all.push(squads[0]);
    assert_eq!(order(&mut w, P0, CommandKind::Garrison { units: all, container: house }), vec![]);
    step_until(&mut w, 30 * 10, "the real squad in", |w| w.entity(squads[0]).unwrap().inside == Some(house));
    assert!(copies.iter().all(|&c| w.entity(c).is_none_or(|e| e.inside.is_none())), "no hologram goes in");
}
