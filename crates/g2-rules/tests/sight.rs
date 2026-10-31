//! Terrain line of sight (`g2_sim::sight`): a direct-fire unit doesn't shoot through a ridge;
//! it closes in until it sees its target, and on flat ground fires from its full range.

use g2_rules::ids::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::map::Heights;
use g2_sim::world::{Event, INPUT_DELAY};
use g2_sim::{CommandKind, EntityId, Faction, Fx, MapDef, MatchSetup, Order, PlayerId, PlayerSetup, Vec2, World};

const P0: PlayerId = PlayerId(0);

/// Crater Ridge with a ring of ridge 5 high (30 SAGE units) 22 to 27 from the GLA command
/// center at (320, 320), so a tank outside the ring can't see the building's centre over it.
fn ringed() -> MapDef {
    let mut map = crater_ridge();
    let (step, n) = (Fx::from_int(2), 201u32);
    let cc = Vec2::from_int(320, 320);
    let z = (0..n * n)
        .map(|i| {
            let p = Vec2::new(step * Fx::from_int((i % n) as i64), step * Fx::from_int((i / n) as i64));
            let d = p.distance(cc);
            if d >= Fx::from_int(22) && d <= Fx::from_int(27) { Fx::from_int(5) } else { Fx::ZERO }
        })
        .collect();
    map.heights = Heights { step, w: n, h: n, z };
    map
}

fn setup(map: MapDef) -> MatchSetup {
    let player = |faction| PlayerSetup {
        faction,
        starting_credits: 100_000,
        ai: None,
        general: None,
        perks: Vec::new(),
        rank: 1,
        team: 0,
    };
    MatchSetup { map, seed: 1, players: vec![player(Faction::Eu), player(Faction::Gla)] }
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

fn first(w: &World, def: g2_sim::DefId) -> EntityId {
    w.entities().find(|e| e.owner == Some(P0) && e.def == def).map(|e| e.id).unwrap()
}

/// Trains a tank, walks it to (280, 280), out of range of the GLA command center but in sight
/// of it, orders the attack and returns the tank's distance from the center at its first hit.
fn first_shot_distance(map: MapDef) -> Fx {
    let mut w = World::new(&setup(map), rules());
    let builder = first(&w, EU_DOZER);
    w.submit(P0, CommandKind::Build { builder, def: EU_FACTORY, pos: Vec2::from_int(120, 140) });
    step_until(&mut w, INPUT_DELAY + 2, "foundation", |w| w.entities().any(|e| e.def == EU_FACTORY));
    let factory = first(&w, EU_FACTORY);
    step_until(&mut w, 30 * 120, "factory", |w| w.entity(factory).is_some_and(|s| s.is_complete()));
    w.submit(P0, CommandKind::Train { producer: factory, def: EU_AEGIS });
    step_until(&mut w, 30 * 120, "tank", |w| w.entities().any(|e| e.def == EU_AEGIS));
    let tank = first(&w, EU_AEGIS);
    w.submit(P0, CommandKind::Move { units: vec![tank], target: Vec2::from_int(280, 280) });
    for _ in 0..=INPUT_DELAY {
        w.step();
    }
    step_until(&mut w, 30 * 120, "arrival", |w| w.entity(tank).unwrap().order == Order::Idle);
    let cc = w.entities().find(|e| e.def == GLA_CC).unwrap().id;
    w.submit(P0, CommandKind::Attack { units: vec![tank], target: cc });
    let mut at = None;
    step_until(&mut w, 30 * 60, "the first shot", |w| {
        at = w
            .events()
            .iter()
            .find(|e| matches!(e, Event::Hit { shooter, target, .. } if *shooter == tank && *target == cc))
            .map(|_| w.entity(tank).unwrap().pos.distance(Vec2::from_int(320, 320)));
        at.is_some()
    });
    at.unwrap()
}

#[test]
fn a_tank_behind_a_ridge_closes_in_until_it_sees_its_target() {
    let flat = first_shot_distance(crater_ridge());
    let ringed = first_shot_distance(ringed());
    assert!(flat > Fx::from_int(40), "on flat ground it fires from its range: {flat:?}");
    assert!(ringed < Fx::from_int(30), "behind the ridge it fires only from on top of it: {ringed:?}");
}
