//! Generals' active powers, driven through public commands only.

use g2_rules::generals::{BULLDOG, JUNKYARD, TASKMASTER};
use g2_rules::ids::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::world::{Event, INPUT_DELAY};
use g2_sim::{
    CommandKind, Faction, GeneralId, MatchSetup, PlayerId, PlayerSetup, Reject, Vec2, World, record, verify_replay,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn setup(f0: Faction, general: GeneralId) -> MatchSetup {
    MatchSetup {
        map: crater_ridge(),
        seed: 1,
        players: vec![
            PlayerSetup { faction: f0, starting_credits: 10_000, ai: None, general: Some(general) },
            PlayerSetup { faction: Faction::Eu, starting_credits: 10_000, ai: None, general: None },
        ],
    }
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

fn run_until(w: &mut World, tick: u32) {
    while w.tick() < tick {
        w.step();
    }
}

fn run_for(w: &mut World, ticks: u32) {
    let end = w.tick() + ticks;
    run_until(w, end);
}

fn ready_at(general: GeneralId) -> u32 {
    rules().general(general).power.unwrap().first_ticks
}

/// A power can't be used before it is ready, nor at ground the player hasn't explored; once
/// used it recharges for its cooldown, then works again.
#[test]
fn powers_charge_and_aim_at_explored_ground() {
    let mut w = World::new(&setup(Faction::Eu, BULLDOG), rules());
    let home = crater_ridge().spawns[0];
    let enemy_base = crater_ridge().spawns[1];
    assert_eq!(order(&mut w, CommandKind::Power { at: home }), vec![Reject::PowerNotReady]);
    run_until(&mut w, ready_at(BULLDOG));
    assert_eq!(order(&mut w, CommandKind::Power { at: enemy_base }), vec![Reject::InvalidTarget], "unexplored");
    let used = w.tick();
    // The start sees about 30 around itself.
    let near = home + Vec2::from_int(25, 0);
    assert_eq!(order(&mut w, CommandKind::Power { at: near }), vec![]);
    assert_eq!(order(&mut w, CommandKind::Power { at: home }), vec![Reject::PowerNotReady]);
    let cooldown = rules().general(BULLDOG).power.unwrap().cooldown_ticks;
    run_until(&mut w, used + cooldown + INPUT_DELAY + 1);
    assert_eq!(order(&mut w, CommandKind::Power { at: near }), vec![]);
    // A player without a general has no power at all.
    w.submit(P1, CommandKind::Power { at: enemy_base });
    let mut refused = false;
    for _ in 0..=INPUT_DELAY {
        w.step();
        refused |= w.events().contains(&Event::Rejected { player: P1, reason: Reject::PowerNotReady });
    }
    assert!(refused);
}

/// The barrage lands its shells over a few seconds, damaging enemies at the point and not the
/// player's own units; the power's shells are part of the state hash.
#[test]
fn a_barrage_hits_enemies_at_the_point_only() {
    let mut w = World::new(&setup(Faction::Eu, BULLDOG), rules());
    let enemy_dozer = w.entities().find(|e| e.owner == Some(P1) && e.def == EU_DOZER).unwrap().id;
    let own_dozer = w.entities().find(|e| e.owner == Some(P0) && e.def == EU_DOZER).unwrap().id;
    let spot = crater_ridge().spawns[0] + Vec2::from_int(30, 30);
    // Both dozers to the spot: P0's explores it, P1's is the target.
    w.submit(P0, CommandKind::Move { units: vec![own_dozer], target: spot });
    w.submit(P1, CommandKind::Move { units: vec![enemy_dozer], target: spot + Vec2::from_int(2, 0) });
    run_until(&mut w, ready_at(BULLDOG));
    let hp = |w: &World, id| w.entity(id).map_or(0, |e| e.hp);
    let (own_before, enemy_before) = (hp(&w, own_dozer), hp(&w, enemy_dozer));
    let before = w.state_hash();
    assert_eq!(order(&mut w, CommandKind::Power { at: spot }), vec![]);
    assert!(w.shells().len() >= 6, "shells in flight: {}", w.shells().len());
    let mut landed = 0;
    for _ in 0..30 * 10 {
        w.step();
        landed += w.events().iter().filter(|e| matches!(e, Event::Landed { .. })).count();
    }
    assert_eq!(landed, 7);
    assert!(hp(&w, enemy_dozer) < enemy_before, "the enemy at the point was hit");
    assert_eq!(hp(&w, own_dozer), own_before, "the player's own unit was spared");
    assert_ne!(before, w.state_hash());
}

/// Field repair restores own units and buildings near the point by up to its amount, never
/// above their full hit points, and leaves the far ones alone. The damage comes from a Bulldog
/// opponent's barrage on the APA base.
#[test]
fn field_repair_restores_nearby_units() {
    let s = MatchSetup {
        map: crater_ridge(),
        seed: 1,
        players: vec![
            PlayerSetup { faction: Faction::Apa, starting_credits: 10_000, ai: None, general: Some(TASKMASTER) },
            PlayerSetup { faction: Faction::Eu, starting_credits: 10_000, ai: None, general: Some(BULLDOG) },
        ],
    };
    let mut w = World::new(&s, rules());
    let cc = w.entities().find(|e| e.owner == Some(P0) && e.def == APA_CC).unwrap().id;
    let cc_pos = w.entity(cc).unwrap().pos;
    // The EU dozer walks up beside the APA command center, so the barrage may aim there.
    let eu_dozer = w.entities().find(|e| e.owner == Some(P1) && e.def == EU_DOZER).unwrap().id;
    w.submit(P1, CommandKind::Move { units: vec![eu_dozer], target: cc_pos + Vec2::from_int(18, 0) });
    run_until(&mut w, ready_at(BULLDOG).max(ready_at(TASKMASTER)));
    w.submit(P1, CommandKind::Power { at: cc_pos });
    run_for(&mut w, 30 * 8);
    let full = rules().def(APA_CC).hp;
    let damaged = w.entity(cc).unwrap().hp;
    assert!(damaged < full, "the barrage hit the command center");
    assert_eq!(order(&mut w, CommandKind::Power { at: cc_pos }), vec![]);
    let repair = 400;
    assert_eq!(w.entity(cc).unwrap().hp, (damaged + repair).min(full));
    // Out of range: the crane scouts 50 east, and a repair there leaves the command center be.
    let crane = w.entities().find(|e| e.owner == Some(P0) && e.def == APA_CRANE).unwrap().id;
    let far = cc_pos + Vec2::from_int(50, 0);
    assert_eq!(order(&mut w, CommandKind::Move { units: vec![crane], target: far }), vec![]);
    run_for(&mut w, rules().general(TASKMASTER).power.unwrap().cooldown_ticks);
    let now = w.entity(cc).unwrap().hp;
    assert!(now < full, "still damaged, so a repair in range would show");
    assert_eq!(order(&mut w, CommandKind::Power { at: far }), vec![]);
    assert_eq!(w.entity(cc).unwrap().hp, now);
}

/// The ambush puts four RPG squads on open ground at and around the point. A point in
/// impassable ground (here a patch of it marked beside the start, which the start can see) is
/// refused, and so is dry land at its edge where a squad of the ring would stand in it; the
/// power stays ready.
#[test]
fn an_ambush_brings_squads_onto_open_ground() {
    let mut s = setup(Faction::Gla, JUNKYARD);
    let start = s.map.spawns[0];
    let lake = start + Vec2::from_int(-20, 20);
    let (w, h) = g2_sim::nav::Grid::dims(s.map.width, s.map.height);
    s.map.terrain = (0..w * h)
        .map(|i| g2_sim::nav::Grid::centre((i % w) as i32, (i / w) as i32).distance(lake) < g2_sim::Fx::from_int(8))
        .collect();
    let mut w = World::new(&s, rules());
    run_until(&mut w, ready_at(JUNKYARD));
    assert_eq!(order(&mut w, CommandKind::Power { at: lake }), vec![Reject::InvalidTarget]);
    // The ring's first spot is 4 east (twice the squad's radius): back into the lake from here.
    let shore = lake + Vec2::from_int(-11, 0);
    assert!(w.map.walkable(shore) && !w.map.terrain_within(shore, g2_sim::Fx::from_int(0)));
    assert_eq!(order(&mut w, CommandKind::Power { at: shore }), vec![Reject::InvalidTarget], "ring in the water");
    let spot = start + Vec2::from_int(25, 25);
    let before = w.entities().filter(|e| e.owner == Some(P0) && e.def == GLA_RPG).count();
    assert_eq!(order(&mut w, CommandKind::Power { at: spot }), vec![], "still ready after the refusal");
    let squads: Vec<_> = w.entities().filter(|e| e.owner == Some(P0) && e.def == GLA_RPG).collect();
    assert_eq!(squads.len(), before + 4);
    assert!(squads.iter().all(|e| e.pos.distance(spot) < g2_sim::Fx::from_int(10) && w.map.walkable(e.pos)));
}

/// A match where powers are used replays exactly, and the replay carries the commands.
#[test]
fn matches_with_powers_replay_exactly() {
    for (f, g) in [(Faction::Eu, BULLDOG), (Faction::Apa, TASKMASTER), (Faction::Gla, JUNKYARD)] {
        let s = setup(f, g);
        let home = s.map.spawns[0] + Vec2::from_int(30, 20);
        let at = ready_at(g);
        let (end, replay) = record(World::new(&s, rules()), &s, at + 30 * 20, |w| {
            if w.tick() == at {
                w.submit(P0, CommandKind::Power { at: home });
            }
        });
        let decoded = g2_sim::Replay::decode(&replay.encode()).unwrap();
        assert!(decoded.commands.iter().any(|c| c.kind == CommandKind::Power { at: home }), "{g:?}");
        assert_eq!(verify_replay(&decoded, crater_ridge(), rules()), Ok(end.state_hash()), "{g:?}");
    }
}

/// A `Normal` AI with a general uses its power during the match (in a mirror match, so the
/// fight lasts long enough for a target worth it), an `Easy` one never does, and those matches
/// replay exactly: the AI's power commands are re-derived on replay like all its other
/// commands.
#[test]
fn the_ai_uses_its_generals_power() {
    for (f, g) in [(Faction::Eu, BULLDOG), (Faction::Apa, TASKMASTER), (Faction::Gla, JUNKYARD)] {
        let mut s = setup(f, g);
        for p in &mut s.players {
            p.ai = Some(g2_sim::AiLevel::Normal);
            p.faction = f;
            p.general = Some(g);
        }
        let mut used = 0;
        let mut w = World::new(&s, rules());
        while w.winner().is_none() && w.tick() < 30 * 60 * 15 {
            w.step();
            used += w.events().iter().filter(|e| matches!(e, Event::PowerUsed { player: P0, .. })).count();
        }
        assert!(used >= 1, "{g:?}: power never used");
        let mut easy = s.clone();
        easy.players[0].ai = Some(g2_sim::AiLevel::Easy);
        let mut e = World::new(&easy, rules());
        while e.winner().is_none() && e.tick() < 30 * 60 * 15 {
            e.step();
            let easy_used = e.events().iter().any(|e| matches!(e, Event::PowerUsed { player: P0, .. }));
            assert!(!easy_used, "{g:?}: Easy used a power");
        }
        let (end, replay) = record(World::new(&s, rules()), &s, w.tick(), |_| {});
        assert_eq!(end.state_hash(), w.state_hash());
        assert_eq!(verify_replay(&replay, crater_ridge(), rules()), Ok(w.state_hash()), "{g:?}");
    }
}
