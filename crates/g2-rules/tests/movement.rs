//! Units on the move around each other, each on a staged scene: a mission on Crater Ridge
//! places the units exactly, seat 0 (no AI) is ordered by the test, seat 1 stands idle far
//! away. Measured with `g2-tools movement scenes`.

use g2_rules::ids::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::mission::{Difficulty, Force, Goal, Mission, MissionId, Objective, Side};
use g2_sim::{CommandKind, DefId, EntityId, Faction, Fx, Order, PlayerId, Rules, Vec2, World};

const P0: PlayerId = PlayerId(0);
const S: u32 = g2_sim::TICKS_PER_SECOND;

/// A world of one mission on Crater Ridge with `forces` for seat 0 (EU) besides seat 1's
/// command center in the far corner. Nobody wins early.
fn scene(forces: Vec<Force>) -> World {
    let base = rules();
    let side = |faction| Side { faction, credits: 0, ai: None, general: None, builds: None };
    let mut all = vec![Force::new(1, GLA_CC, 370, 370)];
    all.extend(forces);
    let objectives = vec![Objective { text: "", goal: Goal::Survive { ticks: 3_600 * S } }];
    let mission = Mission {
        key: "test",
        name: "Test",
        map: "Crater Ridge",
        briefing: "",
        sides: Box::leak(vec![side(Faction::Eu), side(Faction::Gla)].into_boxed_slice()),
        forces: Box::leak(all.into_boxed_slice()),
        objectives: Box::leak(objectives.into_boxed_slice()),
        triggers: &[],
    };
    let r: &'static Rules =
        Box::leak(Box::new(Rules { missions: Box::leak(vec![mission].into_boxed_slice()), ..*base }));
    let s = r.missions[0].setup(r, crater_ridge(), 1, None);
    World::with_mission(&s, r, MissionId(0), Difficulty::Normal)
}

/// `n` units of `def` for seat 0 in a square block centred on `(x, y)`, two units apart at
/// their edges.
fn block(def: DefId, (x, y): (i64, i64), n: i64) -> Vec<Force> {
    let gap = rules().def(def).radius.floor_int() * 2 + 3;
    let side = (1..).find(|s| s * s >= n).unwrap();
    (0..n)
        .map(|k| Force::new(0, def, x + (k / side - (side - 1) / 2) * gap, y + (k % side - (side - 1) / 2) * gap))
        .collect()
}

/// Seat 0's units of `def` within 30 of `(x, y)`, in entity order.
fn units_near(w: &World, def: DefId, (x, y): (i64, i64)) -> Vec<EntityId> {
    let at = Vec2::from_int(x, y);
    w.entities()
        .filter(|e| e.owner == Some(P0) && e.def == def && e.pos.distance(at) < Fx::from_int(30))
        .map(|e| e.id)
        .collect()
}

fn order(w: &mut World, kind: CommandKind) {
    w.submit(P0, kind);
}

/// The deepest any two of `units` stand inside each other, as a share of their radii's sum.
fn deepest(w: &World, units: &[EntityId]) -> Fx {
    let mut worst = Fx::ZERO;
    for (i, &a) in units.iter().enumerate() {
        for &b in &units[i + 1..] {
            let (ea, eb) = (w.entity(a).unwrap(), w.entity(b).unwrap());
            let sum = rules().def(ea.def).radius + rules().def(eb.def).radius;
            worst = worst.max((sum - ea.pos.distance(eb.pos)) / sum);
        }
    }
    worst
}

/// Two groups of nine tanks ordered through each other head-on both arrive within 22 s (160
/// apart at 9 per second is 18 s straight), and no two tanks ever stand more than a sixteenth
/// of their radii's sum inside each other. Measured: both clear in 19 s, never deeper than
/// 0.05; with pushing alone, 25 s, as deep as 0.13 and deeper than a sixteenth on 139 ticks.
#[test]
fn groups_crossing_head_on_pass_without_stacking() {
    let mut forces = block(EU_AEGIS, (110, 200), 9);
    forces.extend(block(EU_AEGIS, (270, 200), 9));
    let mut w = scene(forces);
    let (west, east) = (units_near(&w, EU_AEGIS, (110, 200)), units_near(&w, EU_AEGIS, (270, 200)));
    assert_eq!((west.len(), east.len()), (9, 9));
    order(&mut w, CommandKind::Move { units: west.clone(), target: Vec2::from_int(270, 200) });
    order(&mut w, CommandKind::Move { units: east.clone(), target: Vec2::from_int(110, 200) });
    let all: Vec<EntityId> = west.iter().chain(&east).copied().collect();
    for _ in 0..g2_sim::world::INPUT_DELAY + 1 {
        w.step();
    }
    let slots: Vec<Vec2> = all
        .iter()
        .map(|&u| match w.entity(u).unwrap().order {
            Order::Move { target, .. } => target,
            o => panic!("{u:?} was not sent: {o:?}"),
        })
        .collect();
    let mut worst = Fx::ZERO;
    for _ in 0..22 * S {
        w.step();
        worst = worst.max(deepest(&w, &all));
        if all.iter().all(|&u| w.entity(u).unwrap().order == Order::Idle) {
            break;
        }
    }
    assert!(worst < Fx::from_ratio(1, 16), "two tanks stood {worst:?} of their radii inside each other");
    for (&u, &slot) in all.iter().zip(&slots) {
        let e = w.entity(u).unwrap();
        assert_eq!(e.order, Order::Idle, "{u:?} still walking at {:?} after 22 s", e.pos);
        // A move ends within three radii of a slot another tank already stands on.
        assert!(e.pos.distance(slot) < Fx::from_int(11), "{u:?} stopped at {:?}, short of {slot:?}", e.pos);
    }
}

/// Sixteen infantry ordered through a 10-unit gap between two war factories all get through
/// and arrive within 35 s (170 at 9 per second is 19 s straight). Measured: 22 s. Steering
/// that may stop or slow down for a unit standing ahead held some up in front of it for good.
#[test]
fn a_column_passes_a_gap_between_buildings() {
    let mut forces = vec![Force::new(0, EU_FACTORY, 190, 183), Force::new(0, EU_FACTORY, 190, 217)];
    forces.extend(block(EU_RECON, (110, 200), 16));
    let mut w = scene(forces);
    let group = units_near(&w, EU_RECON, (110, 200));
    assert_eq!(group.len(), 16);
    let target = Vec2::from_int(280, 200);
    order(&mut w, CommandKind::Move { units: group.clone(), target });
    for _ in 0..35 * S {
        w.step();
        if w.tick() > 5 && group.iter().all(|&u| w.entity(u).unwrap().order == Order::Idle) {
            break;
        }
    }
    for &u in &group {
        let e = w.entity(u).unwrap();
        assert_eq!(e.order, Order::Idle, "{u:?} still walking at {:?} after 35 s", e.pos);
        assert!(e.pos.x > Fx::from_int(250), "{u:?} never got through the gap: {:?}", e.pos);
    }
}

/// Collectors pass through each other (a rule, so two meeting head-on in a gap never block
/// each other): two harvesters sent along the same line from opposite ends meet and walk
/// straight through, neither turning aside nor pushed off it.
#[test]
fn collectors_still_pass_through_each_other() {
    let mut w = scene(vec![Force::new(0, EU_HARVESTER, 150, 200), Force::new(0, EU_HARVESTER, 250, 200)]);
    let (a, b) = (units_near(&w, EU_HARVESTER, (150, 200))[0], units_near(&w, EU_HARVESTER, (250, 200))[0]);
    order(&mut w, CommandKind::Move { units: vec![a], target: Vec2::from_int(250, 200) });
    order(&mut w, CommandKind::Move { units: vec![b], target: Vec2::from_int(150, 200) });
    let mut met = false;
    for _ in 0..20 * S {
        w.step();
        let (pa, pb) = (w.entity(a).unwrap().pos, w.entity(b).unwrap().pos);
        met |= pa.distance(pb) < Fx::ONE;
        for p in [pa, pb] {
            assert_eq!(p.y, Fx::from_int(200), "a collector left its line at {p:?}");
        }
    }
    assert!(met, "the two collectors never met");
    // Both arrive (a move ends within one unit of its goal).
    assert!(w.entity(a).unwrap().pos.distance(Vec2::from_int(250, 200)) <= Fx::ONE);
    assert!(w.entity(b).unwrap().pos.distance(Vec2::from_int(150, 200)) <= Fx::ONE);
}
