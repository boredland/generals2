use crate::ai::Ai;
use crate::command::{Command, CommandKind, Reject};
use crate::defs::{BuildingRole, Class, DefId, Faction, Rules};
use crate::entity::{Entities, Entity, EntityId, GatherPhase, Order, PlayerId, Production};
use crate::fixed::{Fx, Vec2};
use crate::map::{MapDef, MatchSetup};
use crate::rng::Pcg32;

/// Logic rate, matching SAGE's `BaseFps`.
pub const TICKS_PER_SECOND: u32 = 30;
/// Ticks between a command being issued and taking effect; leaves room for lockstep networking.
pub const INPUT_DELAY: u32 = 2;
pub const MAX_QUEUE: usize = 5;
/// Distance tolerance for "has reached" checks, so units pushed apart by separation still arrive.
const SLACK: Fx = Fx::from_int(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Player {
    pub faction: Faction,
    /// Chosen general; its modifiers apply to every cost and build time this player pays.
    pub general: Option<crate::defs::GeneralId>,
    pub credits: i64,
    pub oil: i64,
    pub power_supply: i32,
    pub power_drain: i32,
    pub defeated: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    Rejected {
        player: PlayerId,
        reason: Reject,
    },
    /// A collector unloaded `credits` at `depot`. Output only: events never feed back into the
    /// simulation, so they are not part of the state hash.
    Delivered {
        player: PlayerId,
        depot: EntityId,
        credits: i64,
    },
}

pub struct World {
    pub rules: &'static Rules,
    pub map: MapDef,
    pub(crate) tick: u32,
    pub(crate) rng: Pcg32,
    pub(crate) players: Vec<Player>,
    pub(crate) entities: Entities,
    pending: Vec<Command>,
    events: Vec<Event>,
    ai: Vec<Option<Ai>>,
    /// Every human command processed so far, in processing order. AI commands are excluded:
    /// replays re-run the AI, so recording them would apply them twice.
    log: Vec<Command>,
    /// `(tick, state_hash)` taken before stepping every `HASH_INTERVAL`-th tick.
    checkpoints: Vec<(u32, u64)>,
    /// Blocked cells, rebuilt whenever the set of statics changes. A pure function of hashed
    /// state (static entities and the map), so it needs no hash of its own.
    nav: crate::nav::Grid,
    /// Hash of the statics the grid was built from; when it changes, every unit replans.
    nav_key: u64,
}

impl World {
    pub fn new(setup: &MatchSetup, rules: &'static Rules) -> World {
        assert!(setup.players.len() <= setup.map.spawns.len(), "map has fewer spawns than players");
        for p in &setup.players {
            if let Some(g) = p.general {
                assert_eq!(
                    rules.general(g).faction,
                    p.faction,
                    "general {} is not {:?}",
                    rules.general(g).name,
                    p.faction
                );
            }
        }
        let mut world = World {
            rules,
            map: setup.map.clone(),
            tick: 0,
            rng: Pcg32::new(setup.seed, 0),
            players: Vec::new(),
            entities: Entities::default(),
            pending: Vec::new(),
            events: Vec::new(),
            ai: Vec::new(),
            log: Vec::new(),
            nav: crate::nav::Grid::default(),
            nav_key: u64::MAX,
            checkpoints: Vec::new(),
        };
        for &(pos, amount) in &setup.map.supply_piles {
            world.spawn_neutral(rules.supply_pile(), pos, amount);
        }
        for &(pos, reserve) in &setup.map.oil_wells {
            world.spawn_neutral(rules.oil_well(), pos, reserve);
        }
        for (i, p) in setup.players.iter().enumerate() {
            let owner = PlayerId(i as u8);
            world.players.push(Player {
                faction: p.faction,
                general: p.general,
                credits: p.starting_credits,
                oil: 0,
                power_supply: 0,
                power_drain: 0,
                defeated: false,
            });
            world.ai.push(p.ai.then(|| Ai::new(owner)));
            let spawn = setup.map.spawns[i];
            let cc = rules.command_center(p.faction);
            let cc_id = world.spawn_owned(cc, owner, spawn);
            let dozer = rules.starting_constructor(p.faction);
            let offset = rules.def(cc).radius + rules.def(dozer).radius + SLACK;
            world.spawn_owned(dozer, owner, world.map.clamp(spawn + Vec2::new(Fx::ZERO, offset)));
            debug_assert!(world.entities.get(cc_id).is_some());
        }
        world.update_power();
        world
    }

    fn blank_entity(id: EntityId, def: DefId, hp: i32, pos: Vec2) -> Entity {
        Entity {
            id,
            def,
            owner: None,
            pos,
            hp,
            order: Order::Idle,
            cargo: 0,
            construction: None,
            queue: Vec::new(),
            resource: 0,
            on_well: None,
            home: None,
            powered: false,
            income_ticks: 0,
            route: Vec::new(),
            route_key: 0,
        }
    }

    fn spawn_neutral(&mut self, def: DefId, pos: Vec2, resource: i64) -> EntityId {
        let hp = self.rules.def(def).hp;
        self.entities.spawn(|id| Entity { resource, ..Self::blank_entity(id, def, hp, pos) })
    }

    fn spawn_owned(&mut self, def: DefId, owner: PlayerId, pos: Vec2) -> EntityId {
        let hp = self.rules.def(def).hp;
        self.entities.spawn(|id| Entity { owner: Some(owner), ..Self::blank_entity(id, def, hp, pos) })
    }

    // ---- queries -------------------------------------------------------------------------

    pub fn tick(&self) -> u32 {
        self.tick
    }

    pub fn players(&self) -> &[Player] {
        &self.players
    }

    pub fn player(&self, id: PlayerId) -> &Player {
        &self.players[id.0 as usize]
    }

    pub fn entities(&self) -> impl Iterator<Item = &Entity> {
        self.entities.iter()
    }

    pub fn entity(&self, id: EntityId) -> Option<&Entity> {
        self.entities.get(id)
    }

    /// Events produced by the most recent `step`.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// The match so far as a replay: setup, human commands and hash checkpoints. Any driver
    /// (client, tools, tests) can save this; `verify_replay` re-simulates it to `tick()`.
    pub fn replay(&self, setup: &MatchSetup) -> crate::replay::Replay {
        crate::replay::Replay {
            map: self.map.name.to_owned(),
            map_fingerprint: Some(self.map.fingerprint()),
            seed: setup.seed,
            players: setup.players.clone(),
            length_ticks: self.tick,
            commands: self.log.clone(),
            hashes: self.checkpoints.clone(),
        }
    }

    pub fn is_ai(&self, player: PlayerId) -> bool {
        self.ai.get(player.0 as usize).is_some_and(Option::is_some)
    }

    /// The last player standing, once every other player is defeated.
    pub fn winner(&self) -> Option<PlayerId> {
        let mut alive = self.players.iter().enumerate().filter(|(_, p)| !p.defeated);
        let (first, _) = alive.next()?;
        alive.next().is_none().then_some(PlayerId(first as u8))
    }

    /// Collectors bound to `center` plus collectors queued there.
    pub fn depot_load(&self, center: EntityId) -> u32 {
        let bound = self.entities.iter().filter(|e| e.home == Some(center)).count();
        let queued = self
            .entities
            .get(center)
            .map_or(0, |c| c.queue.iter().filter(|p| self.rules.def(p.def).collector().is_some()).count());
        (bound + queued) as u32
    }

    pub fn nearest_pile(&self, from: Vec2) -> Option<EntityId> {
        self.nearest(from, |e| matches!(self.rules.def(e.def).class, Class::SupplyPile) && e.resource > 0)
    }

    fn nearest(&self, from: Vec2, pred: impl Fn(&Entity) -> bool) -> Option<EntityId> {
        let mut best: Option<(Fx, EntityId)> = None;
        for e in self.entities.iter().filter(|e| pred(e)) {
            let d = e.pos.distance(from);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, e.id));
            }
        }
        best.map(|(_, id)| id)
    }

    fn find_depot(&self, player: PlayerId, near: Vec2) -> Option<EntityId> {
        self.nearest(near, |e| {
            e.owner == Some(player)
                && e.is_complete()
                && matches!(self.rules.def(e.def).role(), Some(BuildingRole::SupplyCenter { collector_cap })
                    if self.depot_load(e.id) < collector_cap)
        })
    }

    /// Validates a foundation position. Oil derricks snap onto the free well they cover.
    pub fn check_placement(&self, def: DefId, pos: Vec2) -> Result<(Vec2, Option<EntityId>), Reject> {
        let d = self.rules.def(def);
        let Some(role) = d.role() else { return Err(Reject::CannotBuild) };
        if let BuildingRole::OilDerrick { .. } = role {
            let well = self
                .nearest(pos, |e| {
                    matches!(self.rules.def(e.def).class, Class::OilWell)
                        && e.resource > 0
                        && e.pos.distance(pos) <= d.radius
                        && !self.entities.iter().any(|o| o.on_well == Some(e.id))
                })
                .ok_or(Reject::InvalidPlacement)?;
            return Ok((self.entities.get(well).unwrap().pos, Some(well)));
        }
        let r = d.radius;
        let inside = pos.x - r >= Fx::ZERO
            && pos.y - r >= Fx::ZERO
            && pos.x + r <= self.map.width
            && pos.y + r <= self.map.height;
        let blocked = self.entities.iter().any(|e| {
            let od = self.rules.def(e.def);
            !od.is_unit() && e.pos.distance(pos) < od.radius + r
        });
        if !inside || blocked || self.map.terrain_within(pos, r) {
            return Err(Reject::InvalidPlacement);
        }
        Ok((pos, None))
    }

    // ---- input ---------------------------------------------------------------------------

    /// Schedules a player command `INPUT_DELAY` ticks ahead and returns it for recording.
    pub fn submit(&mut self, player: PlayerId, kind: CommandKind) -> Command {
        let cmd = Command { tick: self.tick + INPUT_DELAY, player, kind };
        self.pending.push(cmd.clone());
        cmd
    }

    /// Schedules a command at its own tick (replays). Commands for past ticks run next step.
    pub fn schedule(&mut self, cmd: Command) {
        self.pending.push(cmd);
    }

    // ---- simulation ----------------------------------------------------------------------

    pub fn step(&mut self) {
        if self.tick.is_multiple_of(crate::replay::HASH_INTERVAL) {
            self.checkpoints.push((self.tick, self.state_hash()));
        }
        self.events.clear();
        self.think_ai();
        self.apply_due_commands();
        self.update_production();
        self.update_construction();
        // After construction, so a building finished this tick is powered (or not) immediately.
        self.update_power();
        self.refresh_nav();
        for id in self.entities.ids() {
            if self.entities.get(id).is_some_and(|e| self.rules.def(e.def).is_unit()) {
                self.update_unit(id);
            }
        }
        self.separate_units();
        self.update_oil();
        self.cleanup();
        self.update_defeat();
        self.tick += 1;
    }

    fn think_ai(&mut self) {
        let mut issued = Vec::new();
        for slot in 0..self.ai.len() {
            let Some(mut ai) = self.ai[slot].take() else { continue };
            if !self.players[slot].defeated {
                ai.think(self, &mut issued);
            }
            self.ai[slot] = Some(ai);
            for kind in issued.drain(..) {
                self.submit(PlayerId(slot as u8), kind);
            }
        }
    }

    fn apply_due_commands(&mut self) {
        let (mut due, later): (Vec<Command>, Vec<Command>) =
            core::mem::take(&mut self.pending).into_iter().partition(|c| c.tick <= self.tick);
        self.pending = later;
        // Stable sort: per-player order is the submission order, which is identical live and in
        // replays; cross-player order must not depend on when commands were inserted.
        due.sort_by_key(|c| c.player);
        for cmd in due {
            if !self.is_ai(cmd.player) {
                // Log the tick it actually ran on: a late command (due tick already past) runs now.
                self.log.push(Command { tick: self.tick, ..cmd.clone() });
            }
            if let Err(reason) = self.apply(&cmd) {
                self.events.push(Event::Rejected { player: cmd.player, reason });
            }
        }
    }

    fn owned(&self, player: PlayerId, id: EntityId) -> Result<&Entity, Reject> {
        let e = self.entities.get(id).ok_or(Reject::NoSuchEntity)?;
        if e.owner != Some(player) {
            return Err(Reject::NotOwner);
        }
        Ok(e)
    }

    /// What `player` pays for `def`, after their general's modifiers. The general is fixed for the
    /// match, so a refund at the same price is exact.
    pub fn cost_for(&self, player: PlayerId, def: DefId) -> crate::defs::Cost {
        self.rules.cost(self.player(player).general, def)
    }

    /// Ticks `player` needs to build or train `def`, after their general's modifiers.
    pub fn build_ticks_for(&self, player: PlayerId, def: DefId) -> u32 {
        self.rules.build_ticks(self.player(player).general, def)
    }

    fn afford(&self, player: PlayerId, def: DefId) -> Result<(), Reject> {
        let cost = self.cost_for(player, def);
        let p = self.player(player);
        if p.credits < cost.credits || p.oil < cost.oil {
            return Err(Reject::InsufficientFunds);
        }
        Ok(())
    }

    fn pay(&mut self, player: PlayerId, def: DefId, sign: i64) {
        let cost = self.cost_for(player, def);
        let p = &mut self.players[player.0 as usize];
        p.credits -= sign * cost.credits;
        p.oil -= sign * cost.oil;
    }

    fn apply(&mut self, cmd: &Command) -> Result<(), Reject> {
        let player = cmd.player;
        if self.player(player).defeated {
            return Err(Reject::NotOwner);
        }
        match &cmd.kind {
            CommandKind::Move { units, target } => {
                let units = self.own_units(player, units, |_| true)?;
                let target = self.map.clamp(*target);
                // Square formation, row-major in selection order, spaced by the widest unit.
                let spacing = units.iter().map(|&u| self.radius_of(u)).max().unwrap_or(SLACK) * Fx::from_int(2) + SLACK;
                let side = ceil_sqrt(units.len() as i64);
                for (i, &u) in units.iter().enumerate() {
                    let (row, col) = (i as i64 / side, i as i64 % side);
                    let offset = Vec2::new(
                        spacing * Fx::from_int(col) - spacing * Fx::from_ratio(side - 1, 2),
                        spacing * Fx::from_int(row) - spacing * Fx::from_ratio(side - 1, 2),
                    );
                    let e = self.entities.get_mut(u).unwrap();
                    e.order = Order::Move { target: self.map.clamp(target + offset) };
                    e.home = None;
                }
                Ok(())
            }
            CommandKind::Gather { units, pile } => {
                let pile_e = self.entities.get(*pile).ok_or(Reject::NoSuchEntity)?;
                if !matches!(self.rules.def(pile_e.def).class, Class::SupplyPile) || pile_e.resource <= 0 {
                    return Err(Reject::NoSuchEntity);
                }
                let pile_pos = pile_e.pos;
                let units = self.own_units(player, units, |d| d.collector().is_some())?;
                let mut result = Err(Reject::DepotFull);
                for u in units {
                    let home = self.entities.get(u).unwrap().home.filter(|&h| self.owned(player, h).is_ok());
                    let Some(home) = home.or_else(|| self.find_depot(player, pile_pos)) else { continue };
                    let e = self.entities.get_mut(u).unwrap();
                    e.home = Some(home);
                    let phase = if e.cargo > 0 { GatherPhase::ToDepot { depot: home } } else { GatherPhase::ToPile };
                    e.order = Order::Gather { pile: *pile, phase };
                    result = Ok(());
                }
                result
            }
            CommandKind::Build { builder, def, pos } => {
                let b = self.owned(player, *builder)?;
                let bd = self.rules.def(b.def);
                if !bd.is_constructor() {
                    return Err(Reject::NotAConstructor);
                }
                if !bd.builds.contains(def) {
                    return Err(Reject::CannotBuild);
                }
                let (pos, well) = self.check_placement(*def, *pos)?;
                self.afford(player, *def)?;
                self.pay(player, *def, 1);
                let site = self.spawn_owned(*def, player, pos);
                let s = self.entities.get_mut(site).unwrap();
                s.construction = Some(0);
                s.on_well = well;
                let b = self.entities.get_mut(*builder).unwrap();
                b.order = Order::Construct { site };
                b.home = None;
                Ok(())
            }
            CommandKind::Construct { units, site } => {
                let s = self.owned(player, *site)?;
                if s.construction.is_none() {
                    return Err(Reject::NoSuchEntity);
                }
                for u in self.own_units(player, units, |d| d.is_constructor())? {
                    let e = self.entities.get_mut(u).unwrap();
                    e.order = Order::Construct { site: *site };
                    e.home = None;
                }
                Ok(())
            }
            CommandKind::Train { producer, def } => {
                let p = self.owned(player, *producer)?;
                let pd = self.rules.def(p.def);
                if !p.is_complete() || !pd.produces.contains(def) {
                    return Err(Reject::CannotProduce);
                }
                if p.queue.len() >= MAX_QUEUE {
                    return Err(Reject::QueueFull);
                }
                if let (Some(_), Some(BuildingRole::SupplyCenter { collector_cap })) =
                    (self.rules.def(*def).collector(), pd.role())
                    && self.depot_load(*producer) >= collector_cap
                {
                    return Err(Reject::DepotFull);
                }
                self.afford(player, *def)?;
                self.pay(player, *def, 1);
                self.entities.get_mut(*producer).unwrap().queue.push(Production { def: *def, progress_halves: 0 });
                Ok(())
            }
            CommandKind::CancelTrain { producer } => {
                self.owned(player, *producer)?;
                let item = self.entities.get_mut(*producer).unwrap().queue.pop().ok_or(Reject::CannotProduce)?;
                self.pay(player, item.def, -1);
                Ok(())
            }
            CommandKind::Stop { units } => {
                for u in self.own_units(player, units, |_| true)? {
                    let e = self.entities.get_mut(u).unwrap();
                    e.order = Order::Idle;
                    e.home = None;
                }
                Ok(())
            }
            CommandKind::Surrender => {
                self.players[player.0 as usize].defeated = true;
                Ok(())
            }
        }
    }

    /// The subset of `ids` that are live units owned by `player` and match `filter`.
    fn own_units(
        &self,
        player: PlayerId,
        ids: &[EntityId],
        filter: impl Fn(&crate::defs::Def) -> bool,
    ) -> Result<Vec<EntityId>, Reject> {
        let mut out: Vec<EntityId> = ids
            .iter()
            .copied()
            .filter(|&id| {
                self.entities.get(id).is_some_and(|e| {
                    let d = self.rules.def(e.def);
                    e.owner == Some(player) && d.is_unit() && filter(d)
                })
            })
            .collect();
        out.dedup();
        if out.is_empty() { Err(Reject::NotOwner) } else { Ok(out) }
    }

    fn radius_of(&self, id: EntityId) -> Fx {
        self.entities.get(id).map_or(Fx::ZERO, |e| self.rules.def(e.def).radius)
    }

    fn update_power(&mut self) {
        for p in &mut self.players {
            p.power_supply = 0;
            p.power_drain = 0;
        }
        // (source id, owner, pos, radius, remaining capacity)
        let mut sources: Vec<(EntityId, PlayerId, Vec2, Fx, i32)> = Vec::new();
        for e in self.entities.iter() {
            if let (Class::Building { power, power_radius, .. }, Some(owner), true) =
                (self.rules.def(e.def).class, e.owner, e.is_complete())
                && power > 0
            {
                sources.push((e.id, owner, e.pos, power_radius, power));
                self.players[owner.0 as usize].power_supply += power;
            }
        }
        for id in self.entities.ids() {
            let e = self.entities.get(id).unwrap();
            let Class::Building { power, .. } = self.rules.def(e.def).class else { continue };
            let Some(owner) = e.owner else { continue };
            let faction = self.players[owner.0 as usize].faction;
            let powered = if !faction.uses_power() || power >= 0 {
                true
            } else if !e.is_complete() {
                false
            } else {
                let demand = -power;
                self.players[owner.0 as usize].power_drain += demand;
                // First source in index order that reaches this consumer and has capacity left.
                match sources
                    .iter_mut()
                    .find(|(_, o, pos, r, left)| *o == owner && *left >= demand && pos.distance(e.pos) <= *r)
                {
                    Some(src) => {
                        src.4 -= demand;
                        true
                    }
                    None => false,
                }
            };
            self.entities.get_mut(id).unwrap().powered = powered;
        }
    }

    fn update_production(&mut self) {
        for id in self.entities.ids() {
            let e = self.entities.get(id).unwrap();
            if !e.is_complete() || e.queue.is_empty() {
                continue;
            }
            let need = self.build_ticks_for(e.owner.unwrap(), e.queue[0].def) * 2;
            let e = self.entities.get_mut(id).unwrap();
            let step = if e.powered { 2 } else { 1 };
            let front = &mut e.queue[0];
            front.progress_halves += step;
            let def = front.def;
            if front.progress_halves < need {
                continue;
            }
            e.queue.remove(0);
            let (owner, pos, radius) = (e.owner.unwrap(), e.pos, self.rules.def(e.def).radius);
            let is_depot = matches!(self.rules.def(e.def).role(), Some(BuildingRole::SupplyCenter { .. }));
            let ud = self.rules.def(def);
            let spawn_at = self.map.clamp(pos + Vec2::new(Fx::ZERO, radius + ud.radius + SLACK));
            let unit = self.spawn_owned(def, owner, spawn_at);
            if is_depot
                && ud.collector().is_some()
                && let Some(pile) = self.nearest_pile(pos)
            {
                let u = self.entities.get_mut(unit).unwrap();
                u.home = Some(id);
                u.order = Order::Gather { pile, phase: GatherPhase::ToPile };
            }
        }
    }

    fn update_construction(&mut self) {
        for site in self.entities.ids() {
            let s = self.entities.get(site).unwrap();
            let Some(progress) = s.construction else { continue };
            let reach = self.rules.def(s.def).radius + SLACK;
            let working = self.entities.iter().any(|b| {
                b.order == Order::Construct { site } && b.pos.distance(s.pos) <= reach + self.rules.def(b.def).radius
            });
            if !working {
                continue;
            }
            let progress = progress + 1;
            let done = progress >= self.build_ticks_for(s.owner.unwrap(), s.def);
            self.entities.get_mut(site).unwrap().construction = (!done).then_some(progress);
            if done {
                for b in self.entities.ids() {
                    let b = self.entities.get_mut(b).unwrap();
                    if b.order == (Order::Construct { site }) {
                        b.order = Order::Idle;
                    }
                }
            }
        }
    }

    /// Rebuilds the blocked-cell grid when the set of statics (buildings, foundations, piles,
    /// wells) changed since the last build. Units notice through `route_key`.
    fn refresh_nav(&mut self) {
        let statics: Vec<(Vec2, Fx)> = self
            .entities
            .iter()
            .filter(|e| !self.rules.def(e.def).is_unit())
            .map(|e| (e.pos, self.rules.def(e.def).radius))
            .collect();
        let key = statics.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &(p, r)| {
            [p.x.0, p.y.0, r.0].iter().fold(h, |h, &v| (h ^ v as u64).wrapping_mul(0x0000_0100_0000_01b3))
        });
        if key != self.nav_key {
            self.nav = crate::nav::Grid::new(self.map.width, self.map.height, &statics, &self.map.terrain);
            self.nav_key = key;
        }
    }

    /// Moves unit `id` toward a point `contact` away from `target`, routing around statics.
    /// Returns the new position and whether the unit is in contact.
    ///
    /// A unit walks straight while nothing blocks the straight line (the common case, and the
    /// pre-pathfinding behaviour); otherwise it plans a route once and follows its waypoints,
    /// replanning when the grid changes or the goal moves.
    fn approach(&mut self, id: EntityId, target: Vec2, contact: Fx, speed: Fx) -> (Vec2, bool) {
        let e = self.entities.get(id).unwrap();
        let pos = e.pos;
        let delta = pos - target;
        let dist = delta.length();
        if dist <= contact + SLACK {
            self.entities.get_mut(id).unwrap().route.clear();
            return (pos, true);
        }
        let stop = target + delta.scale(contact / dist);
        // A plan is kept while the grid is unchanged and it still ends near the goal (the
        // contact point drifts as the unit moves, so compare against the target).
        let planned = !e.route.is_empty()
            && e.route_key == self.nav_key
            && e.route[0].distance(target) <= contact + Fx::from_int(4);
        if !planned {
            let route = if self.straight_is_free(pos, stop) {
                Vec::new()
            } else {
                let reach = contact + SLACK;
                let mut r = self.nav.find(pos, stop, |c| c.distance(target) <= reach + Fx::ONE).unwrap_or_default();
                r.reverse();
                r
            };
            let u = self.entities.get_mut(id).unwrap();
            u.route = route;
            u.route_key = self.nav_key;
        }
        let u = self.entities.get_mut(id).unwrap();
        let mut left = speed;
        let mut p = pos;
        // Waypoints are stored goal-first, so the next one is `last()`.
        while let Some(&wp) = u.route.last() {
            let d = p.distance(wp);
            if d > left {
                p = p.step_toward(wp, left);
                left = Fx::ZERO;
                break;
            }
            p = wp;
            left -= d;
            u.route.pop();
        }
        if u.route.is_empty() && left > Fx::ZERO {
            let delta = p - target;
            let dist = delta.length();
            if dist > contact {
                p = p.step_toward(target + delta.scale(contact / dist), left);
            }
        }
        (p, p.distance(target) <= contact + SLACK)
    }

    /// Whether the straight segment `a`→`b` crosses only free cells, apart from the cells at
    /// its ends (a unit may stand at, and arrive at, an obstacle's edge).
    fn straight_is_free(&self, a: Vec2, b: Vec2) -> bool {
        let steps = (a.distance(b).floor_int() / crate::nav::CELL).max(1);
        let (ca, cb) = (self.nav.cell(a), self.nav.cell(b));
        (1..steps).all(|k| {
            let c = self.nav.cell(a + (b - a).scale(Fx::from_ratio(k, steps)));
            c == ca || c == cb || !self.nav.blocked(c.0, c.1)
        })
    }

    fn update_unit(&mut self, id: EntityId) {
        let e = self.entities.get(id).unwrap();
        let d = self.rules.def(e.def);
        let (pos, speed, radius, owner, order) = (e.pos, d.speed(), d.radius, e.owner.unwrap(), e.order);
        let (new_pos, new_order) = match order {
            Order::Idle => (pos, Order::Idle),
            Order::Move { target } => {
                let (p, arrived) = self.approach(id, target, Fx::ZERO, speed);
                (p, if arrived { Order::Idle } else { order })
            }
            Order::Construct { site } => match self.entities.get(site).filter(|s| s.construction.is_some()) {
                None => (pos, Order::Idle),
                Some(s) => {
                    let (site_pos, site_r) = (s.pos, self.rules.def(s.def).radius);
                    let (p, _) = self.approach(id, site_pos, site_r + radius, speed);
                    (p, order)
                }
            },
            Order::Gather { pile, phase } => self.gather_step(id, pile, phase, pos, speed, radius, owner),
        };
        let e = self.entities.get_mut(id).unwrap();
        e.pos = self.map.clamp(new_pos);
        // A route belongs to one goal; a new order or gather phase plans its own.
        if new_order != order {
            e.route.clear();
        }
        e.order = new_order;
        if new_order == Order::Idle {
            e.home = None;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn gather_step(
        &mut self,
        id: EntityId,
        pile: EntityId,
        phase: GatherPhase,
        pos: Vec2,
        speed: Fx,
        radius: Fx,
        owner: PlayerId,
    ) -> (Vec2, Order) {
        let spec = self.rules.def(self.entities.get(id).unwrap().def).collector().unwrap();
        let live_pile = |w: &World, p: EntityId| {
            w.entities.get(p).filter(|e| e.resource > 0).map(|e| (e.pos, w.rules.def(e.def).radius))
        };
        let live_depot = |w: &World, d: EntityId| {
            w.entities
                .get(d)
                .filter(|e| e.owner == Some(owner) && e.is_complete())
                .map(|e| (e.pos, w.rules.def(e.def).radius))
        };
        match phase {
            GatherPhase::ToPile => {
                let (pile, (ppos, prad)) = match live_pile(self, pile) {
                    Some(info) => (pile, info),
                    None => match self.nearest_pile(pos) {
                        Some(next) => (next, live_pile(self, next).unwrap()),
                        None => return (pos, Order::Idle),
                    },
                };
                let (p, arrived) = self.approach(id, ppos, prad + radius, speed);
                let phase =
                    if arrived { GatherPhase::Loading { ticks_left: spec.load_ticks } } else { GatherPhase::ToPile };
                (p, Order::Gather { pile, phase })
            }
            GatherPhase::Loading { ticks_left } if ticks_left > 1 => {
                (pos, Order::Gather { pile, phase: GatherPhase::Loading { ticks_left: ticks_left - 1 } })
            }
            GatherPhase::Loading { .. } => {
                let Some(pe) = self.entities.get_mut(pile).filter(|e| e.resource > 0) else {
                    return (pos, Order::Gather { pile, phase: GatherPhase::ToPile });
                };
                let take = spec.capacity.min(pe.resource);
                pe.resource -= take;
                let u = self.entities.get_mut(id).unwrap();
                u.cargo += take;
                let home = u.home;
                let depot = home.filter(|&h| live_depot(self, h).is_some()).or_else(|| self.find_depot(owner, pos));
                match depot {
                    Some(depot) => {
                        self.entities.get_mut(id).unwrap().home = Some(depot);
                        (pos, Order::Gather { pile, phase: GatherPhase::ToDepot { depot } })
                    }
                    None => (pos, Order::Idle),
                }
            }
            GatherPhase::ToDepot { depot } => match live_depot(self, depot) {
                None => match self.find_depot(owner, pos) {
                    Some(next) => {
                        self.entities.get_mut(id).unwrap().home = Some(next);
                        (pos, Order::Gather { pile, phase: GatherPhase::ToDepot { depot: next } })
                    }
                    None => (pos, Order::Idle),
                },
                Some((dpos, drad)) => {
                    let (p, arrived) = self.approach(id, dpos, drad + radius, speed);
                    let phase = if arrived {
                        GatherPhase::Unloading { depot, ticks_left: spec.unload_ticks }
                    } else {
                        GatherPhase::ToDepot { depot }
                    };
                    (p, Order::Gather { pile, phase })
                }
            },
            GatherPhase::Unloading { depot, ticks_left } if ticks_left > 1 => {
                (pos, Order::Gather { pile, phase: GatherPhase::Unloading { depot, ticks_left: ticks_left - 1 } })
            }
            GatherPhase::Unloading { depot, .. } => {
                let u = self.entities.get_mut(id).unwrap();
                let cargo = core::mem::take(&mut u.cargo);
                self.players[owner.0 as usize].credits += cargo;
                self.events.push(Event::Delivered { player: owner, depot, credits: cargo });
                (pos, Order::Gather { pile, phase: GatherPhase::ToPile })
            }
        }
    }

    /// Pushes overlapping units apart and out of structures. Pairwise in index order.
    ///
    /// Collectors are exempt from unit-unit pushing: they shuttle the same narrow lanes between
    /// a pile and a depot in both directions, and two of them meeting head-on in a gap between
    /// buildings would otherwise block each other for good.
    fn separate_units(&mut self) {
        let ids = self.entities.ids();
        let units: Vec<EntityId> =
            ids.iter().copied().filter(|&id| self.rules.def(self.entities.get(id).unwrap().def).is_unit()).collect();
        let statics: Vec<(Vec2, Fx)> = ids
            .iter()
            .filter_map(|&id| {
                let e = self.entities.get(id).unwrap();
                let d = self.rules.def(e.def);
                (!d.is_unit()).then_some((e.pos, d.radius))
            })
            .collect();
        for i in 0..units.len() {
            for j in i + 1..units.len() {
                let (a, b) = (self.entities.get(units[i]).unwrap(), self.entities.get(units[j]).unwrap());
                let (ad, bd) = (self.rules.def(a.def), self.rules.def(b.def));
                if ad.collector().is_some() || bd.collector().is_some() {
                    continue;
                }
                let min = ad.radius + bd.radius;
                let delta = b.pos - a.pos;
                let dist = delta.length();
                if dist >= min {
                    continue;
                }
                let dir = if dist == Fx::ZERO { Vec2::new(Fx::ONE, Fx::ZERO) } else { delta.scale(Fx::ONE / dist) };
                let push = dir.scale((min - dist) / Fx::from_int(2));
                let (apos, bpos) = (a.pos - push, b.pos + push);
                self.entities.get_mut(units[i]).unwrap().pos = self.map.clamp(apos);
                self.entities.get_mut(units[j]).unwrap().pos = self.map.clamp(bpos);
            }
        }
        for &u in &units {
            let e = self.entities.get(u).unwrap();
            let r = self.rules.def(e.def).radius;
            let mut pos = e.pos;
            for &(spos, srad) in &statics {
                let delta = pos - spos;
                let dist = delta.length();
                if dist < srad + r {
                    let dir = if dist == Fx::ZERO { Vec2::new(Fx::ONE, Fx::ZERO) } else { delta.scale(Fx::ONE / dist) };
                    pos = spos + dir.scale(srad + r);
                }
            }
            self.entities.get_mut(u).unwrap().pos = self.map.clamp(pos);
        }
    }

    fn update_oil(&mut self) {
        for id in self.entities.ids() {
            let e = self.entities.get(id).unwrap();
            let Some(BuildingRole::OilDerrick { oil_per_second }) = self.rules.def(e.def).role() else { continue };
            let (Some(well), Some(owner), true) = (e.on_well, e.owner, e.is_complete()) else { continue };
            let e = self.entities.get_mut(id).unwrap();
            e.income_ticks += 1;
            if e.income_ticks < TICKS_PER_SECOND {
                continue;
            }
            e.income_ticks = 0;
            let Some(w) = self.entities.get_mut(well) else { continue };
            let amount = oil_per_second.min(w.resource);
            w.resource -= amount;
            self.players[owner.0 as usize].oil += amount;
        }
    }

    fn cleanup(&mut self) {
        for id in self.entities.ids() {
            let e = self.entities.get(id).unwrap();
            let depleted = matches!(self.rules.def(e.def).class, Class::SupplyPile) && e.resource <= 0;
            let owner_out = e.owner.is_some_and(|o| self.players[o.0 as usize].defeated);
            if depleted || e.hp <= 0 || owner_out {
                self.entities.despawn(id);
            }
        }
    }

    fn update_defeat(&mut self) {
        for (i, p) in self.players.iter_mut().enumerate() {
            if p.defeated {
                continue;
            }
            let owner = Some(PlayerId(i as u8));
            let alive = self.entities.iter().any(|e| {
                let d = self.rules.def(e.def);
                e.owner == owner && (d.is_building() || d.is_constructor())
            });
            p.defeated = !alive;
        }
    }
}

/// Smallest `s >= 1` with `s * s >= n`.
fn ceil_sqrt(n: i64) -> i64 {
    let mut s = 1;
    while s * s < n {
        s += 1;
    }
    s
}

impl Rules {
    fn single(&self, pred: impl Fn(&crate::defs::Def) -> bool, what: &str) -> DefId {
        let mut hits = self.defs.iter().enumerate().filter(|(_, d)| pred(d));
        let (i, _) = hits.next().unwrap_or_else(|| panic!("rules define no {what}"));
        assert!(hits.next().is_none(), "rules define more than one {what}");
        DefId(i as u16)
    }

    pub fn supply_pile(&self) -> DefId {
        self.single(|d| matches!(d.class, Class::SupplyPile), "supply pile")
    }

    pub fn oil_well(&self) -> DefId {
        self.single(|d| matches!(d.class, Class::OilWell), "oil well")
    }

    pub fn command_center(&self, faction: Faction) -> DefId {
        self.single(|d| d.faction == Some(faction) && d.role() == Some(BuildingRole::CommandCenter), "command center")
    }

    /// The constructor a faction starts with: the first unit its command center produces.
    pub fn starting_constructor(&self, faction: Faction) -> DefId {
        let cc = self.def(self.command_center(faction));
        *cc.produces.iter().find(|&&u| self.def(u).is_constructor()).expect("command center produces no constructor")
    }
}
