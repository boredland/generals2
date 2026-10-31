//! The Normal AI's superweapon, measured in play on an imported map: rich enough, a Normal GLA
//! builds a Scud Storm, fires it once charged, and never uses the GPS Scrambler; Easy does
//! neither.

use g2_rules::ids::GLA_SCUD_STORM;
use g2_rules::rules;
use g2_sim::world::Event;
use g2_sim::{AiLevel, Faction, MatchSetup, PlayerId, PlayerSetup, World};

/// A GLA player on Tournament Desert with 40,000 credits each, Normal (or Easy) against a
/// Normal APA: Normal's silo stands at 273 s and its strike flies at 694 s (measured). (The
/// GLA mirror ended at 517 s, before the strike, once both sides sabotaged each other; on
/// Tournament Urban it ended at 557 s before a silo; it stood at 862 s and fired at 1,283 s
/// there while the AI walled its war factories in.)
#[test]
fn the_normal_ai_builds_and_fires_the_scud_storm() {
    let map = g2_maps::resolve("gen:Tournament Desert").unwrap();
    for level in [AiLevel::Normal, AiLevel::Easy] {
        let player = |faction, ai| PlayerSetup {
            faction,
            starting_credits: 40_000,
            ai: Some(ai),
            general: None,
            perks: Vec::new(),
            rank: 1,
            team: 0,
        };
        let players = vec![player(Faction::Gla, level), player(Faction::Apa, AiLevel::Normal)];
        let s = MatchSetup { map: map.clone(), seed: 1, players };
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
