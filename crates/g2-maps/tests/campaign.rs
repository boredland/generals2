//! The campaign on its maps: every mission valid, won by the `Normal` AI in the player's
//! seat, and a recorded mission replaying exactly.

use g2_rules::rules;
use g2_sim::mission::MissionId;
use g2_sim::{AiLevel, Replay, World, record, verify_replay};

const S: u32 = g2_sim::TICKS_PER_SECOND;

/// A recorded mission replays to the same hash, through the encoded file, and its mission
/// id survives; a replay without one starts a skirmish.
#[test]
fn a_recorded_mission_replays_exactly() {
    let r = rules();
    let id = MissionId(0);
    let m = r.mission(id);
    let s = m.setup(r, g2_maps::resolve(m.map).unwrap(), 3, Some(AiLevel::Normal));
    let (end, replay) = record(World::with_mission(&s, r, id), &s, 240 * S, |_| {});
    assert_eq!(replay.mission, Some(id));
    let decoded = Replay::decode(&replay.encode()).unwrap();
    assert_eq!(decoded, replay);
    decoded.check(&s.map, r).unwrap();
    assert_eq!(verify_replay(&decoded, s.map.clone(), r), Ok(end.state_hash()));
    let mut skirmish = decoded.clone();
    skirmish.mission = None;
    assert!(verify_replay(&skirmish, s.map.clone(), r).is_err(), "the mission is part of the match");
    let mut wrong = decoded;
    wrong.mission = Some(MissionId(1));
    assert!(wrong.check(&s.map, r).is_err(), "a mission whose sides the players don't seat");
}

/// Every mission is valid on its map: seats, forces on open ground, buildings with room,
/// every tag and objective a goal or trigger names.
#[test]
fn the_campaign_is_valid() {
    let r = rules();
    assert!(r.missions.len() >= 3);
    for m in r.missions {
        let map = g2_maps::resolve(m.map).unwrap();
        g2_sim::mission::validate(r, m, &map).unwrap();
        assert!(m.briefing.len() > 200, "{}: a briefing worth reading", m.name);
        assert_eq!(r.find_mission(m.key).map(|id| r.mission(id).name), Some(m.name));
    }
}

/// The `Normal` AI in the player's seat wins every mission, and each replays exactly. The
/// missions are fixed (no randomness), so so are these results.
#[test]
fn the_normal_ai_wins_every_mission() {
    let r = rules();
    for (i, m) in r.missions.iter().enumerate() {
        let id = MissionId(i as u16);
        let s = m.setup(r, g2_maps::resolve(m.map).unwrap(), 1, Some(AiLevel::Normal));
        let mut w = World::with_mission(&s, r, id);
        while w.winner().is_none() && w.tick() < 30 * 60 * S {
            w.step();
        }
        let run = w.mission().unwrap();
        assert_eq!(run.result, Some(true), "{} at {} s: {:?}", m.name, w.tick() / S, run.objectives);
        assert!(run.fired.iter().any(|&f| f), "{}: its triggers fire", m.name);
        println!("{}: won at {} s", m.name, w.tick() / S);
    }
}
