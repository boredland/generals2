//! Destructible bridges, through public commands only: fire brings a deck down, what stands on
//! it dies, routes go round by another bridge, and a constructor repairs it.

use g2_rules::ids::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::map::BridgeDef;
use g2_sim::world::{Event, INPUT_DELAY};
use g2_sim::{CommandKind, EntityId, Faction, Fx, MapDef, MatchSetup, Order, PlayerId, PlayerSetup, Vec2, World};

const P0: PlayerId = PlayerId(0);

/// Crater Ridge cut in two by a river at x 190..210, crossed by two bridges: one at y 200 and
/// one far to the south at y 360.
fn river() -> MapDef {
    let mut map = crater_ridge();
    let (w, h) = g2_sim::nav::Grid::dims(map.width, map.height);
    let centre = |i: usize| g2_sim::nav::Grid::centre((i % w) as i32, (i / w) as i32);
    let water: Vec<bool> = (0..w * h).map(|i| (190..210).contains(&centre(i).x.floor_int())).collect();
    let deck = |y: i64| {
        move |i: usize| {
            let c = centre(i);
            (178..=222).contains(&c.x.floor_int()) && (c.y - Fx::from_int(y)).abs() <= Fx::from_int(6)
        }
    };
    let bridge = |y: i64| {
        let cells: Vec<u32> = (0..w * h).filter(|&i| deck(y)(i)).map(|i| i as u32).collect();
        let gap = cells.iter().copied().filter(|&i| water[i as usize]).collect();
        BridgeDef {
            a: Vec2::from_int(180, y),
            b: Vec2::from_int(220, y),
            width: Fx::from_int(12),
            hp: Some(2000),
            cells,
            gap,
        }
    };
    map.bridges = vec![bridge(200), bridge(360)];
    map.decks = (0..w * h).map(|i| deck(200)(i) || deck(360)(i)).collect();
    map.terrain = (0..w * h).map(|i| water[i] && !map.decks[i]).collect();
    map
}

fn setup() -> MatchSetup {
    let player = |faction| PlayerSetup {
        faction,
        starting_credits: 100_000,
        ai: None,
        general: None,
        perks: Vec::new(),
        rank: 1,
        team: 0,
    };
    MatchSetup { map: river(), seed: 1, players: vec![player(Faction::Eu), player(Faction::Gla)] }
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

fn build(w: &mut World, def: g2_sim::DefId, pos: Vec2) -> EntityId {
    let builder = first(w, EU_DOZER);
    w.submit(P0, CommandKind::Build { builder, def, pos });
    step_until(w, INPUT_DELAY + 2, "foundation", |w| w.entities().any(|e| e.owner == Some(P0) && e.def == def));
    let site = first(w, def);
    step_until(w, 30 * 120, "construction", |w| w.entity(site).is_some_and(|s| s.is_complete()));
    site
}

fn unit_at(w: &mut World, producer: EntityId, def: g2_sim::DefId, at: Vec2) -> EntityId {
    let before: Vec<_> = w.entities().filter(|e| e.def == def).map(|e| e.id).collect();
    w.submit(P0, CommandKind::Train { producer, def });
    let new = |w: &World| w.entities().find(|e| e.def == def && !before.contains(&e.id)).map(|e| e.id);
    step_until(w, 30 * 120, "training", |w| new(w).is_some());
    let u = new(w).unwrap();
    walk(w, u, at);
    u
}

/// Walks `u` to `to` and returns the furthest south (largest y) it went.
fn walk(w: &mut World, u: EntityId, to: Vec2) -> Fx {
    w.submit(P0, CommandKind::Move { units: vec![u], target: to });
    for _ in 0..=INPUT_DELAY {
        w.step();
    }
    let mut south = Fx::ZERO;
    step_until(w, 30 * 240, "arrival", |w| {
        let e = w.entity(u).unwrap();
        south = south.max(e.pos.y);
        e.order == Order::Idle
    });
    assert!(w.entity(u).unwrap().pos.distance(to) < Fx::from_int(6), "{u:?} reached {to:?}");
    south
}

#[test]
fn a_bridge_falls_to_fire_routes_go_round_and_a_constructor_repairs_it() {
    let mut w = World::new(&setup(), rules());
    let factory = build(&mut w, EU_FACTORY, Vec2::from_int(120, 140));
    let barracks = build(&mut w, EU_BARRACKS, Vec2::from_int(100, 110));
    let tank = unit_at(&mut w, factory, EU_AEGIS, Vec2::from_int(150, 200));
    // Over the near bridge and back while it stands: the short way.
    assert!(walk(&mut w, tank, Vec2::from_int(250, 200)) < Fx::from_int(230));
    assert!(walk(&mut w, tank, Vec2::from_int(150, 200)) < Fx::from_int(230));
    let squad = unit_at(&mut w, barracks, EU_RECON, Vec2::from_int(200, 200));
    assert_eq!(w.bridge_at(w.entity(squad).unwrap().pos), Some(0), "the squad stands on the deck");

    w.submit(P0, CommandKind::AttackBridge { units: vec![tank], bridge: 0 });
    let mut fell = false;
    step_until(&mut w, 30 * 600, "the bridge falling", |w| {
        fell |= w.events().iter().any(|e| matches!(e, Event::BridgeDown { bridge: 0, .. }));
        fell
    });
    assert!(w.bridge(0).unwrap().down && w.bridge(0).unwrap().hp == 0);
    assert!(w.entity(squad).is_none(), "the squad on the deck fell with it");
    assert_eq!(w.bridge(1).unwrap().hp, 2000, "the other bridge is untouched");
    w.step();
    assert_eq!(w.entity(tank).unwrap().order, Order::Idle, "nothing left to fire at");
    // Across now means round by the far bridge.
    assert!(walk(&mut w, tank, Vec2::from_int(250, 200)) > Fx::from_int(340), "went round by the south bridge");

    // A dozer repairs it from the near end; the deck opens once it is done.
    let dozer = first(&w, EU_DOZER);
    w.submit(P0, CommandKind::RepairBridge { units: vec![dozer], bridge: 0 });
    let mut up = false;
    step_until(&mut w, 30 * 240, "the repair", |w| {
        up |= w.events().iter().any(|e| matches!(e, Event::BridgeUp { bridge: 0 }));
        up
    });
    assert_eq!(w.bridge(0).unwrap().hp, 2000);
    assert!(walk(&mut w, tank, Vec2::from_int(150, 200)) < Fx::from_int(230), "the short way again");
}

/// Nothing is hashed for bridges nobody touched, and a match with a fallen bridge replays.
#[test]
fn bridges_replay_and_hash_only_once_hurt() {
    let s = setup();
    let mut plain = s.clone();
    plain.map.bridges.clear();
    let (a, b) = (World::new(&s, rules()), World::new(&plain, rules()));
    assert_eq!(a.state_hash(), b.state_hash());
    let mut w = World::new(&s, rules());
    let factory = build(&mut w, EU_FACTORY, Vec2::from_int(120, 140));
    let tank = unit_at(&mut w, factory, EU_AEGIS, Vec2::from_int(150, 200));
    w.submit(P0, CommandKind::AttackBridge { units: vec![tank], bridge: 0 });
    step_until(&mut w, 30 * 600, "the bridge falling", |w| w.bridge(0).unwrap().down);
    let replay = w.replay(&s);
    assert_eq!(g2_sim::verify_replay(&replay, s.map.clone(), rules()), Ok(w.state_hash()));
}
