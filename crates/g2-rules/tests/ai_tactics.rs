//! The AI's small-scale decisions, each on a staged scene: a mission on Crater Ridge places
//! both sides' units exactly, and an AI plays seat 0 as it would a skirmish; seat 1 has no AI.

use g2_rules::ids::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::mission::{Action, Force, Goal, Mission, MissionId, Objective, Side, Trigger, When};
use g2_sim::world::Event;
use g2_sim::{AiLevel, EntityId, Faction, Fx, Order, PlayerId, Rules, Vec2, World};

const AI: PlayerId = PlayerId(0);
const ENEMY: PlayerId = PlayerId(1);
const HOME: (i64, i64) = (80, 80);
const S: u32 = g2_sim::TICKS_PER_SECOND;

/// A world of one mission on Crater Ridge: seat 0 (`ai` at `level`, its command center at
/// `HOME`) against seat 1 (`enemy`, idle), with `forces` and `triggers`. Nobody wins early.
fn scene(ai: Faction, level: AiLevel, enemy: Faction, forces: Vec<Force>, triggers: Vec<Trigger>) -> World {
    let base = rules();
    let sides = vec![
        Side { faction: ai, credits: 0, ai: level, general: None },
        Side { faction: enemy, credits: 0, ai: AiLevel::Easy, general: None },
    ];
    let cc = |f: Faction| rules().command_center(f);
    let mut all = vec![Force::new(0, cc(ai), HOME.0, HOME.1), Force::new(1, cc(enemy), 320, 320)];
    all.extend(forces);
    let objectives = vec![Objective { text: "", goal: Goal::Survive { ticks: 3_600 * S } }];
    let mission = Mission {
        key: "test",
        name: "Test",
        map: "Crater Ridge",
        briefing: "",
        sides: Box::leak(sides.into_boxed_slice()),
        forces: Box::leak(all.into_boxed_slice()),
        objectives: Box::leak(objectives.into_boxed_slice()),
        triggers: Box::leak(triggers.into_boxed_slice()),
    };
    let r: &'static Rules = Box::leak(Box::new(Rules {
        defs: base.defs,
        generals: base.generals,
        upgrades: base.upgrades,
        perks: base.perks,
        support: base.support,
        missions: Box::leak(vec![mission].into_boxed_slice()),
    }));
    let mut s = r.missions[0].setup(r, crater_ridge(), 1, Some(level));
    s.players[1].ai = None;
    World::with_mission(&s, r, MissionId(0))
}

fn first(w: &World, owner: PlayerId, def: g2_sim::DefId) -> EntityId {
    w.entities().find(|e| e.owner == Some(owner) && e.def == def).unwrap().id
}

/// `Easy` fires a ready strike at a group it sees: five GLA tanks appear beside an EU Easy
/// AI's command center as its Air Raid finishes charging, and the raid flies at them within
/// 5 s (the AI thinks once a second). Before, Easy never used a power.
#[test]
fn easy_fires_a_ready_strike_at_a_visible_group() {
    let ready = rules().support_powers(Faction::Eu).next().unwrap().first_ticks;
    static TANKS: [Force; 1] = [Force::new(1, GLA_TANK, 112, 80).times(5)];
    static ARRIVE: [Action; 1] = [Action::Spawn { forces: &TANKS, attack: None }];
    let arrive = Trigger { when: When::At(ready - S), then: &ARRIVE };
    let mut w = scene(Faction::Eu, AiLevel::Easy, Faction::Gla, vec![], vec![arrive]);
    let mut fired = None;
    while w.tick() < ready + 5 * S && fired.is_none() {
        w.step();
        fired = w.events().iter().find_map(|e| match e {
            Event::PowerUsed { player: AI, slot: 1, at } => Some(*at),
            _ => None,
        });
    }
    let at = fired.expect("Easy never used its ready Air Raid");
    let near =
        w.entities().any(|e| e.owner == Some(ENEMY) && e.def == GLA_TANK && e.pos.distance(at) <= Fx::from_int(30));
    assert!(near, "the raid at {at:?} is aimed away from the tanks");
}

/// `Normal`'s gun steps away from an enemy inside its minimum range rather than standing
/// there unable to fire: a GLA Toxin Artillery (minimum range 15) with an EU tank parked 10
/// from it, out in the field, is beyond its minimum range from the tank within 10 s. Before,
/// a gun stood and was shot (the tank's gun reaches 35).
#[test]
fn a_gun_moves_out_of_its_minimum_range() {
    let forces = vec![Force::new(0, GLA_TOXIN_ARTILLERY, 200, 160), Force::new(1, EU_AEGIS, 207, 167)];
    let mut w = scene(Faction::Gla, AiLevel::Normal, Faction::Eu, forces, vec![]);
    let (gun, tank) = (first(&w, AI, GLA_TOXIN_ARTILLERY), first(&w, ENEMY, EU_AEGIS));
    let min = rules().def(GLA_TOXIN_ARTILLERY).weapon().unwrap().shell.unwrap().min_range;
    let gap = |w: &World| {
        let (g, t) = (w.entity(gun).unwrap().pos, w.entity(tank).unwrap().pos);
        g.distance(t) - rules().def(EU_AEGIS).radius
    };
    assert!(gap(&w) < min, "staged inside the minimum range");
    let mut moved = false;
    while w.tick() < 10 * S && w.entity(gun).is_some() && gap(&w) <= min {
        w.step();
        moved |= w.entity(gun).is_some_and(|g| matches!(g.order, Order::Move { .. }));
    }
    assert!(w.entity(gun).is_some(), "the gun died first");
    assert!(moved, "the gun was never ordered away");
    assert!(gap(&w) > min, "still within the minimum range: {:?}", gap(&w));
}

/// `Normal` routes an endangered constructor home around the threat: a dozer in the field
/// with an armed enemy close by on the straight way home is sent along a way that keeps more
/// than 20 from it (straight home passes within 4).
#[test]
fn a_constructor_goes_home_around_the_threat() {
    let forces = vec![Force::new(0, EU_DOZER, 200, 200), Force::new(1, GLA_RABBLE, 183, 180)];
    let mut w = scene(Faction::Eu, AiLevel::Normal, Faction::Gla, forces, vec![]);
    let (dozer, rabble) = (first(&w, AI, EU_DOZER), first(&w, ENEMY, GLA_RABBLE));
    let threat = w.entity(rabble).unwrap().pos;
    let home = Vec2::from_int(HOME.0, HOME.1);
    let mut target = None;
    while w.tick() < 5 * S && target.is_none() {
        w.step();
        target = match w.entity(dozer).unwrap().order {
            Order::Move { target, .. } => Some(target),
            _ => None,
        };
    }
    let target = target.expect("the dozer was never sent away");
    let from = w.entity(dozer).unwrap().pos;
    // How close a straight walk from `from` to `to` passes `threat`, sampled every unit.
    let passes = |to: Vec2| {
        let steps = from.distance(to).floor_int().max(1);
        (0..=steps).map(|k| (from + (to - from).scale(Fx::from_ratio(k, steps))).distance(threat)).min().unwrap()
    };
    assert!(passes(home) < Fx::from_int(5), "staged with the threat on the straight way home");
    assert!(passes(target) > Fx::from_int(20), "sent to {target:?}, passing {:?} from the enemy", passes(target));
}

/// `Easy` pulls a constructor that is being hit back home; before, it stood and died. A GLA
/// worker in the field is shot by an EU tank and is sent home (to a free spot beside its
/// command center) within 3 s of the first hit.
#[test]
fn easy_pulls_a_constructor_under_fire_home() {
    let forces = vec![Force::new(0, GLA_WORKER, 200, 200), Force::new(1, EU_AEGIS, 215, 215)];
    let mut w = scene(Faction::Gla, AiLevel::Easy, Faction::Eu, forces, vec![]);
    let worker = first(&w, AI, GLA_WORKER);
    let (mut hit, mut sent) = (None, false);
    while w.tick() < 20 * S && !sent {
        w.step();
        if hit.is_none() && w.events().iter().any(|e| matches!(e, Event::Hit { target, .. } if *target == worker)) {
            hit = Some(w.tick());
        }
        let Some(e) = w.entity(worker) else { break };
        let home = Vec2::from_int(HOME.0, HOME.1);
        sent = matches!(e.order, Order::Move { target, .. } if target.distance(home) < Fx::from_int(20));
    }
    let hit = hit.expect("the tank never fired at the worker");
    assert!(sent, "the worker under fire was never sent home");
    assert!(w.tick() <= hit + 3 * S, "sent home {} ticks after the first hit", w.tick() - hit);
}
