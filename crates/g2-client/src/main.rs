//! Generals 2 client.
//!
//!   g2-client [--map NAME] [--faction eu|apa|gla] [--enemy eu|apa|gla] [--general NAME]
//!             [--perks NAME,..] [--enemy-general NAME] [--seed N] [--spectate] [--reveal] [--record FILE]
//!   g2-client --host PORT [--coop] [match flags as above]   two players over the network: you host
//!   g2-client --join HOST:PORT [--reveal] [--record FILE]   ...and the other player joins
//!   g2-client [match flags] --teams 1122                    a team game: a team digit (0-4) per player
//!   g2-client --screenshot out.png [--ticks N] [--view base|map|battle|at:X,Z] [--camera closest|close|medium|far] [--select DEF] [--hover-slot N] [--movie-frame N] [--frames N] [--no-hud] [...]
//!             headless: simulate N ticks, render one frame (or, with --frames, a clip of frames
//!             to a path holding {}, e.g. clip/{}.png); `--hover-slot N` shows the tooltip of
//!             build-grid button N (from 0) as if hovered, for the `--select`ed building
//!
//! `--perks` slots perks of `--general`'s, by name, for you (the menu takes them from your
//! profile): e.g. `--general "Mercury Red" --perks "Strike Team"` shows the power it adds.
//!
//! `--mission N|KEY` plays a mission of the campaign (1-8, or its key, e.g. `first-light`) on
//! its own map and forces instead of a skirmish, at `--difficulty easy|normal|hard` (Normal by
//! default); with `--spectate` (or `--screenshot`) the Normal AI plays your side.
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
//! joiner as player 1 with `--enemy`/`--enemy-general`, on the host's map and seed: against
//! each other, or with `--coop` side by side against the Onslaught's waves (a third player of
//! the first faction neither of them plays, in the order EU, APA, GLA).
//! Both run the same build. The game runs at one shared speed: no pause and no fast-forward.
//! The host's router is asked to forward PORT (`portmap.rs`) and the addresses to join at are
//! printed; `--join` takes a name or an IPv4 or IPv6 address (`[::1]:8088`), 8088 if no port.
//! The main menu's NETWORK page (`--menu lobby`) sets the same matches up without flags.
//!
//! With an imported 2013 alpha (the main menu's 2013 Alpha page or `g2-tools alpha DIR import`, found in `$G2_ALPHA_CACHE` or the
//! game's data folder: `~/.local/share/generals2/alpha` on Linux), the client draws its models
//! and plays its sounds, voices and menu music (`alpha.rs`); the match itself is the same either way.

// A release build on Windows is a window, not a console program, so starting it from Explorer
// opens no console beside the game; its messages (headless runs included) are then not shown.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod alerts;
mod alpha;
mod alpha_hud;
mod audio;
mod birds;
mod camera;
mod feedback;
mod front;
mod fx;
mod hud;
mod importer;
mod input;
mod lobby;
mod look;
mod menu;
mod mission_hud;
mod models;
mod movie;
mod net;
mod portmap;
mod procanim;
mod profile;
mod rig;
mod select;
mod settings;
mod sim;
mod surface;
mod terrain;
mod tooltip;
mod utfwin;
mod view;

use bevy::prelude::*;
use g2_sim::{Faction, GeneralId, PlayerId, PlayerSetup};

use crate::sim::{Sim, SimPlugin};

struct Args {
    map: String,
    factions: [Faction; 2],
    generals: [Option<String>; 2],
    /// The local player's slotted perks, by name, from `--general`'s list (headless shots of
    /// what a perk adds; a played match takes them from the profile).
    perks: Vec<String>,
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
    /// Headless only: show the tooltip of this build-grid button, as if hovered.
    hover_slot: Option<usize>,
    /// Headless only: show the imported intro movie, held at this frame.
    movie_frame: Option<usize>,
    /// Headless only: a clip of this many frames, one sim tick each, instead of one shot.
    frames: Option<u32>,
    /// Headless only: leave the HUD out of the frame.
    no_hud: bool,
    /// Headless only: the battlefield camera the views use (else Close, whatever the player
    /// saved, so shots are the same on every machine), and whether to zoom in as far as the
    /// wheel goes (`--camera closest`: half of Close, where infantry poses can be read).
    battlefield: settings::Battlefield,
    closest: bool,
    screenshot: Option<String>,
    record: Option<String>,
    ticks: u32,
    view: headless::View,
    size: (u32, u32),
    /// Host a two-player network match on this port: the local player with whoever joins.
    host: Option<u16>,
    /// The hosted match is co-op Onslaught: host and joiner on one team against the waves.
    coop: bool,
    /// Join the network match hosted at this `host:port`.
    join: Option<String>,
    /// Play this mission of the campaign instead of a skirmish, at this difficulty.
    mission: Option<g2_sim::MissionId>,
    difficulty: g2_sim::mission::Difficulty,
    /// One team per player (0: none), as many players as digits: a team game with the skirmish
    /// page's teams. Players past the second play the next faction in turn, at the `--ai` level.
    teams: Option<Vec<u8>>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        map: "Crater Ridge".to_owned(),
        factions: [Faction::Eu, Faction::Gla],
        generals: [None, None],
        perks: Vec::new(),
        seed: 1,
        spectate: false,
        ai: g2_sim::AiLevel::Normal,
        direct: false,
        reveal: false,
        menu: None,
        paused: false,
        select: None,
        hover_slot: None,
        movie_frame: None,
        frames: None,
        no_hud: false,
        battlefield: settings::Battlefield::Close,
        closest: false,
        screenshot: None,
        record: None,
        ticks: 900,
        view: headless::View::Base,
        size: (1280, 720),
        host: None,
        coop: false,
        join: None,
        mission: None,
        difficulty: g2_sim::mission::Difficulty::Normal,
        teams: None,
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
                | "--mission"
                | "--coop"
                | "--teams"
        );
        match flag.as_str() {
            "--map" => a.map = it.next().ok_or("--map needs a name")?,
            "--faction" => a.factions[0] = faction(it.next())?,
            "--enemy" => a.factions[1] = faction(it.next())?,
            "--general" => a.generals[0] = Some(it.next().ok_or("--general needs a name")?),
            "--enemy-general" => a.generals[1] = Some(it.next().ok_or("--enemy-general needs a name")?),
            "--perks" => {
                a.perks = it.next().ok_or("--perks needs names")?.split(',').map(|s| s.trim().to_owned()).collect()
            }
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
                    Some("alpha") => menu::Screen::Alpha,
                    Some("options") => menu::Screen::Options,
                    Some("missions") => menu::Screen::Missions,
                    Some("lobby") => menu::Screen::Lobby,
                    Some("netgame") => menu::Screen::NetGame,
                    other => {
                        return Err(format!(
                            "--menu expects main|controls|setup|pause|end|generals|alpha|options|missions|lobby|netgame, got {other:?}"
                        ));
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
            "--hover-slot" => {
                a.hover_slot =
                    Some(it.next().and_then(|v| v.parse().ok()).ok_or("--hover-slot needs a grid button number")?)
            }
            "--movie-frame" => {
                a.movie_frame =
                    Some(it.next().and_then(|v| v.parse().ok()).ok_or("--movie-frame needs a frame number")?)
            }
            "--no-hud" => a.no_hud = true,
            "--camera" => {
                a.battlefield = match it.next().as_deref() {
                    Some("close") => settings::Battlefield::Close,
                    Some("closest") => {
                        a.closest = true;
                        settings::Battlefield::Close
                    }
                    Some("medium") => settings::Battlefield::Medium,
                    Some("far") => settings::Battlefield::Far,
                    other => return Err(format!("--camera expects closest|close|medium|far, got {other:?}")),
                }
            }
            "--frames" => a.frames = Some(it.next().and_then(|v| v.parse().ok()).ok_or("--frames needs a count")?),
            "--size" => {
                let v = it.next().ok_or("--size needs WxH")?;
                let (w, h) = v.split_once('x').ok_or("--size needs WxH")?;
                a.size = (w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?);
            }
            "--host" => a.host = Some(it.next().and_then(|v| v.parse().ok()).ok_or("--host needs a port")?),
            "--join" => a.join = Some(it.next().ok_or("--join needs host:port")?),
            "--mission" => {
                let v = it.next().ok_or("--mission needs a number or a key")?;
                let rules = g2_rules::rules();
                let id = match v.parse::<usize>() {
                    Ok(n) if (1..=rules.missions.len()).contains(&n) => g2_sim::MissionId(n as u16 - 1),
                    _ => rules.find_mission(&v).ok_or_else(|| {
                        let keys: Vec<&str> = rules.missions.iter().map(|m| m.key).collect();
                        format!("--mission expects 1-{} or one of {}, got {v:?}", rules.missions.len(), keys.join(", "))
                    })?,
                };
                a.mission = Some(id);
            }
            "--difficulty" => {
                let v = it.next().unwrap_or_default();
                a.difficulty = g2_sim::mission::Difficulty::from_name(&v)
                    .ok_or_else(|| format!("--difficulty expects easy, normal or hard, got {v:?}"))?;
            }
            "--coop" => a.coop = true,
            "--teams" => {
                let v = it.next().ok_or("--teams needs a team digit per player, e.g. 1122")?;
                let teams: Option<Vec<u8>> =
                    v.chars().map(|c| c.to_digit(10).filter(|&d| d <= 4).map(|d| d as u8)).collect();
                match teams {
                    Some(t) if t.len() >= 2 && t[0] != 0 && t.iter().all(|&x| x == t[0]) => {
                        return Err("--teams puts every player on one team: there is no one to fight".into());
                    }
                    Some(t) if t.len() >= 2 => a.teams = Some(t),
                    _ => return Err(format!("--teams expects a team (0-4) per player, two or more, got {v:?}")),
                }
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    if (a.host.is_some() || a.join.is_some()) && (a.spectate || a.screenshot.is_some()) {
        return Err("--host and --join play a match: no --spectate or --screenshot".into());
    }
    if a.host.is_some() && a.join.is_some() {
        return Err("--host or --join, not both".into());
    }
    if a.mission.is_some() && (a.host.is_some() || a.join.is_some()) {
        return Err("--mission is played alone: no --host or --join".into());
    }
    if a.teams.is_some() && (a.coop || a.host.is_some() || a.join.is_some() || a.mission.is_some()) {
        return Err("--teams sets a skirmish: no --coop, --host, --join or --mission".into());
    }
    // Headless, `--coop` sets the hosted game the `--menu netgame` page shows.
    if a.coop && a.host.is_none() && a.screenshot.is_none() {
        return Err("--coop plays a hosted match: give --host PORT too".into());
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
            let mut ui = menu::Ui {
                screen: args.menu.unwrap_or(menu::Screen::Match),
                paused: args.paused,
                mission: args.mission.unwrap_or(g2_sim::MissionId(0)),
                difficulty: args.difficulty,
                in_mission: args.mission.is_some(),
                ..default()
            };
            // The skirmish page shows the match the flags set up, the local seat as the player's.
            if args.mission.is_none() {
                ui.config.map = args.map.clone();
                ui.config.seed = args.seed;
                ui.config.slots = (sim.setup.players.iter().enumerate())
                    .map(|(i, p)| menu::Slot {
                        faction: p.faction,
                        general: p.general,
                        ai: p.ai.filter(|_| i != 0),
                        team: p.team,
                    })
                    .collect();
            }
            // The lobby pages show a live session: the lobby looking on this network, the game
            // options a game hosted here (on any free port, unlisted) with its seat open, and
            // the router's answer to forwarding that port.
            let net = match ui.screen {
                menu::Screen::Lobby => lobby::Net::Browsing(lobby::Discovery::lan()),
                menu::Screen::NetGame => {
                    let offer = lobby::Offer {
                        map: args.map.clone(),
                        coop: args.coop,
                        seed: args.seed,
                        own: lobby::Pick::plain(args.factions[0]),
                    };
                    match lobby::Host::open(0, None, ui.lobby.name.clone(), offer, portmap::Router::lan()) {
                        Ok(mut h) => {
                            h.wait_for_router(std::time::Duration::from_secs(4));
                            lobby::Net::Hosting(Box::new(h))
                        }
                        Err(e) => {
                            eprintln!("{e}");
                            return AppExit::error();
                        }
                    }
                }
                _ => lobby::Net::Idle,
            };
            let capture = headless::Capture {
                ticks: args.ticks,
                path,
                view: args.view,
                size: args.size,
                select: args.select,
                hover_slot: args.hover_slot,
                movie_frame: args.movie_frame,
                frames: args.frames,
                no_hud: args.no_hud,
                battlefield: args.battlefield,
                closest: args.closest,
            };
            headless::run(sim, ui, net, capture)
        }
        None => windowed(sim, args.record, args.direct, args.mission.map(|m| (m, args.difficulty))),
    }
}

fn load_sim(args: &Args) -> Result<Sim, String> {
    let spectate = args.spectate || args.screenshot.is_some();
    if let Some(id) = args.mission {
        let mut sim = Sim::mission(id, args.difficulty, spectate.then_some(g2_sim::AiLevel::Normal), args.seed)?;
        sim.fog = !args.reveal;
        return Ok(sim);
    }
    let (map, source) = net::resolve_map(&args.map)?;
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
        let mut perks = ai.map_or_else(Vec::new, |level| g2_sim::ai::ai_perks(g2_rules::rules(), general, level));
        if i == 0 && !args.perks.is_empty() {
            let g = general.ok_or("--perks needs --general")?;
            perks = args
                .perks
                .iter()
                .map(|name| {
                    let own = rules.general(g).perks.iter().copied();
                    own.into_iter()
                        .find(|&k| rules.perk(k).name.eq_ignore_ascii_case(name))
                        .ok_or_else(|| format!("{} has no perk {name:?}", rules.general(g).name))
                })
                .collect::<Result<_, String>>()?;
        }
        Ok(PlayerSetup {
            faction: args.factions[i],
            starting_credits: 5_000,
            ai,
            general,
            perks,
            rank: ai.map_or(1, g2_sim::ai::ai_rank),
            team: 0,
        })
    };
    // `--ai onslaught` makes the enemy the waves; a spectated player 0 is then `Normal`.
    let own = match args.ai {
        g2_sim::AiLevel::Onslaught => g2_sim::AiLevel::Normal,
        level => level,
    };
    // A hosted match is two humans: the joiner plays the enemy slot, or in co-op the second
    // defender beside the host, the waves a third player.
    let enemy = args.host.is_none().then_some(args.ai);
    let mut players = vec![player(0, spectate.then_some(own))?, player(1, enemy)?];
    if args.coop {
        players[0].team = net::COOP_TEAM;
        players[1].team = net::COOP_TEAM;
        players.push(net::waves(net::wave_faction(args.factions)));
    }
    if let Some(teams) = &args.teams {
        if map.spawns.len() < teams.len() {
            return Err(format!("{} has {} starts for {} players", args.map, map.spawns.len(), teams.len()));
        }
        for i in 2..teams.len() {
            let faction = g2_sim::Faction::ALL[(args.factions[1] as usize + i - 1) % g2_sim::Faction::ALL.len()];
            let perks = g2_sim::ai::ai_perks(g2_rules::rules(), None, args.ai);
            let (ai, rank) = (Some(args.ai), g2_sim::ai::ai_rank(args.ai));
            players.push(PlayerSetup { faction, starting_credits: 5_000, ai, general: None, perks, rank, team: 0 });
        }
        for (p, &team) in players.iter_mut().zip(teams) {
            p.team = team;
        }
    }
    let mut sim = Sim::new(map, source, args.seed, players);
    sim.fog = !args.reveal;
    Ok(sim)
}

/// Waits for a player to join, then plays the loaded match as player 0 with them. The router
/// is asked to forward the port first, and the forward kept until the match ends.
fn host_sim(mut sim: Sim, port: u16) -> Result<Sim, String> {
    let (link, forward) = net::host(port, sim.world.replay(&sim.setup), portmap::Router::lan())?;
    sim.online = Some(net::Online::new(link, PlayerId(0), PlayerId(1)).with_forward(forward));
    Ok(sim)
}

/// Joins the match hosted at `addr` as player 1. The match comes from the host; it must be on
/// a map this build has, unchanged, with generals this build knows (`net::match_sim`).
fn join_sim(addr: &str, reveal: bool) -> Result<Sim, String> {
    let (link, setup) = net::join(addr)?;
    let mut sim = net::match_sim(&setup, PlayerId(1))?;
    net::accept(&link);
    sim.fog = !reveal;
    sim.online = Some(net::Online::new(link, PlayerId(1), PlayerId(0)));
    Ok(sim)
}

fn windowed(
    mut sim: Sim,
    record: Option<String>,
    direct: bool,
    mission: Option<(g2_sim::MissionId, g2_sim::mission::Difficulty)>,
) -> AppExit {
    // The menu covers a frozen battlefield; a flag-chosen match starts running at once.
    let mut ui = menu::Ui { in_mission: mission.is_some(), ..default() };
    if let Some((id, d)) = mission {
        (ui.mission, ui.difficulty) = (id, d);
    }
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
        (tooltip::TooltipPlugin, alerts::AlertsPlugin, mission_hud::MissionHudPlugin),
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

fn spawn_camera(mut commands: Commands, sim: Res<Sim>, settings: Res<settings::Settings>) {
    let home = sim.world.entities().find(|e| e.owner == Some(sim.local)).map_or(Vec3::ZERO, |e| sim.to_world(e.pos));
    camera::spawn(&mut commands, home, settings.camera.distance());
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
    /// to `path`; optionally with one entity selected or the intro movie on screen. With
    /// `frames`, it is a clip instead: that many frames, one sim tick each (30 per second of
    /// play), saved to `path` with its `{}` replaced by the frame number; with `View::Battle`,
    /// the camera easing toward the fighting.
    pub struct Capture {
        pub ticks: u32,
        pub path: String,
        pub view: View,
        pub size: (u32, u32),
        pub select: Option<String>,
        pub hover_slot: Option<usize>,
        pub movie_frame: Option<usize>,
        pub frames: Option<u32>,
        pub no_hud: bool,
        pub battlefield: crate::settings::Battlefield,
        pub closest: bool,
    }

    pub fn run(mut sim: Sim, ui: crate::menu::Ui, net: crate::lobby::Net, capture: Capture) -> AppExit {
        let Capture { ticks, path, view, size, select, hover_slot, movie_frame, frames, no_hud, battlefield, closest } =
            capture;
        // The wheel zooms in to half the default view (`camera.rs`).
        let distance = battlefield.distance() * if closest { 0.5 } else { 1.0 };
        sim.speed = 0;
        if frames.is_some() && !path.contains("{}") {
            eprintln!("--frames needs a --screenshot path with {{}} for the frame number");
            return AppExit::error();
        }
        // The capture is awaited by polling for the file, so an old one at `path` would be
        // taken for the new frame and nothing would be written.
        if let Err(e) = std::fs::remove_file(&path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("can't replace {path}: {e}");
            return AppExit::error();
        }
        // The last few ticks' tracers and blasts, plus every shell still in the air, drawn
        // where it is along its flight, so the frame shows the battle as it stands. Blasts and
        // kills before those ticks come along with their age, for the craters, wrecks and smoke
        // they leave (`fx::Aftermath`). Alerts are gathered throughout, as in play, so the frame
        // shows the ones still on screen.
        let mut hits = crate::sim::Hits::default();
        let mut alerts = crate::alerts::Alerts::default();
        let mut aftermath = crate::fx::Aftermath::default();
        let reach = (crate::fx::AFTERMATH_SECS * g2_sim::TICKS_PER_SECOND as f32) as u32;
        for t in 0..ticks {
            sim.world.step();
            alerts.observe(&sim.world, sim.local);
            if t + 4 >= ticks {
                crate::sim::collect_shots(&sim.world, &mut hits);
            } else if t + reach >= ticks {
                let mut past = crate::sim::Hits::default();
                crate::sim::collect_shots(&sim.world, &mut past);
                let age = (ticks - t) as f32 / g2_sim::TICKS_PER_SECOND as f32;
                aftermath.0.extend(past.0.into_iter().filter_map(|s| match s {
                    crate::sim::Shot::Blast { .. } | crate::sim::Shot::Hit { killed: true, .. } => Some((s, age)),
                    _ => None,
                }));
            }
        }
        hits.0.retain(|s| !matches!(s, crate::sim::Shot::Shell { .. }));
        let now = sim.world.tick();
        for s in sim.world.shells() {
            let whole = (s.lands - s.fired).max(1);
            let left = s.lands.saturating_sub(now) + 1;
            let done = (now - s.fired) as f32 / whole as f32;
            let look = crate::fx::Look::of_shell(&sim.world, s.source);
            hits.0.push(crate::sim::Shot::Shell { from: s.from, at: s.at, flight: left, done: done.min(0.95), look });
        }
        // Midpoint of the last few ticks' shots (shooter to target), for `View::Battle`, so both
        // sides of the fight are in frame.
        let ends: Vec<(g2_sim::Vec2, g2_sim::Vec2)> = hits
            .0
            .iter()
            .map(|s| match *s {
                crate::sim::Shot::Hit { from, at, .. } | crate::sim::Shot::Shell { from, at, .. } => (from, at),
                crate::sim::Shot::Blast { at, .. } | crate::sim::Shot::DigIn { at } => (at, at),
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
        .insert_resource(net)
        // Shots look the same on every machine: the player's saved settings stay out of them.
        .insert_resource(crate::settings::Settings::default())
        .add_plugins((
            crate::sim::SimPlugin,
            crate::menu::MenuPlugin { blocking: true },
            crate::terrain::TerrainPlugin,
            crate::view::ViewPlugin,
            crate::hud::HudPlugin,
            crate::tooltip::TooltipPlugin,
            crate::alerts::AlertsPlugin,
            crate::mission_hud::MissionHudPlugin,
            crate::rig::RigPlugin,
        ))
        .insert_resource(hits)
        .insert_resource(alerts)
        .insert_resource(aftermath)
        // Warm-up frames run on wall time; frozen effects survive until the capture.
        .insert_resource(crate::view::FreezeEffects)
        .insert_resource(crate::hud::HeadlessScreen(Vec2::new(size.0 as f32, size.1 as f32)))
        .add_systems(Update, crate::camera::fit_view)
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
        if let Some(n) = hover_slot {
            app.insert_resource(crate::tooltip::HoverSlot(n));
        }
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
                    distance,
                    yaw: crate::look::YAW,
                    pitch: crate::look::PITCH,
                },
                View::At(x, z) => RtsCamera {
                    pivot: Vec3::new(x, sim.ground_y(x, z), z),
                    distance,
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
                        distance,
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
        // An image-target camera is not picked as the UI camera automatically; the HUD must be in
        // the shot, unless asked to leave it out.
        let mut camera = world.spawn((crate::look::camera_bundle(), target.clone(), cam.transform(), cam));
        if !no_hud {
            camera.insert(bevy::ui::IsDefaultUiCamera);
        }
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
        // wait (up to ~10 s of frames) until every model's scene and clip has loaded, or the
        // frame would show bare team rings or soldiers in their bind pose.
        for _ in 0..3 {
            wait(&mut apps);
        }
        let loaded = |apps: &SubApps| {
            let w = apps.main.world();
            crate::view::alpha_models_loaded(w) && crate::rig::clips_loaded(w)
        };
        for _ in 0..600 {
            if loaded(&apps) && crate::movie::held_frame_loaded(apps.main.world()) {
                break;
            }
            wait(&mut apps);
        }
        if !loaded(&apps) {
            eprintln!("imported alpha models or clips did not finish loading");
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
        if let Some(n) = frames {
            return clip(apps, &target, &path, n, matches!(view, View::Battle));
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
        // A game options shot hosted a game and asked the router to forward its port: the
        // forward goes now, before the process exits (a thread would die with it).
        if let Some(crate::lobby::Net::Hosting(h)) =
            apps.main.world_mut().get_resource_mut::<crate::lobby::Net>().as_deref_mut()
        {
            h.drop_forward();
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

    /// Plays `n` frames from where the warm-up left the match, one sim tick a frame, saving
    /// each to `pattern`. Effects age and the sim steps on the fixed clock, so the clip runs
    /// at real speed whatever the frame costs. With `follow`, the camera eases toward the
    /// midpoint of the last two seconds' shots.
    fn clip(mut apps: SubApps, target: &RenderTarget, pattern: &str, n: u32, follow: bool) -> AppExit {
        use bevy::time::TimeUpdateStrategy;
        let wait = |apps: &mut SubApps| {
            apps.update();
            apps.main
                .world()
                .resource::<RenderDevice>()
                .wgpu_device()
                .poll(PollType::Wait { submission_index: None, timeout: None })
                .expect("GPU poll failed");
        };
        let world = apps.main.world_mut();
        world.remove_resource::<crate::view::FreezeEffects>();
        world.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
        world.resource_mut::<Sim>().speed = 1;
        let mut recent: std::collections::VecDeque<(u32, Vec2)> = Default::default();
        let names: Vec<String> = (0..n).map(|i| pattern.replace("{}", &format!("{i:05}"))).collect();
        for name in &names {
            if let Err(e) = std::fs::remove_file(name)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                eprintln!("can't replace {name}: {e}");
                return AppExit::error();
            }
        }
        for name in &names {
            wait(&mut apps);
            let world = apps.main.world_mut();
            if follow {
                let sim = world.resource::<Sim>();
                let tick = sim.world.tick();
                for e in sim.world.events() {
                    if let g2_sim::world::Event::Hit { at, .. } | g2_sim::world::Event::Landed { at, .. } = *e {
                        recent.push_back((tick, Vec2::new(at.x.to_f32(), at.y.to_f32())));
                    }
                }
                while recent.front().is_some_and(|&(t, _)| t + 2 * g2_sim::TICKS_PER_SECOND < tick) {
                    recent.pop_front();
                }
                if !recent.is_empty() {
                    let goal = recent.iter().map(|&(_, p)| p).sum::<Vec2>() / recent.len() as f32;
                    let ground = sim.ground_y(goal.x, goal.y);
                    let mut q = world.query::<(&mut RtsCamera, &mut Transform)>();
                    for (mut cam, mut tf) in q.iter_mut(world) {
                        // About 1.5 s to cover most of a jump, so the view drifts rather than cuts.
                        cam.pivot = cam.pivot.lerp(Vec3::new(goal.x, ground, goal.y), 0.02);
                        *tf = cam.transform();
                    }
                }
            }
            world.spawn(Screenshot::image(target.as_image().unwrap().clone())).observe(save_to_disk(name.clone()));
        }
        // Readbacks land a few frames after their request; keep rendering until the last is out.
        for _ in 0..120 {
            if names.iter().all(|f| std::path::Path::new(f).exists()) {
                break;
            }
            wait(&mut apps);
        }
        let missing = names.iter().filter(|f| !std::path::Path::new(f).exists()).count();
        if missing > 0 {
            eprintln!("{missing} of {n} frames were not written");
            return AppExit::error();
        }
        let sim = apps.main.world().resource::<Sim>();
        println!("wrote {n} frames to {pattern}, ticks to {}", sim.world.tick());
        AppExit::Success
    }
}
