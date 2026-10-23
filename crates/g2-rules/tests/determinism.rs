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
            .map(|(&faction, ai)| PlayerSetup { faction, starting_credits: 5_000, ai })
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

#[test]
fn ai_builds_an_economy() {
    for f in Faction::ALL {
        let s = setup(5, [true, true], [f, Faction::Eu]);
        let (w, _) = record(World::new(&s, rules()), &s, 30 * 300, |_| {});
        let p = w.player(PlayerId(0));
        let collectors =
            w.entities().filter(|e| e.owner == Some(PlayerId(0)) && rules().def(e.def).collector().is_some()).count();
        assert!(collectors >= 2, "{f:?}: {collectors} collectors after 5 min");
        assert!(p.oil > 0, "{f:?}: no oil derrick income after 5 min");
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
