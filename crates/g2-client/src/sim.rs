//! Bridge between the deterministic `g2_sim::World` and Bevy. The sim owns all game state;
//! Bevy only mirrors it for rendering and turns input into `CommandKind`s.

use bevy::prelude::*;
use g2_maps::MapFile;
use g2_rules::rules;
use g2_sim::{CommandKind, Faction, MapDef, MatchSetup, PlayerId, PlayerSetup, TICKS_PER_SECOND, World};

/// The local human player.
pub const LOCAL: PlayerId = PlayerId(0);

#[derive(Resource)]
pub struct Sim {
    pub world: World,
    pub setup: MatchSetup,
    /// Sim ticks per fixed step: 0 = paused, 1 = normal, up to 8× fast-forward.
    pub speed: u32,
    /// The parsed source map when playing an imported Generals / Zero Hour map; terrain, roads,
    /// water and trees are drawn from it. `None` for the built-in procedural map.
    pub source: Option<MapFile>,
}

impl Sim {
    pub fn new(map: MapDef, source: Option<MapFile>, seed: u64, factions: [Faction; 2], spectate: bool) -> Sim {
        let setup = MatchSetup {
            map,
            seed,
            players: vec![
                PlayerSetup { faction: factions[0], starting_credits: 5_000, ai: spectate },
                PlayerSetup { faction: factions[1], starting_credits: 5_000, ai: true },
            ],
        };
        Sim { world: World::new(&setup, rules()), setup, speed: 1, source }
    }

    pub fn order(&mut self, kind: CommandKind) {
        if !self.world.is_ai(LOCAL) {
            self.world.submit(LOCAL, kind);
        }
    }

    /// Writes the match so far as a replay that `g2-tools verify` can re-simulate.
    pub fn save_replay(&self, path: &str) -> std::io::Result<()> {
        std::fs::write(path, self.world.replay(&self.setup).encode())
    }

    /// Rendered ground height at world `(x, z)`: the imported map's terrain, or 0 on the
    /// built-in map. The sim itself is flat; this only places models and the cursor.
    pub fn ground_y(&self, x: f32, z: f32) -> f32 {
        self.source.as_ref().map_or(0.0, |m| m.ground_y(x, z))
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

pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(TICKS_PER_SECOND as f64))
            .init_resource::<Previous>()
            .add_systems(FixedUpdate, step_sim);
    }
}

fn step_sim(mut sim: ResMut<Sim>, mut prev: ResMut<Previous>) {
    if sim.speed == 0 {
        return;
    }
    // Snapshot once per fixed step, so with fast-forward the render interpolates across all of
    // this step's sim ticks instead of only the last one.
    prev.0 = sim.world.entities().map(|e| (e.id, e.pos)).collect();
    for _ in 0..sim.speed {
        sim.world.step();
        if sim.world.winner().is_some() {
            sim.speed = 0;
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use g2_rules::crater_ridge;

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
            .insert_resource(Sim::new(crater_ridge(), None, 4, [Faction::Gla, Faction::Apa], false))
            .add_plugins(SimPlugin);
        let run = |app: &mut App, frames: u32| {
            for _ in 0..frames {
                app.update();
            }
        };
        run(&mut app, 20);
        {
            let mut sim = app.world_mut().resource_mut::<Sim>();
            let worker = sim.world.entities().find(|e| e.owner == Some(LOCAL) && e.def == GLA_WORKER).unwrap().id;
            sim.order(CommandKind::Build { builder: worker, def: GLA_STASH, pos: to_sim(Vec3::new(110.3, 0.0, 79.8)) });
            sim.speed = 4;
        }
        run(&mut app, 600);
        {
            let mut sim = app.world_mut().resource_mut::<Sim>();
            let stash = sim.world.entities().find(|e| e.owner == Some(LOCAL) && e.def == GLA_STASH).unwrap();
            assert!(stash.is_complete(), "stash finished during the x4 burst");
            let stash = stash.id;
            sim.order(CommandKind::Train { producer: stash, def: GLA_WORKER });
            sim.order(CommandKind::Train { producer: stash, def: GLA_WORKER });
            sim.speed = 1;
        }
        run(&mut app, 60);
        let sim = app.world().resource::<Sim>();
        let stash = sim.world.entities().find(|e| e.owner == Some(LOCAL) && e.def == GLA_STASH).unwrap();
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
        let mut sim = Sim::new(crater_ridge(), None, 1, [Faction::Eu, Faction::Gla], true);
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
}
