use crate::ai::Ai;
use crate::command::{Command, CommandKind, Reject};
use crate::defs::{BuildingRole, Class, DefId, Effect, Faction, PowerKind, Rules, UpgradeId};
use crate::entity::{
    DeployState, Entities, Entity, EntityId, GatherPhase, Order, PlayerId, Product, Production, Route,
};
use crate::fixed::{Fx, Vec2};
use crate::map::{AiLevel, MapDef, MatchSetup};
use crate::rng::Pcg32;

/// Logic rate, matching SAGE's `BaseFps`.
pub const TICKS_PER_SECOND: u32 = 30;
/// Ticks between a command being issued and taking effect; leaves room for lockstep networking.
pub const INPUT_DELAY: u32 = 2;
pub const MAX_QUEUE: usize = 5;
/// Distance tolerance for "has reached" checks, so units pushed apart by separation still arrive.
const SLACK: Fx = Fx::from_int(1);
/// Armed units notice enemies this far beyond their weapon range (idle guard, attack-move).
const ACQUIRE: Fx = Fx::from_int(15);
/// A unit keeps its planned route while its target moves less than this from where the route
/// was planned: replanning a chase every tick would run an A* per attacker per tick.
const REPLAN: Fx = Fx::from_int(8);
/// The `shooter` of shells from a general's power: no entity fired them. Never a live id
/// (generation `u32::MAX` is never reached), so `World::entity` finds nothing for it.
pub const POWER_SOURCE: EntityId = EntityId { index: u32::MAX, generation: u32::MAX };
/// A stealth unit that fires is revealed for this long: the alpha's `StealthDisableOnFire`
/// (2.5 s on every weapon).
pub const STEALTH_REVEAL_TICKS: u32 = 5 * TICKS_PER_SECOND / 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Player {
    pub faction: Faction,
    /// Chosen general; its modifiers apply to every cost and build time this player pays.
    pub general: Option<crate::defs::GeneralId>,
    pub credits: i64,
    pub power_supply: i32,
    pub power_drain: i32,
    pub defeated: bool,
    /// Per power slot (see `defs::powers`): the first tick it can be used again; `None` for an
    /// empty slot (slot 0 without a general power).
    pub power_ready: Vec<Option<u32>>,
    /// Researched upgrades, in ascending id order.
    pub upgrades: Vec<UpgradeId>,
    /// A stealth unit has hit one of this player's entities: they know stealth is in play.
    pub hit_by_stealth: bool,
    /// Perks slotted for this match, from `PlayerSetup::perks`.
    pub perks: Vec<crate::defs::PerkId>,
    /// Rank with the general, from `PlayerSetup::rank`: sets its power's level.
    pub rank: u8,
}

/// Running totals for the end-of-match screen. Output only: nothing in the simulation reads
/// them, so they are not part of the state hash, but they are exact on every replay.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Credits from collector deliveries and oil derricks.
    pub earned: i64,
    /// Credits paid for buildings and units, less refunds from cancels.
    pub spent: i64,
    pub units_built: u32,
    pub buildings_built: u32,
    /// Enemy units and buildings this player's fire destroyed.
    pub kills: u32,
    pub units_lost: u32,
    pub buildings_lost: u32,
    /// Tick the player was defeated on.
    pub defeated_at: Option<u32>,
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
    /// `shooter` hit `target` (standing at `at`) for `damage`; `killed` when that destroyed
    /// it; `splash` when the damage came from a shell's blast rather than direct fire. `by` is
    /// the shooter's player (a power's too), `victim` the target's owner. Output only, like
    /// `Delivered`.
    Hit {
        shooter: EntityId,
        target: EntityId,
        at: Vec2,
        damage: i32,
        killed: bool,
        splash: bool,
        /// What `target` was, for drawing what it leaves behind once it is swept away.
        target_def: DefId,
        by: PlayerId,
        victim: Option<PlayerId>,
    },
    /// An artillery shell left `shooter` at `from`; it lands at `at` on tick `lands`. Output
    /// only, for drawing the flight.
    Fired {
        shooter: EntityId,
        from: Vec2,
        at: Vec2,
        lands: u32,
        source: ShellSource,
    },
    /// A shell landed at `at` with blast `radius`; each unit it damaged is a `Hit`. Output only.
    Landed {
        at: Vec2,
        radius: Fx,
        source: ShellSource,
    },
    /// `player`'s production finished a `def` unit at `at`. Output only.
    Produced {
        player: PlayerId,
        def: DefId,
        at: Vec2,
    },
    /// `player` finished constructing a `def` building at `at`. Output only.
    Completed {
        player: PlayerId,
        def: DefId,
        at: Vec2,
    },
    /// `player` used their power in `slot` at `at`. Output only.
    PowerUsed {
        player: PlayerId,
        slot: u8,
        at: Vec2,
    },
    /// `player` finished researching `upgrade` at the building standing at `at`. Output only.
    Researched {
        player: PlayerId,
        upgrade: UpgradeId,
        at: Vec2,
    },
    /// `player`'s `unit`, standing at `at`, reached `rank` (1-3). Output only.
    Promoted {
        player: PlayerId,
        unit: EntityId,
        rank: u8,
        at: Vec2,
    },
    /// `player`'s `unit` got into `container` at `at`. Output only.
    Garrisoned {
        player: PlayerId,
        unit: EntityId,
        container: EntityId,
        at: Vec2,
    },
    /// `player`'s unit of def `from` at `at` turned into a `into` (the Battle Bus into its
    /// bunker). Output only.
    Transformed {
        player: PlayerId,
        from: DefId,
        into: DefId,
        at: Vec2,
    },
    /// `shooter` fired its special shot (`Def::special`) at `target` from `from` to `at`;
    /// its damage is a `Hit` on the same tick. Output only, for drawing it.
    SpecialFired {
        shooter: EntityId,
        target: EntityId,
        from: Vec2,
        at: Vec2,
    },
}

/// What sent a shell, for drawing it: the weapon of a unit def (which outlives the shooter,
/// gone or packed up by the time it lands) or a player's power. Output only: not hashed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellSource {
    /// A unit of this def, deployed (its `deploy` weapon) or not.
    Gun { def: DefId, deployed: bool },
    /// `player`'s power in `slot`.
    Power { player: PlayerId, slot: u8 },
}

/// An artillery shell in flight. Part of the state hash, except `source`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShellInFlight {
    pub shooter: EntityId,
    pub owner: PlayerId,
    pub from: Vec2,
    pub at: Vec2,
    /// Tick it was fired on, and the tick on which it lands.
    pub fired: u32,
    pub lands: u32,
    pub damage: i32,
    pub bonus: [i32; 4],
    pub radius: Fx,
    /// Extra damage against a building with units inside when the shell lands.
    pub garrison_bonus: i32,
    pub source: ShellSource,
}

/// A lingering toxin cloud from a missile power. Part of the state hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cloud {
    pub owner: PlayerId,
    pub at: Vec2,
    pub radius: Fx,
    /// Damage each second to every enemy ground unit and building within `radius`.
    pub damage: i32,
    /// The cloud is active from `from` and gone at `until`.
    pub from: u32,
    pub until: u32,
}

pub struct World {
    pub rules: &'static Rules,
    pub map: MapDef,
    pub(crate) tick: u32,
    pub(crate) rng: Pcg32,
    pub(crate) players: Vec<Player>,
    pub(crate) entities: Entities,
    pending: Vec<Command>,
    pub(crate) events: Vec<Event>,
    pub(crate) ai: Vec<Option<Ai>>,
    /// Every human command processed so far, in processing order. AI commands are excluded:
    /// replays re-run the AI, so recording them would apply them twice.
    log: Vec<Command>,
    /// `(tick, state_hash)` taken before stepping every `HASH_INTERVAL`-th tick.
    checkpoints: Vec<(u32, u64)>,
    /// Blocked cells, rebuilt whenever the set of statics changes. A pure function of hashed
    /// state (static entities and the map), so it needs no hash of its own.
    pub(crate) nav: crate::nav::Grid,
    /// Hash of the statics the grid was built from; when it changes, every unit replans.
    nav_key: u64,
    /// `(shooter, shooter's owner, target, damage, splash)` fired this tick, applied together
    /// by `resolve_shots`. The owner travels with the shot because a shell's gun may be gone
    /// by the time it lands. Empty between ticks, so it needs no hash.
    shots: Vec<(EntityId, PlayerId, EntityId, i32, bool)>,
    /// Shells in flight, in firing order.
    pub(crate) shells: Vec<ShellInFlight>,
    /// Toxin clouds, in order of creation.
    pub(crate) clouds: Vec<Cloud>,
    /// What each player sees and has explored (index = player). Updated every tick before
    /// units act, so targeting only ever uses what is visible at the start of the tick.
    pub(crate) visions: Vec<crate::fog::Vision>,
    /// Per-player totals (index = player), output only.
    pub(crate) stats: Vec<Stats>,
    /// The mission in play (`crate::mission`); `None` in a skirmish.
    pub(crate) mission: Option<crate::mission::Run>,
}

impl World {
    pub fn new(setup: &MatchSetup, rules: &'static Rules) -> World {
        World::start(setup, rules, None)
    }

    /// A match of mission `id`: the mission places every side's forces instead of the
    /// skirmish's command center and constructor at each start. `setup` seats its sides
    /// (`Mission::setup`).
    pub fn with_mission(setup: &MatchSetup, rules: &'static Rules, id: crate::mission::MissionId) -> World {
        World::start(setup, rules, Some(id))
    }

    /// `new` or `with_mission`, as `mission` says: how replays start their match.
    pub fn start(setup: &MatchSetup, rules: &'static Rules, mission: Option<crate::mission::MissionId>) -> World {
        if let Some(id) = mission {
            assert!(rules.missions[id.0 as usize].seats(&setup.players), "the setup doesn't seat mission {}", id.0);
        }
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
            assert!(
                p.perks.iter().all(|k| p.general.is_some_and(|g| rules.general(g).perks.contains(k))),
                "perks {:?} are not all the general's",
                p.perks
            );
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
            shots: Vec::new(),
            shells: Vec::new(),
            clouds: Vec::new(),
            visions: setup.players.iter().map(|_| crate::fog::Vision::new(setup.map.width, setup.map.height)).collect(),
            stats: vec![Stats::default(); setup.players.len()],
            checkpoints: Vec::new(),
            mission: None,
        };
        for &(pos, amount) in &setup.map.supply_piles {
            world.spawn_neutral(rules.supply_pile(), pos, amount);
        }
        for &(pos, reserve) in &setup.map.oil_wells {
            world.spawn_neutral(rules.oil_well(), pos, reserve);
        }
        for &(pos, radius) in &setup.map.civilians {
            if let Some(def) = rules.civilian_for(radius) {
                world.spawn_neutral(def, pos, 0);
            }
        }
        for (i, p) in setup.players.iter().enumerate() {
            let owner = PlayerId(i as u8);
            world.players.push(Player {
                faction: p.faction,
                general: p.general,
                credits: p.starting_credits,
                power_supply: 0,
                power_drain: 0,
                defeated: false,
                // A power that needs a building starts unavailable: finishing one charges it.
                power_ready: crate::defs::powers(rules, p.faction, p.general, p.rank, &p.perks)
                    .iter()
                    .map(|pw| pw.filter(|pw| pw.requires.is_none()).map(|pw| pw.first_ticks))
                    .collect(),
                upgrades: Vec::new(),
                hit_by_stealth: false,
                perks: p.perks.clone(),
                rank: p.rank,
            });
            world.ai.push(p.ai.map(|level| Ai::new(owner, level)));
            // The onslaught has no base: its waves are all there is of it. A mission places
            // every side's forces itself.
            if p.ai == Some(AiLevel::Onslaught) || mission.is_some() {
                continue;
            }
            let spawn = setup.map.spawns[i];
            let cc = rules.command_center(p.faction);
            let cc_id = world.spawn_owned(cc, owner, spawn);
            let dozer = rules.starting_constructor(p.faction);
            let offset = rules.def(cc).radius + rules.def(dozer).radius + SLACK;
            let offset = offset * world.toward_centre(spawn);
            world.spawn_owned(dozer, owner, world.map.clamp(spawn + Vec2::new(Fx::ZERO, offset)));
            debug_assert!(world.entities.get(cc_id).is_some());
        }
        if let Some(id) = mission {
            world.start_mission(id);
        }
        world.update_power();
        world.update_vision();
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
            cooldown: 0,
            deploy: DeployState::Packed,
            last_hit: None,
            experience: 0,
            inside: None,
            revealed_until: None,
            cloaked_until: None,
            special_at: 0,
            rally: None,
            route: Route::default(),
        }
    }

    fn spawn_neutral(&mut self, def: DefId, pos: Vec2, resource: i64) -> EntityId {
        let hp = self.rules.def(def).hp;
        self.entities.spawn(|id| Entity { resource, ..Self::blank_entity(id, def, hp, pos) })
    }

    /// At full health, including any health upgrades the owner already researched.
    pub(crate) fn spawn_owned(&mut self, def: DefId, owner: PlayerId, pos: Vec2) -> EntityId {
        let hp = self.max_hp_of(Some(owner), def);
        self.entities.spawn(|id| Entity { owner: Some(owner), ..Self::blank_entity(id, def, hp, pos) })
    }

    /// `+1` or `-1`: the vertical direction from `pos` toward the map centre. Units spawn on
    /// that side of their building, so starts that mirror each other produce mirrored units.
    fn toward_centre(&self, pos: Vec2) -> Fx {
        if pos.y * Fx::from_int(2) > self.map.height { -Fx::ONE } else { Fx::ONE }
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

    /// Artillery shells in the air, in firing order.
    pub fn shells(&self) -> &[ShellInFlight] {
        &self.shells
    }

    /// Toxin clouds, active and still to come.
    pub fn clouds(&self) -> &[Cloud] {
        &self.clouds
    }

    /// Match totals for player `id`.
    pub fn stats(&self, id: PlayerId) -> &Stats {
        &self.stats[id.0 as usize]
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
            mission: self.mission.as_ref().map(|m| m.id),
        }
    }

    pub fn is_ai(&self, player: PlayerId) -> bool {
        self.ai.get(player.0 as usize).is_some_and(Option::is_some)
    }

    /// The built-in AI's level for `player`; `None` for a human.
    pub fn ai_level(&self, player: PlayerId) -> Option<AiLevel> {
        self.ai.get(player.0 as usize).and_then(|a| a.as_ref()).map(|a| a.level())
    }

    /// The last player standing, once every other player is defeated. In a mission, the
    /// mission's result decides: the player (slot 0) on a win, the first enemy on a loss.
    pub fn winner(&self) -> Option<PlayerId> {
        if let Some(run) = &self.mission {
            return run.result.map(|won| if won { crate::mission::PLAYER } else { PlayerId(1) });
        }
        let mut alive = self.players.iter().enumerate().filter(|(_, p)| !p.defeated);
        let (first, _) = alive.next()?;
        alive.next().is_none().then_some(PlayerId(first as u8))
    }

    /// Collectors bound to `center` plus collectors queued there.
    pub fn depot_load(&self, center: EntityId) -> u32 {
        let bound = self.entities.iter().filter(|e| e.home == Some(center)).count();
        let queued = self.entities.get(center).map_or(0, |c| {
            c.queue.iter().filter_map(|p| p.unit()).filter(|&d| self.rules.def(d).collector().is_some()).count()
        });
        (bound + queued) as u32
    }

    /// Whether `player` has researched `upgrade` or has it in any production queue.
    pub fn researched_or_queued(&self, player: PlayerId, upgrade: UpgradeId) -> bool {
        self.player(player).upgrades.contains(&upgrade)
            || self
                .entities
                .iter()
                .any(|e| e.owner == Some(player) && e.queue.iter().any(|p| p.item == Product::Upgrade(upgrade)))
    }

    /// Effects on `player`'s `def`: their finished upgrades', then their slotted perks'.
    pub fn effects(&self, player: PlayerId, def: DefId) -> impl Iterator<Item = Effect> + '_ {
        let p = self.player(player);
        let upgrades = p.upgrades.iter().flat_map(|&u| self.rules.upgrade(u).effects.iter());
        let perks = p.perks.iter().flat_map(|&k| self.rules.perk(k).effects.iter());
        upgrades.chain(perks).filter(move |&&(d, _)| d == def).map(|&(_, e)| e)
    }

    /// `player`'s powers by slot: their general's (at their rank's level, as their perks tune
    /// it; `None` without one), then their faction's support powers.
    pub fn powers_of(&self, player: PlayerId) -> Vec<Option<crate::defs::Power>> {
        let p = self.player(player);
        crate::defs::powers(self.rules, p.faction, p.general, p.rank, &p.perks)
    }

    /// What `maker` (a factory def, or a constructor def) offers `player`: its own list, with
    /// their general's units.
    pub fn offers_of(&self, player: PlayerId, maker: DefId) -> Vec<DefId> {
        self.rules.offers_for(self.player(player).general, maker)
    }

    /// `player`'s power in `slot`, if they have one there.
    pub fn power_in(&self, player: PlayerId, slot: u8) -> Option<crate::defs::Power> {
        self.powers_of(player).get(slot as usize).copied().flatten()
    }

    /// Ticks until `player` can use `slot` (0: now); `None` for an empty slot, or one whose
    /// required building the player doesn't have.
    pub fn power_wait(&self, player: PlayerId, slot: u8) -> Option<u32> {
        let ready = self.player(player).power_ready.get(slot as usize).copied().flatten()?;
        Some(ready.saturating_sub(self.tick))
    }

    /// Charges or disables the powers that need a building: one whose required building
    /// `player` now has a finished copy of, and had none before, starts charging; one whose
    /// last copy is gone becomes unavailable.
    fn update_power_sites(&mut self) {
        for p in 0..self.players.len() {
            let player = PlayerId(p as u8);
            for (slot, power) in self.powers_of(player).into_iter().enumerate() {
                let Some(def) = power.and_then(|pw| pw.requires) else { continue };
                let has = self
                    .entities
                    .iter()
                    .any(|e| e.owner == Some(player) && e.def == def && e.is_complete() && e.hp > 0);
                let ready = &mut self.players[p].power_ready[slot];
                match (has, *ready) {
                    (true, None) => *ready = Some(self.tick + power.unwrap().first_ticks),
                    (false, Some(_)) => *ready = None,
                    _ => {}
                }
            }
        }
    }

    /// Maximum hit points: the def's, plus any health upgrades its owner researched and its
    /// rank's extra health.
    pub fn max_hp(&self, e: &Entity) -> i32 {
        let rank = self.rules.def(e.def).veterancy.filter(|_| self.rank(e) >= 1).map_or(0, |v| v.health);
        self.max_hp_of(e.owner, e.def) + rank
    }

    /// A fresh entity's maximum hit points: the def's plus its owner's health upgrades.
    fn max_hp_of(&self, owner: Option<PlayerId>, def: DefId) -> i32 {
        let extra = owner.map_or(0, |o| {
            self.effects(o, def)
                .map(|x| match x {
                    Effect::Health { add } => add,
                    _ => 0,
                })
                .sum()
        });
        self.rules.def(def).hp + extra
    }

    /// 0 for a recruit, then 1-3 (the alpha's veteran ranks) as experience reaches each of its
    /// def's thresholds. Always 0 for defs without veterancy.
    pub fn rank(&self, e: &Entity) -> u8 {
        rank_for(self.rules.def(e.def).veterancy, e.experience)
    }

    /// `w` as `owner`'s upgrades leave it for a `def`: damage and fire rate. Percentages of a
    /// kind add up, so the result does not depend on research order.
    fn upgraded_weapon(&self, owner: PlayerId, def: DefId, mut w: crate::defs::Weapon) -> crate::defs::Weapon {
        let (mut all, mut vs, mut cooldown, mut any) = (0, [0; 4], 0, false);
        for x in self.effects(owner, def) {
            match x {
                Effect::Damage { percent, against: None } => all += percent - 100,
                Effect::Damage { percent, against: Some(a) } => vs[a as usize] += percent - 100,
                Effect::ShellDamage { percent } if w.shell.is_some() => all += percent - 100,
                Effect::Cooldown { percent } => cooldown += percent as i32 - 100,
                _ => continue,
            }
            any = true;
        }
        if !any {
            return w;
        }
        let damage = w.damage * (100 + all) / 100;
        for (a, bonus) in w.bonus.iter_mut().enumerate() {
            *bonus = (w.damage + *bonus) * (100 + all + vs[a]) / 100 - damage;
        }
        w.damage = damage;
        w.cooldown_ticks = (w.cooldown_ticks as i32 * (100 + cooldown) / 100).max(1) as u32;
        w
    }

    /// `w` as unit `e` fires it: its owner's upgrades, then a rank-3 unit's faster fire.
    pub(crate) fn unit_weapon(&self, e: &Entity, w: crate::defs::Weapon) -> crate::defs::Weapon {
        let mut w = match e.owner {
            Some(owner) => self.upgraded_weapon(owner, e.def, w),
            None => w,
        };
        if let Some(v) = self.rules.def(e.def).veterancy.filter(|_| self.rank(e) >= 3) {
            w.cooldown_ticks = (w.cooldown_ticks * v.cooldown_percent / 100).max(1);
        }
        w
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
        self.depot_for(player, near, None)
    }

    /// The nearest complete supply center of `player` with room under its collector cap; the
    /// collector's own `home` always has room for it, since it already counts there.
    fn depot_for(&self, player: PlayerId, near: Vec2, home: Option<EntityId>) -> Option<EntityId> {
        self.nearest(near, |e| {
            e.owner == Some(player)
                && e.is_complete()
                && matches!(self.rules.def(e.def).role(), Some(BuildingRole::SupplyCenter { collector_cap })
                    if home == Some(e.id) || self.depot_load(e.id) < collector_cap)
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
        self.spawn_onslaught_waves();
        for id in self.entities.ids() {
            let Some(e) = self.entities.get(id) else { continue };
            let d = self.rules.def(e.def);
            if d.is_unit() {
                self.update_unit(id);
            } else if d.role() == Some(BuildingRole::Defense) {
                self.update_defense(id);
            }
        }
        // Shells fired on earlier ticks land before this tick's shots are applied, so their
        // damage joins the same simultaneous resolution.
        self.land_shells();
        self.poison_clouds();
        self.resolve_shots();
        // After the shots, so a unit hit this tick has its quiet spell restart before it heals.
        self.update_repairs();
        self.separate_units();
        // After separation, so a transport pushed aside carries its passengers along.
        self.carry_occupants();
        self.update_derricks();
        // Before defeat, so the side a mission's result puts out goes in the same tick.
        self.update_mission();
        // Defeat first, so an eliminated player's remaining units go in the same tick.
        self.update_defeat();
        self.cleanup();
        // After cleanup, so a superweapon destroyed this tick takes its power with it.
        self.update_power_sites();
        // Last, so the AI and every unit next tick see the state this tick ended in.
        self.update_vision();
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

    fn update_vision(&mut self) {
        crate::fog::update(&mut self.visions, &self.entities, self.rules, ACQUIRE, self.tick);
    }

    /// What `player` sees and has explored.
    pub fn vision(&self, player: PlayerId) -> &crate::fog::Vision {
        &self.visions[player.0 as usize]
    }

    /// Whether `player` sees entity `e` now: its cell is in view and, if it is stealthy, a
    /// detector of theirs has it in range or it fired lately. Own entities are always seen.
    pub fn sees(&self, player: PlayerId, e: &Entity) -> bool {
        if e.owner == Some(player) {
            return true;
        }
        let v = &self.visions[player.0 as usize];
        v.is_visible(e.pos) && (!self.hidden(e) || v.detected.contains(&e.id))
    }

    /// Whether `e` is stealthed right now: a stealth unit that hasn't fired in the last
    /// `STEALTH_REVEAL_TICKS`, or a unit under a cloaking power that hasn't fired since.
    pub fn hidden(&self, e: &Entity) -> bool {
        crate::fog::hidden(self.rules, e, self.tick)
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

    /// The units inside `container`, in entity order.
    pub fn occupants(&self, container: EntityId) -> Vec<EntityId> {
        self.entities.iter().filter(|e| e.inside == Some(container)).map(|e| e.id).collect()
    }

    /// Free places in `container` for `player`'s infantry: its garrison's capacity minus those
    /// already inside. `None` if it has no garrison, isn't finished, or isn't theirs to enter
    /// (an enemy's, or a neutral one someone else holds).
    pub fn garrison_room(&self, player: PlayerId, container: EntityId) -> Option<u32> {
        let c = self.entities.get(container).filter(|c| c.hp > 0 && c.is_complete())?;
        let g = self.rules.def(c.def).garrison?;
        let civilian = self.rules.def(c.def).role() == Some(BuildingRole::Civilian);
        if !(c.owner == Some(player) || (civilian && c.owner.is_none())) {
            return None;
        }
        Some(g.capacity.saturating_sub(self.occupants(container).len() as u32))
    }

    fn enterable(&self, player: PlayerId, container: EntityId) -> bool {
        self.garrison_room(player, container).is_some_and(|room| room > 0)
    }

    /// Everyone inside `container` steps out around it, idle; a civilian building left empty
    /// is neutral again.
    fn evacuate(&mut self, container: EntityId) {
        let Some(c) = self.entities.get(container) else { return };
        let (at, r) = (c.pos, self.rules.def(c.def).radius);
        for (k, u) in self.occupants(container).into_iter().enumerate() {
            let ur = self.radius_of(u);
            let ring = r + ur + SLACK;
            let spot = self.nav.free_point(self.map.clamp(at + around(k as u32, ring)));
            let e = self.entities.get_mut(u).unwrap();
            e.inside = None;
            e.pos = spot;
            e.order = Order::Idle;
            e.route = Route::default();
        }
        if self.rules.def(self.entities.get(container).unwrap().def).role() == Some(BuildingRole::Civilian) {
            self.entities.get_mut(container).unwrap().owner = None;
        }
    }

    /// Unit `id` turns into its `Def::becomes` where it stands: a new entity of that def at full
    /// health, the same owner, its occupants moved inside it; the unit itself goes. Instant: the
    /// alpha's instant bunker has no set-up time.
    fn transform(&mut self, id: EntityId) {
        let e = self.entities.get(id).unwrap();
        let (Some(into), Some(owner), pos, def) = (self.rules.def(e.def).becomes, e.owner, e.pos, e.def) else {
            return;
        };
        let occupants = self.occupants(id);
        let new = self.spawn_owned(into, owner, pos);
        for u in occupants {
            self.entities.get_mut(u).unwrap().inside = Some(new);
        }
        self.entities.despawn(id);
        self.events.push(Event::Transformed { player: owner, from: def, into, at: pos });
    }

    /// Units inside a container sit at its centre: a transport that moved takes them along.
    fn carry_occupants(&mut self) {
        for id in self.entities.ids() {
            let Some(c) = self.entities.get(id).unwrap().inside else { continue };
            if let Some(at) = self.entities.get(c).map(|c| c.pos) {
                self.entities.get_mut(id).unwrap().pos = at;
            }
        }
    }

    /// What `player` pays for `def`, after their general's and perks' modifiers. Both are
    /// fixed for the match, so a refund at the same price is exact.
    pub fn cost_for(&self, player: PlayerId, def: DefId) -> crate::defs::Cost {
        self.rules.cost(self.modifiers(player), def)
    }

    /// Ticks `player` needs to build or train `def`, after their general's and perks' modifiers.
    pub fn build_ticks_for(&self, player: PlayerId, def: DefId) -> u32 {
        self.rules.build_ticks(self.modifiers(player), def)
    }

    /// Cost and build-time changes of `player`'s general, then of each slotted perk.
    fn modifiers(&self, player: PlayerId) -> impl Iterator<Item = crate::defs::Modifier> + '_ {
        let p = self.player(player);
        let general = p.general.into_iter().flat_map(|g| self.rules.general(g).modifiers.iter().copied());
        general.chain(p.perks.iter().flat_map(|&k| self.rules.perk(k).modifiers.iter().copied()))
    }

    /// What a queue item costs `player`: a unit after their general's modifiers, a research
    /// at its listed price.
    pub fn item_cost(&self, player: PlayerId, item: Product) -> i64 {
        match item {
            Product::Unit(d) => self.cost_for(player, d).credits,
            Product::Upgrade(u) => self.rules.upgrade(u).cost.credits,
        }
    }

    /// Full-speed ticks a queue item takes for `player`.
    pub fn item_ticks(&self, player: PlayerId, item: Product) -> u32 {
        match item {
            Product::Unit(d) => self.build_ticks_for(player, d),
            Product::Upgrade(u) => self.rules.upgrade(u).research_ticks.max(1),
        }
    }

    fn afford(&self, player: PlayerId, item: Product) -> Result<(), Reject> {
        if self.player(player).credits < self.item_cost(player, item) {
            return Err(Reject::InsufficientFunds);
        }
        Ok(())
    }

    fn pay(&mut self, player: PlayerId, item: Product, sign: i64) {
        let cost = self.item_cost(player, item);
        let p = &mut self.players[player.0 as usize];
        p.credits -= sign * cost;
        self.stats[player.0 as usize].spent += sign * cost;
    }

    fn apply(&mut self, cmd: &Command) -> Result<(), Reject> {
        let player = cmd.player;
        if self.player(player).defeated {
            return Err(Reject::NotOwner);
        }
        match &cmd.kind {
            CommandKind::Move { units, target } => {
                let units = self.own_units(player, units, |_| true)?;
                let pace = self.group_pace(&units);
                for (u, dest) in self.formation(&units, *target) {
                    let e = self.entities.get_mut(u).unwrap();
                    e.order = Order::Move { target: dest, pace };
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
                if !self.offers_of(player, b.def).contains(def) {
                    return Err(Reject::CannotBuild);
                }
                let (pos, well) = self.check_placement(*def, *pos)?;
                self.afford(player, Product::Unit(*def))?;
                self.pay(player, Product::Unit(*def), 1);
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
                if !p.is_complete() || !self.offers_of(player, p.def).contains(def) {
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
                let item = Product::Unit(*def);
                self.afford(player, item)?;
                self.pay(player, item, 1);
                self.entities.get_mut(*producer).unwrap().queue.push(Production { item, progress_halves: 0 });
                Ok(())
            }
            CommandKind::Research { producer, upgrade } => {
                let p = self.owned(player, *producer)?;
                if !p.is_complete() || !self.rules.def(p.def).researches.contains(upgrade) {
                    return Err(Reject::CannotProduce);
                }
                if p.queue.len() >= MAX_QUEUE {
                    return Err(Reject::QueueFull);
                }
                if self.researched_or_queued(player, *upgrade) {
                    return Err(Reject::AlreadyResearched);
                }
                let item = Product::Upgrade(*upgrade);
                self.afford(player, item)?;
                self.pay(player, item, 1);
                self.entities.get_mut(*producer).unwrap().queue.push(Production { item, progress_halves: 0 });
                Ok(())
            }
            CommandKind::CancelTrain { producer } => {
                self.owned(player, *producer)?;
                let item = self.entities.get_mut(*producer).unwrap().queue.pop().ok_or(Reject::CannotProduce)?;
                self.pay(player, item.item, -1);
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
                self.stats[player.0 as usize].defeated_at = Some(self.tick);
                Ok(())
            }
            CommandKind::Attack { units, target } => {
                if !self.attackable(player, *target) {
                    return Err(Reject::CannotAttack);
                }
                let flying = self.entities.get(*target).is_some_and(|t| self.rules.def(t.def).flies());
                // Units whose weapons can't reach the target's domain keep their orders.
                let units = self
                    .own_units(player, units, |d| {
                        d.weapon().or(d.deploy.map(|dep| dep.weapon)).is_some_and(|w| w.targets.hits(flying))
                    })
                    .map_err(|_| Reject::CannotAttack)?;
                for u in units {
                    let e = self.entities.get_mut(u).unwrap();
                    e.order = Order::Attack { target: *target };
                    e.home = None;
                }
                Ok(())
            }
            CommandKind::AttackMove { units, target } => {
                let units =
                    self.own_units(player, units, |d| d.weapon().is_some()).map_err(|_| Reject::CannotAttack)?;
                let pace = self.group_pace(&units);
                for (u, dest) in self.formation(&units, *target) {
                    let e = self.entities.get_mut(u).unwrap();
                    e.order = Order::AttackMove { dest, pace, engaged: None };
                    e.home = None;
                }
                Ok(())
            }
            CommandKind::Deploy { units, deployed } => {
                let units = self.own_units(player, units, |d| d.deploy.is_some()).map_err(|_| Reject::CannotDeploy)?;
                for u in units {
                    let (state, def) = {
                        let e = self.entities.get(u).unwrap();
                        (e.deploy, e.def)
                    };
                    let ticks = self.rules.def(def).deploy.map_or(0, |d| d.ticks);
                    match (deployed, state) {
                        (true, DeployState::Packed) => {
                            let e = self.entities.get_mut(u).unwrap();
                            e.order = Order::Idle;
                            e.home = None;
                            self.start_deploy(u, ticks);
                        }
                        (false, DeployState::Deployed) => self.pack(u),
                        _ => {}
                    }
                }
                Ok(())
            }
            CommandKind::Power { slot, at } => self.use_power(player, *slot, *at),
            CommandKind::Garrison { units, container } => {
                if !self.enterable(player, *container) {
                    return Err(Reject::CannotGarrison);
                }
                let units = self.own_units(player, units, |d| d.can_garrison()).map_err(|_| Reject::CannotGarrison)?;
                for u in units {
                    let e = self.entities.get_mut(u).unwrap();
                    if e.inside.is_none() {
                        e.order = Order::Garrison { container: *container };
                        e.home = None;
                    }
                }
                Ok(())
            }
            CommandKind::Evacuate { container } => {
                self.owned(player, *container)?;
                if self.occupants(*container).is_empty() {
                    return Err(Reject::CannotGarrison);
                }
                self.evacuate(*container);
                Ok(())
            }
            CommandKind::Transform { units } => {
                let units =
                    self.own_units(player, units, |d| d.becomes.is_some()).map_err(|_| Reject::CannotTransform)?;
                for u in units {
                    self.transform(u);
                }
                Ok(())
            }
            CommandKind::Special { units, target } => {
                if !self.attackable(player, *target) {
                    return Err(Reject::SpecialNotReady);
                }
                let flying = self.entities.get(*target).is_some_and(|t| self.rules.def(t.def).flies());
                let tick = self.tick;
                let ready: Vec<EntityId> = self
                    .own_units(player, units, |d| d.special.is_some_and(|s| s.weapon.targets.hits(flying)))
                    .map_err(|_| Reject::SpecialNotReady)?
                    .into_iter()
                    .filter(|&u| self.entities.get(u).unwrap().special_at <= tick)
                    .collect();
                if ready.is_empty() {
                    return Err(Reject::SpecialNotReady);
                }
                for u in ready {
                    let e = self.entities.get_mut(u).unwrap();
                    e.order = Order::Special { target: *target };
                    e.home = None;
                }
                Ok(())
            }
            CommandKind::SetRally { producer, at } => {
                let p = self.owned(player, *producer)?;
                if self.rules.def(p.def).produces.is_empty() {
                    return Err(Reject::CannotProduce);
                }
                let at = self.map.clamp(*at);
                self.entities.get_mut(*producer).unwrap().rally = Some(at);
                Ok(())
            }
        }
    }

    /// The slowest selected unit's speed: a group moves at one pace and arrives together.
    fn group_pace(&self, units: &[EntityId]) -> Fx {
        units.iter().map(|&u| self.rules.def(self.entities.get(u).unwrap().def).speed()).min().unwrap_or(Fx::ZERO)
    }

    /// One destination per unit around `target`: a square block of slots, spaced by the widest
    /// unit, facing the direction of travel. Slots are dealt out so paths don't cross: a first
    /// guess pairs units and slots by their order front-to-back, then left-to-right, and any
    /// two units whose straight legs still cross swap slots. A swap always shortens the total
    /// of all legs (the triangle inequality), so this ends, and the result has no crossings.
    /// A ground unit's slot inside a building or on blocked terrain moves to the nearest free
    /// cell; otherwise the unit could never arrive, and every attempt to route there would
    /// search the whole reachable map. Aircraft keep theirs: they fly over both.
    fn formation(&self, units: &[EntityId], target: Vec2) -> Vec<(EntityId, Vec2)> {
        let target = self.map.clamp(target);
        let n = units.len() as i64;
        let spacing = units.iter().map(|&u| self.radius_of(u)).max().unwrap_or(SLACK) * Fx::from_int(2) + SLACK;
        let side = ceil_sqrt(n);
        let pos = |u: EntityId| self.entities.get(u).unwrap().pos;
        let centre = units.iter().fold(Vec2::ZERO, |s, &u| s + pos(u));
        let centre = Vec2::new(centre.x / Fx::from_int(n), centre.y / Fx::from_int(n));
        // Travel direction (unit length) and its left-hand normal; +y for a group already there.
        let delta = target - centre;
        let len = delta.length();
        let fwd = if len > Fx::ONE { delta.scale(Fx::ONE / len) } else { Vec2::new(Fx::ZERO, Fx::ONE) };
        let left = Vec2::new(-fwd.y, fwd.x);
        let dot = |a: Vec2, b: Vec2| a.x * b.x + a.y * b.y;
        // Units: front first (largest projection on `fwd`), cut into rows of `side`, each row
        // left to right; slots in the same order.
        let mut order: Vec<EntityId> = units.to_vec();
        order.sort_by_key(|&u| (-dot(pos(u) - centre, fwd), u));
        for row in order.chunks_mut(side as usize) {
            row.sort_by_key(|&u| (-dot(pos(u) - centre, left), u));
        }
        let flies: Vec<bool> =
            order.iter().map(|&u| self.rules.def(self.entities.get(u).unwrap().def).flies()).collect();
        let open: Vec<Vec2> = (0..n)
            .map(|i| {
                let (row, col) = (i / side, i % side);
                let back = spacing * Fx::from_int(row) - spacing * Fx::from_ratio(side - 1, 2);
                let across = spacing * Fx::from_ratio(side - 1, 2) - spacing * Fx::from_int(col);
                self.map.clamp(target - fwd.scale(back) + left.scale(across))
            })
            .collect();
        // Each slot snapped to a cell the group can walk to: the nearest free cell may be a
        // pocket walled in by buildings (measured: 85% of units held up 5 s or more on a move
        // were headed for one), and a unit sent there would push at the wall for good.
        let slots: Vec<Vec2> = open.iter().map(|&p| self.nav.reachable_point(p, centre)).collect();
        let from: Vec<Vec2> = order.iter().map(|&u| pos(u)).collect();
        let mut slot_of: Vec<usize> = (0..order.len()).collect();
        // Bounded for safety: each swap shortens the total, so it converges long before this.
        for _ in 0..order.len() * order.len() {
            let mut swapped = false;
            for i in 0..order.len() {
                for j in i + 1..order.len() {
                    if segments_cross(from[i], slots[slot_of[i]], from[j], slots[slot_of[j]]) {
                        slot_of.swap(i, j);
                        swapped = true;
                    }
                }
            }
            if !swapped {
                break;
            }
        }
        order
            .iter()
            .enumerate()
            .map(|(i, &u)| (u, if flies[i] { open[slot_of[i]] } else { slots[slot_of[i]] }))
            .collect()
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
                    // A support jet flies its one pass and takes no orders.
                    e.owner == Some(player) && d.is_unit() && !d.is_support_jet() && filter(d)
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
            let owner = e.owner.unwrap();
            let need = self.item_ticks(owner, e.queue[0].item) * 2;
            let e = self.entities.get_mut(id).unwrap();
            let step = if e.powered { 2 } else { 1 };
            let front = &mut e.queue[0];
            front.progress_halves += step;
            let item = front.item;
            if front.progress_halves < need {
                continue;
            }
            e.queue.remove(0);
            let (pos, radius, building, rally) = (e.pos, self.rules.def(e.def).radius, e.def, e.rally);
            let def = match item {
                Product::Unit(def) => def,
                Product::Upgrade(upgrade) => {
                    self.finish_research(owner, upgrade);
                    self.events.push(Event::Researched { player: owner, upgrade, at: pos });
                    continue;
                }
            };
            let is_depot = matches!(self.rules.def(building).role(), Some(BuildingRole::SupplyCenter { .. }));
            let ud = self.rules.def(def);
            // On the side facing the map centre, so mirrored starts produce mirrored units.
            let side = self.toward_centre(pos);
            let spawn_at = self.map.clamp(pos + Vec2::new(Fx::ZERO, (radius + ud.radius + SLACK) * side));
            let unit = self.spawn_owned(def, owner, spawn_at);
            self.stats[owner.0 as usize].units_built += 1;
            self.events.push(Event::Produced { player: owner, def, at: spawn_at });
            if is_depot
                && ud.collector().is_some()
                && let Some(pile) = self.nearest_pile(pos)
            {
                let u = self.entities.get_mut(unit).unwrap();
                u.home = Some(id);
                u.order = Order::Gather { pile, phase: GatherPhase::ToPile };
            } else if let Some(rally) = rally {
                // OpenRA's `RallyPoint`: a new unit walks to its building's rally point, to
                // the slot a move order there would give it (off buildings and cliffs).
                let (_, dest) = self.formation(&[unit], rally)[0];
                let u = self.entities.get_mut(unit).unwrap();
                u.order = Order::Move { target: dest, pace: ud.speed() };
            }
        }
    }

    /// Records the upgrade and gives entities the player already has the extra hit points of
    /// any health effect (Zero Hour's `ADD_CURRENT_HEALTH_TOO`: 400/500 becomes 500/600).
    fn finish_research(&mut self, player: PlayerId, upgrade: UpgradeId) {
        let p = &mut self.players[player.0 as usize];
        let at = p.upgrades.binary_search(&upgrade).unwrap_or_else(|i| i);
        p.upgrades.insert(at, upgrade);
        for &(def, effect) in self.rules.upgrade(upgrade).effects {
            let Effect::Health { add } = effect else { continue };
            for id in self.entities.ids() {
                let e = self.entities.get_mut(id).unwrap();
                if e.owner == Some(player) && e.def == def && e.hp > 0 {
                    e.hp += add;
                }
            }
        }
    }

    /// Self-healing, every second, up to the maximum: repair upgrades once an entity hasn't been
    /// hit for the effect's quiet spell, and a rank-2 unit's regeneration while it has no order
    /// (the alpha's `HealthRegenWhenIdle`). Buildings under construction are left alone.
    fn update_repairs(&mut self) {
        if !self.tick.is_multiple_of(TICKS_PER_SECOND) {
            return;
        }
        for id in self.entities.ids() {
            let e = self.entities.get(id).unwrap();
            let (Some(owner), true, true) = (e.owner, e.hp > 0, e.is_complete()) else { continue };
            let calm = |t: u32| e.last_hit.is_none_or(|h| self.tick >= h + t);
            let repair: i32 = self
                .effects(owner, e.def)
                .map(|x| match x {
                    Effect::Repair { hp, calm_ticks } if calm(calm_ticks) => hp,
                    _ => 0,
                })
                .sum();
            let idle = self.rules.def(e.def).veterancy.filter(|_| self.rank(e) >= 2 && e.order == Order::Idle);
            let heal = repair + idle.map_or(0, |v| v.idle_regen);
            let max = self.max_hp(e);
            if heal > 0 && e.hp < max {
                let e = self.entities.get_mut(id).unwrap();
                e.hp = (e.hp + heal).min(max);
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
            let owner = s.owner.unwrap();
            let (def, at) = (s.def, s.pos);
            let done = progress >= self.build_ticks_for(owner, s.def);
            self.entities.get_mut(site).unwrap().construction = (!done).then_some(progress);
            if done {
                self.stats[owner.0 as usize].buildings_built += 1;
                self.events.push(Event::Completed { player: owner, def, at });
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
    /// wells) changed since the last build. Units notice through `Route::key`.
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
    /// A unit walks straight while nothing blocks the straight line (rechecked every tick, so
    /// a unit pushed off its line notices). Otherwise it plans once and follows the waypoints,
    /// keeping the plan while the grid is unchanged and the target stays within `REPLAN` of
    /// where it was planned (a chased unit moves every tick). An unreachable target gets a
    /// partial plan to the closest reachable cell, kept on the same terms.
    fn approach(&mut self, id: EntityId, target: Vec2, contact: Fx, speed: Fx) -> (Vec2, bool) {
        let e = self.entities.get(id).unwrap();
        let pos = e.pos;
        let delta = pos - target;
        let dist = delta.length();
        if dist <= contact + SLACK {
            self.entities.get_mut(id).unwrap().route = Route::default();
            return (pos, true);
        }
        let stop = target + delta.scale(contact / dist);
        // Aircraft fly straight over buildings, cliffs and water.
        if self.rules.def(e.def).flies() {
            let p = pos.step_toward(stop, speed);
            self.entities.get_mut(id).unwrap().route = Route::default();
            return (p, p.distance(target) <= contact + SLACK);
        }
        let planned = e.route.key == self.nav_key && e.route.target.is_some_and(|t| t.distance(target) <= REPLAN);
        if !planned {
            // `find` fails only when the unit stands deep inside blocked cells; it then walks
            // straight, as before pathfinding, until it is out.
            let reach = contact + SLACK + Fx::ONE;
            let found = if self.straight_is_free(pos, stop) {
                None
            } else {
                self.nav.find(pos, stop, |c| c.distance(target) <= reach)
            };
            let route = match found {
                None => Route::default(),
                Some(mut waypoints) => {
                    let partial = waypoints.last().is_none_or(|&w| w.distance(target) > reach);
                    waypoints.reverse();
                    Route { key: self.nav_key, target: Some(target), partial, waypoints }
                }
            };
            self.entities.get_mut(id).unwrap().route = route;
        }
        let radius = self.rules.def(self.entities.get(id).unwrap().def).radius;
        // A turn the unit is already close to counts as taken once the next leg is clear:
        // two units routed through the same turn push each other off it, and neither would
        // ever reach it exactly.
        let route = &self.entities.get(id).unwrap().route.waypoints;
        let skip = route.len() >= 2
            && pos.distance(route[route.len() - 1]) <= radius + SLACK
            && self.straight_is_free(pos, route[route.len() - 2]);
        let u = self.entities.get_mut(id).unwrap();
        if skip {
            u.route.waypoints.pop();
        }
        let mut left = speed;
        let mut p = pos;
        while let Some(&wp) = u.route.waypoints.last() {
            let d = p.distance(wp);
            if d > left {
                p = p.step_toward(wp, left);
                left = Fx::ZERO;
                break;
            }
            p = wp;
            left -= d;
            u.route.waypoints.pop();
        }
        if u.route.waypoints.is_empty() && !u.route.partial {
            // Past the last turn (or never needed one): straight on, and look again next tick.
            u.route = Route::default();
            if left > Fx::ZERO {
                let delta = p - target;
                let dist = delta.length();
                if dist > contact {
                    p = p.step_toward(target + delta.scale(contact / dist), left);
                }
            }
        }
        (p, p.distance(target) <= contact + SLACK)
    }

    /// Whether unit `id`, now at `p`, is within three of its radii of the spot it walks to and
    /// another ground unit already stands on that spot. Its move is then done where it is:
    /// pushed off the spot every tick by the unit on it (often one of a later wave sent to the
    /// same place), it would otherwise keep the order, and the AI treats a unit with an order
    /// as busy, for good (measured: three quarters of the units held up 5 s or more on a move).
    fn slot_taken(&self, id: EntityId, p: Vec2, spot: Vec2) -> bool {
        let d = self.rules.def(self.entities.get(id).unwrap().def);
        if d.flies() || p.distance(spot) > d.radius * Fx::from_int(3) {
            return false;
        }
        self.entities.iter().any(|o| {
            let od = self.rules.def(o.def);
            o.id != id
                && od.is_unit()
                && !od.flies()
                && o.inside.is_none()
                && o.pos.distance(spot) < od.radius + d.radius
        })
    }

    /// Whether the straight segment `a`→`b` crosses only free cells, apart from the cells at
    /// its ends (a unit may stand at, and arrive at, an obstacle's edge).
    fn straight_is_free(&self, a: Vec2, b: Vec2) -> bool {
        let (ca, cb) = (self.nav.cell(a), self.nav.cell(b));
        // Every sampled point lies between `a` and `b` on both axes, so its cell is inside
        // the box of the end cells: a box with nothing blocked needs no sampling.
        if !self.nav.any_blocked_in(ca, cb) {
            return true;
        }
        let steps = (a.distance(b).floor_int() / crate::nav::CELL).max(1);
        (1..steps).all(|k| {
            let c = self.nav.cell(a + (b - a).scale(Fx::from_ratio(k, steps)));
            c == ca || c == cb || !self.nav.blocked(c.0, c.1)
        })
    }

    fn update_unit(&mut self, id: EntityId) {
        let e = self.entities.get(id).unwrap();
        if let Order::Sortie { aim, exit, slot, dropped } = e.order {
            self.fly_sortie(id, aim, exit, slot, dropped);
            return;
        }
        let d = self.rules.def(e.def);
        let (pos, speed, radius, owner, order) = (e.pos, d.speed(), d.radius, e.owner.unwrap(), e.order);
        let deploy = d.deploy.map(|mut dep| {
            dep.weapon = self.unit_weapon(e, dep.weapon);
            dep
        });
        let state = e.deploy;
        if d.weapon().is_some() {
            let e = self.entities.get_mut(id).unwrap();
            e.cooldown = e.cooldown.saturating_sub(1);
        }
        if let Some(container) = self.entities.get(id).unwrap().inside {
            self.fire_from_inside(id, container);
            return;
        }
        // Setting up and packing up take time, during which the unit neither moves nor fires.
        match state {
            DeployState::Deploying(t) | DeployState::Packing(t) if t > 1 => {
                let e = self.entities.get_mut(id).unwrap();
                e.deploy = if matches!(state, DeployState::Deploying(_)) {
                    DeployState::Deploying(t - 1)
                } else {
                    DeployState::Packing(t - 1)
                };
                return;
            }
            DeployState::Deploying(_) => {
                self.entities.get_mut(id).unwrap().deploy = DeployState::Deployed;
                return;
            }
            DeployState::Packing(_) => {
                self.entities.get_mut(id).unwrap().deploy = DeployState::Packed;
                return;
            }
            DeployState::Packed | DeployState::Deployed => {}
        }
        let deployed = state == DeployState::Deployed;
        let e = self.entities.get(id).unwrap();
        let weapon = d.weapon_for(deployed).map(|w| self.unit_weapon(e, w));
        // A deployed unit given anything that moves it packs up first; the order waits. On an
        // attack-move it stays set up while its howitzer has a target, and moves on after.
        let moves = match order {
            Order::Idle | Order::Attack { .. } => false,
            Order::AttackMove { dest, .. } if dest == pos => false,
            Order::AttackMove { .. } => weapon.is_none_or(|w| self.enemy_in_band(owner, pos, w, Fx::ZERO).is_none()),
            _ => true,
        };
        if deployed && moves {
            self.pack(id);
            return;
        }
        let (new_pos, new_order) = match order {
            Order::Idle => {
                // Guard: an idle armed unit engages what comes close and then walks back. A
                // deployable one first sets up if an enemy is within its main gun's reach. A
                // deployed one can't close in, so only an enemy already in its band counts:
                // one just beyond it would flip it to attack-move and straight back every tick.
                if let Some(dep) = deploy.filter(|_| !deployed)
                    && self.enemy_in_band(owner, pos, dep.weapon, ACQUIRE).is_some()
                {
                    self.start_deploy(id, dep.ticks);
                    return;
                }
                let acquire = if deployed { Fx::ZERO } else { ACQUIRE };
                match weapon.and_then(|w| self.enemy_in_band(owner, pos, w, acquire)) {
                    Some(t) => (pos, Order::AttackMove { dest: pos, pace: speed, engaged: Some(t) }),
                    None => (pos, Order::Idle),
                }
            }
            Order::Move { target, pace } => {
                let (p, arrived) = self.approach(id, target, Fx::ZERO, pace.min(speed));
                (p, if arrived || self.slot_taken(id, p, target) { Order::Idle } else { order })
            }
            Order::Construct { site } => match self.entities.get(site).filter(|s| s.construction.is_some()) {
                None => (pos, Order::Idle),
                Some(s) => {
                    let (site_pos, site_r) = (s.pos, self.rules.def(s.def).radius);
                    let (p, _) = self.approach(id, site_pos, site_r + radius, speed);
                    (p, order)
                }
            },
            // The special shot: close to its range, fire once (as `engage` does, with the special
            // weapon and its own recharge), then stop. A target lost (dead, gone into stealth)
            // or a shot already spent drops the order.
            Order::Special { target } => {
                let tick = self.tick;
                let spec = self.rules.def(self.entities.get(id).unwrap().def).special;
                let seen = self
                    .live_enemy(owner, target)
                    .filter(|t| !self.hidden(t) || self.visions[owner.0 as usize].detected.contains(&t.id))
                    .map(|t| (t.pos, self.rules.def(t.def).radius, self.rules.def(t.def).flies()));
                match (spec, seen) {
                    (Some(s), Some((tpos, tr, flying))) if self.entities.get(id).unwrap().special_at <= tick => {
                        let range = s.weapon.range_vs(flying).unwrap_or(Fx::ZERO);
                        if tpos.distance(pos) - tr > range {
                            (self.approach(id, tpos, tr + range - SLACK, speed).0, order)
                        } else {
                            self.fire_special(id, target, s);
                            (pos, Order::Idle)
                        }
                    }
                    _ => (pos, Order::Idle),
                }
            }
            Order::Gather { pile, phase } => self.gather_step(id, pile, phase, pos, speed, radius, owner),
            // A target that slips back into stealth (no detector, no recent shot) is lost; one
            // that only walks into the fog is still chased.
            Order::Attack { target } => match (
                weapon,
                self.live_enemy(owner, target)
                    .filter(|t| !self.hidden(t) || self.visions[owner.0 as usize].detected.contains(&t.id))
                    .map(|t| (t.pos, t.def)),
            ) {
                // A weapon that can't reach the target's domain (a tank at a helicopter) drops
                // the order, as an attack order on it is refused in the first place.
                (Some(w), Some((tpos, tdef))) if w.targets.hits(self.rules.def(tdef).flies()) => {
                    let flying = self.rules.def(tdef).flies();
                    let gap = tpos.distance(pos) - self.rules.def(tdef).radius;
                    let reach = w.range_vs(flying).unwrap_or(Fx::ZERO);
                    match deploy {
                        // Packed, with the target beyond the cannon but within the howitzer's
                        // reach: stop and set up rather than drive closer.
                        Some(dep)
                            if !deployed && gap > reach && dep.weapon.range_vs(flying).is_some_and(|r| gap <= r) =>
                        {
                            self.start_deploy(id, dep.ticks);
                            return;
                        }
                        // Deployed but the target left the howitzer's band: pack up to follow.
                        Some(_) if deployed && !in_band(w, gap, flying) => {
                            self.pack(id);
                            return;
                        }
                        _ => (self.engage(id, target, w, speed), order),
                    }
                }
                _ => (pos, Order::Idle),
            },
            Order::AttackMove { dest, pace, engaged } => match weapon {
                None => (pos, Order::Idle),
                Some(w) => {
                    // Keep the current target while it lives, stays visible and within
                    // acquisition range (a leash: a guard isn't dragged across the map, nor
                    // into the fog); else look around.
                    let target = engaged
                        .filter(|&t| {
                            self.live_enemy(owner, t).is_some_and(|e| {
                                let d = self.rules.def(e.def);
                                self.sees(owner, e)
                                    && w.range_vs(d.flies())
                                        .is_some_and(|r| e.pos.distance(pos) - d.radius <= r + ACQUIRE)
                            })
                        })
                        .or_else(|| self.enemy_in_band(owner, pos, w, ACQUIRE));
                    match target {
                        // A deployed unit can't chase: it fires at what is in its band, else
                        // waits for the guard check (Idle) to pick another target.
                        Some(t) if deployed => {
                            let band = self.entities.get(t).map(|e| {
                                let d = self.rules.def(e.def);
                                (e.pos.distance(pos) - d.radius, d.flies())
                            });
                            if band.is_some_and(|(g, f)| in_band(w, g, f)) {
                                (self.engage(id, t, w, speed), Order::AttackMove { dest, pace, engaged: Some(t) })
                            } else {
                                (pos, Order::Idle)
                            }
                        }
                        Some(t) => (self.engage(id, t, w, speed), Order::AttackMove { dest, pace, engaged: Some(t) }),
                        None if deployed => (pos, Order::Idle),
                        None => {
                            // Nothing for the cannon, but something in the howitzer's band: stop
                            // and set up. The order stays, so it moves on once packed again.
                            if let Some(dep) = deploy
                                && self.enemy_in_band(owner, pos, dep.weapon, Fx::ZERO).is_some()
                            {
                                self.start_deploy(id, dep.ticks);
                                return;
                            }
                            let (p, arrived) = self.approach(id, dest, Fx::ZERO, pace.min(speed));
                            (p, if arrived { Order::Idle } else { Order::AttackMove { dest, pace, engaged: None } })
                        }
                    }
                }
            },
            // Walk to the container's edge and get in, if there is still room; else stop.
            Order::Garrison { container } => match self.entities.get(container) {
                Some(c) if self.enterable(owner, container) => {
                    let (cpos, cr) = (c.pos, self.rules.def(c.def).radius);
                    let (p, arrived) = self.approach(id, cpos, cr + radius, speed);
                    if arrived {
                        self.enter(id, container);
                        return;
                    }
                    (p, order)
                }
                _ => (pos, Order::Idle),
            },
            Order::Sortie { .. } => unreachable!("flown by fly_sortie"),
        };
        let e = self.entities.get_mut(id).unwrap();
        e.pos = self.map.clamp(new_pos);
        // A route belongs to one goal; a new order or gather phase plans its own.
        if new_order != order {
            e.route = Route::default();
        }
        e.order = new_order;
        if new_order == Order::Idle {
            e.home = None;
        }
    }

    /// `unit` gets in: it takes the container's position, and a neutral building becomes its
    /// player's.
    fn enter(&mut self, unit: EntityId, container: EntityId) {
        let (cpos, owner) = (self.entities.get(container).unwrap().pos, self.entities.get(unit).unwrap().owner);
        let e = self.entities.get_mut(unit).unwrap();
        e.inside = Some(container);
        e.pos = cpos;
        e.order = Order::Idle;
        e.home = None;
        e.route = Route::default();
        let c = self.entities.get_mut(container).unwrap();
        if c.owner.is_none() {
            c.owner = owner;
        }
        self.events.push(Event::Garrisoned { player: owner.unwrap(), unit, container, at: cpos });
    }

    /// A unit inside a container that lets its garrison fire shoots out from the container's
    /// edge: at the nearest visible enemy within its weapon's range plus the container's range
    /// bonus, measured from that edge. It never moves.
    fn fire_from_inside(&mut self, id: EntityId, container: EntityId) {
        let Some(c) = self.entities.get(container) else { return };
        let cdef = self.rules.def(c.def);
        let Some(g) = cdef.garrison.filter(|g| g.fires) else { return };
        let e = self.entities.get(id).unwrap();
        let Some(base) = self.rules.def(e.def).weapon() else { return };
        let mut w = self.unit_weapon(e, base);
        // Fired from the edge, reaching the bonus further: centre-based range grows by both.
        let extra = cdef.radius + g.range_bonus;
        w.range += extra;
        w.air_range = w.air_range.map(|r| r + extra);
        let owner = e.owner.unwrap();
        let cpos = c.pos;
        if let Some(t) = self.enemy_in_band(owner, cpos, w, Fx::ZERO) {
            self.engage(id, t, w, Fx::ZERO);
        }
    }

    /// A finished, powered base defence fires at the nearest enemy it sees in its band, from
    /// its edge: range counts from its footprint, as a garrison's does. It never moves.
    fn update_defense(&mut self, id: EntityId) {
        let e = self.entities.get_mut(id).unwrap();
        e.cooldown = e.cooldown.saturating_sub(1);
        let e = self.entities.get(id).unwrap();
        let d = self.rules.def(e.def);
        let (Some(mut w), Some(owner)) = (d.weapon(), e.owner) else { return };
        if !e.is_complete() || !e.powered {
            return;
        }
        w.range += d.radius;
        w.air_range = w.air_range.map(|r| r + d.radius);
        if let Some(t) = self.enemy_in_band(owner, e.pos, w, Fx::ZERO) {
            self.engage(id, t, w, Fx::ZERO);
        }
    }

    fn start_deploy(&mut self, id: EntityId, ticks: u32) {
        let e = self.entities.get_mut(id).unwrap();
        e.deploy = if ticks == 0 { DeployState::Deployed } else { DeployState::Deploying(ticks) };
        e.route = Route::default();
    }

    fn pack(&mut self, id: EntityId) {
        let ticks = self.rules.def(self.entities.get(id).unwrap().def).deploy.map_or(0, |d| d.ticks);
        let e = self.entities.get_mut(id).unwrap();
        e.deploy = if ticks == 0 { DeployState::Packed } else { DeployState::Packing(ticks) };
    }

    /// `target` if it is a live entity owned by an opponent of `owner` that can be shot (units
    /// and buildings, not neutral resources, not units inside something). Fog is not checked:
    /// an attack order, once given, chases its target through fog while it lives. Whether it
    /// may be given is `attackable`.
    fn live_enemy(&self, owner: PlayerId, target: EntityId) -> Option<&Entity> {
        self.entities.get(target).filter(|t| t.owner.is_some_and(|o| o != owner) && t.hp > 0 && t.inside.is_none())
    }

    /// Whether `owner` may order an attack on `target`: a live enemy it sees now (a stealth
    /// unit only while detected or revealed), or an enemy building it remembers.
    fn attackable(&self, owner: PlayerId, target: EntityId) -> bool {
        let vision = &self.visions[owner.0 as usize];
        self.live_enemy(owner, target).is_some_and(|t| self.sees(owner, t) || vision.memory.contains_key(&target))
    }

    /// The visible enemy whose edge is nearest `pos`, in the band weapon `w` can fire at: of a
    /// domain it targets (ground or air), no closer than its shell's minimum range and no
    /// further than its range against that domain plus `extra` (the acquisition margin). Units
    /// come before buildings, so a squad fights the army before the walls; ties go to the lower
    /// entity id.
    fn enemy_in_band(&self, owner: PlayerId, pos: Vec2, w: crate::defs::Weapon, extra: Fx) -> Option<EntityId> {
        let near = w.shell.map_or(Fx::ZERO, |s| s.min_range);
        let mut best: Option<(bool, Fx, EntityId)> = None;
        for e in self.entities.iter() {
            if !e.owner.is_some_and(|o| o != owner) || e.hp <= 0 || e.inside.is_some() {
                continue;
            }
            let d = self.rules.def(e.def);
            let Some(range) = w.range_vs(d.flies()) else { continue };
            let reach = range + extra;
            // Cheap box test first: almost every entity is far away, and `distance` is a sqrt.
            let far = reach + d.radius;
            if (e.pos.x - pos.x).abs() > far || (e.pos.y - pos.y).abs() > far {
                continue;
            }
            let gap = (e.pos.distance(pos) - d.radius).max(Fx::ZERO);
            if gap > reach || gap < near || !self.sees(owner, e) {
                continue;
            }
            let key = (!d.is_unit(), gap, e.id);
            if best.is_none_or(|b| key < b) {
                best = Some(key);
            }
        }
        best.map(|(_, _, id)| id)
    }

    /// Unit `id` fires its special shot `s` at `target`: direct damage against the target's
    /// armour, joining this tick's shots; the shot then recharges, and like any shot it gives
    /// a stealth unit away for `STEALTH_REVEAL_TICKS` (the alpha's `StealthDisableOnFire`) and
    /// ends a cloaking power's cover. The unit's main gun keeps its own cooldown.
    fn fire_special(&mut self, id: EntityId, target: EntityId, s: crate::defs::Special) {
        let t = self.entities.get(target).unwrap();
        let (tpos, armour) = (t.pos, self.rules.def(t.def).armour);
        let tick = self.tick;
        let e = self.entities.get(id).unwrap();
        let (pos, owner, stealthy) = (e.pos, e.owner.unwrap(), self.rules.def(e.def).stealth);
        let e = self.entities.get_mut(id).unwrap();
        e.special_at = tick + s.cooldown_ticks;
        if stealthy {
            e.revealed_until = Some(tick + STEALTH_REVEAL_TICKS);
        }
        e.cloaked_until = None;
        self.shots.push((id, owner, target, s.weapon.damage_vs(armour), false));
        self.events.push(Event::SpecialFired { shooter: id, target, from: pos, at: tpos });
    }

    /// Moves unit `id` into range of `target` and fires when the weapon is ready. Returns the
    /// unit's new position. A deployed unit can't move: out of range it just waits. A shell
    /// weapon holds fire at a target inside its minimum range.
    fn engage(&mut self, id: EntityId, target: EntityId, w: crate::defs::Weapon, speed: Fx) -> Vec2 {
        let t = self.entities.get(target).unwrap();
        let (tpos, tdef) = (t.pos, self.rules.def(t.def));
        let (tr, armour, flying) = (tdef.radius, tdef.armour, tdef.flies());
        let e = self.entities.get(id).unwrap();
        let (pos, ready, owner, deployed) =
            (e.pos, e.cooldown == 0, e.owner.unwrap(), e.deploy == DeployState::Deployed);
        let Some(range) = w.range_vs(flying) else { return pos };
        let gap = pos.distance(tpos) - tr;
        if gap > range {
            if deployed {
                return pos;
            }
            // Close to a point just inside range, so separation can't push it back out.
            return self.approach(id, tpos, tr + range - SLACK, speed).0;
        }
        self.entities.get_mut(id).unwrap().route = Route::default();
        if !ready || !in_band(w, gap, flying) {
            return pos;
        }
        let tick = self.tick;
        let stealthy = self.rules.def(self.entities.get(id).unwrap().def).stealth;
        let e = self.entities.get_mut(id).unwrap();
        e.cooldown = w.cooldown_ticks;
        // Firing gives a stealth unit away for a while, and ends a cloaking power's cover.
        if stealthy {
            e.revealed_until = Some(tick + STEALTH_REVEAL_TICKS);
        }
        e.cloaked_until = None;
        match w.shell {
            None => {
                let damage = w.damage_vs(armour) + self.garrison_bonus_vs(w.garrison_bonus, target);
                self.shots.push((id, owner, target, damage, false));
            }
            Some(s) => {
                let flight = (pos.distance(tpos) / s.speed).floor_int().max(1) as u32;
                let lands = self.tick + flight;
                let source = ShellSource::Gun { def: self.entities.get(id).unwrap().def, deployed };
                self.shells.push(ShellInFlight {
                    shooter: id,
                    owner,
                    from: pos,
                    at: tpos,
                    fired: self.tick,
                    lands,
                    damage: w.damage,
                    bonus: w.bonus,
                    radius: s.radius,
                    garrison_bonus: w.garrison_bonus,
                    source,
                });
                self.events.push(Event::Fired { shooter: id, from: pos, at: tpos, lands, source });
            }
        }
        pos
    }

    /// Shells due this tick land: every live enemy of the shooter's owner whose edge is within
    /// the blast radius of the impact point takes the shell's damage against its armour. The
    /// damage joins this tick's direct shots, so it applies together with them.
    fn land_shells(&mut self) {
        let tick = self.tick;
        // A power's missiles leave their silo on a later tick than the order: shown on launch.
        for s in self.shells.iter().filter(|s| s.fired == tick && s.shooter == POWER_SOURCE && s.from != s.at) {
            let (from, at, lands, source) = (s.from, s.at, s.lands, s.source);
            self.events.push(Event::Fired { shooter: POWER_SOURCE, from, at, lands, source });
        }
        let (due, flying): (Vec<_>, Vec<_>) =
            core::mem::take(&mut self.shells).into_iter().partition(|s| s.lands <= tick);
        self.shells = flying;
        for s in due {
            self.events.push(Event::Landed { at: s.at, radius: s.radius, source: s.source });
            for e in self.entities.iter() {
                // Units inside something take nothing: the building or vehicle shields them.
                if !e.owner.is_some_and(|o| o != s.owner) || e.hp <= 0 || e.inside.is_some() {
                    continue;
                }
                let d = self.rules.def(e.def);
                // Shells burst on the ground; aircraft overhead are untouched.
                if d.flies() {
                    continue;
                }
                if e.pos.distance(s.at) - d.radius > s.radius {
                    continue;
                }
                let damage =
                    (s.damage + s.bonus[d.armour as usize]).max(1) + self.garrison_bonus_vs(s.garrison_bonus, e.id);
                self.shots.push((s.shooter, s.owner, e.id, damage, true));
            }
        }
    }

    /// Every second, each active toxin cloud damages every enemy ground unit and building of
    /// its owner's within its radius (aircraft and units inside something are untouched, as
    /// with shells). Clouds past their end are dropped.
    fn poison_clouds(&mut self) {
        let tick = self.tick;
        self.clouds.retain(|c| c.until > tick);
        if !tick.is_multiple_of(TICKS_PER_SECOND) {
            return;
        }
        for c in self.clouds.iter().filter(|c| c.from <= tick) {
            for e in self.entities.iter() {
                if !e.owner.is_some_and(|o| o != c.owner) || e.hp <= 0 || e.inside.is_some() {
                    continue;
                }
                let d = self.rules.def(e.def);
                if d.flies() || e.pos.distance(c.at) - d.radius > c.radius {
                    continue;
                }
                self.shots.push((POWER_SOURCE, c.owner, e.id, c.damage, true));
            }
        }
    }

    /// `bonus` if `target` has units inside it now, else 0.
    fn garrison_bonus_vs(&self, bonus: i32, target: EntityId) -> i32 {
        if bonus != 0 && self.entities.iter().any(|e| e.inside == Some(target)) { bonus } else { 0 }
    }

    /// `player`'s power in `slot` at `at`. A power aims only at ground the player has explored,
    /// so it can't be used to scout, unless it is `blind`. Barrage shells, airstrike bombs and
    /// missiles fly like artillery shells, with no shooter entity (`POWER_SOURCE`), and are
    /// hashed with every other shell.
    fn use_power(&mut self, player: PlayerId, slot: u8, at: Vec2) -> Result<(), Reject> {
        let power = self.power_in(player, slot).ok_or(Reject::PowerNotReady)?;
        if self.power_wait(player, slot).is_none_or(|t| t > 0) {
            return Err(Reject::PowerNotReady);
        }
        if !self.map.contains(at) || (!power.blind && !self.visions[player.0 as usize].is_explored(at)) {
            return Err(Reject::InvalidTarget);
        }
        match power.kind {
            PowerKind::Barrage { shells, delay_ticks, every_ticks, damage, bonus, radius } => {
                // A fixed pattern, not the RNG: the point first, then around it.
                for k in 0..shells {
                    let spot = if k == 0 { at } else { self.map.clamp(at + around(k - 1, radius * Fx::from_int(2))) };
                    self.shells.push(ShellInFlight {
                        shooter: POWER_SOURCE,
                        owner: player,
                        from: at,
                        at: spot,
                        fired: self.tick,
                        lands: self.tick + delay_ticks + k * every_ticks,
                        damage,
                        bonus,
                        radius,
                        garrison_bonus: 0,
                        source: ShellSource::Power { player, slot },
                    });
                }
            }
            PowerKind::Repair { radius, hp } => {
                for id in self.entities.ids() {
                    let e = self.entities.get(id).unwrap();
                    if e.owner == Some(player) && e.hp > 0 && e.pos.distance(at) <= radius {
                        let max = self.max_hp(e);
                        let e = self.entities.get_mut(id).unwrap();
                        e.hp = (e.hp + hp).min(max);
                    }
                }
            }
            PowerKind::Ambush { def, count } => {
                // Every squad needs open ground (walkable, no water, cliff or bridge deck within
                // its radius): the point itself and each ring spot, else the power is refused.
                let r = self.rules.def(def).radius;
                let spread = r * Fx::from_int(2);
                let spots: Vec<Vec2> =
                    (0..count).map(|k| if k == 0 { at } else { self.map.clamp(at + around(k - 1, spread)) }).collect();
                if spots.iter().any(|&p| !self.map.walkable(p) || self.map.terrain_within(p, r)) {
                    return Err(Reject::InvalidTarget);
                }
                for pos in spots {
                    self.spawn_owned(def, player, pos);
                    self.stats[player.0 as usize].units_built += 1;
                    self.events.push(Event::Produced { player, def, at: pos });
                }
            }
            PowerKind::Airstrike { def, jets, spread, .. } => {
                let (entry, exit) = self.flight_line(player, at);
                let side = perpendicular(exit - entry, spread);
                for k in 0..jets {
                    // Side by side, centred on the line: 0, +1, -1, +2, -2, ... spreads.
                    let n = Fx::from_int((k as i64 + 1) / 2 * if k % 2 == 1 { 1 } else { -1 });
                    let offset = side.scale(n);
                    self.launch(def, player, entry + offset, at + offset, exit + offset, slot);
                }
            }
            PowerKind::Recon { def } => {
                let (entry, exit) = self.flight_line(player, at);
                self.launch(def, player, entry, at, exit, slot);
            }
            PowerKind::Missiles {
                missiles,
                delay_ticks,
                every_ticks,
                speed,
                spread,
                damage,
                radius,
                cloud_radius,
                cloud_damage,
                cloud_ticks,
            } => {
                // From the building that launches them (the first standing one), else the start.
                let site = power.requires.and_then(|d| {
                    self.entities
                        .iter()
                        .find(|e| e.owner == Some(player) && e.def == d && e.is_complete())
                        .map(|e| e.pos)
                });
                let from = site.unwrap_or(self.map.spawns[player.0 as usize]);
                let flight = (from.distance(at) / speed.max(Fx::ONE)).floor_int().max(1) as u32;
                let mut last = self.tick;
                // A fixed pattern, not the RNG: the point first, then around it.
                for k in 0..missiles {
                    let spot = if k == 0 { at } else { self.map.clamp(at + around(k - 1, spread)) };
                    let fired = self.tick + delay_ticks + k * every_ticks;
                    last = fired + flight;
                    self.shells.push(ShellInFlight {
                        shooter: POWER_SOURCE,
                        owner: player,
                        from,
                        at: spot,
                        fired,
                        lands: last,
                        damage,
                        bonus: [0; 4],
                        radius,
                        garrison_bonus: 0,
                        source: ShellSource::Power { player, slot },
                    });
                }
                if cloud_ticks > 0 {
                    self.clouds.push(Cloud {
                        owner: player,
                        at,
                        radius: cloud_radius,
                        damage: cloud_damage,
                        from: last,
                        until: last + cloud_ticks,
                    });
                }
            }
            PowerKind::Cloak { radius, ticks } => {
                let until = self.tick + ticks;
                for id in self.entities.ids() {
                    let e = self.entities.get(id).unwrap();
                    let d = self.rules.def(e.def);
                    if e.owner == Some(player)
                        && d.is_unit()
                        && !d.is_support_jet()
                        && e.hp > 0
                        && e.pos.distance(at) <= radius
                    {
                        self.entities.get_mut(id).unwrap().cloaked_until = Some(until);
                    }
                }
            }
        }
        self.players[player.0 as usize].power_ready[slot as usize] = Some(self.tick + power.cooldown_ticks);
        self.events.push(Event::PowerUsed { player, slot, at });
        Ok(())
    }

    /// Where `player`'s jets enter and leave for a pass over `at`: in from the map edge behind
    /// their start, along the line from the start through `at`, and out the edge beyond. With
    /// `at` on the start itself, they come in from the map's left edge.
    fn flight_line(&self, player: PlayerId, at: Vec2) -> (Vec2, Vec2) {
        let home = self.map.spawns.get(player.0 as usize).copied().unwrap_or(at);
        let delta = at - home;
        let dir =
            if delta.length() < Fx::ONE { Vec2::new(Fx::ONE, Fx::ZERO) } else { delta.scale(Fx::ONE / delta.length()) };
        // Far enough to clear the map from any point on it.
        let far = (self.map.width + self.map.height) * Fx::from_int(2);
        (edge_point(&self.map, at, dir.scale(-Fx::ONE), far), edge_point(&self.map, at, dir, far))
    }

    /// A support jet appears at `entry` on its way to `aim`.
    fn launch(&mut self, def: DefId, player: PlayerId, entry: Vec2, aim: Vec2, exit: Vec2, slot: u8) {
        let (aim, exit) = (self.map.clamp(aim), self.map.clamp(exit));
        let id = self.spawn_owned(def, player, self.map.clamp(entry));
        self.entities.get_mut(id).unwrap().order = Order::Sortie { aim, exit, slot, dropped: false };
    }

    /// A jet's pass: straight at full speed to its aim point, where an airstrike's jet drops
    /// its bombs (a line along its heading, centred on the point, landing a tick apart), then
    /// on to the exit, where it leaves the map without counting as lost.
    fn fly_sortie(&mut self, id: EntityId, aim: Vec2, exit: Vec2, slot: u8, dropped: bool) {
        let e = self.entities.get(id).unwrap();
        let (pos, owner, speed) = (e.pos, e.owner.unwrap(), self.rules.def(e.def).speed());
        if !dropped {
            let next = pos.step_toward(aim, speed);
            self.entities.get_mut(id).unwrap().pos = next;
            if next == aim {
                if let Some(PowerKind::Airstrike { bombs, spacing, damage, bonus, radius, .. }) =
                    self.power_in(owner, slot).map(|p| p.kind)
                {
                    let heading = exit - aim;
                    let step =
                        if heading.length() < Fx::ONE { Vec2::ZERO } else { heading.scale(spacing / heading.length()) };
                    for k in 0..bombs {
                        let along = Fx::from_int(k as i64) - Fx::from_ratio(bombs as i64 - 1, 2);
                        self.shells.push(ShellInFlight {
                            shooter: POWER_SOURCE,
                            owner,
                            from: aim,
                            at: self.map.clamp(aim + step.scale(along)),
                            fired: self.tick,
                            lands: self.tick + 1 + k,
                            damage,
                            bonus,
                            radius,
                            garrison_bonus: 0,
                            source: ShellSource::Power { player: owner, slot },
                        });
                    }
                }
                self.entities.get_mut(id).unwrap().order = Order::Sortie { aim, exit, slot, dropped: true };
            }
            return;
        }
        let next = pos.step_toward(exit, speed);
        let e = self.entities.get_mut(id).unwrap();
        e.pos = next;
        if next == exit {
            // Gone home: removed now, so nothing counts it as a loss.
            self.entities.despawn(id);
        }
    }

    /// Applies this tick's shots together, so every unit alive at the start of the tick fires
    /// and the outcome does not depend on entity order: two units that shoot each other both
    /// land their hit. `killed` is reported once, by the shot that took the target to zero.
    /// The target's toughness upgrades scale each hit (at least 1 damage still lands).
    fn resolve_shots(&mut self) {
        for (shooter, by, target, damage, splash) in core::mem::take(&mut self.shots) {
            let Some(t) = self.entities.get(target) else { continue };
            let toughness: i32 = t.owner.map_or(0, |o| {
                self.effects(o, t.def)
                    .map(|x| match x {
                        Effect::Toughness { percent } => percent - 100,
                        _ => 0,
                    })
                    .sum()
            });
            let damage = if toughness == 0 { damage } else { (damage * (100 + toughness) / 100).max(1) };
            let tick = self.tick;
            let t = self.entities.get_mut(target).unwrap();
            let was_alive = t.hp > 0;
            t.hp -= damage;
            t.last_hit = Some(tick);
            let killed = was_alive && t.hp <= 0;
            let at = t.pos;
            let (victim, def) = (t.owner, t.def);
            if let Some(v) = victim
                && self.entities.get(shooter).is_some_and(|s| self.rules.def(s.def).stealth)
            {
                self.players[v.0 as usize].hit_by_stealth = true;
            }
            self.events.push(Event::Hit { shooter, target, at, damage, killed, splash, target_def: def, by, victim });
            if killed {
                self.stats[by.0 as usize].kills += 1;
                if let Some(v) = victim {
                    let s = &mut self.stats[v.0 as usize];
                    if self.rules.def(def).is_unit() { s.units_lost += 1 } else { s.buildings_lost += 1 }
                }
                self.gain_experience(shooter, self.rules.def(def).experience);
            }
        }
    }

    /// The shooter of a kill (if it still stands: a shell's gun may be gone) gains `xp`, scaled
    /// by its owner's experience effects. A rank reached raises its maximum hit points by the
    /// rank's health and its current ones by the same amount, as Zero Hour keeps the ratio when
    /// a promotion raises the maximum.
    fn gain_experience(&mut self, shooter: EntityId, xp: u32) {
        let Some(e) = self.entities.get(shooter) else { return };
        let Some(v) = self.rules.def(e.def).veterancy else { return };
        let (before, owner, at) = (self.rank(e), e.owner, e.pos);
        let extra: u32 = owner.map_or(0, |o| {
            self.effects(o, e.def)
                .map(|x| match x {
                    Effect::Experience { percent } => percent.saturating_sub(100),
                    _ => 0,
                })
                .sum()
        });
        let xp = xp + xp * extra / 100;
        let e = self.entities.get_mut(shooter).unwrap();
        e.experience += xp;
        let after = rank_for(Some(v), e.experience);
        if after == before {
            return;
        }
        if before == 0 && e.hp > 0 {
            e.hp += v.health;
        }
        if let Some(player) = owner {
            self.events.push(Event::Promoted { player, unit: shooter, rank: after, at });
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
                let home = u.home.filter(|&h| live_depot(self, h).is_some());
                // The nearest supply center with room, so a newer one closer to the pile takes
                // over from the one the collector was trained at.
                let depot = self.depot_for(owner, pos, home);
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
                self.stats[owner.0 as usize].earned += cargo;
                self.events.push(Event::Delivered { player: owner, depot, credits: cargo });
                (pos, Order::Gather { pile, phase: GatherPhase::ToPile })
            }
        }
    }

    /// Pushes overlapping units apart and out of structures. Pairwise in index order.
    ///
    /// Collectors are exempt from unit-unit pushing: they shuttle the same narrow lanes between
    /// a pile and a depot in both directions, and two of them meeting head-on in a gap between
    /// buildings would otherwise block each other for good. Aircraft push only each other and
    /// never against structures: they fly over them.
    fn separate_units(&mut self) {
        let ids = self.entities.ids();
        // Units inside something sit at its centre and take no part.
        let units: Vec<EntityId> = ids
            .iter()
            .copied()
            .filter(|&id| {
                let e = self.entities.get(id).unwrap();
                self.rules.def(e.def).is_unit() && e.inside.is_none()
            })
            .collect();
        let statics: Vec<(Vec2, Fx)> = ids
            .iter()
            .filter_map(|&id| {
                let e = self.entities.get(id).unwrap();
                let d = self.rules.def(e.def);
                (!d.is_unit()).then_some((e.pos, d.radius))
            })
            .collect();
        // Positions and radii copied out so the pair loop touches no entity: pairs still run
        // in index order and each push is seen by the pairs after it.
        let pushed: Vec<usize> = (0..units.len())
            .filter(|&i| self.rules.def(self.entities.get(units[i]).unwrap().def).collector().is_none())
            .collect();
        let mut pos: Vec<Vec2> = pushed.iter().map(|&i| self.entities.get(units[i]).unwrap().pos).collect();
        let rad: Vec<Fx> =
            pushed.iter().map(|&i| self.rules.def(self.entities.get(units[i]).unwrap().def).radius).collect();
        let air: Vec<bool> =
            pushed.iter().map(|&i| self.rules.def(self.entities.get(units[i]).unwrap().def).flies()).collect();
        for i in 0..pos.len() {
            for j in i + 1..pos.len() {
                if air[i] != air[j] {
                    continue;
                }
                let min = rad[i] + rad[j];
                let delta = pos[j] - pos[i];
                // Box test before the sqrt in `length`: nearly every pair is far apart.
                if delta.x.abs() >= min || delta.y.abs() >= min {
                    continue;
                }
                let dist = delta.length();
                if dist >= min {
                    continue;
                }
                let dir = if dist == Fx::ZERO { Vec2::new(Fx::ONE, Fx::ZERO) } else { delta.scale(Fx::ONE / dist) };
                let push = dir.scale((min - dist) / Fx::from_int(2));
                pos[i] = self.map.clamp(pos[i] - push);
                pos[j] = self.map.clamp(pos[j] + push);
            }
        }
        for (k, &i) in pushed.iter().enumerate() {
            self.entities.get_mut(units[i]).unwrap().pos = pos[k];
        }
        for &u in &units {
            let e = self.entities.get(u).unwrap();
            if self.rules.def(e.def).flies() {
                continue;
            }
            let r = self.rules.def(e.def).radius;
            let mut pos = e.pos;
            for &(spos, srad) in &statics {
                let delta = pos - spos;
                let reach = srad + r;
                if delta.x.abs() >= reach || delta.y.abs() >= reach {
                    continue;
                }
                let dist = delta.length();
                if dist < reach {
                    let dir = if dist == Fx::ZERO { Vec2::new(Fx::ONE, Fx::ZERO) } else { delta.scale(Fx::ONE / dist) };
                    pos = spos + dir.scale(srad + r);
                }
            }
            self.entities.get_mut(u).unwrap().pos = self.map.clamp(pos);
        }
    }

    /// Derricks pay credits from their well every second until it runs dry. The 2013 patch
    /// made oil derricks "provide additional funds" instead of a second resource.
    fn update_derricks(&mut self) {
        for id in self.entities.ids() {
            let e = self.entities.get(id).unwrap();
            let Some(BuildingRole::OilDerrick { credits_per_second }) = self.rules.def(e.def).role() else { continue };
            let (Some(well), Some(owner), true) = (e.on_well, e.owner, e.is_complete()) else { continue };
            let e = self.entities.get_mut(id).unwrap();
            e.income_ticks += 1;
            if e.income_ticks < TICKS_PER_SECOND {
                continue;
            }
            e.income_ticks = 0;
            let Some(w) = self.entities.get_mut(well) else { continue };
            let amount = credits_per_second.min(w.resource);
            w.resource -= amount;
            self.players[owner.0 as usize].credits += amount;
            self.stats[owner.0 as usize].earned += amount;
        }
    }

    fn cleanup(&mut self) {
        // A unit that turns into something (the Battle Bus) does so when destroyed, its
        // occupants inside the new entity (the alpha's passive `GLA_BattleBusBunker`).
        for id in self.entities.ids() {
            let e = self.entities.get(id).unwrap();
            if e.hp <= 0
                && self.rules.def(e.def).becomes.is_some()
                && e.owner.is_some_and(|o| !self.players[o.0 as usize].defeated)
            {
                self.transform(id);
            }
        }
        // Units inside a destroyed building or vehicle die with it (the alpha's
        // `DieWithGarrison`).
        for id in self.entities.ids() {
            let e = self.entities.get(id).unwrap();
            let Some(c) = e.inside else { continue };
            if self.entities.get(c).is_none_or(|c| c.hp <= 0) {
                let owner = e.owner;
                self.entities.get_mut(id).unwrap().hp = 0;
                if let Some(o) = owner {
                    self.stats[o.0 as usize].units_lost += 1;
                }
            }
        }
        for id in self.entities.ids() {
            let e = self.entities.get(id).unwrap();
            let d = self.rules.def(e.def);
            let depleted = matches!(d.class, Class::SupplyPile) && e.resource <= 0;
            let owner_out = e.owner.is_some_and(|o| self.players[o.0 as usize].defeated);
            // A civilian building outlives whoever held it: it is neutral again.
            if owner_out && e.hp > 0 && d.role() == Some(BuildingRole::Civilian) {
                self.entities.get_mut(id).unwrap().owner = None;
                continue;
            }
            if depleted || e.hp <= 0 || owner_out {
                self.entities.despawn(id);
            }
        }
    }

    fn update_defeat(&mut self) {
        // A mission's sides leave through its result (`update_mission`), not by losing
        // their base: a mission may start a side without one.
        if self.mission.is_some() {
            return;
        }
        for (i, p) in self.players.iter_mut().enumerate() {
            if p.defeated {
                continue;
            }
            let owner = Some(PlayerId(i as u8));
            let onslaught = self.ai[i].as_ref().is_some_and(|a| a.level() == AiLevel::Onslaught);
            // Entities killed this tick (hp ≤ 0) are swept by `cleanup` right after. A civilian
            // building held, or a bunker a unit turned into, is no base: neither keeps a player
            // in the game.
            let alive = self.entities.iter().any(|e| {
                let d = self.rules.def(e.def);
                e.owner == owner && e.hp > 0 && (d.is_base() || d.is_constructor())
            });
            // The onslaught has no base to lose: it is beaten once its last wave is out and gone.
            p.defeated = if onslaught {
                crate::onslaught::next_wave(self.tick).is_none()
                    && !self.entities.iter().any(|e| e.owner == owner && e.hp > 0 && self.rules.def(e.def).is_unit())
            } else {
                !alive
            };
            if p.defeated {
                self.stats[i].defeated_at = Some(self.tick);
            }
        }
    }

    /// An `Onslaught` player's wave for this tick, if one is due: units of its faction, cycling
    /// through what its barracks and war factory train, placed on open ground in front of its
    /// start (toward the map centre). They arrive idle; its AI sends them on next think.
    fn spawn_onslaught_waves(&mut self) {
        let Some(k) = crate::onslaught::wave_at(self.tick) else { return };
        for i in 0..self.players.len() {
            let level = self.ai_level(PlayerId(i as u8));
            if level != Some(AiLevel::Onslaught) || self.players[i].defeated {
                continue;
            }
            let owner = PlayerId(i as u8);
            let faction = self.players[i].faction;
            // Ground units only: the waves walk in, so the schedule's tuning holds.
            let roster: Vec<DefId> = self
                .rules
                .defs
                .iter()
                .filter(|d| d.faction == Some(faction) && d.role() == Some(BuildingRole::Factory))
                .flat_map(|d| d.produces.iter().copied())
                .filter(|&u| !self.rules.def(u).flies())
                .collect();
            let spawn = self.map.spawns[i];
            let front = spawn + Vec2::new(Fx::ZERO, Fx::from_int(30) * self.toward_centre(spawn));
            for n in 0..crate::onslaught::wave_size(k) {
                let def = roster[(n as usize + k as usize) % roster.len()];
                let at = if n == 0 { front } else { front + around(n - 1, Fx::from_int(6)) };
                let pos = self.nav.free_point(self.map.clamp(at));
                let unit = self.spawn_owned(def, owner, pos);
                let xp = crate::onslaught::wave_experience(k);
                let e = self.entities.get_mut(unit).unwrap();
                e.experience = xp;
                // Arriving ranked: the rank's extra hit points come with it, full.
                let max = self.max_hp(self.entities.get(unit).unwrap());
                self.entities.get_mut(unit).unwrap().hp = max;
                self.stats[i].units_built += 1;
                self.events.push(Event::Produced { player: owner, def, at: pos });
            }
        }
    }
}

/// The rank `experience` earns under `v`: how many of its three thresholds it has reached.
fn rank_for(v: Option<crate::defs::Veterancy>, experience: u32) -> u8 {
    v.map_or(0, |v| v.experience.iter().filter(|&&need| experience >= need).count() as u8)
}

/// Whether a target whose edge is `gap` away is in weapon `w`'s firing band: of a domain it
/// targets, within its range against that domain, and for a shell not inside its minimum range.
fn in_band(w: crate::defs::Weapon, gap: Fx, flying: bool) -> bool {
    w.range_vs(flying).is_some_and(|r| gap <= r) && w.shell.is_none_or(|s| gap >= s.min_range)
}

/// The `k`-th point of a fixed ring pattern around a centre, `r` away along one of eight
/// directions (diagonals at 7/10 of `r` on each axis, near enough to a circle); later laps are
/// farther out. Exact in fixed point, so every machine places the same points.
pub(crate) fn around(k: u32, r: Fx) -> Vec2 {
    const DIRS: [(i64, i64); 8] = [(10, 0), (7, 7), (0, 10), (-7, 7), (-10, 0), (-7, -7), (0, -10), (7, -7)];
    let (x, y) = DIRS[k as usize % 8];
    let lap = Fx::from_int(1 + k as i64 / 8);
    Vec2::new(r * lap * Fx::from_ratio(x, 10), r * lap * Fx::from_ratio(y, 10))
}

/// Where the ray from `from` along unit vector `dir` leaves `map`: `from` moved along `dir`
/// until it first reaches the map edge (at most `far`). Fixed point throughout; the axis it
/// leaves by is set exactly to the edge, so rounding never stops it a hair short.
fn edge_point(map: &crate::map::MapDef, from: Vec2, dir: Vec2, far: Fx) -> Vec2 {
    // Distance to the edge along each axis the ray moves on, and that edge; the nearer one is
    // where it exits.
    let reach = |p: Fx, d: Fx, size: Fx| {
        if d > Fx::ZERO {
            ((size - p) / d, size)
        } else if d < Fx::ZERO {
            (p / -d, Fx::ZERO)
        } else {
            (far, p)
        }
    };
    let ((tx, ex), (ty, ey)) = (reach(from.x, dir.x, map.width), reach(from.y, dir.y, map.height));
    let p = map.clamp(from + dir.scale(tx.min(ty).min(far)));
    if tx.min(ty) > far {
        p
    } else if tx <= ty {
        Vec2::new(ex, p.y)
    } else {
        Vec2::new(p.x, ey)
    }
}

/// `v` turned a quarter left and scaled to length `len`; zero for a zero `v`.
fn perpendicular(v: Vec2, len: Fx) -> Vec2 {
    let l = v.length();
    if l == Fx::ZERO {
        return Vec2::ZERO;
    }
    Vec2::new(-v.y, v.x).scale(len / l)
}

/// Whether segments `a`–`b` and `c`–`d` properly cross (touching ends and collinear overlap
/// don't count). Exact: the products are compared by sign in i128.
fn segments_cross(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let side = |p: Vec2, q: Vec2, r: Vec2| {
        let cross =
            (q.x.0 - p.x.0) as i128 * (r.y.0 - p.y.0) as i128 - (q.y.0 - p.y.0) as i128 * (r.x.0 - p.x.0) as i128;
        cross.signum()
    };
    side(a, b, c) * side(a, b, d) < 0 && side(c, d, a) * side(c, d, b) < 0
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

    /// The civilian building def for a map building of footprint `radius`: of those whose
    /// radius is at most `radius` (so it never blocks ground the map left open), the largest;
    /// `None` if every civilian def is larger. Ties go to the earlier def.
    pub fn civilian_for(&self, radius: Fx) -> Option<DefId> {
        self.defs
            .iter()
            .enumerate()
            .filter(|(_, d)| d.role() == Some(BuildingRole::Civilian) && d.radius <= radius)
            .max_by_key(|&(i, d)| (d.radius, std::cmp::Reverse(i)))
            .map(|(i, _)| DefId(i as u16))
    }
}
