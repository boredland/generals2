//! Teams: allies never shoot or splash each other, share their vision and win together; the
//! Onslaught's waves come at two allied defenders from a start of their own. Through public
//! commands and the match setup only.

use g2_rules::ids::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::onslaught::{WAVES, wave_tick};
use g2_sim::world::Event;
use g2_sim::{
    AiLevel, CommandKind, EntityId, Faction, MapDef, MatchSetup, Order, PlayerId, PlayerSetup, Replay, Vec2, World,
    record, verify_replay,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);

fn player(faction: Faction, ai: Option<AiLevel>, team: u8) -> PlayerSetup {
    PlayerSetup { faction, starting_credits: 100_000, ai, general: None, perks: Vec::new(), rank: 1, team }
}

/// Crater Ridge with a third start in its free north-west corner, for two allies and an enemy.
fn three_starts() -> MapDef {
    let mut map = crater_ridge();
    map.spawns.push(Vec2::from_int(80, 320));
    map
}

/// GLA and EU allied on team 1 against an APA player of no team.
fn allies() -> MatchSetup {
    let players = vec![player(Faction::Gla, None, 1), player(Faction::Eu, None, 1), player(Faction::Apa, None, 0)];
    MatchSetup { map: three_starts(), seed: 1, players }
}

fn first(w: &World, owner: PlayerId, def: g2_sim::DefId) -> Option<EntityId> {
    w.entities().find(|e| e.owner == Some(owner) && e.def == def).map(|e| e.id)
}

fn order(w: &mut World, player: PlayerId, kind: CommandKind) -> Vec<g2_sim::Reject> {
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

/// Builds `factory` with `player`'s starting constructor at `at`, then trains `unit`.
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

/// Team 0 is alone: two players of no team are enemies, as every match before teams.
#[test]
fn team_zero_plays_alone() {
    let mut s = allies();
    s.players[1].team = 0;
    let w = World::new(&s, rules());
    assert!(w.allied(P0, P0));
    assert!(!w.allied(P0, P1) && !w.allied(P1, P2) && !w.allied(P0, P2));
    assert!(w.allied(P0, P0) && w.hostile(P0, Some(P2)) && !w.hostile(P0, None));
    let w = World::new(&allies(), rules());
    assert!(w.allied(P0, P1) && w.allied(P1, P0) && !w.allied(P0, P2) && !w.allied(P2, P1));
}

/// An ally's units can't be ordered attacked, and a guard never fires at them: an EU tank
/// parked beside a GLA dozer sits idle; an APA crane rolling up beside them is shot.
#[test]
fn allies_never_shoot_each_other() {
    let mut w = World::new(&allies(), rules());
    let tank = train(&mut w, P1, EU_DOZER, EU_FACTORY, Vec2::from_int(290, 290), EU_AEGIS);
    let worker = first(&w, P0, GLA_WORKER).unwrap();
    let spot = Vec2::from_int(200, 200);
    walk(&mut w, P0, worker, spot);
    walk(&mut w, P1, tank, spot + Vec2::from_int(8, 0));
    assert_eq!(
        order(&mut w, P1, CommandKind::Attack { units: vec![tank], target: worker }),
        vec![g2_sim::Reject::CannotAttack],
        "an ally is no target"
    );
    for _ in 0..30 * 5 {
        w.step();
        assert!(!w.events().iter().any(|e| matches!(e, Event::Hit { target, .. } if *target == worker)));
    }
    let crane = first(&w, P2, APA_CRANE).unwrap();
    w.submit(P2, CommandKind::Move { units: vec![crane], target: spot + Vec2::from_int(0, 10) });
    let mut shot = false;
    for _ in 0..30 * 60 {
        w.step();
        shot |= w.events().iter().any(|e| matches!(e, Event::Hit { target, .. } if *target == crane));
        if shot {
            break;
        }
    }
    assert!(shot, "the enemy's crane is shot on sight");
}

/// A shell's blast never hurts the shooter's allies, right next to the enemy it hits: the GLA
/// gun shells an APA crane with the EU ally's dozer touching it.
#[test]
fn splash_spares_allies() {
    let mut w = World::new(&allies(), rules());
    let gun = train(&mut w, P0, GLA_WORKER, GLA_FACTORY, Vec2::from_int(110, 110), GLA_TOXIN_ARTILLERY);
    let (crane, dozer) = (first(&w, P2, APA_CRANE).unwrap(), first(&w, P1, EU_DOZER).unwrap());
    walk(&mut w, P2, crane, Vec2::from_int(195, 200));
    walk(&mut w, P1, dozer, Vec2::from_int(195, 207));
    w.submit(P0, CommandKind::Move { units: vec![gun], target: Vec2::from_int(150, 202) });
    let blast = rules().def(GLA_TOXIN_ARTILLERY).weapon().unwrap().shell.unwrap().radius;
    for _ in 0..30 * 60 {
        w.step();
        let Some(&Event::Landed { at, .. }) = w.events().iter().find(|e| matches!(e, Event::Landed { .. })) else {
            continue;
        };
        let hit = |id| w.events().iter().any(|e| matches!(e, Event::Hit { target, .. } if *target == id));
        assert!(!hit(dozer), "the ally's dozer is never hit");
        let inside = w.entity(dozer).is_some_and(|e| e.pos.distance(at) - rules().def(e.def).radius <= blast);
        if hit(crane) && inside {
            return;
        }
    }
    panic!("no shell caught the enemy with the ally's dozer in the blast");
}

/// Allies share vision: what one ally's units see, the other sees, and stealth units of an
/// ally are always seen. Enemies see nothing of it.
#[test]
fn allies_share_vision() {
    let w = World::new(&allies(), rules());
    let (gla, eu, apa) = (w.map.spawns[0], w.map.spawns[1], w.map.spawns[2]);
    assert!(w.vision(P0).is_visible(eu), "GLA sees its ally's base");
    assert!(w.vision(P1).is_visible(gla), "EU sees its ally's base");
    assert!(!w.vision(P2).is_visible(gla) && !w.vision(P2).is_visible(eu), "APA sees neither");
    assert!(!w.vision(P0).is_visible(apa));
    let cc = first(&w, P1, EU_CC).unwrap();
    assert!(w.sees(P0, w.entity(cc).unwrap()));
    assert!(w.vision(P0).memory.is_empty(), "an ally's buildings are not remembered as enemies'");
}

/// The match ends when one side is left: two allies against one enemy have won when it falls,
/// even with one of them fallen first; the winner is a player of that side, and `won` holds for
/// both allies.
#[test]
fn the_last_team_standing_wins() {
    let mut w = World::new(&allies(), rules());
    assert_eq!(order(&mut w, P0, CommandKind::Surrender), vec![]);
    for _ in 0..4 {
        w.step();
    }
    assert!(w.player(P0).defeated);
    assert_eq!(w.winner(), None, "EU and APA are still at war");
    assert_eq!(order(&mut w, P2, CommandKind::Surrender), vec![]);
    for _ in 0..4 {
        w.step();
    }
    assert_eq!(w.winner(), Some(P1));
    assert!(w.won(P0) && w.won(P1) && !w.won(P2), "a side wins together");
}

/// Co-op Onslaught: two allied defenders on a two-start map, the waves a third player with no
/// start. Each wave appears where neither base is, and splits between both bases.
#[test]
fn coop_waves_split_between_both_defenders() {
    let players = vec![
        player(Faction::Eu, None, 1),
        player(Faction::Gla, None, 1),
        PlayerSetup { ai: Some(AiLevel::Onslaught), ..player(Faction::Apa, None, 0) },
    ];
    let s = MatchSetup { map: crater_ridge(), seed: 1, players };
    let mut w = World::new(&s, rules());
    assert!(w.start(P2).is_none());
    let origin = w.onslaught_origin();
    for &start in &w.map.spawns {
        assert!(origin.distance(start) > g2_sim::Fx::from_int(200), "the waves come from far off {origin:?}");
    }
    while w.tick() <= wave_tick(1) + 30 * 2 {
        w.step();
    }
    let toward = |p: PlayerId| {
        w.entities()
            .filter(|e| e.owner == Some(P2))
            .filter(|e| match e.order {
                Order::AttackMove { dest, .. } => dest.distance(w.map.spawns[p.0 as usize]) < g2_sim::Fx::from_int(60),
                _ => false,
            })
            .count()
    };
    assert!(toward(P0) > 0 && toward(P1) > 0, "both bases attacked: {} and {}", toward(P0), toward(P1));
    assert!(toward(P0).abs_diff(toward(P1)) <= 1, "evenly: {} and {}", toward(P0), toward(P1));
}

/// Two passive defenders are overrun by the waves; the setup records and verifies as a replay
/// (v13, teams included) with the Onslaught winning.
#[test]
fn coop_onslaught_ends_and_replays() {
    let players = vec![
        player(Faction::Gla, None, 1),
        player(Faction::Eu, None, 1),
        PlayerSetup { ai: Some(AiLevel::Onslaught), ..player(Faction::Apa, None, 0) },
    ];
    let s = MatchSetup { map: crater_ridge(), seed: 1, players };
    let mut w = World::new(&s, rules());
    while w.winner().is_none() && w.tick() < wave_tick(WAVES) + 30 * 300 {
        w.step();
    }
    assert_eq!(w.winner(), Some(P2), "at {} s", w.tick() / 30);
    assert!(w.player(P0).defeated && w.player(P1).defeated);
    let (end, replay) = record(World::new(&s, rules()), &s, w.tick(), |_| {});
    assert_eq!(end.state_hash(), w.state_hash());
    let decoded = Replay::decode(&replay.encode()).unwrap();
    assert_eq!(decoded.players.iter().map(|p| p.team).collect::<Vec<_>>(), vec![1, 1, 0]);
    assert_eq!(decoded.check(&crater_ridge(), rules()), Ok(()));
    assert_eq!(verify_replay(&decoded, crater_ridge(), rules()), Ok(w.state_hash()));
}

/// A team changes the hash; no team at all hashes as before teams existed (the golden replay
/// guards the rest).
#[test]
fn only_teams_change_the_hash() {
    let mut s = allies();
    let with = World::new(&s, rules()).state_hash();
    for p in &mut s.players {
        p.team = 0;
    }
    assert_ne!(World::new(&s, rules()).state_hash(), with);
}
