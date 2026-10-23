//! Generals 2 client.
//!
//!   g2-client [--map NAME] [--faction eu|apa|gla] [--enemy eu|apa|gla] [--general NAME]
//!             [--enemy-general NAME] [--seed N] [--spectate] [--reveal] [--record FILE]
//!   g2-client --screenshot out.png [--ticks N] [--view base|map|battle] [...]
//!             headless: simulate N ticks, render one frame
//!
//! `--map` takes "Crater Ridge" (built in, the default) or `<gen|zh>:<name>` for an original
//! Generals / Zero Hour map (`g2-tools maps` lists them). `--general` picks a general of your
//! faction (Bulldog, Taskmaster, Junkyard); without one you play the plain faction. `--spectate`
//! lets the AI play your side too (AI vs AI). `--reveal` draws the whole map without your fog
//! of war (the sim's fog still applies to both sides). `--record` saves the match as a replay
//! on exit; check it with `g2-tools verify FILE`. Screenshot mode needs a Vulkan adapter but no
//! window or display server.

mod camera;
mod hud;
mod input;
mod look;
mod models;
mod sim;
mod terrain;
mod view;

use bevy::prelude::*;
use g2_sim::{Faction, GeneralId, PlayerSetup};

use crate::sim::{LOCAL, Sim, SimPlugin};

struct Args {
    map: String,
    factions: [Faction; 2],
    generals: [Option<String>; 2],
    seed: u64,
    spectate: bool,
    /// Level of every AI player in the match.
    ai: g2_sim::AiLevel,
    reveal: bool,
    screenshot: Option<String>,
    record: Option<String>,
    ticks: u32,
    view: headless::View,
    size: (u32, u32),
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        map: "Crater Ridge".to_owned(),
        factions: [Faction::Eu, Faction::Gla],
        generals: [None, None],
        seed: 1,
        spectate: false,
        ai: g2_sim::AiLevel::Normal,
        reveal: false,
        screenshot: None,
        record: None,
        ticks: 900,
        view: headless::View::Base,
        size: (1280, 720),
    };
    let mut it = std::env::args().skip(1);
    let faction = |v: Option<String>| match v.as_deref() {
        Some("eu") => Ok(Faction::Eu),
        Some("apa") => Ok(Faction::Apa),
        Some("gla") => Ok(Faction::Gla),
        other => Err(format!("expected eu|apa|gla, got {other:?}")),
    };
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--map" => a.map = it.next().ok_or("--map needs a name")?,
            "--faction" => a.factions[0] = faction(it.next())?,
            "--enemy" => a.factions[1] = faction(it.next())?,
            "--general" => a.generals[0] = Some(it.next().ok_or("--general needs a name")?),
            "--enemy-general" => a.generals[1] = Some(it.next().ok_or("--enemy-general needs a name")?),
            "--seed" => a.seed = it.next().and_then(|v| v.parse().ok()).ok_or("--seed needs a number")?,
            "--spectate" => a.spectate = true,
            "--ai" => {
                a.ai = match it.next().as_deref() {
                    Some("easy") => g2_sim::AiLevel::Easy,
                    Some("normal") => g2_sim::AiLevel::Normal,
                    other => return Err(format!("--ai expects easy|normal, got {other:?}")),
                }
            }
            "--reveal" => a.reveal = true,
            "--screenshot" => a.screenshot = Some(it.next().ok_or("--screenshot needs a path")?),
            "--record" => a.record = Some(it.next().ok_or("--record needs a path")?),
            "--ticks" => a.ticks = it.next().and_then(|v| v.parse().ok()).ok_or("--ticks needs a number")?,
            "--view" => {
                a.view = match it.next().as_deref() {
                    Some("base") => headless::View::Base,
                    Some("map") => headless::View::Map,
                    Some("battle") => headless::View::Battle,
                    other => return Err(format!("--view expects base|map|battle, got {other:?}")),
                }
            }
            "--size" => {
                let v = it.next().ok_or("--size needs WxH")?;
                let (w, h) = v.split_once('x').ok_or("--size needs WxH")?;
                a.size = (w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?);
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(a)
}

fn main() -> AppExit {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return AppExit::error();
        }
    };
    let sim = match load_sim(&args) {
        Ok(sim) => sim,
        Err(e) => {
            eprintln!("{e}");
            return AppExit::error();
        }
    };
    match args.screenshot {
        Some(path) => headless::run(sim, args.ticks, path, args.view, args.size),
        None => windowed(sim, args.record),
    }
}

fn load_sim(args: &Args) -> Result<Sim, String> {
    let spectate = args.spectate || args.screenshot.is_some();
    let source = if args.map.contains(':') {
        Some(g2_maps::load_file_named(&g2_maps::vendored_root(), &args.map)?)
    } else {
        None
    };
    let map = match &source {
        Some(file) => file.to_map_def(&args.map),
        None => g2_maps::resolve(&args.map)?,
    };
    if map.spawns.len() < 2 {
        return Err(format!(
            "{} has {} player starts; pick a skirmish map (g2-tools maps)",
            args.map,
            map.spawns.len()
        ));
    }
    let rules = g2_rules::rules();
    let general = |i: usize| -> Result<Option<GeneralId>, String> {
        let Some(name) = &args.generals[i] else { return Ok(None) };
        let id = rules.find_general(name).ok_or_else(|| format!("unknown general {name:?}"))?;
        let g = rules.general(id);
        if g.faction != args.factions[i] {
            return Err(format!("{} is a {:?} general, not {:?}", g.name, g.faction, args.factions[i]));
        }
        Ok(Some(id))
    };
    let player = |i: usize, ai: bool| -> Result<PlayerSetup, String> {
        Ok(PlayerSetup {
            faction: args.factions[i],
            starting_credits: 5_000,
            ai: ai.then_some(args.ai),
            general: general(i)?,
        })
    };
    let mut sim = Sim::new(map, source, args.seed, [player(0, spectate)?, player(1, true)?]);
    sim.fog = !args.reveal;
    Ok(sim)
}

#[derive(Resource)]
struct RecordTo(String);

fn windowed(sim: Sim, record: Option<String>) -> AppExit {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window { title: "Generals 2 (open-source recreation)".into(), ..default() }),
        ..default()
    }))
    .insert_resource(sim)
    .add_plugins((
        SimPlugin,
        terrain::TerrainPlugin,
        view::ViewPlugin,
        camera::CameraPlugin,
        input::InputPlugin,
        hud::HudPlugin,
    ))
    .add_systems(Startup, spawn_camera);
    if let Some(path) = record {
        app.insert_resource(RecordTo(path)).add_systems(Last, save_replay_on_exit);
    }
    app.run()
}

fn save_replay_on_exit(mut exits: MessageReader<AppExit>, sim: Res<Sim>, to: Res<RecordTo>) {
    if exits.read().next().is_some() {
        match sim.save_replay(&to.0) {
            Ok(()) => info!("replay saved to {} ({} ticks)", to.0, sim.world.tick()),
            Err(e) => error!("could not save replay to {}: {e}", to.0),
        }
    }
}

fn spawn_camera(mut commands: Commands, sim: Res<Sim>) {
    let home = sim.world.entities().find(|e| e.owner == Some(LOCAL)).map_or(Vec3::ZERO, |e| sim.to_world(e.pos));
    camera::spawn(&mut commands, home);
}

mod headless {
    //! Renders one frame without a window: advance the sim, then capture to PNG.
    //! Modelled on bevy's `examples/app/externally_driven_headless_renderer.rs`.

    use bevy::app::SubApps;
    use bevy::asset::RenderAssetUsages;
    use bevy::camera::RenderTarget;
    use bevy::prelude::*;
    use bevy::render::RenderPlugin;
    use bevy::render::render_resource::{PollType, TextureFormat};
    use bevy::render::renderer::RenderDevice;
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    use bevy::window::ExitCondition;
    use bevy::winit::WinitPlugin;

    use crate::camera::RtsCamera;
    use crate::sim::{LOCAL, Sim};

    #[derive(Clone, Copy)]
    pub enum View {
        /// The in-game camera over the local base: what a player sees.
        Base,
        /// The whole map from above, for checking layout.
        Map,
        /// The in-game camera over the latest shots (the local base if there are none).
        Battle,
    }

    pub fn run(mut sim: Sim, ticks: u32, path: String, view: View, size: (u32, u32)) -> AppExit {
        sim.speed = 0;
        // The last few ticks' tracers and blasts, plus every shell still in the air, drawn
        // where it is along its flight, so the frame shows the battle as it stands.
        let mut hits = crate::sim::Hits::default();
        for t in 0..ticks {
            sim.world.step();
            if t + 4 >= ticks {
                crate::sim::collect_shots(&sim.world, &mut hits);
            }
        }
        hits.0.retain(|s| !matches!(s, crate::sim::Shot::Shell { .. }));
        let now = sim.world.tick();
        for s in sim.world.shells() {
            let whole = (s.lands - s.fired).max(1);
            let left = s.lands.saturating_sub(now) + 1;
            let done = (now - s.fired) as f32 / whole as f32;
            hits.0.push(crate::sim::Shot::Shell { from: s.from, at: s.at, flight: left, done: done.min(0.95) });
        }
        // Midpoint of the last few ticks' shots (shooter to target), for `View::Battle`, so both
        // sides of the fight are in frame.
        let ends: Vec<(g2_sim::Vec2, g2_sim::Vec2)> = hits
            .0
            .iter()
            .map(|s| match *s {
                crate::sim::Shot::Hit { from, at, .. } | crate::sim::Shot::Shell { from, at, .. } => (from, at),
                crate::sim::Shot::Blast { at, .. } => (at, at),
            })
            .collect();
        let fight = (!ends.is_empty()).then(|| {
            let n = ends.len() as f32;
            let p = |v: g2_sim::Vec2| Vec2::new(v.x.to_f32(), v.y.to_f32());
            ends.iter().fold(Vec2::ZERO, |s, &(from, at)| s + (p(from) + p(at)) / 2.0) / n
        });
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin { primary_window: None, exit_condition: ExitCondition::DontExit, ..default() })
                .set(RenderPlugin { synchronous_pipeline_compilation: true, ..default() })
                .disable::<WinitPlugin>(),
        )
        .insert_resource(sim)
        .add_plugins((
            crate::sim::SimPlugin,
            crate::terrain::TerrainPlugin,
            crate::view::ViewPlugin,
            crate::hud::HudPlugin,
        ))
        .insert_resource(hits)
        // Warm-up frames run on wall time; frozen effects survive until the capture.
        .insert_resource(crate::view::FreezeEffects)
        .init_resource::<crate::input::Selection>()
        .init_resource::<crate::input::Placement>();
        app.finish();
        app.cleanup();
        let mut apps: SubApps = std::mem::take(app.sub_apps_mut());

        let world = apps.main.world_mut();
        let mut image = Image::new_target_texture(size.0, size.1, TextureFormat::Rgba8UnormSrgb, None);
        image.asset_usage = RenderAssetUsages::RENDER_WORLD;
        let target: RenderTarget = world.resource_mut::<Assets<Image>>().add(image).into();
        let cam = {
            let sim = world.resource::<Sim>();
            match view {
                View::Battle if let Some(f) = fight => RtsCamera {
                    pivot: Vec3::new(f.x, sim.ground_y(f.x, f.y), f.y),
                    distance: crate::camera::DEFAULT_DISTANCE,
                    yaw: crate::look::YAW,
                    pitch: crate::look::PITCH,
                },
                View::Base | View::Battle => {
                    // The command center, not the first own entity: depleted piles free low
                    // entity slots, which later units reuse.
                    let rules = sim.world.rules;
                    let mine = || sim.world.entities().filter(|e| e.owner == Some(LOCAL));
                    let home = mine()
                        .find(|e| rules.def(e.def).role() == Some(g2_sim::BuildingRole::CommandCenter))
                        .or_else(|| mine().next())
                        .map_or(Vec3::ZERO, |e| sim.to_world(e.pos));
                    RtsCamera {
                        pivot: home + Vec3::new(12.0, 0.0, 4.0),
                        distance: crate::camera::DEFAULT_DISTANCE,
                        yaw: crate::look::YAW,
                        pitch: crate::look::PITCH,
                    }
                }
                View::Map => {
                    let (w, h) = (sim.world.map.width.to_f32(), sim.world.map.height.to_f32());
                    RtsCamera {
                        pivot: Vec3::new(w / 2.0, sim.ground_y(w / 2.0, h / 2.0), h / 2.0),
                        distance: w.max(h) * 2.4,
                        yaw: crate::look::YAW,
                        pitch: crate::look::PITCH,
                    }
                }
            }
        };
        // An image-target camera is not picked as the UI camera automatically; the HUD must be in the shot.
        let mut camera = world.spawn((
            crate::look::camera_bundle(),
            target.clone(),
            cam.transform(),
            cam,
            bevy::ui::IsDefaultUiCamera,
        ));
        // The overview sits far beyond the haze distance tuned for play; fog would hide the whole map.
        if matches!(view, View::Map) {
            camera.remove::<bevy::pbr::DistanceFog>();
        }

        let wait = |apps: &mut SubApps| {
            apps.update();
            apps.main
                .world()
                .resource::<RenderDevice>()
                .wgpu_device()
                .poll(PollType::Wait { submission_index: None, timeout: None })
                .expect("GPU poll failed");
        };
        // Spawn mirrors and compile pipelines before capturing.
        for _ in 0..3 {
            wait(&mut apps);
        }
        apps.main
            .world_mut()
            .spawn(Screenshot::image(target.as_image().unwrap().clone()))
            .observe(save_to_disk(path.clone()));
        // The readback arrives over a channel a few frames later; software adapters take longer.
        for _ in 0..60 {
            wait(&mut apps);
            if std::path::Path::new(&path).exists() {
                break;
            }
        }
        if !std::path::Path::new(&path).exists() {
            eprintln!("screenshot was not written to {path}");
            return AppExit::error();
        }
        // Every sim entity must have exactly one mesh, at its sim position (ground plane x/z).
        // The only other meshes allowed are ghosts: remembered buildings destroyed out of sight.
        let world = apps.main.world_mut();
        let mirrored: Vec<(g2_sim::EntityId, Vec3)> = world
            .query::<(&crate::view::SimRef, &Transform)>()
            .iter(world)
            .map(|(r, tf)| (r.0, tf.translation))
            .collect();
        let client = world.resource::<Sim>();
        let sim = &client.world;
        let total = sim.entities().count();
        let misplaced = sim
            .entities()
            .filter(|e| {
                let want = crate::sim::to_world(e.pos);
                let hits: Vec<_> = mirrored.iter().filter(|(id, _)| *id == e.id).collect();
                hits.len() != 1 || Vec2::new(hits[0].1.x - want.x, hits[0].1.z - want.z).length() > 0.01
            })
            .count();
        let ghosts = mirrored.iter().filter(|(id, _)| sim.entity(*id).is_none()).count();
        let stray = mirrored.iter().filter(|(id, _)| sim.entity(*id).is_none() && !client.remembers(*id)).count();
        println!(
            "wrote {path}; sim entities {total}, models {}, ghosts {ghosts}, misplaced {misplaced}",
            mirrored.len()
        );
        if mirrored.len() != total + ghosts || misplaced > 0 || stray > 0 {
            eprintln!("render mirror does not match the simulation");
            return AppExit::error();
        }
        AppExit::Success
    }
}
