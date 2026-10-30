//! Rally points (OpenRA's `RallyPoint`), driven through public commands only: units a building
//! trains walk to its rally point, collectors from a supply center still go gathering, and only
//! the owner of a building that trains units can set one.

use g2_rules::ids::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::world::{Event, INPUT_DELAY};
use g2_sim::{
    CommandKind, EntityId, Faction, GatherPhase, MatchSetup, Order, PlayerId, PlayerSetup, Reject, Vec2, World,
    verify_replay,
};

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
    };
    MatchSetup { map: crater_ridge(), seed: 1, players: vec![player(Faction::Eu), player(Faction::Gla)] }
}

fn first(w: &World, owner: PlayerId, def: g2_sim::DefId) -> Option<EntityId> {
    w.entities().find(|e| e.owner == Some(owner) && e.def == def).map(|e| e.id)
}

/// Rejections `kind` drew.
fn order(w: &mut World, kind: CommandKind) -> Vec<Reject> {
    w.submit(P0, kind);
    let mut out = Vec::new();
    for _ in 0..=INPUT_DELAY {
        w.step();
        out.extend(w.events().iter().filter_map(|e| match e {
            Event::Rejected { reason, .. } => Some(*reason),
            _ => None,
        }));
    }
    out
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

/// Builds `def` with the starting dozer at `pos` and runs until it stands finished.
fn build(w: &mut World, def: g2_sim::DefId, pos: Vec2) -> EntityId {
    let builder = first(w, P0, EU_DOZER).expect("dozer");
    assert_eq!(order(w, CommandKind::Build { builder, def, pos }), vec![]);
    let site = w.entities().find(|e| e.owner == Some(P0) && e.def == def).expect("foundation").id;
    step_until(w, 20_000, "construction", |w| w.entity(site).unwrap().is_complete());
    site
}

/// A unit trained after `SetRally` walks to the rally point and stops there; the rally is
/// the building's from then on, for every unit it trains.
#[test]
fn trained_units_walk_to_the_rally_point() {
    let s = setup();
    let mut w = World::new(&s, rules());
    let barracks = build(&mut w, EU_BARRACKS, Vec2::from_int(110, 110));
    let rally = Vec2::from_int(160, 150);
    assert_eq!(order(&mut w, CommandKind::SetRally { producer: barracks, at: rally }), vec![]);
    assert_eq!(w.entity(barracks).unwrap().rally, Some(rally));
    for _ in 0..2 {
        assert_eq!(order(&mut w, CommandKind::Train { producer: barracks, def: EU_RECON }), vec![]);
    }
    step_until(&mut w, 30 * 60, "two squads trained", |w| w.entities().filter(|e| e.def == EU_RECON).count() == 2);
    let squads: Vec<EntityId> = w.entities().filter(|e| e.def == EU_RECON).map(|e| e.id).collect();
    assert!(
        squads.iter().any(|&u| matches!(w.entity(u).unwrap().order, Order::Move { .. })),
        "a new squad sets off for the rally point"
    );
    step_until(&mut w, 30 * 60, "the squads arrive", |w| {
        squads.iter().all(|&u| w.entity(u).is_some_and(|e| e.order == Order::Idle))
    });
    for u in squads {
        let d = w.entity(u).unwrap().pos.distance(rally);
        assert!(d <= g2_sim::Fx::from_int(8), "squad stopped {d:?} from the rally point");
    }
    // Replays carry the command (tag 18) and re-simulate to the same state.
    let replay = w.replay(&s);
    let back = g2_sim::Replay::decode(&replay.encode()).unwrap();
    assert_eq!(back, replay);
    assert_eq!(verify_replay(&back, crater_ridge(), rules()), Ok(w.state_hash()));
}

/// A collector trained at a supply center with a rally point still goes to the nearest pile.
#[test]
fn collectors_keep_gathering_past_a_rally_point() {
    let mut w = World::new(&setup(), rules());
    let depot = build(&mut w, EU_SUPPLY, Vec2::from_int(110, 80));
    assert_eq!(order(&mut w, CommandKind::SetRally { producer: depot, at: Vec2::from_int(60, 200) }), vec![]);
    let before = w.entities().filter(|e| e.def == EU_HARVESTER).count();
    assert_eq!(order(&mut w, CommandKind::Train { producer: depot, def: EU_HARVESTER }), vec![]);
    step_until(&mut w, 30 * 60, "a collector", |w| w.entities().filter(|e| e.def == EU_HARVESTER).count() > before);
    let new = w.entities().filter(|e| e.def == EU_HARVESTER).last().unwrap();
    assert!(matches!(new.order, Order::Gather { phase: GatherPhase::ToPile, .. }), "{:?}", new.order);
}

/// Only a building of one's own that trains units takes a rally point: not a power plant, a
/// unit, or an enemy's barracks; a rejection changes nothing.
#[test]
fn set_rally_needs_an_own_producer() {
    let mut w = World::new(&setup(), rules());
    let power = build(&mut w, EU_POWER, Vec2::from_int(110, 110));
    let dozer = first(&w, P0, EU_DOZER).unwrap();
    let at = Vec2::from_int(150, 150);
    assert_eq!(order(&mut w, CommandKind::SetRally { producer: power, at }), vec![Reject::CannotProduce]);
    assert_eq!(order(&mut w, CommandKind::SetRally { producer: dozer, at }), vec![Reject::CannotProduce]);
    let enemy_cc = first(&w, P1, GLA_CC).unwrap();
    assert_eq!(order(&mut w, CommandKind::SetRally { producer: enemy_cc, at }), vec![Reject::NotOwner]);
    assert!(w.entities().all(|e| e.rally.is_none()), "no rally point was set");
}

/// The AI never sets rally points: a whole AI-against-AI match leaves every building without
/// one, so the AI plays exactly as before.
#[test]
fn the_ai_sets_no_rally_points() {
    let mut s = setup();
    for p in &mut s.players {
        p.ai = Some(g2_sim::AiLevel::Normal);
    }
    let mut w = World::new(&s, rules());
    for _ in 0..30 * 60 * 4 {
        w.step();
        assert!(w.entities().all(|e| e.rally.is_none()));
    }
}
