//! Destructible bridges, after Zero Hour's `BridgeBehavior` and `Bridge::updateDamageState`.
//!
//! A bridge is no entity: it is a deck of the map (`MapDef::bridges`) with hit points here. Any
//! player may order armed units to attack it (Zero Hour's force-attack; the AI never does): they
//! fire at the nearer of its two attack points (`BridgeDef::attack_points`). Every shell and
//! power that bursts within its blast radius of the bridge's centre hurts it too, as Zero Hour's
//! radius damage reaches the bridge's damage object there. At 0 hit points the deck falls: every
//! ground unit on its span dies (the engine kills everything on the bridge layer, "regardless of
//! height they might fall") and the nav cells over the gap close. A constructor at either end
//! repairs it at `REPAIR_PERCENT_PER_SECOND` of its hit points a second, one at a time
//! (`attemptHealingFromSoleBenefactor`); the deck stays closed while one works on it (the
//! scaffolding) and opens once nobody is repairing it and it has hit points again.
//!
//! State is hashed only once a bridge has been hurt, so matches where no bridge ever is (and
//! every map without bridges) hash as before.

use crate::defs::Armour;
use crate::entity::{EntityId, Order, PlayerId, Route};
use crate::fixed::{Fx, Vec2};
use crate::map::BridgeDef;
use crate::world::{Event, TICKS_PER_SECOND, World};

/// Zero Hour's dozers and workers: `RepairHealthPercentPerSecond = 2.0%`.
pub const REPAIR_PERCENT_PER_SECOND: i32 = 2;

/// One bridge's state, in `MapDef::bridges` order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bridge {
    pub hp: i32,
    /// Its full hit points; 0 for a bridge that can't be hurt.
    pub max: i32,
    /// The deck has fallen and isn't open again yet.
    pub down: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Bridges {
    pub(crate) state: Vec<Bridge>,
    /// Direct hits this tick, applied with the shells in `resolve_bridge_hits`. Empty between
    /// ticks.
    hits: Vec<(u16, i32, PlayerId)>,
}

impl Bridges {
    pub fn new(defs: &[BridgeDef]) -> Bridges {
        let state = defs
            .iter()
            .map(|d| {
                let max = d.hp.unwrap_or(0);
                Bridge { hp: max, max, down: false }
            })
            .collect();
        Bridges { state, hits: Vec::new() }
    }

    /// Whether anything has happened to a bridge yet; until then nothing is hashed.
    fn touched(&self) -> bool {
        self.state.iter().any(|b| b.hp != b.max || b.down)
    }

    pub(crate) fn hash_into(&self, mut put: impl FnMut(u64)) {
        if !self.touched() {
            return;
        }
        put(0x6272_6964_6765);
        for b in &self.state {
            put(((b.hp as u32 as u64) << 1) | b.down as u64);
        }
    }
}

impl World {
    /// Deals `damage` to bridge `i` as if `by` had fired at it this tick, outside any command:
    /// for rendering tools and tests that need a fallen deck without staging a fight. A match
    /// that uses it no longer replays from its commands alone.
    pub fn damage_bridge(&mut self, i: u16, damage: i32, by: PlayerId) {
        self.bridges.hits.push((i, damage, by));
        self.resolve_bridge_hits();
    }

    /// Bridge `i`'s state.
    pub fn bridge(&self, i: u16) -> Option<&Bridge> {
        self.bridges.state.get(i as usize)
    }

    /// The bridge whose deck covers `p`, if any.
    pub fn bridge_at(&self, p: Vec2) -> Option<u16> {
        let (w, _) = crate::nav::Grid::dims(self.map.width, self.map.height);
        let cell = |v: Fx| (v.floor_int().max(0) / crate::nav::CELL) as u32;
        let i = cell(p.y) * w as u32 + cell(p.x);
        self.map.bridges.iter().position(|b| b.cells.binary_search(&i).is_ok()).map(|k| k as u16)
    }

    /// The map's impassable cells as they are now: fallen decks close the gap they spanned.
    pub(crate) fn nav_terrain(&self) -> std::borrow::Cow<'_, [bool]> {
        if !self.bridges.state.iter().any(|b| b.down) {
            return std::borrow::Cow::Borrowed(&self.map.terrain);
        }
        let mut t = self.map.terrain.clone();
        for (b, def) in self.bridges.state.iter().zip(&self.map.bridges) {
            if b.down {
                for &c in &def.gap {
                    if let Some(cell) = t.get_mut(c as usize) {
                        *cell = true;
                    }
                }
            }
        }
        std::borrow::Cow::Owned(t)
    }

    /// Folded into the nav grid's key, so units replan when a deck falls or opens again.
    pub(crate) fn bridges_key(&self) -> u64 {
        self.bridges.state.iter().enumerate().filter(|(_, b)| b.down).fold(0, |h, (i, _)| h ^ (0x9e37_79b9 << (i % 32)))
    }

    /// Whether `player` may order an attack on bridge `i`: it stands and can be hurt.
    pub(crate) fn bridge_attackable(&self, i: u16) -> bool {
        self.bridge(i).is_some_and(|b| b.max > 0 && !b.down)
    }

    /// Whether bridge `i` has lost hit points, so a constructor has work there.
    pub(crate) fn bridge_repairable(&self, i: u16) -> bool {
        self.bridge(i).is_some_and(|b| b.hp < b.max)
    }

    /// Unit `id` (owned by `owner`, at `pos`, with `weapon`) works on its order to attack
    /// bridge `i`: closes to its weapon's range of the nearer attack point and fires. Returns the
    /// new position and order (idle once the bridge is down).
    pub(crate) fn attack_bridge_step(
        &mut self,
        id: EntityId,
        i: u16,
        weapon: Option<crate::defs::Weapon>,
        speed: Fx,
    ) -> (Vec2, Order) {
        let e = self.entities.get(id).unwrap();
        let (pos, owner, ready) = (e.pos, e.owner.unwrap(), e.cooldown == 0);
        let order = Order::AttackBridge { bridge: i };
        let (Some(w), Some(def)) = (weapon.filter(|w| w.targets.hits(false)), self.map.bridges.get(i as usize)) else {
            return (pos, Order::Idle);
        };
        if !self.bridge_attackable(i) {
            return (pos, Order::Idle);
        }
        let [p, q] = def.attack_points();
        let aim = if p.distance(pos) <= q.distance(pos) { p } else { q };
        let gap = aim.distance(pos);
        if gap > w.range {
            let (p, _) = self.approach(id, aim, w.range - Fx::ONE, speed);
            return (p, order);
        }
        self.entities.get_mut(id).unwrap().route = Route::default();
        if !ready || w.shell.is_some_and(|s| gap < s.min_range) {
            return (pos, order);
        }
        let tick = self.tick;
        let stealthy = self.rules.def(self.entities.get(id).unwrap().def).stealth;
        let e = self.entities.get_mut(id).unwrap();
        e.cooldown = w.cooldown_ticks;
        if stealthy {
            e.revealed_until = Some(tick + crate::world::STEALTH_REVEAL_TICKS);
        }
        e.cloaked_until = None;
        let damage = w.damage_vs(Armour::Structure);
        match w.shell {
            None => {
                self.bridges.hits.push((i, damage, owner));
                self.events.push(Event::BridgeHit { bridge: i, from: pos, at: aim, damage, by: owner });
            }
            Some(s) => {
                // The shell bursts on the attack point like any other, hurting enemies there;
                // `land_bridge_shells` gives the bridge its share.
                let lands = tick + (gap / s.speed).floor_int().max(1) as u32;
                let source =
                    crate::world::ShellSource::Gun { def: self.entities.get(id).unwrap().def, deployed: false };
                self.shells.push(crate::world::ShellInFlight {
                    shooter: id,
                    owner,
                    from: pos,
                    at: aim,
                    fired: tick,
                    lands,
                    damage: w.damage,
                    bonus: w.bonus,
                    radius: s.radius,
                    garrison_bonus: w.garrison_bonus,
                    source,
                });
                self.events.push(Event::Fired { shooter: id, from: pos, at: aim, lands, source });
            }
        }
        (pos, order)
    }

    /// Shells landing this tick (before `land_shells` takes them) hurt every standing bridge
    /// whose centre is within their blast, where Zero Hour keeps the bridge's damage object, and
    /// the bridge whose attack point they were aimed at.
    pub(crate) fn land_bridge_shells(&mut self) {
        if self.map.bridges.is_empty() {
            return;
        }
        let tick = self.tick;
        for s in self.shells.iter().filter(|s| s.lands <= tick) {
            for (k, def) in self.map.bridges.iter().enumerate() {
                let centre = def.a + (def.b - def.a).scale(Fx::from_ratio(1, 2));
                if centre.distance(s.at) <= s.radius || def.attack_points().contains(&s.at) {
                    let damage = (s.damage + s.bonus[Armour::Structure as usize]).max(1);
                    self.bridges.hits.push((k as u16, damage, s.owner));
                }
            }
        }
    }

    /// Applies this tick's hits on bridges. A bridge brought to 0 falls: every ground unit on its
    /// span dies and the gap below closes (through `nav_terrain`, next `refresh_nav`).
    pub(crate) fn resolve_bridge_hits(&mut self) {
        for (i, damage, by) in core::mem::take(&mut self.bridges.hits) {
            let Some(b) = self.bridges.state.get_mut(i as usize) else { continue };
            if b.max == 0 || b.down {
                continue;
            }
            b.hp = (b.hp - damage).max(0);
            if b.hp > 0 {
                continue;
            }
            b.down = true;
            let def = &self.map.bridges[i as usize];
            let (a, d) = (def.a, def.b - def.a);
            let len_sq = d.length_sq();
            let (w, _) = crate::nav::Grid::dims(self.map.width, self.map.height);
            let cell = |v: Fx| (v.floor_int().max(0) / crate::nav::CELL) as u32;
            let doomed: Vec<EntityId> = self
                .entities
                .iter()
                .filter(|e| {
                    let ed = self.rules.def(e.def);
                    ed.is_unit() && !ed.flies() && e.inside.is_none() && e.hp > 0 && {
                        let t = (e.pos - a).x * d.x + (e.pos - a).y * d.y;
                        let on = def.cells.binary_search(&(cell(e.pos.y) * w as u32 + cell(e.pos.x))).is_ok();
                        on && t >= Fx::ZERO && t <= len_sq
                    }
                })
                .map(|e| e.id)
                .collect();
            for u in doomed {
                let e = self.entities.get_mut(u).unwrap();
                e.hp = 0;
                if let Some(o) = e.owner {
                    self.stats[o.0 as usize].units_lost += 1;
                }
            }
            let centre = a + d.scale(Fx::from_ratio(1, 2));
            self.events.push(Event::BridgeDown { bridge: i, at: centre, by });
        }
    }

    /// Constructor `id` works on its order to repair bridge `i`: walks to the nearer end and,
    /// there, `update_bridge_repairs` heals the bridge. Idle once the bridge is whole.
    pub(crate) fn repair_bridge_step(&mut self, id: EntityId, i: u16, speed: Fx) -> (Vec2, Order) {
        let pos = self.entities.get(id).unwrap().pos;
        let Some(def) = self.map.bridges.get(i as usize) else { return (pos, Order::Idle) };
        if !self.bridge_repairable(i) {
            return (pos, Order::Idle);
        }
        let end = if def.a.distance(pos) <= def.b.distance(pos) { def.a } else { def.b };
        let reach = def.width / Fx::from_int(2);
        let (p, _) = self.approach(id, end, reach, speed);
        (p, Order::RepairBridge { bridge: i })
    }

    /// The first constructor (in entity order) at an end of each damaged bridge heals it, once a
    /// second; a fallen deck opens again when it has hit points and nobody is repairing it.
    pub(crate) fn update_bridge_repairs(&mut self) {
        for k in 0..self.bridges.state.len() {
            let def = &self.map.bridges[k];
            let reach = def.width / Fx::from_int(2) + Fx::from_int(2);
            let working = self.entities.iter().any(|e| {
                e.order == (Order::RepairBridge { bridge: k as u16 })
                    && (e.pos.distance(def.a) <= reach || e.pos.distance(def.b) <= reach)
            });
            let b = &mut self.bridges.state[k];
            if working && self.tick.is_multiple_of(TICKS_PER_SECOND) && b.hp < b.max {
                b.hp = (b.hp + (b.max * REPAIR_PERCENT_PER_SECOND / 100).max(1)).min(b.max);
            }
            if b.down && !working && b.hp > 0 {
                b.down = false;
                self.events.push(Event::BridgeUp { bridge: k as u16 });
            }
        }
    }
}
