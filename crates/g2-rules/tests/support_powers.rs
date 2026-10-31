//! Faction support powers, driven through public commands only: jets fly in from behind the
//! player's start, strike or look, and fly out; anti-air shoots them down.

use g2_rules::ids::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::defs::PowerKind;
use g2_sim::world::{Event, INPUT_DELAY};
use g2_sim::{
    CommandKind, EntityId, Faction, Fx, MatchSetup, Order, PlayerId, PlayerSetup, Reject, Vec2, World, record,
    verify_replay,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn setup(f0: Faction, f1: Faction) -> MatchSetup {
    let player = |faction| PlayerSetup {
        faction,
        starting_credits: 10_000,
        ai: None,
        general: None,
        perks: Vec::new(),
        rank: 1,
        team: 0,
    };
    MatchSetup { map: crater_ridge(), seed: 1, players: vec![player(f0), player(f1)] }
}

/// Submits `kind` for `who` and steps until it applied; returns that tick's rejections.
fn order(w: &mut World, who: PlayerId, kind: CommandKind) -> Vec<Reject> {
    w.submit(who, kind);
    let mut out = Vec::new();
    for _ in 0..=INPUT_DELAY {
        w.step();
        out.extend(w.events().iter().filter_map(|e| match e {
            Event::Rejected { player, reason } if *player == who => Some(*reason),
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

fn jets(w: &World, owner: PlayerId) -> Vec<EntityId> {
    w.entities().filter(|e| e.owner == Some(owner) && w.rules.def(e.def).is_support_jet()).map(|e| e.id).collect()
}

/// Slot `slot`'s first readiness for `faction`.
fn ready_at(faction: Faction, slot: usize) -> u32 {
    rules().support_powers(faction).nth(slot - 1).unwrap().first_ticks
}

/// Every faction's slots: an empty slot 0 without a general, then its support powers. EU and
/// APA have two each, GLA none; they charge on their own timers and a used one recharges.
#[test]
fn support_powers_have_their_own_slots_and_timers() {
    let w = World::new(&setup(Faction::Eu, Faction::Gla), rules());
    let names = |p: PlayerId| w.powers_of(p).iter().map(|p| p.map(|p| p.name)).collect::<Vec<_>>();
    assert_eq!(names(P0), vec![None, Some("Air Raid"), Some("Fuel Air Bomb")]);
    assert_eq!(names(P1), vec![None, Some("Scud Storm"), Some("GPS Scrambler")]);
    let w = World::new(&setup(Faction::Apa, Faction::Eu), rules());
    assert_eq!(
        w.powers_of(P0).iter().map(|p| p.map(|p| p.name)).collect::<Vec<_>>(),
        vec![None, Some("Carpet Bombing"), Some("Spy Sorties")]
    );
    assert_eq!(w.power_wait(P0, 0), None, "no general, no slot 0");
    assert_eq!(w.power_wait(P0, 2), Some(ready_at(Faction::Apa, 2)));
    let mut w = w;
    assert_eq!(
        order(&mut w, P0, CommandKind::Power { slot: 2, at: Vec2::from_int(10, 10) }),
        vec![Reject::PowerNotReady]
    );
    assert_eq!(
        order(&mut w, P0, CommandKind::Power { slot: 9, at: Vec2::from_int(10, 10) }),
        vec![Reject::PowerNotReady]
    );
}

/// Spy Sorties may be aimed into unexplored ground: the fighter flies in from behind the
/// player's start, over the point (where its sight explores the enemy base) and off the map
/// edge, where it is gone without counting as a loss. It takes no orders on the way.
#[test]
fn a_spy_sortie_scouts_the_fog_and_leaves() {
    let mut w = World::new(&setup(Faction::Apa, Faction::Eu), rules());
    let enemy = crater_ridge().spawns[1];
    run_until(&mut w, ready_at(Faction::Apa, 2));
    assert!(!w.vision(P0).is_explored(enemy));
    assert_eq!(order(&mut w, P0, CommandKind::Power { slot: 2, at: enemy }), vec![]);
    let (used_at, wait) = (w.tick(), w.power_wait(P0, 2).unwrap());
    let cooldown = rules().support_powers(Faction::Apa).nth(1).unwrap().cooldown_ticks;
    assert!(wait <= cooldown && wait + INPUT_DELAY + 1 >= cooldown, "recharging from the use: {wait}");
    let fighter = jets(&w, P0);
    assert_eq!(fighter.len(), 1);
    let start = w.entity(fighter[0]).unwrap().pos;
    let home = crater_ridge().spawns[0];
    assert!(start.distance(enemy) > home.distance(enemy), "it comes in from behind the start");
    assert_eq!(order(&mut w, P0, CommandKind::Stop { units: fighter.clone() }), vec![Reject::NotOwner], "no orders");
    let mut explored_at = None;
    for _ in 0..30 * 60 {
        w.step();
        if explored_at.is_none() && w.vision(P0).is_explored(enemy) {
            explored_at = Some(w.tick());
        }
        if jets(&w, P0).is_empty() {
            break;
        }
    }
    assert!(explored_at.is_some(), "the pass explored the enemy start");
    assert!(jets(&w, P0).is_empty(), "the fighter left the map");
    assert_eq!(w.stats(P0).units_lost, 0, "leaving is no loss");
    assert_eq!(w.power_wait(P0, 2), Some(wait - (w.tick() - used_at)), "still recharging on its own timer");
}

/// An airstrike can't be aimed into unexplored ground; aimed at a visible enemy group, each
/// bomber drops its line of bombs at the point and the group takes the damage. The Fuel Air
/// Bomb is one big blast.
#[test]
fn an_air_raid_bombs_the_point() {
    let mut w = World::new(&setup(Faction::Eu, Faction::Apa), rules());
    let enemy = crater_ridge().spawns[1];
    run_until(&mut w, ready_at(Faction::Eu, 1));
    assert_eq!(order(&mut w, P0, CommandKind::Power { slot: 1, at: enemy }), vec![Reject::InvalidTarget], "unexplored");
    // The APA crane (their constructor) walks into view and stops; the raid aims at it.
    let crane = w.entities().find(|e| e.owner == Some(P1) && e.def == APA_CRANE).unwrap().id;
    let field = Vec2::from_int(140, 120);
    w.submit(P1, CommandKind::Move { units: vec![crane], target: field });
    for _ in 0..30 * 120 {
        w.step();
        if w.entity(crane).unwrap().pos.distance(field) < Fx::from_int(5) {
            break;
        }
    }
    let at = w.entity(crane).unwrap().pos;
    assert!(at.distance(field) < Fx::from_int(5), "the crane arrived");
    // Our own scout keeps it in sight: a Recon Squad would need a barracks, so the dozer goes.
    let dozer = w.entities().find(|e| e.owner == Some(P0) && e.def == EU_DOZER).unwrap().id;
    w.submit(P0, CommandKind::Move { units: vec![dozer], target: at - Vec2::from_int(20, 0) });
    for _ in 0..30 * 120 {
        w.step();
        if w.vision(P0).is_visible(at) {
            break;
        }
    }
    assert!(w.vision(P0).is_explored(at));
    let hp = w.entity(crane).unwrap().hp;
    assert_eq!(order(&mut w, P0, CommandKind::Power { slot: 1, at }), vec![]);
    assert_eq!(jets(&w, P0).len(), 3, "three bombers");
    let mut hits = 0;
    for _ in 0..30 * 30 {
        w.step();
        hits += w.events().iter().filter(|e| matches!(e, Event::Hit { target, .. } if *target == crane)).count();
        if jets(&w, P0).is_empty() {
            break;
        }
    }
    assert!(hits >= 2, "the bombs hit the crane ({hits} hits)");
    assert!(w.entity(crane).is_none_or(|c| c.hp < hp));
    assert!(jets(&w, P0).is_empty(), "the bombers flew out");
}

/// Anti-air shoots a bomber down, and a bomber lost before its point drops nothing: a ring
/// of GLA rocket sites (68 a hit against the Heavy bomber's 300 hit points, each firing as it
/// comes in range) stops a lone Fuel Air Bomb. The sites stand away from the EU base, whose
/// dozer explored the spot before they went up.
#[test]
fn anti_air_shoots_the_bombers_down() {
    let s = setup(Faction::Eu, Faction::Gla);
    let mut w = World::new(&s, rules());
    let at = s.map.spawns[0] + Vec2::from_int(110, 0);
    let dozer = w.entities().find(|e| e.owner == Some(P0) && e.def == EU_DOZER).unwrap().id;
    let home = w.entity(dozer).unwrap().pos;
    for target in [at - Vec2::from_int(10, 0), home] {
        w.submit(P0, CommandKind::Move { units: vec![dozer], target });
        for _ in 0..30 * 60 {
            w.step();
            if w.entity(dozer).unwrap().pos.distance(target) < Fx::from_int(3) {
                break;
            }
        }
    }
    assert!(w.vision(P0).is_explored(at));
    let worker = w.entities().find(|e| e.owner == Some(P1) && e.def == GLA_WORKER).unwrap().id;
    let mut sites = Vec::new();
    for spot in [(12, 0), (-12, 0), (0, 12), (0, -12), (12, 12)] {
        w.submit(
            P1,
            CommandKind::Build { builder: worker, def: GLA_ROCKET_SITE, pos: at + Vec2::from_int(spot.0, spot.1) },
        );
        let mut done = None;
        for _ in 0..30 * 180 {
            w.step();
            done = w
                .entities()
                .find(|e| e.owner == Some(P1) && e.def == GLA_ROCKET_SITE && e.is_complete() && !sites.contains(&e.id))
                .map(|e| e.id);
            if done.is_some() {
                break;
            }
        }
        sites.push(done.expect("each rocket site is built"));
    }
    let ready = ready_at(Faction::Eu, 2).max(w.tick());
    run_until(&mut w, ready);
    assert_eq!(order(&mut w, P0, CommandKind::Power { slot: 2, at }), vec![]);
    let bomber = jets(&w, P0)[0];
    let (mut shot_by_sites, mut landed) = (0, false);
    for _ in 0..30 * 30 {
        w.step();
        shot_by_sites += w
            .events()
            .iter()
            .filter(|e| matches!(e, Event::Hit { target, shooter, .. } if *target == bomber && sites.contains(shooter)))
            .count();
        landed |= w.events().iter().any(|e| matches!(e, Event::Landed { .. }));
        if w.entity(bomber).is_none() {
            break;
        }
    }
    assert!(shot_by_sites >= 5, "the sites fired at the bomber ({shot_by_sites} hits)");
    assert!(w.entity(bomber).is_none());
    assert_eq!(w.stats(P0).units_lost, 1, "shot down, not flown home");
    assert!(!landed, "no bomb fell");
    assert!(sites.iter().all(|&id| w.entity(id).is_some()), "the sites stand");
}

/// A match with support powers replays exactly, and their commands carry the slot.
#[test]
fn matches_with_support_powers_replay_exactly() {
    let s = setup(Faction::Apa, Faction::Eu);
    let at = ready_at(Faction::Apa, 2);
    let enemy = s.map.spawns[1];
    let (end, replay) = record(World::new(&s, rules()), &s, at + 30 * 20, |w| {
        if w.tick() == at {
            w.submit(P0, CommandKind::Power { slot: 2, at: enemy });
        }
    });
    let decoded = g2_sim::Replay::decode(&replay.encode()).unwrap();
    assert!(decoded.commands.iter().any(|c| c.kind == CommandKind::Power { slot: 2, at: enemy }));
    assert_eq!(verify_replay(&decoded, crater_ridge(), rules()), Ok(end.state_hash()));
}

/// The jets' flight line always runs edge to edge through the point: whatever the aim,
/// every jet launched starts and ends on the map edge.
#[test]
fn jets_fly_edge_to_edge() {
    let s = setup(Faction::Apa, Faction::Eu);
    let (wd, ht) = (s.map.width, s.map.height);
    let on_edge = |p: Vec2| p.x == Fx::ZERO || p.y == Fx::ZERO || p.x == wd || p.y == ht;
    for at in [Vec2::from_int(10, 10), s.map.spawns[1], s.map.spawns[0], Vec2::new(wd, ht)] {
        let mut w = World::new(&s, rules());
        run_until(&mut w, ready_at(Faction::Apa, 2));
        assert_eq!(order(&mut w, P0, CommandKind::Power { slot: 2, at }), vec![], "{at:?}");
        let j = w.entity(jets(&w, P0)[0]).unwrap();
        let Order::Sortie { exit, .. } = j.order else { panic!("a sortie") };
        assert!(on_edge(exit), "exit {exit:?} for {at:?}");
    }
}

/// A Normal AI flies its faction's airstrikes, never the recon; an Easy one never the recon
/// either. Against the Onslaught, whose waves keep coming at the base for about 14 minutes:
/// the strikes charge for 6-7 minutes, and a mirror skirmish on Crater Ridge is often over by
/// then. (Easy aims only at three or more enemies it sees together, which it may not meet
/// here; `easy_fires_a_ready_strike_at_a_visible_group` in `ai_tactics.rs` pins that it fires.)
#[test]
fn the_normal_ai_uses_airstrikes() {
    for f in [Faction::Eu, Faction::Apa] {
        for level in [g2_sim::AiLevel::Normal, g2_sim::AiLevel::Easy] {
            let mut s = setup(f, Faction::Gla);
            s.players[0].ai = Some(level);
            s.players[1].ai = Some(g2_sim::AiLevel::Onslaught);
            let mut w = World::new(&s, rules());
            let mut used = Vec::new();
            while w.winner().is_none() && w.tick() < 30 * 60 * 20 {
                w.step();
                used.extend(w.events().iter().filter_map(|e| match e {
                    Event::PowerUsed { player: P0, slot, .. } => Some(*slot),
                    _ => None,
                }));
            }
            let recon = rules()
                .support_powers(f)
                .position(|p| matches!(p.kind, g2_sim::defs::PowerKind::Recon { .. }))
                .map(|r| r as u8 + 1);
            let strikes = used.iter().filter(|&&k| k >= 1 && Some(k) != recon).count();
            assert!(recon.is_none_or(|r| !used.contains(&r)), "{f:?} {level:?} flew the recon");
            if level == g2_sim::AiLevel::Normal {
                assert!(strikes >= 1, "{f:?} Normal flew no airstrike: {used:?}");
            }
        }
    }
}

/// Builds `def` for `who` with their first constructor at `pos` and steps until it is done.
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

/// The Scud Storm power needs the building: unavailable until one is finished, then it charges
/// from that moment. Destroyed by the enemy's own Scud Storm, the last silo takes the power
/// away; a new one charges it from the start again.
#[test]
fn the_scud_storm_needs_its_building() {
    let s = setup(Faction::Gla, Faction::Gla);
    let mut w = World::new(&s, rules());
    let recharge = rules().support_powers(Faction::Gla).next().unwrap().first_ticks;
    assert_eq!(w.power_wait(P0, 1), None, "no Scud Storm, no power");
    assert_eq!(w.power_wait(P0, 2), Some(rules().support_powers(Faction::Gla).nth(1).unwrap().first_ticks));
    // Far from P0's base, so the strikes on it spare the command center and the worker.
    let spot = s.map.spawns[0] + Vec2::from_int(110, 0);
    let silo = build(&mut w, P0, GLA_SCUD_STORM, spot);
    let worker = w.entities().find(|e| e.owner == Some(P0) && e.def == GLA_WORKER).unwrap().id;
    w.submit(P0, CommandKind::Move { units: vec![worker], target: s.map.spawns[0] + Vec2::from_int(-20, -20) });
    let wait = w.power_wait(P0, 1).expect("the finished silo charges the power");
    assert!(wait + 2 >= recharge && wait <= recharge, "charging from completion: {wait} of {recharge}");
    assert_eq!(order(&mut w, P0, CommandKind::Power { slot: 1, at: s.map.spawns[1] }), vec![Reject::PowerNotReady]);
    // P1 builds its own and, once charged, fires it at P0's silo until it falls.
    build(&mut w, P1, GLA_SCUD_STORM, s.map.spawns[1] + Vec2::from_int(25, -25));
    while w.entity(silo).is_some() {
        let ready = w.tick() + w.power_wait(P1, 1).unwrap();
        run_until(&mut w, ready);
        assert_eq!(order(&mut w, P1, CommandKind::Power { slot: 1, at: spot }), vec![]);
        let later = w.tick() + 30 * 20;
        run_until(&mut w, later);
    }
    assert_eq!(w.power_wait(P0, 1), None, "the silo is gone, and the power with it");
    build(&mut w, P0, GLA_SCUD_STORM, spot + Vec2::from_int(-20, 0));
    let wait = w.power_wait(P0, 1).expect("a new silo");
    assert!(wait + 2 >= recharge, "charging from the start again: {wait}");
}

/// A Scud Storm aimed into the fog launches eight missiles from the silo 6 s later; they land
/// on and around the point (together the alpha's 2000 damage: the command center there falls),
/// then the cloud poisons enemy ground units and buildings within 65 for 9 s, a hit each
/// second, and goes.
#[test]
fn a_scud_storm_strikes_and_poisons() {
    let s = setup(Faction::Gla, Faction::Eu);
    let mut w = World::new(&s, rules());
    let silo = build(&mut w, P0, GLA_SCUD_STORM, s.map.spawns[0] + Vec2::from_int(-25, 25));
    let wait = w.power_wait(P0, 1).unwrap();
    let ready = w.tick() + wait;
    let target = s.map.spawns[1];
    // The EU dozer waits in the cloud's reach, beyond every missile's blast.
    let dozer = w.entities().find(|e| e.owner == Some(P1) && e.def == EU_DOZER).unwrap().id;
    let edge = target - Vec2::from_int(60, 0);
    w.submit(P1, CommandKind::Move { units: vec![dozer], target: edge });
    run_until(&mut w, ready);
    assert!(w.entity(dozer).unwrap().pos.distance(edge) < Fx::from_int(3));
    let cc = w.entities().find(|e| e.owner == Some(P1) && e.def == EU_CC).unwrap().id;
    assert!(!w.vision(P0).is_explored(target), "aimed into the fog");
    assert_eq!(order(&mut w, P0, CommandKind::Power { slot: 1, at: target }), vec![]);
    let (mut fired, mut landed, mut dozer_hits, mut first_fired) = (0, 0, 0, None);
    let silo_at = w.entity(silo).unwrap().pos;
    for _ in 0..30 * 40 {
        w.step();
        for e in w.events() {
            match e {
                Event::Fired { from, .. } if *from == silo_at => {
                    fired += 1;
                    first_fired.get_or_insert(w.tick());
                }
                Event::Landed { at, .. } if at.distance(target) <= Fx::from_int(65) => landed += 1,
                Event::Hit { target, damage, .. } if *target == dozer => {
                    assert_eq!(*damage, 20, "only the cloud reaches the dozer");
                    dozer_hits += 1;
                }
                _ => {}
            }
        }
    }
    assert_eq!((fired, landed), (8, 8), "eight missiles from the silo, all at the point");
    assert!(first_fired.unwrap() >= ready + g2_sim::TICKS_PER_SECOND * 6, "6 s before launch");
    assert!(w.entity(cc).is_none(), "the command center fell");
    assert!((8..=10).contains(&dozer_hits), "a cloud hit each second for 9 s: {dozer_hits}");
    assert!(w.clouds().is_empty(), "the cloud is gone");
}

/// The GPS Scrambler hides the player's units at the point from the enemy, who can neither see
/// nor target them; firing gives a unit away, and the cloak wears off.
#[test]
fn the_gps_scrambler_cloaks_units() {
    let s = setup(Faction::Gla, Faction::Eu);
    let mut w = World::new(&s, rules());
    let worker = w.entities().find(|e| e.owner == Some(P0) && e.def == GLA_WORKER).unwrap().id;
    let power = *rules().support_powers(Faction::Gla).nth(1).unwrap();
    let PowerKind::Cloak { ticks, .. } = power.kind else { panic!("the scrambler cloaks") };
    run_until(&mut w, power.first_ticks);
    // The EU dozer walks over to watch the worker.
    let at = w.entity(worker).unwrap().pos;
    let dozer = w.entities().find(|e| e.owner == Some(P1) && e.def == EU_DOZER).unwrap().id;
    w.submit(P1, CommandKind::Move { units: vec![dozer], target: at + Vec2::from_int(15, 0) });
    for _ in 0..30 * 120 {
        w.step();
        if w.sees(P1, w.entity(worker).unwrap()) {
            break;
        }
    }
    assert!(w.sees(P1, w.entity(worker).unwrap()), "in plain view");
    let at = w.entity(worker).unwrap().pos;
    assert_eq!(order(&mut w, P0, CommandKind::Power { slot: 2, at }), vec![]);
    assert!(!w.sees(P1, w.entity(worker).unwrap()), "cloaked");
    assert!(w.sees(P0, w.entity(worker).unwrap()), "its owner still sees it");
    let until = w.entity(worker).unwrap().cloaked_until.unwrap();
    assert!(until > w.tick() && until <= w.tick() + ticks);
    run_until(&mut w, until);
    assert!(w.sees(P1, w.entity(worker).unwrap()), "worn off");
}

/// Firing ends a cloak at once, even for a unit that isn't a stealth unit.
#[test]
fn firing_ends_a_cloak() {
    let s = setup(Faction::Gla, Faction::Gla);
    let mut w = World::new(&s, rules());
    let power = *rules().support_powers(Faction::Gla).nth(1).unwrap();
    let barracks = build(&mut w, P0, GLA_BARRACKS, s.map.spawns[0] + Vec2::from_int(30, 0));
    w.submit(P0, CommandKind::Train { producer: barracks, def: GLA_RABBLE });
    let mut rabble = None;
    for _ in 0..30 * 60 {
        w.step();
        rabble = w.entities().find(|e| e.owner == Some(P0) && e.def == GLA_RABBLE).map(|e| e.id);
        if rabble.is_some() {
            break;
        }
    }
    let rabble = rabble.unwrap();
    let ready = power.first_ticks.max(w.tick());
    run_until(&mut w, ready);
    let at = w.entity(rabble).unwrap().pos;
    assert_eq!(order(&mut w, P0, CommandKind::Power { slot: 2, at }), vec![]);
    assert!(w.hidden(w.entity(rabble).unwrap()));
    // P1's worker comes close; the rabble's guard fires at it.
    let worker = w.entities().find(|e| e.owner == Some(P1) && e.def == GLA_WORKER).unwrap().id;
    w.submit(P1, CommandKind::Move { units: vec![worker], target: at + Vec2::from_int(10, 0) });
    let mut fired = false;
    for _ in 0..30 * 60 {
        w.step();
        if w.events().iter().any(|e| matches!(e, Event::Hit { shooter, .. } if *shooter == rabble)) {
            fired = true;
            break;
        }
    }
    assert!(fired, "the rabble fired");
    assert!(!w.hidden(w.entity(rabble).unwrap()), "firing ended the cloak");
}
