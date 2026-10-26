//! Aircraft, driven through public commands only: they fly straight over terrain ground units
//! can't cross, only weapons that target the air can hit them, shells never do, and they
//! neither push nor are pushed by ground units.

use g2_rules::ids::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::command::Reject;
use g2_sim::world::{Event, INPUT_DELAY};
use g2_sim::{
    AiLevel, CommandKind, EntityId, Faction, Fx, MatchSetup, Order, PlayerId, PlayerSetup, Vec2, World, record,
    verify_replay,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn world(f0: Faction, f1: Faction) -> World {
    let setup = MatchSetup {
        map: crater_ridge(),
        seed: 1,
        players: vec![
            PlayerSetup { faction: f0, starting_credits: 100_000, ai: None, general: None, perks: Vec::new(), rank: 1 },
            PlayerSetup { faction: f1, starting_credits: 100_000, ai: None, general: None, perks: Vec::new(), rank: 1 },
        ],
    };
    World::new(&setup, rules())
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

fn first(w: &World, owner: PlayerId, def: g2_sim::DefId) -> EntityId {
    w.entities().find(|e| e.owner == Some(owner) && e.def == def).map(|e| e.id).unwrap()
}

fn build(w: &mut World, who: PlayerId, def: g2_sim::DefId, pos: Vec2) -> EntityId {
    let builder = w.entities().find(|e| e.owner == Some(who) && w.rules.def(e.def).is_constructor()).unwrap().id;
    w.submit(who, CommandKind::Build { builder, def, pos });
    step_until(w, INPUT_DELAY + 2, "foundation", |w| w.entities().any(|e| e.owner == Some(who) && e.def == def));
    let site = first(w, who, def);
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

/// A trained `def` of `who`'s, sent to `at` and stopped there.
fn unit_at(w: &mut World, who: PlayerId, producer: EntityId, def: g2_sim::DefId, at: Vec2) -> EntityId {
    let u = train(w, who, producer, def);
    w.submit(who, CommandKind::Move { units: vec![u], target: at });
    for _ in 0..=INPUT_DELAY {
        w.step();
    }
    step_until(w, 30 * 90, "arrival", |w| w.entity(u).is_none_or(|e| e.order == Order::Idle));
    u
}

fn rejected(w: &mut World, who: PlayerId, cmd: CommandKind) -> Option<Reject> {
    w.submit(who, cmd);
    let mut reason = None;
    for _ in 0..=INPUT_DELAY {
        w.step();
        for e in w.events() {
            if let Event::Rejected { player, reason: r } = e
                && *player == who
            {
                reason = Some(*r);
            }
        }
    }
    reason
}

/// Crater Ridge with a wall of impassable terrain across it from top to bottom at `x` 180..190,
/// so no ground unit gets from one start to the other.
fn walled() -> g2_sim::MapDef {
    let mut map = crater_ridge();
    let (w, h) = g2_sim::nav::Grid::dims(map.width, map.height);
    let cell = g2_sim::nav::CELL as usize;
    map.terrain = (0..w * h).map(|i| (180 / cell..190 / cell).contains(&(i % w))).collect();
    map
}

/// A helicopter flies across a wall ground units can't cross, onto the wall and over it,
/// straight there at its speed.
#[test]
fn aircraft_fly_straight_over_impassable_terrain() {
    let setup = MatchSetup {
        map: walled(),
        seed: 1,
        players: vec![
            PlayerSetup {
                faction: Faction::Eu,
                starting_credits: 100_000,
                ai: None,
                general: None,
                perks: Vec::new(),
                rank: 1,
            },
            PlayerSetup {
                faction: Faction::Gla,
                starting_credits: 100_000,
                ai: None,
                general: None,
                perks: Vec::new(),
                rank: 1,
            },
        ],
    };
    let mut w = World::new(&setup, rules());
    let airfield = build(&mut w, P0, EU_AIRFIELD, Vec2::from_int(110, 80));
    let heli = train(&mut w, P0, airfield, EU_VALKYRIE);
    let speed = rules().def(EU_VALKYRIE).speed();
    for target in [Vec2::from_int(185, 120), Vec2::from_int(260, 120)] {
        let start = w.entity(heli).unwrap().pos;
        let straight = (start.distance(target) / speed).floor_int() as u32;
        w.submit(P0, CommandKind::Move { units: vec![heli], target });
        for _ in 0..=INPUT_DELAY {
            w.step();
        }
        step_until(&mut w, straight + 10, "flight", |w| w.entity(heli).unwrap().order == Order::Idle);
        let at = w.entity(heli).unwrap().pos;
        assert!(at.distance(target) < Fx::from_int(1), "stopped at {at:?}, short of {target:?}");
    }
    assert!(!w.map.walkable(Vec2::from_int(185, 120)));
}

/// Ground-only weapons can't be ordered onto an aircraft, and an idle tank leaves one hovering
/// in its gun's range alone; anti-air units of the same faction shoot it down. A ground-only
/// helicopter can't attack another helicopter either.
#[test]
fn only_weapons_that_target_the_air_hit_aircraft() {
    let mut w = world(Faction::Apa, Faction::Eu);
    let factory = build(&mut w, P0, APA_FACTORY, Vec2::from_int(110, 80));
    let apa_air = build(&mut w, P0, APA_AIRFIELD, Vec2::from_int(140, 50));
    let eu_air = build(&mut w, P1, EU_AIRFIELD, crater_ridge().spawns[1] + Vec2::from_int(-30, 0));
    let field = Vec2::from_int(200, 200);
    let tank = unit_at(&mut w, P0, factory, APA_WARLORD, field);
    let heli = unit_at(&mut w, P1, eu_air, EU_VALKYRIE, field + Vec2::from_int(15, 0));
    let hp = w.entity(heli).unwrap().hp;
    assert_eq!(
        rejected(&mut w, P0, CommandKind::Attack { units: vec![tank], target: heli }),
        Some(Reject::CannotAttack)
    );
    for _ in 0..30 * 10 {
        w.step();
    }
    assert_eq!(w.entity(heli).unwrap().hp, hp, "a tank shot at a helicopter");
    let mantis = unit_at(&mut w, P0, apa_air, APA_MANTIS, field + Vec2::from_int(0, 20));
    assert_eq!(
        rejected(&mut w, P1, CommandKind::Attack { units: vec![heli], target: mantis }),
        Some(Reject::CannotAttack)
    );
    // Meanwhile the Valkyrie shoots up the tank. Two Gatlings end the fight.
    let gatlings = [-10, -20].map(|x| unit_at(&mut w, P0, factory, APA_GATLING, field + Vec2::from_int(x, 0)));
    w.submit(P0, CommandKind::Attack { units: gatlings.to_vec(), target: heli });
    step_until(&mut w, 30 * 60, "the Gatlings down the Valkyrie", |w| w.entity(heli).is_none());
}

/// An artillery shell bursting on a tank leaves the helicopter hovering right above it unhurt.
#[test]
fn shells_burst_on_the_ground_only() {
    let mut w = world(Faction::Eu, Faction::Gla);
    let factory = build(&mut w, P0, EU_FACTORY, Vec2::from_int(110, 80));
    let gla_factory = build(&mut w, P1, GLA_FACTORY, crater_ridge().spawns[1] + Vec2::from_int(-30, 0));
    let spot = Vec2::from_int(220, 220);
    let tank = unit_at(&mut w, P1, gla_factory, GLA_TANK, spot);
    let chopper = unit_at(&mut w, P1, gla_factory, GLA_TOXIN_CHOPPER, spot);
    let howitzer = unit_at(&mut w, P0, factory, EU_ARTILLERY, spot - Vec2::from_int(50, 0));
    let hp = w.entity(chopper).unwrap().hp;
    w.submit(P0, CommandKind::Attack { units: vec![howitzer], target: tank });
    let mut landed = 0;
    step_until(&mut w, 30 * 60, "three shells on the tank", |w| {
        landed += w.events().iter().filter(|e| matches!(e, Event::Hit { target, .. } if *target == tank)).count();
        landed >= 3 || w.entity(tank).is_none()
    });
    let over = w.entity(chopper).unwrap().pos.distance(w.entity(tank).map_or(spot, |t| t.pos));
    assert!(over < Fx::from_int(6), "the helicopter drifted {over:?} off the tank");
    assert_eq!(w.entity(chopper).unwrap().hp, hp, "a shell hit the helicopter");
}

/// A helicopter parked over a tank: neither is pushed aside, though two tanks would be.
#[test]
fn aircraft_and_ground_units_share_a_spot() {
    let mut w = world(Faction::Gla, Faction::Eu);
    let factory = build(&mut w, P0, GLA_FACTORY, Vec2::from_int(110, 80));
    let spot = Vec2::from_int(200, 200);
    let tank = unit_at(&mut w, P0, factory, GLA_TANK, spot);
    let parked = w.entity(tank).unwrap().pos;
    let chopper = unit_at(&mut w, P0, factory, GLA_TOXIN_CHOPPER, parked);
    let (t, c) = (w.entity(tank).unwrap().pos, w.entity(chopper).unwrap().pos);
    assert!(t.distance(c) < Fx::from_int(1), "the helicopter stopped short of the tank");
    for _ in 0..30 * 5 {
        w.step();
    }
    assert_eq!((w.entity(tank).unwrap().pos, w.entity(chopper).unwrap().pos), (t, c));
    let second = unit_at(&mut w, P0, factory, GLA_TANK, t);
    for _ in 0..30 * 5 {
        w.step();
    }
    let gap = w.entity(second).unwrap().pos.distance(w.entity(tank).unwrap().pos);
    assert!(gap > Fx::from_int(2), "two tanks overlap ({gap:?} apart)");
}

/// A match where aircraft are built and fire replays exactly.
#[test]
fn matches_with_aircraft_replay_exactly() {
    let s = MatchSetup {
        map: crater_ridge(),
        seed: 1,
        players: vec![
            PlayerSetup {
                faction: Faction::Gla,
                starting_credits: 5_000,
                ai: Some(AiLevel::Easy),
                general: None,
                perks: Vec::new(),
                rank: 1,
            },
            PlayerSetup {
                faction: Faction::Apa,
                starting_credits: 5_000,
                ai: Some(AiLevel::Normal),
                general: None,
                perks: Vec::new(),
                rank: 1,
            },
        ],
    };
    let mut w = World::new(&s, rules());
    let mut air_shots = 0;
    while w.winner().is_none() && w.tick() < 30 * 60 * 15 {
        w.step();
        air_shots += w
            .events()
            .iter()
            .filter(|e| matches!(e, Event::Hit { shooter, .. } if w.entity(*shooter).is_some_and(|x| w.rules.def(x.def).flies())))
            .count();
    }
    assert!(air_shots > 0, "no aircraft fired in a whole match");
    let (end, replay) = record(World::new(&s, rules()), &s, w.tick(), |_| {});
    assert_eq!(end.state_hash(), w.state_hash());
    assert_eq!(verify_replay(&replay, crater_ridge(), rules()), Ok(w.state_hash()));
}
