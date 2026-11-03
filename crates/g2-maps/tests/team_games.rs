//! Team games between AIs on an imported four-start map: two Normal AIs on each side.

use g2_rules::rules;
use g2_sim::world::Event;
use g2_sim::{AiLevel, Faction, MatchSetup, PlayerId, PlayerSetup, World, verify_replay};

/// A 2v2 of Hard AIs on Tournament Tundra (the west starts on team 1, the east on team 2),
/// each faction in each seat once: every match ends within 25 minutes with one side standing
/// and everyone on it winning, no one ever hits an ally (direct fire, shells or powers), and
/// the replay plays back to the same state. Measured: the three end at 395, 418 and 548 s
/// (1244, 626 and 819 s with the alpha's unit times and the cards' building values, the first
/// stalled past 40 minutes until an AI with no income left sent what it had; 695, 563 and
/// 426 s before either).
#[test]
fn normal_ais_play_a_two_on_two_to_the_end() {
    let map = g2_maps::resolve("gen:Tournament Tundra").unwrap();
    assert_eq!(map.spawns.len(), 4);
    for rot in 0..Faction::ALL.len() {
        let players: Vec<PlayerSetup> = (0..4)
            .map(|i| PlayerSetup {
                faction: Faction::ALL[(i + rot) % Faction::ALL.len()],
                starting_credits: 5_000,
                ai: Some(AiLevel::Hard),
                general: None,
                perks: Vec::new(),
                rank: 1,
                team: [1, 1, 2, 2][i],
            })
            .collect();
        let s = MatchSetup { map: map.clone(), seed: 1, players };
        let mut ally_hits = 0;
        let (end, replay) = g2_sim::record(World::new(&s, rules()), &s, 30 * 60 * 25, |w| {
            if w.winner().is_some() {
                return;
            }
            ally_hits += w
                .events()
                .iter()
                .filter(|e| matches!(e, Event::Hit { by, victim: Some(v), .. } if by != v && w.allied(*by, *v)))
                .count();
        });
        let winner = end.winner().unwrap_or_else(|| panic!("rotation {rot}: no side won in 25 minutes"));
        let side: Vec<bool> = (0..4).map(|i| end.won(PlayerId(i))).collect();
        assert!(side == [true, true, false, false] || side == [false, false, true, true], "{rot}: {side:?}");
        assert!(end.won(winner));
        assert_eq!(ally_hits, 0, "rotation {rot}: allies hit each other");
        assert_eq!(verify_replay(&replay, map.clone(), rules()), Ok(end.state_hash()), "rotation {rot}");
    }
}
