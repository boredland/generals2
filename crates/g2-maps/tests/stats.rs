//! Match statistics: running totals that must agree with what actually happened.

use g2_rules::{crater_ridge, rules};
use g2_sim::{AiLevel, Faction, MatchSetup, PlayerId, PlayerSetup, World, world::Event};

/// Three AIs on a 4-start map, played out to one survivor while every event is tallied
/// independently. The stats must match the tallies, credits must balance, and every player but
/// the winner must be recorded as defeated on the tick its assets went.
#[test]
fn stats_agree_with_the_match() {
    for (map, factions) in [
        (crater_ridge(), vec![Faction::Eu, Faction::Gla]),
        (g2_maps::resolve("gen:Tournament Tundra").unwrap(), vec![Faction::Apa, Faction::Gla, Faction::Eu]),
    ] {
        let n = factions.len();
        let setup = MatchSetup {
            map,
            seed: 1,
            players: factions
                .iter()
                .enumerate()
                .map(|(i, &faction)| PlayerSetup {
                    faction,
                    starting_credits: 5_000,
                    ai: Some(if i == 0 { AiLevel::Normal } else { AiLevel::Easy }),
                    general: None,
                    perks: Vec::new(),
                    rank: 1,
                })
                .collect(),
        };
        let name = setup.map.name.clone();
        let mut w = World::new(&setup, rules());
        let owner_of: std::collections::BTreeMap<g2_sim::EntityId, PlayerId> =
            w.entities().filter_map(|e| Some((e.id, e.owner?))).collect();
        let mut owner_of = owner_of;
        let (mut earned, mut kills, mut lost, mut gone) = (vec![0i64; n], vec![0u32; n], vec![0u32; n], vec![None; n]);
        let mut seen_units = vec![0u32; n];
        let mut seen: std::collections::BTreeSet<g2_sim::EntityId> = w.entities().map(|e| e.id).collect();
        while w.winner().is_none() && w.tick() < 30 * 60 * 30 {
            w.step();
            for e in w.events() {
                match *e {
                    Event::Delivered { player, credits, .. } => earned[player.0 as usize] += credits,
                    Event::Hit { shooter, target, killed: true, .. } => {
                        if let Some(&by) = owner_of.get(&shooter) {
                            kills[by.0 as usize] += 1;
                        }
                        lost[owner_of[&target].0 as usize] += 1;
                    }
                    _ => {}
                }
            }
            for e in w.entities() {
                if seen.insert(e.id) {
                    let o = e.owner.unwrap();
                    owner_of.insert(e.id, o);
                    // A support power's jets are launched, not built.
                    if rules().def(e.def).is_unit() && !rules().def(e.def).is_support_jet() {
                        seen_units[o.0 as usize] += 1;
                    }
                }
            }
            for (p, g) in gone.iter_mut().enumerate() {
                if g.is_none() && w.player(PlayerId(p as u8)).defeated {
                    *g = Some(w.tick() - 1);
                }
            }
        }
        let winner = w.winner().unwrap_or_else(|| panic!("{name}: no winner"));
        for p in 0..n {
            let id = PlayerId(p as u8);
            let s = w.stats(id);
            // Derricks pay too, so deliveries are a lower bound on earnings.
            assert!(
                s.earned >= earned[p] && earned[p] > 0,
                "{name} p{p}: earned {} vs delivered {}",
                s.earned,
                earned[p]
            );
            assert_eq!(w.player(id).credits, 5_000 + s.earned - s.spent, "{name} p{p}: credits don't balance");
            assert_eq!(s.units_built, seen_units[p], "{name} p{p}: units built");
            assert_eq!(s.units_lost + s.buildings_lost, lost[p], "{name} p{p}: losses");
            assert!(s.kills >= kills[p], "{name} p{p}: kills {} vs tallied {}", s.kills, kills[p]);
            assert!(s.buildings_built >= 1, "{name} p{p}: built nothing");
            assert_eq!(s.defeated_at, gone[p], "{name} p{p}: defeat tick");
            assert_eq!(s.defeated_at.is_none(), id == winner, "{name} p{p}");
        }
        let total_kills: u32 = (0..n).map(|p| w.stats(PlayerId(p as u8)).kills).sum();
        assert_eq!(total_kills, lost.iter().sum::<u32>(), "{name}: every loss is someone's kill");
    }
}
