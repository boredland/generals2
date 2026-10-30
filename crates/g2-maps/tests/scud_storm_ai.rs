//! The Normal AI's superweapon, measured in play on an imported map: rich enough, a Normal GLA
//! builds a Scud Storm, fires it once charged, and never uses the GPS Scrambler; Easy does
//! neither.

use g2_rules::ids::GLA_SCUD_STORM;
use g2_rules::rules;
use g2_sim::world::Event;
use g2_sim::{AiLevel, Faction, MatchSetup, PlayerId, PlayerSetup, World};

/// A GLA mirror on Tournament Urban with 40,000 credits each, Normal (or Easy) against
/// Normal: Normal's silo stands at 862 s and its strike flies at 1,283 s (measured). (On
/// Tournament Desert, with the alpha's footprints, the mirror ends at 349 s before a silo.)
#[test]
fn the_normal_ai_builds_and_fires_the_scud_storm() {
    let map = g2_maps::resolve("zh:Tournament Urban").unwrap();
    for level in [AiLevel::Normal, AiLevel::Easy] {
        let player = |ai| PlayerSetup {
            faction: Faction::Gla,
            starting_credits: 40_000,
            ai: Some(ai),
            general: None,
            perks: Vec::new(),
            rank: 1,
            team: 0,
        };
        let s = MatchSetup { map: map.clone(), seed: 1, players: vec![player(level), player(AiLevel::Normal)] };
        let mut w = World::new(&s, rules());
        let (mut built, mut used) = (false, Vec::new());
        while w.winner().is_none() && w.tick() < 30 * 60 * 25 {
            w.step();
            built |= w
                .events()
                .iter()
                .any(|e| matches!(e, Event::Completed { player: PlayerId(0), def, .. } if *def == GLA_SCUD_STORM));
            used.extend(w.events().iter().filter_map(|e| match e {
                Event::PowerUsed { player: PlayerId(0), slot, .. } => Some(*slot),
                _ => None,
            }));
        }
        if level == AiLevel::Easy {
            assert!(!built && used.is_empty(), "Easy built {built}, used {used:?}");
        } else {
            assert!(built, "Normal built no Scud Storm");
            assert!(used.contains(&1), "Normal never fired it: {used:?}");
            assert!(!used.contains(&2), "Normal used the scrambler: {used:?}");
        }
    }
}
