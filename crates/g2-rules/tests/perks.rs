//! Generals' perks, driven through public commands only: a slotted perk changes its player's
//! costs, build times, power and units from the first tick, and only that player's.

use g2_rules::generals::{BULLDOG, JUNKYARD, TASKMASTER};
use g2_rules::ids::*;
use g2_rules::perks::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::defs::{Effect, PerkId, PowerKind};
use g2_sim::world::INPUT_DELAY;
use g2_sim::{
    CommandKind, DefId, EntityId, Faction, GeneralId, MatchSetup, PlayerId, PlayerSetup, Vec2, World, record,
    verify_replay,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

/// P0 and P1 both play `general`; only P0 slots `perks`.
fn setup(f: Faction, general: GeneralId, perks: &[PerkId]) -> MatchSetup {
    let player = |perks: &[PerkId]| PlayerSetup {
        faction: f,
        starting_credits: 10_000,
        ai: None,
        general: Some(general),
        perks: perks.to_vec(),
    };
    MatchSetup { map: crater_ridge(), seed: 1, players: vec![player(perks), player(&[])] }
}

/// Cost and build-time perks apply to their def only, on top of the general's own modifiers.
#[test]
fn cost_and_build_time_perks_change_only_their_owner() {
    let w = World::new(&setup(Faction::Eu, BULLDOG, &[BD_INSOURCED, BD_FACTORY_OVERTIME]), rules());
    assert_eq!(w.cost_for(P0, EU_AEGIS).credits, w.cost_for(P1, EU_AEGIS).credits * 90 / 100);
    assert_eq!(w.cost_for(P0, EU_RECON), w.cost_for(P1, EU_RECON), "infantry keep their price");
    assert_eq!(w.build_ticks_for(P0, EU_AEGIS), w.build_ticks_for(P1, EU_AEGIS) * 85 / 100);
    assert_eq!(w.build_ticks_for(P0, EU_VALKYRIE), w.build_ticks_for(P1, EU_VALKYRIE), "aircraft aren't vehicles here");
}

/// Power perks shorten the wait and strengthen the power; tunings of the same kind add up.
#[test]
fn power_perks_tune_the_generals_power() {
    let plain = World::new(&setup(Faction::Gla, JUNKYARD, &[]), rules()).power_in(P0, 0).unwrap();
    let w = World::new(&setup(Faction::Gla, JUNKYARD, &[JY_DISCIPLINE, JY_LEVERAGE]), rules());
    let tuned = w.power_in(P0, 0).unwrap();
    assert_eq!(tuned.cooldown_ticks, plain.cooldown_ticks * 80 / 100);
    assert_eq!(tuned.first_ticks, plain.first_ticks * 80 / 100);
    let (PowerKind::Ambush { count: a, .. }, PowerKind::Ambush { count: b, .. }) = (plain.kind, tuned.kind) else {
        panic!("Junkyard's power is the ambush")
    };
    assert_eq!((a, b), (4, 6));
    assert_eq!(w.power_in(P1, 0), Some(plain), "the other player's power is untouched");

    // The shorter first wait is real: the power works before the untuned one could.
    let mut w = w;
    let spot = crater_ridge().spawns[0] + Vec2::from_int(25, 25);
    while w.tick() < tuned.first_ticks {
        w.step();
    }
    let before = w.entities().filter(|e| e.owner == Some(P0) && e.def == GLA_RPG).count();
    w.submit(P0, CommandKind::Power { slot: 0, at: spot });
    for _ in 0..=INPUT_DELAY {
        w.step();
    }
    assert!(w.tick() < plain.first_ticks);
    assert_eq!(w.entities().filter(|e| e.owner == Some(P0) && e.def == GLA_RPG).count(), before + 6);
}

fn build(w: &mut World, who: PlayerId, def: DefId, pos: Vec2) -> EntityId {
    let builder = w.entities().find(|e| e.owner == Some(who) && w.rules.def(e.def).is_constructor()).unwrap().id;
    w.submit(who, CommandKind::Build { builder, def, pos });
    step_until(w, |w| w.entities().any(|e| e.owner == Some(who) && e.def == def && e.is_complete()));
    w.entities().find(|e| e.owner == Some(who) && e.def == def).unwrap().id
}

fn train(w: &mut World, who: PlayerId, producer: EntityId, def: DefId) -> EntityId {
    w.submit(who, CommandKind::Train { producer, def });
    step_until(w, |w| w.entities().any(|e| e.owner == Some(who) && e.def == def));
    w.entities().find(|e| e.owner == Some(who) && e.def == def).unwrap().id
}

fn step_until(w: &mut World, done: impl Fn(&World) -> bool) {
    for _ in 0..30 * 240 {
        if done(w) {
            return;
        }
        w.step();
    }
    panic!("not done by tick {}", w.tick());
}

/// A health perk raises the maximum (and starting) hit points of every unit trained, both
/// tiers adding up. Toughness tiers stack too, so infantry with both Protectors take 90% of
/// 90% damage.
#[test]
fn unit_perks_apply_from_the_start() {
    let mut w = World::new(&setup(Faction::Eu, BULLDOG, &[BD_AERODYNAMICS_I, BD_AERODYNAMICS_II]), rules());
    let home = crater_ridge().spawns[0];
    build(&mut w, P0, EU_POWER, home + Vec2::from_int(-30, 0));
    let field = build(&mut w, P0, EU_AIRFIELD, home + Vec2::from_int(0, 40));
    let jet = train(&mut w, P0, field, EU_VALKYRIE);
    let hp = rules().def(EU_VALKYRIE).hp;
    let e = w.entity(jet).unwrap();
    assert_eq!((e.hp, w.max_hp(e)), (hp + 2 * 93, hp + 2 * 93));
    let tough = |perks: &[PerkId]| -> Vec<i32> {
        World::new(&setup(Faction::Apa, TASKMASTER, perks), rules())
            .effects(P0, APA_RED_GUARD)
            .filter_map(|e| match e {
                Effect::Toughness { percent } => Some(percent),
                _ => None,
            })
            .collect()
    };
    assert_eq!(tough(&[TM_PROTECTOR_I, TM_PROTECTOR_II]), vec![90, 90]);
    assert_eq!(tough(&[TM_KANBAN]), Vec::<i32>::new());
}

/// Perks travel in the replay: a match with them replays to the same hash, and a replay whose
/// perks are taken away no longer does.
#[test]
fn matches_with_perks_replay_exactly() {
    let s = setup(Faction::Gla, JUNKYARD, &[JY_DISCIPLINE, JY_LEVERAGE, JY_SCAVENGER]);
    let at = World::new(&s, rules()).power_in(P0, 0).unwrap().first_ticks;
    let home = s.map.spawns[0] + Vec2::from_int(30, 20);
    let (end, replay) = record(World::new(&s, rules()), &s, at + 30 * 20, |w| {
        if w.tick() == at {
            w.submit(P0, CommandKind::Power { slot: 0, at: home });
        }
    });
    let mut decoded = g2_sim::Replay::decode(&replay.encode()).unwrap();
    assert_eq!(decoded.players[0].perks, vec![JY_DISCIPLINE, JY_LEVERAGE, JY_SCAVENGER]);
    assert_eq!(verify_replay(&decoded, crater_ridge(), rules()), Ok(end.state_hash()));
    decoded.players[0].perks.clear();
    assert_ne!(verify_replay(&decoded, crater_ridge(), rules()), Ok(end.state_hash()));
}

/// A replay can't slot another general's perk, the same perk twice, more than five, or perks
/// with no general.
#[test]
fn replays_refuse_impossible_perks() {
    let check = |general: Option<GeneralId>, perks: Vec<PerkId>| {
        let mut s = setup(Faction::Eu, BULLDOG, &[]);
        s.players[0].general = general;
        s.players[0].perks = perks;
        let (_, replay) = record(World::new(&setup(Faction::Eu, BULLDOG, &[]), rules()), &s, 1, |_| {});
        replay.check(&crater_ridge(), rules())
    };
    assert_eq!(check(Some(BULLDOG), vec![BD_INSOURCED, BD_PROTECTOR_I]), Ok(()));
    assert!(check(Some(BULLDOG), vec![JY_LEADERSHIP]).unwrap_err().contains("not one of"));
    assert!(check(Some(BULLDOG), vec![BD_INSOURCED, BD_INSOURCED]).unwrap_err().contains("twice"));
    let six =
        vec![BD_DOUBLE_SHIFTS, BD_HEAVY_PAYLOAD, BD_PROTECTOR_I, BD_PROTECTOR_II, BD_INSOURCED, BD_FACTORY_OVERTIME];
    assert!(check(Some(BULLDOG), six).unwrap_err().contains("at most 5"));
    assert!(check(None, vec![BD_INSOURCED]).unwrap_err().contains("without a general"));
}

/// Leadership doubles the experience a kill gives: a GLA tank with it ranks up in half the
/// kills; the other player's tank earns the normal amount.
#[test]
fn leadership_doubles_experience() {
    let mut s = setup(Faction::Gla, JUNKYARD, &[JY_LEADERSHIP]);
    s.players[1].faction = Faction::Eu;
    s.players[1].general = None;
    let mut w = World::new(&s, rules());
    let factory = build(&mut w, P0, GLA_FACTORY, crater_ridge().spawns[0] + Vec2::from_int(30, 0));
    let eu_barracks = build(&mut w, P1, EU_BARRACKS, crater_ridge().spawns[1] + Vec2::from_int(-30, 0));
    let tank = train(&mut w, P0, factory, GLA_TANK);
    let field = Vec2::from_int(200, 200);
    w.submit(P0, CommandKind::Move { units: vec![tank], target: field });
    step_until(&mut w, |w| w.entity(tank).unwrap().pos.distance(field) < g2_sim::Fx::from_int(5));
    for kill in 1..=3u32 {
        w.submit(P1, CommandKind::Train { producer: eu_barracks, def: EU_RECON });
        step_until(&mut w, |w| w.entities().any(|e| e.owner == Some(P1) && e.def == EU_RECON));
        let squad = w.entities().find(|e| e.owner == Some(P1) && e.def == EU_RECON).unwrap().id;
        w.submit(P1, CommandKind::Move { units: vec![squad], target: field + Vec2::from_int(20, 0) });
        step_until(&mut w, |w| w.vision(P0).is_visible(w.entity(squad).map_or(field, |e| e.pos)));
        w.submit(P0, CommandKind::Attack { units: vec![tank], target: squad });
        step_until(&mut w, |w| w.entity(squad).is_none());
        assert_eq!(w.entity(tank).unwrap().experience, 2 * kill, "two experience per kill");
    }
    let v = rules().def(GLA_TANK).veterancy.unwrap();
    assert_eq!(w.rank(w.entity(tank).unwrap()), v.experience.iter().filter(|&&x| x <= 6).count() as u8);
}
