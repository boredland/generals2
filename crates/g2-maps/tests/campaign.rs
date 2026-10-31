//! The campaign on its maps: every mission valid, won by the `Normal` AI in the player's
//! seat on Easy and Normal, and a recorded scripted mission replaying exactly.

use g2_rules::rules;
use g2_sim::mission::{Difficulty, Script};
use g2_sim::{AiLevel, Replay, World, record, verify_replay};

const S: u32 = g2_sim::TICKS_PER_SECOND;

/// The mission `key` at `d`, with the `Normal` AI in the player's seat.
fn start(key: &str, d: Difficulty, seed: u64) -> (World, g2_sim::MatchSetup) {
    let r = rules();
    let id = r.find_mission(key).unwrap();
    let m = r.mission(id);
    let s = m.setup(r, g2_maps::resolve(m.map).unwrap(), seed, Some(AiLevel::Normal));
    (World::with_mission(&s, r, id, d), s)
}

/// How a mission played out: won (`Some(true)`), lost or undecided, and the second it ended.
type Outcome = (Option<bool>, u32);

/// Plays a mission out (30 minutes at most).
fn play(key: &str, d: Difficulty) -> Outcome {
    let (mut w, _) = start(key, d, 1);
    while w.winner().is_none() && w.tick() < 30 * 60 * S {
        w.step();
    }
    let run = w.mission().unwrap();
    assert!(run.fired.iter().any(|&f| f > 0), "{key}: its triggers fire");
    (run.result, w.tick() / S)
}

/// A recorded mission with scripted squads replays to the same hash, through the encoded
/// file, its mission and difficulty kept; a replay without one starts a skirmish, and one at
/// another difficulty plays another match.
#[test]
fn a_recorded_scripted_mission_replays_exactly() {
    let r = rules();
    let (w, s) = start("ash-road", Difficulty::Hard, 3);
    let id = r.find_mission("ash-road").unwrap();
    assert!(w.mission().unwrap().squads.iter().any(|q| matches!(q.script, Script::Patrol(_))));
    let (end, replay) = record(w, &s, 240 * S, |_| {});
    assert_eq!(replay.mission, Some((id, Difficulty::Hard)));
    let decoded = Replay::decode(&replay.encode()).unwrap();
    assert_eq!(decoded, replay);
    decoded.check(&s.map, r).unwrap();
    assert_eq!(verify_replay(&decoded, s.map.clone(), r), Ok(end.state_hash()));
    let mut skirmish = decoded.clone();
    skirmish.mission = None;
    assert!(verify_replay(&skirmish, s.map.clone(), r).is_err(), "the mission is part of the match");
    let mut easier = decoded.clone();
    easier.mission = Some((id, Difficulty::Normal));
    assert!(verify_replay(&easier, s.map.clone(), r).is_err(), "so is its difficulty");
    let mut wrong = decoded;
    wrong.mission = Some((r.find_mission("frozen-watch").unwrap(), Difficulty::Hard));
    assert!(wrong.check(&s.map, r).is_err(), "a mission whose sides the players don't seat");
}

/// Every mission is valid on its map: seats, forces on open ground, buildings with room,
/// scripts on open ground, every tag and objective a goal or trigger names.
#[test]
fn the_campaign_is_valid() {
    let r = rules();
    assert!(r.missions.len() >= 8);
    for m in r.missions {
        let map = g2_maps::resolve(m.map).unwrap();
        g2_sim::mission::validate(r, m, &map).unwrap();
        assert!(m.briefing.len() > 200, "{}: a briefing worth reading", m.name);
        assert_eq!(r.find_mission(m.key).map(|id| r.mission(id).name), Some(m.name));
    }
}

/// The `Normal` AI in the player's seat wins every mission on Easy and Normal. Hard is
/// played and reported, not required (the README records the results). The missions are
/// fixed (no randomness), so so are these results.
#[test]
fn the_normal_ai_wins_every_mission() {
    let r = rules();
    let results: Vec<(&str, Difficulty, Outcome)> = std::thread::scope(|scope| {
        let runs: Vec<_> = r
            .missions
            .iter()
            .flat_map(|m| Difficulty::ALL.map(|d| (m.key, d)))
            .map(|(key, d)| (key, d, scope.spawn(move || play(key, d))))
            .collect();
        runs.into_iter().map(|(key, d, h)| (key, d, h.join().unwrap())).collect()
    });
    for (key, d, (result, secs)) in &results {
        println!("{key} on {}: {:?} at {secs} s", d.name(), result);
    }
    for (key, d, (result, secs)) in results {
        if d != Difficulty::Hard {
            assert_eq!(result, Some(true), "{key} on {} at {secs} s", d.name());
        }
    }
}
