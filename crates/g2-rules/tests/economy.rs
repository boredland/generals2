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
            PlayerSetup { faction: f0, starting_credits: credits, ai: false, general: None },
            PlayerSetup { faction: Faction::Eu, starting_credits: credits, ai: false, general: None },
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

/// The AI packs its first buildings tightly around the command center. Every one of its
/// collectors must still reach the pile and come back: none may sit still for 20 seconds while
/// it has a gather order. (Before pathfinding, EU harvesters wedged between buildings; with
/// routes but unit-unit pushing, two APA harvesters met head-on in a gap and blocked each other.)
/// The opponent is passive, so no attack raids the measured economy.
#[test]
fn ai_collectors_route_between_tightly_packed_buildings() {
    for faction in Faction::ALL {
        let setup = MatchSetup {
            map: crater_ridge(),
            seed: 1,
            players: vec![
                PlayerSetup { faction, starting_credits: 5_000, ai: true, general: None },
                PlayerSetup { faction: Faction::Gla, starting_credits: 5_000, ai: false, general: None },
            ],
        };
        let mut w = World::new(&setup, rules());
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

/// A move into a spot enclosed by terrain stops at the closest reachable point and stays
/// there: the partial route is kept (not replanned into a wander) while the target stands.
#[test]
fn a_move_to_an_unreachable_point_stops_at_the_closest_reachable_one() {
    let mut setup = MatchSetup {
        map: crater_ridge(),
        seed: 1,
        players: vec![
            PlayerSetup { faction: Faction::Eu, starting_credits: 0, ai: false, general: None },
            PlayerSetup { faction: Faction::Gla, starting_credits: 0, ai: false, general: None },
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
    assert!((134.0..=144.0).contains(&x) && (130.0..=150.0).contains(&y), "stops on the near shore, at {stop:?}");
    run(&mut w, 30 * 10);
    assert_eq!(w.entity(dozer).unwrap().pos, stop, "waits there instead of wandering");
    assert!(matches!(w.entity(dozer).unwrap().order, Order::Move { .. }), "the order stands");
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
            PlayerSetup { faction: Faction::Gla, starting_credits: 10_000, ai: false, general: None },
            PlayerSetup { faction: Faction::Eu, starting_credits: 0, ai: false, general: None },
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
            PlayerSetup { faction: Faction::Gla, starting_credits: 10_000, ai: false, general: None },
            PlayerSetup { faction: Faction::Eu, starting_credits: 0, ai: false, general: None },
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
    let before = w.player(P0).credits;
    order(&mut w, CommandKind::Train { producer: depot, def: EU_HARVESTER });
    order(&mut w, CommandKind::CancelTrain { producer: depot });
    assert_eq!(w.player(P0).credits, before);
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
                        ai: false,
                        general: Some(g2_rules::generals::BULLDOG),
                    },
                    PlayerSetup { faction: Faction::Eu, starting_credits: 10_000, ai: false, general: None },
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
            .map(|p| PlayerSetup { faction: p.faction, starting_credits: 10_000, ai: false, general: p.general })
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
                ai: false,
                general: Some(g2_rules::generals::JUNKYARD),
            },
            PlayerSetup { faction: Faction::Gla, starting_credits: 0, ai: false, general: None },
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
                PlayerSetup { faction: Faction::Eu, starting_credits: 0, ai: false, general },
                PlayerSetup { faction: Faction::Gla, starting_credits: 0, ai: false, general: None },
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
                ai: true,
                general: Some(g2_rules::generals::BULLDOG),
            },
            PlayerSetup { faction: Faction::Gla, starting_credits: 0, ai: true, general: None },
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
                PlayerSetup { faction: Faction::Eu, starting_credits: 0, ai: false, general: None },
                PlayerSetup { faction: Faction::Gla, starting_credits: 0, ai: false, general: None },
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
            .map(|faction| PlayerSetup { faction, starting_credits: 100_000, ai: false, general: None })
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
    // Both stop out of range of each other, then both attack on the same tick: equal speed,
    // equal distance, so they come into range together and fire together.
    let mid = Vec2::from_int(200, 260);
    for (&t, dx) in tanks.iter().zip([-30, 30]) {
        let owner = w.entity(t).unwrap().owner.unwrap();
        w.submit(owner, CommandKind::Move { units: vec![t], target: mid + Vec2::from_int(dx, 0) });
    }
    run(&mut w, 30 * 30);
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
/// with no surrender involved.
#[test]
fn destroying_every_building_and_constructor_defeats_a_player() {
    let mut w = world(Faction::Apa, 100_000);
    let tank = train_one(&mut w, APA_CRANE, APA_FACTORY, APA_WARLORD);
    for _ in 0..30 * 600 {
        if w.winner().is_some() {
            break;
        }
        let next = w.entities().find(|e| e.owner == Some(PlayerId(1))).map(|e| e.id);
        if w.entity(tank).unwrap().order == Order::Idle
            && let Some(t) = next
        {
            w.submit(P0, CommandKind::Attack { units: vec![tank], target: t });
        }
        w.step();
    }
    assert!(w.player(PlayerId(1)).defeated);
    assert_eq!(w.winner(), Some(P0));
}
