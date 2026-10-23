//! Economy behaviour, driven through public commands only.

use g2_rules::ids::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::{
    Armour, BuildingRole, Class, CommandKind, EntityId, Faction, Fx, GatherPhase, MatchSetup, Order, PlayerId,
    PlayerSetup, Reject, Vec2, World, world::Event,
};

const P0: PlayerId = PlayerId(0);

fn world(f0: Faction, credits: i64) -> World {
    let setup = MatchSetup {
        map: crater_ridge(),
        seed: 1,
        players: vec![
            PlayerSetup { faction: f0, starting_credits: credits, ai: None, general: None },
            PlayerSetup { faction: Faction::Eu, starting_credits: credits, ai: None, general: None },
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
    w.events()
        .iter()
        .filter_map(|e| match e {
            Event::Rejected { reason, .. } => Some(*reason),
            _ => None,
        })
        .collect()
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
    let mut delivered = 0;
    for _ in 0..30 * 60 {
        w.step();
        for e in w.events() {
            if let Event::Delivered { player, depot: d, credits } = *e {
                assert_eq!((player, d), (P0, depot));
                delivered += credits;
            }
        }
    }
    let harvester = first(&w, P0, EU_HARVESTER).expect("harvester trained");
    assert!(matches!(w.entity(harvester).unwrap().order, Order::Gather { .. }));
    let earned = w.player(P0).credits - (before - cost);
    assert!(earned >= 300, "only {earned} credits delivered in 60 s");
    assert_eq!(earned % 100, 0, "deliveries come in full 100-credit loads");
    assert_eq!(delivered, earned, "every credit earned is reported as a delivery");
}

/// Collectors must route between tightly packed buildings: none may sit still for 20 seconds
/// on a gather run. The layout is pinned to the one that exposed two bugs, the AI's original
/// placement around Crater Ridge's first start: supply center (60,60), power plant (80,64),
/// command center (80,80), pile at (130,60), so every trip passes the 6.4-unit gap between
/// supply center and power plant. Before pathfinding, EU harvesters wedged in it; with routes
/// but unit-unit pushing, APA harvesters met head-on there and blocked each other. (The AI no
/// longer builds this layout, so the test places the buildings itself.)
#[test]
fn collectors_route_between_tightly_packed_buildings() {
    for (faction, ctor, depot_def, power, collector, count) in [
        (Faction::Eu, EU_DOZER, EU_SUPPLY, EU_POWER, EU_HARVESTER, 3),
        (Faction::Apa, APA_CRANE, APA_SUPPLY, APA_POWER, APA_HARVESTER, 5),
    ] {
        let mut w = world(faction, 100_000);
        let depot = build(&mut w, ctor, depot_def, Vec2::from_int(60, 60));
        build(&mut w, ctor, power, Vec2::from_int(80, 64));
        for _ in 0..count {
            assert_eq!(order(&mut w, CommandKind::Train { producer: depot, def: collector }), vec![]);
        }
        let mut still: std::collections::BTreeMap<EntityId, (Vec2, u32)> = Default::default();
        let mut delivered = 0;
        for t in 0..30 * 300 {
            w.step();
            for e in w.events() {
                if let Event::Delivered { player: P0, credits, .. } = *e {
                    delivered += credits;
                }
            }
            for e in w.entities().filter(|e| e.owner == Some(P0) && rules().def(e.def).collector().is_some()) {
                let entry = still.entry(e.id).or_insert((e.pos, t));
                // Loading and unloading stand still on purpose. A wedged unit jitters by tiny
                // amounts, so "moving" means getting 2 units away from where it last was.
                let busy = matches!(
                    e.order,
                    Order::Gather { phase: GatherPhase::Loading { .. } | GatherPhase::Unloading { .. }, .. }
                );
                if entry.0.distance(e.pos) >= Fx::from_int(2) || busy || !matches!(e.order, Order::Gather { .. }) {
                    *entry = (e.pos, t);
                }
                assert!(
                    t - entry.1 < 30 * 20,
                    "{faction:?} collector {:?} stuck near {:?} since tick {}",
                    e.id,
                    e.pos,
                    entry.1
                );
            }
        }
        assert!(delivered >= 5_000, "{faction:?} delivered only {delivered} in 5 minutes");
    }
}

/// A move into a spot enclosed by terrain (blocked within 10 units, ring search reach) ends on
/// the free cell nearest the clicked point, and the unit stays there; no route keeps searching.
#[test]
fn a_move_to_an_unreachable_point_stops_at_the_closest_reachable_one() {
    let mut setup = MatchSetup {
        map: crater_ridge(),
        seed: 1,
        players: vec![
            PlayerSetup { faction: Faction::Eu, starting_credits: 0, ai: None, general: None },
            PlayerSetup { faction: Faction::Gla, starting_credits: 0, ai: None, general: None },
        ],
    };
    // A square "lake" of terrain from (140,120) to (180,160); the target is its centre.
    let (w, h) = g2_sim::nav::Grid::dims(setup.map.width, setup.map.height);
    let cell = g2_sim::nav::CELL as usize;
    setup.map.terrain =
        (0..w * h).map(|i| (140 / cell..160 / cell + 10).contains(&(i % w)) && (60..80).contains(&(i / w))).collect();
    let mut w = World::new(&setup, rules());
    let dozer = first(&w, P0, EU_DOZER).unwrap();
    let target = Vec2::from_int(150, 140);
    assert_eq!(order(&mut w, CommandKind::Move { units: vec![dozer], target }), vec![]);
    run(&mut w, 30 * 30);
    let stop = w.entity(dozer).unwrap().pos;
    let (x, y) = (stop.x.to_f32(), stop.y.to_f32());
    // The lake spans x 140..180, y 120..160; the nearest free cells to (150,140) are on its
    // west edge, just outside x = 140.
    assert!((134.0..=140.0).contains(&x) && (120.0..=160.0).contains(&y), "stops on the near shore, at {stop:?}");
    assert_eq!(w.entity(dozer).unwrap().order, Order::Idle, "arrived");
    run(&mut w, 30 * 10);
    assert_eq!(w.entity(dozer).unwrap().pos, stop, "waits there instead of wandering");
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
        assert_eq!(
            order(&mut w, CommandKind::Train { producer: depot, def: harvester }),
            vec![Reject::DepotFull],
            "{faction:?}"
        );
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
            PlayerSetup { faction: Faction::Gla, starting_credits: 10_000, ai: None, general: None },
            PlayerSetup { faction: Faction::Eu, starting_credits: 0, ai: None, general: None },
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

/// Derricks pay credits (the 2013 patch's "additional funds"), exactly as much as the well
/// holds, then stop.
#[test]
fn derricks_only_go_on_free_wells_and_stop_when_dry() {
    let mut map = crater_ridge();
    map.oil_wells = vec![(Vec2::from_int(60, 150), 12)];
    let setup = MatchSetup {
        map,
        seed: 1,
        players: vec![
            PlayerSetup { faction: Faction::Gla, starting_credits: 10_000, ai: None, general: None },
            PlayerSetup { faction: Faction::Eu, starting_credits: 0, ai: None, general: None },
        ],
    };
    let mut w = World::new(&setup, rules());
    let worker = first(&w, P0, GLA_WORKER).unwrap();
    let off_well =
        order(&mut w, CommandKind::Build { builder: worker, def: GLA_DERRICK, pos: Vec2::from_int(100, 150) });
    assert_eq!(off_well, vec![Reject::InvalidPlacement]);
    // Slightly off-centre still snaps onto the well.
    let derrick = build(&mut w, GLA_WORKER, GLA_DERRICK, Vec2::from_int(62, 151));
    assert_eq!(w.entity(derrick).unwrap().pos, Vec2::from_int(60, 150));
    let worker = first(&w, P0, GLA_WORKER).unwrap();
    let taken = order(&mut w, CommandKind::Build { builder: worker, def: GLA_DERRICK, pos: Vec2::from_int(60, 150) });
    assert_eq!(taken, vec![Reject::InvalidPlacement], "a well holds one derrick");
    let before = w.player(P0).credits;
    run(&mut w, 30 * 10);
    assert_eq!(w.player(P0).credits - before, 12, "income stops at the well's reserve");
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
    assert_eq!(
        order(&mut w, CommandKind::Build { builder: dozer, def: EU_SUPPLY, pos: Vec2::from_int(150, 100) }),
        vec![]
    );
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
    let (before, spent) = (w.player(P0).credits, w.stats(P0).spent);
    order(&mut w, CommandKind::Train { producer: depot, def: EU_HARVESTER });
    assert!(w.stats(P0).spent > spent);
    order(&mut w, CommandKind::CancelTrain { producer: depot });
    assert_eq!(w.player(P0).credits, before);
    assert_eq!(w.stats(P0).spent, spent, "the refund comes off the match's spending");
    assert!(w.entity(depot).unwrap().queue.is_empty());
}

/// A general's modifiers change what the player pays and how long training takes; a cancel
/// refunds the discounted price exactly, and the general survives a replay round trip.
#[test]
fn a_general_discounts_and_speeds_up_its_units() {
    let train_one = |general| {
        let mut w = world(Faction::Eu, 10_000);
        if general {
            // Same match, but P0 plays Bulldog: harvesters at 80 % cost, 85 % build time.
            let setup = MatchSetup {
                map: crater_ridge(),
                seed: 1,
                players: vec![
                    PlayerSetup {
                        faction: Faction::Eu,
                        starting_credits: 10_000,
                        ai: None,
                        general: Some(g2_rules::generals::BULLDOG),
                    },
                    PlayerSetup { faction: Faction::Eu, starting_credits: 10_000, ai: None, general: None },
                ],
            };
            w = World::new(&setup, rules());
        }
        let depot = build(&mut w, EU_DOZER, EU_SUPPLY, Vec2::from_int(110, 80));
        let before = w.player(P0).credits;
        assert_eq!(order(&mut w, CommandKind::Train { producer: depot, def: EU_HARVESTER }), vec![]);
        let paid = before - w.player(P0).credits;
        let mut ticks = 0;
        while first(&w, P0, EU_HARVESTER).is_none() {
            w.step();
            ticks += 1;
        }
        (paid, ticks, w)
    };
    let (plain_paid, plain_ticks, _) = train_one(false);
    let (paid, ticks, w) = train_one(true);
    assert_eq!(plain_paid, 450);
    assert_eq!(paid, 450 * 80 / 100);
    assert_eq!(w.cost_for(P0, EU_HARVESTER).credits, 360);
    // Training time scales by the general's 85 %: both runs are counted the same way (the first
    // production tick happens inside `order`), so the extra tick cancels out.
    let plain_build = rules().def(EU_HARVESTER).build_ticks;
    assert_eq!(plain_ticks + 1, plain_build);
    assert_eq!(ticks + 1, plain_build * 85 / 100);

    // Cancelling refunds the discounted price, not the list price.
    let mut w = w;
    let depot = first(&w, P0, EU_SUPPLY).unwrap();
    let before = w.player(P0).credits;
    order(&mut w, CommandKind::Train { producer: depot, def: EU_HARVESTER });
    order(&mut w, CommandKind::CancelTrain { producer: depot });
    assert_eq!(w.player(P0).credits, before);

    // The general is part of the recorded setup.
    let setup = MatchSetup {
        map: crater_ridge(),
        seed: 1,
        players: w
            .players()
            .iter()
            .map(|p| PlayerSetup { faction: p.faction, starting_credits: 10_000, ai: None, general: p.general })
            .collect(),
    };
    let decoded = g2_sim::Replay::decode(&w.replay(&setup).encode()).unwrap();
    assert_eq!(decoded.players[0].general, Some(g2_rules::generals::BULLDOG));
    assert_eq!(decoded.players[1].general, None);
}

#[test]
#[should_panic(expected = "is not")]
fn a_general_of_another_faction_is_refused_at_setup() {
    let setup = MatchSetup {
        map: crater_ridge(),
        seed: 1,
        players: vec![
            PlayerSetup {
                faction: Faction::Eu,
                starting_credits: 0,
                ai: None,
                general: Some(g2_rules::generals::JUNKYARD),
            },
            PlayerSetup { faction: Faction::Gla, starting_credits: 0, ai: None, general: None },
        ],
    };
    World::new(&setup, rules());
}

/// The general changes future ticks, so it is part of the state: two otherwise identical worlds
/// differ in hash from tick 0.
#[test]
fn the_general_is_part_of_the_state_hash() {
    let with = |general| {
        let setup = MatchSetup {
            map: crater_ridge(),
            seed: 1,
            players: vec![
                PlayerSetup { faction: Faction::Eu, starting_credits: 0, ai: None, general },
                PlayerSetup { faction: Faction::Gla, starting_credits: 0, ai: None, general: None },
            ],
        };
        World::new(&setup, rules()).state_hash()
    };
    assert_ne!(with(None), with(Some(g2_rules::generals::BULLDOG)));
}

/// A replay file naming a general that doesn't exist, or one of another faction, is refused with
/// an error instead of reaching `World::new`'s assertions.
#[test]
fn replays_with_invalid_generals_are_refused() {
    let setup = MatchSetup {
        map: crater_ridge(),
        seed: 1,
        players: vec![
            PlayerSetup {
                faction: Faction::Eu,
                starting_credits: 0,
                ai: Some(g2_sim::AiLevel::Easy),
                general: Some(g2_rules::generals::BULLDOG),
            },
            PlayerSetup { faction: Faction::Gla, starting_credits: 0, ai: Some(g2_sim::AiLevel::Easy), general: None },
        ],
    };
    let (_, mut replay) = g2_sim::record(World::new(&setup, rules()), &setup, 60, |_| {});
    replay.check(&crater_ridge(), rules()).unwrap();
    replay.players[1].general = Some(g2_sim::GeneralId(999));
    assert!(replay.check(&crater_ridge(), rules()).unwrap_err().contains("unknown general"));
    replay.players[1].general = Some(g2_rules::generals::BULLDOG);
    assert!(replay.check(&crater_ridge(), rules()).unwrap_err().contains("does not belong"));
}

#[test]
fn surrender_defeats_the_player_and_the_last_one_standing_wins() {
    let mut w = World::new(
        &MatchSetup {
            map: crater_ridge(),
            seed: 1,
            players: vec![
                PlayerSetup { faction: Faction::Eu, starting_credits: 0, ai: None, general: None },
                PlayerSetup { faction: Faction::Gla, starting_credits: 0, ai: None, general: None },
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

/// A group moves at its slowest member's speed: Scorpion tanks (11/s) ordered together with
/// Rabble squads (10/s) don't pull ahead. Over 10 s of open-ground march, every member covers
/// at most the slow pace's distance (plus a little sideways separation).
#[test]
fn a_group_moves_at_its_slowest_members_pace() {
    let mut w = world(Faction::Gla, 1_000_000);
    let factory = build(&mut w, GLA_WORKER, GLA_FACTORY, Vec2::from_int(110, 110));
    for _ in 0..3 {
        order(&mut w, CommandKind::Train { producer: factory, def: GLA_TANK });
    }
    let barracks = build(&mut w, GLA_WORKER, GLA_BARRACKS, Vec2::from_int(110, 140));
    for _ in 0..3 {
        order(&mut w, CommandKind::Train { producer: barracks, def: GLA_RABBLE });
    }
    while all(&w, P0, GLA_TANK).len() < 3 || all(&w, P0, GLA_RABBLE).len() < 3 {
        w.step();
    }
    let group: Vec<EntityId> = all(&w, P0, GLA_TANK).into_iter().chain(all(&w, P0, GLA_RABBLE)).collect();
    let (slow, fast) = (rules().def(GLA_RABBLE).speed(), rules().def(GLA_TANK).speed());
    assert!(fast > slow, "the test needs two speeds");
    order(&mut w, CommandKind::Move { units: group.clone(), target: Vec2::from_int(330, 330) });
    let start: Vec<Vec2> = group.iter().map(|&u| w.entity(u).unwrap().pos).collect();
    let ticks = 30 * 10;
    run(&mut w, ticks);
    for (k, &u) in group.iter().enumerate() {
        let moved = w.entity(u).unwrap().pos.distance(start[k]);
        // 10 s at the slow pace is 100 units, at the fast one 110; allow 4 for separation.
        assert!(
            moved <= slow * Fx::from_int(ticks as i64) + Fx::from_int(4),
            "{u:?} moved {moved:?}, ahead of the group"
        );
    }
}

/// Formation slots are dealt out so paths don't cross: no two units' straight lines from
/// start to slot intersect.
#[test]
fn formation_slots_do_not_cross() {
    let mut w = world(Faction::Gla, 100_000);
    let depot = build(&mut w, GLA_WORKER, GLA_STASH, Vec2::from_int(110, 80));
    for _ in 0..8 {
        order(&mut w, CommandKind::Train { producer: depot, def: GLA_WORKER });
    }
    run(&mut w, 30 * 60);
    let workers = all(&w, P0, GLA_WORKER);
    assert!(workers.len() >= 6);
    order(&mut w, CommandKind::Stop { units: workers.clone() });
    for target in [Vec2::from_int(300, 300), Vec2::from_int(40, 300), Vec2::from_int(300, 40)] {
        order(&mut w, CommandKind::Move { units: workers.clone(), target });
        let legs: Vec<(Vec2, Vec2)> = workers
            .iter()
            .map(|&u| {
                let e = w.entity(u).unwrap();
                let Order::Move { target, .. } = e.order else { panic!("{u:?} has no move order") };
                (e.pos, target)
            })
            .collect();
        let cross = |(a, b): (Vec2, Vec2), (c, d): (Vec2, Vec2)| {
            let side = |p: Vec2, q: Vec2, r: Vec2| ((q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x)).0.signum();
            side(a, b, c) * side(a, b, d) < 0 && side(c, d, a) * side(c, d, b) < 0
        };
        for i in 0..legs.len() {
            for j in i + 1..legs.len() {
                assert!(
                    !cross(legs[i], legs[j]),
                    "to {target:?}: paths {i} and {j} cross: {:?} {:?}",
                    legs[i],
                    legs[j]
                );
            }
        }
        run(&mut w, 30 * 60);
    }
}

/// Builds `factory` with the starting constructor, then trains one `unit` from it.
fn train_one(w: &mut World, ctor: g2_sim::DefId, factory: g2_sim::DefId, unit: g2_sim::DefId) -> EntityId {
    let f = build(w, ctor, factory, Vec2::from_int(110, 110));
    assert_eq!(order(w, CommandKind::Train { producer: f, def: unit }), vec![]);
    for _ in 0..30 * 60 {
        if let Some(u) = first(w, P0, unit) {
            return u;
        }
        w.step();
    }
    panic!("{} never trained", rules().def(unit).name);
}

/// Damage per hit is the card value plus the bonus against the target's armour class, the
/// next hit only lands after the weapon's cooldown, and exactly the last hit reports the kill.
#[test]
fn hits_follow_armour_bonus_and_cooldown() {
    // The Warlord's card: 40, +30 vs. Light. The EU bulldozer's card says Heavy (no bonus);
    // the EU harvester has no armour card and is Light.
    for (victim, armour, per_hit) in [(EU_DOZER, Armour::Heavy, 40), (EU_HARVESTER, Armour::Light, 70)] {
        assert_eq!(rules().def(victim).armour, armour);
        let mut w = world(Faction::Apa, 100_000);
        let tank = train_one(&mut w, APA_CRANE, APA_FACTORY, APA_WARLORD);
        let gun = rules().def(APA_WARLORD).weapon().unwrap();
        assert_eq!(gun.damage_vs(armour), per_hit);
        let target = match first(&w, PlayerId(1), victim) {
            Some(t) => t,
            None => {
                let dozer = first(&w, PlayerId(1), EU_DOZER).unwrap();
                let pos = Vec2::from_int(290, 300);
                w.submit(PlayerId(1), CommandKind::Build { builder: dozer, def: EU_SUPPLY, pos });
                while first(&w, PlayerId(1), EU_SUPPLY).is_none_or(|s| !w.entity(s).unwrap().is_complete()) {
                    w.step();
                }
                let depot = first(&w, PlayerId(1), EU_SUPPLY).unwrap();
                w.submit(PlayerId(1), CommandKind::Train { producer: depot, def: victim });
                while first(&w, PlayerId(1), victim).is_none() {
                    w.step();
                }
                first(&w, PlayerId(1), victim).unwrap()
            }
        };
        let hp = rules().def(victim).hp;
        // Fog: an attack order needs the target in sight. Bring the victim next to our base
        // and wait until our units see it.
        let near = w.entity(tank).unwrap().pos + Vec2::from_int(30, 0);
        w.submit(PlayerId(1), CommandKind::Move { units: vec![target], target: near });
        while !w.vision(P0).is_visible(w.entity(target).unwrap().pos) {
            w.step();
        }
        // A harvester trained at a depot gathers; stop it so the cadence check sees a still target.
        w.submit(PlayerId(1), CommandKind::Stop { units: vec![target] });
        run(&mut w, g2_sim::world::INPUT_DELAY + 1);
        assert_eq!(order(&mut w, CommandKind::Attack { units: vec![tank], target }), vec![]);
        let mut hits = Vec::new();
        for _ in 0..30 * 180 {
            w.step();
            for e in w.events() {
                if let Event::Hit { shooter, target: t, damage, killed, .. } = *e {
                    assert_eq!((shooter, t), (tank, target));
                    hits.push((w.tick(), damage, killed));
                }
            }
            if w.entity(target).is_none() {
                break;
            }
        }
        assert!(w.entity(target).is_none(), "a destroyed unit is removed");
        assert!(hits.iter().all(|h| h.1 == per_hit), "{hits:?}");
        let needed = (hp + per_hit - 1) / per_hit;
        let mut kills = vec![false; needed as usize];
        kills[needed as usize - 1] = true;
        assert_eq!(hits.iter().map(|h| h.2).collect::<Vec<_>>(), kills, "{hp} hp at {per_hit} per hit");
        for pair in hits.windows(2) {
            // Exactly the cooldown while the target stands still; longer when it moves away.
            assert!(pair[1].0 - pair[0].0 >= gun.cooldown_ticks, "fires faster than its cooldown: {hits:?}");
        }
        if victim == EU_DOZER {
            assert!(hits.windows(2).all(|p| p[1].0 - p[0].0 == gun.cooldown_ticks), "{hits:?}");
        }
        w.step();
        assert_eq!(w.entity(tank).unwrap().order, Order::Idle, "an attack order ends with its target");
    }
}

/// Shots in one tick resolve together, after every unit has moved and aimed: two tanks that
/// can each kill the other with their next shot both fire, and both die, whichever comes first
/// in entity order. Each kill is reported once.
#[test]
fn mutual_kills_resolve_together() {
    let setup = MatchSetup {
        map: crater_ridge(),
        seed: 1,
        players: [Faction::Apa, Faction::Apa]
            .map(|faction| PlayerSetup { faction, starting_credits: 100_000, ai: None, general: None })
            .to_vec(),
    };
    let mut w = World::new(&setup, rules());
    // One Warlord each, built side by side at the map centre so they meet straight away.
    let mut tanks = Vec::new();
    for (p, at) in [(P0, Vec2::from_int(170, 200)), (PlayerId(1), Vec2::from_int(230, 200))] {
        let crane = first(&w, p, APA_CRANE).unwrap();
        w.submit(p, CommandKind::Build { builder: crane, def: APA_FACTORY, pos: at });
        run(&mut w, 1);
    }
    while [P0, PlayerId(1)]
        .iter()
        .any(|&p| first(&w, p, APA_FACTORY).is_none_or(|f| !w.entity(f).unwrap().is_complete()))
    {
        w.step();
    }
    for p in [P0, PlayerId(1)] {
        let f = first(&w, p, APA_FACTORY).unwrap();
        w.submit(p, CommandKind::Train { producer: f, def: APA_WARLORD });
    }
    while [P0, PlayerId(1)].iter().any(|&p| first(&w, p, APA_WARLORD).is_none()) {
        w.step();
    }
    for p in [P0, PlayerId(1)] {
        tanks.push(first(&w, p, APA_WARLORD).unwrap());
    }
    // Both park far apart (out of sight, so neither guard reacts), mirrored about x = 200,
    // then both attack the other on the same tick. Under fog an attack needs the target in
    // sight or remembered, so first both walk in until they see each other, still as mirror
    // images, and then the attacks go out on the same tick: equal speed, equal distance, so
    // they come into range together and fire together.
    let mid = Vec2::from_int(200, 260);
    for (&t, dx) in tanks.iter().zip([-60, 60]) {
        let owner = w.entity(t).unwrap().owner.unwrap();
        w.submit(owner, CommandKind::Move { units: vec![t], target: mid + Vec2::from_int(dx, 0) });
    }
    run(&mut w, 30 * 40);
    for (&t, dx) in tanks.iter().zip([-1, 1]) {
        let owner = w.entity(t).unwrap().owner.unwrap();
        w.submit(owner, CommandKind::Move { units: vec![t], target: mid + Vec2::from_int(dx, 0) });
    }
    while !w.vision(P0).is_visible(w.entity(tanks[1]).unwrap().pos) {
        w.step();
    }
    assert!(w.vision(PlayerId(1)).is_visible(w.entity(tanks[0]).unwrap().pos), "sight is symmetric");
    for (i, &t) in tanks.iter().enumerate() {
        let other = tanks[1 - i];
        let owner = w.entity(t).unwrap().owner.unwrap();
        w.submit(owner, CommandKind::Attack { units: vec![t], target: other });
    }
    let mut kills = 0;
    for _ in 0..30 * 120 {
        w.step();
        kills += w.events().iter().filter(|e| matches!(e, Event::Hit { killed: true, .. })).count();
        if tanks.iter().any(|&t| w.entity(t).is_none()) {
            break;
        }
    }
    // 610 hp at 40 per hit is 16 hits each; firing in lockstep, both fall on the same tick.
    assert!(tanks.iter().all(|&t| w.entity(t).is_none()), "both tanks die together");
    assert_eq!(kills, 2, "each kill is reported once");
}

#[test]
fn attacks_on_friendly_or_neutral_targets_and_unarmed_attackers_are_rejected() {
    let mut w = world(Faction::Apa, 100_000);
    let tank = train_one(&mut w, APA_CRANE, APA_FACTORY, APA_WARLORD);
    let crane = first(&w, P0, APA_CRANE).unwrap();
    let pile = w.nearest_pile(Vec2::from_int(0, 0)).unwrap();
    let enemy = first(&w, PlayerId(1), EU_CC).unwrap();
    assert_eq!(order(&mut w, CommandKind::Attack { units: vec![tank], target: crane }), vec![Reject::CannotAttack]);
    assert_eq!(order(&mut w, CommandKind::Attack { units: vec![tank], target: pile }), vec![Reject::CannotAttack]);
    assert_eq!(order(&mut w, CommandKind::Attack { units: vec![crane], target: enemy }), vec![Reject::CannotAttack]);
    assert_eq!(w.entity(tank).unwrap().order, Order::Idle);
}

/// Fog: an attack order needs a target the player sees or a building it remembers. A building
/// never seen can't be attacked; once scouted, it can be from anywhere, and the tank keeps the
/// order through the fog until it is in range.
#[test]
fn attacks_need_a_seen_or_remembered_target() {
    let mut w = world(Faction::Apa, 100_000);
    let tank = train_one(&mut w, APA_CRANE, APA_FACTORY, APA_WARLORD);
    let enemy = first(&w, PlayerId(1), EU_CC).unwrap();
    let enemy_pos = w.entity(enemy).unwrap().pos;
    assert_eq!(order(&mut w, CommandKind::Attack { units: vec![tank], target: enemy }), vec![Reject::CannotAttack]);
    // Scout: drive until the command center comes into view, then back off out of sight.
    order(&mut w, CommandKind::Move { units: vec![tank], target: enemy_pos });
    while !w.vision(P0).memory.contains_key(&enemy) {
        w.step();
    }
    let home = Vec2::from_int(150, 150);
    order(&mut w, CommandKind::Move { units: vec![tank], target: home });
    run(&mut w, 30 * 30);
    assert!(!w.vision(P0).is_visible(enemy_pos), "the tank is back out of sight");
    assert_eq!(order(&mut w, CommandKind::Attack { units: vec![tank], target: enemy }), vec![]);
    let mut hit = false;
    for _ in 0..30 * 60 {
        w.step();
        hit |= w
            .events()
            .iter()
            .any(|e| matches!(*e, Event::Hit { shooter, target, .. } if (shooter, target) == (tank, enemy)));
        if hit {
            break;
        }
        assert_eq!(w.entity(tank).unwrap().order, Order::Attack { target: enemy }, "the order holds in the fog");
    }
    assert!(hit, "the tank reached the remembered building and fired");
}

/// An idle armed unit engages an enemy that comes close, chases it while it lives, then
/// walks back to where it stood.
#[test]
fn idle_units_guard_their_spot() {
    let mut w = world(Faction::Apa, 100_000);
    let tank = train_one(&mut w, APA_CRANE, APA_FACTORY, APA_WARLORD);
    run(&mut w, 30 * 5);
    let post = w.entity(tank).unwrap().pos;
    let dozer = first(&w, PlayerId(1), EU_DOZER).unwrap();
    w.submit(PlayerId(1), CommandKind::Move { units: vec![dozer], target: post + Vec2::from_int(35, 0) });
    let mut engaged = false;
    for _ in 0..30 * 120 {
        w.step();
        engaged |= matches!(w.entity(tank).unwrap().order, Order::AttackMove { engaged: Some(t), .. } if t == dozer);
        if w.entity(dozer).is_none() && w.entity(tank).unwrap().order == Order::Idle {
            break;
        }
    }
    assert!(engaged, "the tank engaged the intruder");
    assert!(w.entity(dozer).is_none(), "and destroyed it");
    assert!(w.entity(tank).unwrap().pos.distance(post) <= Fx::from_int(2), "then returned to its post");
}

/// Elimination: a player without buildings or constructors is defeated and the other wins,
/// with no surrender involved. Under fog the tank first drives to the enemy start; after that
/// every target it attacks is one it sees or remembers.
#[test]
fn destroying_every_building_and_constructor_defeats_a_player() {
    let mut w = world(Faction::Apa, 100_000);
    let tank = train_one(&mut w, APA_CRANE, APA_FACTORY, APA_WARLORD);
    let enemy_start = crater_ridge().spawns[1];
    order(&mut w, CommandKind::Move { units: vec![tank], target: enemy_start - Vec2::from_int(25, 25) });
    for _ in 0..30 * 600 {
        if w.winner().is_some() {
            break;
        }
        let vision = w.vision(P0);
        let known = w
            .entities()
            .filter(|e| e.owner == Some(PlayerId(1)))
            .find(|e| vision.is_visible(e.pos) || vision.memory.contains_key(&e.id))
            .map(|e| e.id);
        if matches!(w.entity(tank).unwrap().order, Order::Idle | Order::Move { .. })
            && let Some(t) = known
        {
            w.submit(P0, CommandKind::Attack { units: vec![tank], target: t });
        }
        w.step();
    }
    assert!(w.player(PlayerId(1)).defeated);
    assert_eq!(w.winner(), Some(P0));
    // The whole match, attack orders included, survives encoding and replays to the same state.
    let setup = MatchSetup {
        map: crater_ridge(),
        seed: 1,
        players: w
            .players()
            .iter()
            .map(|p| PlayerSetup { faction: p.faction, starting_credits: 100_000, ai: None, general: None })
            .collect(),
    };
    w.submit(P0, CommandKind::AttackMove { units: vec![tank], target: Vec2::from_int(200, 200) });
    run(&mut w, g2_sim::world::INPUT_DELAY + 1);
    let replay = w.replay(&setup);
    assert!(replay.commands.iter().any(|c| matches!(c.kind, CommandKind::Attack { .. })));
    assert!(replay.commands.iter().any(|c| matches!(c.kind, CommandKind::AttackMove { .. })));
    let decoded = g2_sim::Replay::decode(&replay.encode()).unwrap();
    assert_eq!(decoded, replay);
    assert_eq!(g2_sim::verify_replay(&decoded, crater_ridge(), rules()), Ok(w.state_hash()));
}
