//! Garrisons, driven through public commands only: infantry enter civilian buildings and
//! transports, fire out of buildings with the range bonus, can't be shot inside, die with what
//! they are in, and leave it neutral again once out.

use g2_rules::ids::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::command::Reject;
use g2_sim::world::{Event, INPUT_DELAY};
use g2_sim::{
    CommandKind, EntityId, Faction, Fx, MapDef, MatchSetup, Order, PlayerId, PlayerSetup, Vec2, World, verify_replay,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

/// Crater Ridge with civilian buildings: a town house in the middle and a shanty, too small
/// for anything but itself.
fn town() -> MapDef {
    let mut map = crater_ridge();
    map.civilians = vec![(Vec2::from_int(200, 200), Fx::from_int(3)), (Vec2::from_int(150, 230), Fx::from_int(2))];
    map
}

fn setup(f0: Faction, f1: Faction) -> MatchSetup {
    MatchSetup {
        map: town(),
        seed: 1,
        players: vec![
            PlayerSetup { faction: f0, starting_credits: 100_000, ai: None, general: None, perks: Vec::new() },
            PlayerSetup { faction: f1, starting_credits: 100_000, ai: None, general: None, perks: Vec::new() },
        ],
    }
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

fn steps(w: &mut World, n: u32) {
    for _ in 0..n {
        w.step();
    }
}

fn first(w: &World, owner: Option<PlayerId>, def: g2_sim::DefId) -> EntityId {
    w.entities().find(|e| e.owner == owner && e.def == def).map(|e| e.id).unwrap()
}

fn build(w: &mut World, who: PlayerId, def: g2_sim::DefId, pos: Vec2) -> EntityId {
    let builder = w.entities().find(|e| e.owner == Some(who) && w.rules.def(e.def).is_constructor()).unwrap().id;
    w.submit(who, CommandKind::Build { builder, def, pos });
    step_until(w, INPUT_DELAY + 2, "foundation", |w| w.entities().any(|e| e.owner == Some(who) && e.def == def));
    let site = first(w, Some(who), def);
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

fn unit_at(w: &mut World, who: PlayerId, producer: EntityId, def: g2_sim::DefId, at: Vec2) -> EntityId {
    let u = train(w, who, producer, def);
    w.submit(who, CommandKind::Move { units: vec![u], target: at });
    steps(w, INPUT_DELAY + 1);
    step_until(w, 30 * 90, "arrival", |w| w.entity(u).is_none_or(|e| e.order == Order::Idle));
    u
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

fn garrison(w: &mut World, who: PlayerId, units: &[EntityId], container: EntityId) {
    assert_eq!(order(w, who, CommandKind::Garrison { units: units.to_vec(), container }), vec![]);
    step_until(w, 30 * 60, "getting in", |w| {
        units.iter().all(|&u| w.entity(u).is_none_or(|e| e.inside.is_some() || e.order == Order::Idle))
    });
}

/// A town house holds three squads, and only infantry: a fourth squad and a tank are refused.
/// It becomes its occupants' while they are inside and neutral again once they leave.
#[test]
fn infantry_take_a_civilian_building_up_to_its_capacity() {
    let mut w = World::new(&setup(Faction::Eu, Faction::Gla), rules());
    let house = first(&w, None, TOWN_HOUSE);
    assert_eq!(rules().def(TOWN_HOUSE).garrison.unwrap().capacity, 3);
    let barracks = build(&mut w, P0, EU_BARRACKS, Vec2::from_int(120, 80));
    let factory = build(&mut w, P0, EU_FACTORY, Vec2::from_int(80, 130));
    let near = Vec2::from_int(185, 185);
    let squads: Vec<EntityId> = (0..4).map(|_| unit_at(&mut w, P0, barracks, EU_RECON, near)).collect();
    let tank = unit_at(&mut w, P0, factory, EU_AEGIS, near);
    assert_eq!(
        order(&mut w, P0, CommandKind::Garrison { units: vec![tank], container: house }),
        vec![Reject::CannotGarrison]
    );
    garrison(&mut w, P0, &squads, house);
    let inside: Vec<_> = squads.iter().filter(|&&s| w.entity(s).unwrap().inside == Some(house)).collect();
    assert_eq!(inside.len(), 3, "three squads fit");
    assert_eq!(w.occupants(house).len(), 3);
    assert_eq!(w.entity(house).unwrap().owner, Some(P0));
    for &&s in &inside {
        assert_eq!(w.entity(s).unwrap().pos, w.entity(house).unwrap().pos);
    }
    assert_eq!(order(&mut w, P1, CommandKind::Evacuate { container: house }), vec![Reject::NotOwner]);
    assert_eq!(order(&mut w, P0, CommandKind::Evacuate { container: house }), vec![]);
    assert!(w.occupants(house).is_empty());
    assert_eq!(w.entity(house).unwrap().owner, None, "empty again, so neutral");
    let r = rules().def(TOWN_HOUSE).radius;
    for &s in &squads[..3] {
        let gap = w.entity(s).unwrap().pos.distance(w.entity(house).unwrap().pos);
        assert!(gap >= r, "squad stepped out at {gap:?} from the centre, inside the walls");
    }
}

/// Riflemen inside reach the building's edge plus their own range plus the alpha's 8-unit
/// bonus; outside they reach their range only. Inside they can't be targeted; the building
/// is shot instead.
#[test]
fn a_garrison_fires_further_and_cannot_be_targeted() {
    let mut w = World::new(&setup(Faction::Eu, Faction::Gla), rules());
    let house = first(&w, None, TOWN_HOUSE);
    let hpos = w.entity(house).unwrap().pos;
    let barracks = build(&mut w, P0, EU_BARRACKS, Vec2::from_int(120, 80));
    let squad = unit_at(&mut w, P0, barracks, EU_RECON, hpos - Vec2::from_int(15, 15));
    garrison(&mut w, P0, &[squad], house);
    assert_eq!(w.entity(squad).unwrap().inside, Some(house));
    // A GLA worker parked where only the bonus reaches it: further than the rifle's range from
    // the building's edge, within that range plus 8.
    let rifle = rules().def(EU_RECON).weapon().unwrap().range;
    let edge = rules().def(TOWN_HOUSE).radius;
    let worker = first(&w, Some(P1), GLA_WORKER);
    let at = hpos + Vec2::new(edge + rifle + Fx::from_int(5) + rules().def(GLA_WORKER).radius, Fx::ZERO);
    w.submit(P1, CommandKind::Move { units: vec![worker], target: at });
    steps(&mut w, INPUT_DELAY + 1);
    step_until(&mut w, 30 * 120, "worker in place", |w| w.entity(worker).unwrap().order == Order::Idle);
    step_until(&mut w, 30 * 30, "the garrison shoots the worker", |w| {
        w.events()
            .iter()
            .any(|e| matches!(e, Event::Hit { shooter, target, .. } if *shooter == squad && *target == worker))
    });
    // The enemy can't order an attack on the squad inside, only on the house.
    let gla_barracks = build(&mut w, P1, GLA_BARRACKS, crater_ridge().spawns[1] + Vec2::from_int(-30, 0));
    let rebels = unit_at(&mut w, P1, gla_barracks, GLA_RABBLE, hpos + Vec2::from_int(30, 0));
    assert_eq!(
        order(&mut w, P1, CommandKind::Attack { units: vec![rebels], target: squad }),
        vec![Reject::CannotAttack]
    );
    let hp = w.entity(squad).unwrap().hp;
    steps(&mut w, 30 * 10);
    assert_eq!(w.entity(squad).unwrap().hp, hp, "the squad inside was hit");
    assert!(w.entity(house).unwrap().hp < rules().def(TOWN_HOUSE).hp, "the rebels fired at the house");
}

/// Units inside die with the building, and the kill counts for its destroyer's side.
#[test]
fn a_garrison_dies_with_its_building() {
    let mut w = World::new(&setup(Faction::Gla, Faction::Apa), rules());
    let house = first(&w, None, TOWN_HOUSE);
    let hpos = w.entity(house).unwrap().pos;
    let barracks = build(&mut w, P0, GLA_BARRACKS, Vec2::from_int(120, 80));
    let squads: Vec<_> =
        (0..2).map(|_| unit_at(&mut w, P0, barracks, GLA_RABBLE, hpos - Vec2::from_int(15, 15))).collect();
    garrison(&mut w, P0, &squads, house);
    let factory = build(&mut w, P1, APA_FACTORY, crater_ridge().spawns[1] + Vec2::from_int(-30, 0));
    let tanks: Vec<_> =
        (0..3).map(|k| unit_at(&mut w, P1, factory, APA_WARLORD, hpos + Vec2::from_int(40, 10 * k - 10))).collect();
    w.submit(P1, CommandKind::Attack { units: tanks, target: house });
    step_until(&mut w, 30 * 120, "the house falls", |w| w.entity(house).is_none());
    for s in squads {
        assert!(w.entity(s).is_none(), "a squad survived its building");
    }
}

/// The Thunderfist howitzer's +100 applies only to a building with a garrison: its shell does
/// 100 more to an occupied house than to a barracks, both `Structure`. (An empty house is
/// neutral and can't be attacked at all.)
#[test]
fn the_howitzer_bonus_needs_a_garrison() {
    let mut w = World::new(&setup(Faction::Gla, Faction::Apa), rules());
    let house = first(&w, None, TOWN_HOUSE);
    let hpos = w.entity(house).unwrap().pos;
    let barracks = build(&mut w, P0, GLA_BARRACKS, hpos + Vec2::from_int(0, 30));
    let s = unit_at(&mut w, P0, barracks, GLA_RABBLE, hpos - Vec2::from_int(15, 15));
    garrison(&mut w, P0, &[s], house);
    let factory = build(&mut w, P1, APA_FACTORY, crater_ridge().spawns[1] + Vec2::from_int(-30, 0));
    let gun = unit_at(&mut w, P1, factory, APA_THUNDERFIST, hpos + Vec2::from_int(55, 15));
    let shell_on = |w: &mut World, target: EntityId| {
        w.submit(P1, CommandKind::Attack { units: vec![gun], target });
        let mut dealt = None;
        step_until(w, 30 * 60, "a shell on the target", |w| {
            dealt = w.events().iter().find_map(|e| match e {
                Event::Hit { shooter, target: t, damage, .. } if *shooter == gun && *t == target => Some(*damage),
                _ => None,
            });
            dealt.is_some()
        });
        dealt.unwrap()
    };
    let on_barracks = shell_on(&mut w, barracks);
    let on_house = shell_on(&mut w, house);
    assert_eq!(on_barracks, rules().def(APA_THUNDERFIST).deploy.unwrap().weapon.damage_vs(g2_sim::Armour::Structure));
    assert_eq!(on_house, on_barracks + 100);
}

/// An APC carries two infantry squads who can't fire from inside; they ride along and get
/// out where it stops.
#[test]
fn an_apc_carries_infantry() {
    let mut w = World::new(&setup(Faction::Eu, Faction::Gla), rules());
    let barracks = build(&mut w, P0, EU_BARRACKS, Vec2::from_int(120, 80));
    let factory = build(&mut w, P0, EU_FACTORY, Vec2::from_int(80, 130));
    let apc = unit_at(&mut w, P0, factory, EU_APC, Vec2::from_int(150, 120));
    let squads: Vec<_> = (0..2).map(|_| unit_at(&mut w, P0, barracks, EU_ROCKETS, Vec2::from_int(140, 110))).collect();
    garrison(&mut w, P0, &squads, apc);
    assert!(squads.iter().all(|&s| w.entity(s).unwrap().inside == Some(apc)));
    let dest = Vec2::from_int(250, 150);
    w.submit(P0, CommandKind::Move { units: vec![apc], target: dest });
    steps(&mut w, INPUT_DELAY + 1);
    step_until(&mut w, 30 * 60, "the APC drives", |w| w.entity(apc).unwrap().order == Order::Idle);
    for &s in &squads {
        assert_eq!(w.entity(s).unwrap().pos, w.entity(apc).unwrap().pos, "a passenger was left behind");
    }
    assert_eq!(order(&mut w, P0, CommandKind::Evacuate { container: apc }), vec![]);
    for &s in &squads {
        let e = w.entity(s).unwrap();
        assert!(e.inside.is_none() && e.pos.distance(dest) < Fx::from_int(15));
    }
}

/// A player defeated while holding a civilian building leaves it standing and neutral.
#[test]
fn a_defeated_players_building_turns_neutral() {
    let mut w = World::new(&setup(Faction::Gla, Faction::Eu), rules());
    let house = first(&w, None, TOWN_HOUSE);
    let hpos = w.entity(house).unwrap().pos;
    let barracks = build(&mut w, P0, GLA_BARRACKS, Vec2::from_int(120, 80));
    let s = unit_at(&mut w, P0, barracks, GLA_RABBLE, hpos - Vec2::from_int(15, 15));
    garrison(&mut w, P0, &[s], house);
    assert_eq!(order(&mut w, P0, CommandKind::Surrender), vec![]);
    steps(&mut w, 2);
    let h = w.entity(house).expect("the house was removed with its holder");
    assert_eq!(h.owner, None);
    assert!(w.occupants(house).is_empty());
    assert_eq!(w.winner(), Some(P1), "holding a civilian building doesn't keep a player in");
}

/// A match with garrisons replays exactly.
#[test]
fn matches_with_garrisons_replay_exactly() {
    let s = setup(Faction::Eu, Faction::Gla);
    let mut live = World::new(&s, rules());
    let house = first(&live, None, TOWN_HOUSE);
    let barracks = build(&mut live, P0, EU_BARRACKS, Vec2::from_int(120, 80));
    let squad = unit_at(&mut live, P0, barracks, EU_RECON, Vec2::from_int(185, 185));
    garrison(&mut live, P0, &[squad], house);
    steps(&mut live, 30 * 5);
    assert_eq!(order(&mut live, P0, CommandKind::Evacuate { container: house }), vec![]);
    steps(&mut live, 30 * 5);
    let replay = live.replay(&s);
    assert!(replay.commands.iter().any(|c| matches!(c.kind, CommandKind::Garrison { .. })));
    let decoded = g2_sim::Replay::decode(&replay.encode()).unwrap();
    assert_eq!(verify_replay(&decoded, town(), rules()), Ok(live.state_hash()));
}
