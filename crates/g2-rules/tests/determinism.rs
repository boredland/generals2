//! Replays must reproduce bit for bit: same setup + same commands → same hash every tick.

use g2_rules::ids::*;
use g2_rules::{crater_ridge, map_by_name, rules};
use g2_sim::replay::Replay;
use g2_sim::{CommandKind, Faction, MatchSetup, PlayerId, PlayerSetup, Vec2, World, record, verify_replay};

fn setup(seed: u64, ai: [bool; 2], factions: [Faction; 2]) -> MatchSetup {
    MatchSetup {
        map: crater_ridge(),
        seed,
        players: factions
            .iter()
            .zip(ai)
            .map(|(&faction, ai)| PlayerSetup {
                faction,
                starting_credits: 5_000,
                ai: ai.then_some(g2_sim::AiLevel::Easy),
                general: None,
                perks: Vec::new(),
            })
            .collect(),
    }
}

/// A human playing player 0 with a scripted opening, against the built-in AI.
fn scripted_match(seed: u64, ticks: u32) -> (World, Replay) {
    let s = setup(seed, [false, true], [Faction::Eu, Faction::Gla]);
    let world = World::new(&s, rules());
    record(world, &s, ticks, |w| {
        let me = PlayerId(0);
        let mine = |def| w.entities().find(|e| e.owner == Some(me) && e.def == def).map(|e| e.id);
        match w.tick() {
            10 => {
                let builder = mine(EU_DOZER).unwrap();
                w.submit(me, CommandKind::Build { builder, def: EU_SUPPLY, pos: Vec2::from_int(110, 80) });
            }
            1_000 => {
                let producer = mine(EU_SUPPLY).unwrap();
                w.submit(me, CommandKind::Train { producer, def: EU_HARVESTER });
                w.submit(me, CommandKind::Train { producer, def: EU_HARVESTER });
            }
            2_500 => {
                let builder = mine(EU_DOZER).unwrap();
                w.submit(me, CommandKind::Build { builder, def: EU_DERRICK, pos: Vec2::from_int(60, 150) });
            }
            _ => {}
        }
    })
}

#[test]
fn identical_runs_produce_identical_hashes() {
    let (a, ra) = scripted_match(7, 4_000);
    let (b, rb) = scripted_match(7, 4_000);
    assert_eq!(ra.hashes, rb.hashes);
    assert_eq!(a.state_hash(), b.state_hash());
}

#[test]
fn recorded_replay_reproduces_the_live_match() {
    let (live, replay) = scripted_match(11, 4_000);
    assert!(replay.commands.len() >= 4, "human commands recorded, AI commands not");
    assert!(replay.commands.iter().all(|c| c.player == PlayerId(0)));
    let decoded = Replay::decode(&replay.encode()).expect("round trip");
    assert_eq!(decoded, replay);
    let end = verify_replay(&decoded, map_by_name(&decoded.map).unwrap(), rules()).expect("no divergence");
    assert_eq!(end, live.state_hash());
}

#[test]
fn a_changed_command_is_caught_at_the_first_checkpoint_after_it() {
    let (_, mut replay) = scripted_match(11, 4_000);
    let cmd = replay.commands.iter_mut().find(|c| matches!(c.kind, CommandKind::Build { .. })).unwrap();
    let CommandKind::Build { pos, .. } = &mut cmd.kind else { unreachable!() };
    *pos = Vec2::from_int(150, 100);
    let tick = cmd.tick;
    let err = verify_replay(&replay, crater_ridge(), rules()).unwrap_err();
    assert!(err.tick > tick && err.tick <= tick + 30, "diverged at {} for a change at {tick}", err.tick);
}

#[test]
fn a_corrupted_checkpoint_is_reported_at_exactly_that_tick() {
    let (_, mut replay) = scripted_match(11, 4_000);
    let (tick, hash) = &mut replay.hashes[50];
    *hash ^= 1;
    let tick = *tick;
    assert_eq!(verify_replay(&replay, crater_ridge(), rules()).unwrap_err().tick, tick);
}

/// The client submits between fixed steps, sometimes after a command's due tick has passed (for
/// example right after a fast-forward burst). Such commands run on the next step, and the log
/// must record that tick so the replay applies them at the same point.
#[test]
fn late_commands_replay_at_the_tick_they_actually_ran() {
    let s = setup(21, [false, true], [Faction::Apa, Faction::Eu]);
    let mut world = World::new(&s, rules());
    let me = PlayerId(0);
    for _ in 0..50 {
        world.step();
    }
    let crane = world.entities().find(|e| e.owner == Some(me) && e.def == APA_CRANE).unwrap().id;
    world.schedule(g2_sim::Command {
        tick: 3,
        player: me,
        kind: CommandKind::Move { units: vec![crane], target: Vec2::from_int(200, 200) },
    });
    for _ in 0..400 {
        world.step();
    }
    let replay = world.replay(&s);
    assert_eq!(replay.commands.len(), 1);
    assert_eq!(replay.commands[0].tick, 50, "logged at the tick it ran, not the stale due tick");
    assert_eq!(verify_replay(&replay, crater_ridge(), rules()), Ok(world.state_hash()));
}

#[test]
fn ai_vs_ai_is_deterministic_for_every_faction_pairing() {
    for f0 in Faction::ALL {
        for f1 in Faction::ALL {
            let s = setup(3, [true, true], [f0, f1]);
            let (a, ra) = record(World::new(&s, rules()), &s, 3_000, |_| {});
            let end = verify_replay(&ra, crater_ridge(), rules()).unwrap();
            assert_eq!(end, a.state_hash());
            assert!(ra.commands.is_empty());
        }
    }
}

/// In a mirror match on Crater Ridge (starts at (80,80) and (320,320)), the second AI's base is
/// the first one's rotated 180° about the map centre: same buildings, mirrored positions.
/// Checked after the opening, when every building is placed by the AI's own layout rules.
#[test]
fn mirror_matches_build_mirrored_bases() {
    for f in Faction::ALL {
        let s = setup(1, [true, true], [f, f]);
        let mut w = World::new(&s, rules());
        while w.tick() < 30 * 100 {
            w.step();
        }
        let base = |p: u8| {
            let mut v: Vec<(&str, i64, i64)> = w
                .entities()
                .filter(|e| e.owner == Some(PlayerId(p)) && rules().def(e.def).is_building())
                .map(|e| {
                    let (x, y) = (e.pos.x.floor_int(), e.pos.y.floor_int());
                    let (x, y) = if p == 1 { (400 - x, 400 - y) } else { (x, y) };
                    (rules().def(e.def).name, x, y)
                })
                .collect();
            v.sort();
            v
        };
        assert!(base(0).len() >= 4, "{f:?}: {:?}", base(0));
        assert_eq!(base(0), base(1), "{f:?}");
    }
}

/// Every faction pairing, played AI against AI, ends with exactly one winner within 15 minutes,
/// and the recorded match reproduces exactly: combat (targeting, hits, deaths, the AI's
/// waves and rebuilding) is deterministic end to end.
#[test]
fn ai_matches_end_with_a_winner_and_replay_exactly() {
    for f0 in Faction::ALL {
        for f1 in Faction::ALL {
            let s = setup(1, [true, true], [f0, f1]);
            let mut w = World::new(&s, rules());
            while w.winner().is_none() && w.tick() < 30 * 60 * 15 {
                w.step();
            }
            let winner = w.winner().unwrap_or_else(|| panic!("{f0:?} vs {f1:?}: no winner in 15 minutes"));
            let loser = PlayerId(1 - winner.0);
            assert!(w.player(loser).defeated);
            assert!(w.entities().all(|e| e.owner != Some(loser)), "{f0:?} vs {f1:?}: loser's assets remain");
            let (end, replay) = record(World::new(&s, rules()), &s, w.tick(), |_| {});
            assert_eq!(end.state_hash(), w.state_hash());
            assert_eq!(verify_replay(&replay, crater_ridge(), rules()), Ok(w.state_hash()));
        }
    }
}

/// The opponent is passive: since combat, an AI opponent can end the match before five
/// minutes, and this measures one AI's economy, not who wins.
#[test]
fn ai_builds_an_economy() {
    for f in Faction::ALL {
        let s = setup(5, [true, false], [f, Faction::Eu]);
        let (w, _) = record(World::new(&s, rules()), &s, 30 * 300, |_| {});
        let collectors =
            w.entities().filter(|e| e.owner == Some(PlayerId(0)) && rules().def(e.def).collector().is_some()).count();
        assert!(collectors >= 2, "{f:?}: {collectors} collectors after 5 min");
        let start = |pos| s.map.oil_wells.iter().find(|&&(p, _)| p == pos).map_or(0, |&(_, r)| r);
        let pumping = w.entities().any(|e| {
            e.owner == Some(PlayerId(0))
                && e.is_complete()
                && e.on_well.and_then(|wl| w.entity(wl)).is_some_and(|wl| wl.resource < start(wl.pos))
        });
        assert!(pumping, "{f:?}: no derrick has drawn on its well after 5 min");
    }
}

/// Golden replay: guards against accidental behaviour changes. If a change is intended,
/// regenerate with `G2_BLESS=1 cargo test -p g2-rules --test determinism golden`.
#[test]
fn golden_replay_still_matches() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/eu-vs-gla-ai.g2rp");
    if std::env::var("G2_BLESS").as_deref() == Ok("1") {
        let (_, replay) = scripted_match(1, 30 * 180);
        std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap()).unwrap();
        std::fs::write(path, replay.encode()).unwrap();
    }
    let bytes = std::fs::read(path).expect("golden replay missing; run with G2_BLESS=1 once");
    let replay = Replay::decode(&bytes).unwrap();
    if let Err(d) = verify_replay(&replay, crater_ridge(), rules()) {
        panic!(
            "simulation changed: first divergence at tick {} (expected {:016x}, got {:016x})",
            d.tick, d.expected, d.actual
        );
    }
}
