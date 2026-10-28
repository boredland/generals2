//! Stealth and detection, driven through public commands only: a stealth unit can't be seen,
//! targeted or attacked unless a detector of the enemy's has it in range or it just fired;
//! base defences detect and fire on their own while powered.

use g2_rules::ids::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::command::Reject;
use g2_sim::world::{Event, INPUT_DELAY, STEALTH_REVEAL_TICKS};
use g2_sim::{CommandKind, EntityId, Faction, MatchSetup, Order, PlayerId, PlayerSetup, Vec2, World, verify_replay};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn setup(f0: Faction, f1: Faction) -> MatchSetup {
    MatchSetup {
        map: crater_ridge(),
        seed: 1,
        players: vec![
            PlayerSetup { faction: f0, starting_credits: 100_000, ai: None, general: None, perks: Vec::new(), rank: 1 },
            PlayerSetup { faction: f1, starting_credits: 100_000, ai: None, general: None, perks: Vec::new(), rank: 1 },
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

fn first(w: &World, owner: PlayerId, def: g2_sim::DefId) -> EntityId {
    w.entities().find(|e| e.owner == Some(owner) && e.def == def).map(|e| e.id).unwrap()
}

fn build(w: &mut World, who: PlayerId, def: g2_sim::DefId, pos: Vec2) -> EntityId {
    let builder = w.entities().find(|e| e.owner == Some(who) && w.rules.def(e.def).is_constructor()).unwrap().id;
    w.submit(who, CommandKind::Build { builder, def, pos });
    step_until(w, INPUT_DELAY + 2, "foundation", |w| w.entities().any(|e| e.owner == Some(who) && e.def == def));
    let site = w.entities().filter(|e| e.owner == Some(who) && e.def == def).map(|e| e.id).max().unwrap();
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

/// An RPG squad standing in plain view of an enemy tank is invisible to it: the tank doesn't
/// engage, can't be ordered to attack it, and the squad takes no damage. Once the squad fires
/// it is revealed for the alpha's 2.5 s and the tank shoots back.
#[test]
fn a_stealth_unit_is_unseen_until_it_fires() {
    let mut w = World::new(&setup(Faction::Gla, Faction::Apa), rules());
    let barracks = build(&mut w, P0, GLA_BARRACKS, Vec2::from_int(120, 80));
    let factory = build(&mut w, P1, APA_FACTORY, crater_ridge().spawns[1] + Vec2::from_int(-30, 0));
    let field = Vec2::from_int(200, 200);
    // The squad waits out of its own reach of the tank (its rocket reaches 26), in the tank's
    // sight, so it doesn't open fire as a guard.
    let rpg = unit_at(&mut w, P0, barracks, GLA_RPG, field);
    let tank = unit_at(&mut w, P1, factory, APA_WARLORD, field + Vec2::from_int(40, 0));
    assert!(w.vision(P1).is_visible(w.entity(rpg).unwrap().pos));
    assert!(!w.sees(P1, w.entity(rpg).unwrap()), "stealthed in plain view");
    assert_eq!(order(&mut w, P1, CommandKind::Attack { units: vec![tank], target: rpg }), vec![Reject::CannotAttack]);
    let hp = w.entity(rpg).unwrap().hp;
    steps(&mut w, 30 * 5);
    assert_eq!(w.entity(rpg).unwrap().hp, hp, "the hidden squad was shot");
    assert_eq!(w.entity(tank).unwrap().order, Order::Idle, "the tank engaged an unseen squad");
    // The squad attacks once a worker has the tank in sight: firing reveals it, and the tank's
    // guard picks it up.
    let worker = first(&w, P0, GLA_WORKER);
    w.submit(P0, CommandKind::Move { units: vec![worker], target: field + Vec2::from_int(10, 10) });
    let tpos = w.entity(tank).unwrap().pos;
    step_until(&mut w, 30 * 120, "tank in sight", |w| w.vision(P0).is_visible(tpos));
    assert_eq!(order(&mut w, P0, CommandKind::Attack { units: vec![rpg], target: tank }), vec![]);
    step_until(&mut w, 30 * 20, "the squad fires", |w| {
        w.events().iter().any(|e| matches!(e, Event::Hit { shooter, .. } if *shooter == rpg))
    });
    assert!(w.sees(P1, w.entity(rpg).unwrap()), "firing didn't reveal the squad");
    step_until(&mut w, 30 * 10, "the tank fires back", |w| {
        w.events().iter().any(|e| matches!(e, Event::Hit { shooter, target, .. } if *shooter == tank && *target == rpg))
    });
}

/// A stealth unit is revealed from its shot for `STEALTH_REVEAL_TICKS`, then hidden again.
#[test]
fn a_stealth_unit_hides_again_after_its_reveal() {
    let mut w = World::new(&setup(Faction::Gla, Faction::Eu), rules());
    let barracks = build(&mut w, P0, GLA_BARRACKS, Vec2::from_int(120, 80));
    let rpg = unit_at(&mut w, P0, barracks, GLA_RPG, Vec2::from_int(200, 200));
    let dozer = first(&w, P1, EU_DOZER);
    w.submit(P1, CommandKind::Move { units: vec![dozer], target: Vec2::from_int(222, 200) });
    steps(&mut w, INPUT_DELAY + 1);
    step_until(&mut w, 30 * 120, "dozer in place", |w| w.entity(dozer).unwrap().order == Order::Idle);
    step_until(&mut w, 30 * 20, "the guard fires", |w| w.entity(rpg).unwrap().revealed_until.is_some());
    // One shot, then stand down: the dozer drives off out of reach, and no more shots follow.
    w.submit(P1, CommandKind::Move { units: vec![dozer], target: crater_ridge().spawns[1] });
    w.submit(P0, CommandKind::Stop { units: vec![rpg] });
    let until = w.entity(rpg).unwrap().revealed_until.unwrap();
    step_until(&mut w, 30 * 10, "out of reach", |w| {
        w.entity(rpg).unwrap().revealed_until == Some(until) && w.tick() >= until - 1
    });
    assert!(!w.hidden(w.entity(rpg).unwrap()), "revealed until the end of its window");
    w.step();
    assert!(w.hidden(w.entity(rpg).unwrap()), "still revealed after {STEALTH_REVEAL_TICKS} ticks");
}

/// A base defence detects stealth units in its range: a hidden RPG squad walking up to an EU
/// gun turret is seen and shot well before it fires, while one beyond the detection range
/// stays hidden.
#[test]
fn a_turret_detects_and_shoots_stealth_units() {
    let mut w = World::new(&setup(Faction::Eu, Faction::Gla), rules());
    build(&mut w, P0, EU_POWER, Vec2::from_int(110, 120));
    let turret = build(&mut w, P0, EU_GUN_TURRET, Vec2::from_int(140, 140));
    assert!(w.entity(turret).unwrap().powered, "the turret has power");
    let at = w.entity(turret).unwrap().pos;
    let detect = rules().def(EU_GUN_TURRET).detection;
    let barracks = build(&mut w, P1, GLA_BARRACKS, crater_ridge().spawns[1] + Vec2::from_int(-30, 0));
    let far =
        unit_at(&mut w, P1, barracks, GLA_RPG, at + Vec2::new(detect + g2_sim::Fx::from_int(20), g2_sim::Fx::ZERO));
    assert!(!w.sees(P0, w.entity(far).unwrap()), "beyond detection range, still hidden");
    let near = unit_at(&mut w, P1, barracks, GLA_RPG, at + Vec2::from_int(40, 10));
    assert!(w.vision(P0).detected.contains(&near));
    assert!(w.sees(P0, w.entity(near).unwrap()));
    step_until(&mut w, 30 * 10, "the turret fires at the detected squad", |w| {
        w.events()
            .iter()
            .any(|e| matches!(e, Event::Hit { shooter, target, .. } if *shooter == turret && *target == near))
    });
}

/// A turret without power holds fire, and fires once a power plant reaches it. It stands
/// more than the power radius (60) from the command center, so it starts unpowered.
#[test]
fn a_turret_needs_power() {
    let mut w = World::new(&setup(Faction::Eu, Faction::Gla), rules());
    let spot = crater_ridge().spawns[0] + Vec2::from_int(75, 50);
    let turret = build(&mut w, P0, EU_GUN_TURRET, spot);
    assert!(!w.entity(turret).unwrap().powered, "the command center's power reaches the turret");
    let worker = first(&w, P1, GLA_WORKER);
    w.submit(P1, CommandKind::Move { units: vec![worker], target: spot + Vec2::from_int(30, 0) });
    steps(&mut w, INPUT_DELAY + 1);
    step_until(&mut w, 30 * 120, "worker in place", |w| w.entity(worker).unwrap().order == Order::Idle);
    let hp = w.entity(worker).unwrap().hp;
    steps(&mut w, 30 * 5);
    assert_eq!(w.entity(worker).unwrap().hp, hp, "an unpowered turret fired");
    build(&mut w, P0, EU_POWER, spot - Vec2::from_int(20, 0));
    step_until(&mut w, 30 * 10, "the powered turret fires", |w| {
        w.events().iter().any(|e| matches!(e, Event::Hit { shooter, .. } if *shooter == turret))
    });
}

/// A match with stealth units firing and turrets replays exactly.
#[test]
fn matches_with_stealth_replay_exactly() {
    let s = setup(Faction::Gla, Faction::Eu);
    let mut live = World::new(&s, rules());
    let barracks = build(&mut live, P0, GLA_BARRACKS, Vec2::from_int(120, 80));
    let rpg = unit_at(&mut live, P0, barracks, GLA_RPG, Vec2::from_int(200, 200));
    let dozer = first(&live, P1, EU_DOZER);
    live.submit(P0, CommandKind::Attack { units: vec![rpg], target: dozer });
    steps(&mut live, 30 * 30);
    let replay = live.replay(&s);
    let decoded = g2_sim::Replay::decode(&replay.encode()).unwrap();
    assert_eq!(verify_replay(&decoded, crater_ridge(), rules()), Ok(live.state_hash()));
}
