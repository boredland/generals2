//! Timed statuses, the powers that put them on (Bulwark, Thunderclap, Supply Drop, Faulty
//! Construction) and the unit abilities (Nitro, Radar Pulse, the self-destructs, the
//! Sunstorm's rod and parting blast), driven through public commands only.

use g2_rules::generals::{INFERNO, JUNKYARD, KITBASH, MERCURY_RED, RED_ARROW, TASKMASTER, THOUSAND_SUNS, THRAX};
use g2_rules::ids::*;
use g2_rules::perks::JY_FAULTY_CONSTRUCTION;
use g2_rules::{crater_ridge, rules};
use g2_sim::defs::{PerkId, PowerKind, StatusKind};
use g2_sim::world::{Event, INPUT_DELAY, ShellSource};
use g2_sim::{
    CommandKind, EntityId, Faction, Fx, GeneralId, MatchSetup, PlayerId, PlayerSetup, Reject, Vec2, World, record,
    verify_replay,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
/// Out in the middle of Crater Ridge, open ground.
const MEET: Vec2 = Vec2::from_int(200, 200);

fn setup(f0: Faction, general: GeneralId, rank: u8, perks: &[PerkId], f1: Faction) -> MatchSetup {
    let player = |faction, general, perks: &[PerkId], rank| PlayerSetup {
        faction,
        starting_credits: 100_000,
        ai: None,
        general,
        perks: perks.to_vec(),
        rank,
        team: 0,
    };
    MatchSetup {
        map: crater_ridge(),
        seed: 1,
        players: vec![player(f0, Some(general), perks, rank), player(f1, None, &[], 1)],
    }
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

/// Rejections `cmd` drew.
fn order(w: &mut World, who: PlayerId, cmd: CommandKind) -> Vec<Reject> {
    w.submit(who, cmd);
    let mut out = Vec::new();
    for _ in 0..=INPUT_DELAY {
        w.step();
        for e in w.events() {
            if let Event::Rejected { player, reason } = e
                && *player == who
            {
                out.push(*reason);
            }
        }
    }
    out
}

fn build(w: &mut World, who: PlayerId, def: g2_sim::DefId, pos: Vec2) -> EntityId {
    let builder = w.entities().find(|e| e.owner == Some(who) && w.rules.def(e.def).is_constructor()).unwrap().id;
    w.submit(who, CommandKind::Build { builder, def, pos });
    step_until(w, INPUT_DELAY + 2, "foundation", |w| w.entities().any(|e| e.owner == Some(who) && e.def == def));
    let site = w.entities().find(|e| e.owner == Some(who) && e.def == def).unwrap().id;
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

fn walk(w: &mut World, who: PlayerId, u: EntityId, to: Vec2) {
    w.submit(who, CommandKind::Move { units: vec![u], target: to });
    for _ in 0..=INPUT_DELAY {
        w.step();
    }
    step_until(w, 30 * 120, "arrival", |w| w.entity(u).is_none_or(|e| e.order == g2_sim::Order::Idle));
}

/// A factory of `who`'s at its start, toward the map centre.
fn factory(w: &mut World, who: PlayerId, def: g2_sim::DefId) -> EntityId {
    let home = w.map.spawns[who.0 as usize];
    let off = if who == P0 { Vec2::from_int(30, 0) } else { Vec2::from_int(-30, 0) };
    build(w, who, def, home + off)
}

/// The enemy's Aegis tank (heavy armour, a 40-damage gun) at `at`.
fn enemy_aegis(w: &mut World, at: Vec2) -> EntityId {
    let f = factory(w, P1, EU_FACTORY);
    let t = train(w, P1, f, EU_AEGIS);
    walk(w, P1, t, at);
    t
}

fn wait_power(w: &mut World, slot: u8) {
    let at = w.tick() + w.power_wait(P0, slot).unwrap();
    step_until(w, at - w.tick() + 1, "power ready", |w| w.power_wait(P0, slot) == Some(0));
}

fn status(w: &World, id: EntityId, kind: StatusKind) -> Option<g2_sim::entity::Status> {
    w.entity(id)?.statuses.iter().find(|s| s.kind == kind).copied()
}

/// Hits `target` took this step from `shooter`, by damage.
fn hits(w: &World, shooter: EntityId, target: EntityId) -> Vec<i32> {
    w.events()
        .iter()
        .filter_map(|e| match *e {
            Event::Hit { shooter: s, target: t, damage, .. } if s == shooter && t == target => Some(damage),
            _ => None,
        })
        .collect()
}

/// Nitro: a Scorpion's speed is half again as much for exactly 5 s from the tick the order
/// applies; the status is gone as its end tick begins, and the unit runs at its own speed
/// after. The ability then waits its recharge.
#[test]
fn nitro_lasts_exactly_its_ticks_and_speeds_the_tank_up() {
    let s = setup(Faction::Gla, KITBASH, 1, &[], Faction::Eu);
    let mut w = World::new(&s, rules());
    let f = factory(&mut w, P0, GLA_FACTORY);
    let tank = train(&mut w, P0, f, GLA_TANK);
    walk(&mut w, P0, tank, Vec2::from_int(150, 100));
    w.submit(P0, CommandKind::Move { units: vec![tank], target: Vec2::from_int(390, 100) });
    w.submit(P0, CommandKind::Ability { units: vec![tank] });
    let mut applied = None;
    for _ in 0..=INPUT_DELAY {
        w.step();
        if w.events().iter().any(|e| matches!(e, Event::AbilityUsed { unit, .. } if *unit == tank)) {
            applied = Some(w.tick() - 1);
        }
    }
    let applied = applied.expect("nitro used");
    let boost = status(&w, tank, StatusKind::Speed).expect("boosted");
    assert_eq!((boost.percent, boost.until), (150, applied + 5 * 30));
    let pace = |w: &mut World| {
        let x = w.entity(tank).unwrap().pos.x;
        w.step();
        w.entity(tank).unwrap().pos.x - x
    };
    let fast = pace(&mut w);
    step_until(&mut w, 30 * 10, "the last boosted tick", |w| w.tick() == boost.until - 1);
    assert!(status(&w, tank, StatusKind::Speed).is_some(), "still on the tick before its end");
    w.step();
    assert!(status(&w, tank, StatusKind::Speed).is_none(), "gone as its end tick begins");
    let normal = pace(&mut w);
    let own = rules().def(GLA_TANK).speed();
    // Its own speed again (a hair less where the straight leg is re-planned).
    assert!(normal <= own && normal.0 * 100 >= own.0 * 99, "{normal:?} against {own:?}");
    let near = |got: Fx, want: i64| (got.0 - want).abs() * 100 <= want;
    assert!(near(fast, own.0 * 3 / 2), "boosted {fast:?} against {own:?}");
    assert_eq!(order(&mut w, P0, CommandKind::Ability { units: vec![tank] }), vec![Reject::AbilityNotReady]);
}

/// Thunderclap: 5 s after the order, 100 damage and a 3 s stun (level 1) on enemy units
/// within 20; the stunned tank neither moves nor fires though it has an order and a target,
/// then does both again. The player's own unit in the blast is spared.
#[test]
fn thunderclap_stuns_enemies_and_spares_allies() {
    let s = setup(Faction::Apa, RED_ARROW, 1, &[], Faction::Eu);
    let mut w = World::new(&s, rules());
    let aegis = enemy_aegis(&mut w, MEET);
    let f = factory(&mut w, P0, APA_FACTORY);
    let ours = train(&mut w, P0, f, APA_WARLORD);
    walk(&mut w, P0, ours, MEET - Vec2::from_int(12, 0));
    wait_power(&mut w, 0);
    let hp = |w: &World, id| w.entity(id).map_or(0, |e| e.hp);
    let ours_before = hp(&w, ours);
    assert_eq!(order(&mut w, P0, CommandKind::Power { slot: 0, at: MEET }), vec![]);
    let mut struck = false;
    for _ in 0..30 * 6 {
        w.step();
        if w.events().iter().any(|e| matches!(e, Event::FieldLanded { player: P0, hostile: true, .. })) {
            struck = true;
            assert_eq!(hits(&w, g2_sim::world::POWER_SOURCE, aegis), vec![100]);
            break;
        }
    }
    assert!(struck, "the shockwave lands");
    let stun = status(&w, aegis, StatusKind::Stun).expect("stunned");
    assert_eq!(stun.until, w.tick() - 1 + 3 * 30);
    assert!(status(&w, ours, StatusKind::Stun).is_none(), "own unit not stunned");
    assert!(hp(&w, ours) >= ours_before - 40 * 4, "own unit not hit by the shockwave");
    // Ordered away while stunned: it stays, and its gun stays silent.
    w.submit(P1, CommandKind::Move { units: vec![aegis], target: MEET + Vec2::from_int(60, 0) });
    let at = w.entity(aegis).unwrap().pos;
    while w.tick() < stun.until {
        w.step();
        assert_eq!(w.entity(aegis).unwrap().pos, at, "a stunned unit doesn't move");
        assert!(hits(&w, aegis, ours).is_empty(), "a stunned unit doesn't fire");
    }
    step_until(&mut w, 30 * 3, "moving again", |w| w.entity(aegis).unwrap().pos != at);
}

/// Bulwark: own units within 25 take 75% damage for 10 s (level 1); the enemy beside them
/// takes full damage.
#[test]
fn bulwark_shields_own_units_only() {
    let s = setup(Faction::Eu, MERCURY_RED, 1, &[], Faction::Apa);
    let mut w = World::new(&s, rules());
    let f1 = factory(&mut w, P1, APA_FACTORY);
    let warlord = train(&mut w, P1, f1, APA_WARLORD);
    let f = factory(&mut w, P0, EU_FACTORY);
    let aegis = train(&mut w, P0, f, EU_AEGIS);
    // Both wait at home for the power, then meet.
    wait_power(&mut w, 0);
    walk(&mut w, P1, warlord, MEET);
    walk(&mut w, P0, aegis, MEET - Vec2::from_int(20, 0));
    assert_eq!(order(&mut w, P0, CommandKind::Power { slot: 0, at: MEET - Vec2::from_int(10, 0) }), vec![]);
    let shield = status(&w, aegis, StatusKind::Toughness).expect("shielded");
    assert_eq!(shield.percent, 75);
    assert!(status(&w, warlord, StatusKind::Toughness).is_none(), "the enemy is not shielded");
    let (mut taken, mut dealt) = (Vec::new(), Vec::new());
    while w.tick() < shield.until {
        w.step();
        taken.extend(hits(&w, warlord, aegis));
        dealt.extend(hits(&w, aegis, warlord));
    }
    assert!(!taken.is_empty() && taken.iter().all(|&d| d == 30), "40 at 75%: {taken:?}");
    assert!(!dealt.is_empty() && dealt.iter().all(|&d| d == 40), "the enemy takes full damage: {dealt:?}");
    let mut after = Vec::new();
    step_until(&mut w, 30 * 5, "a hit after", |w| {
        after.extend(hits(w, warlord, aegis));
        !after.is_empty()
    });
    assert_eq!(after[0], 40);
}

/// Supply Drop: 2 s after the order, own units within 30 gain a veterancy rank (with its extra
/// hit points) and deal 115% damage for 30 s (level 1).
#[test]
fn supply_drop_promotes_and_buffs_damage() {
    let s = setup(Faction::Apa, TASKMASTER, 1, &[], Faction::Eu);
    let mut w = World::new(&s, rules());
    let f1 = factory(&mut w, P1, EU_FACTORY);
    let aegis = train(&mut w, P1, f1, EU_AEGIS);
    let f = factory(&mut w, P0, APA_FACTORY);
    let warlord = train(&mut w, P0, f, APA_WARLORD);
    wait_power(&mut w, 0);
    walk(&mut w, P1, aegis, MEET);
    walk(&mut w, P0, warlord, MEET - Vec2::from_int(30, 0));
    assert_eq!(w.rank(w.entity(warlord).unwrap()), 0);
    let at = w.entity(warlord).unwrap().pos;
    assert_eq!(order(&mut w, P0, CommandKind::Power { slot: 0, at }), vec![]);
    step_until(&mut w, 30 * 3, "the crate lands", |w| status(w, warlord, StatusKind::Damage).is_some());
    let buff = status(&w, warlord, StatusKind::Damage).unwrap();
    assert_eq!(buff.percent, 115);
    assert_eq!(w.rank(w.entity(warlord).unwrap()), 1, "one rank up");
    assert!(status(&w, aegis, StatusKind::Damage).is_none(), "nothing for the enemy");
    let mut dealt = Vec::new();
    step_until(&mut w, 30 * 20, "a buffed hit", |w| {
        dealt.extend(hits(w, warlord, aegis));
        !dealt.is_empty()
    });
    assert_eq!(dealt[0], 46, "40 at 115%");
}

/// Faulty Construction (Junkyard's power perk, its fifth slot after the GLA's three): enemy
/// vehicles within 30 drive at 60% and hit at 75% for 15 s; infantry beside them are spared.
#[test]
fn faulty_construction_slows_and_weakens_enemy_vehicles() {
    let s = setup(Faction::Gla, JUNKYARD, 4, &[JY_FAULTY_CONSTRUCTION], Faction::Eu);
    let mut w = World::new(&s, rules());
    let slot = 4;
    assert_eq!(w.power_in(P0, slot).map(|p| p.name), Some("Faulty Construction"));
    let aegis = enemy_aegis(&mut w, MEET);
    let b1 = build(&mut w, P1, EU_BARRACKS, s.map.spawns[1] + Vec2::from_int(0, -30));
    let squad = train(&mut w, P1, b1, EU_RECON);
    walk(&mut w, P1, squad, MEET + Vec2::from_int(0, 8));
    let worker = w.entities().find(|e| e.owner == Some(P0) && e.def == GLA_WORKER).unwrap().id;
    walk(&mut w, P0, worker, MEET - Vec2::from_int(22, 0));
    wait_power(&mut w, slot);
    assert_eq!(order(&mut w, P0, CommandKind::Power { slot, at: MEET }), vec![]);
    step_until(&mut w, 30 * 2, "the field lands", |w| status(w, aegis, StatusKind::Speed).is_some());
    assert_eq!(status(&w, aegis, StatusKind::Speed).map(|s| s.percent), Some(60));
    assert_eq!(status(&w, aegis, StatusKind::Damage).map(|s| s.percent), Some(75));
    assert!(w.entity(squad).unwrap().statuses.is_empty(), "infantry are spared");
    w.submit(P1, CommandKind::Move { units: vec![aegis], target: MEET + Vec2::from_int(80, 0) });
    for _ in 0..=INPUT_DELAY + 5 {
        w.step();
    }
    let x = w.entity(aegis).unwrap().pos.x;
    w.step();
    let pace = w.entity(aegis).unwrap().pos.x - x;
    let own = rules().def(EU_AEGIS).speed();
    // Within 1%: the step toward a waypoint rounds in fixed point.
    assert!((pace.0 - own.0 * 60 / 100).abs() * 100 <= own.0 * 60 / 100, "{pace:?} against {own:?}");
}

/// Detonate: the Bomb Truck blows up where it stands on command; on the next tick its blast
/// hits the enemy tank within 13 for 185 (+85 against heavy armour) and spares the player's
/// own unit beside it.
#[test]
fn a_bomb_truck_detonates_hitting_enemies_only() {
    let s = setup(Faction::Gla, THRAX, 1, &[], Faction::Eu);
    let mut w = World::new(&s, rules());
    let aegis = enemy_aegis(&mut w, MEET);
    let f = factory(&mut w, P0, GLA_FACTORY);
    let truck = train(&mut w, P0, f, GLA_BOMB_TRUCK);
    let ally = train(&mut w, P0, f, GLA_TOXIN_TANK);
    walk(&mut w, P0, ally, MEET - Vec2::from_int(9, 0));
    walk(&mut w, P0, truck, MEET - Vec2::from_int(6, 4));
    w.submit(P0, CommandKind::Ability { units: vec![truck] });
    let mut blast = None;
    step_until(&mut w, 30, "the blast", |w| {
        blast = w.events().iter().find_map(|e| match *e {
            Event::Landed { at, radius, source: ShellSource::Blast { def } } => Some((at, radius, def)),
            _ => None,
        });
        blast.is_some()
    });
    assert!(w.entity(truck).is_none(), "the truck is gone");
    let (_, radius, def) = blast.unwrap();
    assert_eq!((radius, def), (Fx::from_int(13), GLA_BOMB_TRUCK));
    assert_eq!(hits(&w, truck, aegis), vec![185 + 85], "the enemy tank takes the blast, +85 vs heavy");
    assert!(hits(&w, truck, ally).is_empty(), "the ally beside it is spared");
}

/// Driven at the enemy, the Bomb Truck's charge is its gun: the first shot destroys the
/// truck and the blast deals the damage. Shot down instead, it goes off all the same.
#[test]
fn a_bomb_truck_blows_its_charge_by_attacking_or_dying() {
    let s = setup(Faction::Gla, THRAX, 1, &[], Faction::Eu);
    let mut w = World::new(&s, rules());
    let aegis = enemy_aegis(&mut w, MEET);
    let f = factory(&mut w, P0, GLA_FACTORY);
    let truck = train(&mut w, P0, f, GLA_BOMB_TRUCK);
    walk(&mut w, P0, truck, MEET - Vec2::from_int(25, 0));
    assert_eq!(order(&mut w, P0, CommandKind::Attack { units: vec![truck], target: aegis }), vec![]);
    let mut blew = false;
    step_until(&mut w, 30 * 20, "the truck goes off", |w| {
        blew |= !hits(w, truck, aegis).is_empty();
        blew
    });
    assert!(w.entity(truck).is_none());
    assert_eq!(w.stats(P0).units_lost, 1);

    // A second truck left standing near the tank: the tank shoots it, and it goes off.
    let truck = train(&mut w, P0, f, GLA_BOMB_TRUCK);
    let at = w.entity(aegis).map_or(MEET, |e| e.pos);
    walk(&mut w, P0, truck, at - Vec2::from_int(10, 10));
    let mut blasted = false;
    step_until(&mut w, 30 * 30, "shot down, it goes off", |w| {
        blasted |= w
            .events()
            .iter()
            .any(|e| matches!(e, Event::Landed { source: ShellSource::Blast { def: GLA_BOMB_TRUCK }, .. }));
        blasted
    });
}

/// Ignition: the Dragon Tank blows up on command, 50 within 20 at once, then its fire burns
/// 10 a second for 5 s on enemies in it.
#[test]
fn a_dragon_tank_ignites_into_a_firestorm() {
    let s = setup(Faction::Apa, INFERNO, 1, &[], Faction::Eu);
    let mut w = World::new(&s, rules());
    let aegis = enemy_aegis(&mut w, MEET);
    let f = factory(&mut w, P0, APA_FACTORY);
    let dragon = train(&mut w, P0, f, APA_DRAGON);
    walk(&mut w, P0, dragon, MEET - Vec2::from_int(15, 0));
    assert_eq!(order(&mut w, P0, CommandKind::Ability { units: vec![dragon] }), vec![]);
    assert!(w.entity(dragon).is_none() || w.entity(dragon).unwrap().hp <= 0);
    let mut burned = Vec::new();
    for _ in 0..30 * 7 {
        w.step();
        burned.extend(hits(&w, dragon, aegis));
        burned.extend(hits(&w, g2_sim::world::POWER_SOURCE, aegis));
    }
    assert_eq!(burned.first(), Some(&50), "{burned:?}");
    assert_eq!(burned.iter().filter(|&&d| d == 10).count(), 5, "{burned:?}");
}

/// Radar Pulse: the Longbow sees half as far again for 10 s, then as before.
#[test]
fn radar_pulse_stretches_sight_for_a_while() {
    let s = setup(Faction::Eu, MERCURY_RED, 1, &[], Faction::Apa);
    let mut w = World::new(&s, rules());
    let air = build(&mut w, P0, EU_AIRFIELD, s.map.spawns[0] + Vec2::from_int(0, 40));
    let heli = train(&mut w, P0, air, EU_LONGBOW);
    walk(&mut w, P0, heli, MEET);
    let sight = g2_sim::fog::sight(rules(), EU_LONGBOW, true, Fx::from_int(15));
    let far = MEET + Vec2::new(sight + Fx::from_int(12), Fx::ZERO);
    assert!(!w.vision(P0).is_visible(far));
    assert_eq!(order(&mut w, P0, CommandKind::Ability { units: vec![heli] }), vec![]);
    let pulse = status(&w, heli, StatusKind::Sight).expect("pulsing");
    assert_eq!(pulse.percent, 150);
    assert!(w.vision(P0).is_visible(far), "sees further");
    step_until(&mut w, 30 * 12, "the pulse ends", |w| w.tick() >= pulse.until);
    assert!(!w.vision(P0).is_visible(far), "back to its own sight");
}

/// The Sunstorm's Uranium Pile Eject bursts where the target stood (100, +75 against heavy
/// armour) and leaves a radiation cloud; the Sunstorm itself, destroyed, leaves its own.
#[test]
fn the_sunstorm_ejects_a_rod_and_breaches_on_death() {
    let s = setup(Faction::Apa, THOUSAND_SUNS, 1, &[], Faction::Eu);
    let mut w = World::new(&s, rules());
    let aegis = enemy_aegis(&mut w, MEET);
    let f = factory(&mut w, P0, APA_FACTORY);
    let sun = train(&mut w, P0, f, APA_SUNSTORM);
    walk(&mut w, P0, sun, MEET - Vec2::from_int(45, 0));
    let breached = |w: &World| {
        w.events().iter().any(|e| matches!(e, Event::Landed { source: ShellSource::Blast { def: APA_SUNSTORM }, .. }))
    };
    let mut breach = false;
    assert_eq!(order(&mut w, P0, CommandKind::Special { units: vec![sun], target: aegis }), vec![]);
    let mut rod = Vec::new();
    step_until(&mut w, 30 * 10, "the rod lands", |w| {
        rod.extend(hits(w, sun, aegis));
        breach |= breached(w);
        !rod.is_empty()
    });
    assert!(rod.contains(&175), "{rod:?}");
    assert!(!w.clouds().is_empty(), "the rod leaves radiation");
    // The tank and three more finish the Sunstorm; it breaches.
    let f1 = factory(&mut w, P1, EU_FACTORY);
    breach |= breached(&w);
    if let Some(at) = w.entity(sun).map(|e| e.pos) {
        let tanks: Vec<_> = (0..3).map(|_| train(&mut w, P1, f1, EU_AEGIS)).collect();
        w.submit(P1, CommandKind::AttackMove { units: tanks, target: at });
    }
    step_until(&mut w, 30 * 180, "the breach", |w| {
        breach |= breached(w);
        breach
    });
}

/// A match with statuses and abilities in play replays exactly: the `Ability` command goes
/// through the replay format, and statuses and pending blasts are in the state hash.
#[test]
fn statuses_and_abilities_replay_exactly() {
    let s = setup(Faction::Apa, RED_ARROW, 1, &[], Faction::Eu);
    let mut w = World::new(&s, rules());
    let aegis = enemy_aegis(&mut w, MEET);
    let f = factory(&mut w, P0, APA_FACTORY);
    let warlord = train(&mut w, P0, f, APA_WARLORD);
    walk(&mut w, P0, warlord, MEET - Vec2::from_int(15, 0));
    let ready = w.tick() + w.power_wait(P0, 0).unwrap();
    let end = ready + 30 * 8;
    let (done, replay) = record(w, &s, end, |w| {
        if w.tick() == ready {
            w.submit(P0, CommandKind::Power { slot: 0, at: MEET });
            w.submit(P0, CommandKind::Ability { units: vec![warlord] });
        }
    });
    assert!(done.entity(aegis).is_none_or(|e| e.hp > 0));
    let back = g2_sim::Replay::decode(&replay.encode()).unwrap();
    assert_eq!(back, replay);
    assert!(back.commands.iter().any(|c| matches!(c.kind, CommandKind::Ability { .. })));
    assert_eq!(verify_replay(&back, crater_ridge(), rules()), Ok(done.state_hash()));
}

/// The new area powers are each general's own kind at every level, and level 3 is the
/// strongest: the stun, the shield and the buff grow.
#[test]
fn the_new_powers_grow_with_rank() {
    let field = |g, rank| match World::new(&setup(rules().general(g).faction, g, rank, &[], Faction::Eu), rules())
        .power_in(P0, 0)
        .unwrap()
        .kind
    {
        PowerKind::Field(f) => f,
        k => panic!("{k:?}"),
    };
    assert_eq!([1, 3, 5].map(|r| field(RED_ARROW, r).ticks), [90, 120, 150]);
    assert_eq!([1, 3, 5].map(|r| field(MERCURY_RED, r).allies[0].unwrap().1), [75, 70, 65]);
    assert_eq!([1, 3, 5].map(|r| field(TASKMASTER, r).radius), [30, 40, 40].map(Fx::from_int));
}
