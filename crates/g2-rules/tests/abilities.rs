//! Unit abilities, driven through public commands only: the Battle Bus becomes its bunker on
//! command and when destroyed, occupants and all, and the bunker holds no base; the Shadow
//! Tank's Particle Accelerator fires once at a chosen target, then recharges.

use g2_rules::generals::{BLACKJACK, JUNKYARD};
use g2_rules::ids::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::command::Reject;
use g2_sim::world::{Event, INPUT_DELAY};
use g2_sim::{
    CommandKind, EntityId, Faction, GeneralId, MatchSetup, Order, PlayerId, PlayerSetup, Vec2, World, verify_replay,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn setup(f0: Faction, general: GeneralId, f1: Faction) -> MatchSetup {
    let player = |faction, general| PlayerSetup {
        faction,
        starting_credits: 100_000,
        ai: None,
        general,
        perks: Vec::new(),
        rank: 1,
    };
    MatchSetup { map: crater_ridge(), seed: 1, players: vec![player(f0, Some(general)), player(f1, None)] }
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

/// Rejections `cmd` drew.
fn order(w: &mut World, who: PlayerId, cmd: CommandKind) -> Vec<Reject> {
    w.submit(who, cmd);
    let mut out = Vec::new();
    for _ in 0..=INPUT_DELAY {
        w.step();
        for e in w.events() {
            if let Event::Rejected { player, reason } = e
                && *player == who
            {
                out.push(*reason);
            }
        }
    }
    out
}

fn build(w: &mut World, who: PlayerId, def: g2_sim::DefId, pos: Vec2) -> EntityId {
    let builder = w.entities().find(|e| e.owner == Some(who) && w.rules.def(e.def).is_constructor()).unwrap().id;
    w.submit(who, CommandKind::Build { builder, def, pos });
    step_until(w, INPUT_DELAY + 2, "foundation", |w| w.entities().any(|e| e.owner == Some(who) && e.def == def));
    let site = w.entities().find(|e| e.owner == Some(who) && e.def == def).unwrap().id;
    step_until(w, 30 * 120, "construction", |w| w.entity(site).is_some_and(|s| s.is_complete()));
    site
}

fn train(w: &mut World, who: PlayerId, producer: EntityId, def: g2_sim::DefId) -> EntityId {
    let before: Vec<_> = w.entities().filter(|e| e.owner == Some(who) && e.def == def).map(|e| e.id).collect();
    w.submit(who, CommandKind::Train { producer, def });
    let new = |w: &World| {
        w.entities().find(|e| e.owner == Some(who) && e.def == def && !before.contains(&e.id)).map(|e| e.id)
    };
    step_until(w, 30 * 120, "training", |w| new(w).is_some());
    new(w).unwrap()
}

fn walk(w: &mut World, who: PlayerId, u: EntityId, to: Vec2) {
    w.submit(who, CommandKind::Move { units: vec![u], target: to });
    for _ in 0..=INPUT_DELAY {
        w.step();
    }
    step_until(w, 30 * 120, "arrival", |w| w.entity(u).is_none_or(|e| e.order == Order::Idle));
}

/// Junkyard's bus with two Rabble squads inside, at `at` out in the field.
fn loaded_bus(w: &mut World, at: Vec2) -> (EntityId, [EntityId; 2]) {
    let home = w.map.spawns[0];
    let factory = build(w, P0, GLA_FACTORY, home + Vec2::from_int(30, 0));
    let barracks = build(w, P0, GLA_BARRACKS, home + Vec2::from_int(0, 30));
    let bus = train(w, P0, factory, GLA_BATTLE_BUS);
    let squads = [train(w, P0, barracks, GLA_RABBLE), train(w, P0, barracks, GLA_RABBLE)];
    assert_eq!(order(w, P0, CommandKind::Garrison { units: squads.to_vec(), container: bus }), vec![]);
    step_until(w, 30 * 60, "boarding", |w| squads.iter().all(|&s| w.entity(s).unwrap().inside == Some(bus)));
    walk(w, P0, bus, at);
    (bus, squads)
}

fn bunker_of(w: &World) -> Option<&g2_sim::Entity> {
    w.entities().find(|e| e.owner == Some(P0) && e.def == GLA_BATTLE_BUNKER)
}

/// On command the bus becomes the bunker where it stands, at once, with its squads inside;
/// the bunker can't move, and units that can't become anything are refused.
#[test]
fn the_battle_bus_digs_in_on_command() {
    let s = setup(Faction::Gla, JUNKYARD, Faction::Eu);
    let mut w = World::new(&s, rules());
    let spot = Vec2::from_int(200, 200);
    let (bus, squads) = loaded_bus(&mut w, spot);
    let at = w.entity(bus).unwrap().pos;
    assert_eq!(order(&mut w, P0, CommandKind::Transform { units: vec![squads[0]] }), vec![Reject::CannotTransform]);
    assert_eq!(order(&mut w, P0, CommandKind::Transform { units: vec![bus] }), vec![]);
    assert!(w.entity(bus).is_none(), "the bus is gone");
    let bunker = bunker_of(&w).expect("a bunker").clone();
    assert_eq!(bunker.pos, at, "dug in where it stood");
    assert_eq!(bunker.hp, rules().def(GLA_BATTLE_BUNKER).hp);
    for s in squads {
        assert_eq!(w.entity(s).unwrap().inside, Some(bunker.id), "the squads stay aboard");
    }
    assert_eq!(
        order(&mut w, P0, CommandKind::Move { units: vec![bunker.id], target: spot + Vec2::from_int(30, 0) }),
        vec![Reject::NotOwner]
    );
    assert_eq!(w.entity(bunker.id).unwrap().pos, at);
}

/// A bus destroyed with squads aboard leaves the bunker with the squads alive inside (they
/// die with anything else that holds them); the bunker fires out with the 8-unit bonus.
#[test]
fn a_destroyed_bus_leaves_a_manned_bunker_that_fires_out() {
    let s = setup(Faction::Gla, JUNKYARD, Faction::Eu);
    let mut w = World::new(&s, rules());
    let (bus, squads) = loaded_bus(&mut w, Vec2::from_int(200, 200));
    // Eu tanks drive up (an attack needs a target seen) and shoot the bus.
    let home1 = w.map.spawns[1];
    let factory = build(&mut w, P1, EU_FACTORY, home1 + Vec2::from_int(-30, 0));
    let tanks: Vec<EntityId> = (0..3).map(|_| train(&mut w, P1, factory, EU_AEGIS)).collect();
    let at = w.entity(bus).unwrap().pos;
    w.submit(P1, CommandKind::Move { units: tanks.clone(), target: at + Vec2::from_int(30, 30) });
    step_until(&mut w, 30 * 120, "the bus in sight", |w| w.sees(P1, w.entity(bus).unwrap()));
    assert_eq!(order(&mut w, P1, CommandKind::Attack { units: tanks.clone(), target: bus }), vec![]);
    step_until(&mut w, 30 * 120, "bus destroyed", |w| w.entity(bus).is_none());
    let bunker = bunker_of(&w).expect("the bus left a bunker").id;
    for s in squads {
        let e = w.entity(s).expect("the squads survived");
        assert_eq!(e.inside, Some(bunker));
    }
    // The Rabble inside fire out, with the bunker's radius and range bonus on their reach.
    let fired = (0..30 * 20).any(|_| {
        w.step();
        w.events().iter().any(|e| matches!(e, Event::Hit { shooter, .. } if squads.contains(shooter)))
    });
    assert!(fired, "the squads inside fire");
}
/// The bunker is no base: once every building of the base and every constructor is gone, the
/// bunker and its squads don't keep the player in the game (fails if a bunker counts like a
/// building).
#[test]
fn a_bunker_does_not_keep_a_player_in_the_game() {
    let s = setup(Faction::Gla, JUNKYARD, Faction::Eu);
    let mut w = World::new(&s, rules());
    let (bus, _) = loaded_bus(&mut w, Vec2::from_int(200, 200));
    assert_eq!(order(&mut w, P0, CommandKind::Transform { units: vec![bus] }), vec![]);
    assert!(bunker_of(&w).is_some());
    assert!(!w.player(P0).defeated, "the base still stands");
    let base: Vec<EntityId> = w
        .entities()
        .filter(|e| e.owner == Some(P0) && (rules().def(e.def).is_base() || rules().def(e.def).is_constructor()))
        .map(|e| e.id)
        .collect();
    let home1 = w.map.spawns[1];
    let factory = build(&mut w, P1, EU_FACTORY, home1 + Vec2::from_int(-30, 0));
    let tanks: Vec<EntityId> = (0..6).map(|_| train(&mut w, P1, factory, EU_AEGIS)).collect();
    // They drive into the base, seeing it, then attack each building in turn.
    w.submit(P1, CommandKind::Move { units: tanks.clone(), target: w.map.spawns[0] + Vec2::from_int(20, 20) });
    step_until(&mut w, 30 * 120, "the base in sight", |w| {
        base.iter().all(|&b| w.entity(b).is_none_or(|e| w.sees(P1, e)))
    });
    for &b in &base {
        if w.entity(b).is_some() && !w.player(P0).defeated {
            assert_eq!(order(&mut w, P1, CommandKind::Attack { units: tanks.clone(), target: b }), vec![]);
            step_until(&mut w, 30 * 240, "a base building destroyed", |w| w.entity(b).is_none());
        }
    }
    step_until(&mut w, 30 * 5, "defeat", |w| w.player(P0).defeated);
    assert_eq!(w.winner(), Some(P1));
}

/// Blackjack's Shadow Tank fires its Particle Accelerator once at the chosen target for 300
/// (no armour bonus), which reveals it; the order is refused until it has recharged, and the
/// tank's cannon keeps firing meanwhile.
#[test]
fn the_particle_accelerator_fires_once_then_recharges() {
    let s = setup(Faction::Apa, BLACKJACK, Faction::Eu);
    let mut w = World::new(&s, rules());
    let home = w.map.spawns[0];
    let factory = build(&mut w, P0, APA_FACTORY, home + Vec2::from_int(30, 0));
    let tank = train(&mut w, P0, factory, APA_SHADOW_TANK);
    let home1 = w.map.spawns[1];
    let factory1 = build(&mut w, P1, EU_FACTORY, home1 + Vec2::from_int(-30, 0));
    let target = train(&mut w, P1, factory1, EU_AEGIS);
    let meet = Vec2::from_int(200, 200);
    walk(&mut w, P1, target, meet);
    // Within the tank's sight (its range + 15) and the accelerator's 50, beyond the cannon's 35.
    walk(&mut w, P0, tank, meet - Vec2::from_int(30, 28));
    w.submit(P0, CommandKind::Special { units: vec![tank], target });
    let mut fired = None;
    for _ in 0..30 * 10 {
        let before = w.entity(target).unwrap().hp;
        w.step();
        if w.events().iter().any(|e| matches!(e, Event::SpecialFired { shooter, .. } if *shooter == tank)) {
            // The cannon may land on the same tick: take its hits off.
            let cannon: i32 = w
                .events()
                .iter()
                .filter_map(|e| match *e {
                    Event::Hit { shooter, target: t, damage, .. } if shooter == tank && t == target => Some(damage),
                    _ => None,
                })
                .sum::<i32>()
                - 300;
            fired = Some(before - w.entity(target).unwrap().hp - cannon);
            break;
        }
    }
    assert_eq!(fired, Some(300), "one shot of 300, no bonus against the Aegis's armour");
    let e = w.entity(tank).unwrap();
    assert_eq!(e.order, Order::Idle, "the order ends with the shot");
    assert!(!w.hidden(e), "firing reveals the stealth tank");
    assert_eq!(order(&mut w, P0, CommandKind::Special { units: vec![tank], target }), vec![Reject::SpecialNotReady]);
    let ready = w.entity(tank).unwrap().special_at;
    step_until(&mut w, 30 * 30, "recharge", |w| w.tick() >= ready);
    if w.entity(target).is_some() {
        assert_eq!(order(&mut w, P0, CommandKind::Special { units: vec![tank], target }), vec![]);
    }
}

/// Both new commands go through a replay and verify to the same hash.
#[test]
fn ability_commands_replay_exactly() {
    let s = setup(Faction::Gla, JUNKYARD, Faction::Eu);
    let mut w = World::new(&s, rules());
    let (bus, _) = loaded_bus(&mut w, Vec2::from_int(200, 200));
    w.submit(P0, CommandKind::Transform { units: vec![bus] });
    for _ in 0..30 * 5 {
        w.step();
    }
    let replay = w.replay(&s);
    let bytes = replay.encode();
    let back = g2_sim::Replay::decode(&bytes).unwrap();
    assert_eq!(back, replay);
    assert_eq!(verify_replay(&back, crater_ridge(), rules()), Ok(w.state_hash()));
}
