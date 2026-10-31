//! Missions: every objective and trigger kind on small missions of their own on Crater Ridge,
//! and win and loss. The campaign, on its imported maps, is tested in g2-maps.

use g2_rules::ids::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::mission::{
    Action, Area, Difficulty, Force, Goal, Mission, MissionId, Objective, Script, Side, Status, Trigger, What, When,
};
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
    Side { faction: Faction::Eu, credits: 0, ai: None, general: None, builds: None },
    Side { faction: Faction::Gla, credits: 0, ai: None, general: None, builds: None },
];

fn mission(forces: &'static [Force], objectives: &'static [Objective], triggers: &'static [Trigger]) -> Mission {
    Mission { key: "test", name: "Test", map: "Crater Ridge", briefing: "", sides: SIDES, forces, objectives, triggers }
}

/// A world of the only mission in `r`, every seat idle (no AI anywhere: the player is a
/// human, and the enemy has none until a trigger wakes it).
fn start(r: &'static Rules) -> (World, MatchSetup) {
    start_at(r, Difficulty::Normal)
}

fn start_at(r: &'static Rules, d: Difficulty) -> (World, MatchSetup) {
    let m = &r.missions[0];
    let s = m.setup(r, crater_ridge(), 1, None);
    (World::with_mission(&s, r, MissionId(0), d), s)
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
                Action::Spawn { forces: &[Force::new(1, GLA_TECHNICAL, 330, 300).times(2)], route: &[(200, 200)] },
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

/// The enemy's units tagged `tag`'s squad, in spawn order.
fn squad(w: &World, tag: u8) -> Vec<g2_sim::EntityId> {
    w.mission().unwrap().squads.iter().find(|s| s.tag == Some(tag)).unwrap().units.clone()
}

/// A patrol loops its points: a technical pair passes near every point, and after the last
/// comes back to the first, over and over.
#[test]
fn a_patrol_loops_its_points() {
    const LOOP: &[(i64, i64)] = &[(200, 200), (300, 200), (300, 300)];
    static FORCES: [Force; 3] = [
        Force::new(0, EU_DOZER, 60, 60),
        Force::new(1, GLA_TECHNICAL, 200, 200).times(2).tagged(5).patrol(LOOP),
        Force::new(1, GLA_CC, 340, 60),
    ];
    static OBJ: [Objective; 1] = [Objective { text: "", goal: Goal::Survive { ticks: 1_000 * S } }];
    let r = with(vec![mission(&FORCES, &OBJ, &[])]);
    let (mut w, _) = start(r);
    let lead = squad(&w, 5)[0];
    // Which point the lead is at, each time it comes within 8 of one.
    let mut visits: Vec<usize> = Vec::new();
    while w.tick() < 300 * S {
        w.step();
        let at = w.entity(lead).unwrap().pos;
        let near = LOOP.iter().position(|&(x, y)| at.distance(g2_sim::Vec2::from_int(x, y)) <= g2_sim::Fx::from_int(8));
        if let Some(i) = near
            && visits.last() != Some(&i)
        {
            visits.push(i);
        }
    }
    assert!(visits.len() >= 7, "two laps and more in 5 minutes: {visits:?}");
    for pair in visits.windows(2) {
        assert_eq!(pair[1], (pair[0] + 1) % LOOP.len(), "in order, and round again: {visits:?}");
    }
}

/// A patrol fights what it meets on a leg (an attack-move): a recon squad standing on its
/// loop is destroyed.
#[test]
fn a_patrol_engages_on_its_way() {
    const LOOP: &[(i64, i64)] = &[(150, 200), (300, 200)];
    static FORCES: [Force; 3] = [
        Force::new(0, EU_RECON, 230, 200).tagged(1),
        Force::new(1, GLA_TECHNICAL, 150, 200).times(2).tagged(5).patrol(LOOP),
        Force::new(1, GLA_CC, 340, 60),
    ];
    static OBJ: [Objective; 2] = [
        Objective { text: "", goal: Goal::Protect { tag: 1 } },
        Objective { text: "", goal: Goal::Survive { ticks: 1_000 * S } },
    ];
    let r = with(vec![mission(&FORCES, &OBJ, &[])]);
    let (mut w, _) = start(r);
    run_until(&mut w, 120 * S);
    assert_eq!(w.mission().unwrap().result, Some(false), "the patrol killed the squad in its way");
}

/// A guard holds its area: guards standing outside it walk in; guards that engage a squad
/// walking past break off once it is beyond the leash, and stand in the area again.
#[test]
fn a_guard_returns_to_its_area() {
    const POST: Area = Area { at: (200, 200), radius: 10 };
    static FORCES: [Force; 4] = [
        Force::new(0, EU_RECON, 225, 200).tagged(1),
        Force::new(1, GLA_TECHNICAL, 200, 200).times(2).tagged(5).guard(POST),
        Force::new(1, GLA_TECHNICAL, 120, 320).tagged(6).guard(POST),
        Force::new(1, GLA_CC, 340, 60),
    ];
    static OBJ: [Objective; 1] = [Objective { text: "", goal: Goal::Survive { ticks: 1_000 * S } }];
    let r = with(vec![mission(&FORCES, &OBJ, &[])]);
    let (mut w, _) = start(r);
    let decoy = w.mission().unwrap().tagged(1).unwrap();
    let guards: Vec<_> = squad(&w, 5).into_iter().chain(squad(&w, 6)).collect();
    w.submit(ME, CommandKind::Move { units: vec![decoy], target: g2_sim::Vec2::from_int(390, 200) });
    let mut engaged = false;
    while w.tick() < 120 * S {
        w.step();
        engaged |= guards.iter().any(
            |&g| matches!(w.entity(g).unwrap().order, g2_sim::Order::AttackMove { engaged: Some(t), .. } if t == decoy),
        );
    }
    assert!(engaged, "the guards fought the passing squad");
    assert!(w.entity(decoy).is_some(), "and let it go once out of reach");
    for &g in &guards {
        let e = w.entity(g).unwrap();
        assert!(POST.contains(e.pos), "every guard in the area: {:?}", e.pos);
        assert_eq!(e.order, g2_sim::Order::Idle);
    }
}

/// A wave with a route walks it point by point, and its squad is done once the last is
/// reached (left to the seat's AI, if it has one).
#[test]
fn a_wave_walks_its_route() {
    const ROUTE: &[(i64, i64)] = &[(300, 100), (300, 300), (100, 300)];
    static FORCES: [Force; 2] = [Force::new(0, EU_DOZER, 60, 60), Force::new(1, GLA_CC, 340, 60)];
    static OBJ: [Objective; 1] = [Objective { text: "", goal: Goal::Survive { ticks: 1_000 * S } }];
    static TRIG: [Trigger; 1] = [Trigger {
        when: When::At(S),
        then: &[Action::Spawn { forces: &[Force::new(1, GLA_TECHNICAL, 200, 100).times(3)], route: ROUTE }],
    }];
    let r = with(vec![mission(&FORCES, &OBJ, &TRIG)]);
    let (mut w, _) = start(r);
    run_until(&mut w, 2 * S);
    let wave: Vec<_> = w.entities().filter(|e| e.def == GLA_TECHNICAL).map(|e| e.id).collect();
    let mut reached = 0;
    while w.tick() < 400 * S && reached < ROUTE.len() {
        w.step();
        let (x, y) = ROUTE[reached];
        if w.entity(wave[0]).unwrap().pos.distance(g2_sim::Vec2::from_int(x, y)) <= g2_sim::Fx::from_int(10) {
            reached += 1;
        }
    }
    assert_eq!(reached, ROUTE.len(), "every point in order");
    let end = w.tick() + 10 * S;
    run_until(&mut w, end);
    let s = &w.mission().unwrap().squads[0];
    assert_eq!(s.script, Script::Hold, "the route is done");
    assert!(!w.scripted(wave[0]));
}

/// Timed reinforcements: every period (scaled by the difficulty), as many times as set, its
/// gate standing.
#[test]
fn reinforcements_come_on_time() {
    static FORCES: [Force; 3] = [
        Force::new(0, EU_DOZER, 60, 60),
        Force::new(1, GLA_BARRACKS, 340, 340).tagged(9),
        Force::new(1, GLA_CC, 340, 60),
    ];
    static OBJ: [Objective; 1] = [Objective { text: "", goal: Goal::Survive { ticks: 1_000 * S } }];
    static TRIG: [Trigger; 1] = [Trigger {
        when: When::Every { from: 10 * S, every: 20 * S, times: 4, while_alive: Some(9) },
        then: &[Action::Spawn { forces: &[Force::new(1, GLA_RABBLE, 300, 340)], route: &[] }],
    }];
    let r = with(vec![mission(&FORCES, &OBJ, &TRIG)]);
    let ticks = |d: Difficulty| {
        let (mut w, _) = start_at(r, d);
        let mut at = Vec::new();
        while w.tick() < 200 * S {
            w.step();
            at.extend(w.events().iter().filter(|e| matches!(e, Event::Produced { .. })).map(|_| w.tick() - 1));
        }
        at
    };
    assert_eq!(ticks(Difficulty::Normal), [10 * S, 30 * S, 50 * S, 70 * S]);
    assert_eq!(ticks(Difficulty::Hard), [10 * S, 25 * S, 40 * S, 55 * S], "three quarters of the wait");
    assert_eq!(ticks(Difficulty::Easy), [10 * S, 38 * S, 66 * S, 94 * S], "seven fifths of it");
}

/// Reinforcements stop when their gate falls.
#[test]
fn reinforcements_stop_with_their_gate() {
    static FORCES: [Force; 3] = [
        Force::new(0, EU_AEGIS, 60, 60).times(3),
        Force::new(1, GLA_ROCKET_SITE, 100, 60).tagged(9),
        Force::new(1, GLA_CC, 340, 340),
    ];
    static OBJ: [Objective; 1] = [Objective { text: "", goal: Goal::Survive { ticks: 1_000 * S } }];
    static TRIG: [Trigger; 1] = [Trigger {
        when: When::Every { from: 60 * S, every: 30 * S, times: 5, while_alive: Some(9) },
        then: &[Action::Spawn { forces: &[Force::new(1, GLA_RABBLE, 300, 300)], route: &[] }],
    }];
    let r = with(vec![mission(&FORCES, &OBJ, &TRIG)]);
    let (mut w, _) = start(r);
    let site = w.mission().unwrap().tagged(9).unwrap();
    let tanks: Vec<_> = w.entities().filter(|e| e.def == EU_AEGIS).map(|e| e.id).collect();
    w.submit(ME, CommandKind::Attack { units: tanks, target: site });
    run_until(&mut w, 200 * S);
    assert!(w.entity(site).is_none(), "the tanks razed the site before the first reinforcement");
    assert_eq!(w.mission().unwrap().fired[0], 0, "and none came after");
}

/// The difficulty scales, from one place, the enemy's starting credits and credits a trigger
/// gives it, the size of the forces its triggers spawn, and nothing of the player's.
#[test]
fn difficulty_scales_the_enemy() {
    static SIDES2: [Side; 2] = [
        Side { faction: Faction::Eu, credits: 5_000, ai: None, general: None, builds: None },
        Side { faction: Faction::Gla, credits: 2_000, ai: None, general: None, builds: None },
    ];
    static FORCES: [Force; 2] = [Force::new(0, EU_DOZER, 60, 60), Force::new(1, GLA_CC, 340, 340)];
    static OBJ: [Objective; 1] = [Objective { text: "", goal: Goal::Survive { ticks: 1_000 * S } }];
    static TRIG: [Trigger; 1] = [Trigger {
        when: When::At(S),
        then: &[
            Action::Spawn {
                forces: &[Force::new(1, GLA_RABBLE, 300, 300).times(5), Force::new(0, EU_RECON, 100, 100).times(5)],
                route: &[],
            },
            Action::Credits { player: 1, amount: 1_000 },
            Action::Credits { player: 0, amount: 1_000 },
        ],
    }];
    let r = with(vec![Mission { sides: &SIDES2, ..mission(&FORCES, &OBJ, &TRIG) }]);
    let at = |d: Difficulty| {
        let (mut w, _) = start_at(r, d);
        run_until(&mut w, 2 * S);
        let count = |def| w.entities().filter(|e| e.def == def).count();
        (w.player(ME).credits, w.player(PlayerId(1)).credits, count(EU_RECON), count(GLA_RABBLE))
    };
    assert_eq!(at(Difficulty::Normal), (6_000, 3_000, 5, 5));
    assert_eq!(at(Difficulty::Easy), (6_000, 1_800, 5, 3));
    assert_eq!(at(Difficulty::Hard), (6_000, 4_200, 5, 7));
}

/// A side's build list limits what its seat may build and train, the AI's orders too.
#[test]
fn a_build_list_limits_the_seat() {
    static SIDES2: [Side; 2] = [
        Side { faction: Faction::Eu, credits: 0, ai: None, general: None, builds: None },
        Side { faction: Faction::Gla, credits: 5_000, ai: None, general: None, builds: Some(&[GLA_RABBLE]) },
    ];
    static FORCES: [Force; 3] =
        [Force::new(0, EU_DOZER, 60, 60), Force::new(1, GLA_BARRACKS, 340, 300), Force::new(1, GLA_WORKER, 300, 340)];
    static OBJ: [Objective; 1] = [Objective { text: "", goal: Goal::Survive { ticks: 1_000 * S } }];
    let r = with(vec![Mission { sides: &SIDES2, ..mission(&FORCES, &OBJ, &[]) }]);
    let (w, _) = start(r);
    assert_eq!(w.offers_of(PlayerId(1), GLA_BARRACKS), [GLA_RABBLE]);
    assert!(w.offers_of(PlayerId(1), GLA_WORKER).is_empty(), "builds nothing");
    assert_eq!(w.offers_of(ME, EU_DOZER), r.def(EU_DOZER).builds, "the player's are their faction's");
}

/// An idle seat plays from the moment a trigger wakes it: before, its credits stay unspent;
/// after, its AI builds with them. Its scripted squads keep their script.
#[test]
fn a_trigger_hands_a_seat_to_the_ai() {
    static SIDES2: [Side; 2] = [
        Side { faction: Faction::Eu, credits: 0, ai: None, general: None, builds: None },
        Side { faction: Faction::Gla, credits: 3_000, ai: None, general: None, builds: None },
    ];
    const LOOP: &[(i64, i64)] = &[(200, 300), (300, 200)];
    static FORCES: [Force; 4] = [
        Force::new(0, EU_DOZER, 60, 60),
        Force::new(1, GLA_CC, 320, 320),
        Force::new(1, GLA_WORKER, 300, 340),
        Force::new(1, GLA_RABBLE, 200, 300).times(2).tagged(5).patrol(LOOP),
    ];
    static OBJ: [Objective; 1] = [Objective { text: "", goal: Goal::Survive { ticks: 1_000 * S } }];
    static TRIG: [Trigger; 1] =
        [Trigger { when: When::At(60 * S), then: &[Action::Wake { player: 1, level: AiLevel::Easy }] }];
    let r = with(vec![Mission { sides: &SIDES2, ..mission(&FORCES, &OBJ, &TRIG) }]);
    let (mut w, _) = start(r);
    run_until(&mut w, 59 * S);
    assert!(!w.is_ai(PlayerId(1)));
    assert_eq!(w.player(PlayerId(1)).credits, 3_000, "idle: nothing bought");
    run_until(&mut w, 120 * S);
    assert!(w.is_ai(PlayerId(1)));
    assert!(w.player(PlayerId(1)).credits < 3_000, "the AI spends");
    assert!(w.scripted(squad(&w, 5)[0]), "the patrol keeps patrolling");
}

/// `validate` checks the scripts: points on open ground clear of the mission's buildings, a
/// squad an order names, an enemy's armed units only, and a woken seat that was idle.
#[test]
fn validation_covers_scripts() {
    let check = |forces: &'static [Force], triggers: &'static [Trigger]| {
        static OBJ: [Objective; 1] = [Objective { text: "", goal: Goal::Survive { ticks: S } }];
        let m = mission(forces, &OBJ, triggers);
        g2_sim::mission::validate(rules(), &m, &crater_ridge())
    };
    static OK: [Force; 2] = [
        Force::new(0, EU_DOZER, 60, 60),
        Force::new(1, GLA_RABBLE, 200, 200).tagged(5).patrol(&[(200, 200), (250, 250)]),
    ];
    assert_eq!(check(&OK, &[]), Ok(()));
    static OFF_MAP: [Force; 1] = [Force::new(1, GLA_RABBLE, 200, 200).patrol(&[(200, 200), (900, 250)])];
    assert!(check(&OFF_MAP, &[]).is_err(), "a patrol point off the map");
    static IN_BUILDING: [Force; 2] = [
        Force::new(1, GLA_CC, 300, 300),
        Force::new(1, GLA_RABBLE, 200, 200).guard(Area { at: (300, 300), radius: 20 }),
    ];
    assert!(check(&IN_BUILDING, &[]).is_err(), "a guard post inside a building");
    static ON_PILE: [Force; 1] = [Force::new(1, GLA_RABBLE, 200, 200).along(&[(130, 60)])];
    assert!(check(&ON_PILE, &[]).is_err(), "a route point on a supply pile");
    static PLAYERS: [Force; 1] = [Force::new(0, EU_RECON, 200, 200).patrol(&[(200, 200), (250, 250)])];
    assert!(check(&PLAYERS, &[]).is_err(), "the player's units take no script");
    static UNARMED: [Force; 1] = [Force::new(1, GLA_WORKER, 200, 200).patrol(&[(200, 200), (250, 250)])];
    assert!(check(&UNARMED, &[]).is_err(), "nor do unarmed ones");
    static ORDER: [Trigger; 1] = [Trigger {
        when: When::At(S),
        then: &[Action::Order { tag: 6, script: Script::Guard(Area { at: (200, 200), radius: 10 }) }],
    }];
    assert!(check(&OK, &ORDER).is_err(), "an order for a squad that doesn't exist");
    static WAKE: [Trigger; 1] =
        [Trigger { when: When::At(S), then: &[Action::Wake { player: 0, level: AiLevel::Easy }] }];
    assert!(check(&OK, &WAKE).is_err(), "the player's seat is not the mission's to wake");
    static GATE: [Trigger; 1] = [Trigger {
        when: When::Every { from: 0, every: S, times: 2, while_alive: Some(42) },
        then: &[Action::Say("")],
    }];
    assert!(check(&OK, &GATE).is_err(), "a gate tag that doesn't exist");
}
