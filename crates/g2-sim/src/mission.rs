//! Missions: scripted matches. A mission places every side's starting forces itself (no
//! command center and constructor at each start, as a skirmish gives), lists objectives for
//! the player (slot 0, `PLAYER`) and runs triggers that spawn reinforcements and enemy waves,
//! hand out credits and say things. The player wins once every objective is done and loses
//! when an objective fails (something to protect is destroyed) or everything they have is.
//!
//! Missions are content (`Rules::missions`, written in g2-rules); a match plays one when it
//! starts from `World::with_mission`, and the replay keeps its id. Objectives and triggers
//! are checked once a tick, after combat, so what died this tick counts; their state is part
//! of the state hash, only in a mission, so other matches hash as before.

use crate::defs::{BuildingRole, DefId, Faction, GeneralId, Rules};
use crate::entity::{EntityId, Order, PlayerId};
use crate::fixed::{Fx, Vec2};
use crate::map::{AiLevel, MapDef, MatchSetup, PlayerSetup};
use crate::world::{Event, World};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct MissionId(pub u16);

/// The player's seat in every mission; every other seat is an enemy.
pub const PLAYER: PlayerId = PlayerId(0);

#[derive(Debug)]
pub struct Mission {
    /// Stable name the player's progress is saved under.
    pub key: &'static str,
    pub name: &'static str,
    /// The map, as `g2_maps::resolve` takes it.
    pub map: &'static str,
    /// Read before the mission, on the briefing screen.
    pub briefing: &'static str,
    /// The seats, the player's first.
    pub sides: &'static [Side],
    /// Everything on the map at the start, spawned in this order.
    pub forces: &'static [Force],
    pub objectives: &'static [Objective],
    pub triggers: &'static [Trigger],
}

/// One seat. `ai` plays an enemy seat; the player's seat is the human's, or the AI's at the
/// level `Mission::setup` is given.
#[derive(Clone, Copy, Debug)]
pub struct Side {
    pub faction: Faction,
    pub credits: i64,
    pub ai: AiLevel,
    pub general: Option<GeneralId>,
}

/// `count` entities of `def` for seat `owner`, the first at `at` (whole world units) and the
/// rest in rings around it. A tag names the first one for objectives and triggers.
#[derive(Clone, Copy, Debug)]
pub struct Force {
    pub owner: u8,
    pub def: DefId,
    pub at: (i64, i64),
    pub count: u8,
    pub tag: Option<u8>,
}

impl Force {
    pub const fn new(owner: u8, def: DefId, x: i64, y: i64) -> Force {
        Force { owner, def, at: (x, y), count: 1, tag: None }
    }

    pub const fn times(self, count: u8) -> Force {
        Force { count, ..self }
    }

    pub const fn tagged(self, tag: u8) -> Force {
        Force { tag: Some(tag), ..self }
    }

    /// Where its entities stand before the map clamps them: the first at `at`, the rest in
    /// rings around it, two world units apart at their edges.
    pub fn points(&self, rules: &Rules) -> impl Iterator<Item = Vec2> {
        let gap = rules.def(self.def).radius * Fx::from_int(2) + Fx::from_int(2);
        let at = Vec2::from_int(self.at.0, self.at.1);
        (0..self.count as u32).map(move |n| if n == 0 { at } else { at + crate::world::around(n - 1, gap) })
    }
}

/// A circle on the map, in whole world units.
#[derive(Clone, Copy, Debug)]
pub struct Area {
    pub at: (i64, i64),
    pub radius: i64,
}

impl Area {
    pub fn centre(self) -> Vec2 {
        Vec2::from_int(self.at.0, self.at.1)
    }

    pub fn contains(self, p: Vec2) -> bool {
        p.distance(self.centre()) <= Fx::from_int(self.radius)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Objective {
    pub text: &'static str,
    pub goal: Goal,
}

#[derive(Clone, Copy, Debug)]
pub enum Goal {
    /// Seat `owner` has nothing of `what` left.
    DestroyAll { owner: u8, what: What },
    /// The entity tagged `tag` is destroyed.
    Destroy { tag: u8 },
    /// The match reaches `ticks`.
    Survive { ticks: u32 },
    /// The entity tagged `tag` must live: its loss fails the mission. Done with the rest.
    Protect { tag: u8 },
    /// A unit of the player's (the one tagged, if `tag` is set) stands in `area`.
    Reach { area: Area, tag: Option<u8> },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum What {
    /// Base buildings: not a civilian building held, nor a bunker a unit turned into.
    Buildings,
    Units,
    Everything,
}

/// Fires once, the first tick `when` holds, and carries out `then` in order.
#[derive(Clone, Copy, Debug)]
pub struct Trigger {
    pub when: When,
    pub then: &'static [Action],
}

#[derive(Clone, Copy, Debug)]
pub enum When {
    /// On this tick.
    At(u32),
    /// A unit of seat `player` stands in `area`.
    Entered { player: u8, area: Area },
    /// The entity tagged so is destroyed.
    Destroyed(u8),
    /// Objective `n` is done.
    Done(u8),
}

#[derive(Clone, Copy, Debug)]
pub enum Action {
    /// Spawns `forces` on open ground; with `attack`, their armed units attack-move there
    /// together, at the slowest one's pace.
    Spawn {
        forces: &'static [Force],
        attack: Option<(i64, i64)>,
    },
    /// A message on the player's screen.
    Say(&'static str),
    Credits {
        player: u8,
        amount: i64,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Active,
    Done,
    Failed,
}

/// A mission in play. Everything but `said` is part of the state hash.
#[derive(Clone, Debug)]
pub struct Run {
    pub id: MissionId,
    pub def: &'static Mission,
    /// Per objective, in the mission's order.
    pub objectives: Vec<Status>,
    /// Per trigger: it has fired.
    pub fired: Vec<bool>,
    /// Tagged entities, in spawn order.
    pub tags: Vec<(u8, EntityId)>,
    /// `Some(true)` won, `Some(false)` lost.
    pub result: Option<bool>,
    /// Every message said, with its tick. Output only: nothing reads it back.
    pub said: Vec<(u32, &'static str)>,
}

impl Run {
    /// The entity tagged `tag`, while it lives.
    pub fn tagged(&self, tag: u8) -> Option<EntityId> {
        self.tags.iter().find(|&&(t, _)| t == tag).map(|&(_, e)| e)
    }
}

impl Mission {
    /// The match this mission is played as: its seats, the player's played by `player` (a
    /// human for `None`), the enemies by their own AI levels with what an AI general slots.
    pub fn setup(&self, rules: &Rules, map: MapDef, seed: u64, player: Option<AiLevel>) -> MatchSetup {
        let players = self
            .sides
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let ai = if i == PLAYER.0 as usize { player } else { Some(s.ai) };
                PlayerSetup {
                    faction: s.faction,
                    starting_credits: s.credits,
                    ai,
                    general: s.general,
                    perks: ai.map_or_else(Vec::new, |level| crate::ai::ai_perks(rules, s.general, level)),
                    rank: ai.map_or(1, crate::ai::ai_rank),
                }
            })
            .collect();
        MatchSetup { map, seed, players }
    }

    /// Whether `setup` seats this mission's sides: one player per side, of its faction.
    pub fn seats(&self, setup: &[PlayerSetup]) -> bool {
        setup.len() == self.sides.len() && setup.iter().zip(self.sides).all(|(p, s)| p.faction == s.faction)
    }
}

impl World {
    /// The mission in play, if any.
    pub fn mission(&self) -> Option<&Run> {
        self.mission.as_ref()
    }

    /// Spawns the mission's starting forces; called once, from `with_mission`.
    pub(crate) fn start_mission(&mut self, id: MissionId) {
        let def = &self.rules.missions[id.0 as usize];
        let mut tags = Vec::new();
        for f in def.forces {
            self.spawn_force(f, false, &mut tags);
        }
        self.mission = Some(Run {
            id,
            def,
            objectives: vec![Status::Active; def.objectives.len()],
            fired: vec![false; def.triggers.len()],
            tags,
            result: None,
            said: Vec::new(),
        });
    }

    /// Spawns `f`, returning its entities and recording its tag in `tags`. `live` (from a
    /// trigger) places units on free ground and announces them as produced; the starting
    /// forces go exactly where the mission puts them, before any grid exists.
    fn spawn_force(&mut self, f: &Force, live: bool, tags: &mut Vec<(u8, EntityId)>) -> Vec<EntityId> {
        let owner = PlayerId(f.owner);
        let d = self.rules.def(f.def);
        let mut out = Vec::with_capacity(f.count as usize);
        for at in f.points(self.rules) {
            let at = self.map.clamp(at);
            let at = if live && d.is_unit() { self.nav.free_point(at) } else { at };
            let id = self.spawn_owned(f.def, owner, at);
            if live && d.is_unit() {
                self.events.push(Event::Produced { player: owner, def: f.def, at });
            }
            out.push(id);
        }
        if let (Some(tag), Some(&first)) = (f.tag, out.first()) {
            tags.push((tag, first));
        }
        out
    }

    /// Checks objectives, then fires triggers, then settles the result. Runs after combat
    /// and before defeat, so what was destroyed this tick (hp ≤ 0) counts as gone.
    pub(crate) fn update_mission(&mut self) {
        let Some(mut run) = self.mission.take() else { return };
        if run.result.is_none() {
            self.check_objectives(&mut run);
            for (i, t) in run.def.triggers.iter().enumerate() {
                if !run.fired[i] && self.holds(&run, t.when) {
                    run.fired[i] = true;
                    for a in t.then {
                        self.act(&mut run, *a);
                    }
                }
            }
            // The player is out when they surrender or nothing of theirs is left standing.
            let beaten = self.players[PLAYER.0 as usize].defeated
                || !self.entities.iter().any(|e| {
                    let d = self.rules.def(e.def);
                    e.owner == Some(PLAYER) && e.hp > 0 && (d.is_base() || (d.is_unit() && !d.is_support_jet()))
                });
            let failed = beaten || run.objectives.contains(&Status::Failed);
            let all_done = run
                .def
                .objectives
                .iter()
                .zip(&run.objectives)
                .all(|(o, s)| *s == Status::Done || matches!(o.goal, Goal::Protect { .. }));
            if failed {
                run.result = Some(false);
            } else if all_done && !run.objectives.is_empty() {
                run.result = Some(true);
                for (o, s) in run.def.objectives.iter().zip(run.objectives.iter_mut()) {
                    if matches!(o.goal, Goal::Protect { .. }) {
                        *s = Status::Done;
                    }
                }
            }
            // The losing side is out, as a surrender: its assets go in this tick's cleanup.
            if let Some(won) = run.result {
                for i in 0..self.players.len() {
                    let out = if won { i != PLAYER.0 as usize } else { i == PLAYER.0 as usize };
                    if out && !self.players[i].defeated {
                        self.players[i].defeated = true;
                        self.stats[i].defeated_at = Some(self.tick);
                    }
                }
            }
        }
        self.mission = Some(run);
    }

    fn alive(&self, run: &Run, tag: u8) -> bool {
        run.tagged(tag).and_then(|id| self.entities.get(id)).is_some_and(|e| e.hp > 0)
    }

    /// Whether a live unit of `player` (or the entity tagged `tag`) stands in `area`.
    fn in_area(&self, run: &Run, player: PlayerId, area: Area, tag: Option<u8>) -> bool {
        match tag {
            Some(t) => {
                run.tagged(t).and_then(|id| self.entities.get(id)).is_some_and(|e| e.hp > 0 && area.contains(e.pos))
            }
            None => self.entities.iter().any(|e| {
                e.owner == Some(player) && e.hp > 0 && self.rules.def(e.def).is_unit() && area.contains(e.pos)
            }),
        }
    }

    fn check_objectives(&self, run: &mut Run) {
        for (i, o) in run.def.objectives.iter().enumerate() {
            if run.objectives[i] != Status::Active {
                continue;
            }
            let now = match o.goal {
                Goal::DestroyAll { owner, what } => {
                    let left = self.entities.iter().any(|e| {
                        let d = self.rules.def(e.def);
                        let unit = d.is_unit() && !d.is_support_jet();
                        let counts = match what {
                            What::Buildings => d.is_base(),
                            What::Units => unit,
                            What::Everything => d.is_base() || unit,
                        };
                        e.owner == Some(PlayerId(owner)) && e.hp > 0 && counts
                    });
                    (!left).then_some(Status::Done)
                }
                Goal::Destroy { tag } => (!self.alive(run, tag)).then_some(Status::Done),
                Goal::Survive { ticks } => (self.tick >= ticks).then_some(Status::Done),
                Goal::Protect { tag } => (!self.alive(run, tag)).then_some(Status::Failed),
                Goal::Reach { area, tag } => self.in_area(run, PLAYER, area, tag).then_some(Status::Done),
            };
            if let Some(s) = now {
                run.objectives[i] = s;
            }
        }
    }

    fn holds(&self, run: &Run, when: When) -> bool {
        match when {
            When::At(t) => self.tick >= t,
            When::Entered { player, area } => self.in_area(run, PlayerId(player), area, None),
            When::Destroyed(tag) => !self.alive(run, tag),
            When::Done(n) => run.objectives.get(n as usize) == Some(&Status::Done),
        }
    }

    fn act(&mut self, run: &mut Run, a: Action) {
        match a {
            Action::Say(text) => run.said.push((self.tick, text)),
            Action::Credits { player, amount } => self.players[player as usize].credits += amount,
            Action::Spawn { forces, attack } => {
                let ids: Vec<EntityId> = forces.iter().flat_map(|f| self.spawn_force(f, true, &mut run.tags)).collect();
                let Some(to) = attack else { return };
                let armed: Vec<EntityId> = ids
                    .into_iter()
                    .filter(|&id| {
                        let d = self.rules.def(self.entities.get(id).unwrap().def);
                        d.is_unit() && d.weapon().is_some()
                    })
                    .collect();
                let pace = armed
                    .iter()
                    .map(|&id| self.rules.def(self.entities.get(id).unwrap().def).speed())
                    .min()
                    .unwrap_or(Fx::ZERO);
                let dest = self.nav.free_point(self.map.clamp(Vec2::from_int(to.0, to.1)));
                for id in armed {
                    self.entities.get_mut(id).unwrap().order = Order::AttackMove { dest, pace, engaged: None };
                }
            }
        }
    }
}

/// Checks a mission's content against the rules and its map: seats exist, every force
/// stands on walkable ground inside the map, buildings don't overlap each other or terrain,
/// and every tag and objective a goal or trigger names exists. Tests run it on every
/// shipped mission.
pub fn validate(rules: &Rules, m: &Mission, map: &MapDef) -> Result<(), String> {
    let seats = m.sides.len();
    if seats < 2 || seats > map.spawns.len().max(2) {
        return Err(format!("{}: {seats} sides", m.name));
    }
    let all_forces = m.forces.iter().chain(m.triggers.iter().flat_map(|t| t.then).flat_map(|a| match a {
        Action::Spawn { forces, .. } => *forces,
        _ => &[],
    }));
    // What stands on the map from the start: supply piles, oil wells, civilian buildings.
    let statics: Vec<(Vec2, Fx)> = map
        .supply_piles
        .iter()
        .map(|&(p, _)| (p, rules.def(rules.supply_pile()).radius))
        .chain(map.oil_wells.iter().map(|&(p, _)| (p, rules.def(rules.oil_well()).radius)))
        .chain(map.civilians.iter().copied())
        .collect();
    let clear = |p: Vec2, r: Fx, of: &[(Vec2, Fx)]| of.iter().all(|&(q, qr)| q.distance(p) >= qr + r);
    // Buildings first, so units are checked against every one of them.
    let mut buildings: Vec<(Vec2, Fx)> = Vec::new();
    for f in all_forces.clone().filter(|f| rules.def(f.def).is_building()) {
        let d = rules.def(f.def);
        let at = Vec2::from_int(f.at.0, f.at.1);
        let r = d.radius;
        let inside = at.x - r >= Fx::ZERO && at.y - r >= Fx::ZERO && at.x + r <= map.width && at.y + r <= map.height;
        let room = clear(at, r, &buildings) && clear(at, r, &statics);
        if !inside || !room || map.terrain_within(at, r) || f.count != 1 {
            return Err(format!("{}: {} at {:?} can't stand there", m.name, d.name, f.at));
        }
        if matches!(d.role(), Some(BuildingRole::OilDerrick { .. })) {
            return Err(format!("{}: derricks need a well; leave them to the players", m.name));
        }
        buildings.push((at, r));
    }
    // A unit starts clear of every building's footprint by the pathfinder's clearance, so it
    // never starts on a blocked cell.
    let blocked: Vec<(Vec2, Fx)> =
        buildings.iter().chain(&statics).map(|&(p, r)| (p, r + Fx::from_int(crate::nav::CLEARANCE))).collect();
    let mut tags = Vec::new();
    for f in all_forces {
        let d = rules.def(f.def);
        if f.owner as usize >= seats || f.count == 0 {
            return Err(format!("{}: force {:?}", m.name, f));
        }
        if d.faction.is_some_and(|fa| fa != m.sides[f.owner as usize].faction) {
            return Err(format!("{}: {} for a {:?} seat", m.name, d.name, m.sides[f.owner as usize].faction));
        }
        if d.is_unit() {
            for p in f.points(rules) {
                if !map.contains(p) || !map.walkable(p) || !clear(p, d.radius, &blocked) {
                    return Err(format!("{}: a {} of {:?} lands off open ground at {p:?}", m.name, d.name, f.at));
                }
            }
        }
        tags.extend(f.tag);
    }
    let tag_ok = |t: u8| tags.contains(&t);
    for o in m.objectives {
        let ok = match o.goal {
            Goal::Destroy { tag } | Goal::Protect { tag } => tag_ok(tag),
            Goal::Reach { tag, area } => tag.is_none_or(tag_ok) && map.walkable(area.centre()),
            Goal::DestroyAll { owner, .. } => (owner as usize) < seats && owner != PLAYER.0,
            Goal::Survive { .. } => true,
        };
        if !ok {
            return Err(format!("{}: objective {:?}", m.name, o.text));
        }
    }
    for t in m.triggers {
        let ok = match t.when {
            When::Destroyed(tag) => tag_ok(tag),
            When::Done(n) => (n as usize) < m.objectives.len(),
            When::Entered { player, .. } => (player as usize) < seats,
            When::At(_) => true,
        };
        if !ok {
            return Err(format!("{}: trigger {:?}", m.name, t.when));
        }
    }
    if m.objectives.iter().all(|o| matches!(o.goal, Goal::Protect { .. })) {
        return Err(format!("{}: nothing to do but protect: it can't be won", m.name));
    }
    Ok(())
}
