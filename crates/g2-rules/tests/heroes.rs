//! The three heroes, driven through public commands only: one per player, standing or in
//! training; each of their abilities' effect, reach and recharge; the trap that waits for
//! aircraft; and a replay of a match using them.

use g2_rules::ids::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::ability::in_place;
use g2_sim::command::Aim;
use g2_sim::defs::StatusKind;
use g2_sim::world::{Event, INPUT_DELAY, ShellSource};
use g2_sim::{
    CommandKind, DefId, EntityId, Faction, Fx, MatchSetup, Order, PlayerId, PlayerSetup, Reject, Vec2, World, record,
    verify_replay,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
/// Out in the middle of Crater Ridge, open ground.
const MEET: Vec2 = Vec2::from_int(200, 200);

fn setup(f0: Faction, f1: Faction) -> MatchSetup {
    let player = |faction| PlayerSetup {
        faction,
        starting_credits: 100_000,
        ai: None,
        general: None,
        perks: Vec::new(),
        rank: 1,
        team: 0,
    };
    MatchSetup { map: crater_ridge(), seed: 1, players: vec![player(f0), player(f1)] }
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

/// Rejections `cmd` drew.
fn order(w: &mut World, who: PlayerId, cmd: CommandKind) -> Vec<Reject> {
    w.submit(who, cmd);
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

fn build(w: &mut World, who: PlayerId, def: DefId, pos: Vec2) -> EntityId {
    let builder = w.entities().find(|e| e.owner == Some(who) && w.rules.def(e.def).is_constructor()).unwrap().id;
    w.submit(who, CommandKind::Build { builder, def, pos });
    step_until(w, INPUT_DELAY + 2, "foundation", |w| w.entities().any(|e| e.owner == Some(who) && e.def == def));
    let site = w.entities().find(|e| e.owner == Some(who) && e.def == def).unwrap().id;
    step_until(w, 30 * 120, "construction", |w| w.entity(site).is_some_and(|s| s.is_complete()));
    site
}

fn train(w: &mut World, who: PlayerId, producer: EntityId, def: DefId) -> EntityId {
    let before: Vec<_> = w.entities().filter(|e| e.owner == Some(who) && e.def == def).map(|e| e.id).collect();
    w.submit(who, CommandKind::Train { producer, def });
    let new = |w: &World| {
        w.entities().find(|e| e.owner == Some(who) && e.def == def && !before.contains(&e.id)).map(|e| e.id)
    };
    step_until(w, 30 * 120, "training", |w| new(w).is_some());
    new(w).unwrap()
}

fn walk(w: &mut World, who: PlayerId, u: EntityId, to: Vec2) {
    w.submit(who, CommandKind::Move { units: vec![u], target: to });
    for _ in 0..=INPUT_DELAY {
        w.step();
    }
    step_until(w, 30 * 120, "arrival", |w| w.entity(u).is_none_or(|e| e.order == Order::Idle));
}

fn command_center(w: &World, who: PlayerId) -> EntityId {
    w.entities()
        .find(|e| e.owner == Some(who) && w.rules.def(e.def).role() == Some(g2_sim::BuildingRole::CommandCenter))
        .unwrap()
        .id
}

/// `who`'s hero, trained at its command center.
fn hero(w: &mut World, who: PlayerId, def: DefId) -> EntityId {
    let cc = command_center(w, who);
    train(w, who, cc, def)
}

/// A factory of `who`'s at its start, toward the map centre.
fn factory(w: &mut World, who: PlayerId, def: DefId) -> EntityId {
    let home = w.map.spawns[who.0 as usize];
    let off = if who == P0 { Vec2::from_int(30, 0) } else { Vec2::from_int(-30, 0) };
    build(w, who, def, home + off)
}

/// The enemy's main battle tank (heavy armour) at `at`.
fn enemy_tank(w: &mut World, at: Vec2) -> EntityId {
    let (f, t) = match w.player(P1).faction {
        Faction::Eu => (EU_FACTORY, EU_AEGIS),
        Faction::Apa => (APA_FACTORY, APA_WARLORD),
        Faction::Gla => (GLA_FACTORY, GLA_TANK),
    };
    let f = factory(w, P1, f);
    let t = train(w, P1, f, t);
    walk(w, P1, t, at);
    t
}

/// Hits `target` took this step from `shooter`, by damage.
fn hits(w: &World, shooter: EntityId, target: EntityId) -> Vec<i32> {
    w.events()
        .iter()
        .filter_map(|e| match *e {
            Event::Hit { shooter: s, target: t, damage, .. } if s == shooter && t == target => Some(damage),
            _ => None,
        })
        .collect()
}

fn status(w: &World, id: EntityId, kind: StatusKind) -> Option<g2_sim::entity::Status> {
    w.entity(id)?.statuses.iter().find(|s| s.kind == kind).copied()
}

fn ability(units: Vec<EntityId>, slot: u8, aim: Aim) -> CommandKind {
    CommandKind::Ability { units, slot, aim }
}

/// Steps until `unit` uses ability `slot`; returns where it stood then.
fn until_used(w: &mut World, unit: EntityId, slot: u8, limit: u32) -> Vec2 {
    let mut at = None;
    step_until(w, limit, "the ability is used", |w| {
        if w.events()
            .iter()
            .any(|e| matches!(*e, Event::AbilityUsed { unit: u, slot: s, .. } if u == unit && s == slot))
        {
            at = w.entity(unit).map(|e| e.pos);
        }
        at.is_some()
    });
    at.unwrap()
}

/// One hero a player: a second is refused while one is in training, and while one stands;
/// once it has fallen, another may be trained.
#[test]
fn a_player_has_one_hero_at_a_time() {
    let s = setup(Faction::Eu, Faction::Apa);
    let mut w = World::new(&s, rules());
    let cc = command_center(&w, P0);
    assert_eq!(order(&mut w, P0, CommandKind::Train { producer: cc, def: EU_COMMANDO }), vec![]);
    assert_eq!(
        order(&mut w, P0, CommandKind::Train { producer: cc, def: EU_COMMANDO }),
        vec![Reject::LimitReached],
        "one in training"
    );
    step_until(&mut w, 30 * 30, "trained", |w| w.entities().any(|e| e.def == EU_COMMANDO));
    let commando = w.entities().find(|e| e.def == EU_COMMANDO).unwrap().id;
    assert_eq!(
        order(&mut w, P0, CommandKind::Train { producer: cc, def: EU_COMMANDO }),
        vec![Reject::LimitReached],
        "one standing"
    );
    // The enemy's tanks shoot it down.
    let f = factory(&mut w, P1, APA_FACTORY);
    let tanks: Vec<_> = (0..4).map(|_| train(&mut w, P1, f, APA_WARLORD)).collect();
    // It walks into the enemy's tanks by their factory, and they shoot it down.
    let at = w.entity(f).unwrap().pos - Vec2::from_int(30, 0);
    w.submit(P1, CommandKind::AttackMove { units: tanks, target: at });
    walk(&mut w, P0, commando, at);
    step_until(&mut w, 30 * 120, "the commando falls", |w| w.entity(commando).is_none());
    assert_eq!(order(&mut w, P0, CommandKind::Train { producer: cc, def: EU_COMMANDO }), vec![], "a new one");
}

/// Grenade Volley: aimed at a point beyond its reach, the commando walks within 50 and throws
/// its 50 grenades there, 100 each within 5; then it recharges for 50 s. Unexplored ground
/// can't be aimed at.
#[test]
fn the_grenade_volley_walks_into_reach_and_lands_fifty_grenades() {
    let s = setup(Faction::Eu, Faction::Apa);
    let mut w = World::new(&s, rules());
    let tank = enemy_tank(&mut w, MEET);
    let c = hero(&mut w, P0, EU_COMMANDO);
    assert_eq!(order(&mut w, P0, ability(vec![c], 0, Aim::Point(MEET))), vec![Reject::InvalidTarget], "unexplored");
    walk(&mut w, P0, c, MEET - Vec2::from_int(35, 0));
    walk(&mut w, P0, c, MEET - Vec2::from_int(90, 0));
    assert_eq!(order(&mut w, P0, ability(vec![c], 0, Aim::Point(MEET))), vec![]);
    let from = until_used(&mut w, c, 0, 30 * 20);
    let used = w.tick() - 1;
    assert!(from.distance(MEET) <= Fx::from_int(50), "thrown from within reach: {from:?}");
    assert!(from.distance(MEET) >= Fx::from_int(45), "and not from closer than it had to: {from:?}");
    assert_eq!(w.entity(c).unwrap().ability_at[0], used + 50 * 30);
    let (mut landed, mut struck) = (0, Vec::new());
    step_until(&mut w, 30 * 10, "every grenade lands", |w| {
        landed += w
            .events()
            .iter()
            .filter(|e| matches!(e, Event::Landed { source: ShellSource::Ability { def: EU_COMMANDO, slot: 0 }, .. }))
            .count();
        struck.extend(hits(w, c, tank));
        landed == 50
    });
    // Its machine gun (50) joins in once the tank is in its reach.
    struck.retain(|&d| d != 50);
    assert!(!struck.is_empty() && struck.iter().all(|&d| d == 100), "{struck:?}");
    assert_eq!(order(&mut w, P0, ability(vec![c], 0, Aim::Point(MEET))), vec![Reject::AbilityNotReady]);
}

/// Unstoppable: for 10 s the commando takes half damage and deals a quarter more; its other
/// slots recharge on their own.
#[test]
fn unstoppable_halves_damage_taken_and_raises_damage_dealt() {
    let s = setup(Faction::Eu, Faction::Apa);
    let mut w = World::new(&s, rules());
    let tank = enemy_tank(&mut w, MEET);
    let c = hero(&mut w, P0, EU_COMMANDO);
    walk(&mut w, P0, c, MEET - Vec2::from_int(28, 0));
    assert_eq!(order(&mut w, P0, ability(vec![c], 0, Aim::Here)), vec![Reject::InvalidTarget], "slot 0 is aimed");
    assert_eq!(order(&mut w, P0, in_place(vec![c], 1)), vec![]);
    assert_eq!(status(&w, c, StatusKind::Toughness).map(|s| s.percent), Some(50));
    assert_eq!(status(&w, c, StatusKind::Damage).map(|s| s.percent), Some(125));
    assert_eq!(w.entity(c).unwrap().ability_at[0], 0, "the volley is still ready");
    w.submit(P1, CommandKind::Attack { units: vec![tank], target: c });
    w.submit(P0, CommandKind::Attack { units: vec![c], target: tank });
    let (mut taken, mut dealt) = (Vec::new(), Vec::new());
    for _ in 0..30 * 6 {
        w.step();
        taken.extend(hits(&w, tank, c));
        dealt.extend(hits(&w, c, tank));
    }
    // The Warlord's gun: 40 against infantry, halved.
    assert!(!taken.is_empty() && taken.iter().all(|&d| d == 20), "{taken:?}");
    assert!(!dealt.is_empty() && dealt.iter().all(|&d| d == 62), "{dealt:?}");
    assert_eq!(order(&mut w, P0, in_place(vec![c], 1)), vec![Reject::AbilityNotReady]);
}

/// Rally: own infantry within 30 deal a quarter more for 15 s; a squad further out, and the
/// enemy, don't.
#[test]
fn rally_lifts_nearby_infantry_only() {
    let s = setup(Faction::Eu, Faction::Apa);
    let mut w = World::new(&s, rules());
    let b = factory(&mut w, P0, EU_BARRACKS);
    let near = train(&mut w, P0, b, EU_RECON);
    let far = train(&mut w, P0, b, EU_RECON);
    let c = hero(&mut w, P0, EU_COMMANDO);
    // The enemy's crane, unarmed, beside the commando: the squads may shoot at it, but
    // nothing shoots back to scatter them.
    let crane = w.entities().find(|e| e.owner == Some(P1) && w.rules.def(e.def).is_constructor()).unwrap().id;
    walk(&mut w, P1, crane, MEET - Vec2::from_int(35, -22));
    walk(&mut w, P0, c, MEET - Vec2::from_int(35, 0));
    walk(&mut w, P0, far, MEET - Vec2::from_int(65, 30));
    walk(&mut w, P0, near, MEET - Vec2::from_int(20, 0));
    assert_eq!(order(&mut w, P0, in_place(vec![c], 2)), vec![]);
    let lifted = |w: &World, id| status(w, id, StatusKind::Damage).map(|s| (s.percent, s.until));
    let (p, until) = lifted(&w, near).expect("the squad near is lifted");
    assert_eq!(p, 125);
    assert_eq!(lifted(&w, c).map(|l| l.1), Some(until), "the commando too");
    assert!(lifted(&w, far).is_none(), "out of reach");
    assert!(lifted(&w, crane).is_none(), "not the enemy");
    assert_eq!(until, w.tick() - 1 + 15 * 30);
}

/// Anti-Materiel Shot: aimed at an enemy tank out of reach, the commando closes to 50 and
/// fires once, 1000 damage, destroying it; the shot recharges for 60 s.
#[test]
fn the_anti_materiel_shot_kills_a_tank_from_fifty() {
    let s = setup(Faction::Apa, Faction::Eu);
    let mut w = World::new(&s, rules());
    let tank = enemy_tank(&mut w, MEET);
    let c = hero(&mut w, P0, APA_COMMANDO);
    let at = w.entity(tank).unwrap().pos;
    walk(&mut w, P0, c, at - Vec2::from_int(60, 0));
    assert!(w.entity(c).unwrap().pos.distance(at) > Fx::from_int(55), "out of reach");
    assert!(w.sees(P0, w.entity(tank).unwrap()), "but in sight");
    assert_eq!(
        order(&mut w, P0, ability(vec![c], 0, Aim::Point(MEET))),
        vec![Reject::InvalidTarget],
        "aimed at a unit"
    );
    assert_eq!(order(&mut w, P0, ability(vec![c], 0, Aim::Unit(tank))), vec![]);
    let mut shot = Vec::new();
    step_until(&mut w, 30 * 10, "the shot", |w| {
        shot.extend(hits(w, c, tank).into_iter().filter(|&d| d >= 1000));
        !shot.is_empty()
    });
    let gap = w.entity(c).unwrap().pos.distance(at) - rules().def(EU_AEGIS).radius;
    assert!(gap <= Fx::from_int(50) && gap >= Fx::from_int(47), "from its reach: {gap:?}");
    assert_eq!(shot, vec![1000]);
    w.step();
    assert!(w.entity(tank).is_none(), "destroyed");
    assert!(w.entity(c).unwrap().ability_at[0] > w.tick() + 59 * 30);
}

/// EMP Grenade: enemy units within 15 of the point can neither move nor fire for 5 s; the
/// commando's own unit there is spared. Its reach is 40.
#[test]
fn the_emp_grenade_stuns_enemies_for_five_seconds() {
    let s = setup(Faction::Apa, Faction::Eu);
    let mut w = World::new(&s, rules());
    let tank = enemy_tank(&mut w, MEET);
    let f = factory(&mut w, P0, APA_FACTORY);
    let ours = train(&mut w, P0, f, APA_WARLORD);
    walk(&mut w, P0, ours, MEET - Vec2::from_int(10, 0));
    let c = hero(&mut w, P0, APA_COMMANDO);
    walk(&mut w, P0, c, MEET - Vec2::from_int(70, 0));
    assert_eq!(order(&mut w, P0, ability(vec![c], 1, Aim::Point(MEET))), vec![]);
    let from = until_used(&mut w, c, 1, 30 * 10);
    assert!(from.distance(MEET) <= Fx::from_int(40) && from.distance(MEET) >= Fx::from_int(35), "{from:?}");
    let stun = status(&w, tank, StatusKind::Stun).expect("stunned");
    assert_eq!(stun.until, w.tick() - 1 + 5 * 30);
    assert!(status(&w, ours, StatusKind::Stun).is_none(), "own unit spared");
}

/// Incendiary Grenade: 40 where it bursts, then the fire burns 20 a second for 5 s.
#[test]
fn the_incendiary_grenade_bursts_and_burns() {
    let s = setup(Faction::Apa, Faction::Eu);
    let mut w = World::new(&s, rules());
    let tank = enemy_tank(&mut w, MEET);
    let c = hero(&mut w, P0, APA_COMMANDO);
    walk(&mut w, P0, c, MEET - Vec2::from_int(45, 0));
    w.submit(P1, CommandKind::Stop { units: vec![tank] });
    assert_eq!(order(&mut w, P0, ability(vec![c], 2, Aim::Point(MEET))), vec![]);
    w.submit(P0, CommandKind::Stop { units: vec![c] });
    let mut burned = Vec::new();
    for _ in 0..30 * 7 {
        w.step();
        burned.extend(hits(&w, c, tank));
        burned.extend(hits(&w, g2_sim::world::POWER_SOURCE, tank));
    }
    let rifle = rules().def(APA_COMMANDO).weapon().unwrap().damage;
    burned.retain(|&d| d != rifle);
    assert_eq!(burned.first(), Some(&40), "{burned:?}");
    assert_eq!(burned.iter().filter(|&&d| d == 20).count(), 5, "{burned:?}");
}

/// Vanish: the agent is hidden from the enemy and half again as fast for 10 s.
#[test]
fn vanish_hides_the_agent_and_speeds_it_up() {
    let s = setup(Faction::Gla, Faction::Eu);
    let mut w = World::new(&s, rules());
    let a = hero(&mut w, P0, GLA_AGENT);
    assert_eq!(order(&mut w, P0, in_place(vec![a], 0)), vec![]);
    assert!(w.hidden(w.entity(a).unwrap()), "hidden");
    assert_eq!(status(&w, a, StatusKind::Speed).map(|s| s.percent), Some(150));
    let used = w.tick() - 1;
    step_until(&mut w, 30 * 11, "it shows again", |w| !w.hidden(w.entity(a).unwrap()));
    assert_eq!(w.tick() - used, 10 * 30, "after 10 s");
    assert_eq!(order(&mut w, P0, in_place(vec![a], 0)), vec![Reject::AbilityNotReady]);
    step_until(&mut w, 30 * 14, "recharged", |w| w.ability_wait(w.entity(a).unwrap(), 0) == Some(0));
    assert_eq!(w.tick(), used + 23 * 30, "after its 23 s");
}

/// Flak Trap: set at a point within 50, it lies hidden while a ground unit drives over it;
/// the first enemy aircraft within 15 takes 125 and the trap is spent.
#[test]
fn the_flak_trap_hits_aircraft_only_and_is_spent() {
    let s = setup(Faction::Gla, Faction::Eu);
    let mut w = World::new(&s, rules());
    let tank = enemy_tank(&mut w, MEET + Vec2::from_int(60, 0));
    let air = build(&mut w, P1, EU_AIRFIELD, s.map.spawns[1] + Vec2::from_int(0, -40));
    let heli = train(&mut w, P1, air, EU_VALKYRIE);
    let a = hero(&mut w, P0, GLA_AGENT);
    walk(&mut w, P0, a, MEET - Vec2::from_int(20, 0));
    walk(&mut w, P0, a, MEET - Vec2::from_int(80, 0));
    assert_eq!(order(&mut w, P0, ability(vec![a], 1, Aim::Point(MEET))), vec![]);
    let from = until_used(&mut w, a, 1, 30 * 10);
    assert!(from.distance(MEET) <= Fx::from_int(50), "{from:?}");
    assert_eq!(w.traps().len(), 1);
    walk(&mut w, P0, a, MEET - Vec2::from_int(80, 60));
    // The tank drives right over it: nothing.
    walk(&mut w, P1, tank, MEET - Vec2::from_int(30, 0));
    assert_eq!(w.traps().len(), 1, "ground units don't set it off");
    assert_eq!(w.entity(tank).unwrap().hp, rules().def(EU_AEGIS).hp);
    // The helicopter flies over it.
    let full = w.entity(heli).unwrap().hp;
    w.submit(P1, CommandKind::Move { units: vec![heli], target: MEET - Vec2::from_int(40, 0) });
    let mut struck = Vec::new();
    step_until(&mut w, 30 * 60, "the trap goes off", |w| {
        struck.extend(hits(w, a, heli));
        !struck.is_empty()
    });
    assert_eq!(struck, vec![125]);
    assert_eq!(w.entity(heli).unwrap().hp, full - 125);
    assert!(w.traps().is_empty(), "spent");
}

/// Last Stand: every other armed infantry squad of the agent's within 30 of the point blows
/// up, each blast hitting the enemy around it; squads further out and the agent itself live.
#[test]
fn last_stand_turns_nearby_squads_into_blasts() {
    let s = setup(Faction::Gla, Faction::Eu);
    let mut w = World::new(&s, rules());
    let tank = enemy_tank(&mut w, MEET + Vec2::from_int(8, 0));
    w.submit(P1, CommandKind::Stop { units: vec![tank] });
    let b = factory(&mut w, P0, GLA_BARRACKS);
    let near = train(&mut w, P0, b, GLA_RABBLE);
    let far = train(&mut w, P0, b, GLA_RABBLE);
    let a = hero(&mut w, P0, GLA_AGENT);
    walk(&mut w, P0, a, MEET - Vec2::from_int(60, 0));
    walk(&mut w, P0, far, MEET - Vec2::from_int(60, 20));
    walk(&mut w, P0, near, MEET + Vec2::from_int(2, 4));
    let lost = w.stats(P0).units_lost;
    assert_eq!(order(&mut w, P0, ability(vec![a], 2, Aim::Point(MEET))), vec![]);
    let mut blasted = Vec::new();
    step_until(&mut w, 30 * 2, "the blast", |w| {
        blasted.extend(hits(w, near, tank));
        !blasted.is_empty()
    });
    assert_eq!(blasted, vec![100]);
    assert!(w.entity(near).is_none(), "the squad is gone");
    assert!(w.entity(far).is_some() && w.entity(a).is_some(), "the far squad and the agent live");
    assert_eq!(w.stats(P0).units_lost, lost + 1);
}

/// A match where both heroes use every kind of aim (in place, a point, an enemy) replays
/// exactly: the command's slot and aim go through the replay, and the traps, casts and
/// recharges through the state hash.
#[test]
fn hero_abilities_replay_exactly() {
    let s = setup(Faction::Apa, Faction::Gla);
    let mut w = World::new(&s, rules());
    let tank = enemy_tank(&mut w, MEET);
    let c = hero(&mut w, P0, APA_COMMANDO);
    let a = hero(&mut w, P1, GLA_AGENT);
    walk(&mut w, P0, c, MEET - Vec2::from_int(45, 0));
    walk(&mut w, P1, a, MEET + Vec2::from_int(30, 0));
    let start = w.tick();
    let (done, replay) = record(w, &s, start + 30 * 15, |w| {
        let t = w.tick() - start;
        if t == 1 {
            w.submit(P0, ability(vec![c], 1, Aim::Point(MEET)));
            w.submit(P1, in_place(vec![a], 0));
            w.submit(P1, ability(vec![a], 1, Aim::Point(MEET + Vec2::from_int(30, 10))));
        }
        if t == 30 {
            w.submit(P0, ability(vec![c], 0, Aim::Unit(tank)));
        }
        if t == 60 {
            w.submit(P0, ability(vec![c], 2, Aim::Point(MEET + Vec2::from_int(0, 5))));
        }
    });
    assert!(done.entity(tank).is_none(), "the shot went home");
    let back = g2_sim::Replay::decode(&replay.encode()).unwrap();
    assert_eq!(back, replay);
    let aims: Vec<_> = back
        .commands
        .iter()
        .filter_map(|c| match c.kind {
            CommandKind::Ability { slot, aim, .. } => Some((slot, aim)),
            _ => None,
        })
        .collect();
    assert_eq!(aims.len(), 5);
    assert_eq!(verify_replay(&back, crater_ridge(), rules()), Ok(done.state_hash()));
}
