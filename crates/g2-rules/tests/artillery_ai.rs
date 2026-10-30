//! Where the AI's artillery stands in a fight. `g2-tools artillery LEVEL --map NAME` prints the
//! same measure for any map.

use g2_rules::{crater_ridge, rules};
use g2_sim::entity::DeployState;
use g2_sim::world::Event;
use g2_sim::{AiLevel, DefId, Faction, MatchSetup, PlayerId, PlayerSetup, TICKS_PER_SECOND, Vec2, World};

/// Regression guard on gun stand-off: while `Normal`'s field army is in a fight, a gun out of
/// the base stands farther from the nearest visible enemy than the line's closest unit, by a
/// good margin, nearly always; most guns leave the base, and most survive. Nothing positions
/// them on purpose: range alone does it (a gun stops at its reach, 55-65, while the line closes
/// to 25-35), and `Easy` stands as far back (median 38). Where the levels differ is losses.
/// Measured on Crater Ridge, every pairing, both starts, against Easy: median 36 units behind,
/// 96% of samples more than 5 behind, 21 of 24 guns out of the base, 10 lost (11 of 19 out
/// and 10 lost before the AI stopped walling its war factory in, when the guns that never
/// left were EU howitzers trapped at home).
#[test]
fn normal_artillery_stands_behind_its_line() {
    let r = rules();
    let is_gun = |d: DefId| {
        let d = r.def(d);
        d.weapon_for(d.deploy.is_some()).is_some_and(|w| w.shell.is_some())
    };
    let (mut behind, mut built, mut left, mut lost) = (Vec::new(), 0, 0, 0);
    for fa in Faction::ALL {
        for fb in Faction::ALL {
            for seat in 0..2u8 {
                let me = PlayerId(seat);
                let mut players = vec![
                    PlayerSetup {
                        faction: fa,
                        starting_credits: 5_000,
                        ai: Some(AiLevel::Normal),
                        general: None,
                        perks: Vec::new(),
                        rank: 1,
                        team: 0,
                    },
                    PlayerSetup {
                        faction: fb,
                        starting_credits: 5_000,
                        ai: Some(AiLevel::Easy),
                        general: None,
                        perks: Vec::new(),
                        rank: 1,
                        team: 0,
                    },
                ];
                if seat == 1 {
                    players.swap(0, 1);
                }
                let mut w = World::new(&MatchSetup { map: crater_ridge(), seed: 1, players }, r);
                let (mut guns, mut out) = (std::collections::BTreeSet::new(), std::collections::BTreeSet::new());
                while w.winner().is_none() && w.tick() < 20 * 60 * TICKS_PER_SECOND {
                    w.step();
                    lost += w
                        .events()
                        .iter()
                        .filter(|e| matches!(e, Event::Hit { killed: true, target, .. } if guns.contains(target)))
                        .count();
                    if !w.tick().is_multiple_of(2 * TICKS_PER_SECOND) {
                        continue;
                    }
                    let vision = w.vision(me);
                    let enemies: Vec<Vec2> = w
                        .entities()
                        .filter(|e| e.owner.is_some_and(|o| o != me) && vision.is_visible(e.pos))
                        .map(|e| e.pos)
                        .collect();
                    let buildings: Vec<(Vec2, f32)> = w
                        .entities()
                        .filter(|e| e.owner == Some(me) && r.def(e.def).is_building())
                        .map(|e| (e.pos, r.def(e.def).radius.to_f32()))
                        .collect();
                    // Out of the base: 40 units clear of every building's edge, as the AI's
                    // own base zone (`ai.rs`, `DEFEND_RADIUS`).
                    let in_field = |p: Vec2| buildings.iter().all(|&(b, br)| b.distance(p).to_f32() > 40.0 + br);
                    let nearest = |p: Vec2| enemies.iter().map(|q| q.distance(p).to_f32()).fold(f32::MAX, f32::min);
                    let armed = |e: &&g2_sim::Entity| {
                        is_gun(e.def) || r.def(e.def).weapon_for(e.deploy != DeployState::Packed).is_some()
                    };
                    let mine: Vec<_> = w.entities().filter(|e| e.owner == Some(me)).filter(armed).collect();
                    for g in mine.iter().filter(|e| is_gun(e.def)) {
                        guns.insert(g.id);
                        if in_field(g.pos) {
                            out.insert(g.id);
                        }
                    }
                    let front = mine
                        .iter()
                        .filter(|e| !is_gun(e.def) && in_field(e.pos))
                        .map(|e| nearest(e.pos))
                        .fold(f32::MAX, f32::min);
                    if front <= 60.0 {
                        behind.extend(
                            mine.iter().filter(|e| is_gun(e.def) && in_field(e.pos)).map(|g| nearest(g.pos) - front),
                        );
                    }
                }
                built += guns.len();
                left += out.len();
            }
        }
    }
    behind.sort_by(f32::total_cmp);
    let median = behind[behind.len() / 2];
    let held = behind.iter().filter(|&&d| d > 5.0).count() as f32 / behind.len() as f32;
    assert!(behind.len() > 300, "too few fight samples: {}", behind.len());
    assert!(median >= 20.0, "guns stand only {median} behind the front at the median");
    assert!(held >= 0.75, "guns behind the front in only {:.0}% of samples", held * 100.0);
    assert!(left * 2 >= built, "only {left} of {built} guns ever left the base");
    assert!(lost * 3 <= built * 2, "{lost} of {built} guns lost");
}
