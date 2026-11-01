//! The GLA tunnel network, through public commands only: infantry that go in at one tunnel
//! come out at any other of their player's, the network holds ten, units inside heal, and
//! they live on while one tunnel stands and die with the last.

use g2_rules::ids::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::command::Reject;
use g2_sim::world::{Event, INPUT_DELAY};
use g2_sim::{CommandKind, EntityId, Faction, MatchSetup, Order, PlayerId, PlayerSetup, Vec2, World};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn setup() -> MatchSetup {
    let player = |faction| PlayerSetup {
        faction,
        starting_credits: 100_000,
        ai: None,
        general: None,
        perks: Vec::new(),
        rank: 1,
        team: 0,
    };
    MatchSetup { map: crater_ridge(), seed: 1, players: vec![player(Faction::Gla), player(Faction::Eu)] }
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

fn build(w: &mut World, who: PlayerId, def: g2_sim::DefId, pos: Vec2) -> EntityId {
    let builder = w.entities().find(|e| e.owner == Some(who) && w.rules.def(e.def).is_constructor()).unwrap().id;
    let before: Vec<_> = w.entities().filter(|e| e.def == def).map(|e| e.id).collect();
    w.submit(who, CommandKind::Build { builder, def, pos });
    let new = |w: &World| w.entities().find(|e| e.def == def && !before.contains(&e.id)).map(|e| e.id);
    step_until(w, INPUT_DELAY + 2, "foundation", |w| new(w).is_some());
    let site = new(w).unwrap();
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

fn get_in(w: &mut World, units: &[EntityId], tunnel: EntityId) {
    assert_eq!(order(w, P0, CommandKind::Garrison { units: units.to_vec(), container: tunnel }), vec![]);
    step_until(w, 30 * 90, "getting in", |w| {
        units.iter().all(|&u| w.entity(u).is_none_or(|e| e.inside.is_some() || e.order == Order::Idle))
    });
}

/// `units` of the EU player's walk up to `target` and attack it: the order needs it in sight.
fn assault(w: &mut World, units: &[EntityId], target: EntityId) {
    let at = w.entity(target).unwrap().pos + Vec2::from_int(0, 18);
    w.submit(P1, CommandKind::Move { units: units.to_vec(), target: at });
    for _ in 0..=INPUT_DELAY {
        w.step();
    }
    step_until(w, 30 * 90, "arrival", |w| units.iter().all(|&u| w.entity(u).is_none_or(|e| e.order == Order::Idle)));
    // Idle guards may already have opened fire on it, or even destroyed it.
    if w.entity(target).is_some() {
        assert_eq!(order(w, P1, CommandKind::Attack { units: units.to_vec(), target }), vec![]);
    }
}

/// Two tunnels of the GLA player's, far apart, and its barracks.
fn network(w: &mut World) -> (EntityId, EntityId, EntityId) {
    let barracks = build(w, P0, GLA_BARRACKS, Vec2::from_int(70, 110));
    let a = build(w, P0, GLA_TUNNEL, Vec2::from_int(110, 70));
    let b = build(w, P0, GLA_TUNNEL, Vec2::from_int(200, 190));
    (barracks, a, b)
}

#[test]
fn units_entering_one_tunnel_leave_by_another() {
    let mut w = World::new(&setup(), rules());
    let (barracks, a, b) = network(&mut w);
    let squads: Vec<EntityId> = (0..3).map(|_| train(&mut w, P0, barracks, GLA_RABBLE)).collect();
    get_in(&mut w, &squads, a);
    assert!(squads.iter().all(|&s| w.entity(s).unwrap().inside == Some(a)), "all in by tunnel A");
    assert_eq!(w.occupants(b).len(), 3, "the far tunnel holds the same three");
    assert_eq!(order(&mut w, P0, CommandKind::Evacuate { container: b }), vec![]);
    let bpos = w.entity(b).unwrap().pos;
    for &s in &squads {
        let e = w.entity(s).unwrap();
        assert!(e.inside.is_none(), "out");
        assert!(e.pos.distance(bpos) < g2_sim::Fx::from_int(20), "out at tunnel B, not A: {:?}", e.pos);
    }
    assert!(w.occupants(a).is_empty());
}

#[test]
fn the_network_holds_ten_and_an_unfinished_tunnel_takes_nobody() {
    let mut w = World::new(&setup(), rules());
    assert_eq!(rules().def(GLA_TUNNEL).garrison.unwrap().capacity, 10);
    let (barracks, a, b) = network(&mut w);
    let squads: Vec<EntityId> = (0..11).map(|_| train(&mut w, P0, barracks, GLA_RABBLE)).collect();
    get_in(&mut w, &squads[..6], a);
    get_in(&mut w, &squads[6..], b);
    let inside = squads.iter().filter(|&&s| w.entity(s).unwrap().inside.is_some()).count();
    assert_eq!(inside, 10, "ten fit in the whole network, whichever tunnel they use");
    assert_eq!(w.garrison_room(P0, a), Some(0));
    assert_eq!(w.garrison_room(P0, b), Some(0));
    // An enemy can't use our network.
    assert_eq!(w.garrison_room(P1, a), None);
}

#[test]
fn units_inside_heal_to_full_within_five_seconds() {
    let mut w = World::new(&setup(), rules());
    let (barracks, a, _) = network(&mut w);
    let rabble = train(&mut w, P0, barracks, GLA_RABBLE);
    // An EU squad wounds the rabble, then it ducks into the tunnel.
    let eu_barracks = build(&mut w, P1, EU_BARRACKS, Vec2::from_int(150, 90));
    let shooter = train(&mut w, P1, eu_barracks, EU_RECON);
    assault(&mut w, &[shooter], rabble);
    step_until(&mut w, 30 * 60, "wounded", |w| w.entity(rabble).is_some_and(|e| e.hp < rules().def(GLA_RABBLE).hp / 2));
    w.submit(P1, CommandKind::Stop { units: vec![shooter] });
    w.submit(P0, CommandKind::Stop { units: vec![rabble] });
    get_in(&mut w, &[rabble], a);
    assert_eq!(w.entity(rabble).unwrap().inside, Some(a));
    let max = w.max_hp(w.entity(rabble).unwrap());
    step_until(&mut w, 30 * 6, "healed", |w| w.entity(rabble).unwrap().hp >= max);
}

#[test]
fn units_inside_survive_a_tunnel_and_die_with_the_last() {
    let mut w = World::new(&setup(), rules());
    let (barracks, a, b) = network(&mut w);
    let squads: Vec<EntityId> = (0..2).map(|_| train(&mut w, P0, barracks, GLA_RABBLE)).collect();
    // The tanks pass the GLA base on the way and may raze it; a barracks away from the fight
    // keeps the GLA in the game (a player goes with their last base building).
    build(&mut w, P0, GLA_BARRACKS, Vec2::from_int(40, 300));
    get_in(&mut w, &squads, a);
    let factory = build(&mut w, P1, EU_FACTORY, Vec2::from_int(150, 120));
    let tanks: Vec<EntityId> = (0..5).map(|_| train(&mut w, P1, factory, EU_AEGIS)).collect();
    assault(&mut w, &tanks, a);
    step_until(&mut w, 30 * 240, "tunnel A falls", |w| w.entity(a).is_none());
    for &s in &squads {
        let e = w.entity(s).expect("alive after the first tunnel falls");
        assert_eq!(e.inside, Some(b), "moved over to the standing tunnel");
    }
    assault(&mut w, &tanks, b);
    step_until(&mut w, 30 * 240, "tunnel B falls", |w| w.entity(b).is_none());
    assert!(squads.iter().all(|&s| w.entity(s).is_none()), "everyone inside dies with the last tunnel");
}
