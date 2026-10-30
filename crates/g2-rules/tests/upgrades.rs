//! Upgrades, driven through public commands only: research at a building's queue, then the
//! effect on every unit of the listed defs, existing and future.

use g2_rules::ids::*;
use g2_rules::upgrades::*;
use g2_rules::{crater_ridge, rules};
use g2_sim::world::{Event, INPUT_DELAY};
use g2_sim::{
    Armour, CommandKind, EntityId, Faction, MatchSetup, PlayerId, PlayerSetup, Reject, Vec2, World, record,
    verify_replay,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn world(f0: Faction, f1: Faction) -> World {
    let setup = MatchSetup {
        map: crater_ridge(),
        seed: 1,
        players: vec![
            PlayerSetup {
                faction: f0,
                starting_credits: 100_000,
                ai: None,
                general: None,
                perks: Vec::new(),
                rank: 1,
                team: 0,
            },
            PlayerSetup {
                faction: f1,
                starting_credits: 100_000,
                ai: None,
                general: None,
                perks: Vec::new(),
                rank: 1,
                team: 0,
            },
        ],
    };
    World::new(&setup, rules())
}

fn first(w: &World, owner: PlayerId, def: g2_sim::DefId) -> Option<EntityId> {
    w.entities().find(|e| e.owner == Some(owner) && e.def == def).map(|e| e.id)
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

/// Steps until `done`, failing the test after `limit` ticks instead of hanging.
fn step_until(w: &mut World, limit: u32, what: &str, mut done: impl FnMut(&World) -> bool) {
    for _ in 0..limit {
        if done(w) {
            return;
        }
        w.step();
    }
    panic!("{what}: not within {limit} ticks");
}

/// Builds `def` with `who`'s starting constructor at `pos` (near its base) and runs until done.
fn build(w: &mut World, who: PlayerId, def: g2_sim::DefId, pos: Vec2) -> EntityId {
    let builder = w.entities().find(|e| e.owner == Some(who) && w.rules.def(e.def).is_constructor()).unwrap().id;
    assert_eq!(order(w, who, CommandKind::Build { builder, def, pos }), vec![]);
    let site = w.entities().find(|e| e.owner == Some(who) && e.def == def && e.construction.is_some()).unwrap().id;
    step_until(w, 30 * 120, "construction", |w| w.entity(site).is_some_and(|s| s.is_complete()));
    site
}

/// Trains one `def` at `producer` and returns it once it has rolled out.
fn train(w: &mut World, who: PlayerId, producer: EntityId, def: g2_sim::DefId) -> EntityId {
    let before: Vec<_> = w.entities().filter(|e| e.owner == Some(who) && e.def == def).map(|e| e.id).collect();
    assert_eq!(order(w, who, CommandKind::Train { producer, def }), vec![]);
    let new = |w: &World| {
        w.entities().find(|e| e.owner == Some(who) && e.def == def && !before.contains(&e.id)).map(|e| e.id)
    };
    step_until(w, 30 * 120, "training", |w| new(w).is_some());
    new(w).unwrap()
}

/// Researches `upgrade` at `producer` and runs until it is done.
fn research(w: &mut World, who: PlayerId, producer: EntityId, upgrade: g2_sim::defs::UpgradeId) {
    assert_eq!(order(w, who, CommandKind::Research { producer, upgrade }), vec![]);
    step_until(w, 30 * 120, "research", |w| w.player(who).upgrades.contains(&upgrade));
}

/// Every hit `shooter` lands on `target` over `ticks`, after ordering the attack: (damage,
/// whether it was a shell's splash).
fn hits(w: &mut World, shooter: EntityId, target: EntityId, ticks: u32) -> Vec<(i32, bool)> {
    let who = w.entity(shooter).unwrap().owner.unwrap();
    w.submit(who, CommandKind::Attack { units: vec![shooter], target });
    let mut out = Vec::new();
    for _ in 0..ticks {
        w.step();
        out.extend(w.events().iter().filter_map(|e| match *e {
            Event::Hit { shooter: s, target: t, damage, splash, .. } if s == shooter && t == target => {
                Some((damage, splash))
            }
            _ => None,
        }));
        if w.entity(target).is_none() {
            break;
        }
    }
    out
}

/// The damage of each direct (not splash) hit.
fn direct(h: &[(i32, bool)]) -> Vec<i32> {
    h.iter().filter(|x| !x.1).map(|x| x.0).collect()
}

/// Puts `target` next to `near` and waits until `shooter`'s owner sees it.
fn bring(w: &mut World, target: EntityId, near: Vec2, viewer: PlayerId) {
    let owner = w.entity(target).unwrap().owner.unwrap();
    w.submit(owner, CommandKind::Move { units: vec![target], target: near });
    step_until(w, 30 * 120, "target in sight", |w| w.vision(viewer).is_visible(w.entity(target).unwrap().pos));
    w.submit(owner, CommandKind::Stop { units: vec![target] });
    for _ in 0..=INPUT_DELAY {
        w.step();
    }
}

/// Research takes the upgrade's price and time at a building that offers it, once: a second
/// research of it is refused while queued and after, a building that doesn't offer it refuses,
/// and a cancel refunds in full.
#[test]
fn research_costs_queues_and_completes_once() {
    let mut w = world(Faction::Eu, Faction::Gla);
    let lab = build(&mut w, P0, EU_TECH_LAB, Vec2::from_int(110, 80));
    let barracks = build(&mut w, P0, EU_BARRACKS, Vec2::from_int(60, 110));
    let price = rules().upgrade(HIGHTECH_ALLOYS).cost.credits;
    assert_eq!(price, 750, "the alpha's price");
    let before = w.player(P0).credits;
    let r = order(&mut w, P0, CommandKind::Research { producer: barracks, upgrade: HIGHTECH_ALLOYS });
    assert_eq!(r, vec![Reject::CannotProduce], "the barracks doesn't offer it");
    assert_eq!(order(&mut w, P0, CommandKind::Research { producer: lab, upgrade: HIGHTECH_ALLOYS }), vec![]);
    assert_eq!(w.player(P0).credits, before - price);
    let r = order(&mut w, P0, CommandKind::Research { producer: lab, upgrade: HIGHTECH_ALLOYS });
    assert_eq!(r, vec![Reject::AlreadyResearched], "already queued");
    assert_eq!(order(&mut w, P0, CommandKind::CancelTrain { producer: lab }), vec![]);
    assert_eq!(w.player(P0).credits, before, "a cancelled research refunds in full");
    assert!(w.player(P0).upgrades.is_empty());

    let started = w.tick();
    assert_eq!(order(&mut w, P0, CommandKind::Research { producer: lab, upgrade: HIGHTECH_ALLOYS }), vec![]);
    let mut done_at = None;
    while done_at.is_none() {
        w.step();
        for e in w.events() {
            if let Event::Researched { player: P0, upgrade, .. } = *e {
                assert_eq!(upgrade, HIGHTECH_ALLOYS);
                done_at = Some(w.tick());
            }
        }
    }
    // Powered (the command center's 110 supply covers the lab's 30): full speed, 25 s.
    let took = done_at.unwrap() - started;
    let want = rules().upgrade(HIGHTECH_ALLOYS).research_ticks;
    assert!((want..=want + INPUT_DELAY + 1).contains(&took), "took {took} ticks for {want}");
    assert_eq!(w.player(P0).upgrades, vec![HIGHTECH_ALLOYS]);
    let r = order(&mut w, P0, CommandKind::Research { producer: lab, upgrade: HIGHTECH_ALLOYS });
    assert_eq!(r, vec![Reject::AlreadyResearched], "already done");
    assert!(w.entity(lab).unwrap().queue.is_empty());
}

/// Hollow Point Ammo raises the rifle squad's damage against infantry by Zero Hour's 25% and
/// leaves its damage against vehicles alone.
#[test]
fn a_damage_upgrade_applies_against_its_armour_only() {
    let mut w = world(Faction::Eu, Faction::Gla);
    let barracks = build(&mut w, P0, EU_BARRACKS, Vec2::from_int(110, 80));
    let recon = train(&mut w, P0, barracks, EU_RECON);
    let rifle = rules().def(EU_RECON).weapon().unwrap();
    assert_eq!(rules().def(GLA_WORKER).armour, Armour::Infantry);
    // The GLA side first, at home and out of the fight: two workers and a Technical.
    let gla_cc = first(&w, P1, GLA_CC).unwrap();
    let factory = build(&mut w, P1, GLA_FACTORY, crater_ridge().spawns[1] + Vec2::from_int(-30, 0));
    let tech = train(&mut w, P1, factory, GLA_TECHNICAL);
    let w1 = train(&mut w, P1, gla_cc, GLA_WORKER);
    let w2 = train(&mut w, P1, gla_cc, GLA_WORKER);

    let near = w.entity(recon).unwrap().pos + Vec2::from_int(20, 0);
    bring(&mut w, w1, near, P0);
    let base = direct(&hits(&mut w, recon, w1, 30 * 3));
    assert!(!base.is_empty() && base.iter().all(|&d| d == rifle.damage_vs(Armour::Infantry)), "{base:?}");

    research(&mut w, P0, barracks, HOLLOW_POINT);
    bring(&mut w, w2, near, P0);
    let after = direct(&hits(&mut w, recon, w2, 30 * 3));
    let want = rifle.damage_vs(Armour::Infantry) * 125 / 100;
    assert!(!after.is_empty() && after.iter().all(|&d| d == want), "{after:?}, want {want}");

    // A Technical is Light: no change.
    bring(&mut w, tech, near, P0);
    let light = direct(&hits(&mut w, recon, tech, 30 * 3));
    assert!(!light.is_empty() && light.iter().all(|&d| d == rifle.damage_vs(Armour::Light)), "{light:?}");
}

/// Hightech Alloys: EU buildings take 75% damage; a unit of the same player takes full damage.
#[test]
fn toughness_cuts_damage_taken_by_the_listed_defs() {
    let mut w = world(Faction::Eu, Faction::Apa);
    let lab = build(&mut w, P0, EU_TECH_LAB, Vec2::from_int(110, 80));
    research(&mut w, P0, lab, HIGHTECH_ALLOYS);
    let factory = build(&mut w, P1, APA_FACTORY, crater_ridge().spawns[1] + Vec2::from_int(-30, 0));
    let tank = train(&mut w, P1, factory, APA_WARLORD);
    let gun = rules().def(APA_WARLORD).weapon().unwrap();
    let cc = first(&w, P0, EU_CC).unwrap();
    let cc_pos = w.entity(cc).unwrap().pos;
    w.submit(P1, CommandKind::Move { units: vec![tank], target: cc_pos + Vec2::from_int(25, 0) });
    step_until(&mut w, 30 * 120, "command center in sight", |w| w.vision(P1).is_visible(cc_pos));
    let on_cc = direct(&hits(&mut w, tank, cc, 30 * 5));
    let want = gun.damage_vs(Armour::Structure) * 75 / 100;
    assert!(!on_cc.is_empty() && on_cc.iter().all(|&d| d == want), "{on_cc:?}, want {want}");
    let dozer = first(&w, P0, EU_DOZER).unwrap();
    let near_tank = w.entity(tank).unwrap().pos + Vec2::from_int(10, 0);
    bring(&mut w, dozer, near_tank, P1);
    let on_dozer = direct(&hits(&mut w, tank, dozer, 30 * 4));
    assert!(
        !on_dozer.is_empty() && on_dozer.iter().all(|&d| d == gun.damage_vs(Armour::Heavy)),
        "the bulldozer isn't a building: {on_dozer:?}"
    );
}

/// Population Control adds a man's hit points to every Red Guard squad: ones already fielded
/// gain them on top of their current hit points (Zero Hour's `ADD_CURRENT_HEALTH_TOO`), new
/// ones roll out with the higher maximum.
#[test]
fn a_health_upgrade_reaches_fielded_and_new_squads() {
    let mut w = world(Faction::Apa, Faction::Eu);
    let barracks = build(&mut w, P0, APA_BARRACKS, Vec2::from_int(110, 80));
    let old = train(&mut w, P0, barracks, APA_RED_GUARD);
    let base = rules().def(APA_RED_GUARD).hp;
    research(&mut w, P0, barracks, POPULATION_CONTROL);
    assert_eq!(w.entity(old).unwrap().hp, base + 60);
    assert_eq!(w.max_hp(w.entity(old).unwrap()), base + 60);
    let new = train(&mut w, P0, barracks, APA_RED_GUARD);
    assert_eq!(w.entity(new).unwrap().hp, base + 60);
    // The other player's Red Guard-less army is untouched; so is a unit the upgrade doesn't list.
    let crane = first(&w, P0, APA_CRANE).unwrap();
    assert_eq!(w.max_hp(w.entity(crane).unwrap()), rules().def(APA_CRANE).hp);
}

/// Scrap Repair: a damaged Scorpion regains 2 hp a second, but only after 3 s without a hit,
/// and never past its maximum.
#[test]
fn repair_waits_for_a_quiet_spell_and_stops_at_full() {
    let mut w = world(Faction::Gla, Faction::Apa);
    let bay = build(&mut w, P0, GLA_SALVAGE_BAY, Vec2::from_int(110, 80));
    let factory = build(&mut w, P0, GLA_FACTORY, Vec2::from_int(60, 120));
    let scorpion = train(&mut w, P0, factory, GLA_TANK);
    research(&mut w, P0, bay, SCRAP_REPAIR);
    // An APA tank hits it twice, then is sent home.
    let apa_factory = build(&mut w, P1, APA_FACTORY, crater_ridge().spawns[1] + Vec2::from_int(-30, 0));
    let warlord = train(&mut w, P1, apa_factory, APA_WARLORD);
    w.submit(P0, CommandKind::Move { units: vec![scorpion], target: Vec2::from_int(200, 200) });
    w.submit(P1, CommandKind::Move { units: vec![warlord], target: Vec2::from_int(215, 200) });
    let mut landed = 0;
    while landed < 2 {
        w.step();
        landed += w.events().iter().filter(|e| matches!(e, Event::Hit { target, .. } if *target == scorpion)).count();
    }
    w.submit(P1, CommandKind::Move { units: vec![warlord], target: crater_ridge().spawns[1] });
    w.submit(P0, CommandKind::Stop { units: vec![scorpion] });
    let hit_at = w.tick();
    let hp_after_hits = w.entity(scorpion).unwrap().hp;
    // No healing inside the quiet spell (allow for one more hit in flight while it disengages).
    while w.tick() < hit_at + 30 * 2 {
        w.step();
    }
    let last_hit_hp = w.entity(scorpion).unwrap().hp;
    assert!(last_hit_hp <= hp_after_hits, "healed within 2 s of a hit");
    // Then 2 a second.
    while w.tick() < hit_at + 30 * 12 {
        w.step();
    }
    let healed = w.entity(scorpion).unwrap().hp - last_hit_hp;
    assert!((14..=20).contains(&healed), "healed {healed} in ~8-10 s of quiet");
    // And no further than full.
    for _ in 0..30 * 400 {
        w.step();
    }
    assert_eq!(w.entity(scorpion).unwrap().hp, rules().def(GLA_TANK).hp);
}

/// Firestorm Shells strengthen the Thunderfist's howitzer by half; its direct-fire cannon is
/// unchanged.
#[test]
fn a_shell_upgrade_leaves_the_direct_fire_gun_alone() {
    let mut w = world(Faction::Apa, Faction::Eu);
    let factory = build(&mut w, P0, APA_FACTORY, Vec2::from_int(110, 80));
    let tf = train(&mut w, P0, factory, APA_THUNDERFIST);
    research(&mut w, P0, factory, FIRESTORM_SHELLS);
    let cannon = rules().def(APA_THUNDERFIST).weapon().unwrap();
    let howitzer = rules().def(APA_THUNDERFIST).deploy.unwrap().weapon;
    // The EU dozer waits in open ground at the map centre and the Thunderfist drives up beside
    // it, inside the howitzer's minimum range: only the cannon can fire, at its normal damage.
    let dozer = first(&w, P1, EU_DOZER).unwrap();
    let centre = Vec2::from_int(200, 200);
    w.submit(P1, CommandKind::Move { units: vec![dozer], target: centre });
    step_until(&mut w, 30 * 60, "dozer at the centre", |w| {
        w.entity(dozer).unwrap().pos.distance(centre) < g2_sim::Fx::from_int(2)
    });
    assert_eq!(
        order(&mut w, P0, CommandKind::Move { units: vec![tf], target: centre + Vec2::from_int(-8, 0) }),
        vec![]
    );
    step_until(&mut w, 30 * 60, "Thunderfist beside it", |w| w.entity(tf).unwrap().order == g2_sim::Order::Idle);
    let close = hits(&mut w, tf, dozer, 30 * 6);
    assert!(
        !close.is_empty() && close.iter().all(|&(d, splash)| !splash && d == cannon.damage_vs(Armour::Heavy)),
        "{close:?}"
    );
    // The howitzer, deployed, on the EU command center.
    let cc = first(&w, P1, EU_CC).unwrap();
    let cc_pos = w.entity(cc).unwrap().pos;
    w.submit(P0, CommandKind::Move { units: vec![tf], target: cc_pos + Vec2::from_int(-50, -10) });
    step_until(&mut w, 30 * 120, "command center in sight", |w| w.vision(P0).is_visible(cc_pos));
    let shells: Vec<i32> = hits(&mut w, tf, cc, 30 * 40).iter().filter(|x| x.1).map(|x| x.0).collect();
    let want = (howitzer.damage + howitzer.bonus[Armour::Structure as usize]) * 150 / 100;
    assert!(!shells.is_empty() && shells.iter().all(|&d| d == want), "howitzer {shells:?}, want {want}");
}

/// Upgrades change the state hash only once researched, and a match with research replays
/// exactly.
#[test]
fn matches_with_upgrades_replay_exactly() {
    let s = MatchSetup {
        map: crater_ridge(),
        seed: 3,
        players: vec![
            PlayerSetup {
                faction: Faction::Eu,
                starting_credits: 20_000,
                ai: None,
                general: None,
                perks: Vec::new(),
                rank: 1,
                team: 0,
            },
            PlayerSetup {
                faction: Faction::Gla,
                starting_credits: 20_000,
                ai: Some(g2_sim::AiLevel::Normal),
                general: None,
                perks: Vec::new(),
                rank: 1,
                team: 0,
            },
        ],
    };
    let (end, replay) = record(World::new(&s, rules()), &s, 30 * 150, |w| {
        let mine = |def| w.entities().find(|e| e.owner == Some(P0) && e.def == def).map(|e| (e.id, e.is_complete()));
        match w.tick() {
            5 => {
                let builder = mine(EU_DOZER).unwrap().0;
                w.submit(P0, CommandKind::Build { builder, def: EU_TECH_LAB, pos: Vec2::from_int(110, 80) });
            }
            t if t > 5 && t % 30 == 0 => {
                if let Some((lab, true)) = mine(EU_TECH_LAB)
                    && w.entity(lab).unwrap().queue.is_empty()
                    && !w.player(P0).upgrades.contains(&SENIOR_WARFIGHTER)
                {
                    let next = [HIGHTECH_ALLOYS, IMPROVED_BALLISTICS, SENIOR_WARFIGHTER]
                        .into_iter()
                        .find(|&u| !w.researched_or_queued(P0, u));
                    if let Some(upgrade) = next {
                        w.submit(P0, CommandKind::Research { producer: lab, upgrade });
                    }
                }
            }
            _ => {}
        }
    });
    assert_eq!(end.player(P0).upgrades, vec![HIGHTECH_ALLOYS, IMPROVED_BALLISTICS, SENIOR_WARFIGHTER]);
    let decoded = g2_sim::Replay::decode(&replay.encode()).unwrap();
    assert!(decoded.commands.iter().any(|c| matches!(c.kind, CommandKind::Research { .. })));
    assert_eq!(verify_replay(&decoded, crater_ridge(), rules()), Ok(end.state_hash()));
}

/// A `Normal` AI builds a tech building and researches during a long match, for every faction
/// that has upgrades on offer; an `Easy` one never researches.
#[test]
fn the_normal_ai_researches() {
    for (f, level, want) in [
        (Faction::Eu, g2_sim::AiLevel::Normal, true),
        (Faction::Apa, g2_sim::AiLevel::Normal, true),
        (Faction::Gla, g2_sim::AiLevel::Normal, true),
        (Faction::Eu, g2_sim::AiLevel::Easy, false),
    ] {
        let s = MatchSetup {
            map: crater_ridge(),
            seed: 1,
            players: vec![
                PlayerSetup {
                    faction: f,
                    starting_credits: 10_000,
                    ai: Some(level),
                    general: None,
                    perks: Vec::new(),
                    rank: 1,
                    team: 0,
                },
                PlayerSetup {
                    faction: Faction::Eu,
                    starting_credits: 10_000,
                    ai: None,
                    general: None,
                    perks: Vec::new(),
                    rank: 1,
                    team: 0,
                },
            ],
        };
        let mut w = World::new(&s, rules());
        for _ in 0..30 * 600 {
            w.step();
            if !w.player(P0).upgrades.is_empty() {
                break;
            }
        }
        assert_eq!(!w.player(P0).upgrades.is_empty(), want, "{f:?} {level:?}: {:?}", w.player(P0).upgrades);
    }
}
