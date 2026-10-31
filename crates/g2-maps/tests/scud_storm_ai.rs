//! The Normal AI's superweapon, measured in play on an imported map: rich enough, a Normal GLA
//! builds a Scud Storm, fires it once charged, and never uses the GPS Scrambler; Easy builds
//! no silo (it has no superweapon in its build order) and never uses the scrambler either (it
//! fires only powers that hit).

use g2_rules::ids::GLA_SCUD_STORM;
use g2_rules::rules;
use g2_sim::world::Event;
use g2_sim::{AiLevel, Faction, MatchSetup, PlayerId, PlayerSetup, World};

/// A GLA mirror on Heartland Shield with 40,000 credits each, Normal (or Easy) in the first
/// seat against Normal: in the Normal mirror both silos stand by 284 s and the second seat's
/// strike flies at 702 s, winning at 789 s. On Tournament Desert (the map before the GLA
/// tanks had Nitro) the first seat's silo now falls at about 675 s, just short of its 420 s
/// charge, and neither fires (it fired at 694 s before). Against an APA or EU Normal the
/// match ends before the charge, and on Tournament Urban, Golden Oasis or Winding River
/// before a silo stands.
#[test]
fn the_normal_ai_builds_and_fires_the_scud_storm() {
    let map = g2_maps::resolve("gen:Heartland Shield").unwrap();
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
            assert!(used.iter().any(|s| s.contains(&1)), "no Normal fired it: {used:?}");
        }
    }
}
