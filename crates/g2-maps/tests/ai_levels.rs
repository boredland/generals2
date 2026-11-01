//! The AI levels against each other on a fixed sample of maps, and the slower levels' pace
//! against a player who is still building.

use g2_rules::rules;
use g2_sim::world::Event;
use g2_sim::{AiLevel, Faction, MatchSetup, PlayerId, PlayerSetup, World, record, verify_replay};

fn player(faction: Faction, ai: Option<AiLevel>) -> PlayerSetup {
    PlayerSetup {
        faction,
        starting_credits: 5_000,
        ai,
        general: None,
        perks: Vec::new(),
        rank: ai.map_or(1, g2_sim::ai::ai_rank),
        team: 0,
    }
}

/// Each level against the one below, every faction pairing from both starts on three maps:
/// the stronger wins clearly (measured on Crater Ridge, Tournament Desert and Alpine
/// Assault: Hard against Normal 49 of 54, 17, 14 and 18; Normal against Easy 51 of 54, 16,
/// 18 and 17; Hard against Easy all 54). Hard is the AI that was `Normal` before the levels
/// were split, whose record against Easy was 45 of 54 (1,389 to 158 across all 86 skirmish
/// maps); Easy now plays with its handicaps and pace, and loses every one. The AIs have no
/// randomness, so these matches are fixed. Mirror matches, with the AI's own memory in the
/// state hash, replay exactly.
#[test]
fn each_ai_level_beats_the_one_below_and_replays_exactly() {
    for (strong, weak, need) in [(AiLevel::Hard, AiLevel::Normal, 45), (AiLevel::Normal, AiLevel::Easy, 45)] {
        let mut wins = 0;
        for name in ["Crater Ridge", "zh:Tournament Desert", "gen:Alpine Assault"] {
            let map = g2_maps::resolve(name).unwrap();
            for f0 in Faction::ALL {
                for f1 in Faction::ALL {
                    for seat in [PlayerId(0), PlayerId(1)] {
                        let level = |i: usize| if i == seat.0 as usize { strong } else { weak };
                        let players = vec![player(f0, Some(level(0))), player(f1, Some(level(1)))];
                        let s = MatchSetup { map: map.clone(), seed: 1, players };
                        let (end, replay) = record(World::new(&s, rules()), &s, 30 * 60 * 20, |_| {});
                        wins += usize::from(end.winner() == Some(seat));
                        if f0 == f1 && name == "Crater Ridge" {
                            assert_eq!(verify_replay(&replay, map.clone(), rules()), Ok(end.state_hash()), "{f0:?}");
                        }
                    }
                }
            }
        }
        assert!(wins >= need, "{strong:?} won only {wins} of 54 against {weak:?}");
    }
}

/// Against a player who builds nothing, Easy and Normal hold their first attack until their
/// pace allows it (6 and 4 minutes) and keep their army small: measured on Crater Ridge, the
/// first hit lands at 397-401 s from Easy and 275-292 s from Normal, against 194-230 s from
/// Hard, and at 6:00 Easy has 7 armed units and Normal 13-14, against Hard's 33-40.
#[test]
fn easy_and_normal_give_a_new_player_time() {
    let r = rules();
    for level in [AiLevel::Easy, AiLevel::Normal, AiLevel::Hard] {
        for f in Faction::ALL {
            let s = MatchSetup {
                map: g2_rules::crater_ridge(),
                seed: 1,
                players: vec![player(Faction::Eu, None), PlayerSetup { team: 1, ..player(f, Some(level)) }],
            };
            let mut w = World::new(&s, r);
            let mut first_hit = None;
            while w.tick() < 30 * 60 * 6 {
                w.step();
                let hit_ours = w.events().iter().any(|e| matches!(*e, Event::Hit { victim: Some(PlayerId(0)), .. }));
                if hit_ours && first_hit.is_none() {
                    first_hit = Some(w.tick());
                }
            }
            let army = w
                .entities()
                .filter(|e| e.owner == Some(PlayerId(1)) && r.def(e.def).is_unit() && r.def(e.def).weapon().is_some())
                .count();
            let pace = level.pace();
            assert!(
                first_hit.is_none_or(|t| t >= pace.first_attack),
                "{level:?} {f:?} hit us at {first_hit:?}, before its first attack at {}",
                pace.first_attack
            );
            match level {
                AiLevel::Easy => assert!(army <= 8, "{f:?} Easy has {army} armed units at 6:00"),
                AiLevel::Normal => assert!(army <= 15, "{f:?} Normal has {army} armed units at 6:00"),
                _ => assert!(army >= 25 && first_hit.is_some(), "{f:?} Hard: {army} units, first hit {first_hit:?}"),
            }
        }
    }
}
