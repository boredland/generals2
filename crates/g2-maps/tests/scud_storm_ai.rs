//! The Normal AI's superweapon, measured in play on an imported map: rich enough, a Normal GLA
//! builds a Scud Storm, fires it once charged, and never uses the GPS Scrambler; Easy builds
//! no silo (it has no superweapon in its build order) and never uses the scrambler either (it
//! fires only powers that hit).

use g2_rules::ids::GLA_SCUD_STORM;
use g2_rules::rules;
use g2_sim::world::Event;
use g2_sim::{AiLevel, Faction, MatchSetup, PlayerId, PlayerSetup, World};

/// A GLA mirror on Heartland Shield with 40,000 credits each, Normal (or Easy) in the first
/// seat against Normal in the second: the second seat's silo stands at 281 s and its strike
/// flies at 702 s against Normal (measured; against Easy, which builds no silo, it wins at
/// 410 s first). On Tournament Desert the Normal mirror now ends at 589 s, before either
/// silo (273 and 271 s) has charged its 420 s: Normal places its guns since, and that mirror
/// fired at 694 s before. (On Tournament Urban it stood at 862 s and fired at 1,283 s while
/// the AI walled its war factories in.)
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
            assert!(built[0] && built[1], "Normal built no Scud Storm: {built:?}");
            assert!(used[1].contains(&1), "Normal never fired it: {:?}", used[1]);
        }
    }
}
