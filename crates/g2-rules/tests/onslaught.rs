//! Onslaught: a human (here scripted or passive) against scheduled waves, through public
//! commands and the match setup only.

use g2_rules::{crater_ridge, rules};
use g2_sim::onslaught::{WAVES, next_wave, wave_size, wave_tick};
use g2_sim::world::Event;
use g2_sim::{AiLevel, Faction, MatchSetup, PlayerId, PlayerSetup, Replay, World, record, verify_replay};

const HUMAN: PlayerId = PlayerId(0);
const WAVES_PLAYER: PlayerId = PlayerId(1);

fn setup(human: Option<AiLevel>, f: [Faction; 2]) -> MatchSetup {
    let players = vec![
        PlayerSetup {
            faction: f[0],
            starting_credits: 5_000,
            ai: human,
            general: None,
            perks: Vec::new(),
            rank: 1,
            team: 0,
        },
        PlayerSetup {
            faction: f[1],
            starting_credits: 5_000,
            ai: Some(AiLevel::Onslaught),
            general: None,
            perks: Vec::new(),
            rank: 1,
            team: 0,
        },
    ];
    MatchSetup { map: crater_ridge(), seed: 1, players }
}

/// Each wave appears on its tick with its size, all of the onslaught's faction, on open ground,
/// and nothing else of the onslaught's ever appears: it has no economy, builds nothing and its
/// credits never move.
#[test]
fn waves_appear_on_schedule_and_nothing_else_does() {
    let s = setup(None, [Faction::Eu, Faction::Apa]);
    let mut w = World::new(&s, rules());
    let start_credits = w.player(WAVES_PLAYER).credits;
    let mut seen = Vec::new();
    while w.tick() <= wave_tick(2) {
        w.step();
        let wave: Vec<_> = w
            .events()
            .iter()
            .filter_map(|e| match *e {
                Event::Produced { player, def, at } if player == WAVES_PLAYER => Some((def, at)),
                Event::Completed { player, .. } if player == WAVES_PLAYER => panic!("the onslaught built something"),
                _ => None,
            })
            .collect();
        if !wave.is_empty() {
            assert!(wave.iter().all(|&(d, at)| rules().def(d).faction == Some(Faction::Apa) && w.map.walkable(at)));
            seen.push((w.tick() - 1, wave.len() as u32));
        }
    }
    assert_eq!(seen, (0..3).map(|k| (wave_tick(k), wave_size(k))).collect::<Vec<_>>());
    assert_eq!(w.player(WAVES_PLAYER).credits, start_credits);
}

/// Left alone, the player is overrun: the waves find the base and destroy it.
#[test]
fn an_idle_player_is_overrun() {
    let s = setup(None, [Faction::Gla, Faction::Eu]);
    let mut w = World::new(&s, rules());
    while w.winner().is_none() && w.tick() < wave_tick(WAVES) + 30 * 300 {
        w.step();
    }
    assert_eq!(w.winner(), Some(WAVES_PLAYER), "at {} s", w.tick() / 30);
    assert!(w.player(HUMAN).defeated);
}

/// A `Normal` AI in the player's seat, every faction pairing on Crater Ridge (the hardest of
/// the three measured maps: 4 of 9 held, 21 of 27 across all three): every match ends, with
/// the onslaught beaten only once its last wave is out and gone, and replays exactly. The
/// waves are a real threat but can be held, so neither side wins them all.
#[test]
fn the_waves_can_be_held_or_overrun_and_replay_exactly() {
    let (mut held, mut overrun) = (0, 0);
    for f0 in Faction::ALL {
        for f1 in Faction::ALL {
            let s = setup(Some(AiLevel::Normal), [f0, f1]);
            let mut w = World::new(&s, rules());
            while w.winner().is_none() && w.tick() < wave_tick(WAVES) + 30 * 600 {
                w.step();
            }
            match w.winner() {
                Some(HUMAN) => {
                    held += 1;
                    assert!(next_wave(w.tick()).is_none(), "{f0:?}/{f1:?}: beaten before its last wave");
                }
                Some(WAVES_PLAYER) => overrun += 1,
                other => panic!("{f0:?}/{f1:?}: no result ({other:?}) at {} s", w.tick() / 30),
            }
            let (end, replay) = record(World::new(&s, rules()), &s, w.tick(), |_| {});
            assert_eq!(end.state_hash(), w.state_hash());
            let decoded = Replay::decode(&replay.encode()).unwrap();
            assert_eq!(decoded.players[1].ai, Some(AiLevel::Onslaught));
            assert_eq!(verify_replay(&decoded, crater_ridge(), rules()), Ok(w.state_hash()), "{f0:?}/{f1:?}");
        }
    }
    assert!(held > 0 && overrun > 0, "held {held}, overrun {overrun}");
}
