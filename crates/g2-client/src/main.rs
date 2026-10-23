//! Generals 2 client.
//!
//!   g2-client [--faction eu|apa|gla] [--enemy eu|apa|gla] [--seed N] [--spectate] [--record FILE]
//!   g2-client --screenshot out.png [--ticks N] [...]   headless: simulate N ticks, render one frame
//!
//! `--spectate` lets the AI play your side too (AI vs AI). `--record` saves the match as a replay
//! on exit; check it with `g2-tools verify FILE`. Screenshot mode needs a Vulkan adapter but no
//! window or display server.

mod camera;
mod hud;
mod input;
mod sim;
mod view;

use bevy::prelude::*;
use g2_sim::Faction;

use crate::sim::{LOCAL, Sim, SimPlugin, to_world};

struct Args {
    factions: [Faction; 2],
    seed: u64,
    spectate: bool,
    screenshot: Option<String>,
    record: Option<String>,
    ticks: u32,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        factions: [Faction::Eu, Faction::Gla],
        seed: 1,
        spectate: false,
        screenshot: None,
        record: None,
        ticks: 900,
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
            "--faction" => a.factions[0] = faction(it.next())?,
            "--enemy" => a.factions[1] = faction(it.next())?,
            "--seed" => a.seed = it.next().and_then(|v| v.parse().ok()).ok_or("--seed needs a number")?,
            "--spectate" => a.spectate = true,
            "--screenshot" => a.screenshot = Some(it.next().ok_or("--screenshot needs a path")?),
            "--record" => a.record = Some(it.next().ok_or("--record needs a path")?),
            "--ticks" => a.ticks = it.next().and_then(|v| v.parse().ok()).ok_or("--ticks needs a number")?,
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
    let sim = Sim::new(args.seed, args.factions, args.spectate || args.screenshot.is_some());
    match args.screenshot {
        Some(path) => headless::run(sim, args.ticks, path),
        None => windowed(sim, args.record),
    }
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
    .add_plugins((SimPlugin, view::ViewPlugin, camera::CameraPlugin, input::InputPlugin, hud::HudPlugin))
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
    let home = sim.world.entities().find(|e| e.owner == Some(LOCAL)).map_or(Vec3::ZERO, |e| to_world(e.pos));
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
    use crate::sim::Sim;

    const SIZE: (u32, u32) = (1280, 720);

    pub fn run(mut sim: Sim, ticks: u32, path: String) -> AppExit {
        sim.speed = 0;
        for _ in 0..ticks {
            sim.world.step();
        }
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin { primary_window: None, exit_condition: ExitCondition::DontExit, ..default() })
                .set(RenderPlugin { synchronous_pipeline_compilation: true, ..default() })
                .disable::<WinitPlugin>(),
        )
        .insert_resource(sim)
        .add_plugins((crate::sim::SimPlugin, crate::view::ViewPlugin, crate::hud::HudPlugin))
        .init_resource::<crate::input::Selection>()
        .init_resource::<crate::input::Placement>();
        app.finish();
        app.cleanup();
        let mut apps: SubApps = std::mem::take(app.sub_apps_mut());

        let world = apps.main.world_mut();
        let mut image = Image::new_target_texture(SIZE.0, SIZE.1, TextureFormat::Rgba8UnormSrgb, None);
        image.asset_usage = RenderAssetUsages::RENDER_WORLD;
        let target: RenderTarget = world.resource_mut::<Assets<Image>>().add(image).into();
        // Frame the whole map so both bases, piles and wells are in one verification shot.
        let (pivot, span) = {
            let map = &world.resource::<Sim>().world.map;
            let (w, h) = (map.width.to_f32(), map.height.to_f32());
            (Vec3::new(w / 2.0, 0.0, h / 2.0), w.max(h))
        };
        let cam = RtsCamera { pivot, distance: span * 1.05, yaw: 0.0, pitch: 1.05 };
        // An image-target camera is not picked as the UI camera automatically; the HUD must be in the shot.
        world.spawn((Camera3d::default(), target.clone(), cam.transform(), cam, bevy::ui::IsDefaultUiCamera));

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
        let world = apps.main.world_mut();
        let mirrored: Vec<(g2_sim::EntityId, Vec3)> = world
            .query::<(&crate::view::SimRef, &Transform)>()
            .iter(world)
            .map(|(r, tf)| (r.0, tf.translation))
            .collect();
        let sim = &world.resource::<Sim>().world;
        let total = sim.entities().count();
        let misplaced = sim
            .entities()
            .filter(|e| {
                let want = crate::sim::to_world(e.pos);
                let hits: Vec<_> = mirrored.iter().filter(|(id, _)| *id == e.id).collect();
                hits.len() != 1 || Vec2::new(hits[0].1.x - want.x, hits[0].1.z - want.z).length() > 0.01
            })
            .count();
        println!("wrote {path}; sim entities {total}, meshes {}, misplaced {misplaced}", mirrored.len());
        if mirrored.len() != total || misplaced > 0 {
            eprintln!("render mirror does not match the simulation");
            return AppExit::error();
        }
        AppExit::Success
    }
}
