//! Economy behaviour, driven through public commands only.

use g2_rules::ids::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::{
    BuildingRole, Class, CommandKind, EntityId, Faction, Fx, MatchSetup, Order, PlayerId, PlayerSetup, Reject, Vec2,
    World, world::Event,
};

const P0: PlayerId = PlayerId(0);

fn world(f0: Faction, credits: i64) -> World {
    let setup = MatchSetup {
        map: crater_ridge(),
        seed: 1,
        players: vec![
            PlayerSetup { faction: f0, starting_credits: credits, ai: false },
            PlayerSetup { faction: Faction::Eu, starting_credits: credits, ai: false },
        ],
    };
    World::new(&setup, rules())
}

fn run(w: &mut World, ticks: u32) {
    for _ in 0..ticks {
        w.step();
    }
}

fn first(w: &World, owner: PlayerId, def: g2_sim::DefId) -> Option<EntityId> {
    w.entities().find(|e| e.owner == Some(owner) && e.def == def).map(|e| e.id)
}

fn all(w: &World, owner: PlayerId, def: g2_sim::DefId) -> Vec<EntityId> {
    w.entities().filter(|e| e.owner == Some(owner) && e.def == def).map(|e| e.id).collect()
}

fn rejections(w: &World) -> Vec<Reject> {
    w.events().iter().map(|Event::Rejected { reason, .. }| *reason).collect()
}

/// Issues a command and steps until it has been applied; returns rejections from that tick.
fn order(w: &mut World, kind: CommandKind) -> Vec<Reject> {
    w.submit(P0, kind);
    let mut out = Vec::new();
    for _ in 0..=g2_sim::world::INPUT_DELAY {
        w.step();
        out.extend(rejections(w));
    }
    out
}

/// Builds `def` with the starting constructor at `pos` and runs until it completes.
fn build(w: &mut World, ctor: g2_sim::DefId, def: g2_sim::DefId, pos: Vec2) -> EntityId {
    let builder = first(w, P0, ctor).expect("constructor");
    assert_eq!(order(w, CommandKind::Build { builder, def, pos }), vec![]);
    let site = w
        .entities()
        .find(|e| e.owner == Some(P0) && e.def == def && e.construction.is_some())
        .expect("foundation placed")
        .id;
    for _ in 0..20_000 {
        if w.entity(site).unwrap().is_complete() {
            return site;
        }
        w.step();
    }
    panic!("construction never finished");
}

#[test]
fn harvesters_carry_supplies_from_pile_to_center() {
    let mut w = world(Faction::Eu, 10_000);
    let depot = build(&mut w, EU_DOZER, EU_SUPPLY, Vec2::from_int(110, 80));
    // The depot sits next to the 6000-credit pile at (130, 60); start with a known balance.
    let before = w.player(P0).credits;
    assert_eq!(order(&mut w, CommandKind::Train { producer: depot, def: EU_HARVESTER }), vec![]);
    let cost = rules().def(EU_HARVESTER).cost.credits;
    assert_eq!(w.player(P0).credits, before - cost);
    run(&mut w, 30 * 60);
    let harvester = first(&w, P0, EU_HARVESTER).expect("harvester trained");
    assert!(matches!(w.entity(harvester).unwrap().order, Order::Gather { .. }));
    let earned = w.player(P0).credits - (before - cost);
    assert!(earned >= 300, "only {earned} credits delivered in 60 s");
    assert_eq!(earned % 100, 0, "deliveries come in full 100-credit loads");
}

#[test]
fn collector_cap_is_per_faction_and_enforced() {
    for (faction, ctor, depot_def, harvester, cap) in [
        (Faction::Eu, EU_DOZER, EU_SUPPLY, EU_HARVESTER, 3),
        (Faction::Apa, APA_CRANE, APA_SUPPLY, APA_HARVESTER, 5),
        (Faction::Gla, GLA_WORKER, GLA_STASH, GLA_WORKER, 8),
    ] {
        let Some(BuildingRole::SupplyCenter { collector_cap }) = rules().def(depot_def).role() else { panic!() };
        assert_eq!(collector_cap, cap);
        let mut w = world(faction, 100_000);
        let depot = build(&mut w, ctor, depot_def, Vec2::from_int(110, 80));
        for _ in 0..cap {
            assert_eq!(order(&mut w, CommandKind::Train { producer: depot, def: harvester }), vec![], "{faction:?}");
            run(&mut w, rules().def(harvester).build_ticks + 1);
        }
        assert_eq!(w.depot_load(depot), cap);
        assert_eq!(order(&mut w, CommandKind::Train { producer: depot, def: harvester }), vec![Reject::DepotFull], "{faction:?}");
    }
}

#[test]
fn queued_collectors_count_toward_the_cap() {
    let mut w = world(Faction::Eu, 100_000);
    let depot = build(&mut w, EU_DOZER, EU_SUPPLY, Vec2::from_int(110, 80));
    let mut rejected = Vec::new();
    for _ in 0..4 {
        rejected.extend(order(&mut w, CommandKind::Train { producer: depot, def: EU_HARVESTER }));
    }
    assert_eq!(rejected, vec![Reject::DepotFull]);
    assert_eq!(w.entity(depot).unwrap().queue.len(), 3);
}

#[test]
fn piles_deplete_and_vanish() {
    let mut map = crater_ridge();
    map.supply_piles = vec![(Vec2::from_int(130, 60), 250)];
    let setup = MatchSetup {
        map,
        seed: 1,
        players: vec![
            PlayerSetup { faction: Faction::Gla, starting_credits: 10_000, ai: false },
            PlayerSetup { faction: Faction::Eu, starting_credits: 0, ai: false },
        ],
    };
    let mut w = World::new(&setup, rules());
    let depot = build(&mut w, GLA_WORKER, GLA_STASH, Vec2::from_int(110, 80));
    let before = w.player(P0).credits;
    for _ in 0..3 {
        order(&mut w, CommandKind::Train { producer: depot, def: GLA_WORKER });
    }
    run(&mut w, 30 * 90);
    let worker_cost = rules().def(GLA_WORKER).cost.credits;
    assert_eq!(w.player(P0).credits, before - 3 * worker_cost + 250, "exactly the pile's 250 credits are collected");
    assert!(!w.entities().any(|e| matches!(rules().def(e.def).class, Class::SupplyPile)));
    for id in all(&w, P0, GLA_WORKER) {
        assert_eq!(w.entity(id).unwrap().order, Order::Idle, "collectors stop once no pile is left");
    }
}

#[test]
fn derricks_only_go_on_free_wells_and_stop_when_dry() {
    let mut map = crater_ridge();
    map.oil_wells = vec![(Vec2::from_int(60, 150), 12)];
    let setup = MatchSetup {
        map,
        seed: 1,
        players: vec![
            PlayerSetup { faction: Faction::Gla, starting_credits: 10_000, ai: false },
            PlayerSetup { faction: Faction::Eu, starting_credits: 0, ai: false },
        ],
    };
    let mut w = World::new(&setup, rules());
    let worker = first(&w, P0, GLA_WORKER).unwrap();
    let off_well = order(&mut w, CommandKind::Build { builder: worker, def: GLA_DERRICK, pos: Vec2::from_int(100, 150) });
    assert_eq!(off_well, vec![Reject::InvalidPlacement]);
    // Slightly off-centre still snaps onto the well.
    let derrick = build(&mut w, GLA_WORKER, GLA_DERRICK, Vec2::from_int(62, 151));
    assert_eq!(w.entity(derrick).unwrap().pos, Vec2::from_int(60, 150));
    let worker = first(&w, P0, GLA_WORKER).unwrap();
    let taken = order(&mut w, CommandKind::Build { builder: worker, def: GLA_DERRICK, pos: Vec2::from_int(60, 150) });
    assert_eq!(taken, vec![Reject::InvalidPlacement], "a well holds one derrick");
    run(&mut w, 30 * 10);
    assert_eq!(w.player(P0).oil, 12, "income stops at the well's reserve");
}

#[test]
fn consumers_outside_power_radius_produce_at_half_speed() {
    let near = Vec2::from_int(110, 80);
    let far = Vec2::from_int(300, 80);
    let mut ticks = Vec::new();
    for pos in [near, far] {
        let mut w = world(Faction::Eu, 100_000);
        let depot = build(&mut w, EU_DOZER, EU_SUPPLY, pos);
        assert_eq!(w.entity(depot).unwrap().powered, pos == near);
        order(&mut w, CommandKind::Train { producer: depot, def: EU_HARVESTER });
        let mut n = 0;
        while first(&w, P0, EU_HARVESTER).is_none() {
            w.step();
            n += 1;
        }
        ticks.push(n);
    }
    let full = rules().def(EU_HARVESTER).build_ticks;
    assert!(ticks[0] <= full + 1, "powered: {} ticks", ticks[0]);
    assert!(ticks[1] >= 2 * full - 1 && ticks[1] <= 2 * full + 1, "unpowered: {} ticks", ticks[1]);
}

#[test]
fn power_drain_and_supply_are_reported_per_player() {
    let mut w = world(Faction::Apa, 100_000);
    assert_eq!(w.player(P0).power_supply, 110);
    build(&mut w, APA_CRANE, APA_SUPPLY, Vec2::from_int(110, 80));
    assert_eq!(w.player(P0).power_drain, 20);
    let mut gla = world(Faction::Gla, 100_000);
    build(&mut gla, GLA_WORKER, GLA_STASH, Vec2::from_int(110, 80));
    assert_eq!((gla.player(P0).power_supply, gla.player(P0).power_drain), (0, 0));
    assert!(gla.entities().filter(|e| e.owner == Some(P0) && rules().def(e.def).is_building()).all(|e| e.powered));
}

#[test]
fn foundations_cost_up_front_and_need_a_constructor_on_site() {
    let mut w = world(Faction::Eu, 1_000);
    let dozer = first(&w, P0, EU_DOZER).unwrap();
    assert_eq!(order(&mut w, CommandKind::Build { builder: dozer, def: EU_SUPPLY, pos: Vec2::from_int(150, 100) }), vec![]);
    assert_eq!(w.player(P0).credits, 1_000 - rules().def(EU_SUPPLY).cost.credits);
    let site = first(&w, P0, EU_SUPPLY).unwrap();
    order(&mut w, CommandKind::Stop { units: vec![dozer] });
    let frozen = w.entity(site).unwrap().construction;
    run(&mut w, 90);
    assert_eq!(w.entity(site).unwrap().construction, frozen, "no progress without a builder");
    assert_eq!(order(&mut w, CommandKind::Construct { units: vec![dozer], site }), vec![]);
    run(&mut w, rules().def(EU_SUPPLY).build_ticks + 30 * 20);
    assert!(w.entity(site).unwrap().is_complete());
    assert_eq!(w.entity(dozer).unwrap().order, Order::Idle);
}

#[test]
fn unaffordable_and_foreign_commands_are_rejected_without_side_effects() {
    let mut w = world(Faction::Eu, 100);
    let dozer = first(&w, P0, EU_DOZER).unwrap();
    let r = order(&mut w, CommandKind::Build { builder: dozer, def: EU_SUPPLY, pos: Vec2::from_int(150, 100) });
    assert_eq!(r, vec![Reject::InsufficientFunds]);
    assert_eq!(w.player(P0).credits, 100);
    assert!(first(&w, P0, EU_SUPPLY).is_none(), "no foundation placed");
    assert_eq!(w.entity(dozer).unwrap().order, Order::Idle);
    let enemy_dozer = first(&w, PlayerId(1), EU_DOZER).unwrap();
    let r = order(&mut w, CommandKind::Move { units: vec![enemy_dozer], target: Vec2::from_int(0, 0) });
    assert_eq!(r, vec![Reject::NotOwner]);
    assert_eq!(w.entity(enemy_dozer).unwrap().order, Order::Idle);
}

#[test]
fn cancelling_production_refunds_in_full() {
    let mut w = world(Faction::Eu, 10_000);
    let depot = build(&mut w, EU_DOZER, EU_SUPPLY, Vec2::from_int(110, 80));
    let before = w.player(P0).credits;
    order(&mut w, CommandKind::Train { producer: depot, def: EU_HARVESTER });
    order(&mut w, CommandKind::CancelTrain { producer: depot });
    assert_eq!(w.player(P0).credits, before);
    assert!(w.entity(depot).unwrap().queue.is_empty());
}

#[test]
fn surrender_defeats_the_player_and_the_last_one_standing_wins() {
    let mut w = World::new(
        &MatchSetup {
            map: crater_ridge(),
            seed: 1,
            players: vec![
                PlayerSetup { faction: Faction::Eu, starting_credits: 0, ai: false },
                PlayerSetup { faction: Faction::Gla, starting_credits: 0, ai: false },
            ],
        },
        rules(),
    );
    assert_eq!(w.winner(), None);
    w.submit(PlayerId(1), CommandKind::Surrender);
    run(&mut w, g2_sim::world::INPUT_DELAY + 1);
    assert!(w.player(PlayerId(1)).defeated);
    assert!(w.entities().all(|e| e.owner != Some(PlayerId(1))), "a defeated player's assets are removed");
    assert_eq!(w.winner(), Some(P0));
}

#[test]
fn group_moves_spread_units_instead_of_stacking() {
    let mut w = world(Faction::Gla, 100_000);
    let depot = build(&mut w, GLA_WORKER, GLA_STASH, Vec2::from_int(110, 80));
    for _ in 0..4 {
        order(&mut w, CommandKind::Train { producer: depot, def: GLA_WORKER });
    }
    run(&mut w, 30 * 40);
    let workers = all(&w, P0, GLA_WORKER);
    order(&mut w, CommandKind::Move { units: workers.clone(), target: Vec2::from_int(200, 200) });
    run(&mut w, 30 * 30);
    let pos: Vec<Vec2> = workers.iter().map(|&id| w.entity(id).unwrap().pos).collect();
    for i in 0..pos.len() {
        assert!(pos[i].distance(Vec2::from_int(200, 200)) < Fx::from_int(20));
        for j in i + 1..pos.len() {
            assert!(pos[i].distance(pos[j]) >= Fx::from_int(3), "units {i} and {j} overlap");
        }
    }
}
