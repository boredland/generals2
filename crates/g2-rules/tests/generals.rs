//! Generals' own units and power levels, through public commands only: a general's factories
//! train its units (in place of the ones they replace), its constructors place its defences,
//! and its power grows with the player's rank with that general.

use g2_rules::generals::*;
use g2_rules::ids::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::defs::{GeneralId, PowerKind};
use g2_sim::world::{Event, INPUT_DELAY};
use g2_sim::{
    CommandKind, EntityId, Faction, MatchSetup, PlayerId, PlayerSetup, Reject, Vec2, World, record, verify_replay,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn setup(f: Faction, general: Option<GeneralId>, rank: u8) -> MatchSetup {
    let player = |general, rank| PlayerSetup {
        faction: f,
        starting_credits: 20_000,
        ai: None,
        general,
        perks: Vec::new(),
        rank,
    };
    MatchSetup { map: crater_ridge(), seed: 1, players: vec![player(general, rank), player(None, 1)] }
}

/// Submits `kind` for P0 and steps until it applied; returns that tick's rejections.
fn order(w: &mut World, kind: CommandKind) -> Vec<Reject> {
    w.submit(P0, kind);
    let mut out = Vec::new();
    for _ in 0..=INPUT_DELAY {
        w.step();
        out.extend(w.events().iter().filter_map(|e| match e {
            Event::Rejected { player: P0, reason } => Some(*reason),
            _ => None,
        }));
    }
    out
}

fn build(w: &mut World, who: PlayerId, def: g2_sim::DefId, pos: Vec2) -> EntityId {
    let builder = w.entities().find(|e| e.owner == Some(who) && w.rules.def(e.def).is_constructor()).unwrap().id;
    w.submit(who, CommandKind::Build { builder, def, pos });
    for _ in 0..30 * 240 {
        w.step();
        if let Some(e) = w.entities().find(|e| e.owner == Some(who) && e.def == def && e.is_complete()) {
            return e.id;
        }
    }
    panic!("{} not built by tick {}", w.rules.def(def).name, w.tick())
}

/// Junkyard's war factory trains the Shrapnel Tank in place of the Scorpion, and the Battle Bus
/// and Black Market Artillery besides; the same factory without a general trains neither, and
/// the replaced Scorpion is refused to Junkyard.
#[test]
fn a_generals_factory_trains_its_units() {
    let s = setup(Faction::Gla, Some(JUNKYARD), 1);
    let mut w = World::new(&s, rules());
    let factory = build(&mut w, P0, GLA_FACTORY, s.map.spawns[0] + Vec2::from_int(30, 0));
    assert_eq!(
        w.offers_of(P0, GLA_FACTORY),
        vec![
            GLA_SHRAPNEL_TANK,
            GLA_TOXIN_ARTILLERY,
            GLA_TECHNICAL,
            GLA_TOXIN_CHOPPER,
            GLA_BATTLE_BUS,
            GLA_BLACK_MARKET_ARTILLERY
        ]
    );
    assert_eq!(w.offers_of(P1, GLA_FACTORY), rules().def(GLA_FACTORY).produces.to_vec(), "no general, plain list");
    assert_eq!(order(&mut w, CommandKind::Train { producer: factory, def: GLA_TANK }), vec![Reject::CannotProduce]);
    assert_eq!(order(&mut w, CommandKind::Train { producer: factory, def: GLA_BATTLE_BUS }), vec![]);
    for _ in 0..30 * 30 {
        w.step();
    }
    let bus = w.entities().find(|e| e.owner == Some(P0) && e.def == GLA_BATTLE_BUS).expect("a battle bus");
    assert_eq!(rules().def(bus.def).garrison.map(|g| g.capacity), Some(4));
}

/// Cutting Edge's constructor places the Railgun Turret in place of the Gun Turret; without the
/// general the Railgun Turret is refused.
#[test]
fn a_generals_constructor_places_its_defence() {
    let s = setup(Faction::Eu, Some(CUTTING_EDGE), 1);
    let mut w = World::new(&s, rules());
    let home = s.map.spawns[0];
    build(&mut w, P0, EU_POWER, home + Vec2::from_int(-30, 0));
    let dozer = w.entities().find(|e| e.owner == Some(P0) && e.def == EU_DOZER).unwrap().id;
    let spot = home + Vec2::from_int(0, 30);
    assert_eq!(
        order(&mut w, CommandKind::Build { builder: dozer, def: EU_GUN_TURRET, pos: spot }),
        vec![Reject::CannotBuild]
    );
    assert_eq!(order(&mut w, CommandKind::Build { builder: dozer, def: EU_RAILGUN_TURRET, pos: spot }), vec![]);
    let plain = World::new(&setup(Faction::Eu, None, 1), rules());
    assert!(!plain.offers_of(P0, EU_DOZER).contains(&EU_RAILGUN_TURRET));
    assert!(plain.offers_of(P0, EU_DOZER).contains(&EU_GUN_TURRET));
}

/// A general's power is at level 1 below rank 3, level 2 from rank 3 and level 3 from rank 5:
/// Junkyard's ambush brings 3, 4 then 5 squads, and the level comes into play.
#[test]
fn the_power_grows_with_rank() {
    let count = |rank| {
        let w = World::new(&setup(Faction::Gla, Some(JUNKYARD), rank), rules());
        match w.power_in(P0, 0).unwrap().kind {
            PowerKind::Ambush { count, .. } => count,
            k => panic!("{k:?}"),
        }
    };
    assert_eq!([1, 2, 3, 4, 5].map(count), [3, 3, 4, 4, 5]);
    let s = setup(Faction::Gla, Some(JUNKYARD), 5);
    let mut w = World::new(&s, rules());
    let ready = w.power_wait(P0, 0).unwrap();
    while w.tick() < ready {
        w.step();
    }
    let before = w.entities().filter(|e| e.owner == Some(P0) && e.def == GLA_RPG).count();
    assert_eq!(order(&mut w, CommandKind::Power { slot: 0, at: s.map.spawns[0] + Vec2::from_int(25, 25) }), vec![]);
    assert_eq!(w.entities().filter(|e| e.owner == Some(P0) && e.def == GLA_RPG).count(), before + 5);
}

/// A replay keeps each player's rank: a rank-5 match replays to the same hash, and the same
/// commands at rank 1 (a different power) don't.
#[test]
fn rank_travels_in_the_replay() {
    let s = setup(Faction::Gla, Some(JUNKYARD), 5);
    let at = World::new(&s, rules()).power_wait(P0, 0).unwrap();
    let spot = s.map.spawns[0] + Vec2::from_int(25, 25);
    let (end, replay) = record(World::new(&s, rules()), &s, at + 30 * 5, |w| {
        if w.tick() == at {
            w.submit(P0, CommandKind::Power { slot: 0, at: spot });
        }
    });
    let mut decoded = g2_sim::Replay::decode(&replay.encode()).unwrap();
    assert_eq!(decoded.players[0].rank, 5);
    assert_eq!(verify_replay(&decoded, crater_ridge(), rules()), Ok(end.state_hash()));
    decoded.players[0].rank = 1;
    assert_ne!(verify_replay(&decoded, crater_ridge(), rules()), Ok(end.state_hash()));
    decoded.players[0].rank = 9;
    assert!(decoded.check(&crater_ridge(), rules()).unwrap_err().contains("rank"));
}

/// Every general can play: a Normal AI as each one against Easy, on Crater Ridge, builds and
/// trains with no command refused for an unknown unit, and uses its power.
#[test]
fn every_general_plays() {
    for (i, g) in rules().generals.iter().enumerate() {
        let player = |faction, ai, general| PlayerSetup {
            faction,
            starting_credits: 10_000,
            ai: Some(ai),
            general,
            perks: Vec::new(),
            rank: 3,
        };
        let s = MatchSetup {
            map: crater_ridge(),
            seed: 1,
            players: vec![
                player(g.faction, g2_sim::AiLevel::Normal, Some(GeneralId(i as u16))),
                player(Faction::Gla, g2_sim::AiLevel::Onslaught, None),
            ],
        };
        let mut w = World::new(&s, rules());
        let (mut refused, mut used, mut theirs) = (Vec::new(), false, false);
        let own: Vec<_> = g.units.iter().map(|x| x.unit).collect();
        while w.winner().is_none() && w.tick() < 30 * 60 * 12 {
            w.step();
            for e in w.events() {
                match e {
                    Event::Rejected { player: P0, reason: r @ (Reject::CannotProduce | Reject::CannotBuild) } => {
                        refused.push(*r)
                    }
                    Event::PowerUsed { player: P0, slot: 0, .. } => used = true,
                    Event::Produced { player: P0, def, .. } if own.contains(def) => theirs = true,
                    _ => {}
                }
            }
        }
        assert!(refused.is_empty(), "{}: refused {refused:?}", g.name);
        assert!(used, "{}: power never used", g.name);
        assert!(own.is_empty() || theirs, "{}: none of its own units trained", g.name);
    }
}
