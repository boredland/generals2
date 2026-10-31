//! The Normal AI's superweapon, measured in play on an imported map: rich enough, a Normal GLA
//! builds a Scud Storm, fires it once charged, and never uses the GPS Scrambler; Easy builds
//! no silo (it has no superweapon in its build order) and never uses the scrambler either (it
//! fires only powers that hit).

use g2_rules::ids::GLA_SCUD_STORM;
use g2_rules::rules;
use g2_sim::world::Event;
use g2_sim::{AiLevel, Faction, MatchSetup, PlayerId, PlayerSetup, World};

/// A GLA mirror on Tournament Desert with 40,000 credits each, Normal (or Easy) in the first
/// seat against Normal: the first seat's silo stands at 273 s and its strike flies at 694 s,
/// and it wins at 757 s; Easy loses at 330 s with no silo (measured with Sabotage, Easy's
/// powers and Normal's gun placement in). Against an APA or EU Normal, on this map or Heartland
/// Shield, the match ends before the 420 s charge, and on Tournament Urban, Golden Oasis or
/// Winding River before a silo stands.
#[test]
fn the_normal_ai_builds_and_fires_the_scud_storm() {
    let map = g2_maps::resolve("gen:Tournament Desert").unwrap();
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
        let (mut built, mut used) = ([false; 2], [Vec::new(), Vec::new()]);
        while w.winner().is_none() && w.tick() < 30 * 60 * 25 {
            w.step();
            for e in w.events() {
                match *e {
                    Event::Completed { player: PlayerId(p), def, .. } if def == GLA_SCUD_STORM => {
                        built[p as usize] = true
                    }
                    Event::PowerUsed { player: PlayerId(p), slot, .. } => used[p as usize].push(slot),
                    _ => {}
                }
            }
        }
        for (p, slots) in used.iter().enumerate() {
            assert!(!slots.contains(&2), "seat {p} used the scrambler: {slots:?}");
        }
        if level == AiLevel::Easy {
            assert!(!built[0], "Easy built a Scud Storm");
        } else {
            assert!(built[0], "Normal built no Scud Storm");
            assert!(used[0].contains(&1), "Normal never fired it: {:?}", used[0]);
        }
    }
}
