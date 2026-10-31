//! Built-in opponent. It runs inside the simulation and only issues ordinary commands, so a
//! replay of an AI match reproduces exactly without recording AI decisions separately.
//!
//! Both levels run the same economy and base layout. `Easy` (the original AI) sends its army
//! in waves of five, defends with the units at home, fires a ready strike at a group of three
//! or more enemies it sees and pulls a constructor that is being hit back home. `Normal`
//! judges the battle first: it keeps an estimate of the enemy army it has seen, attacks only
//! with an army that outweighs it, pulls units out of a fight they are losing, calls the
//! field army home when the base is attacked by more than its guards can hold, keeps its
//! guns at the far end of their band, routes constructors home around enemies, builds more
//! factories when credits pile up and opens a second supply center when the base's piles run
//! low.

use crate::command::CommandKind;
use crate::defs::{Armour, BuildingRole, Class, DefId, GeneralId, PerkId, Power, PowerKind, Rules, Weapon};
use crate::entity::{DeployState, Entity, EntityId, Order, PlayerId};
use crate::fixed::{Fx, Vec2};
use crate::map::AiLevel;
use crate::world::{TICKS_PER_SECOND, World};

/// Decisions every second, not every tick: cheaper and closer to a human's pace.
const THINK_INTERVAL: u32 = TICKS_PER_SECOND;
/// Idle combat units gather at the factory until there are this many, then attack together.
const WAVE: usize = 5;
/// Normal's first waves are bigger: around the alpha's larger buildings a wave of five met
/// defenders it couldn't beat.
const NORMAL_WAVE: usize = 7;
/// Enemy units this close to the edge of one of our buildings trigger the whole army's
/// defence; the zone grows with the buildings themselves.
const DEFEND_RADIUS: Fx = Fx::from_int(40);
/// A wave's own fight: enemies this close to its centre count against it.
const BATTLE_RADIUS: Fx = Fx::from_int(60);
/// Normal opens its second supply center by this tick even with its home pile still full: an
/// economy that waits for the pile to run dry falls behind the Onslaught's later waves.
const EXPAND_BY: u32 = 300 * TICKS_PER_SECOND;
/// Constructors move home when an armed enemy comes this close.
const DANGER_RADIUS: Fx = Fx::from_int(45);

pub struct Ai {
    player: PlayerId,
    level: AiLevel,
    /// Combat strength of the largest enemy army seen lately (see `strength`), fading by a
    /// 64th every second, so an army out of sight still counts for a while.
    enemy_seen: i64,
    /// No new wave before this tick: set by a retreat, so the army regroups first.
    regroup_until: u32,
}

/// The general rank an AI plays at: `Easy` at 1, like a new player; `Normal` and `Onslaught`
/// at 3, matching the perks they slot (`ai_perks`), so their power is at its second level.
pub fn ai_rank(level: AiLevel) -> u8 {
    if level == AiLevel::Easy { 1 } else { 3 }
}

/// What an AI general slots: `Easy` plays without perks; `Normal` and `Onslaught` slot as a
/// rank 3 player would, the first three of the general's perks open by rank 3 whose lower tier
/// is also slotted.
pub fn ai_perks(rules: &Rules, general: Option<GeneralId>, level: AiLevel) -> Vec<PerkId> {
    let Some(g) = general else { return Vec::new() };
    if level == AiLevel::Easy {
        return Vec::new();
    }
    let mut out: Vec<PerkId> = Vec::new();
    for &k in rules.general(g).perks {
        let p = rules.perk(k);
        if out.len() < 3 && p.rank <= 3 && p.requires.is_none_or(|r| out.contains(&r)) {
            out.push(k);
        }
    }
    out
}

/// What a unit adds to a fight: hit points times damage per second, so a fresh tank counts
/// for more than a damaged one and a fast gun for more than a slow one, with its owner's
/// upgrades and its rank. Unarmed units and every building (a base defence holds its ground,
/// it is no army to outweigh): 0.
pub fn strength(world: &World, e: &Entity) -> i64 {
    let d = world.rules.def(e.def);
    if !d.is_unit() {
        return 0;
    }
    let Some(w) = d.weapon_for(e.deploy == DeployState::Deployed).or(d.deploy.map(|dep| dep.weapon)) else {
        return 0;
    };
    let w = world.unit_weapon(e, w);
    e.hp.max(0) as i64 * w.damage as i64 * TICKS_PER_SECOND as i64 / w.cooldown_ticks.max(1) as i64
}

/// The armour class (index into `Weapon::bonus`) a weapon is strongest against; a weapon with
/// no bonus against any unit armour is a general-purpose one (`usize::MAX`).
fn best_against(w: Weapon) -> usize {
    (0..3).filter(|&a| w.bonus[a] > 0).max_by_key(|&a| (w.bonus[a], std::cmp::Reverse(a))).unwrap_or(usize::MAX)
}

impl Ai {
    pub fn new(player: PlayerId, level: AiLevel) -> Ai {
        Ai { player, level, enemy_seen: 0, regroup_until: 0 }
    }

    pub fn level(&self) -> AiLevel {
        self.level
    }

    /// Hash input: the state a `Normal` AI decides on. `Easy` and `Onslaught` keep none, so
    /// they add nothing.
    pub(crate) fn hash_into(&self, mut put: impl FnMut(u64)) {
        if self.level == AiLevel::Normal {
            put(self.enemy_seen as u64);
            put(self.regroup_until as u64);
        }
    }

    /// The armour most of the visible enemy army (by combat strength) wears, if any is seen.
    fn enemy_armour(&self, world: &World) -> Option<Armour> {
        let mut by = [0i64; 3];
        for e in world.entities().filter(|e| world.hostile(self.player, e.owner) && world.sees(self.player, e)) {
            let a = world.rules.def(e.def).armour;
            if a != Armour::Structure {
                by[a as usize] += strength(world, e);
            }
        }
        let (i, &s) = by.iter().enumerate().max_by_key(|&(i, &s)| (s, std::cmp::Reverse(i)))?;
        (s > 0).then_some([Armour::Infantry, Armour::Light, Armour::Heavy][i])
    }

    /// Whether any visible enemy unit flies.
    fn enemy_air(&self, world: &World) -> bool {
        world.entities().any(|e| {
            world.hostile(self.player, e.owner) && world.rules.def(e.def).flies() && world.sees(self.player, e)
        })
    }

    /// Where to use `power` now, if it is worth it: a barrage, an airstrike or a toxin bomb on
    /// the visible enemy army's strongest spot; a repair on the spot where our own units and
    /// buildings have lost the most hit points (at least 600); an ambush beside the nearest
    /// visible enemy building, else in front of our own base; a superweapon on the remembered
    /// enemy base; the general's own cloak over our largest group of combat units out in the
    /// field. Never a recon: flown over the enemy start, its early look sent Normal's waves at
    /// the base too soon (Normal won 36 of 54 against Easy with it, 40 without, and lost 12 of
    /// 15 field guns instead of under two thirds). Never the faction's cloak (the GLA's GPS
    /// Scrambler, `slot` above 0): a wide cover for a base, which Normal has no plan for.
    fn power_target(&self, world: &World, mine: &[&Entity], slot: u8, power: Power) -> Option<Vec2> {
        let player = world.player(self.player);
        let vision = world.vision(self.player);
        // The best of `points` by the sum of `weight` within `r` of each.
        let best = |points: &[Vec2], weight: &dyn Fn(Vec2) -> i64| {
            points.iter().map(|&p| (weight(p), p)).filter(|&(w, _)| w > 0).max_by_key(|&(w, p)| (w, p.x, p.y))
        };
        match power.kind {
            PowerKind::Recon { .. } => None,
            PowerKind::Cloak { .. } if slot > 0 => None,
            // A cloak over our largest group of combat units out in the field (at least three,
            // away from home), so they close in unseen.
            PowerKind::Cloak { radius, .. } => {
                let home =
                    mine.iter().find(|e| world.rules.def(e.def).role() == Some(BuildingRole::CommandCenter))?.pos;
                let field: Vec<&&Entity> = mine
                    .iter()
                    .filter(|e| {
                        let d = world.rules.def(e.def);
                        d.is_unit()
                            && d.weapon().is_some()
                            && e.inside.is_none()
                            && e.pos.distance(home) > Fx::from_int(80)
                    })
                    .collect();
                let points: Vec<Vec2> = field.iter().map(|e| e.pos).collect();
                let weight = |p: Vec2| field.iter().filter(|e| e.pos.distance(p) <= radius).count() as i64;
                best(&points, &weight).filter(|&(w, _)| w >= 3).map(|(_, p)| p)
            }
            // The superweapon: on the enemy base, at the remembered building with the most
            // building hit points around it within the cloud.
            PowerKind::Missiles { cloud_radius, .. } if power.requires.is_some() => {
                let seen: Vec<(Vec2, i64)> = vision
                    .memory
                    .values()
                    .filter(|s| world.hostile(self.player, Some(s.owner)))
                    .map(|s| (s.pos, world.rules.def(s.def).hp as i64))
                    .collect();
                let points: Vec<Vec2> = seen.iter().map(|&(p, _)| p).collect();
                let weight =
                    |p: Vec2| seen.iter().filter(|&&(q, _)| q.distance(p) <= cloud_radius).map(|&(_, hp)| hp).sum();
                best(&points, &weight).map(|(_, p)| p)
            }
            PowerKind::Barrage { radius, shells, .. }
            | PowerKind::Airstrike { radius, bombs: shells, .. }
            | PowerKind::Missiles { radius, missiles: shells, .. } => {
                let enemies: Vec<&Entity> = world
                    .entities()
                    .filter(|e| world.hostile(self.player, e.owner) && world.sees(self.player, e))
                    .collect();
                let points: Vec<Vec2> = enemies.iter().map(|e| e.pos).collect();
                // A pattern of shells covers about three radii; a single blast just its own.
                let reach = if shells > 1 { radius * Fx::from_int(3) } else { radius };
                let weight =
                    |p: Vec2| enemies.iter().filter(|e| e.pos.distance(p) <= reach).map(|e| strength(world, e)).sum();
                // Worth a barrage: at least two of our faction's strongest standard units' (at
                // full health, undeployed) worth of enemy fighting strength in the blast area.
                // Fixed by the rules, not the general: an elite general's unit (a railgun tank)
                // would set the bar out of reach of its own general's strike.
                let tank = world
                    .rules
                    .defs
                    .iter()
                    .filter(|d| d.faction == Some(player.faction) && d.is_building())
                    .flat_map(|b| b.produces.iter().map(|&u| world.rules.def(u)))
                    .filter(|d| d.is_unit())
                    .filter_map(|d| {
                        d.weapon_for(false).map(|w| {
                            d.hp as i64 * w.damage as i64 * TICKS_PER_SECOND as i64 / w.cooldown_ticks.max(1) as i64
                        })
                    })
                    .max()
                    .unwrap_or(1);
                best(&points, &weight).filter(|&(w, _)| w >= 2 * tank).map(|(_, p)| p)
            }
            PowerKind::Repair { radius, .. } => {
                let points: Vec<Vec2> = mine.iter().map(|e| e.pos).collect();
                let weight = |p: Vec2| {
                    mine.iter()
                        .filter(|e| e.pos.distance(p) <= radius)
                        .map(|e| (world.max_hp(e) - e.hp).max(0) as i64)
                        .sum()
                };
                best(&points, &weight).filter(|&(w, _)| w >= 600).map(|(_, p)| p)
            }
            PowerKind::Ambush { .. } => {
                let home =
                    mine.iter().find(|e| world.rules.def(e.def).role() == Some(BuildingRole::CommandCenter))?.pos;
                let target = world
                    .entities()
                    .filter(|e| {
                        world.hostile(self.player, e.owner)
                            && world.rules.def(e.def).is_building()
                            && world.sees(self.player, e)
                    })
                    .min_by_key(|e| (e.pos.distance(home), e.id))
                    .map(|e| {
                        let delta = home - e.pos;
                        let gap = world.rules.def(e.def).radius + Fx::from_int(10);
                        e.pos + delta.scale(gap / delta.length().max(Fx::from_int(1)))
                    });
                target
                    .or_else(|| {
                        let toward = other_starts(world, self.player, home)
                            .into_iter()
                            .max_by_key(|&s| (s.distance(home), s.x, s.y))?;
                        let delta = toward - home;
                        Some(home + delta.scale(Fx::from_int(25) / delta.length().max(Fx::from_int(1))))
                    })
                    .filter(|&p| world.map.walkable(p) && vision.is_explored(p))
            }
        }
    }

    /// `Easy`'s simpler use of powers: only the ones that hit (a barrage, an airstrike, a
    /// missile strike), on the visible enemy unit or building with the most visible enemies,
    /// by count, within the blast area, at least three. No weighing of strength and no recon,
    /// repair, ambush or cloak (so never the GLA's GPS Scrambler). Measured, Normal against
    /// Easy on all 86 skirmish maps: 1,392 to 151 (1,397 to 147 without); aiming only at armed
    /// units or waiting for five changed no more than 3 matches.
    fn easy_power_target(&self, world: &World, power: Power) -> Option<Vec2> {
        let reach = match power.kind {
            PowerKind::Barrage { radius, shells, .. }
            | PowerKind::Airstrike { radius, bombs: shells, .. }
            | PowerKind::Missiles { radius, missiles: shells, .. } => {
                if shells > 1 {
                    radius * Fx::from_int(3)
                } else {
                    radius
                }
            }
            _ => return None,
        };
        let enemies: Vec<Vec2> = world
            .entities()
            .filter(|e| world.hostile(self.player, e.owner) && e.hp > 0 && world.sees(self.player, e))
            .map(|e| e.pos)
            .collect();
        let count = |p: Vec2| enemies.iter().filter(|q| q.distance(p) <= reach).count();
        enemies
            .iter()
            .map(|&p| (count(p), p))
            .filter(|&(n, _)| n >= 3)
            .max_by_key(|&(n, p)| (n, p.x, p.y))
            .map(|(_, p)| p)
    }

    pub(crate) fn think(&mut self, world: &World, out: &mut Vec<CommandKind>) {
        if world.tick() % THINK_INTERVAL != self.player.0 as u32 % THINK_INTERVAL {
            return;
        }
        if self.level == AiLevel::Onslaught {
            self.onslaught(world, out);
            return;
        }
        let me = self.player;
        let rules = world.rules;
        let mine: Vec<_> = world.entities().filter(|e| e.owner == Some(me)).collect();
        let cc = mine.iter().find(|e| rules.def(e.def).role() == Some(BuildingRole::CommandCenter));
        // Endangered constructors get away first; one sent off this second builds nothing.
        let fleeing = self.protect_constructors(world, &mine, cc.map(|c| c.pos), out);
        let idle_constructor = mine
            .iter()
            .find(|e| rules.def(e.def).is_constructor() && e.order == Order::Idle && !fleeing.contains(&e.id))
            .map(|e| e.id);
        let count =
            |role: fn(BuildingRole) -> bool| mine.iter().filter(|e| rules.def(e.def).role().is_some_and(role)).count();

        // Before anything that may return early: ready powers are used at once, one a think.
        let ready = world.powers_of(me).into_iter().enumerate().filter_map(|(slot, p)| {
            let p = p.filter(|_| world.power_wait(me, slot as u8) == Some(0))?;
            Some((slot as u8, p))
        });
        for (slot, p) in ready {
            let at = if self.level == AiLevel::Easy {
                self.easy_power_target(world, p)
            } else {
                self.power_target(world, &mine, slot, p)
            };
            if let Some(at) = at {
                out.push(CommandKind::Power { slot, at });
                break;
            }
        }
        // 0. Never be left without a constructor: one lost (or never built) is retrained, and
        //    until it is queued the army waits, so its credits can't keep the rebuild away.
        let constructors = mine.iter().filter(|e| rules.def(e.def).is_constructor()).count()
            + mine
                .iter()
                .flat_map(|e| &e.queue)
                .filter(|p| p.unit().is_some_and(|d| rules.def(d).is_constructor()))
                .count();
        let rebuild = mine.iter().filter(|e| e.is_complete()).find_map(|e| {
            rules.def(e.def).produces.iter().find(|&&u| rules.def(u).is_constructor()).map(|&u| (e.id, u))
        });
        // Once a stealth unit has hit it, Normal (whose factories all stand) also holds back the
        // army until its detector is placed, so its credits can't keep the turret away.
        let needs_detector = self.level == AiLevel::Normal
            && world.player(me).hit_by_stealth
            && !mine.iter().any(|e| rules.def(e.def).role() == Some(BuildingRole::Defense))
            && mine.iter().filter(|e| rules.def(e.def).role() == Some(BuildingRole::Factory)).count() >= 2;
        let saving = (constructors == 0 && rebuild.is_some()) || needs_detector;
        if constructors == 0
            && let Some((producer, def)) = rebuild
            && world.player(me).credits >= rules.def(def).cost.credits
        {
            out.push(CommandKind::Train { producer, def });
        }

        // 1. Finish any abandoned foundation first.
        if let (Some(builder), Some(site)) = (idle_constructor, mine.iter().find(|e| e.construction.is_some())) {
            out.push(CommandKind::Construct { units: vec![builder], site: site.id });
            return;
        }

        // 2. Base layout: supply center, power (if the faction uses it), one oil derrick, a
        //    barracks and a war factory; then derricks on any other free well.
        // A player whose command center is gone rebuilds around its constructor, so a lost
        // base is replaced rather than left standing forever as idle constructors.
        let anchor = cc.map(|c| c.pos).or_else(|| idle_constructor.and_then(|b| world.entity(b)).map(|e| e.pos));
        if let (Some(builder), Some(base)) = (idle_constructor, anchor) {
            // What this builder places for us, with our general's defences.
            let builds = world.offers_of(me, world.entity(builder).unwrap().def);
            let find = |pred: fn(&crate::defs::Def) -> bool| builds.iter().copied().find(|&b| pred(rules.def(b)));
            let derrick = find(|d| matches!(d.role(), Some(BuildingRole::OilDerrick { .. })));
            let derricks = count(|r| matches!(r, BuildingRole::OilDerrick { .. }));
            // Barracks first: the faction's factory producing the cheaper unit. Factories that
            // train only aircraft (airfields) aren't part of the core base.
            let air_only = |b: crate::defs::DefId| {
                let p = rules.def(b).produces;
                !p.is_empty() && p.iter().all(|&u| rules.def(u).flies())
            };
            let factories: Vec<_> = {
                let mut f: Vec<_> = builds
                    .iter()
                    .copied()
                    .filter(|&b| rules.def(b).role() == Some(BuildingRole::Factory) && !air_only(b))
                    .collect();
                f.sort_by_key(|&b| rules.def(b).produces.first().map_or(0, |&u| rules.def(u).cost.credits));
                f
            };
            // Normal: one airfield, once credits pile up with the core factories standing.
            let airfield = builds.iter().copied().filter(|&b| air_only(b)).find(|&b| !mine.iter().any(|e| e.def == b));
            // Normal: one superweapon, last of all, from a surplus well above its price.
            let superweapon = builds
                .iter()
                .copied()
                .find(|&b| rules.def(b).role() == Some(BuildingRole::Superweapon))
                .filter(|&b| !mine.iter().any(|e| e.def == b))
                .filter(|&b| world.player(me).credits >= rules.def(b).cost.credits + 2_000);
            let missing_factory = factories.iter().copied().find(|&f| !mine.iter().any(|e| e.def == f));
            let depots: Vec<&&Entity> = mine
                .iter()
                .filter(|e| matches!(rules.def(e.def).role(), Some(BuildingRole::SupplyCenter { .. })))
                .collect();
            let supply = find(|d| matches!(d.role(), Some(BuildingRole::SupplyCenter { .. })));
            let smart = self.level != AiLevel::Easy;
            // Normal: expand to the nearest pile with no supply center of our side close by (an
            // ally's pile is theirs), once the base's own pile (within 88 of the command
            // center's edge) is nearly spent, or by `EXPAND_BY`; add a factory when credits pile up.
            let home_reach = Fx::from_int(88) + cc.map_or(Fx::ZERO, |c| rules.def(c.def).radius);
            let expansion = || {
                let near_ours = |p: Vec2| {
                    world.entities().any(|d| {
                        d.owner.is_some_and(|o| world.allied(me, o))
                            && matches!(rules.def(d.def).role(), Some(BuildingRole::SupplyCenter { .. }))
                            && d.pos.distance(p) <= Fx::from_int(60)
                    })
                };
                let spent = world.tick() >= EXPAND_BY
                    || world
                        .nearest_pile(base)
                        .and_then(|p| world.entity(p))
                        .is_none_or(|p| p.resource < 2_000 || p.pos.distance(base) > home_reach);
                spent
                    .then(|| {
                        world
                            .entities()
                            .filter(|e| matches!(rules.def(e.def).class, Class::SupplyPile) && e.resource >= 2_000)
                            .filter(|e| !near_ours(e.pos))
                            .min_by_key(|e| (e.pos.distance(base), e.id))
                            .map(|e| e.pos)
                    })
                    .flatten()
            };
            let expand_to = if smart && depots.len() < 3 { expansion() } else { None };
            let rich = world.player(me).credits >= 3_000
                && mine.iter().filter(|e| rules.def(e.def).role() == Some(BuildingRole::Factory)).count() < 4;
            // Normal: each tech building once, cheapest first, once credits pile up with every
            // factory it wants already standing (its army comes first).
            let lab = {
                let mut labs: Vec<_> =
                    builds.iter().copied().filter(|&b| rules.def(b).role() == Some(BuildingRole::Lab)).collect();
                labs.sort_by_key(|&b| rules.def(b).cost.credits);
                labs.into_iter().find(|&l| !mine.iter().any(|e| e.def == l))
            };
            // Normal: one base defence, its only detector: as soon as its factories stand once
            // a stealth unit has hit it, else after its tech buildings (earlier, it cost wins).
            let turret = builds
                .iter()
                .copied()
                .find(|&b| rules.def(b).role() == Some(BuildingRole::Defense))
                .filter(|&b| !mine.iter().any(|e| e.def == b));
            let want = if depots.is_empty() {
                supply
            } else if world.player(me).faction.uses_power() && count(|r| r == BuildingRole::PowerPlant) == 0 {
                find(|d| d.role() == Some(BuildingRole::PowerPlant))
            } else if derricks == 0 && derrick.is_some_and(|d| self.free_well(world, d, base).is_some()) {
                derrick
            } else if let Some(f) = missing_factory {
                Some(f)
            } else if expand_to.is_some() {
                supply
            } else if smart && world.player(me).hit_by_stealth && turret.is_some() {
                turret
            } else if smart && rich {
                factories.last().copied()
            } else if smart && lab.is_some() && world.player(me).credits >= 3_000 {
                lab
            } else if smart && turret.is_some() && world.player(me).credits >= 3_000 {
                turret
            } else if smart && airfield.is_some() && world.player(me).credits >= 3_000 {
                airfield
            } else if smart && superweapon.is_some() {
                superweapon
            } else {
                derrick
            };
            if let Some(def) = want
                && world.player(me).credits >= rules.def(def).cost.credits
            {
                let pos = if Some(def) == derrick {
                    self.free_well(world, def, base)
                } else if matches!(rules.def(def).role(), Some(BuildingRole::SupplyCenter { .. })) {
                    match expand_to.filter(|_| !depots.is_empty()) {
                        // Beside the new pile, toward home.
                        Some(pile) => self.find_spot_toward(world, def, pile, base),
                        // Toward the nearest pile: shorter trips, and the same layout from every
                        // start (the spiral's fixed ring order favours one corner of the map).
                        None => {
                            let toward = world.nearest_pile(base).and_then(|p| world.entity(p)).map_or(base, |p| p.pos);
                            self.find_spot_toward(world, def, base, toward)
                        }
                    }
                } else {
                    self.find_spot(world, def, base)
                };
                if let Some(pos) = pos {
                    out.push(CommandKind::Build { builder, def, pos });
                    return;
                }
            }
        }

        // 3. Fill every supply center up to its collector cap, while any pile has supplies left:
        //    once the last is spent, collectors go idle and leave their center, which then looks
        //    short of collectors, and every credit from oil went into new ones (Easy APA against
        //    EU on Crater Ridge had 18 and no army at 15:00, and neither side ever won).
        let piles_left =
            world.entities().any(|e| matches!(rules.def(e.def).class, Class::SupplyPile) && e.resource > 0);
        for depot in mine.iter().filter(|e| piles_left && e.is_complete()) {
            let d = rules.def(depot.def);
            let Some(BuildingRole::SupplyCenter { collector_cap }) = d.role() else { continue };
            let Some(&collector) = d.produces.iter().find(|&&u| rules.def(u).collector().is_some()) else { continue };
            if world.depot_load(depot.id) < collector_cap
                && world.player(me).credits >= rules.def(collector).cost.credits
            {
                out.push(CommandKind::Train { producer: depot.id, def: collector });
            }
        }

        // 3b. Keep every factory training (one at a time, so collectors come first). A factory
        // trains the unit this AI owns fewest of, its first (the line unit) weighted double, so
        // a barracks trains two rifle squads per rocket squad and a war factory two tanks per
        // artillery piece and per light vehicle. `Normal` then counters what it has seen: while
        // an enemy aircraft is in sight, a unit that fires at aircraft counts as owned only half
        // as much and wins ties; otherwise so does a unit whose weapon is strongest against the
        // armour most of the visible enemy army wears.
        let owned = |def| {
            mine.iter().filter(|e| e.def == def).count()
                + mine.iter().flat_map(|e| &e.queue).filter(|p| p.unit() == Some(def)).count()
        };
        let smart = self.level != AiLevel::Easy;
        let air = smart && self.enemy_air(world);
        let counter = (smart && !air).then(|| self.enemy_armour(world)).flatten();
        let counters = |u: DefId| {
            rules.def(u).weapon().is_some_and(|w| {
                if air { w.targets.hits(true) } else { counter.is_some_and(|a| best_against(w) == a as usize) }
            })
        };
        for f in mine.iter().filter(|e| !saving && e.is_complete() && e.queue.is_empty()) {
            if rules.def(f.def).role() != Some(BuildingRole::Factory) {
                continue;
            }
            let produces = world.offers_of(me, f.def);
            let pick = produces
                .iter()
                .enumerate()
                .min_by_key(|&(i, &u)| {
                    let weight = if i == 0 { 2 } else { 4 };
                    let counter = counters(u);
                    let weight = if counter { weight / 2 } else { weight };
                    (owned(u) * weight, !counter, i)
                })
                .map(|(_, &u)| u);
            if let Some(unit) = pick
                && world.player(me).credits >= rules.def(unit).cost.credits
            {
                out.push(CommandKind::Train { producer: f.id, def: unit });
            }
        }

        // 3c. Normal: research what its buildings offer, in table order, one at a time and only
        // from credits beyond a reserve, so the army it trains above keeps coming.
        if self.level == AiLevel::Normal && !saving {
            let busy = mine.iter().any(|e| e.queue.iter().any(|p| p.unit().is_none()));
            let offer = mine.iter().filter(|e| e.is_complete()).find_map(|b| {
                let u = rules.def(b.def).researches.iter().copied().find(|&u| !world.researched_or_queued(me, u))?;
                Some((b.id, u))
            });
            if let Some((producer, upgrade)) = offer
                && !busy
                && world.player(me).credits >= rules.upgrade(upgrade).cost.credits + 2_000
            {
                out.push(CommandKind::Research { producer, upgrade });
            }
        }

        // 4. Put idle collectors back to work (not a GLA worker just sent away from danger).
        let idle: Vec<EntityId> = mine
            .iter()
            .filter(|e| rules.def(e.def).collector().is_some() && e.order == Order::Idle && !fleeing.contains(&e.id))
            .map(|e| e.id)
            .collect();
        if let (Some(&first), false) = (idle.first(), idle.is_empty())
            && let Some(pile) = world.nearest_pile(world.entity(first).unwrap().pos)
        {
            out.push(CommandKind::Gather { units: idle, pile });
        }

        // 5. Army: armed units (not base defences, which don't move). Units inside a garrison
        // hold it; they aren't sent anywhere.
        let army: Vec<_> = mine
            .iter()
            .copied()
            .filter(|e| rules.def(e.def).is_unit() && rules.def(e.def).weapon().is_some() && e.inside.is_none())
            .collect();
        let home = cc.map(|c| c.pos).or_else(|| mine.first().map(|e| e.pos));
        let placed = self.place_guns(world, &army, home, out);
        let army: Vec<_> = army.into_iter().filter(|e| !placed.contains(&e.id)).collect();
        if self.level == AiLevel::Easy {
            self.waves(world, &mine, &army, home, out);
        } else {
            self.command_army(world, &mine, &army, home, out);
        }
    }

    /// `Normal`'s artillery (a unit whose main gun fires shells), placed on purpose from its
    /// band, the main gun's minimum range to its range; returns the guns given an order this
    /// second, which the army leaves alone until the next. Against the nearest armed ground
    /// enemy it sees: inside the minimum range plus 5, a gun that fires on the move steps back
    /// (as far as the middle of its band) along a way clear of the enemies in its reach
    /// (`escape`), and a deployed one packs up to its direct-fire gun; nearer than the middle
    /// of its band, a packed gun steps back to 5 inside its range, behind its line; standing
    /// idle with the enemy in its band, a deployable gun sets up. Measured on its own, Normal
    /// against Easy on all 86 skirmish maps (Easy firing its powers): Normal lost 1,016 of
    /// 8,162 guns (1,425 of 8,429 without) and won 1,396 to 151, 1 undecided (1,392 to 151,
    /// 5). Also tried: re-aiming a ready gun at the enemy with the most units around it (lost
    /// 1,073 and left 7 matches undecided), and stepping a deployed gun back instead of
    /// packing it up (no change).
    fn place_guns(
        &self,
        world: &World,
        army: &[&Entity],
        home: Option<Vec2>,
        out: &mut Vec<CommandKind>,
    ) -> Vec<EntityId> {
        let (me, rules) = (self.player, world.rules);
        let mut sent = Vec::new();
        let Some(home) = home.filter(|_| self.level == AiLevel::Normal) else { return sent };
        let armed: Vec<&Entity> = world
            .entities()
            .filter(|e| {
                world.hostile(me, e.owner)
                    && e.inside.is_none()
                    && !rules.def(e.def).flies()
                    && strength(world, e) > 0
                    && world.sees(me, e)
            })
            .collect();
        for g in army {
            let d = rules.def(g.def);
            let Some(w) = d.weapon_for(d.deploy.is_some()) else { continue };
            let Some(shell) = w.shell else { continue };
            let gap = |e: &Entity| e.pos.distance(g.pos) - rules.def(e.def).radius;
            let Some(near) = armed.iter().map(|e| gap(e)).min() else { continue };
            let nearest = armed.iter().filter(|e| gap(e) == near).min_by_key(|e| e.id).unwrap();
            let moving = matches!(g.order, Order::Move { .. });
            let middle = (shell.min_range + w.range) / Fx::from_int(2);
            let order = if near < shell.min_range + Fx::from_int(5) {
                if d.deploy.is_some() {
                    (g.deploy == DeployState::Deployed)
                        .then(|| CommandKind::Deploy { units: vec![g.id], deployed: false })
                } else {
                    let threats: Vec<Vec2> = armed.iter().filter(|e| gap(e) <= w.range).map(|e| e.pos).collect();
                    (!moving).then(|| CommandKind::Move {
                        units: vec![g.id],
                        target: escape(world, g.pos, home, &threats, middle),
                    })
                }
            } else if near < middle && g.deploy == DeployState::Packed && !moving {
                let away = g.pos - nearest.pos;
                let hold = w.range - Fx::from_int(5);
                let target = world.map.clamp(g.pos + away.scale((hold - near) / away.length().max(Fx::ONE)));
                Some(CommandKind::Move { units: vec![g.id], target })
            } else if d.deploy.is_some() && g.deploy == DeployState::Packed && g.order == Order::Idle && near <= w.range
            {
                Some(CommandKind::Deploy { units: vec![g.id], deployed: true })
            } else {
                None
            };
            if let Some(order) = order {
                out.push(order);
                sent.push(g.id);
            }
        }
        sent
    }

    /// Constructors in danger get away; returns the ones sent off. `Normal` moves one away as
    /// soon as an armed enemy it sees comes within `DANGER_RADIUS`, home along a way that
    /// keeps clear of every armed enemy it sees nearby (`escape`), and changes course when one
    /// of them comes within 20 of the way it is taking. `Easy` only pulls one that was hit in
    /// the last 2 s straight back home. Measured on their own, Normal against Easy on all 86
    /// skirmish maps: Normal lost 264 constructors (281 going straight home), Easy 3,550
    /// (4,665 left alone); Easy against Easy lost 2,348 (3,318), and Normal won 1,390 to 153
    /// (1,397 to 147).
    fn protect_constructors(
        &self,
        world: &World,
        mine: &[&Entity],
        home: Option<Vec2>,
        out: &mut Vec<CommandKind>,
    ) -> Vec<EntityId> {
        let (me, rules) = (self.player, world.rules);
        let mut sent = Vec::new();
        let Some(home) = home.or_else(|| mine.first().map(|e| e.pos)) else { return sent };
        let armed: Vec<&Entity> = world
            .entities()
            .filter(|e| world.hostile(me, e.owner) && strength(world, e) > 0 && world.sees(me, e))
            .collect();
        for c in mine.iter().filter(|e| rules.def(e.def).is_constructor() && e.inside.is_none()) {
            if c.pos.distance(home) <= Fx::from_int(20) {
                continue;
            }
            let target = if self.level == AiLevel::Easy {
                let hit = c.last_hit.is_some_and(|h| world.tick() < h + 2 * THINK_INTERVAL);
                (hit && !matches!(c.order, Order::Move { .. })).then_some(home)
            } else {
                let threats: Vec<Vec2> = armed
                    .iter()
                    .filter(|e| e.pos.distance(c.pos) <= DANGER_RADIUS * Fx::from_int(2))
                    .map(|e| e.pos)
                    .collect();
                let close = threats.iter().any(|t| t.distance(c.pos) <= DANGER_RADIUS);
                let blocked = match c.order {
                    Order::Move { target, .. } => {
                        threats.iter().any(|&t| clearance(c.pos, target, t) < Fx::from_int(20))
                    }
                    _ => true,
                };
                (close && blocked).then(|| escape(world, c.pos, home, &threats, DANGER_RADIUS))
            };
            if let Some(target) = target
                && !matches!(c.order, Order::Move { target: t, .. } if t == target)
            {
                out.push(CommandKind::Move { units: vec![c.id], target });
                sent.push(c.id);
            }
        }
        sent
    }

    /// `Easy`: defend with the units at home, attack in waves of five. Only visible, armed
    /// intruders call the defence: a passing harvester or dozer is no threat, and idle guards
    /// shoot it anyway if it comes within range.
    fn waves(&self, world: &World, mine: &[&Entity], army: &[&Entity], home: Option<Vec2>, out: &mut Vec<CommandKind>) {
        let (me, rules) = (self.player, world.rules);
        let intruder = world.entities().find(|e| {
            world.hostile(me, e.owner)
                && rules.def(e.def).is_unit()
                && rules.def(e.def).weapon().is_some()
                && world.sees(me, e)
                && near_base(world, mine, e.pos)
        });
        if let Some(intruder) = intruder {
            // Only units at home defend: a wave already under way keeps going, and a unit
            // already fighting keeps its target.
            let home: Vec<EntityId> = army
                .iter()
                .filter(|e| {
                    !matches!(e.order, Order::Attack { .. } | Order::AttackMove { engaged: Some(_), .. })
                        && near_base(world, mine, e.pos)
                })
                .map(|e| e.id)
                .collect();
            if !home.is_empty() {
                out.push(CommandKind::Attack { units: home, target: intruder.id });
                // Those units are busy this second; the wave check waits for the next.
                return;
            }
        }
        let idle: Vec<EntityId> = army.iter().filter(|e| e.order == Order::Idle).map(|e| e.id).collect();
        // With every pile spent and no derrick of ours, the army can't grow to a wave any more:
        // it goes as it is, or a match between two spent bases never ends (Easy APA against EU on
        // Crater Ridge, once the EU harvester carried the alpha's 190 a trip: three EU units
        // waited at home from 7:00 on).
        let income = mine.iter().any(|e| matches!(rules.def(e.def).role(), Some(BuildingRole::OilDerrick { .. })))
            || world.entities().any(|e| matches!(rules.def(e.def).class, Class::SupplyPile) && e.resource > 0);
        if (idle.len() >= WAVE || (!income && !idle.is_empty()))
            && let Some(home) = home
            && let Some(target) = self.enemy_base(world, home)
        {
            out.push(CommandKind::AttackMove { units: idle, target });
        }
    }

    /// `Onslaught`: no economy, no base building. Every idle unit (a wave the sim has just
    /// placed, or what is left of one) attack-moves on the nearest enemy building it knows, else
    /// scouts for one, so every wave reaches the player. Without a start of its own (co-op, the
    /// waves coming from `World::onslaught_origin`) each wave splits evenly between the enemy
    /// players still in the game, in unit order, so no defender is left out.
    fn onslaught(&self, world: &World, out: &mut Vec<CommandKind>) {
        let (me, rules) = (self.player, world.rules);
        let idle: Vec<EntityId> = world
            .entities()
            .filter(|e| {
                e.owner == Some(me)
                    && e.order == Order::Idle
                    && rules.def(e.def).is_unit()
                    && rules.def(e.def).weapon().is_some()
                    && e.inside.is_none()
            })
            .map(|e| e.id)
            .collect();
        if idle.is_empty() {
            return;
        }
        let Some(home) = world.start(me) else {
            let origin = world.onslaught_origin();
            let targets: Vec<PlayerId> = (0..world.players().len() as u8)
                .map(PlayerId)
                .filter(|&p| world.hostile(me, Some(p)) && !world.player(p).defeated && world.start(p).is_some())
                .collect();
            for (k, &p) in targets.iter().enumerate() {
                let units: Vec<EntityId> = idle.iter().skip(k).step_by(targets.len()).copied().collect();
                if let (false, Some(target)) = (units.is_empty(), self.base_of(world, p, origin)) {
                    out.push(CommandKind::AttackMove { units, target });
                }
            }
            return;
        };
        if let Some(target) = self.enemy_base(world, home) {
            out.push(CommandKind::AttackMove { units: idle, target });
        }
    }

    /// Where a split wave sent at `player` heads: just outside the building of theirs this AI
    /// remembers nearest `from`, else outside their start's command center while it is out of
    /// sight, else wherever `enemy_base` scouts (their last buildings stand elsewhere).
    fn base_of(&self, world: &World, player: PlayerId, from: Vec2) -> Option<Vec2> {
        let vision = world.vision(self.player);
        let seen = vision
            .memory
            .iter()
            .filter(|(_, s)| s.owner == player)
            .min_by_key(|(id, s)| (s.pos.distance(from), **id))
            .map(|(_, s)| (s.pos, world.rules.def(s.def).radius));
        let cc = world.rules.def(world.rules.command_center(world.player(player).faction)).radius;
        match (seen, world.start(player)) {
            (Some((pos, radius)), _) => Some(stand_off(pos, radius, from)),
            (None, Some(start)) if !vision.is_visible(start) => Some(stand_off(start, cc, from)),
            _ => self.enemy_base(world, from),
        }
    }

    /// `Normal`: weigh the fight before every move. Strength is `strength` summed; the army
    /// attacks once it is half again as strong as the enemy army seen lately (and at least a
    /// wave), a unit whose own fight goes worse than one to two falls back with the units
    /// near it to regroup, and a base attacked by more than its guards can hold calls the
    /// field army home.
    fn command_army(
        &mut self,
        world: &World,
        mine: &[&Entity],
        army: &[&Entity],
        home: Option<Vec2>,
        out: &mut Vec<CommandKind>,
    ) {
        let (me, rules) = (self.player, world.rules);
        let Some(home) = home else { return };
        let vision = world.vision(me);
        let enemies: Vec<&Entity> =
            world.entities().filter(|e| world.hostile(me, e.owner) && e.hp > 0 && world.sees(me, e)).collect();
        let sum = |units: &mut dyn Iterator<Item = &&Entity>| units.map(|e| strength(world, e)).sum::<i64>();
        let seen_now = sum(&mut enemies.iter());
        // Slow fade (a 64th per second): an army that walked out of sight still exists.
        self.enemy_seen = (self.enemy_seen - self.enemy_seen / 64).max(seen_now);
        let near_base = |p: Vec2| near_base(world, mine, p);
        let at_home = |e: &&Entity| near_base(e.pos);

        // Special shots (the Shadow Tank's Particle Accelerator): every unit with one ready
        // fires it at the strongest enemy it sees within reach (units first, as a gun picks;
        // ties by id), wherever it is. A unit already firing one is left alone.
        for u in mine.iter().filter(|e| e.inside.is_none() && !matches!(e.order, Order::Special { .. })) {
            let Some(sp) = rules.def(u.def).special.filter(|_| u.special_at <= world.tick()) else { continue };
            let target = enemies
                .iter()
                .filter(|t| {
                    let d = rules.def(t.def);
                    sp.weapon.range_vs(d.flies()).is_some_and(|r| t.pos.distance(u.pos) - d.radius <= r)
                })
                .max_by_key(|t| {
                    (rules.def(t.def).is_unit(), strength(world, t).max(t.hp as i64), std::cmp::Reverse(t.id))
                });
            if let Some(t) = target {
                out.push(CommandKind::Special { units: vec![u.id], target: t.id });
            }
        }

        // Defence: armed intruders near the base. Guards at home fight them; if the intruders
        // outweigh the guards, every wave comes home too.
        let intruders: Vec<&&Entity> = enemies.iter().filter(|e| strength(world, e) > 0 && near_base(e.pos)).collect();
        if let Some(first) = intruders.iter().min_by_key(|e| (e.pos.distance(home), e.id)) {
            let threat = sum(&mut intruders.iter().copied());
            let guards: Vec<&&Entity> = army.iter().filter(|e| at_home(e)).collect();
            let free: Vec<EntityId> = guards
                .iter()
                .filter(|e| !matches!(e.order, Order::Attack { .. } | Order::AttackMove { engaged: Some(_), .. }))
                .map(|e| e.id)
                .collect();
            if !free.is_empty() {
                out.push(CommandKind::AttackMove { units: free, target: first.pos });
            }
            if sum(&mut guards.iter().copied()) < threat {
                let away: Vec<EntityId> = army.iter().filter(|e| !at_home(e)).map(|e| e.id).collect();
                if !away.is_empty() {
                    out.push(CommandKind::AttackMove { units: away, target: first.pos });
                }
            }
            return;
        }

        // Units in the field: each one in a fight judges the fight around itself, so two waves
        // in different places don't average out. A losing group falls back to regroup; units
        // already on their way home are left alone, so a retreat is issued once.
        let out_there: Vec<&&Entity> = army.iter().filter(|e| !at_home(e)).collect();
        let retreating = |e: &&Entity| matches!(e.order, Order::Move { target, .. } if target == home);
        let mut fleeing: Vec<EntityId> = Vec::new();
        for e in out_there.iter().filter(|e| !retreating(e)) {
            let near = |p: Vec2| p.distance(e.pos) <= BATTLE_RADIUS;
            let theirs = sum(&mut enemies.iter().filter(|x| near(x.pos)));
            if theirs == 0 {
                continue;
            }
            let ours = sum(&mut out_there.iter().copied().filter(|x| near(x.pos)));
            if ours * 2 < theirs {
                fleeing.push(e.id);
            }
        }
        if !fleeing.is_empty() {
            out.push(CommandKind::Move { units: fleeing, target: home });
            self.regroup_until = world.tick() + 20 * TICKS_PER_SECOND;
        }

        // Garrisons: idle infantry at home take an empty civilian building near the base, one
        // building at a time, so the base's approaches have a strongpoint.
        if let Some(house) = world
            .entities()
            .filter(|e| {
                e.owner.is_none()
                    && rules.def(e.def).role() == Some(BuildingRole::Civilian)
                    && near_base(e.pos)
                    && vision.is_visible(e.pos)
            })
            .min_by_key(|e| (e.pos.distance(home), e.id))
            && let Some(room) = world.garrison_room(me, house.id)
        {
            let squads: Vec<EntityId> = army
                .iter()
                .filter(|e| e.order == Order::Idle && at_home(e) && rules.def(e.def).can_garrison())
                .take(room as usize)
                .map(|e| e.id)
                .collect();
            if !squads.is_empty() {
                out.push(CommandKind::Garrison { units: squads, container: house.id });
            }
        }

        // New wave: every idle armed unit, at home or left in the field by an earlier wave,
        // once it outweighs the enemy army seen lately by half and the army isn't regrouping.
        if world.tick() < self.regroup_until {
            return;
        }
        let idle: Vec<&&Entity> = army.iter().filter(|e| e.order == Order::Idle).collect();
        let ready = sum(&mut idle.iter().copied());
        if idle.len() >= NORMAL_WAVE
            && ready * 2 >= self.enemy_seen * 3
            && let Some(target) = self.enemy_base(world, home)
        {
            out.push(CommandKind::AttackMove { units: idle.iter().map(|e| e.id).collect(), target });
        }
    }

    /// Spiral search outward from `around` for the first valid foundation spot.
    fn find_spot(&self, world: &World, def: crate::defs::DefId, around: Vec2) -> Option<Vec2> {
        let step = world.rules.def(def).radius * Fx::from_int(2) + Fx::from_int(4);
        for ring in 1..12i64 {
            for k in 0..8 * ring {
                let pos = spiral_cell(world, around, step, ring, k);
                if world.check_placement(def, pos).is_ok() && !blocks_exit(world, def, pos) {
                    return Some(pos);
                }
            }
        }
        None
    }

    /// The valid foundation spot nearest `toward` among the spiral's first rings around
    /// `around`; ties go to the earlier ring cell.
    fn find_spot_toward(&self, world: &World, def: crate::defs::DefId, around: Vec2, toward: Vec2) -> Option<Vec2> {
        let step = world.rules.def(def).radius * Fx::from_int(2) + Fx::from_int(4);
        let mut best: Option<(Fx, Vec2)> = None;
        for ring in 1..4i64 {
            for k in 0..8 * ring {
                let pos = spiral_cell(world, around, step, ring, k);
                if world.check_placement(def, pos).is_ok()
                    && !blocks_exit(world, def, pos)
                    && best.is_none_or(|(d, _)| pos.distance(toward) < d)
                {
                    best = Some((pos.distance(toward), pos));
                }
            }
        }
        best.map(|(_, p)| p).or_else(|| self.find_spot(world, def, around))
    }

    /// Where an attack wave heads: just outside the nearest enemy building this AI remembers,
    /// on the side facing home (not its centre, a blocked cell no route can end on). With none
    /// remembered, it scouts the nearest place it can't see: start positions, oil wells and
    /// supply piles first (the map is public; enemies build there), then any cell it has
    /// never explored whose centre can be walked to, so a hidden last building is always found.
    /// Unexplored cells are searched outward from the other players' starts, where their last
    /// buildings most likely stand, rather than outward from home. (A cell in a lake may stay
    /// unexplored for good: no unit gets close enough to see it, and scouting it would send
    /// every wave there forever.) Units engage whatever they meet once they are close anyway.
    fn enemy_base(&self, world: &World, home: Vec2) -> Option<Vec2> {
        let vision = world.vision(self.player);
        let seen = vision
            .memory
            .iter()
            .min_by_key(|(id, s)| (s.pos.distance(home), **id))
            .map(|(_, s)| (s.pos, world.rules.def(s.def).radius));
        let scout = || {
            let resources = world
                .entities()
                .filter(|e| matches!(world.rules.def(e.def).class, Class::OilWell | Class::SupplyPile))
                .map(|e| e.pos);
            let nearest = |points: &mut dyn Iterator<Item = Vec2>| points.min_by_key(|&p| (p.distance(home), p.x, p.y));
            let likely: Vec<Vec2> = world.map.spawns.iter().copied().chain(resources).collect();
            let theirs = other_starts(world, self.player, home);
            let near_theirs = |p: Vec2| theirs.iter().map(|&s| s.distance(p)).min().unwrap_or(Fx::ZERO);
            nearest(&mut likely.iter().copied().filter(|&p| !vision.is_explored(p)))
                .or_else(|| {
                    vision
                        .unexplored_centres()
                        .filter(|&p| world.map.walkable(p))
                        .min_by_key(|&p| (near_theirs(p), p.distance(home), p.x, p.y))
                })
                .or_else(|| nearest(&mut likely.iter().copied().filter(|&p| !vision.is_visible(p))))
                .map(|p| (p, Fx::from_int(5)))
        };
        let (pos, radius) = seen.or_else(scout)?;
        Some(stand_off(pos, radius, home))
    }

    fn free_well(&self, world: &World, derrick: crate::defs::DefId, around: Vec2) -> Option<Vec2> {
        let mut best: Option<(Fx, Vec2)> = None;
        for w in world.entities().filter(|e| matches!(world.rules.def(e.def).class, Class::OilWell)) {
            if world.check_placement(derrick, w.pos).is_err() {
                continue;
            }
            let d = w.pos.distance(around);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, w.pos));
            }
        }
        best.map(|(_, p)| p)
    }
}

/// Just outside a building of `radius` at `pos`, on the side facing `from`: not its centre, a
/// blocked cell no route can end on.
fn stand_off(pos: Vec2, radius: Fx, from: Vec2) -> Vec2 {
    let delta = from - pos;
    let dist = delta.length();
    if dist == Fx::ZERO {
        return pos;
    }
    let gap = radius + Fx::from_int(8);
    pos + delta.scale(gap.min(dist) / dist)
}

/// How close the straight way from `from` to `to` comes to `threat`.
fn clearance(from: Vec2, to: Vec2, threat: Vec2) -> Fx {
    let (way, rel) = (to - from, threat - from);
    let len_sq = way.length_sq();
    if len_sq == Fx::ZERO {
        return rel.length();
    }
    let s = ((way.x * rel.x + way.y * rel.y) / len_sq).max(Fx::ZERO).min(Fx::ONE);
    threat.distance(from + way.scale(s))
}

/// Where a unit at `from` gets away from `threats` (enemy positions) to: of `toward` itself and
/// 16 walkable points `step` away around it, the one whose straight way there keeps farthest
/// from every threat, ties going to the point nearest `toward`. Every way starts beside the
/// nearest threat, so the best keep that distance: they lead straight away from all of them,
/// and home when home lies that way, else to the side.
fn escape(world: &World, from: Vec2, toward: Vec2, threats: &[Vec2], step: Fx) -> Vec2 {
    let safety = |to: Vec2| threats.iter().map(|&t| clearance(from, to, t)).min().unwrap_or(Fx::ZERO);
    let around = (0..16).map(|k| {
        let (dx, dy) = ring_offset(2, k);
        let dir = Vec2::from_int(dx, dy);
        from + dir.scale(step / dir.length())
    });
    let open = |p: Vec2| {
        world.map.walkable(p) && {
            let (cx, cy) = world.nav.cell(p);
            !world.nav.blocked(cx, cy)
        }
    };
    std::iter::once(toward)
        .chain(around.filter(|&p| open(p)))
        .max_by_key(|&p| (safety(p), std::cmp::Reverse(p.distance(toward)), p.x, p.y))
        .unwrap_or(toward)
}

/// Whether `p` is within `DEFEND_RADIUS` of the edge of one of the base's buildings (a held
/// civilian building or a bunker is no base to defend or fall back to).
fn near_base(world: &World, mine: &[&Entity], p: Vec2) -> bool {
    mine.iter().any(|b| {
        let d = world.rules.def(b.def);
        d.is_base() && b.pos.distance(p) <= DEFEND_RADIUS + d.radius
    })
}

/// Every start but `me`'s own (the one nearest `home`; none for a startless onslaught, whose
/// home is where its waves appear) and its allies'.
fn other_starts(world: &World, me: PlayerId, home: Vec2) -> Vec<Vec2> {
    let own =
        world.start(me).and_then(|_| world.map.spawns.iter().copied().min_by_key(|&s| (s.distance(home), s.x, s.y)));
    let ally = |i: usize| i < world.players().len() && i != me.0 as usize && world.allied(me, PlayerId(i as u8));
    (world.map.spawns.iter().copied().enumerate())
        .filter(|&(i, s)| Some(s) != own && !ally(i))
        .map(|(_, s)| s)
        .collect()
}

/// Room a new unit needs where it leaves its building: a tank's footprint (3.5) and a margin.
const EXIT_ROOM: Fx = Fx::from_int(6);

/// Where units trained at a building of `radius` at `pos` appear: on the side facing the map
/// centre, as `World` places them.
fn exit_of(world: &World, pos: Vec2, radius: Fx) -> Vec2 {
    let side = if pos.y * Fx::from_int(2) > world.map.height { -Fx::ONE } else { Fx::ONE };
    pos + Vec2::new(Fx::ZERO, (radius + EXIT_ROOM) * side)
}

/// A building of `def` at `pos` would wall in a unit's way out: its own exit lies against
/// another building, or it stands on the exit of any building that trains units. Without
/// this, a war factory placed just behind the command center put every tank it trained into
/// the gap between them, where no route leads out (Easy EU against EU on Crater Ridge: the
/// whole vehicle army stood at home from the first wave on, and neither side ever won).
fn blocks_exit(world: &World, def: crate::defs::DefId, pos: Vec2) -> bool {
    let rules = world.rules;
    let d = rules.def(def);
    let trains = |d: &crate::defs::Def| d.produces.iter().any(|&u| rules.def(u).is_unit());
    let statics = || world.entities().filter(|e| !rules.def(e.def).is_unit());
    let exit = exit_of(world, pos, d.radius);
    (trains(d) && statics().any(|e| e.pos.distance(exit) < rules.def(e.def).radius + EXIT_ROOM))
        || statics().any(|e| {
            let od = rules.def(e.def);
            od.is_building() && trains(od) && exit_of(world, e.pos, od.radius).distance(pos) < d.radius + EXIT_ROOM
        })
}

/// The `k`-th spiral cell around `around`, `step` apart, with both axes flipped to point at the
/// map centre. Starts that mirror each other through the centre (as on every 2-player map)
/// then scan mirrored cells in the same order and build mirrored bases.
fn spiral_cell(world: &World, around: Vec2, step: Fx, ring: i64, k: i64) -> Vec2 {
    let toward = |v: Fx, size: Fx| if v * Fx::from_int(2) > size { -1 } else { 1 };
    let (fx, fy) = (toward(around.x, world.map.width), toward(around.y, world.map.height));
    let (dx, dy) = ring_offset(ring, k);
    around + Vec2::new(step * Fx::from_int(dx * fx), step * Fx::from_int(dy * fy))
}

/// The `k`-th cell (0-based, `0..8*ring`) on the square ring at Chebyshev distance `ring`.
fn ring_offset(ring: i64, k: i64) -> (i64, i64) {
    let side = 2 * ring;
    let (edge, t) = (k / side, k % side);
    match edge {
        0 => (-ring + t, -ring),
        1 => (ring, -ring + t),
        2 => (ring - t, ring),
        _ => (-ring, ring - t),
    }
}

#[cfg(test)]
mod tests {
    use super::ring_offset;

    #[test]
    fn ring_covers_each_cell_once() {
        for ring in 1..5 {
            let mut cells: Vec<_> = (0..8 * ring).map(|k| ring_offset(ring, k)).collect();
            assert!(cells.iter().all(|&(x, y)| x.abs().max(y.abs()) == ring));
            cells.sort();
            cells.dedup();
            assert_eq!(cells.len() as i64, 8 * ring);
        }
    }
}
