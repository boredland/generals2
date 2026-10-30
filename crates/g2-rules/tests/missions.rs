//! Missions: every objective and trigger kind on small missions of their own on Crater Ridge,
//! and win and loss. The campaign, on its imported maps, is tested in g2-maps.

use g2_rules::ids::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::mission::{Action, Area, Force, Goal, Mission, MissionId, Objective, Side, Status, Trigger, What, When};
use g2_sim::world::Event;
use g2_sim::{AiLevel, CommandKind, Faction, MatchSetup, PlayerId, Rules, World};

const ME: PlayerId = PlayerId(0);
const S: u32 = g2_sim::TICKS_PER_SECOND;

/// The rules with `missions` in place of the campaign.
fn with(missions: Vec<Mission>) -> &'static Rules {
    let base = rules();
    Box::leak(Box::new(Rules {
        defs: base.defs,
        generals: base.generals,
        upgrades: base.upgrades,
        perks: base.perks,
        support: base.support,
        missions: Box::leak(missions.into_boxed_slice()),
    }))
}

const SIDES: &[Side] = &[
    Side { faction: Faction::Eu, credits: 0, ai: AiLevel::Easy, general: None },
    Side { faction: Faction::Gla, credits: 0, ai: AiLevel::Easy, general: None },
];

fn mission(forces: &'static [Force], objectives: &'static [Objective], triggers: &'static [Trigger]) -> Mission {
    Mission { key: "test", name: "Test", map: "Crater Ridge", briefing: "", sides: SIDES, forces, objectives, triggers }
}

/// A world of the only mission in `r`, every seat idle (no AI anywhere: the player is a
/// human, and the enemy's AI is never asked to play).
fn start(r: &'static Rules) -> (World, MatchSetup) {
    let m = &r.missions[0];
    let mut s = m.setup(r, crater_ridge(), 1, None);
    s.players[1].ai = None;
    (World::with_mission(&s, r, MissionId(0)), s)
}

fn run_until(w: &mut World, ticks: u32) {
    while w.tick() < ticks && w.winner().is_none() {
        w.step();
    }
}

fn status(w: &World) -> Vec<Status> {
    w.mission().unwrap().objectives.clone()
}

/// A mission's forces are its only entities besides the map's; no skirmish base appears.
#[test]
fn forces_replace_the_skirmish_start() {
    static FORCES: [Force; 3] =
        [Force::new(0, EU_AEGIS, 80, 80), Force::new(0, EU_RECON, 100, 100).times(5), Force::new(1, GLA_CC, 320, 320)];
    static OBJ: [Objective; 1] = [Objective { text: "", goal: Goal::Survive { ticks: 1_000 } }];
    let r = with(vec![mission(&FORCES, &OBJ, &[])]);
    let (w, _) = start(r);
    let owned: Vec<_> = w.entities().filter(|e| e.owner.is_some()).map(|e| (e.owner.unwrap(), e.def)).collect();
    assert_eq!(owned.len(), 7);
    assert_eq!(owned.iter().filter(|&&(o, d)| o == ME && d == EU_RECON).count(), 5);
    assert!(!owned.iter().any(|&(_, d)| d == EU_CC || d == GLA_WORKER), "no skirmish start");
}

/// Survive: done on its tick, and the mission is won the same tick, the enemy out.
#[test]
fn surviving_wins() {
    static FORCES: [Force; 2] = [Force::new(0, EU_AEGIS, 80, 80), Force::new(1, GLA_CC, 320, 320)];
    static OBJ: [Objective; 1] = [Objective { text: "", goal: Goal::Survive { ticks: 10 * S } }];
    let r = with(vec![mission(&FORCES, &OBJ, &[])]);
    let (mut w, _) = start(r);
    run_until(&mut w, 20 * S);
    assert_eq!(w.tick(), 10 * S + 1);
    assert_eq!(status(&w), [Status::Done]);
    assert_eq!(w.mission().unwrap().result, Some(true));
    assert_eq!(w.winner(), Some(ME));
    assert!(w.player(PlayerId(1)).defeated);
    assert!(!w.entities().any(|e| e.owner == Some(PlayerId(1))), "the losing side's assets go");
}

/// Destroy a tagged target and destroy all of a kind: an Aegis tank parked next to an idle
/// rabble squad and a worker kills both; the tagged one first is its own objective.
#[test]
fn destroy_objectives_fire_when_their_targets_are_gone() {
    static FORCES: [Force; 3] = [
        Force::new(0, EU_AEGIS, 200, 200).times(3),
        Force::new(1, GLA_RABBLE, 215, 200).tagged(7),
        Force::new(1, GLA_WORKER, 200, 215),
    ];
    static OBJ: [Objective; 2] = [
        Objective { text: "", goal: Goal::Destroy { tag: 7 } },
        Objective { text: "", goal: Goal::DestroyAll { owner: 1, what: What::Units } },
    ];
    let r = with(vec![mission(&FORCES, &OBJ, &[])]);
    let (mut w, _) = start(r);
    let mut first_done = None;
    while w.tick() < 120 * S && w.winner().is_none() {
        w.step();
        if first_done.is_none() && status(&w)[0] == Status::Done {
            first_done = Some(w.tick());
            assert!(w.entities().any(|e| e.owner == Some(PlayerId(1))), "the worker outlives the tagged squad");
        }
    }
    assert!(first_done.is_some());
    assert_eq!(status(&w), [Status::Done, Status::Done]);
    assert_eq!(w.winner(), Some(ME));
}

/// Protect: losing the protected unit fails the mission at once.
#[test]
fn losing_a_protected_unit_fails() {
    static FORCES: [Force; 3] = [
        Force::new(0, EU_DOZER, 200, 200).tagged(1),
        Force::new(0, EU_RECON, 60, 60),
        Force::new(1, GLA_TANK, 215, 200).times(2),
    ];
    static OBJ: [Objective; 2] = [
        Objective { text: "", goal: Goal::Survive { ticks: 600 * S } },
        Objective { text: "", goal: Goal::Protect { tag: 1 } },
    ];
    let r = with(vec![mission(&FORCES, &OBJ, &[])]);
    let (mut w, _) = start(r);
    run_until(&mut w, 120 * S);
    assert_eq!(status(&w), [Status::Active, Status::Failed]);
    assert_eq!(w.mission().unwrap().result, Some(false));
    assert_eq!(w.winner(), Some(PlayerId(1)));
    assert!(w.player(ME).defeated);
}

/// A protected unit that lives is done along with the rest.
#[test]
fn protect_is_done_with_the_rest() {
    static FORCES: [Force; 2] = [Force::new(0, EU_DOZER, 80, 80).tagged(1), Force::new(1, GLA_CC, 320, 320)];
    static OBJ: [Objective; 2] = [
        Objective { text: "", goal: Goal::Protect { tag: 1 } },
        Objective { text: "", goal: Goal::Survive { ticks: 3 * S } },
    ];
    let r = with(vec![mission(&FORCES, &OBJ, &[])]);
    let (mut w, _) = start(r);
    run_until(&mut w, 10 * S);
    assert_eq!(status(&w), [Status::Done, Status::Done]);
    assert_eq!(w.winner(), Some(ME));
}

/// Reach, and the area trigger: a unit ordered into the area completes the objective and
/// fires the trigger; any unit counts for an untagged area, only the tagged one for a tagged.
#[test]
fn reaching_an_area_completes_and_triggers() {
    static FORCES: [Force; 3] = [
        Force::new(0, EU_RECON, 100, 100).tagged(1),
        Force::new(0, EU_RECON, 100, 120),
        Force::new(1, GLA_CC, 320, 320),
    ];
    const SPOT: Area = Area { at: (160, 100), radius: 10 };
    static OBJ: [Objective; 1] = [Objective { text: "", goal: Goal::Reach { area: SPOT, tag: Some(1) } }];
    static TRIG: [Trigger; 1] =
        [Trigger { when: When::Entered { player: 0, area: SPOT }, then: &[Action::Say("in the area")] }];
    let r = with(vec![mission(&FORCES, &OBJ, &TRIG)]);
    let (mut w, _) = start(r);
    let untagged = w.entities().filter(|e| e.def == EU_RECON).nth(1).unwrap().id;
    let tagged = w.mission().unwrap().tagged(1).unwrap();
    w.submit(ME, CommandKind::Move { units: vec![untagged], target: g2_sim::Vec2::from_int(160, 100) });
    run_until(&mut w, 30 * S);
    let run = w.mission().unwrap();
    assert_eq!(run.said.iter().map(|&(_, s)| s).collect::<Vec<_>>(), ["in the area"]);
    assert_eq!(status(&w), [Status::Active], "only the tagged unit completes it");
    w.submit(ME, CommandKind::Move { units: vec![tagged], target: g2_sim::Vec2::from_int(160, 100) });
    run_until(&mut w, 60 * S);
    assert_eq!(w.winner(), Some(ME));
}

/// Timed, destroyed and objective triggers: each fires once, in order; spawned waves attack
/// where told, credits arrive, messages are said on their tick.
#[test]
fn triggers_fire_once_and_carry_out_their_actions() {
    // The tagged squad stands out of the tanks' sight until the wave's tick has passed.
    static FORCES: [Force; 3] = [
        Force::new(0, EU_AEGIS, 100, 100).times(2),
        Force::new(1, GLA_RABBLE, 160, 100).tagged(3),
        Force::new(1, GLA_CC, 330, 330),
    ];
    static OBJ: [Objective; 2] = [
        Objective { text: "", goal: Goal::Destroy { tag: 3 } },
        Objective { text: "", goal: Goal::Survive { ticks: 200 * S } },
    ];
    static TRIG: [Trigger; 3] = [
        Trigger {
            when: When::At(5 * S),
            then: &[
                Action::Say("wave"),
                Action::Spawn { forces: &[Force::new(1, GLA_TECHNICAL, 330, 300).times(2)], attack: Some((200, 200)) },
                Action::Credits { player: 0, amount: 1_234 },
            ],
        },
        Trigger { when: When::Destroyed(3), then: &[Action::Say("tag gone")] },
        Trigger { when: When::Done(0), then: &[Action::Say("objective done")] },
    ];
    let r = with(vec![mission(&FORCES, &OBJ, &TRIG)]);
    let (mut w, _) = start(r);
    let mut produced = Vec::new();
    while w.tick() < 6 * S {
        w.step();
        produced.extend(w.events().iter().filter_map(|e| match *e {
            Event::Produced { player, def, .. } => Some((w.tick() - 1, player, def)),
            _ => None,
        }));
    }
    assert_eq!(produced, [(5 * S, PlayerId(1), GLA_TECHNICAL), (5 * S, PlayerId(1), GLA_TECHNICAL)]);
    assert_eq!(w.player(ME).credits, 1_234);
    let wave: Vec<_> = w.entities().filter(|e| e.def == GLA_TECHNICAL).collect();
    assert!(wave.iter().all(|e| matches!(e.order, g2_sim::Order::AttackMove { .. })), "the wave attacks");
    assert!(w.mission().unwrap().tagged(3).is_some_and(|id| w.entity(id).is_some()));
    let tanks: Vec<_> = w.entities().filter(|e| e.def == EU_AEGIS).map(|e| e.id).collect();
    w.submit(ME, CommandKind::AttackMove { units: tanks, target: g2_sim::Vec2::from_int(150, 100) });
    run_until(&mut w, 120 * S);
    let said: Vec<_> = w.mission().unwrap().said.iter().map(|&(_, s)| s).collect();
    assert_eq!(said, ["wave", "tag gone", "objective done"]);
    let (t_gone, t_done) = (w.mission().unwrap().said[1].0, w.mission().unwrap().said[2].0);
    assert_eq!(t_gone, t_done, "an objective done and its trigger on the same tick");
    assert_eq!(w.player(ME).credits, 1_234, "a trigger fires once");
}

/// The player loses when nothing of theirs is left, or when they surrender.
#[test]
fn losing_everything_or_surrendering_loses() {
    static FORCES: [Force; 2] = [Force::new(0, EU_RECON, 200, 200), Force::new(1, GLA_TANK, 212, 200).times(3)];
    static OBJ: [Objective; 1] = [Objective { text: "", goal: Goal::Survive { ticks: 600 * S } }];
    let r = with(vec![mission(&FORCES, &OBJ, &[])]);
    let (mut w, _) = start(r);
    run_until(&mut w, 120 * S);
    assert_eq!(w.winner(), Some(PlayerId(1)));
    let (mut w, _) = start(r);
    w.submit(ME, CommandKind::Surrender);
    run_until(&mut w, 10);
    assert_eq!(w.mission().unwrap().result, Some(false));
}
