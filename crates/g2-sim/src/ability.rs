//! Unit abilities (`Def::abilities`): up to `MAX_ABILITIES` per unit, each with its own
//! recharge, used where the unit stands, at a point on the ground or at an enemy
//! (`AbilityKind::aim`). Aimed ones walk the unit within reach first (`Order::Cast`). The
//! heroes' abilities are built from the same pieces as the powers: timed statuses, shells, a
//! field, blasts with lingering clouds, a direct shot; the one new piece is the trap, a hidden
//! charge that waits for enemy aircraft.
//!
//! Kept apart from `world.rs` so the combat code there stays as it was; `World` calls in from
//! its command handling (`order_ability`), the unit update (`cast_step`), the step
//! (`spring_traps`) and production (`has_or_trains`).

use crate::command::{Aim, CommandKind, Reject};
use crate::defs::{AbilityAim, AbilityKind, Armour, DefId, Weapon};
use crate::entity::{EntityId, Order, PlayerId, Product};
use crate::fixed::{Fx, Vec2};
use crate::world::{
    Cloud, Event, PendingBlast, PendingField, SLACK, STEALTH_REVEAL_TICKS, ShellInFlight, ShellSource, World, around,
};

/// The `PendingField::slot` of a unit ability's field: this flag over its ability slot, so it
/// is never taken for a power slot.
pub const ABILITY_FIELD: u8 = 0x80;

/// A trap a unit ability set (`AbilityKind::Trap`): hidden from every other player, it lies at
/// `at` until an aircraft of an enemy of `owner` comes within `radius`, then deals `damage` to
/// every such aircraft that near and is spent. Part of the state hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trap {
    pub shooter: EntityId,
    pub owner: PlayerId,
    /// The def and ability slot that set it, for drawing it.
    pub def: DefId,
    pub slot: u8,
    pub at: Vec2,
    pub radius: Fx,
    pub damage: i32,
}

impl World {
    /// The traps lying in wait, in order of setting. The client shows only its own player's.
    pub fn traps(&self) -> &[Trap] {
        &self.traps
    }

    /// Ticks until unit `e`'s ability in `slot` is ready (0: now); `None` if it has none there.
    pub fn ability_wait(&self, e: &crate::Entity, slot: u8) -> Option<u32> {
        self.rules.def(e.def).abilities.get(slot as usize)?;
        Some(e.ability_at[slot as usize].saturating_sub(self.tick))
    }

    /// Whether `player` has a `def` standing (a real one, not a hologram) or in a queue.
    pub fn has_or_trains(&self, player: PlayerId, def: DefId) -> bool {
        self.entities.iter().filter(|e| e.owner == Some(player)).any(|e| {
            (e.def == def && e.hp > 0 && e.decoy_until.is_none())
                || e.queue.iter().any(|q| q.item == Product::Unit(def))
        })
    }

    /// A splash weapon's hit on `target` at `at` reaches the other enemy ground units and
    /// buildings around it (`Weapon::splash`): full damage within three quarters of the
    /// splash, 40% beyond, by their edge.
    pub(crate) fn splash(&mut self, shooter: EntityId, owner: PlayerId, target: EntityId, at: Vec2, w: Weapon) {
        let inner = w.splash * Fx::from_ratio(3, 4);
        let hit: Vec<(EntityId, i32)> = self
            .entities
            .iter()
            .filter(|e| e.id != target && self.hostile(owner, e.owner) && e.hp > 0 && e.inside.is_none())
            .filter_map(|e| {
                let d = self.rules.def(e.def);
                let gap = e.pos.distance(at) - d.radius;
                if d.flies() || gap > w.splash {
                    return None;
                }
                let full = w.damage_vs(d.armour);
                Some((e.id, if gap <= inner { full } else { (full * 2 / 5).max(1) }))
            })
            .collect();
        for (e, damage) in hit {
            self.shots.push((shooter, owner, e, damage, true));
        }
    }

    /// `CommandKind::Ability`: the units with ability `slot` ready use it at once, or, for one
    /// aimed elsewhere, are sent to (`Order::Cast`). A point must be on the map and explored,
    /// an enemy one the player may attack and the ability's shot can hit. Units whose ability
    /// there doesn't take that aim are left alone; with none left the order is refused.
    pub(crate) fn order_ability(
        &mut self,
        player: PlayerId,
        units: &[EntityId],
        slot: u8,
        aim: Aim,
    ) -> Result<(), Reject> {
        let tick = self.tick;
        let ready: Vec<EntityId> = self
            .own_units(player, units, |d| d.abilities.len() > slot as usize)
            .map_err(|_| Reject::AbilityNotReady)?
            .into_iter()
            .filter(|&u| {
                let e = self.entities.get(u).unwrap();
                e.ability_at[slot as usize] <= tick && e.inside.is_none() && !e.stunned() && e.decoy_until.is_none()
            })
            .collect();
        if ready.is_empty() {
            return Err(Reject::AbilityNotReady);
        }
        let fits = |w: &World, u: EntityId| {
            let kind = w.rules.def(w.entities.get(u).unwrap().def).abilities[slot as usize].kind;
            match (kind.aim(), aim) {
                (AbilityAim::Here, Aim::Here) => true,
                (AbilityAim::Point(_), Aim::Point(p)) => {
                    w.map.contains(p) && w.visions[player.0 as usize].is_explored(p)
                }
                (AbilityAim::Unit(weapon), Aim::Unit(t)) => {
                    w.attackable(player, t) && weapon.targets.hits(w.rules.def(w.entities.get(t).unwrap().def).flies())
                }
                _ => false,
            }
        };
        let fitting: Vec<EntityId> = ready.into_iter().filter(|&u| fits(self, u)).collect();
        if fitting.is_empty() {
            return Err(Reject::InvalidTarget);
        }
        for u in fitting {
            if aim == Aim::Here { self.use_ability(u, slot, aim) } else { self.send_to_cast(u, slot, aim) }
        }
        Ok(())
    }

    fn send_to_cast(&mut self, unit: EntityId, slot: u8, aim: Aim) {
        let e = self.entities.get_mut(unit).unwrap();
        e.order = Order::Cast { slot, aim };
        e.home = None;
    }

    /// One tick of `Order::Cast` for unit `id` at `pos`: close to within the ability's reach
    /// of its aim, then use it and stop. A target lost (dead, gone into stealth or inside
    /// something) or the ability no longer ready drops the order.
    pub(crate) fn cast_step(&mut self, id: EntityId, slot: u8, aim: Aim, pos: Vec2, speed: Fx) -> (Vec2, Order) {
        let e = self.entities.get(id).unwrap();
        let owner = e.owner.unwrap();
        let order = Order::Cast { slot, aim };
        let Some(a) = self.rules.def(e.def).abilities.get(slot as usize).copied() else { return (pos, Order::Idle) };
        if e.ability_at[slot as usize] > self.tick {
            return (pos, Order::Idle);
        }
        let (at, reach, edge) = match (a.kind.aim(), aim) {
            (AbilityAim::Point(range), Aim::Point(p)) => (p, range, Fx::ZERO),
            (AbilityAim::Unit(w), Aim::Unit(t)) => {
                let detected = |t: &crate::Entity| self.visions[owner.0 as usize].detected.contains(&t.id);
                let Some(t) = self.live_enemy(owner, t).filter(|t| !self.hidden(t) || detected(t)) else {
                    return (pos, Order::Idle);
                };
                let d = self.rules.def(t.def);
                let Some(range) = w.range_vs(d.flies()) else { return (pos, Order::Idle) };
                (t.pos, range, d.radius)
            }
            _ => return (pos, Order::Idle),
        };
        if at.distance(pos) - edge > reach {
            // Just inside reach, so separation can't push it back out.
            let contact = (edge + reach - SLACK).max(Fx::ZERO);
            return (self.approach(id, at, contact, speed).0, order);
        }
        self.use_ability(id, slot, aim);
        (pos, Order::Idle)
    }

    /// Unit `id` uses its ability in `slot` now, aimed at `aim`; it recharges from this tick.
    pub(crate) fn use_ability(&mut self, id: EntityId, slot: u8, aim: Aim) {
        let tick = self.tick;
        let e = self.entities.get(id).unwrap();
        let a = self.rules.def(e.def).abilities[slot as usize];
        let (owner, def, pos) = (e.owner.unwrap(), e.def, e.pos);
        let target = match aim {
            Aim::Unit(t) => self.entities.get(t).map(|t| (t.id, t.pos, self.rules.def(t.def).armour)),
            _ => None,
        };
        let at = match aim {
            Aim::Here => pos,
            Aim::Point(p) => p,
            Aim::Unit(_) => target.map_or(pos, |t| t.1),
        };
        self.entities.get_mut(id).unwrap().ability_at[slot as usize] = tick + a.cooldown_ticks;
        match a.kind {
            AbilityKind::Boost { statuses, radius, infantry, cloak_ticks } => {
                for u in self.entities.ids() {
                    let e = self.entities.get(u).unwrap();
                    let d = self.rules.def(e.def);
                    let near = radius > Fx::ZERO
                        && e.owner == Some(owner)
                        && d.is_unit()
                        && !d.is_support_jet()
                        && e.hp > 0
                        && e.inside.is_none()
                        && (!infantry || d.armour == Armour::Infantry)
                        && e.pos.distance(pos) <= radius;
                    if u == id || near {
                        let e = self.entities.get_mut(u).unwrap();
                        for s in statuses.into_iter().flatten() {
                            e.add_status(s, tick);
                        }
                    }
                }
                if cloak_ticks > 0 {
                    self.entities.get_mut(id).unwrap().cloaked_until = Some(tick + cloak_ticks);
                }
            }
            AbilityKind::Detonate => {
                // Its blast goes off as it is swept away (`cleanup`).
                self.entities.get_mut(id).unwrap().hp = 0;
                self.stats[owner.0 as usize].units_lost += 1;
            }
            AbilityKind::Throw { blast, shells, every_ticks, spread, speed, .. } => {
                let mut last = tick;
                // A fixed pattern, not the RNG: the point first, then rings of eight at half
                // and at the whole spread in turn.
                for k in 0..shells {
                    let spot = if k == 0 {
                        at
                    } else {
                        let ring = Fx::from_ratio(1 + ((k - 1) / 8 % 2) as i64, 2);
                        self.map.clamp(at + around((k - 1) % 8, spread * ring))
                    };
                    let fired = tick + k * every_ticks;
                    let lands = fired + (pos.distance(spot) / speed.max(Fx::ONE)).floor_int().max(1) as u32;
                    last = last.max(lands);
                    self.shells.push(ShellInFlight {
                        shooter: id,
                        owner,
                        from: pos,
                        at: spot,
                        fired,
                        lands,
                        damage: blast.damage,
                        bonus: blast.bonus,
                        radius: blast.radius,
                        garrison_bonus: 0,
                        source: ShellSource::Ability { def, slot },
                    });
                }
                if blast.cloud_ticks > 0 {
                    let (radius, damage) = (blast.cloud_radius, blast.cloud_damage);
                    self.clouds.push(Cloud { owner, at, radius, damage, from: last, until: last + blast.cloud_ticks });
                }
            }
            AbilityKind::Area { field, .. } => {
                let lands = tick + field.delay_ticks;
                self.fields.push(PendingField { owner, slot: ABILITY_FIELD | slot, at, lands, field });
            }
            AbilityKind::Shot { weapon } => {
                if let Some((t, tpos, armour)) = target {
                    self.shots.push((id, owner, t, weapon.damage_vs(armour), false));
                    // As any shot: a stealth unit is given away, a cloak's cover ends.
                    let stealthy = self.rules.def(def).stealth;
                    let e = self.entities.get_mut(id).unwrap();
                    if stealthy {
                        e.revealed_until = Some(tick + STEALTH_REVEAL_TICKS);
                    }
                    e.cloaked_until = None;
                    self.events.push(Event::SpecialFired { shooter: id, target: t, from: pos, at: tpos });
                }
            }
            AbilityKind::Trap { radius, damage, .. } => {
                self.traps.push(Trap { shooter: id, owner, def, slot, at, radius, damage });
            }
            AbilityKind::Sacrifice { radius, blast, .. } => {
                let fallen: Vec<(EntityId, Vec2)> = self
                    .entities
                    .iter()
                    .filter(|e| {
                        let d = self.rules.def(e.def);
                        e.id != id
                            && e.owner == Some(owner)
                            && d.is_unit()
                            && !d.flies()
                            && d.armour == Armour::Infantry
                            && d.weapon().is_some()
                            && e.hp > 0
                            && e.inside.is_none()
                            && e.decoy_until.is_none()
                            && e.pos.distance(at) - d.radius <= radius
                    })
                    .map(|e| (e.id, e.pos))
                    .collect();
                for (u, upos) in fallen {
                    self.entities.get_mut(u).unwrap().hp = 0;
                    self.stats[owner.0 as usize].units_lost += 1;
                    self.blasts.push(PendingBlast { shooter: u, owner, def, at: upos, lands: tick + 1, blast });
                }
            }
        }
        self.events.push(Event::AbilityUsed { player: owner, unit: id, def, slot, at });
    }

    /// Traps whose owner is out are dropped; the rest go off once an enemy aircraft is within
    /// reach, hitting every enemy aircraft that near (the damage joins this tick's shots).
    pub(crate) fn spring_traps(&mut self) {
        if self.traps.is_empty() {
            return;
        }
        let traps = core::mem::take(&mut self.traps);
        for t in traps {
            if self.players[t.owner.0 as usize].defeated {
                continue;
            }
            let caught: Vec<EntityId> = self
                .entities
                .iter()
                .filter(|e| {
                    let d = self.rules.def(e.def);
                    d.flies()
                        && self.hostile(t.owner, e.owner)
                        && e.hp > 0
                        && e.inside.is_none()
                        && e.pos.distance(t.at) - d.radius <= t.radius
                })
                .map(|e| e.id)
                .collect();
            if caught.is_empty() {
                self.traps.push(t);
                continue;
            }
            for e in caught {
                self.shots.push((t.shooter, t.owner, e, t.damage, true));
            }
            let source = ShellSource::Ability { def: t.def, slot: t.slot };
            self.events.push(Event::Landed { at: t.at, radius: t.radius, source });
        }
    }
}

/// `units` use ability `slot` where they stand: the command an AI or a test gives for one that
/// needs no aim.
pub fn in_place(units: Vec<EntityId>, slot: u8) -> CommandKind {
    CommandKind::Ability { units, slot, aim: Aim::Here }
}
