//! Missions: scripted matches. A mission places every side's starting forces itself (no
//! command center and constructor at each start, as a skirmish gives), lists objectives for
//! the player (slot 0, `PLAYER`) and runs triggers that spawn reinforcements and enemy waves,
//! hand out credits, wake an idle enemy seat's AI and say things. The player wins once every
//! objective is done and loses when an objective fails (something to protect is destroyed) or
//! everything they have is.
//!
//! An enemy's units can carry standing orders (`Script`): patrol a loop of points, guard an
//! area, or walk a route of points as a wave. The mission drives them as squads, once a
//! second, and the seat's AI (if it plays) leaves them alone until their script ends. An enemy
//! seat may be limited to a build list (`Side::builds`), so its AI rebuilds and trains only
//! from that. The difficulty (`Difficulty`) scales the enemy's credits, its waves and the wait
//! between its timed reinforcements, all in one place.
//!
//! Missions are content (`Rules::missions`, written in g2-rules); a match plays one when it
//! starts from `World::with_mission`, and the replay keeps its id and difficulty. Objectives
//! and triggers are checked once a tick, after combat, so what died this tick counts; their
//! state is part of the state hash, only in a mission, so other matches hash as before.

use crate::defs::{BuildingRole, DefId, Faction, GeneralId, Rules};
use crate::entity::{EntityId, Order, PlayerId};
use crate::fixed::{Fx, Vec2};
use crate::map::{AiLevel, MapDef, MatchSetup, PlayerSetup};
use crate::world::{Event, TICKS_PER_SECOND, World};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct MissionId(pub u16);

/// The player's seat in every mission; every other seat is an enemy.
pub const PLAYER: PlayerId = PlayerId(0);
/// The team every enemy seat is on when a mission has more than one.
const ENEMY_TEAM: u8 = 1;

/// A patrol or route leg some unit can't finish (stuck, or fighting on and on) is given up
/// after this long, and the squad moves on without waiting for it.
const LEG_TICKS: u32 = 90 * TICKS_PER_SECOND;
/// A guard chasing an enemy keeps at it while the enemy is this close to the guarded area's
/// edge; beyond, it walks back.
const LEASH: Fx = Fx::from_int(20);

/// How hard a mission plays. Only the enemy changes, and only in what `Difficulty`'s methods
/// scale: its credits, the size of the forces its triggers spawn, and the wait between its
/// timed reinforcements (`When::Every`). The percentages are placeholders, measured only
/// against the `Normal` AI in the player's seat (`g2-maps/tests/campaign.rs`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum Difficulty {
    Easy,
    #[default]
    Normal,
    Hard,
}

impl Difficulty {
    pub const ALL: [Difficulty; 3] = [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard];

    pub fn name(self) -> &'static str {
        match self {
            Difficulty::Easy => "Easy",
            Difficulty::Normal => "Normal",
            Difficulty::Hard => "Hard",
        }
    }

    pub fn from_name(s: &str) -> Option<Difficulty> {
        Difficulty::ALL.into_iter().find(|d| d.name().eq_ignore_ascii_case(s))
    }

    /// An enemy's starting credits, or credits a trigger gives it.
    pub fn credits(self, amount: i64) -> i64 {
        amount * [60, 100, 140][self as usize] / 100
    }

    /// The units of an enemy force a trigger spawns: at least one is always left.
    pub fn count(self, n: u8) -> u8 {
        ((n as u32 * [60, 100, 140][self as usize] + 50) / 100).clamp(1, u8::MAX as u32) as u8
    }

    /// The wait between an enemy's timed reinforcements.
    pub fn period(self, ticks: u32) -> u32 {
        (ticks as u64 * [140, 100, 75][self as usize] / 100).max(1) as u32
    }
}

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

/// One seat. `ai` plays an enemy seat (`None`: it stands idle, its scripted units aside,
/// until a trigger wakes it with `Action::Wake`); the player's seat is the human's, or the
/// AI's at the level `Mission::setup` is given.
#[derive(Clone, Copy, Debug)]
pub struct Side {
    pub faction: Faction,
    pub credits: i64,
    pub ai: Option<AiLevel>,
    pub general: Option<GeneralId>,
    /// Everything this seat may build and train; `None`: all its faction offers. Its AI
    /// rebuilds and trains from this list only.
    pub builds: Option<&'static [DefId]>,
}

/// Standing orders a mission gives a group of an enemy's armed units.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Script {
    /// None: the seat's AI, if it plays, commands them as any other units.
    Hold,
    /// Loops the points, attack-moving from each to the next and from the last back to the
    /// first, so it fights what it meets on the way; the next leg starts once every unit is
    /// idle again (arrived, or back from a fight).
    Patrol(&'static [(i64, i64)]),
    /// Attack-moves along the points once, as a patrol's legs, then holds (`Hold`).
    Route(&'static [(i64, i64)]),
    /// Holds the area: fights what comes near, and a unit drawn out of it (idle out there, or
    /// chasing an enemy more than `LEASH` beyond its edge) walks back in.
    Guard(Area),
}

impl Script {
    fn points(self) -> &'static [(i64, i64)] {
        match self {
            Script::Patrol(p) | Script::Route(p) => p,
            Script::Hold | Script::Guard(_) => &[],
        }
    }
}

/// `count` entities of `def` for seat `owner`, the first at `at` (whole world units) and the
/// rest in rings around it. A tag names the first one for objectives and triggers, and the
/// force's armed units as a squad for `Action::Order`.
#[derive(Clone, Copy, Debug)]
pub struct Force {
    pub owner: u8,
    pub def: DefId,
    pub at: (i64, i64),
    pub count: u8,
    pub tag: Option<u8>,
    pub script: Script,
}

impl Force {
    pub const fn new(owner: u8, def: DefId, x: i64, y: i64) -> Force {
        Force { owner, def, at: (x, y), count: 1, tag: None, script: Script::Hold }
    }

    pub const fn times(self, count: u8) -> Force {
        Force { count, ..self }
    }

    pub const fn tagged(self, tag: u8) -> Force {
        Force { tag: Some(tag), ..self }
    }

    pub const fn patrol(self, route: &'static [(i64, i64)]) -> Force {
        Force { script: Script::Patrol(route), ..self }
    }

    pub const fn guard(self, area: Area) -> Force {
        Force { script: Script::Guard(area), ..self }
    }

    pub const fn along(self, route: &'static [(i64, i64)]) -> Force {
        Force { script: Script::Route(route), ..self }
    }

    /// Where its entities stand before the map clamps them: the first at `at`, the rest in
    /// rings around it, two world units apart at their edges.
    pub fn points(&self, rules: &Rules) -> impl Iterator<Item = Vec2> {
        let gap = rules.def(self.def).radius * Fx::from_int(2) + Fx::from_int(2);
        let at = Vec2::from_int(self.at.0, self.at.1);
        (0..self.count as u32).map(move |n| if n == 0 { at } else { at + crate::world::around(n - 1, gap) })
    }

    /// This force as a trigger spawns it at `d`: an enemy's scaled, the player's as written.
    pub fn at_difficulty(self, d: Difficulty) -> Force {
        if self.owner == PLAYER.0 { self } else { Force { count: d.count(self.count), ..self } }
    }
}

/// A circle on the map, in whole world units.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

/// Carries out `then` in order, the first tick `when` holds: once, or for `When::Every`, as
/// often as it says.
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
    /// Timed reinforcements: at `from`, then every `every` ticks (scaled by the difficulty,
    /// `Difficulty::period`), `times` times in all, and only while the entity tagged
    /// `while_alive` (a gate, a depot) stands, if one is named.
    Every { from: u32, every: u32, times: u16, while_alive: Option<u8> },
}

#[derive(Clone, Copy, Debug)]
pub enum Action {
    /// Spawns `forces` on open ground (an enemy's scaled by the difficulty). Their armed units
    /// with a script of their own follow it; the rest, with a `route`, attack-move along it
    /// together as one squad (`Script::Route`) at the slowest one's pace.
    Spawn { forces: &'static [Force], route: &'static [(i64, i64)] },
    /// A message on the player's screen.
    Say(&'static str),
    /// Credits for seat `player` (an enemy's scaled by the difficulty).
    Credits { player: u8, amount: i64 },
    /// The squads tagged `tag` take up `script`, from its first point.
    Order { tag: u8, script: Script },
    /// An idle enemy seat's AI starts playing at `level`: from here on it builds, trains and
    /// attacks as in a skirmish, with what the seat has (and `Side::builds`).
    Wake { player: u8, level: AiLevel },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Active,
    Done,
    Failed,
}

/// A group of an enemy's armed units the mission drives by its `script`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Squad {
    /// The force tag it was spawned with, for `Action::Order`.
    pub tag: Option<u8>,
    pub owner: PlayerId,
    pub script: Script,
    /// Its live units; the fallen are dropped once a second.
    pub units: Vec<EntityId>,
    /// Patrol and route: the index of the point the next leg goes to.
    pub leg: u16,
    /// The tick the current leg was ordered.
    pub since: u32,
}

/// A mission in play. Everything but `said` is part of the state hash.
#[derive(Clone, Debug)]
pub struct Run {
    pub id: MissionId,
    pub def: &'static Mission,
    pub difficulty: Difficulty,
    /// Per objective, in the mission's order.
    pub objectives: Vec<Status>,
    /// Per trigger: how many times it has fired.
    pub fired: Vec<u16>,
    /// Tagged entities, in spawn order.
    pub tags: Vec<(u8, EntityId)>,
    /// Squads with standing orders, in spawn order.
    pub squads: Vec<Squad>,
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
    /// human for `None`), the enemies by their own AI levels (or idle) with what an AI general
    /// slots. With two enemies or more they are one team, so they fight only the player. The
    /// difficulty is applied when the match starts (`World::with_mission`).
    pub fn setup(&self, rules: &Rules, map: MapDef, seed: u64, player: Option<AiLevel>) -> MatchSetup {
        let players = self
            .sides
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let ai = if i == PLAYER.0 as usize { player } else { s.ai };
                PlayerSetup {
                    faction: s.faction,
                    starting_credits: s.credits,
                    ai,
                    general: s.general,
                    perks: ai.map_or_else(Vec::new, |level| crate::ai::ai_perks(rules, s.general, level)),
                    rank: ai.map_or(1, crate::ai::ai_rank),
                    team: if i != PLAYER.0 as usize && self.sides.len() > 2 { ENEMY_TEAM } else { 0 },
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

    /// Whether unit `id` follows a mission script, so its seat's AI leaves it alone.
    pub fn scripted(&self, id: EntityId) -> bool {
        self.mission
            .as_ref()
            .is_some_and(|m| m.squads.iter().any(|s| s.script != Script::Hold && s.units.contains(&id)))
    }

    /// Spawns the mission's starting forces; called once, from `with_mission`. Their scripts
    /// start on the first second's update, once the grid stands.
    pub(crate) fn start_mission(&mut self, id: MissionId, difficulty: Difficulty) {
        let def = &self.rules.missions[id.0 as usize];
        let mut run = Run {
            id,
            def,
            difficulty,
            objectives: vec![Status::Active; def.objectives.len()],
            fired: vec![0; def.triggers.len()],
            tags: Vec::new(),
            squads: Vec::new(),
            result: None,
            said: Vec::new(),
        };
        for f in def.forces {
            self.spawn_force(f, false, &mut run);
        }
        // The setup holds the mission's own credits; the difficulty scales the enemy's here,
        // so the replay's setup stays the mission's.
        for p in self.players.iter_mut().skip(1) {
            p.credits = difficulty.credits(p.credits);
        }
        self.mission = Some(run);
    }

    /// Spawns `f`, recording its tag and, for a script, its armed units as a squad; returns the
    /// entities and that squad's index. `live` (from a trigger) places units on free ground and
    /// announces them as produced; the starting forces go exactly where the mission puts them,
    /// before any grid exists.
    fn spawn_force(&mut self, f: &Force, live: bool, run: &mut Run) -> (Vec<EntityId>, Option<usize>) {
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
            run.tags.push((tag, first));
        }
        let armed = d.is_unit() && d.weapon().is_some();
        let squad = (armed && f.script != Script::Hold).then(|| {
            run.squads.push(Squad {
                tag: f.tag,
                owner,
                script: f.script,
                units: out.clone(),
                leg: 0,
                since: self.tick,
            });
            run.squads.len() - 1
        });
        (out, squad)
    }

    /// Checks objectives, then fires triggers, then drives the squads (once a second), then
    /// settles the result. Runs after combat and before defeat, so what was destroyed this tick
    /// (hp ≤ 0) counts as gone.
    pub(crate) fn update_mission(&mut self) {
        let Some(mut run) = self.mission.take() else { return };
        if run.result.is_none() {
            self.check_objectives(&mut run);
            for (i, t) in run.def.triggers.iter().enumerate() {
                let limit = match t.when {
                    When::Every { times, .. } => times,
                    _ => 1,
                };
                if run.fired[i] < limit && self.holds(&run, t.when, run.fired[i]) {
                    run.fired[i] += 1;
                    for a in t.then {
                        self.act(&mut run, *a);
                    }
                }
            }
            if self.tick.is_multiple_of(TICKS_PER_SECOND) {
                for s in &mut run.squads {
                    self.drive(s);
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

    /// Gives squad `s` its next orders, by its script. Orders are set directly, as a wave's
    /// spawn sets them: the mission is the seat's script, not a player, so nothing is logged.
    fn drive(&mut self, s: &mut Squad) {
        s.units.retain(|&u| self.entities.get(u).is_some_and(|e| e.hp > 0));
        if s.units.is_empty() {
            return;
        }
        let points = s.script.points();
        match s.script {
            Script::Hold => {}
            Script::Patrol(_) | Script::Route(_) => {
                let idle = s.units.iter().all(|&u| self.entities.get(u).unwrap().order == Order::Idle);
                if !idle && self.tick < s.since + LEG_TICKS {
                    return;
                }
                if s.leg as usize >= points.len() {
                    // A route walked to its end: the seat's AI (or its guard) takes over.
                    s.script = Script::Hold;
                    return;
                }
                let (x, y) = points[s.leg as usize];
                self.attack_move(&s.units, Vec2::from_int(x, y));
                s.leg += 1;
                if matches!(s.script, Script::Patrol(_)) && s.leg as usize == points.len() {
                    s.leg = 0;
                }
                s.since = self.tick;
            }
            Script::Guard(area) => {
                let reach = Fx::from_int(area.radius) + LEASH;
                let mut back: Vec<EntityId> = Vec::new();
                let mut walk: Vec<EntityId> = Vec::new();
                for &u in &s.units {
                    let e = self.entities.get(u).unwrap();
                    match e.order {
                        Order::Idle if !area.contains(e.pos) => back.push(u),
                        Order::AttackMove { engaged: Some(t), .. } | Order::Attack { target: t } => {
                            let far = self.entities.get(t).is_none_or(|t| t.pos.distance(area.centre()) > reach);
                            if far {
                                walk.push(u);
                            }
                        }
                        _ => {}
                    }
                }
                // Out of it and idle: back in, fighting on the way. Dragged off by a chase:
                // back without stopping to fight, so the chase ends.
                if !back.is_empty() {
                    self.attack_move(&back, area.centre());
                }
                if !walk.is_empty() {
                    let pace = self.group_pace(&walk);
                    for (u, dest) in self.formation(&walk, area.centre()) {
                        let e = self.entities.get_mut(u).unwrap();
                        e.order = Order::Move { target: dest, pace };
                        e.home = None;
                    }
                }
            }
        }
    }

    /// `units` attack-move to `to` together, in formation, at the slowest one's pace.
    fn attack_move(&mut self, units: &[EntityId], to: Vec2) {
        let pace = self.group_pace(units);
        for (u, dest) in self.formation(units, to) {
            let e = self.entities.get_mut(u).unwrap();
            e.order = Order::AttackMove { dest, pace, engaged: None };
            e.home = None;
        }
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

    /// Whether `when` holds now, for a trigger that has fired `fired` times.
    fn holds(&self, run: &Run, when: When, fired: u16) -> bool {
        match when {
            When::At(t) => self.tick >= t,
            When::Entered { player, area } => self.in_area(run, PlayerId(player), area, None),
            When::Destroyed(tag) => !self.alive(run, tag),
            When::Done(n) => run.objectives.get(n as usize) == Some(&Status::Done),
            When::Every { from, every, while_alive, .. } => {
                let due = from as u64 + run.difficulty.period(every) as u64 * fired as u64;
                self.tick as u64 >= due && while_alive.is_none_or(|t| self.alive(run, t))
            }
        }
    }

    fn act(&mut self, run: &mut Run, a: Action) {
        let d = run.difficulty;
        match a {
            Action::Say(text) => run.said.push((self.tick, text)),
            Action::Credits { player, amount } => {
                let amount = if player == PLAYER.0 { amount } else { d.credits(amount) };
                self.players[player as usize].credits += amount;
            }
            Action::Spawn { forces, route } => {
                let mut wave: Vec<EntityId> = Vec::new();
                for f in forces {
                    let (ids, squad) = self.spawn_force(&f.at_difficulty(d), true, run);
                    match squad {
                        Some(i) => {
                            let mut s = run.squads[i].clone();
                            self.drive(&mut s);
                            run.squads[i] = s;
                        }
                        None => wave.extend(ids.into_iter().filter(|&id| {
                            let d = self.rules.def(self.entities.get(id).unwrap().def);
                            d.is_unit() && d.weapon().is_some()
                        })),
                    }
                }
                if route.is_empty() || wave.is_empty() {
                    return;
                }
                let owner = self.entities.get(wave[0]).unwrap().owner.unwrap();
                let mut s =
                    Squad { tag: None, owner, script: Script::Route(route), units: wave, leg: 0, since: self.tick };
                self.drive(&mut s);
                run.squads.push(s);
            }
            Action::Order { tag, script } => {
                for i in 0..run.squads.len() {
                    if run.squads[i].tag == Some(tag) {
                        let mut s = run.squads[i].clone();
                        (s.script, s.leg, s.since) = (script, 0, self.tick);
                        // A new leg starts at once, whatever the squad was doing.
                        for &u in &s.units {
                            if let Some(e) = self.entities.get_mut(u) {
                                e.order = Order::Idle;
                            }
                        }
                        self.drive(&mut s);
                        run.squads[i] = s;
                    }
                }
            }
            // An AI already playing keeps what it has learnt; a second trigger may wake it too.
            Action::Wake { player, level } => {
                self.ai[player as usize].get_or_insert_with(|| crate::ai::Ai::new(PlayerId(player), level));
            }
        }
    }
}

/// Checks a mission's content against the rules and its map: seats exist, every force
/// stands on walkable ground inside the map, buildings don't overlap each other or terrain,
/// every tag and objective a goal or trigger names exists, every script's points (patrols,
/// routes, guarded areas) lie on open ground clear of the mission's buildings, only an
/// enemy's armed units carry a script, every squad an order names exists, and only an idle
/// seat is woken. Tests run it on every shipped mission.
pub fn validate(rules: &Rules, m: &Mission, map: &MapDef) -> Result<(), String> {
    let seats = m.sides.len();
    if seats < 2 || seats > map.spawns.len().max(2) {
        return Err(format!("{}: {seats} sides", m.name));
    }
    if m.sides[PLAYER.0 as usize].builds.is_some() {
        return Err(format!("{}: the player builds what their faction offers", m.name));
    }
    for (i, s) in m.sides.iter().enumerate() {
        let wrong = s.builds.unwrap_or(&[]).iter().find(|&&d| rules.def(d).faction.is_some_and(|f| f != s.faction));
        if let Some(&d) = wrong {
            return Err(format!("{}: seat {i} may build {}, not its faction's", m.name, rules.def(d).name));
        }
    }
    let actions = || m.triggers.iter().flat_map(|t| t.then);
    let all_forces = m.forces.iter().chain(actions().flat_map(|a| match a {
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
    // never starts on a blocked cell; so does every point a script sends units to.
    let blocked: Vec<(Vec2, Fx)> =
        buildings.iter().chain(&statics).map(|&(p, r)| (p, r + Fx::from_int(crate::nav::CLEARANCE))).collect();
    let open = |(x, y): (i64, i64)| {
        let p = Vec2::from_int(x, y);
        map.contains(p) && map.walkable(p) && clear(p, Fx::ZERO, &blocked)
    };
    let script_ok = |s: Script| match s {
        Script::Hold => true,
        Script::Patrol(p) => p.len() >= 2 && p.iter().all(|&q| open(q)),
        Script::Route(p) => !p.is_empty() && p.iter().all(|&q| open(q)),
        Script::Guard(a) => a.radius > 0 && open(a.at),
    };
    let mut tags = Vec::new();
    let mut squads = Vec::new();
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
        if f.script != Script::Hold {
            let armed = d.is_unit() && d.weapon().is_some();
            if !armed || f.owner == PLAYER.0 || !script_ok(f.script) {
                return Err(format!("{}: the script of {} at {:?}: {:?}", m.name, d.name, f.at, f.script));
            }
            squads.extend(f.tag);
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
            When::Every { every, times, while_alive, .. } => every > 0 && times > 0 && while_alive.is_none_or(tag_ok),
        };
        if !ok {
            return Err(format!("{}: trigger {:?}", m.name, t.when));
        }
    }
    for a in actions() {
        let ok = match *a {
            // A route drives an enemy's wave: the player's reinforcements are theirs to command.
            Action::Spawn { forces, route } => {
                route.iter().all(|&q| open(q)) && (route.is_empty() || forces.iter().all(|f| f.owner != PLAYER.0))
            }
            Action::Order { tag, script } => squads.contains(&tag) && script_ok(script),
            Action::Wake { player, .. } => {
                player != PLAYER.0 && m.sides.get(player as usize).is_some_and(|s| s.ai.is_none())
            }
            Action::Credits { player, .. } => (player as usize) < seats,
            Action::Say(_) => true,
        };
        if !ok {
            return Err(format!("{}: action {a:?}", m.name));
        }
    }
    if m.objectives.iter().all(|o| matches!(o.goal, Goal::Protect { .. })) {
        return Err(format!("{}: nothing to do but protect: it can't be won", m.name));
    }
    Ok(())
}
