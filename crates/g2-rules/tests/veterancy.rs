//! Veterancy, driven through public commands only: kills give experience, experience gives
//! ranks, ranks give the alpha's per-unit gains.

use g2_rules::ids::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::world::{Event, INPUT_DELAY};
use g2_sim::{
    CommandKind, EntityId, Faction, MatchSetup, Order, PlayerId, PlayerSetup, Vec2, World, record, verify_replay,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn world(f0: Faction, f1: Faction) -> World {
    let setup = MatchSetup {
        map: crater_ridge(),
        seed: 1,
        players: vec![
            PlayerSetup { faction: f0, starting_credits: 100_000, ai: None, general: None, perks: Vec::new() },
            PlayerSetup { faction: f1, starting_credits: 100_000, ai: None, general: None, perks: Vec::new() },
        ],
    };
    World::new(&setup, rules())
}

fn step_until(w: &mut World, limit: u32, what: &str, mut done: impl FnMut(&World) -> bool) {
    for _ in 0..limit {
        if done(w) {
            return;
        }
        w.step();
    }
    panic!("{what}: not within {limit} ticks");
}

fn first(w: &World, owner: PlayerId, def: g2_sim::DefId) -> EntityId {
    w.entities().find(|e| e.owner == Some(owner) && e.def == def).map(|e| e.id).unwrap()
}

fn build(w: &mut World, who: PlayerId, def: g2_sim::DefId, pos: Vec2) -> EntityId {
    let builder = w.entities().find(|e| e.owner == Some(who) && w.rules.def(e.def).is_constructor()).unwrap().id;
    w.submit(who, CommandKind::Build { builder, def, pos });
    step_until(w, INPUT_DELAY + 2, "foundation", |w| w.entities().any(|e| e.owner == Some(who) && e.def == def));
    let site = first(w, who, def);
    step_until(w, 30 * 120, "construction", |w| w.entity(site).is_some_and(|s| s.is_complete()));
    site
}

fn train(w: &mut World, who: PlayerId, producer: EntityId, def: g2_sim::DefId) -> EntityId {
    let before: Vec<_> = w.entities().filter(|e| e.owner == Some(who) && e.def == def).map(|e| e.id).collect();
    w.submit(who, CommandKind::Train { producer, def });
    let new = |w: &World| {
        w.entities().find(|e| e.owner == Some(who) && e.def == def && !before.contains(&e.id)).map(|e| e.id)
    };
    step_until(w, 30 * 120, "training", |w| new(w).is_some());
    new(w).unwrap()
}

/// A trained `def` of `who`'s, walked to `at` and stopped there.
fn unit_at(w: &mut World, who: PlayerId, producer: EntityId, def: g2_sim::DefId, at: Vec2) -> EntityId {
    let u = train(w, who, producer, def);
    w.submit(who, CommandKind::Move { units: vec![u], target: at });
    step_until(w, 30 * 60, "arrival", |w| w.entity(u).is_none_or(|e| e.order == Order::Idle));
    u
}

/// A veteran Aegis: it kills ten lone Technicals walked up to it one at a time, and its rank,
/// maximum hit points and fire rate follow the alpha's thresholds and gains.
#[test]
fn kills_earn_ranks_and_each_rank_its_gain() {
    let mut w = world(Faction::Eu, Faction::Gla);
    let factory = build(&mut w, P0, EU_FACTORY, Vec2::from_int(110, 80));
    let gla_factory = build(&mut w, P1, GLA_FACTORY, crater_ridge().spawns[1] + Vec2::from_int(-30, 0));
    let field = Vec2::from_int(200, 200);
    let aegis = unit_at(&mut w, P0, factory, EU_AEGIS, field);
    let v = rules().def(EU_AEGIS).veterancy.unwrap();
    assert_eq!(v.experience, [3, 6, 10], "the alpha's thresholds");
    let base_hp = rules().def(EU_AEGIS).hp;
    let gun = rules().def(EU_AEGIS).weapon().unwrap();
    let mut promotions = Vec::new();
    for kill in 1..=10u32 {
        let tech = unit_at(&mut w, P1, gla_factory, GLA_TECHNICAL, field + Vec2::from_int(25, 0));
        w.submit(P0, CommandKind::Attack { units: vec![aegis], target: tech });
        step_until(&mut w, 30 * 60, "kill", |w| w.entity(tech).is_none());
        let e = w.entity(aegis).unwrap();
        assert_eq!(e.experience, kill, "one experience per kill");
        promotions.push(w.rank(e));
        let max = w.max_hp(e);
        let want = base_hp + if w.rank(e) >= 1 { v.health } else { 0 };
        assert_eq!(max, want, "max hp at {kill} kills");
    }
    assert_eq!(promotions, vec![0, 0, 1, 1, 1, 2, 2, 2, 2, 3]);
    // Rank 3 fires faster: shots at a target that stands still (a GLA worker brought into view
    // and stopped) come at the cut interval.
    let dozer = first(&w, P1, GLA_WORKER);
    w.submit(P1, CommandKind::Move { units: vec![dozer], target: field + Vec2::from_int(20, 0) });
    step_until(&mut w, 30 * 90, "worker in sight", |w| w.vision(P0).is_visible(w.entity(dozer).unwrap().pos));
    w.submit(P1, CommandKind::Stop { units: vec![dozer] });
    w.submit(P0, CommandKind::Attack { units: vec![aegis], target: dozer });
    let mut times = Vec::new();
    step_until(&mut w, 30 * 30, "the worker dies", |w| {
        for e in w.events() {
            if let Event::Hit { shooter, target: t, .. } = *e
                && shooter == aegis
                && t == dozer
            {
                times.push(w.tick());
            }
        }
        w.entity(dozer).is_none()
    });
    let want = gun.cooldown_ticks * v.cooldown_percent / 100;
    assert!(want < gun.cooldown_ticks);
    assert!(times.len() >= 2, "{times:?}");
    for pair in times.windows(2) {
        assert_eq!(pair[1] - pair[0], want, "rank 3 interval: {times:?}");
    }
}

/// Rank 1's extra hit points come with the promotion (current and maximum alike), and a
/// rank-2 unit heals 5 a second while idle, never past its maximum, and not while it has an
/// order.
#[test]
fn promotion_health_and_idle_regeneration() {
    let mut w = world(Faction::Eu, Faction::Gla);
    let factory = build(&mut w, P0, EU_FACTORY, Vec2::from_int(110, 80));
    let gla_factory = build(&mut w, P1, GLA_FACTORY, crater_ridge().spawns[1] + Vec2::from_int(-30, 0));
    let field = Vec2::from_int(200, 200);
    let aegis = unit_at(&mut w, P0, factory, EU_AEGIS, field);
    let v = rules().def(EU_AEGIS).veterancy.unwrap();
    let mut checked = false;
    for _ in 1..=6u32 {
        let tech = unit_at(&mut w, P1, gla_factory, GLA_TECHNICAL, field + Vec2::from_int(25, 0));
        w.submit(P0, CommandKind::Attack { units: vec![aegis], target: tech });
        for _ in 0..30 * 60 {
            let before = w.entity(aegis).unwrap().hp;
            w.step();
            let promoted =
                w.events().iter().any(|e| matches!(*e, Event::Promoted { unit, rank: 1, .. } if unit == aegis));
            if promoted {
                let taken: i32 = w
                    .events()
                    .iter()
                    .filter_map(|e| match *e {
                        Event::Hit { target, damage, .. } if target == aegis => Some(damage),
                        _ => None,
                    })
                    .sum();
                assert_eq!(
                    w.entity(aegis).unwrap().hp,
                    before - taken + v.health,
                    "the rank's hit points come with it"
                );
                assert_eq!(w.max_hp(w.entity(aegis).unwrap()), rules().def(EU_AEGIS).hp + v.health);
                checked = true;
            }
            if w.entity(tech).is_none() {
                break;
            }
        }
    }
    assert!(checked, "promoted to rank 1 on the way");
    assert_eq!(w.rank(w.entity(aegis).unwrap()), 2);
    // Idle: 5 a second, up to the maximum.
    w.submit(P0, CommandKind::Stop { units: vec![aegis] });
    step_until(&mut w, INPUT_DELAY + 2, "stop", |w| w.entity(aegis).unwrap().order == Order::Idle);
    let max = w.max_hp(w.entity(aegis).unwrap());
    let start = w.entity(aegis).unwrap().hp;
    assert!(start < max - 20, "damaged enough to watch it heal ({start}/{max})");
    let t0 = w.tick();
    step_until(&mut w, 30 * 5, "4 s idle", |w| w.tick() >= t0 + 30 * 4);
    let healed = w.entity(aegis).unwrap().hp - start;
    assert!((15..=25).contains(&healed), "healed {healed} in 4 s idle");
    // Moving: no regeneration.
    let mid = w.entity(aegis).unwrap().hp;
    w.submit(P0, CommandKind::Move { units: vec![aegis], target: Vec2::from_int(120, 120) });
    let t1 = w.tick();
    step_until(&mut w, 30 * 4, "3 s moving", |w| w.tick() >= t1 + 30 * 3);
    assert!(w.entity(aegis).unwrap().order != Order::Idle, "still on the move");
    assert_eq!(w.entity(aegis).unwrap().hp, mid, "no regeneration on the move");
    // Never past full.
    step_until(&mut w, 30 * 60, "back at rest", |w| w.entity(aegis).unwrap().order == Order::Idle);
    for _ in 0..30 * 120 {
        w.step();
    }
    assert_eq!(w.entity(aegis).unwrap().hp, max);
}

/// Experience is part of the state, so a match with kills and promotions replays exactly.
#[test]
fn matches_with_promotions_replay_exactly() {
    let s = MatchSetup {
        map: crater_ridge(),
        seed: 2,
        players: vec![
            PlayerSetup {
                faction: Faction::Apa,
                starting_credits: 10_000,
                ai: Some(g2_sim::AiLevel::Normal),
                general: None,
                perks: Vec::new(),
            },
            PlayerSetup {
                faction: Faction::Gla,
                starting_credits: 10_000,
                ai: Some(g2_sim::AiLevel::Normal),
                general: None,
                perks: Vec::new(),
            },
        ],
    };
    let mut promotions = 0;
    let mut w = World::new(&s, rules());
    while w.winner().is_none() && w.tick() < 30 * 60 * 15 {
        w.step();
        promotions += w.events().iter().filter(|e| matches!(e, Event::Promoted { .. })).count();
    }
    assert!(promotions > 0, "no unit was promoted in a whole match");
    let (end, replay) = record(World::new(&s, rules()), &s, w.tick(), |_| {});
    assert_eq!(end.state_hash(), w.state_hash());
    assert_eq!(verify_replay(&replay, crater_ridge(), rules()), Ok(w.state_hash()));
}
