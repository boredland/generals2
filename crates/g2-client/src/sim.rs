//! Bridge between the deterministic `g2_sim::World` and Bevy. The sim owns all game state;
//! Bevy only mirrors it for rendering and turns input into `CommandKind`s.

use bevy::prelude::*;
use g2_maps::MapFile;
use g2_rules::rules;
use g2_sim::{CommandKind, MapDef, MatchSetup, PlayerId, PlayerSetup, TICKS_PER_SECOND, World};

#[derive(Resource)]
pub struct Sim {
    pub world: World,
    pub setup: MatchSetup,
    /// Sim ticks per fixed step: 0 = paused, 1 = normal, up to 8× fast-forward.
    pub speed: u32,
    /// The parsed source map when playing an imported Generals / Zero Hour map; terrain, roads,
    /// water and trees are drawn from it. `None` for the built-in procedural map.
    pub source: Option<MapFile>,
    /// Draw the local player's fog of war. Off (`--reveal`) shows everything, for watching.
    pub fog: bool,
    /// Each player's index into `models::TEAM_PALETTE` (index = player).
    pub teams: Vec<usize>,
    /// Orders given since the sound last acknowledged them (drained every frame by `audio`).
    pub unheard_orders: u32,
    /// The player this client controls and sees through (0 unless joined over the network).
    pub local: PlayerId,
    /// A network match: orders go through the lockstep and the sim steps in time with the
    /// other player's machine. `None` for a match on this machine alone.
    pub online: Option<crate::net::Online>,
}

impl Sim {
    pub fn new(map: MapDef, source: Option<MapFile>, seed: u64, players: impl Into<Vec<PlayerSetup>>) -> Sim {
        let players = players.into();
        let teams = crate::models::team_slots(&players.iter().map(|p| p.faction).collect::<Vec<_>>());
        let setup = MatchSetup { map, seed, players };
        Sim {
            world: World::new(&setup, rules()),
            setup,
            speed: 1,
            source,
            fog: true,
            teams,
            unheard_orders: 0,
            local: PlayerId(0),
            online: None,
        }
    }

    /// Team colour of `player`.
    pub fn team_color(&self, player: PlayerId) -> Color {
        crate::models::TEAM_PALETTE[self.teams[player.0 as usize]]
    }

    /// Ticks until the local player's general's power can be used (0: now); `None` without
    /// a power.
    pub fn power_ready_in(&self) -> Option<u32> {
        self.world.player(self.local).power_ready.map(|t| t.saturating_sub(self.world.tick()))
    }

    /// The local player's general's power, if any.
    pub fn power(&self) -> Option<g2_sim::defs::Power> {
        self.world.player(self.local).general.and_then(|g| self.world.rules.general(g).power)
    }

    /// Whether the local player sees `e` right now: own entities always, neutral resources once
    /// their spot is explored (they never change hands or move), enemies only where visible.
    pub fn sees(&self, e: &g2_sim::Entity) -> bool {
        let v = self.world.vision(self.local);
        !self.fog
            || match e.owner {
                Some(o) => o == self.local || v.is_visible(e.pos),
                None => v.is_explored(e.pos),
            }
    }

    /// Whether `e` is drawn: what the local player sees, plus enemy buildings it remembers.
    /// Buildings don't move, so a remembered one stands where it was seen. One destroyed out
    /// of sight stays drawn as a ghost (`view::sync_entities`) until the spot is seen again.
    pub fn shows(&self, e: &g2_sim::Entity) -> bool {
        self.sees(e) || self.remembers(e.id)
    }

    /// Whether the local player's fog still remembers entity `id` (an enemy building, possibly
    /// since destroyed). Always false with the fog off.
    pub fn remembers(&self, id: g2_sim::EntityId) -> bool {
        self.fog && self.world.vision(self.local).memory.contains_key(&id)
    }

    /// The local player's fog cells for drawing, row-major on the sim's fog grid: 0 never
    /// explored, 1 explored but not visible now, 2 visible. All 2 with the fog off.
    pub fn fog_states(&self, out: &mut Vec<u8>) {
        let v = self.world.vision(self.local);
        out.clear();
        out.extend((0..v.h).flat_map(|y| (0..v.w).map(move |x| (x, y))).map(|(x, y)| match v.cell_state(x, y) {
            _ if !self.fog => 2,
            (_, true) => 2,
            (true, false) => 1,
            (false, false) => 0,
        }));
    }

    /// Whether the local player has explored world point `(x, z)` (always, with the fog off).
    pub fn explored_at(&self, x: f32, z: f32) -> bool {
        let v = self.world.vision(self.local);
        let c = g2_sim::fog::CELL as f32;
        let (cx, cy) = ((x / c).floor() as i32, (z / c).floor() as i32);
        !self.fog || ((0..v.w).contains(&cx) && (0..v.h).contains(&cy) && v.cell_state(cx, cy).0)
    }

    /// Players with 5,000 credits and no general; the second is always the AI. Shared by the
    /// client's test modules.
    #[cfg(test)]
    pub fn players(factions: [g2_sim::Faction; 2], spectate: bool) -> [PlayerSetup; 2] {
        [
            PlayerSetup {
                faction: factions[0],
                starting_credits: 5_000,
                ai: spectate.then_some(g2_sim::AiLevel::Easy),
                general: None,
            },
            PlayerSetup {
                faction: factions[1],
                starting_credits: 5_000,
                ai: Some(g2_sim::AiLevel::Easy),
                general: None,
            },
        ]
    }

    pub fn order(&mut self, kind: CommandKind) {
        if self.world.is_ai(self.local) {
            return;
        }
        match &mut self.online {
            Some(net) => net.order(kind),
            None => {
                self.world.submit(self.local, kind);
            }
        }
        self.unheard_orders += 1;
    }

    /// Whether the sim may take its next step: always off the network; online, once the other
    /// player's orders for it are in. `first`: the first step of this fixed update.
    pub fn may_step(&mut self, first: bool) -> bool {
        match &mut self.online {
            Some(net) => net.ready(&mut self.world, first),
            None => true,
        }
    }

    /// Writes the match so far as a replay that `g2-tools verify` can re-simulate.
    pub fn save_replay(&self, path: &str) -> std::io::Result<()> {
        std::fs::write(path, self.world.replay(&self.setup).encode())
    }

    /// Rendered height of what units stand on at world `(x, z)`: the imported map's terrain or
    /// bridge deck, or 0 on the built-in map. The sim itself is flat; this only places models
    /// and the cursor.
    pub fn ground_y(&self, x: f32, z: f32) -> f32 {
        self.source.as_ref().map_or(0.0, |m| m.surface_y(x, z))
    }

    /// Sim position → rendered position on the ground.
    pub fn to_world(&self, p: g2_sim::Vec2) -> Vec3 {
        let flat = to_world(p);
        Vec3::new(flat.x, self.ground_y(flat.x, flat.z), flat.z)
    }
}

/// Fixed-point sim position → Bevy world on the flat sim plane (sim x → +X, sim y → +Z, Y = 0).
/// Use `Sim::to_world` to place things on the rendered terrain.
pub fn to_world(p: g2_sim::Vec2) -> Vec3 {
    Vec3::new(p.x.to_f32(), 0.0, p.y.to_f32())
}

/// Bevy ground point → sim position, snapped to whole world units. Snapping keeps recorded
/// commands independent of float rounding in the camera ray.
pub fn to_sim(p: Vec3) -> g2_sim::Vec2 {
    g2_sim::Vec2::from_int(p.x.round() as i64, p.z.round() as i64)
}

/// Sim positions at the start of the current tick, for interpolating between ticks.
#[derive(Resource, Default)]
pub struct Previous(pub std::collections::BTreeMap<g2_sim::EntityId, g2_sim::Vec2>);

/// The local player's deliveries since the HUD last drained this: `(depot, credits)`. Sim events
/// only live for one step and a frame can run several, so they are gathered here every step.
#[derive(Resource, Default)]
pub struct Deliveries(pub Vec<(g2_sim::EntityId, i64)>);

/// Every shot since the view last drained this, gathered per step like `Deliveries`.
#[derive(Resource, Default)]
pub struct Hits(pub Vec<Shot>);

/// Sounds the local player should hear, gathered per step like `Deliveries` (see
/// `audio::sim_cues`); the sound drains it every frame. Apps without sound (headless renders,
/// tests) never drain it, so it keeps only the newest `HEARD_MAX`.
#[derive(Resource, Default)]
pub struct Heard(pub Vec<(crate::audio::Cue, Option<g2_sim::Vec2>)>);

/// Far more than a frame can play (`audio::VOICES_PER_FRAME` per kind).
const HEARD_MAX: usize = 256;

/// Where `--record` saves the match when the app exits.
#[derive(Resource)]
pub struct RecordTo(pub String);

/// Saves the match on any app exit: the menu's Quit, closing the window, Ctrl-C.
pub fn save_replay_on_exit(mut exits: MessageReader<AppExit>, sim: Res<Sim>, to: Res<RecordTo>) {
    if exits.read().next().is_some() {
        match sim.save_replay(&to.0) {
            Ok(()) => info!("replay saved to {} ({} ticks)", to.0, sim.world.tick()),
            Err(e) => error!("could not save replay to {}: {e}", to.0),
        }
    }
}

/// What the view draws for combat.
#[derive(Clone, Copy, Debug)]
pub enum Shot {
    /// A hit on a target at `at`; `tracer` for direct fire (a shell's hits are its blast).
    Hit { from: g2_sim::Vec2, at: g2_sim::Vec2, killed: bool, tracer: bool, target: Option<g2_sim::DefId> },
    /// A shell from `from` to `at`, in the air for `flight` more ticks, `done` of its whole flight
    /// (0 at the muzzle) already behind it.
    Shell { from: g2_sim::Vec2, at: g2_sim::Vec2, flight: u32, done: f32 },
    /// A shell landed at `at`; its blast has `radius`.
    Blast { at: g2_sim::Vec2, radius: g2_sim::Fx },
}

pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(TICKS_PER_SECOND as f64))
            .init_resource::<Previous>()
            .init_resource::<Deliveries>()
            .init_resource::<Hits>()
            .init_resource::<Heard>()
            .add_systems(FixedUpdate, step_sim);
    }
}

fn step_sim(
    mut sim: ResMut<Sim>,
    mut prev: ResMut<Previous>,
    mut deliveries: ResMut<Deliveries>,
    mut hits: ResMut<Hits>,
    mut heard: ResMut<Heard>,
) {
    if sim.speed == 0 {
        return;
    }
    // Snapshot once per fixed step, so with fast-forward the render interpolates across all of
    // this step's sim ticks instead of only the last one.
    prev.0 = sim.world.entities().map(|e| (e.id, e.pos)).collect();
    let me = sim.local;
    // Online, a step beyond the first only catches up with the other player; there is no
    // fast-forward (`speed` stays 1, and `+` is refused in `input`).
    let steps = if sim.online.is_some() { 2 } else { sim.speed };
    for i in 0..steps {
        if !sim.may_step(i == 0) {
            break;
        }
        sim.world.step();
        for e in sim.world.events() {
            if let g2_sim::world::Event::Delivered { player, depot, credits } = *e
                && player == me
            {
                deliveries.0.push((depot, credits));
            }
        }
        collect_shots(&sim.world, &mut hits);
        heard.0.extend(crate::audio::sim_cues(&sim));
        if heard.0.len() > HEARD_MAX {
            let over = heard.0.len() - HEARD_MAX;
            heard.0.drain(..over);
        }
        if sim.world.winner().is_some() {
            sim.speed = 0;
            break;
        }
    }
}

/// Turns the last step's combat events into `Shot`s for the view. A shell's blast is drawn at
/// its landing, so its hits get no tracer of their own.
pub fn collect_shots(world: &g2_sim::World, hits: &mut Hits) {
    let tick = world.tick();
    for e in world.events() {
        match *e {
            g2_sim::world::Event::Hit { shooter, at, killed, splash, target_def, .. } => {
                let from = world.entity(shooter).map_or(at, |s| s.pos);
                hits.0.push(Shot::Hit { from, at, killed, tracer: !splash, target: killed.then_some(target_def) });
            }
            g2_sim::world::Event::Fired { from, at, lands, .. } => {
                hits.0.push(Shot::Shell { from, at, flight: lands.saturating_sub(tick) + 1, done: 0.0 })
            }
            g2_sim::world::Event::Landed { at, radius } => hits.0.push(Shot::Blast { at, radius }),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use g2_rules::crater_ridge;
    use g2_sim::Faction;

    #[test]
    fn clicks_snap_to_whole_units() {
        assert_eq!(to_sim(Vec3::new(123.4, 0.0, 76.6)), g2_sim::Vec2::from_int(123, 77));
    }

    /// Plays through the real Bevy schedule with human orders issued between frames and a speed
    /// change mid-match, then checks the client's saved replay re-simulates to the same state.
    #[test]
    fn a_match_played_through_the_client_replays_exactly() {
        use bevy::time::TimeUpdateStrategy;
        use g2_rules::ids::*;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(16)))
            .insert_resource(Sim::new(crater_ridge(), None, 4, Sim::players([Faction::Gla, Faction::Apa], false)))
            .add_plugins(SimPlugin);
        let run = |app: &mut App, frames: u32| {
            for _ in 0..frames {
                app.update();
            }
        };
        run(&mut app, 20);
        {
            let mut sim = app.world_mut().resource_mut::<Sim>();
            let me = sim.local;
            let worker = sim.world.entities().find(|e| e.owner == Some(me) && e.def == GLA_WORKER).unwrap().id;
            sim.order(CommandKind::Build { builder: worker, def: GLA_STASH, pos: to_sim(Vec3::new(110.3, 0.0, 79.8)) });
            sim.speed = 4;
        }
        run(&mut app, 600);
        {
            let mut sim = app.world_mut().resource_mut::<Sim>();
            let me = sim.local;
            let stash = sim.world.entities().find(|e| e.owner == Some(me) && e.def == GLA_STASH).unwrap();
            assert!(stash.is_complete(), "stash finished during the x4 burst");
            let stash = stash.id;
            sim.order(CommandKind::Train { producer: stash, def: GLA_WORKER });
            sim.order(CommandKind::Train { producer: stash, def: GLA_WORKER });
            sim.speed = 1;
        }
        run(&mut app, 60);
        let sim = app.world().resource::<Sim>();
        let me = sim.local;
        let stash = sim.world.entities().find(|e| e.owner == Some(me) && e.def == GLA_STASH).unwrap();
        assert_eq!(stash.queue.len(), 2, "both training orders were accepted");
        let path = std::env::temp_dir().join(format!("g2-client-test-{}.g2rp", std::process::id()));
        sim.save_replay(path.to_str().unwrap()).unwrap();
        let replay = g2_sim::Replay::decode(&std::fs::read(&path).unwrap()).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(replay.commands.len(), 3);
        assert_eq!(g2_sim::verify_replay(&replay, crater_ridge(), rules()), Ok(sim.world.state_hash()));
    }

    /// Runs the real Bevy schedule (no renderer) for `secs` of virtual time.
    fn ticks_after(speed: u32, secs: u32) -> u32 {
        use bevy::time::TimeUpdateStrategy;
        let frame = std::time::Duration::from_millis(10);
        let mut app = App::new();
        let mut sim = Sim::new(crater_ridge(), None, 1, Sim::players([Faction::Eu, Faction::Gla], true));
        sim.speed = speed;
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(frame))
            .insert_resource(sim)
            .add_plugins(SimPlugin);
        for _ in 0..secs * 100 {
            app.update();
        }
        app.world().resource::<Sim>().world.tick()
    }

    #[test]
    fn fixed_update_drives_the_sim_at_30_hz_and_scales_with_speed() {
        let normal = ticks_after(1, 4);
        assert!((118..=120).contains(&normal), "{normal} ticks in 4 s");
        let fast = ticks_after(8, 4);
        assert!((8 * 118..=8 * 120).contains(&fast), "{fast} ticks in 4 s at x8");
        assert_eq!(ticks_after(0, 2), 0, "paused");
    }

    /// At ×8 a fixed step runs eight sim ticks; every local delivery in them still reaches the
    /// HUD queue. The sim is deterministic, so a reference world stepped one tick at a time from
    /// the same setup gives the exact deliveries to expect over the same ticks.
    #[test]
    fn every_local_delivery_is_collected_at_fast_forward() {
        use bevy::time::TimeUpdateStrategy;
        let mut app = App::new();
        let mut sim = Sim::new(crater_ridge(), None, 1, Sim::players([Faction::Eu, Faction::Gla], true));
        sim.speed = 8;
        let me = sim.local;
        let mut reference = World::new(&sim.setup, rules());
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(10)))
            .insert_resource(sim)
            .add_plugins(SimPlugin);
        let mut collected = Vec::new();
        for _ in 0..2_000 {
            app.update();
            collected.append(&mut app.world_mut().resource_mut::<Deliveries>().0);
        }
        let ticks = app.world().resource::<Sim>().world.tick();
        let mut expected = Vec::new();
        while reference.tick() < ticks {
            reference.step();
            for e in reference.events() {
                if let g2_sim::world::Event::Delivered { player, depot, credits } = *e
                    && player == me
                {
                    expected.push((depot, credits));
                }
            }
        }
        // 2,000 frames of 10 ms at ×8 and 30 Hz ≈ 4,800 ticks, about 2.7 minutes of match time.
        assert!(ticks > 4_700, "ran {ticks} ticks");
        assert!(!expected.is_empty(), "the AI delivered nothing in {ticks} ticks");
        assert_eq!(collected, expected);
    }
}
