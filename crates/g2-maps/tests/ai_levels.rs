//! The two AI levels against each other on a fixed sample of maps.

use g2_rules::rules;
use g2_sim::{AiLevel, Faction, MatchSetup, PlayerId, PlayerSetup, World, record, verify_replay};

/// `Normal` against `Easy`, every faction pairing from both starts on three maps: `Normal`
/// wins clearly overall (measured 47 of 54: Crater Ridge 12, Tournament Desert 18, Alpine
/// Assault 17; across all 86 skirmish maps 1,387 to 152, 9 undecided). Before Easy fired
/// its powers and pulled hit constructors home, and Normal placed its guns and routed its
/// constructors around threats: 48 of 54 (Alpine Assault 18) and 1,401 to 143, 4 undecided;
/// 36 of 54 and 1,254 to 272 before the balance pass, when war factories could wall in
/// their own tanks. Crater Ridge is still the closest: Normal's first wave is small and Easy
/// can out-trade it at both ends of the map. The AIs have no randomness, so these matches
/// are fixed. Mirror matches, with the AI's own memory in the state hash, replay exactly.
#[test]
fn normal_ai_beats_easy_and_replays_exactly() {
    let mut normal_wins = 0;
    for name in ["Crater Ridge", "zh:Tournament Desert", "gen:Alpine Assault"] {
        let map = g2_maps::resolve(name).unwrap();
        for f0 in Faction::ALL {
            for f1 in Faction::ALL {
                for normal in [PlayerId(0), PlayerId(1)] {
                    let players = [f0, f1]
                        .iter()
                        .enumerate()
                        .map(|(i, &faction)| PlayerSetup {
                            faction,
                            starting_credits: 5_000,
                            ai: Some(if i == normal.0 as usize { AiLevel::Normal } else { AiLevel::Easy }),
                            general: None,
                            perks: Vec::new(),
                            rank: 1,
                            team: 0,
                        })
                        .collect();
                    let s = MatchSetup { map: map.clone(), seed: 1, players };
                    let (end, replay) = record(World::new(&s, rules()), &s, 30 * 60 * 20, |_| {});
                    normal_wins += usize::from(end.winner() == Some(normal));
                    if f0 == f1 && name == "Crater Ridge" {
                        assert_eq!(verify_replay(&replay, map.clone(), rules()), Ok(end.state_hash()), "{f0:?}");
                    }
                }
            }
        }
    }
    assert!(normal_wins >= 47, "Normal won only {normal_wins} of 54 against Easy");
}
