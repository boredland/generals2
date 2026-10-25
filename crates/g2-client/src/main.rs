//! Generals 2 client.
//!
//!   g2-client [--map NAME] [--faction eu|apa|gla] [--enemy eu|apa|gla] [--general NAME]
//!             [--enemy-general NAME] [--seed N] [--spectate] [--reveal] [--record FILE]
//!   g2-client --host PORT [match flags as above]      two players over the network: you host
//!   g2-client --join HOST:PORT [--reveal] [--record FILE]   ...and the other player joins
//!   g2-client --screenshot out.png [--ticks N] [--view base|map|battle|at:X,Z] [--select DEF] [--movie-frame N] [...]
//!             headless: simulate N ticks, render one frame
//!
//! `--ai easy|normal|onslaught` sets the AI level; `onslaught` makes the enemy the Onslaught
//! mode: no base, waves on a schedule, and you win by outlasting the last one.
//! `--map` takes "Crater Ridge" (built in, the default) or `<gen|zh>:<name>` for an original
//! Generals / Zero Hour map (`g2-tools maps` lists them). `--general` picks a general of your
//! faction (Bulldog, Taskmaster, Junkyard); without one you play the plain faction. `--spectate`
//! lets the AI play your side too (AI vs AI). `--reveal` draws the whole map without your fog
//! of war (the sim's fog still applies to both sides). `--record` saves the match as a replay
//! on exit; check it with `g2-tools verify FILE`. Screenshot mode needs a Vulkan adapter but no
//! window or display server.
//!
//! A network match is two humans, the host as player 0 with `--faction`/`--general`, the
//! joiner as player 1 with `--enemy`/`--enemy-general`, on the host's map and seed. Both run
//! the same build. The game runs at one shared speed: no pause and no fast-forward.
//!
//! With an imported 2013 alpha (`g2-tools alpha DIR import`, found in `$G2_ALPHA_CACHE` or
//! `~/.local/share/generals2/alpha`), the client draws its models and plays its sounds,
//! voices and menu music (`alpha.rs`); the match itself is the same either way.

mod alpha;
mod alpha_hud;
mod audio;
mod camera;
mod hud;
mod input;
mod look;
mod menu;
mod models;
mod movie;
mod net;
mod profile;
mod rig;
mod sim;
mod terrain;
mod utfwin;
mod view;

use bevy::prelude::*;
use g2_sim::{Faction, GeneralId, PlayerId, PlayerSetup};

use crate::sim::{Sim, SimPlugin};

struct Args {
    map: String,
    factions: [Faction; 2],
    generals: [Option<String>; 2],
    seed: u64,
    spectate: bool,
    /// Level of every AI player in the match.
    ai: g2_sim::AiLevel,
    /// A flag chose the match: skip the main menu and start playing.
    direct: bool,
    reveal: bool,
    /// Headless only: draw this menu screen over the battlefield.
    menu: Option<menu::Screen>,
    /// Headless only: freeze on the pause menu.
    paused: bool,
    /// Headless only: select the local player's first entity of this def.
    select: Option<String>,
    /// Headless only: show the imported intro movie, held at this frame.
    movie_frame: Option<usize>,
    screenshot: Option<String>,
    record: Option<String>,
    ticks: u32,
    view: headless::View,
    size: (u32, u32),
    /// Host a two-player network match on this port: the local player against whoever joins.
    host: Option<u16>,
    /// Join the network match hosted at this `host:port`.
    join: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        map: "Crater Ridge".to_owned(),
        factions: [Faction::Eu, Faction::Gla],
        generals: [None, None],
        seed: 1,
        spectate: false,
        ai: g2_sim::AiLevel::Normal,
        direct: false,
        reveal: false,
        menu: None,
        paused: false,
        select: None,
        movie_frame: None,
        screenshot: None,
        record: None,
        ticks: 900,
        view: headless::View::Base,
        size: (1280, 720),
        host: None,
        join: None,
    };
    let mut it = std::env::args().skip(1);
    let faction = |v: Option<String>| match v.as_deref() {
        Some("eu") => Ok(Faction::Eu),
        Some("apa") => Ok(Faction::Apa),
        Some("gla") => Ok(Faction::Gla),
        other => Err(format!("expected eu|apa|gla, got {other:?}")),
    };
    while let Some(flag) = it.next() {
        a.direct |= matches!(
            flag.as_str(),
            "--map"
                | "--faction"
                | "--enemy"
                | "--general"
                | "--enemy-general"
                | "--seed"
                | "--spectate"
                | "--host"
                | "--join"
        );
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
                    Some("onslaught") => g2_sim::AiLevel::Onslaught,
                    other => return Err(format!("--ai expects easy|normal|onslaught, got {other:?}")),
                }
            }
            "--reveal" => a.reveal = true,
            "--menu" => {
                a.menu = Some(match it.next().as_deref() {
                    Some("main") => menu::Screen::Main,
                    Some("controls") => menu::Screen::Controls,
                    Some("setup") => menu::Screen::Setup,
                    Some("pause") => {
                        a.paused = true;
                        menu::Screen::Match
                    }
                    Some("end") => menu::Screen::End,
                    Some("generals") => menu::Screen::Generals,
                    other => {
                        return Err(format!("--menu expects main|controls|setup|pause|end|generals, got {other:?}"));
                    }
                })
            }
            "--screenshot" => a.screenshot = Some(it.next().ok_or("--screenshot needs a path")?),
            "--record" => a.record = Some(it.next().ok_or("--record needs a path")?),
            "--ticks" => a.ticks = it.next().and_then(|v| v.parse().ok()).ok_or("--ticks needs a number")?,
            "--view" => {
                a.view = match it.next().as_deref() {
                    Some("base") => headless::View::Base,
                    Some("map") => headless::View::Map,
                    Some("battle") => headless::View::Battle,
                    Some(v) if let Some((x, z)) = v.strip_prefix("at:").and_then(|p| p.split_once(',')) => {
                        let n =
                            |s: &str| s.parse::<f32>().map_err(|_| format!("--view at:X,Z needs numbers, got {v:?}"));
                        headless::View::At(n(x)?, n(z)?)
                    }
                    other => return Err(format!("--view expects base|map|battle|at:X,Z, got {other:?}")),
                }
            }
            "--select" => a.select = Some(it.next().ok_or("--select needs a def name")?),
            "--movie-frame" => {
                a.movie_frame =
                    Some(it.next().and_then(|v| v.parse().ok()).ok_or("--movie-frame needs a frame number")?)
            }
            "--size" => {
                let v = it.next().ok_or("--size needs WxH")?;
                let (w, h) = v.split_once('x').ok_or("--size needs WxH")?;
                a.size = (w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?);
            }
            "--host" => a.host = Some(it.next().and_then(|v| v.parse().ok()).ok_or("--host needs a port")?),
            "--join" => a.join = Some(it.next().ok_or("--join needs host:port")?),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    if (a.host.is_some() || a.join.is_some()) && (a.spectate || a.screenshot.is_some()) {
        return Err("--host and --join play a match: no --spectate or --screenshot".into());
    }
    if a.host.is_some() && a.join.is_some() {
        return Err("--host or --join, not both".into());
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
    let loaded = match (&args.join, args.host) {
        (Some(addr), _) => join_sim(addr, args.reveal),
        (None, Some(port)) => load_sim(&args).and_then(|sim| host_sim(sim, port)),
        (None, None) => load_sim(&args),
    };
    let sim = match loaded {
        Ok(sim) => sim,
        Err(e) => {
            eprintln!("{e}");
            return AppExit::error();
        }
    };
    match args.screenshot {
        Some(path) => {
            let ui = menu::Ui { screen: args.menu.unwrap_or(menu::Screen::Match), paused: args.paused, ..default() };
            let capture = headless::Capture {
                ticks: args.ticks,
                path,
                view: args.view,
                size: args.size,
                select: args.select,
                movie_frame: args.movie_frame,
            };
            headless::run(sim, ui, capture)
        }
        None => windowed(sim, args.record, args.direct),
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
    let player = |i: usize, ai: Option<g2_sim::AiLevel>| -> Result<PlayerSetup, String> {
        let general = general(i)?;
        Ok(PlayerSetup {
            faction: args.factions[i],
            starting_credits: 5_000,
            ai,
            general,
            perks: ai.map_or_else(Vec::new, |level| g2_sim::ai::ai_perks(g2_rules::rules(), general, level)),
        })
    };
    // `--ai onslaught` makes the enemy the waves; a spectated player 0 is then `Normal`.
    let own = match args.ai {
        g2_sim::AiLevel::Onslaught => g2_sim::AiLevel::Normal,
        level => level,
    };
    // A hosted match is two humans: the joiner plays the enemy slot.
    let enemy = args.host.is_none().then_some(args.ai);
    let mut sim = Sim::new(map, source, args.seed, [player(0, spectate.then_some(own))?, player(1, enemy)?]);
    sim.fog = !args.reveal;
    Ok(sim)
}

/// Waits for a player to join, then plays the loaded match as player 0 against them.
fn host_sim(mut sim: Sim, port: u16) -> Result<Sim, String> {
    let link = net::host(port, sim.world.replay(&sim.setup))?;
    sim.online = Some(net::Online::new(link, PlayerId(0), PlayerId(1)));
    Ok(sim)
}

/// Joins the match hosted at `addr` as player 1. The match comes from the host; it must be on
/// a map this build has, unchanged, with generals this build knows.
fn join_sim(addr: &str, reveal: bool) -> Result<Sim, String> {
    let (link, setup) = net::join(addr)?;
    let source = if setup.map.contains(':') {
        Some(g2_maps::load_file_named(&g2_maps::vendored_root(), &setup.map)?)
    } else {
        None
    };
    let map = match &source {
        Some(file) => file.to_map_def(&setup.map),
        None => g2_maps::resolve(&setup.map)?,
    };
    setup.check(&map, g2_rules::rules())?;
    if setup.players.len() != 2 || setup.players.iter().any(|p| p.ai.is_some()) {
        return Err("the host offered a match that isn't two players against each other".into());
    }
    net::accept(&link);
    let mut sim = Sim::new(map, source, setup.seed, setup.players);
    sim.local = PlayerId(1);
    sim.fog = !reveal;
    sim.online = Some(net::Online::new(link, PlayerId(1), PlayerId(0)));
    Ok(sim)
}

fn windowed(mut sim: Sim, record: Option<String>, direct: bool) -> AppExit {
    // The menu covers a frozen battlefield; a flag-chosen match starts running at once.
    let mut ui = menu::Ui::default();
    if direct {
        ui.screen = menu::Screen::Match;
    } else {
        sim.speed = 0;
    }
    let mut app = App::new();
    alpha::register(&mut app);
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin { primary_window: Some(Window { title: "Open RTS".into(), ..default() }), ..default() }),
    )
    .insert_resource(sim)
    .insert_resource(ui)
    .add_plugins((
        SimPlugin,
        menu::MenuPlugin { blocking: false },
        terrain::TerrainPlugin,
        view::ViewPlugin,
        camera::CameraPlugin,
        input::InputPlugin,
        hud::HudPlugin,
        audio::SoundPlugin,
        movie::MoviePlugin,
        rig::RigPlugin,
    ))
    .add_systems(Startup, spawn_camera);
    if let Some(path) = record {
        app.insert_resource(sim::RecordTo(path)).add_systems(Last, sim::save_replay_on_exit);
    }
    app.run()
}

fn spawn_camera(mut commands: Commands, sim: Res<Sim>) {
    let home = sim.world.entities().find(|e| e.owner == Some(sim.local)).map_or(Vec3::ZERO, |e| sim.to_world(e.pos));
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

    #[derive(Clone, Copy)]
    pub enum View {
        /// The in-game camera over the local base: what a player sees.
        Base,
        /// The whole map from above, for checking layout.
        Map,
        /// The in-game camera over the latest shots (the local base if there are none).
        Battle,
        /// The in-game camera over world position `(x, z)`.
        At(f32, f32),
    }

    /// What to capture: the sim's state after `ticks`, seen through `view`, at `size`, saved
    /// to `path`; optionally with one entity selected or the intro movie on screen.
    pub struct Capture {
        pub ticks: u32,
        pub path: String,
        pub view: View,
        pub size: (u32, u32),
        pub select: Option<String>,
        pub movie_frame: Option<usize>,
    }

    pub fn run(mut sim: Sim, ui: crate::menu::Ui, capture: Capture) -> AppExit {
        let Capture { ticks, path, view, size, select, movie_frame } = capture;
        sim.speed = 0;
        // The capture is awaited by polling for the file, so an old one at `path` would be
        // taken for the new frame and nothing would be written.
        if let Err(e) = std::fs::remove_file(&path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("can't replace {path}: {e}");
            return AppExit::error();
        }
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
        crate::alpha::register(&mut app);
        app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin { primary_window: None, exit_condition: ExitCondition::DontExit, ..default() })
                .set(RenderPlugin { synchronous_pipeline_compilation: true, ..default() })
                .disable::<WinitPlugin>()
                // A screenshot needs no sound device.
                .disable::<bevy::audio::AudioPlugin>(),
        )
        .insert_resource(sim)
        .insert_resource(ui)
        .add_plugins((
            crate::sim::SimPlugin,
            crate::menu::MenuPlugin { blocking: true },
            crate::terrain::TerrainPlugin,
            crate::view::ViewPlugin,
            crate::hud::HudPlugin,
            crate::rig::RigPlugin,
        ))
        .insert_resource(hits)
        // Warm-up frames run on wall time; frozen effects survive until the capture.
        .insert_resource(crate::view::FreezeEffects)
        .insert_resource(crate::hud::HeadlessScreen(Vec2::new(size.0 as f32, size.1 as f32)))
        .init_resource::<crate::input::Placement>();
        let selected = select.as_deref().and_then(|name| {
            let world = &app.world().resource::<Sim>().world;
            let me = app.world().resource::<Sim>().local;
            world.entities().find(|e| e.owner == Some(me) && world.rules.def(e.def).name == name).map(|e| e.id)
        });
        if let (Some(name), None) = (&select, selected) {
            eprintln!("--select: no entity of the local player is a {name}");
            return AppExit::error();
        }
        app.insert_resource(crate::input::Selection(selected.into_iter().collect()));
        if let Some(frame) = movie_frame {
            let Some(cache) = app.world().get_resource::<crate::alpha::Cache>().cloned() else {
                eprintln!("--movie-frame: no imported alpha");
                return AppExit::error();
            };
            if !crate::movie::hold(&mut app, &cache, frame) {
                eprintln!("--movie-frame: the import has no intro frame {frame}");
                return AppExit::error();
            }
        }
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
                View::At(x, z) => RtsCamera {
                    pivot: Vec3::new(x, sim.ground_y(x, z), z),
                    distance: crate::camera::DEFAULT_DISTANCE,
                    yaw: crate::look::YAW,
                    pitch: crate::look::PITCH,
                },
                View::Base | View::Battle => {
                    // The command center, not the first own entity: depleted piles free low
                    // entity slots, which later units reuse.
                    let rules = sim.world.rules;
                    let mine = || sim.world.entities().filter(|e| e.owner == Some(sim.local));
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
        // Spawn mirrors and compile pipelines before capturing; with an imported alpha, also
        // wait (up to ~10 s of frames) until every model's scene has loaded, or the frame
        // would show bare team rings.
        for _ in 0..3 {
            wait(&mut apps);
        }
        for _ in 0..600 {
            if crate::view::alpha_models_loaded(apps.main.world()) && crate::movie::held_frame_loaded(apps.main.world())
            {
                break;
            }
            wait(&mut apps);
        }
        if !crate::view::alpha_models_loaded(apps.main.world()) {
            eprintln!("imported alpha models did not finish loading");
            return AppExit::error();
        }
        if !crate::movie::held_frame_loaded(apps.main.world()) {
            eprintln!("--movie-frame: the movie frame did not finish loading");
            return AppExit::error();
        }
        // Scenes spawn their meshes a frame or two after the load completes.
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
        // What the frame costs: every mesh drawn, and the average of 30 more full frames
        // (render, then wait for the GPU), so scenery and model detail can be budgeted.
        let meshes = apps.main.world_mut().query::<&Mesh3d>().iter(apps.main.world()).count();
        let start = std::time::Instant::now();
        for _ in 0..30 {
            wait(&mut apps);
        }
        let frame_ms = start.elapsed().as_secs_f64() * 1000.0 / 30.0;
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
            "wrote {path}; sim entities {total}, models {}, ghosts {ghosts}, misplaced {misplaced}; meshes {meshes}, {frame_ms:.1} ms/frame",
            mirrored.len()
        );
        if mirrored.len() != total + ghosts || misplaced > 0 || stray > 0 {
            eprintln!("render mirror does not match the simulation");
            return AppExit::error();
        }
        AppExit::Success
    }
}
