//! Artillery: shells with travel time and splash, and the Thunderfist's deploy mode.

use g2_rules::ids::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::entity::DeployState;
use g2_sim::{
    CommandKind, EntityId, Faction, MatchSetup, Order, PlayerId, PlayerSetup, Reject, Vec2, World, world::Event,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn setup(f0: Faction, f1: Faction, ai: [bool; 2]) -> MatchSetup {
    MatchSetup {
        map: crater_ridge(),
        seed: 1,
        players: [(f0, ai[0]), (f1, ai[1])]
            .map(|(faction, ai)| PlayerSetup {
                faction,
                starting_credits: 100_000,
                ai: ai.then_some(g2_sim::AiLevel::Easy),
                general: None,
            })
            .to_vec(),
    }
}

fn run(w: &mut World, ticks: u32) {
    for _ in 0..ticks {
        w.step();
    }
}

fn first(w: &World, owner: PlayerId, def: g2_sim::DefId) -> Option<EntityId> {
    w.entities().find(|e| e.owner == Some(owner) && e.def == def).map(|e| e.id)
}

fn order(w: &mut World, player: PlayerId, kind: CommandKind) -> Vec<Reject> {
    w.submit(player, kind);
    let mut out = Vec::new();
    for _ in 0..=g2_sim::world::INPUT_DELAY {
        w.step();
        out.extend(w.events().iter().filter_map(|e| match e {
            Event::Rejected { player: p, reason } if *p == player => Some(*reason),
            _ => None,
        }));
    }
    out
}

/// Builds `factory` for `player` with its starting constructor at `at`, then trains `unit`.
fn train(
    w: &mut World,
    player: PlayerId,
    ctor: g2_sim::DefId,
    factory: g2_sim::DefId,
    at: Vec2,
    unit: g2_sim::DefId,
) -> EntityId {
    let builder = first(w, player, ctor).unwrap();
    assert_eq!(order(w, player, CommandKind::Build { builder, def: factory, pos: at }), vec![]);
    for _ in 0..30 * 120 {
        if first(w, player, factory).is_some_and(|f| w.entity(f).unwrap().is_complete()) {
            break;
        }
        w.step();
    }
    let f = first(w, player, factory).expect("factory placed");
    assert_eq!(order(w, player, CommandKind::Train { producer: f, def: unit }), vec![]);
    for _ in 0..30 * 90 {
        if let Some(u) = first(w, player, unit) {
            return u;
        }
        w.step();
    }
    panic!("{} never trained", rules().def(unit).name);
}

/// Places `unit` of `player` at `at` by walking it there.
fn walk(w: &mut World, player: PlayerId, unit: EntityId, at: Vec2) {
    assert_eq!(order(w, player, CommandKind::Move { units: vec![unit], target: at }), vec![]);
    for _ in 0..30 * 120 {
        if w.entity(unit).unwrap().order == Order::Idle {
            return;
        }
        w.step();
    }
    panic!("never arrived");
}

/// GLA Toxin Artillery (fires on the move) against EU units at a test range; returns the world,
/// the gun and the enemy's starting dozer and a second enemy unit.
fn duel() -> (World, EntityId, EntityId, EntityId) {
    let mut w = World::new(&setup(Faction::Gla, Faction::Eu, [false, false]), rules());
    let gun = train(&mut w, P0, GLA_WORKER, GLA_FACTORY, Vec2::from_int(110, 110), GLA_TOXIN_ARTILLERY);
    let aegis = train(&mut w, P1, EU_DOZER, EU_FACTORY, Vec2::from_int(290, 290), EU_AEGIS);
    let dozer = first(&w, P1, EU_DOZER).unwrap();
    // Hold fire while placing: the gun walks to the meeting point, the enemies park far off.
    walk(&mut w, P0, gun, Vec2::from_int(150, 200));
    walk(&mut w, P1, dozer, Vec2::from_int(300, 200));
    walk(&mut w, P1, aegis, Vec2::from_int(300, 210));
    (w, gun, dozer, aegis)
}

fn hits_on(w: &World, target: EntityId) -> i32 {
    w.events()
        .iter()
        .map(|e| if let Event::Hit { target: t, damage, .. } = *e { if t == target { damage } else { 0 } } else { 0 })
        .sum()
}

/// A shell flies for distance / speed and lands on the spot the target stood on when it fired:
/// a target standing still is hit then, one that moved away in the meantime is missed.
#[test]
fn a_shell_lands_on_the_spot_and_misses_a_target_that_moved() {
    let shell = rules().def(GLA_TOXIN_ARTILLERY).weapon().unwrap().shell.expect("a shell weapon");
    for dodge in [false, true] {
        let (mut w, gun, dozer, _) = duel();
        // The dozer 45 from the gun: inside range 55, outside the minimum range, in sight.
        walk(&mut w, P1, dozer, Vec2::from_int(195, 200));
        let spot = w.entity(dozer).unwrap().pos;
        assert_eq!(order(&mut w, P0, CommandKind::Attack { units: vec![gun], target: dozer }), vec![]);
        let mut fired = None;
        for _ in 0..30 * 10 {
            w.step();
            if let Some(&Event::Fired { at, lands, .. }) = w.events().iter().find(|e| matches!(e, Event::Fired { .. }))
            {
                fired = Some((at, lands, w.tick()));
                break;
            }
        }
        let (at, lands, fired_on) = fired.expect("the gun fired");
        assert_eq!(at, spot, "aimed at where the dozer stands");
        let flight = (w.entity(gun).unwrap().pos.distance(at) / shell.speed).floor_int() as u32;
        assert_eq!(lands, fired_on - 1 + flight.max(1), "flight time is distance over speed");
        if dodge {
            // Drive off straight away; a dozer covers more than the blast radius in the flight.
            w.submit(P1, CommandKind::Move { units: vec![dozer], target: Vec2::from_int(195, 260) });
        }
        let hp = w.entity(dozer).unwrap().hp;
        let mut taken = 0;
        while w.tick() <= lands {
            w.step();
            taken += hits_on(&w, dozer);
        }
        if dodge {
            assert_eq!(taken, 0, "moved out of the blast: missed");
            assert!(w.entity(dozer).unwrap().hp == hp);
        } else {
            assert_eq!(
                taken,
                rules().def(GLA_TOXIN_ARTILLERY).weapon().unwrap().damage_vs(rules().def(EU_DOZER).armour)
            );
        }
    }
}

/// A shell's blast hurts every enemy within its radius of the impact point, and none of the
/// shooter's own units, even right next to the target. Two unarmed enemies (a dozer and a
/// harvester, so nobody shoots back) stand touching, our worker beside them, and the idle gun
/// fires at them as a guard.
#[test]
fn splash_hits_every_enemy_in_the_blast_and_no_friend() {
    let mut w = World::new(&setup(Faction::Gla, Faction::Eu, [false, false]), rules());
    let gun = train(&mut w, P0, GLA_WORKER, GLA_FACTORY, Vec2::from_int(110, 110), GLA_TOXIN_ARTILLERY);
    let aegis = train(&mut w, P1, EU_DOZER, EU_SUPPLY, Vec2::from_int(290, 290), EU_HARVESTER);
    let (dozer, worker) = (first(&w, P1, EU_DOZER).unwrap(), first(&w, P0, GLA_WORKER).unwrap());
    // Everyone in place before the gun comes within range.
    walk(&mut w, P1, dozer, Vec2::from_int(195, 200));
    walk(&mut w, P1, aegis, Vec2::from_int(195, 205));
    walk(&mut w, P0, worker, Vec2::from_int(192, 196));
    w.submit(P0, CommandKind::Move { units: vec![gun], target: Vec2::from_int(150, 202) });
    let blast = rules().def(GLA_TOXIN_ARTILLERY).weapon().unwrap().shell.unwrap().radius;
    let mut caught_friend = false;
    for _ in 0..30 * 60 {
        w.step();
        let Some(&Event::Landed { at, .. }) = w.events().iter().find(|e| matches!(e, Event::Landed { .. })) else {
            continue;
        };
        let hit: Vec<EntityId> = w
            .events()
            .iter()
            .filter_map(|e| if let Event::Hit { target, .. } = *e { Some(target) } else { None })
            .collect();
        let inside =
            |id: EntityId| w.entity(id).is_some_and(|e| e.pos.distance(at) - rules().def(e.def).radius <= blast);
        for id in [dozer, aegis] {
            assert_eq!(hit.contains(&id), inside(id), "an enemy is hit exactly when it is in the blast");
        }
        assert!(!hit.contains(&worker), "the gun's own worker is never hit");
        caught_friend |= inside(worker);
        if hit.contains(&dozer) && hit.contains(&aegis) && caught_friend {
            return;
        }
    }
    panic!("no shell caught both enemies with our worker in a blast (worker in a blast: {caught_friend})");
}

/// A shell weapon does not fire at a target inside its minimum range.
#[test]
fn artillery_holds_fire_inside_its_minimum_range() {
    let (mut w, gun, dozer, _) = duel();
    let min = rules().def(GLA_TOXIN_ARTILLERY).weapon().unwrap().shell.unwrap().min_range;
    assert!(min > g2_sim::Fx::from_int(5));
    walk(&mut w, P1, dozer, Vec2::from_int(158, 200));
    // Pin the gun: an attack order would chase, so keep it deployed-like by checking for shots
    // only until it has moved. It must not fire while the dozer's edge is inside `min`.
    assert_eq!(order(&mut w, P0, CommandKind::Attack { units: vec![gun], target: dozer }), vec![]);
    for _ in 0..30 * 5 {
        w.step();
        let (g, d) = (w.entity(gun).unwrap().pos, w.entity(dozer).unwrap().pos);
        let gap = g.distance(d) - rules().def(EU_DOZER).radius;
        if w.events().iter().any(|e| matches!(e, Event::Fired { .. })) {
            assert!(gap >= min, "fired at a target {gap:?} away, minimum {min:?}");
        }
    }
}

/// The Thunderfist moves with its cannon and must deploy for the howitzer: setting up takes the
/// card time, a deployed Thunderfist can't move until it packs up again, and only deployed
/// does it fire shells.
#[test]
fn the_thunderfist_deploys_for_its_howitzer_and_packs_up_to_move() {
    let mut w = World::new(&setup(Faction::Apa, Faction::Eu, [false, false]), rules());
    let tf = train(&mut w, P0, APA_CRANE, APA_FACTORY, Vec2::from_int(110, 110), APA_THUNDERFIST);
    let dep = rules().def(APA_THUNDERFIST).deploy.expect("deployable");
    walk(&mut w, P0, tf, Vec2::from_int(150, 200));
    assert_eq!(order(&mut w, P0, CommandKind::Deploy { units: vec![tf], deployed: true }), vec![]);
    assert!(matches!(w.entity(tf).unwrap().deploy, DeployState::Deploying(_)));
    run(&mut w, dep.ticks);
    assert_eq!(w.entity(tf).unwrap().deploy, DeployState::Deployed);
    // A move order packs it up first: no movement until packed.
    let at = w.entity(tf).unwrap().pos;
    assert_eq!(order(&mut w, P0, CommandKind::Move { units: vec![tf], target: Vec2::from_int(150, 260) }), vec![]);
    run(&mut w, dep.ticks - 3);
    assert_eq!(w.entity(tf).unwrap().pos, at, "still packing up");
    run(&mut w, 30 * 3);
    assert_eq!(w.entity(tf).unwrap().deploy, DeployState::Packed);
    assert!(w.entity(tf).unwrap().pos != at, "moving once packed");
    // Units that can't deploy are rejected.
    let crane = first(&w, P0, APA_CRANE).unwrap();
    assert_eq!(
        order(&mut w, P0, CommandKind::Deploy { units: vec![crane], deployed: true }),
        vec![Reject::CannotDeploy]
    );
}

/// Packed, the Thunderfist shoots its cannon (instant, no shells); given a target beyond the
/// cannon but within the howitzer's reach, it stops, deploys and shells it, whether ordered to
/// attack it or attack-moving past it. It sees as far as the howitzer reaches, so it can.
#[test]
fn the_thunderfist_deploys_on_its_own_to_reach_a_far_target() {
    for attack_move in [false, true] {
        let mut w = World::new(&setup(Faction::Apa, Faction::Eu, [false, false]), rules());
        let tf = train(&mut w, P0, APA_CRANE, APA_FACTORY, Vec2::from_int(110, 110), APA_THUNDERFIST);
        walk(&mut w, P0, tf, Vec2::from_int(150, 200));
        let dozer = first(&w, P1, EU_DOZER).unwrap();
        // About 55 away: beyond the cannon (30, plus 15 acquisition), inside the howitzer (65).
        walk(&mut w, P1, dozer, Vec2::from_int(207, 200));
        let cmd = if attack_move {
            CommandKind::AttackMove { units: vec![tf], target: Vec2::from_int(150, 230) }
        } else {
            CommandKind::Attack { units: vec![tf], target: dozer }
        };
        assert_eq!(order(&mut w, P0, cmd), vec![]);
        let (mut cannon_shots, mut shells) = (0, 0);
        for _ in 0..30 * 15 {
            w.step();
            for e in w.events() {
                match e {
                    Event::Fired { shooter, .. } if *shooter == tf => shells += 1,
                    Event::Hit { shooter, .. } if *shooter == tf => cannon_shots += 1,
                    _ => {}
                }
            }
            if shells > 0 {
                break;
            }
        }
        assert_eq!(w.entity(tf).unwrap().deploy, DeployState::Deployed, "set up (attack-move {attack_move})");
        assert_eq!(cannon_shots, 0, "beyond the cannon's range: no cannon shot");
        assert!(shells > 0, "the howitzer fired (attack-move {attack_move})");
    }
}

/// A whole AI match with artillery on both sides replays exactly: shells in flight and deploy
/// states are part of the state hash.
#[test]
fn artillery_matches_replay_exactly() {
    for (a, b) in [(Faction::Apa, Faction::Gla), (Faction::Eu, Faction::Apa)] {
        let s = setup(a, b, [true, true]);
        let mut w = World::new(&s, rules());
        let mut fired = 0;
        while w.winner().is_none() && w.tick() < 30 * 60 * 15 {
            w.step();
            fired += w.events().iter().filter(|e| matches!(e, Event::Fired { .. })).count();
        }
        assert!(fired > 0, "{a:?} vs {b:?}: no artillery fired");
        let replay = w.replay(&s);
        assert_eq!(g2_sim::verify_replay(&replay, crater_ridge(), rules()), Ok(w.state_hash()));
    }
}
