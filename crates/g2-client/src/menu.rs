//! Menus: main menu, controls, skirmish page, missions and their briefings, the network lobby's
//! pages (their sessions are in `lobby.rs`), in-match pause and the score screen, laid out as
//! Command & Conquer: Generals' own shell (docs/STYLE.md, "Menus").
//!
//! The menu is UI drawn over the battlefield: the `Sim` resource always exists and is frozen
//! (`speed = 0`) while a menu covers it, so opening or closing a menu needs no scene teardown.
//! Starting a match swaps the world instead: `MatchEntity` spawns are despawned, a fresh `Sim`
//! replaces the old, and the `MatchEpoch` bump makes every scene system respawn in the same
//! frame (they run after `MenuSet`, so nothing renders between teardown and rebuild).
//!
//! UI state is one `Ui` resource. Clicks and keys become `Act`s; the exclusive `apply` system
//! carries them out; `rebuild` redraws whichever screen `Ui` now names. Buttons are plain UI
//! nodes with `RelativeCursorPosition` (kept current by `bevy_ui`'s focus system), so no
//! picking backend is involved. The official-map catalogue parses off-thread, so the setup
//! screen shows a placeholder until it lands.

use std::sync::mpsc;

use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use g2_sim::defs::PerkId;
use g2_sim::{AiLevel, CommandKind, Faction, GeneralId, PlayerId, PlayerSetup};

use crate::hud::clock;
use g2_sim::ai::{ai_perks, ai_rank};

use crate::profile::{MAX_RANK, Profile, Refused, match_experience, promotion_cost};
use crate::settings::Settings;
use crate::sim::Sim;

/// Spawns that belong to the running match (scene, models, HUD, camera). `swap_world`
/// despawns them and `scene_start` respawns them from the new `Sim`; `despawn` takes their
/// children with them.
#[derive(Component)]
pub struct MatchEntity;

/// Spawns that belong to the menu overlay. Despawned and rebuilt whenever `Ui` changes.
#[derive(Component)]
pub struct MenuUi;

/// Bumped on every world swap; `scene_start` runs the scene systems once per bump.
#[derive(Resource, Default)]
pub struct MatchEpoch(pub u64);

/// Queued menu actions, drained by `apply`.
#[derive(Resource, Default)]
pub struct Pending(pub Vec<Act>);

/// Menu systems run as one ordered group; scene systems respawn after it, in the same frame.
#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub struct MenuSet;

/// Every menu click and key press, as one value `apply` can carry out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    ShowControls,
    /// The main menu's SKIRMISH: the skirmish page, with Onslaught AIs back to Normal.
    OpenSetup,
    BackToMain,
    QuitGame,
    Start,
    ChangeSetup,
    MapPage(i32),
    MapPick(usize),
    /// A skirmish row's Player cell: an AI steps Easy, Normal, Onslaught, then the last row
    /// closes (two players always stay); an Open row adds an AI player.
    CyclePlayer(usize),
    /// A skirmish row's Faction cell: the next faction, or general of it, as the LAN page's.
    CycleSide(usize),
    /// A skirmish row's Team cell: no team, then teams 1 to 4.
    CycleTeam(usize),
    /// Opens the map selection over the skirmish page, or closes it keeping the pick (ACCEPT).
    ToggleMapList,
    /// Closes the map selection and restores the map from before it opened (its BACK).
    CancelMapList,
    /// The main menu's ONSLAUGHT: the skirmish page with every AI playing Onslaught.
    OpenOnslaught,
    CycleCredits,
    ToggleFog,
    NextSeed,
    Pause,
    Resume,
    Surrender,
    QuitToMenu,
    /// The Generals screen, on the general it shows.
    OpenGenerals,
    CycleShownGeneral(i32),
    Promote,
    BuyPerk(PerkId),
    SlotPerk(PerkId),
    UnslotPerk(PerkId),
    /// The alpha page: importing the player's copy of the 2013 alpha.
    OpenAlpha,
    /// Opens the file dialog to find the alpha's `CnC.exe`, then imports it.
    ImportAlpha,
    /// The options page, from the main menu or the in-game menu (it returns to either).
    OpenOptions,
    /// An options cell steps its setting; changes apply at once and ACCEPT saves them.
    CycleOption(Opt),
    /// DEFAULTS: every option back to its default (not saved until ACCEPT).
    DefaultOptions,
    /// ACCEPT: save the options and go back.
    AcceptOptions,
    /// BACK: drop the changes made on the page and go back.
    CancelOptions,
    /// The main menu's MISSIONS: the campaign page.
    OpenMissions,
    /// A mission in the campaign list: its briefing shows beside it.
    PickMission(u16),
    /// START MISSION: plays the picked mission.
    StartMission,
    /// The main menu's NETWORK: the LAN lobby, listing games on the local network.
    OpenLobby,
    /// The lobby's BACK: stop looking for games, back to the main menu.
    LeaveLobby,
    /// Starts typing into a lobby text field, or stops (`None`).
    Type(Option<crate::lobby::Entry>),
    /// Selects a game of the lobby's list; its info shows beside it.
    PickGame(usize),
    /// HOST GAME: opens a game on the port field's port, for one player to join.
    HostGame,
    /// JOIN GAME: joins the game picked in the list.
    JoinGame,
    /// DIRECT CONNECT: joins the game at the address field's `host:port`.
    DirectConnect,
    /// The game options' BACK: leave the game (closing it, for the host), back to the lobby.
    LeaveGame,
    /// Host: versus or co-op Onslaught.
    NetMode,
    /// Host: the next (or previous) map with two starts or more.
    NetMap(i32),
    /// The local player's side: the next faction, or general of it, as Zero Hour's list.
    NetSide,
    /// Joiner: accept the game as it stands, or take the accept back.
    NetAccept,
    /// Host: start the accepted game.
    NetStart,
}

/// One setting on the options page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opt {
    Camera,
    Scheme,
    Pan,
    AlternatePan,
    EdgePan,
    LockMouse,
    PanSpeed,
    ZoomSpeed,
    ZoomModifier,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Screen {
    #[default]
    Main,
    Controls,
    Setup,
    Match,
    End,
    Generals,
    Alpha,
    Options,
    /// The campaign: missions down the left, the picked one's briefing and objectives.
    Missions,
    /// Generals' LAN lobby: the games on the local network, hosting and joining.
    Lobby,
    /// Generals' LAN game options: the hosted or joined game's seats, map and mode.
    NetGame,
}

/// One skirmish player: slot 0 is the human at the keyboard, the rest are AI.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Slot {
    pub faction: Faction,
    pub general: Option<GeneralId>,
    pub ai: Option<AiLevel>,
    /// `PlayerSetup::team`: 0 alone, else allied with every slot of the same team.
    pub team: u8,
}

/// Everything the setup screen edits, and everything a match needs to start.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SkirmishConfig {
    pub map: String,
    pub slots: Vec<Slot>,
    pub credits: i64,
    pub fog: bool,
    pub seed: u64,
}

impl Default for SkirmishConfig {
    fn default() -> Self {
        SkirmishConfig {
            map: "Crater Ridge".to_owned(),
            slots: vec![
                Slot { faction: Faction::Eu, general: None, ai: None, team: 0 },
                Slot { faction: Faction::Gla, general: None, ai: Some(AiLevel::Normal), team: 0 },
            ],
            credits: 5_000,
            fog: true,
            seed: 1,
        }
    }
}

impl SkirmishConfig {
    /// Builds the match this config describes. Slot 0 is the human player (`Sim::local`), who
    /// plays their general at `rank` with `perks` slotted (from their profile); AI slots pick
    /// their own. Refused when every player is on one team: there would be no one to fight.
    pub fn build(&self, perks: &[PerkId], rank: u8) -> Result<Sim, String> {
        if self.slots[0].team != 0 && self.slots.iter().all(|s| s.team == self.slots[0].team) {
            return Err("Every player is on one team: there is no one to fight.".to_owned());
        }
        let source = if self.map.contains(':') {
            Some(g2_maps::load_file_named(&g2_maps::vendored_root(), &self.map)?)
        } else {
            None
        };
        let map = match &source {
            Some(file) => file.to_map_def(&self.map),
            None => g2_maps::resolve(&self.map)?,
        };
        if map.spawns.len() < self.slots.len() {
            return Err(format!("{} has {} starts for {} players", self.map, map.spawns.len(), self.slots.len()));
        }
        let players: Vec<PlayerSetup> = self
            .slots
            .iter()
            .enumerate()
            .map(|(i, s)| PlayerSetup {
                faction: s.faction,
                starting_credits: self.credits,
                ai: s.ai,
                general: s.general,
                perks: match s.ai {
                    Some(level) => ai_perks(g2_rules::rules(), s.general, level),
                    None if i == 0 && s.general.is_some() => perks.to_vec(),
                    None => Vec::new(),
                },
                rank: match s.ai {
                    Some(level) => ai_rank(level),
                    None if i == 0 && s.general.is_some() => rank,
                    None => 1,
                },
                team: s.team,
            })
            .collect();
        let mut sim = Sim::new(map, source, self.seed, players);
        sim.fog = self.fog;
        Ok(sim)
    }

    fn clamp_slots(&mut self, starts: usize) {
        while self.slots.len() > starts.max(2) {
            self.slots.pop();
        }
        while self.slots.len() < 2.min(starts) {
            self.add_slot();
        }
    }

    fn add_slot(&mut self) {
        let faction = Faction::ALL[self.slots.len() % Faction::ALL.len()];
        self.slots.push(Slot { faction, general: None, ai: Some(AiLevel::Normal), team: 0 });
    }
}

#[derive(Resource)]
pub struct Ui {
    pub screen: Screen,
    pub paused: bool,
    pub config: SkirmishConfig,
    pub map_page: usize,
    /// The map selection is open over the skirmish page; it holds the map (and its slots)
    /// from before, which BACK restores.
    pub map_select: Option<(String, Vec<Slot>)>,
    /// The general the Generals screen shows.
    pub shown_general: GeneralId,
    /// The last Generals screen refusal, shown until the next action there.
    pub refused: Option<Refused>,
    /// Why START GAME was last refused on the skirmish page, shown until the next action.
    pub start_refused: Option<String>,
    /// The experience the finished match earned, for the end screen; set once per match.
    pub earned: Option<(GeneralId, u32)>,
    /// The settings as they were when the options page opened, for BACK.
    pub options_before: Option<Settings>,
    /// The mission the campaign page shows, and the one PLAY AGAIN replays after a mission.
    pub mission: g2_sim::MissionId,
    /// The match running is that mission (else a skirmish): the end screen offers the
    /// campaign, and a win is saved to the profile.
    pub in_mission: bool,
    /// What the lobby pages edit.
    pub lobby: crate::lobby::Form,
}

impl Default for Ui {
    fn default() -> Self {
        Ui {
            screen: Screen::Main,
            paused: false,
            config: SkirmishConfig::default(),
            map_page: 0,
            map_select: None,
            shown_general: GeneralId(0),
            refused: None,
            start_refused: None,
            earned: None,
            options_before: None,
            mission: g2_sim::MissionId(0),
            in_mission: false,
            lobby: crate::lobby::Form::default(),
        }
    }
}

/// A clickable node: left-click queues its `Act`. `bevy_ui`'s focus system keeps the sibling
/// `RelativeCursorPosition` current.
#[derive(Component)]
pub struct Btn(pub Act);

/// The map list the setup screen pages through. Parsing every official map takes a moment, so
/// `MenuPlugin` starts it on a thread and `poll_maps` picks the result up.
#[derive(Resource)]
struct MapList(Vec<g2_maps::SkirmishMap>);

#[derive(Resource)]
struct MapListChannel(std::sync::Mutex<mpsc::Receiver<Vec<g2_maps::SkirmishMap>>>);

/// Whether gameplay input (orders, camera) is accepted: no menu covers the match. Tests that
/// run without a menu count as playing. The lobby's text fields are on pages that cover it, so
/// letters typed there never reach the game.
pub fn playing(ui: Option<Res<Ui>>) -> bool {
    ui.is_none_or(|u| u.screen == Screen::Match && !u.paused)
}

/// Whether a lobby text field takes the keys, so the global keys (M mutes) leave them to it.
pub fn typing(ui: Option<Res<Ui>>) -> bool {
    ui.is_some_and(|u| u.lobby.typing.is_some())
}

/// True once per `MatchEpoch` bump, so scene systems run again after a world swap (and once at
/// startup, to build the battlefield behind the menu).
pub fn scene_start(epoch: Option<Res<MatchEpoch>>, mut seen: Local<Option<u64>>) -> bool {
    let now = epoch.map_or(0, |e| e.0);
    if *seen != Some(now) {
        *seen = Some(now);
        true
    } else {
        false
    }
}

/// `blocking`: load the map list before the first frame (headless screenshots) instead of on a
/// thread (the window, which must not stall on parsing every official map).
pub struct MenuPlugin {
    pub blocking: bool,
}

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        let (tx, rx) = mpsc::channel();
        if self.blocking {
            let _ = tx.send(g2_maps::skirmish_maps(&g2_maps::vendored_root()));
        } else {
            std::thread::spawn(move || {
                let _ = tx.send(g2_maps::skirmish_maps(&g2_maps::vendored_root()));
            });
        }
        if !app.world().contains_resource::<Profile>() {
            app.insert_resource(Profile::load(Profile::default_path(), g2_rules::rules()));
        }
        if !app.world().contains_resource::<Settings>() {
            app.insert_resource(Settings::load(Settings::default_path()));
        }
        app.init_resource::<Ui>()
            .init_resource::<Pending>()
            .init_resource::<MatchEpoch>()
            .init_resource::<crate::importer::Import>()
            .init_resource::<crate::lobby::Net>()
            .add_message::<bevy::input::keyboard::KeyboardInput>()
            .insert_resource(MapListChannel(std::sync::Mutex::new(rx)))
            .add_systems(Startup, load_fonts)
            .add_systems(
                Update,
                (
                    poll_maps,
                    crate::lobby::type_text,
                    interact,
                    apply,
                    crate::lobby::poll,
                    watch_winner,
                    award_experience,
                    poll_import,
                    rebuild,
                    hover,
                    hide_hud,
                )
                    .chain()
                    .in_set(MenuSet)
                    .run_if(resource_exists::<Sim>),
            );
        app.add_systems(Last, release_forwards_on_exit);
    }
}

/// On any app exit (the menu's Quit, closing the window), the router's forward of a hosted
/// game or a network match goes before the process does.
fn release_forwards_on_exit(
    mut exits: MessageReader<AppExit>,
    sim: Option<ResMut<Sim>>,
    net: Option<ResMut<crate::lobby::Net>>,
) {
    if exits.read().next().is_none() {
        return;
    }
    if let Some(online) = sim.and_then(|s| s.into_inner().online.as_mut()) {
        online.drop_forward();
    }
    if let Some(mut net) = net
        && let crate::lobby::Net::Hosting(h) = &mut *net
    {
        h.drop_forward();
    }
}

fn poll_maps(mut commands: Commands, rx: Option<Res<MapListChannel>>, done: Option<Res<MapList>>) {
    if done.is_some() {
        return;
    }
    if let Ok(maps) = rx.expect("map channel").0.lock().unwrap().try_recv() {
        commands.insert_resource(MapList(maps));
    }
}

/// The alpha page redraws when the import moves on: the worker thread only writes `Import`.
fn poll_import(import: Res<crate::importer::Import>, mut ui: ResMut<Ui>, mut seen: Local<crate::importer::Status>) {
    if ui.screen != Screen::Alpha {
        return;
    }
    let now = import.status();
    if *seen != now {
        *seen = now;
        ui.set_changed();
    }
}

/// Clicks on buttons and the menu keys (Esc, Enter) become `Act`s. Runs under menus too, which
/// is how the pause and end screens are driven.
fn interact(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    ui: Res<Ui>,
    mut placing: Option<ResMut<crate::input::Placement>>,
    mut aim: Option<ResMut<crate::input::PowerAim>>,
    buttons: Query<(&Btn, &RelativeCursorPosition)>,
    mut pending: ResMut<Pending>,
) {
    if mouse.just_pressed(MouseButton::Left) {
        for (btn, cur) in &buttons {
            if cur.cursor_over() {
                pending.0.push(btn.0);
            }
        }
    }
    // Esc while typing only ends the typing (`lobby::type_text`).
    if keys.just_pressed(KeyCode::Escape) && ui.lobby.typing.is_none() {
        // Esc closes whatever is open: a placement or an armed power first, then a menu.
        let placed = placing.as_mut().is_some_and(|p| p.0.take().is_some());
        let aimed = aim.as_mut().is_some_and(|a| a.0.take().is_some());
        if !placed && !aimed {
            pending.0.push(match (ui.screen, ui.paused) {
                (Screen::Setup, _) if ui.map_select.is_some() => Act::CancelMapList,
                (Screen::Options, _) => Act::CancelOptions,
                (Screen::Setup | Screen::Controls | Screen::Generals | Screen::Alpha | Screen::Missions, _) => {
                    Act::BackToMain
                }
                (Screen::Lobby, _) => Act::LeaveLobby,
                (Screen::NetGame, _) => Act::LeaveGame,
                (Screen::Match, false) => Act::Pause,
                (Screen::Match, true) => Act::Resume,
                (Screen::End, _) => Act::QuitToMenu,
                (Screen::Main, _) => Act::QuitGame,
            });
        }
    }
    if keys.just_pressed(KeyCode::Enter) && ui.screen == Screen::Setup {
        pending.0.push(Act::Start);
    }
    if keys.just_pressed(KeyCode::Enter) && ui.screen == Screen::Missions {
        pending.0.push(Act::StartMission);
    }
}

/// Hover feedback, as Generals': the element under the cursor gets a gold edge and an amber
/// label, a button a red glow.
fn hover(
    mut buttons: Query<(&Look, &RelativeCursorPosition, &Children, &mut BackgroundColor, &mut BorderColor)>,
    mut labels: Query<&mut TextColor>,
) {
    for (look, cur, children, mut bg, mut edge) in &mut buttons {
        let over = cur.cursor_over();
        bg.set_if_neq(BackgroundColor(look.fill(over)));
        edge.set_if_neq(BorderColor::all(look.edge(over)));
        for &child in &**children {
            if let Ok(mut ink) = labels.get_mut(child) {
                ink.set_if_neq(TextColor(look.ink(over)));
            }
        }
    }
}

/// Top-level match spawns that are UI nodes: the HUD panels, not the 3D scene.
type HudRoots = (With<MatchEntity>, With<Node>);

/// The match HUD is hidden behind the full-screen menus (everything but the in-game menu and
/// the options opened from it, which keep it, dimmed, as context).
fn hide_hud(ui: Res<Ui>, mut roots: Query<&mut Visibility, HudRoots>) {
    let hidden = match ui.screen {
        Screen::Match => false,
        Screen::Options => !ui.paused,
        Screen::Main
        | Screen::Controls
        | Screen::Setup
        | Screen::Generals
        | Screen::Alpha
        | Screen::End
        | Screen::Missions
        | Screen::Lobby
        | Screen::NetGame => true,
    };
    for mut v in &mut roots {
        v.set_if_neq(if hidden { Visibility::Hidden } else { Visibility::Inherited });
    }
}

/// Carries out queued acts. Exclusive: starting a match needs the world itself (swap `Sim`,
/// despawn the old scene).
fn apply(world: &mut World) {
    let acts = core::mem::take(&mut world.resource_mut::<Pending>().0);
    for act in acts {
        apply_one(world, act);
    }
}

fn apply_one(world: &mut World, act: Act) {
    if world.resource::<Ui>().start_refused.is_some() {
        world.resource_mut::<Ui>().start_refused = None;
    }
    match act {
        Act::QuitGame => {
            world.write_message(AppExit::Success);
        }
        // Starting always rolls a fresh world, so "start" after quitting a match is a new game
        // and never a silent resume of the frozen one.
        Act::StartMission => {
            let id = world.resource::<Ui>().mission;
            let unlocked = world.resource::<Profile>().unlocked(g2_rules::rules(), id.0 as usize);
            if !unlocked {
                return;
            }
            match Sim::mission(id, None, 1) {
                Ok(sim) => {
                    swap_world(world, sim);
                    let mut ui = world.resource_mut::<Ui>();
                    ui.in_mission = true;
                    ui.earned = None;
                    ui.screen = Screen::Match;
                    ui.paused = false;
                    world.resource_mut::<Sim>().speed = 1;
                }
                Err(e) => error!("cannot start mission: {e}"),
            }
        }
        Act::Start => {
            let mut config = world.resource::<Ui>().config.clone();
            if let Some(starts) = map_starts(world, &config.map) {
                config.clamp_slots(starts);
            }
            let profile = world.resource::<Profile>();
            let general = config.slots[0].general;
            let (perks, rank) = (profile.slotted(general), general.map_or(1, |g| profile.progress(g).rank));
            match config.build(&perks, rank) {
                Ok(sim) => {
                    swap_world(world, sim);
                    let mut ui = world.resource_mut::<Ui>();
                    ui.in_mission = false;
                    ui.config = config;
                    ui.earned = None;
                    ui.screen = Screen::Match;
                    ui.paused = false;
                    world.resource_mut::<Sim>().speed = 1;
                }
                Err(e) => world.resource_mut::<Ui>().start_refused = Some(e),
            }
        }
        // Everything below only edits `Ui` and the live `Sim`. The two borrow the world
        // mutably, so arms touch them one at a time.
        _ => match act {
            Act::ShowControls => world.resource_mut::<Ui>().screen = Screen::Controls,
            Act::OpenAlpha => world.resource_mut::<Ui>().screen = Screen::Alpha,
            Act::ImportAlpha => world.resource::<crate::importer::Import>().start(),
            Act::OpenSetup => {
                let mut ui = world.resource_mut::<Ui>();
                ui.screen = Screen::Setup;
                for s in ui.config.slots.iter_mut().filter(|s| s.ai == Some(AiLevel::Onslaught)) {
                    s.ai = Some(AiLevel::Normal);
                }
            }
            Act::OpenOnslaught => {
                let mut ui = world.resource_mut::<Ui>();
                ui.screen = Screen::Setup;
                for s in ui.config.slots.iter_mut().skip(1) {
                    s.ai = Some(AiLevel::Onslaught);
                }
            }
            Act::BackToMain => {
                let mut ui = world.resource_mut::<Ui>();
                ui.screen = Screen::Main;
                ui.map_select = None;
            }
            Act::ChangeSetup => {
                world.resource_mut::<Ui>().screen = Screen::Setup;
                world.resource_mut::<Sim>().speed = 0;
            }
            Act::QuitToMenu => {
                let mut ui = world.resource_mut::<Ui>();
                ui.screen = Screen::Main;
                ui.paused = false;
                let mut sim = world.resource_mut::<Sim>();
                sim.speed = 0;
                // Leaving a network match ends it: the other player sees the link close, and
                // the router's forward goes without the frame waiting for the router.
                if let Some(online) = &mut sim.online {
                    online.end_forward();
                }
                sim.online = None;
            }
            // Online the menu opens over a running match: the clock is shared, so stopping it
            // here would only stall the other player.
            Act::Pause => {
                world.resource_mut::<Ui>().paused = true;
                let mut sim = world.resource_mut::<Sim>();
                if sim.online.is_none() {
                    sim.speed = 0;
                }
            }
            Act::Resume => {
                world.resource_mut::<Ui>().paused = false;
                world.resource_mut::<Sim>().speed = 1;
            }
            Act::Surrender => {
                world.resource_mut::<Ui>().paused = false;
                world.resource_mut::<Sim>().speed = 1;
                let mut sim = world.resource_mut::<Sim>();
                sim.order(CommandKind::Surrender);
                // The button already clicked as a menu choice; the order is the same action.
                sim.unheard_orders = sim.unheard_orders.saturating_sub(1);
            }
            Act::MapPage(d) => {
                let pages = map_list(world).map_or(1, |m| m.len().div_ceil(MAPS_PER_PAGE).max(1));
                let mut ui = world.resource_mut::<Ui>();
                ui.map_page = ui.map_page.saturating_add_signed(d as isize).min(pages - 1);
            }
            // Picking keeps the list open, as Generals' map selection: the preview follows the
            // pick, and ACCEPT closes it.
            Act::MapPick(i) => {
                if let Some(m) = map_list(world).and_then(|m| m.get(i).cloned()) {
                    let mut ui = world.resource_mut::<Ui>();
                    ui.config.map = m.name;
                    ui.config.clamp_slots(m.starts);
                    ui.map_page = i / MAPS_PER_PAGE;
                }
            }
            Act::CyclePlayer(i) => {
                let starts = map_starts(world, &world.resource::<Ui>().config.map).unwrap_or(MAX_PLAYERS);
                let mut ui = world.resource_mut::<Ui>();
                let n = ui.config.slots.len();
                if i >= n {
                    if n < starts.min(MAX_PLAYERS) {
                        ui.config.add_slot();
                    }
                } else if i > 0 {
                    let next = match ui.config.slots[i].ai {
                        Some(AiLevel::Easy) => Some(AiLevel::Normal),
                        Some(AiLevel::Normal) => Some(AiLevel::Onslaught),
                        _ => None,
                    };
                    match next {
                        Some(level) => ui.config.slots[i].ai = Some(level),
                        None if i == n - 1 && n > 2 => {
                            ui.config.slots.pop();
                        }
                        None => ui.config.slots[i].ai = Some(AiLevel::Easy),
                    }
                }
            }
            Act::CycleSide(i) => {
                if let Some(s) = world.resource_mut::<Ui>().config.slots.get_mut(i) {
                    (s.faction, s.general) = crate::lobby::next_side(s.faction, s.general);
                }
            }
            Act::CycleTeam(i) => {
                if let Some(s) = world.resource_mut::<Ui>().config.slots.get_mut(i) {
                    s.team = (s.team + 1) % (MAX_TEAMS + 1);
                }
            }
            Act::ToggleMapList => {
                let mut ui = world.resource_mut::<Ui>();
                ui.map_select = match ui.map_select.take() {
                    Some(_) => None,
                    None => Some((ui.config.map.clone(), ui.config.slots.clone())),
                };
            }
            Act::CancelMapList => {
                let mut ui = world.resource_mut::<Ui>();
                if let Some((map, slots)) = ui.map_select.take() {
                    ui.config.map = map;
                    ui.config.slots = slots;
                }
            }
            Act::CycleCredits => {
                let mut ui = world.resource_mut::<Ui>();
                ui.config.credits = match ui.config.credits {
                    2_500 => 5_000,
                    5_000 => 10_000,
                    10_000 => 20_000,
                    _ => 2_500,
                };
            }
            Act::ToggleFog => {
                let mut ui = world.resource_mut::<Ui>();
                ui.config.fog = !ui.config.fog;
            }
            Act::NextSeed => {
                let mut ui = world.resource_mut::<Ui>();
                ui.config.seed = ui.config.seed.wrapping_add(1);
            }
            // Opens on the general the player last chose in the setup, if any.
            Act::OpenGenerals => {
                let mut ui = world.resource_mut::<Ui>();
                ui.screen = Screen::Generals;
                ui.refused = None;
                if let Some(g) = ui.config.slots[0].general {
                    ui.shown_general = g;
                }
            }
            Act::CycleShownGeneral(d) => {
                let n = g2_rules::rules().generals.len() as i32;
                let mut ui = world.resource_mut::<Ui>();
                ui.shown_general = GeneralId((ui.shown_general.0 as i32 + d).rem_euclid(n) as u16);
                ui.refused = None;
            }
            Act::Promote | Act::BuyPerk(_) | Act::SlotPerk(_) | Act::UnslotPerk(_) => {
                let g = world.resource::<Ui>().shown_general;
                let rules = g2_rules::rules();
                let mut profile = world.resource_mut::<Profile>();
                let done = match act {
                    Act::Promote => profile.promote(g),
                    Act::BuyPerk(k) => profile.buy(rules, g, k),
                    Act::SlotPerk(k) => profile.slot(g, k),
                    _ => {
                        let Act::UnslotPerk(k) = act else { unreachable!() };
                        profile.unslot(g, k);
                        Ok(())
                    }
                };
                if done.is_ok() {
                    profile.save(rules);
                }
                world.resource_mut::<Ui>().refused = done.err();
            }
            Act::OpenOptions => {
                let before = world.resource::<Settings>().clone();
                let mut ui = world.resource_mut::<Ui>();
                ui.options_before = Some(before);
                // From the in-game menu the match stays paused behind the page.
                ui.screen = Screen::Options;
            }
            // Opens on the first mission not yet won, also from the end of a mission, so a win
            // leads on to the next one.
            Act::OpenMissions => {
                world.resource_mut::<Sim>().speed = 0;
                let rules = g2_rules::rules();
                let profile = world.resource::<Profile>();
                let next = rules.missions.iter().position(|m| !profile.won(m.key)).unwrap_or(0);
                let mut ui = world.resource_mut::<Ui>();
                ui.screen = Screen::Missions;
                ui.paused = false;
                ui.mission = g2_sim::MissionId(next as u16);
            }
            Act::PickMission(i) => world.resource_mut::<Ui>().mission = g2_sim::MissionId(i),
            Act::CycleOption(opt) => cycle_option(&mut world.resource_mut::<Settings>(), opt),
            Act::DefaultOptions => world.resource_mut::<Settings>().reset(),
            Act::AcceptOptions | Act::CancelOptions => {
                let before = world.resource_mut::<Ui>().options_before.take();
                if act == Act::AcceptOptions {
                    world.resource::<Settings>().save();
                } else if let Some(before) = before {
                    world.insert_resource(before);
                }
                let mut ui = world.resource_mut::<Ui>();
                ui.screen = if ui.paused { Screen::Match } else { Screen::Main };
            }
            Act::OpenLobby
            | Act::LeaveLobby
            | Act::Type(_)
            | Act::PickGame(_)
            | Act::HostGame
            | Act::JoinGame
            | Act::DirectConnect
            | Act::LeaveGame
            | Act::NetMode
            | Act::NetMap(_)
            | Act::NetSide
            | Act::NetAccept
            | Act::NetStart => {
                // Every map a game may be hosted on; before the list lands, the built-in one.
                let maps: Vec<String> = map_list(world)
                    .map_or_else(|| vec!["Crater Ridge".to_owned()], |m| m.iter().map(|m| m.name.clone()).collect());
                crate::lobby::apply(world, act, &maps);
            }
            Act::Start | Act::StartMission | Act::QuitGame => unreachable!("handled above"),
        },
    }
}

/// Swaps in a fresh match: the old scene goes, the new world arrives, and the epoch bump
/// respawns everything from it this same frame (scene systems run after `MenuSet`).
pub(crate) fn swap_world(world: &mut World, sim: Sim) {
    world.despawn_all::<With<MatchEntity>>();
    world.insert_resource(sim);
    world.insert_resource(crate::view::Mirror::default());
    world.insert_resource(crate::sim::Previous::default());
    world.insert_resource(crate::sim::Hits::default());
    world.insert_resource(crate::sim::Deliveries::default());
    world.insert_resource(crate::sim::Heard::default());
    world.insert_resource(crate::alerts::Alerts::default());
    world.insert_resource(crate::input::Selection::default());
    world.insert_resource(crate::input::Placement::default());
    world.insert_resource(crate::select::Groups::default());
    world.insert_resource(crate::feedback::Marks::default());
    world.insert_resource(crate::input::PowerAim::default());
    world.resource_mut::<MatchEpoch>().0 += 1;
}

/// The match is over: freeze it and show the results. A network match needs the router's
/// forward no more.
fn watch_winner(mut sim: ResMut<Sim>, mut ui: ResMut<Ui>) {
    if ui.screen == Screen::Match && sim.world.winner().is_some() {
        ui.screen = Screen::End;
        ui.paused = false;
        if let Some(online) = &mut sim.online {
            online.end_forward();
        }
    }
}

/// A finished match earns the local human's general experience in their profile, once.
/// Watching AIs, or playing without a general, earns nothing.
fn award_experience(sim: Res<Sim>, mut ui: ResMut<Ui>, mut profile: ResMut<Profile>) {
    if ui.screen != Screen::End || ui.earned.is_some() || sim.world.is_ai(sim.local) {
        return;
    }
    // A mission won is saved, so the next one opens.
    if let Some(run) = sim.world.mission().filter(|r| r.result == Some(true) && ui.in_mission)
        && !profile.won(run.def.key)
    {
        profile.win(run.def.key);
        profile.save(g2_rules::rules());
    }
    let Some(g) = sim.world.player(sim.local).general else { return };
    let xp = match_experience(sim.world.won(sim.local), sim.world.stats(sim.local).kills);
    profile.earn(g, xp);
    profile.save(g2_rules::rules());
    ui.earned = Some((g, xp));
}

const MAPS_PER_PAGE: usize = 12;

/// Generals' skirmish page has eight player rows.
const MAX_PLAYERS: usize = 8;

/// Generals' Team combo holds no team and teams 1 to 4.
const MAX_TEAMS: u8 = 4;

fn map_list(world: &World) -> Option<&Vec<g2_maps::SkirmishMap>> {
    world.get_resource::<MapList>().map(|m| &m.0)
}

fn map_starts(world: &World, map: &str) -> Option<usize> {
    map_list(world).and_then(|maps| maps.iter().find(|m| m.name == map).map(|m| m.starts))
}

pub(crate) fn next_general(faction: Faction, current: Option<GeneralId>) -> Option<GeneralId> {
    let rules = g2_rules::rules();
    let options: Vec<GeneralId> = rules
        .generals
        .iter()
        .enumerate()
        .filter(|(_, g)| g.faction == faction)
        .map(|(i, _)| GeneralId(i as u16))
        .collect();
    match current {
        None => options.first().copied(),
        Some(g) => {
            let at = options.iter().position(|&o| o == g).map_or(0, |i| i + 1);
            options.get(at).copied()
        }
    }
}

// ---- drawing ----------------------------------------------------------------------------------
//
// `rebuild` replaces the whole menu overlay whenever `Ui` changes. The screens are laid out as
// Command & Conquer: Generals' shell, from its window layouts in EA's GPL release
// (`vendor/superhackers/GeneralsGamePatch2/Generals/Window/Menus/*.wnd`): the battlefield
// behind a ruled gold frame, black panels with gold edges, orange-red button labels that turn
// amber under the cursor, and everything where those layouts put it (docs/STYLE.md, "Menus").
// Each screen is their 800x600 canvas, centred and scaled to the window's height, so every
// rectangle below is in the layouts' own coordinates.

/// Generals' shell colours, from the layouts: gold edges and headings, the rust of list and
/// cell borders, the button labels and their hover colour, and the panels' blacks.
const GOLD: Color = Color::srgb_u8(255, 190, 1);
const RUST: Color = Color::srgb_u8(122, 48, 1);
const INK: Color = Color::srgb_u8(226, 76, 23);
const INK_HOT: Color = Color::srgb_u8(255, 191, 0);
const INK_OFF: Color = Color::srgb_u8(159, 143, 95);
const WHITE: Color = Color::srgb_u8(254, 254, 254);
const PANEL: Color = Color::srgba_u8(2, 2, 2, 193);
const BOX: Color = Color::srgba_u8(2, 2, 2, 126);
const DIALOG: Color = Color::srgba_u8(2, 2, 2, 190);
/// Outside the ruled frame, where Generals' `MainMenuRuler` darkens the shell map.
const SHADE: Color = Color::srgba(0.0, 0.0, 0.0, 0.6);

/// What a clickable element looks like and how it lights up under the cursor.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Look {
    /// A push button: dark, rust-edged, an orange-red label.
    Button,
    /// A combo box cell, whose click steps its value.
    Cell,
    /// A list row; `on` is the selected one.
    Row { on: bool },
}

impl Look {
    fn fill(self, hot: bool) -> Color {
        match self {
            Look::Button if hot => Color::srgba_u8(120, 26, 6, 235),
            Look::Button => Color::srgba_u8(24, 18, 12, 235),
            Look::Cell => Color::srgba_u8(2, 2, 2, 220),
            Look::Row { on: true } => Color::srgba_u8(150, 45, 10, 220),
            Look::Row { .. } if hot => Color::srgba_u8(120, 40, 10, 110),
            Look::Row { .. } => Color::NONE,
        }
    }

    fn edge(self, hot: bool) -> Color {
        match self {
            Look::Row { .. } => Color::NONE,
            _ if hot => GOLD,
            _ => RUST,
        }
    }

    fn ink(self, hot: bool) -> Color {
        match self {
            Look::Button | Look::Cell if hot => INK_HOT,
            Look::Button => INK,
            _ => WHITE,
        }
    }
}

/// The menus' and the HUD's typeface: DejaVu Sans Condensed, bundled (Bitstream Vera licence),
/// in place of the condensed faces Generals' shell is set in.
#[derive(Resource)]
pub struct MenuFonts {
    pub(crate) regular: Handle<Font>,
    pub(crate) bold: Handle<Font>,
}

fn load_fonts(mut commands: Commands, fonts: Option<ResMut<Assets<Font>>>) {
    let Some(mut fonts) = fonts else { return };
    let mut add = |bytes: &[u8]| fonts.add(Font::from_bytes(bytes.to_vec()));
    commands.insert_resource(MenuFonts {
        regular: add(include_bytes!("../assets/fonts/DejaVuSansCondensed.ttf")),
        bold: add(include_bytes!("../assets/fonts/DejaVuSansCondensed-Bold.ttf")),
    });
}

/// Fonts and art for this rebuild: general portraits come from the alpha when imported.
struct Kit<'a> {
    fonts: Option<&'a MenuFonts>,
    server: Option<&'a AssetServer>,
    alpha: bool,
}

impl Kit<'_> {
    /// `size` is in the layouts' pixels: the canvas is the window's height, so text scales
    /// with it.
    fn text(&self, s: &str, size: f32, color: Color, bold: bool) -> impl Bundle {
        let mut font = TextFont { font_size: FontSize::Vh(size / 6.0), ..default() };
        if let Some(f) = self.fonts {
            font.font = (if bold { f.bold.clone() } else { f.regular.clone() }).into();
        }
        (Text::new(s), font, TextColor(color))
    }

    /// An image from the import, or `None` without one.
    fn art(&self, rel: &str) -> Option<Handle<Image>> {
        let server = self.server.filter(|_| self.alpha)?;
        Some(crate::alpha_hud::image(server, rel))
    }
}

#[allow(clippy::too_many_arguments)]
fn rebuild(
    mut commands: Commands,
    ui: Res<Ui>,
    sim: Res<Sim>,
    profile: Res<Profile>,
    settings: Res<Settings>,
    maps: Option<Res<MapList>>,
    fonts: Option<Res<MenuFonts>>,
    cache: Option<Res<crate::alpha::Cache>>,
    server: Option<Res<AssetServer>>,
    mut images: Option<ResMut<Assets<Image>>>,
    mut preview: Local<Option<(String, Option<Handle<Image>>)>>,
    import: Res<crate::importer::Import>,
    net: Res<crate::lobby::Net>,
    old: Query<Entity, With<MenuUi>>,
) {
    if !ui.is_changed()
        && !profile.is_changed()
        && !settings.is_changed()
        && !fonts.as_ref().is_some_and(|f| f.is_added())
    {
        return;
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    // A running match shows the HUD alone: no overlay, no dimming.
    if ui.screen == Screen::Match && !ui.paused {
        return;
    }
    // The map preview is decoded once per map, not on every rebuild: the skirmish page's map,
    // the campaign page's picked mission's, or the network game's as offered.
    let shown = match ui.screen {
        Screen::Setup => Some(ui.config.map.clone()),
        Screen::Missions => Some(g2_rules::rules().mission(ui.mission).map.to_owned()),
        Screen::NetGame => net.offered_map(),
        _ => None,
    };
    if let Some(map) = shown
        && preview.as_ref().is_none_or(|(m, _)| *m != map)
    {
        let handle = crate::front::map_preview(&map).and_then(|img| images.as_mut().map(|i| i.add(img)));
        *preview = Some((map, handle));
    }
    let map_image = preview.as_ref().and_then(|(_, h)| h.clone());
    let maps = maps.as_ref().map(|m| m.0.as_slice());
    let kit = Kit { fonts: fonts.as_deref(), server: server.as_deref(), alpha: cache.is_some() };
    // The in-game menu sits on the match as Generals' does, and so do the options opened from
    // it; every other screen is framed by the ruler, the battlefield behind it standing in for
    // Generals' shell map.
    let ruled = ui.screen != Screen::Match && !ui.paused;
    let dim = if ruled { Color::NONE } else { Color::srgba(0.0, 0.0, 0.0, 0.3) };
    commands
        .spawn((
            MenuUi,
            GlobalZIndex(10),
            BackgroundColor(dim),
            Node {
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                overflow: Overflow::clip(),
                ..default()
            },
        ))
        .with_children(|root| {
            // Beside the 4:3 canvas the ruler's shade runs on to the window's edges.
            let side = |root: &mut ChildSpawnerCommands| {
                if ruled {
                    root.spawn((Node { flex_grow: 1.0, ..default() }, BackgroundColor(SHADE)));
                }
            };
            side(root);
            root.spawn(Node { height: percent(100), aspect_ratio: Some(4.0 / 3.0), flex_shrink: 0.0, ..default() })
                .with_children(|c| {
                    if ruled {
                        ruler(c);
                    }
                    match ui.screen {
                        Screen::Main => main_menu(c, &kit),
                        Screen::Controls => controls(c, &kit),
                        Screen::Setup => setup(c, &kit, &ui, &profile, maps, map_image),
                        Screen::Match => pause(c, &kit),
                        Screen::End => end_screen(c, &kit, &sim, &ui),
                        Screen::Generals => generals(c, &kit, &ui, &profile),
                        Screen::Alpha => alpha(c, &kit, &import.status()),
                        Screen::Options => options(c, &kit, &settings, ui.paused),
                        Screen::Missions => missions(c, &kit, &ui, &profile, map_image),
                        Screen::Lobby => lobby(c, &kit, &ui, &net),
                        Screen::NetGame => net_game(c, &kit, &ui, &net, &profile, map_image),
                    }
                });
            side(root);
        });
}

/// A rectangle of the 800x600 layouts, placed on the canvas.
fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: percent(x0 / 8.0),
        top: percent(y0 / 6.0),
        width: percent((x1 - x0) / 8.0),
        height: percent((y1 - y0) / 6.0),
        ..default()
    }
}

/// A box with a one-pixel edge, as the layouts' bordered windows.
fn frame(c: &mut ChildSpawnerCommands, r: Node, fill: Color, edge: Color) {
    c.spawn((Node { border: UiRect::all(px(1)), ..r }, BackgroundColor(fill), BorderColor::all(edge)));
}

/// Text in a rectangle, vertically centred; `centre` centres it across too.
fn label(c: &mut ChildSpawnerCommands, kit: &Kit, s: &str, r: Node, size: f32, color: Color, centre: bool) {
    let (across, lines) =
        if centre { (JustifyContent::Center, Justify::Center) } else { (JustifyContent::FlexStart, Justify::Left) };
    c.spawn(Node { align_items: AlignItems::Center, justify_content: across, ..r }).with_children(|t| {
        t.spawn((kit.text(s, size, color, false), TextLayout { justify: lines, ..default() }));
    });
}

/// A gold column heading, as the layouts' Arial 14 headings.
fn heading(c: &mut ChildSpawnerCommands, kit: &Kit, s: &str, r: Node) {
    c.spawn(Node { align_items: AlignItems::Center, ..r }).with_child(kit.text(s, 14.0, GOLD, true));
}

/// A page's title over its gold rule, top left of the panel.
fn title(c: &mut ChildSpawnerCommands, kit: &Kit, s: &str) {
    c.spawn(Node { align_items: AlignItems::Center, ..rect(63.0, 43.0, 600.0, 79.0) })
        .with_child(kit.text(s, 26.0, GOLD, true));
    c.spawn((rect(42.0, 81.0, 760.0, 82.0), BackgroundColor(GOLD)));
}

/// The full-page panel the skirmish, generals and controls pages sit in.
fn page(c: &mut ChildSpawnerCommands, kit: &Kit, name: &str) {
    frame(c, rect(42.0, 41.0, 760.0, 559.0), PANEL, GOLD);
    title(c, kit, name);
}

/// A clickable rectangle: `hover` lights it, a click queues its `Act`.
fn button(c: &mut ChildSpawnerCommands, kit: &Kit, s: &str, act: Act, look: Look, r: Node, size: f32) {
    let justify = if look == Look::Button { JustifyContent::Center } else { JustifyContent::FlexStart };
    c.spawn((
        Btn(act),
        look,
        RelativeCursorPosition::default(),
        Node {
            border: UiRect::all(px(1)),
            padding: UiRect::horizontal(px(4)),
            align_items: AlignItems::Center,
            justify_content: justify,
            ..r
        },
        BackgroundColor(look.fill(false)),
        BorderColor::all(look.edge(false)),
    ))
    .with_child(kit.text(s, size, look.ink(false), look == Look::Button));
}

/// A combo box: its value and the drop arrow; a click steps the value.
fn cell(c: &mut ChildSpawnerCommands, kit: &Kit, s: &str, act: Act, r: Node) {
    c.spawn((
        Btn(act),
        Look::Cell,
        RelativeCursorPosition::default(),
        Node {
            border: UiRect::all(px(1)),
            padding: UiRect::horizontal(px(4)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            ..r
        },
        BackgroundColor(Look::Cell.fill(false)),
        BorderColor::all(Look::Cell.edge(false)),
    ))
    .with_children(|b| {
        b.spawn(kit.text(s, 12.0, WHITE, false));
        b.spawn(kit.text("\u{25bc}", 8.0, WHITE, false));
    });
}

/// A cell that can't change: the human's own row, a closed seat.
fn fixed(c: &mut ChildSpawnerCommands, kit: &Kit, s: &str, r: Node, color: Color) {
    c.spawn((
        Node { border: UiRect::all(px(1)), padding: UiRect::horizontal(px(4)), align_items: AlignItems::Center, ..r },
        BackgroundColor(Look::Cell.fill(false)),
        BorderColor::all(RUST.with_alpha(0.6)),
    ))
    .with_child(kit.text(s, 12.0, color, false));
}

/// Lines of `label: value`, gold and white, as Generals' career box.
fn stats(c: &mut ChildSpawnerCommands, kit: &Kit, x: (f32, f32, f32), y0: f32, lines: &[(&str, String)]) {
    for (k, (name, value)) in lines.iter().enumerate() {
        let y = y0 + 22.0 * k as f32;
        label(c, kit, name, rect(x.0, y, x.1, y + 22.0), 12.0, GOLD, false);
        label(c, kit, value, rect(x.1, y, x.2, y + 22.0), 12.0, WHITE, false);
    }
}

/// Generals' `MainMenuRuler`: the shell map framed by a ruled gold line, darker outside it.
fn ruler(c: &mut ChildSpawnerCommands) {
    for (x0, y0, x1, y1) in
        [(0.0, 0.0, 800.0, 40.0), (0.0, 560.0, 800.0, 600.0), (0.0, 40.0, 40.0, 560.0), (760.0, 40.0, 800.0, 560.0)]
    {
        c.spawn((rect(x0, y0, x1, y1), BackgroundColor(SHADE)));
    }
    frame(c, rect(40.0, 40.0, 760.0, 560.0), Color::NONE, GOLD.with_alpha(0.8));
    // Ticks every 10 units outside the line, longer every 50.
    let tick = GOLD.with_alpha(0.45);
    for x in (40..=760).step_by(10) {
        let (x, l) = (x as f32, if x % 50 == 0 { 8.0 } else { 4.0 });
        c.spawn((Node { width: px(1), ..rect(x, 40.0 - l, x, 40.0) }, BackgroundColor(tick)));
        c.spawn((Node { width: px(1), ..rect(x, 560.0, x, 560.0 + l) }, BackgroundColor(tick)));
    }
    for y in (40..=560).step_by(10) {
        let (y, l) = (y as f32, if y % 50 == 0 { 8.0 } else { 4.0 });
        c.spawn((Node { height: px(1), ..rect(40.0 - l, y, 40.0, y) }, BackgroundColor(tick)));
        c.spawn((Node { height: px(1), ..rect(760.0, y, 760.0 + l, y) }, BackgroundColor(tick)));
    }
}

/// Where Generals has its logo: this project's name in gold, and what it is.
fn logo(c: &mut ChildSpawnerCommands, kit: &Kit, r: Node) {
    c.spawn(Node {
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        ..r
    })
    .with_children(|l| {
        l.spawn((kit.text("OPEN RTS", 46.0, GOLD, true), TextShadow { offset: Vec2::splat(2.0), color: RUST }));
        l.spawn(kit.text("an open-source RTS", 12.0, INK_OFF, false));
    });
}

/// The main menu, as Generals' (`MainMenu.wnd`): the logo top right and the button column in
/// its gold-edged box below it, over the battlefield.
fn main_menu(c: &mut ChildSpawnerCommands, kit: &Kit) {
    logo(c, kit, rect(504.0, 16.0, 791.0, 110.0));
    let items = [
        ("MISSIONS", Act::OpenMissions),
        ("SKIRMISH", Act::OpenSetup),
        ("ONSLAUGHT", Act::OpenOnslaught),
        ("NETWORK", Act::OpenLobby),
        ("GENERALS", Act::OpenGenerals),
        ("2013 ALPHA", Act::OpenAlpha),
        ("OPTIONS", Act::OpenOptions),
        ("CONTROLS", Act::ShowControls),
        ("EXIT GAME", Act::QuitGame),
    ];
    frame(c, rect(532.0, 108.0, 756.0, 120.0 + 40.0 * items.len() as f32), BOX, GOLD);
    for (k, (s, act)) in items.into_iter().enumerate() {
        let y = 116.0 + 40.0 * k as f32;
        button(c, kit, s, act, Look::Button, rect(540.0, y, 748.0, y + 36.0), 16.0);
    }
}

/// The alpha page: what importing the player's copy of the 2013 alpha does, where it goes,
/// and how far an import is.
fn alpha(c: &mut ChildSpawnerCommands, kit: &Kit, status: &crate::importer::Status) {
    use crate::importer::Status;
    page(c, kit, "2013 ALPHA");
    let cache = g2_frostbite::import::default_cache();
    let imported = cache.as_ref().is_some_and(|d| d.join("manifest.txt").is_file());
    let about = "If you have the 2013 closed alpha of Generals 2 (\"Generals2 F2P\"), the game can use \
                 its unit and building models, sound effects, unit voices and menu music, and the \
                 Generals page its portraits. Pick the alpha's CnC.exe: its files are decoded into a \
                 folder on this computer (about 4 GB, a few minutes). Nothing is uploaded or shared, \
                 and the game plays the same either way.";
    c.spawn((Node { padding: UiRect::all(px(6)), ..rect(64.0, 92.0, 740.0, 200.0) },))
        .with_child(kit.text(about, 14.0, WHITE, false));
    let folder = cache.as_ref().map_or("none: set G2_ALPHA_CACHE".to_owned(), |d| d.display().to_string());
    let (state, colour) = match status {
        Status::Idle if imported => ("Imported. Import again to replace it.".to_owned(), GOLD),
        Status::Idle => ("Not imported.".to_owned(), INK_OFF),
        Status::Choosing => ("Pick the alpha's CnC.exe in the file dialog...".to_owned(), INK_HOT),
        Status::Importing(part) => (format!("Importing: {part}..."), INK_HOT),
        Status::Done { .. } => ("Imported. Restart the game to use it.".to_owned(), GOLD),
        Status::Failed(why) => (format!("Import failed: {why}"), INK),
    };
    frame(c, rect(58.0, 210.0, 745.0, 493.0), BOX, RUST);
    let mut lines = vec![("Folder", folder), ("Status", state.clone())];
    if let Status::Done { summary, .. } = status {
        lines.push(("Written", summary.clone()));
    }
    for (k, (name, value)) in lines.iter().enumerate() {
        let y = 222.0 + 44.0 * k as f32;
        label(c, kit, name, rect(70.0, y, 170.0, y + 40.0), 13.0, GOLD, false);
        let ink = if *name == "Status" { colour } else { WHITE };
        c.spawn((Node { align_items: AlignItems::Center, ..rect(170.0, y, 735.0, y + 40.0) },))
            .with_child(kit.text(value, 13.0, ink, false));
    }
    let busy = matches!(status, Status::Choosing | Status::Importing(_));
    if !busy {
        button(c, kit, "FIND CNC.EXE", Act::ImportAlpha, Look::Button, rect(94.0, 509.0, 268.0, 545.0), 16.0);
    }
    button(c, kit, "BACK", Act::BackToMain, Look::Button, rect(530.0, 509.0, 701.0, 545.0), 16.0);
}

fn controls(c: &mut ChildSpawnerCommands, kit: &Kit) {
    page(c, kit, "CONTROLS");
    for (k, (keys, what)) in [
        ("Left click, drag; double click", "Select; a double click adds every one of its kind on screen"),
        ("Right click", "Context order: attack, gather, build, move; on the ground with a factory: rally point"),
        ("Ctrl + right click", "Attack-move"),
        ("Click the build grid", "Build, train, research (hover: what it is); Backspace cancels"),
        ("Ctrl + 0-9, 0-9", "Make a control group, select it (twice: go there)"),
        ("Shift / Alt + 0-9", "Add the group to the selection / go to it; Ctrl + Shift adds to it"),
        ("Z, X", "Select all units on screen, select more of the selected kinds"),
        ("Wheel, ] / [, .", "Zoom toward the cursor, zoom in / out, toggle closest view"),
        ("WASD, edge, middle drag", "Pan the battlefield; Q / E rotate"),
        ("T, R, E", "Deploy or transform; Particle Accelerator; evacuate"),
        ("G, F1-F3", "General's power, faction support powers"),
        ("Tab, V", "Next collector; go to the last attack"),
        ("Space, + / -, Esc, M", "Pause, game speed, menu, mute"),
    ]
    .into_iter()
    .enumerate()
    {
        let y = 94.0 + 31.0 * k as f32;
        label(c, kit, keys, rect(72.0, y, 290.0, y + 28.0), 14.0, GOLD, false);
        label(c, kit, what, rect(300.0, y, 745.0, y + 28.0), 14.0, WHITE, false);
    }
    button(c, kit, "BACK", Act::BackToMain, Look::Button, rect(530.0, 509.0, 701.0, 545.0), 16.0);
}

/// The in-game menu, as Generals' `QuitMenu.wnd`: the logo over a column of buttons, Options
/// among them.
fn pause(c: &mut ChildSpawnerCommands, kit: &Kit) {
    frame(c, rect(252.0, 100.0, 552.0, 380.0), DIALOG, GOLD);
    logo(c, kit, rect(262.0, 101.0, 550.0, 180.0));
    for (k, (s, act)) in [
        ("OPTIONS", Act::OpenOptions),
        ("SURRENDER", Act::Surrender),
        ("EXIT TO MAIN MENU", Act::QuitToMenu),
        ("RETURN TO GAME", Act::Resume),
    ]
    .into_iter()
    .enumerate()
    {
        let y = 183.0 + 48.0 * k as f32;
        button(c, kit, s, act, Look::Button, rect(288.0, y, 512.0, y + 40.0), 16.0);
    }
}

/// Steps `opt` to its next value, wrapping.
fn cycle_option(s: &mut Settings, opt: Opt) {
    use crate::settings::{Battlefield, Modifier, PAN_SPEEDS, Pan, Scheme, ZOOM_SPEEDS};
    fn next<T: Copy + PartialEq>(all: &[T], v: T) -> T {
        all[(all.iter().position(|&x| x == v).unwrap_or(0) + 1) % all.len()]
    }
    // A slider steps tick by tick: the next tick above the value, back to the first.
    let tick = |ticks: &[f32; 7], v: f32| *ticks.iter().find(|&&t| t > v + 1e-4).unwrap_or(&ticks[0]);
    match opt {
        Opt::Camera => s.camera = next(&Battlefield::ALL, s.camera),
        Opt::Scheme => s.scheme = next(&[Scheme::Classic, Scheme::Modern], s.scheme),
        Opt::Pan => s.pan = next(&Pan::ALL, s.pan),
        Opt::AlternatePan => s.alternate_pan = !s.alternate_pan,
        Opt::EdgePan => s.edge_pan = !s.edge_pan,
        Opt::LockMouse => s.lock_mouse = !s.lock_mouse,
        Opt::PanSpeed => s.pan_speed = tick(&PAN_SPEEDS, s.pan_speed),
        Opt::ZoomSpeed => s.zoom_speed = tick(&ZOOM_SPEEDS, s.zoom_speed),
        Opt::ZoomModifier => s.zoom_modifier = next(&Modifier::ALL, s.zoom_modifier),
    }
}

/// The options page, in the frame of Generals' `OptionsMenu.wnd` (its parent window at
/// 120,12-661,597, the gold title, the bordered Control Options panel and DEFAULTS, ACCEPT and
/// BACK along the bottom), with OpenRA's Display camera and Input settings in it: a click steps
/// a cell, OpenRA's dropdowns and sliders as Generals' combo boxes. Below the cells, what the
/// mouse does under the chosen scheme, in OpenRA's words.
fn options(c: &mut ChildSpawnerCommands, kit: &Kit, s: &Settings, in_match: bool) {
    use crate::settings::{Battlefield, Modifier, PAN_SPEEDS, Pan, Scheme, ZOOM_SPEEDS};
    frame(c, rect(120.0, 12.0, 661.0, 597.0), if in_match { DIALOG } else { PANEL }, GOLD);
    c.spawn(Node { align_items: AlignItems::Center, ..rect(152.0, 21.0, 640.0, 54.0) })
        .with_child(kit.text("OPTIONS", 26.0, GOLD, true));
    c.spawn((rect(140.0, 56.0, 641.0, 57.0), BackgroundColor(GOLD)));
    let on = |b: bool| if b { "On" } else { "Off" };
    let level = |ticks: &[f32; 7], v: f32| {
        let i = ticks.iter().position(|&t| (t - v).abs() < 1e-3).map_or(String::from("?"), |i| (i + 1).to_string());
        format!("{i} of 7")
    };
    let rows: [(&str, String, Opt); 9] = [
        (
            "Battlefield Camera",
            match s.camera {
                Battlefield::Close => "Close",
                Battlefield::Medium => "Medium",
                Battlefield::Far => "Far",
            }
            .into(),
            Opt::Camera,
        ),
        (
            "Control Scheme",
            match s.scheme {
                Scheme::Classic => "Classic",
                Scheme::Modern => "Modern",
            }
            .into(),
            Opt::Scheme,
        ),
        (
            "Pan Behaviour",
            match s.pan {
                Pan::Disabled => "Disabled",
                Pan::Standard => "Standard",
                Pan::Inverted => "Inverted",
                Pan::Joystick => "Joystick",
            }
            .into(),
            Opt::Pan,
        ),
        ("Alternate Mouse Panning", on(s.alternate_pan).into(), Opt::AlternatePan),
        ("Screen Edge Panning", on(s.edge_pan).into(), Opt::EdgePan),
        ("Lock Mouse to Window", on(s.lock_mouse).into(), Opt::LockMouse),
        ("Pan Speed", level(&PAN_SPEEDS, s.pan_speed), Opt::PanSpeed),
        ("Zoom Speed", level(&ZOOM_SPEEDS, s.zoom_speed), Opt::ZoomSpeed),
        (
            "Zoom Modifier",
            match s.zoom_modifier {
                Modifier::None => "None",
                Modifier::Alt => "Alt",
                Modifier::Ctrl => "Ctrl",
                Modifier::Meta => "Meta",
                Modifier::Shift => "Shift",
            }
            .into(),
            Opt::ZoomModifier,
        ),
    ];
    frame(c, rect(151.0, 69.0, 635.0, 376.0), BOX, RUST);
    heading(c, kit, "Control Options", rect(161.0, 72.0, 600.0, 96.0));
    for (k, (name, value, opt)) in rows.iter().enumerate() {
        let y = 100.0 + 30.0 * k as f32;
        label(c, kit, name, rect(168.0, y, 400.0, y + 24.0), 14.0, WHITE, false);
        cell(c, kit, value, Act::CycleOption(*opt), rect(420.0, y + 1.0, 615.0, y + 23.0));
    }
    // OpenRA's control scheme description, for the scheme, pan button and zoom modifier set.
    let (select, order) = match s.scheme {
        Scheme::Classic => ("Left", "Left"),
        Scheme::Modern => ("Left", "Right"),
    };
    let pan_button = if s.pan_button() == MouseButton::Right { "Right" } else { "Middle" };
    let zoom = match s.zoom_modifier {
        Modifier::None => "<Scroll Wheel>".to_owned(),
        m => format!("<{m:?} + Scroll Wheel>"),
    };
    let mut lines = vec![
        format!("- Select units using the <{select}> mouse button"),
        format!("- Command units using the <{order}> mouse button"),
        "- Place structures and aim powers using the <Left> mouse button".to_owned(),
        format!("- Zoom the battlefield using {zoom}"),
    ];
    if s.pan != Pan::Disabled {
        let edge = if s.edge_pan { " or the screen edge" } else { "" };
        lines.push(format!("- Pan the battlefield using the <{pan_button}> mouse button{edge}"));
    } else if s.edge_pan {
        lines.push("- Pan the battlefield using the screen edge".to_owned());
    }
    frame(c, rect(151.0, 384.0, 635.0, 516.0), BOX, RUST);
    for (k, line) in lines.iter().enumerate() {
        let y = 390.0 + 24.0 * k as f32;
        label(c, kit, line, rect(164.0, y, 630.0, y + 22.0), 13.0, INK_OFF, false);
    }
    for (s, act, x0, x1) in [
        ("DEFAULTS", Act::DefaultOptions, 152.0, 308.0),
        ("ACCEPT", Act::AcceptOptions, 312.0, 471.0),
        ("BACK", Act::CancelOptions, 476.0, 635.0),
    ] {
        button(c, kit, s, act, Look::Button, rect(x0, 528.0, x1, 560.0), 16.0);
    }
}

/// The skirmish page, as Generals' `SkirmishGameOptionsMenu.wnd`: eight player rows (player,
/// colour, faction, general), the map preview with its name and SELECT MAP, the match options
/// under the rows, the player's general in the career box, the map and mode in the info box,
/// START GAME and BACK. With the map list open, Generals' map selection replaces it.
fn setup(
    c: &mut ChildSpawnerCommands,
    kit: &Kit,
    ui: &Ui,
    profile: &Profile,
    maps: Option<&[g2_maps::SkirmishMap]>,
    preview: Option<Handle<Image>>,
) {
    if ui.map_select.is_some() {
        map_select(c, kit, ui, maps, preview);
        return;
    }
    let rules = g2_rules::rules();
    let onslaught = ui.config.slots.iter().any(|s| s.ai == Some(AiLevel::Onslaught));
    page(c, kit, if onslaught { "ONSLAUGHT" } else { "SKIRMISH" });
    for (s, x0, x1) in
        [("Players", 72.0, 216.0), ("Color", 224.0, 308.0), ("Faction", 316.0, 424.0), ("Team", 432.0, 544.0)]
    {
        heading(c, kit, s, rect(x0, 87.0, x1, 111.0));
    }
    let starts = maps.and_then(|m| m.iter().find(|m| m.name == ui.config.map)).map_or(2, |m| m.starts);
    let colours = crate::models::team_slots(&ui.config.slots.iter().map(|s| s.faction).collect::<Vec<_>>());
    for (i, y) in (0..MAX_PLAYERS).map(|i| (i, 111.0 + 24.0 * i as f32)) {
        let at = |x0: f32, x1: f32| rect(x0, y + 1.0, x1, y + 23.0);
        match ui.config.slots.get(i).zip(colours.get(i)) {
            Some((slot, &colour)) => {
                match slot.ai {
                    None => fixed(c, kit, "Player", at(72.0, 216.0), WHITE),
                    Some(level) => cell(c, kit, ai_name(level), Act::CyclePlayer(i), at(72.0, 216.0)),
                }
                frame(c, at(224.0, 308.0), Look::Cell.fill(false), RUST.with_alpha(0.6));
                c.spawn((rect(229.0, y + 6.0, 303.0, y + 18.0), BackgroundColor(crate::models::TEAM_PALETTE[colour])));
                cell(c, kit, &side_name(slot.faction, slot.general, true), Act::CycleSide(i), at(316.0, 424.0));
                cell(c, kit, &team_name(slot.team), Act::CycleTeam(i), at(432.0, 508.0));
            }
            None if i < starts => cell(c, kit, "Open", Act::CyclePlayer(i), at(72.0, 216.0)),
            None => fixed(c, kit, "Closed", at(72.0, 216.0), INK_OFF),
        }
    }
    // The map: preview, name, and the button to the map selection.
    heading(c, kit, "Map Preview", rect(545.0, 87.0, 709.0, 111.0));
    frame(c, rect(545.0, 114.0, 709.0, 250.0), Color::BLACK, RUST);
    if let Some(img) = preview {
        c.spawn((ImageNode::new(img), rect(560.0, 115.0, 694.0, 249.0)));
    }
    label(
        c,
        kit,
        &format!("{} ({})", map_title(&ui.config.map), starts),
        rect(532.0, 251.0, 716.0, 279.0),
        12.0,
        WHITE,
        true,
    );
    button(c, kit, "SELECT MAP", Act::ToggleMapList, Look::Button, rect(543.0, 280.0, 709.0, 304.0), 13.0);
    // Match options.
    for (s, x0, x1) in [("Starting Cash", 72.0, 216.0), ("Fog of War", 224.0, 372.0), ("Seed", 380.0, 520.0)] {
        heading(c, kit, s, rect(x0, 312.0, x1, 336.0));
    }
    cell(c, kit, &format!("${}", thousands(ui.config.credits)), Act::CycleCredits, rect(72.0, 337.0, 216.0, 359.0));
    cell(c, kit, if ui.config.fog { "On" } else { "Off" }, Act::ToggleFog, rect(224.0, 337.0, 372.0, 359.0));
    cell(c, kit, &ui.config.seed.to_string(), Act::NextSeed, rect(380.0, 337.0, 520.0, 359.0));
    // The career box: the general the player brings.
    frame(c, rect(58.0, 380.0, 291.0, 493.0), BOX, RUST);
    let general = ui.config.slots[0].general;
    let lines: Vec<(&str, String)> = match general {
        Some(g) => {
            let p = profile.progress(g);
            vec![
                ("General", rules.general(g).name.to_owned()),
                ("Rank", format!("{} of {MAX_RANK}", p.rank)),
                ("Experience", format!("{} XP", p.xp)),
                ("Perks slotted", format!("{} of {}", p.slotted.len(), p.rank)),
            ]
        }
        None => vec![("General", "None".to_owned()), ("Rank", "-".to_owned()), ("Experience", "-".to_owned())],
    };
    stats(c, kit, (64.0, 170.0, 287.0), 386.0, &lines);
    // The info box: what this match is.
    frame(c, rect(303.0, 380.0, 745.0, 493.0), BOX, RUST);
    let mode = if onslaught {
        "Onslaught: the enemy has no base. Ten waves of its army come at you, the first at 2:30, then one \
         every 75 seconds; outlast the last one."
    } else {
        "Skirmish: destroy every enemy building and unit. Players on one team are allies and win \
         together. Click an AI player to set it Easy, Normal or Onslaught; an Open seat adds one."
    };
    c.spawn(Node {
        flex_direction: FlexDirection::Column,
        row_gap: px(6),
        padding: UiRect::all(px(6)),
        ..rect(305.0, 382.0, 743.0, 491.0)
    })
    .with_children(|info| {
        info.spawn(kit.text(
            &format!("{}, {} players", map_title(&ui.config.map), ui.config.slots.len()),
            13.0,
            GOLD,
            true,
        ));
        if let Some(why) = &ui.start_refused {
            info.spawn(kit.text(why, 13.0, INK_HOT, true));
        }
        info.spawn(kit.text(mode, 12.0, WHITE, false));
    });
    button(c, kit, "START GAME", Act::Start, Look::Button, rect(94.0, 509.0, 268.0, 545.0), 16.0);
    button(c, kit, "BACK", Act::BackToMain, Look::Button, rect(530.0, 509.0, 701.0, 545.0), 16.0);
}

/// Generals' map selection (`SkirmishMapSelectMenu.wnd`): the list with its red scroll bar,
/// the picked map's preview beside it, ACCEPT and BACK.
fn map_select(
    c: &mut ChildSpawnerCommands,
    kit: &Kit,
    ui: &Ui,
    maps: Option<&[g2_maps::SkirmishMap]>,
    preview: Option<Handle<Image>>,
) {
    heading(c, kit, "Select a map", rect(221.0, 89.0, 726.0, 112.0));
    frame(c, rect(223.0, 111.0, 726.0, 303.0), DIALOG, GOLD);
    let Some(maps) = maps else {
        label(c, kit, "loading maps...", rect(230.0, 144.0, 574.0, 297.0), 12.0, WHITE, true);
        return;
    };
    let pages = maps.len().div_ceil(MAPS_PER_PAGE).max(1);
    let page = ui.map_page.min(pages - 1);
    fixed(c, kit, &format!("Official Maps ({})", maps.len()), rect(230.0, 118.0, 400.0, 139.0), WHITE);
    label(c, kit, &format!("page {} of {pages}", page + 1), rect(406.0, 118.0, 574.0, 139.0), 11.0, INK_OFF, true);
    frame(c, rect(230.0, 144.0, 574.0, 297.0), BOX, RUST);
    for (k, (i, m)) in maps.iter().enumerate().skip(page * MAPS_PER_PAGE).take(MAPS_PER_PAGE).enumerate() {
        let y = 146.0 + 12.4 * k as f32;
        let row = format!("{} ({})", map_title(&m.name), m.starts);
        button(
            c,
            kit,
            &row,
            Act::MapPick(i),
            Look::Row { on: m.name == ui.config.map },
            rect(232.0, y, 572.0, y + 12.4),
            11.0,
        );
    }
    // The scroll bar pages: Generals' red track between two arrows, the thumb where the page is.
    c.spawn((rect(576.0, 156.0, 588.0, 285.0), BackgroundColor(Color::srgb_u8(170, 20, 10))));
    let thumb = 156.0 + 117.0 * page as f32 / (pages - 1).max(1) as f32;
    c.spawn((rect(576.0, thumb, 588.0, thumb + 12.0), BackgroundColor(GOLD)));
    button(c, kit, "\u{25b2}", Act::MapPage(-1), Look::Button, rect(576.0, 144.0, 588.0, 156.0), 8.0);
    button(c, kit, "\u{25bc}", Act::MapPage(1), Look::Button, rect(576.0, 285.0, 588.0, 297.0), 8.0);
    frame(c, rect(591.0, 120.0, 716.0, 235.0), Color::BLACK, RUST);
    if let Some(img) = preview {
        c.spawn((ImageNode::new(img), rect(597.0, 121.0, 710.0, 234.0)));
    }
    button(c, kit, "ACCEPT", Act::ToggleMapList, Look::Button, rect(590.0, 240.0, 718.0, 267.0), 14.0);
    button(c, kit, "BACK", Act::CancelMapList, Look::Button, rect(590.0, 272.0, 718.0, 298.0), 14.0);
}

/// A text field, as the layouts' entry fields: its text, a caret while typing, and a click
/// starts typing into it.
fn entry(c: &mut ChildSpawnerCommands, kit: &Kit, ui: &Ui, e: crate::lobby::Entry, r: Node) {
    let typing = ui.lobby.typing == Some(e);
    let text = format!("{}{}", ui.lobby.text(e), if typing { "_" } else { "" });
    c.spawn((
        Btn(Act::Type(Some(e))),
        Look::Cell,
        RelativeCursorPosition::default(),
        Node { border: UiRect::all(px(1)), padding: UiRect::horizontal(px(4)), align_items: AlignItems::Center, ..r },
        BackgroundColor(Look::Cell.fill(false)),
        BorderColor::all(if typing { GOLD } else { Look::Cell.edge(false) }),
    ))
    .with_child(kit.text(&text, 12.0, WHITE, false));
}

/// A Team cell, as Generals' combo: "-" for no team.
fn team_name(team: u8) -> String {
    if team == 0 { "-".to_owned() } else { format!("Team {team}") }
}

/// A side as the lobby lists it: the faction, or the general with its faction's tag. `short`
/// for the game options' narrow column (Generals' 108 wide), which takes the tag alone.
fn side_name(faction: Faction, general: Option<GeneralId>, short: bool) -> String {
    match general {
        Some(g) => format!("{} {}", faction_tag(faction), g2_rules::rules().general(g).name),
        None if short => faction_tag(faction).to_owned(),
        None => faction_name(faction).to_owned(),
    }
}

/// The LAN lobby, as Generals' `LanLobbyMenu.wnd`: the player name entry top left, the
/// players and games lists side by side with the picked game's info beside them, the chat
/// window (here what happened, and the direct-connect address and the port a hosted game
/// listens on, which Generals asks in its own dialog) and HOST, JOIN, DIRECT CONNECT and BACK
/// along the bottom.
fn lobby(c: &mut ChildSpawnerCommands, kit: &Kit, ui: &Ui, net: &crate::lobby::Net) {
    use crate::lobby::Entry;
    frame(c, rect(41.0, 41.0, 759.0, 558.0), PANEL, GOLD);
    c.spawn(Node { align_items: AlignItems::Center, ..rect(47.0, 43.0, 655.0, 76.0) }).with_child(kit.text(
        "LAN LOBBY",
        26.0,
        GOLD,
        true,
    ));
    c.spawn((rect(41.0, 75.0, 759.0, 76.0), BackgroundColor(GOLD)));
    label(c, kit, "Player Name:", rect(52.0, 80.0, 213.0, 104.0), 13.0, GOLD, false);
    entry(c, kit, ui, Entry::Name, rect(217.0, 81.0, 417.0, 106.0));
    // The players in the lobby: this one, and the hosts heard (each sits in their own game).
    let games = match net {
        crate::lobby::Net::Browsing(d) => d.games.as_slice(),
        _ => &[],
    };
    heading(c, kit, "Players", rect(53.0, 103.0, 309.0, 131.0));
    frame(c, rect(53.0, 131.0, 274.0, 311.0), BOX, RUST);
    let players = std::iter::once(ui.lobby.name.clone()).chain(games.iter().map(|g| g.name.clone()));
    for (k, name) in players.take(14).enumerate() {
        let y = 134.0 + 12.4 * k as f32;
        label(c, kit, &name, rect(58.0, y, 270.0, y + 12.4), 11.0, if k == 0 { GOLD } else { WHITE }, false);
    }
    heading(c, kit, "Games", rect(275.0, 103.0, 515.0, 131.0));
    frame(c, rect(277.0, 131.0, 517.0, 311.0), BOX, RUST);
    for (i, g) in games.iter().take(14).enumerate() {
        let y = 133.0 + 12.4 * i as f32;
        let row = format!("{}{}", g.name, if g.compatible { "" } else { " (other version)" });
        button(
            c,
            kit,
            &row,
            Act::PickGame(i),
            Look::Row { on: ui.lobby.picked == Some(i) },
            rect(279.0, y, 515.0, y + 12.4),
            11.0,
        );
    }
    if games.is_empty() {
        label(c, kit, "No games found on this network yet.", rect(280.0, 135.0, 515.0, 160.0), 11.0, INK_OFF, true);
    }
    heading(c, kit, "Game Info", rect(521.0, 103.0, 709.0, 127.0));
    let info: Vec<(&str, String)> = match ui.lobby.picked.and_then(|i| games.get(i)) {
        Some(g) => vec![
            ("Host", g.name.clone()),
            ("Map", map_title(&g.map)),
            ("Mode", (if g.coop { "Co-op Onslaught" } else { "Versus" }).to_owned()),
            ("Players", (if g.open { "1 of 2" } else { "2 of 2" }).to_owned()),
            ("Address", g.addr.to_string()),
        ],
        None => vec![("", "Click a game to see it here.".to_owned())],
    };
    stats(c, kit, (525.0, 590.0, 753.0), 132.0, &info);
    // The chat window's place: what happened, the address to connect to and the port to host on.
    frame(c, rect(52.0, 315.0, 751.0, 462.0), BOX, RUST);
    let mut lines = vec![
        "Games on this local network appear in the list. HOST opens one for a second player (and asks your \
         router to forward its port, for players on the internet); JOIN takes the picked game; DIRECT CONNECT \
         reaches a host by its address or name, with its port (8088 if left out); IPv6 as [address]:port."
            .to_owned(),
    ];
    lines.extend(ui.lobby.note.as_ref().map(|n| format!("Can't do that: {n}")));
    c.spawn(Node {
        flex_direction: FlexDirection::Column,
        row_gap: px(4),
        padding: UiRect::all(px(6)),
        ..rect(54.0, 317.0, 749.0, 400.0)
    })
    .with_children(|t| {
        for (k, l) in lines.iter().enumerate() {
            t.spawn(kit.text(l, 12.0, if k == 0 { WHITE } else { INK_HOT }, false));
        }
    });
    label(c, kit, "Host port:", rect(62.0, 404.0, 162.0, 428.0), 13.0, GOLD, false);
    entry(c, kit, ui, Entry::Port, rect(166.0, 405.0, 246.0, 428.0));
    label(c, kit, "Side:", rect(262.0, 404.0, 312.0, 428.0), 13.0, GOLD, false);
    cell(c, kit, &side_name(ui.lobby.faction, ui.lobby.general, false), Act::NetSide, rect(316.0, 405.0, 516.0, 428.0));
    label(c, kit, "Direct connect to:", rect(62.0, 432.0, 212.0, 456.0), 13.0, GOLD, false);
    entry(c, kit, ui, Entry::Address, rect(216.0, 433.0, 516.0, 456.0));
    button(c, kit, "HOST", Act::HostGame, Look::Button, rect(54.0, 507.0, 225.0, 543.0), 16.0);
    button(c, kit, "JOIN", Act::JoinGame, Look::Button, rect(228.0, 507.0, 399.0, 543.0), 16.0);
    button(c, kit, "DIRECT CONNECT", Act::DirectConnect, Look::Button, rect(402.0, 507.0, 574.0, 543.0), 16.0);
    button(c, kit, "BACK", Act::LeaveLobby, Look::Button, rect(576.0, 507.0, 747.0, 543.0), 16.0);
}

/// The LAN game options, as Generals' `LanGameOptionsMenu.wnd`: the accept marks and the
/// player, colour, faction and team columns, the map preview with its name and the host's
/// map buttons, the chat window (here the mode and what happened) and START GAME (the host;
/// the joiner's is ACCEPT) and BACK.
fn net_game(
    c: &mut ChildSpawnerCommands,
    kit: &Kit,
    ui: &Ui,
    net: &crate::lobby::Net,
    profile: &Profile,
    preview: Option<Handle<Image>>,
) {
    frame(c, rect(42.0, 41.0, 758.0, 558.0), PANEL, GOLD);
    c.spawn(Node { align_items: AlignItems::Center, ..rect(48.0, 46.0, 384.0, 86.0) }).with_child(kit.text(
        "GAME OPTIONS",
        26.0,
        GOLD,
        true,
    ));
    c.spawn((rect(42.0, 85.0, 758.0, 86.0), BackgroundColor(GOLD)));
    for (s, x0, x1) in
        [("Players", 79.0, 199.0), ("Color", 231.0, 311.0), ("Faction", 323.0, 431.0), ("Team", 439.0, 551.0)]
    {
        heading(c, kit, s, rect(x0, 94.0, x1, 118.0));
    }
    let Some((setup, rows, host)) = net.game() else {
        label(c, kit, "Connecting...", rect(79.0, 119.0, 515.0, 143.0), 13.0, WHITE, false);
        for (k, l) in net.log().iter().chain(&ui.lobby.note).enumerate() {
            let y = 330.0 + 18.0 * k as f32;
            label(c, kit, l, rect(84.0, y, 708.0, y + 18.0), 12.0, WHITE, false);
        }
        button(c, kit, "BACK", Act::LeaveGame, Look::Button, rect(530.0, 509.0, 701.0, 545.0), 16.0);
        return;
    };
    let coop = setup.players.len() == 3;
    let colours = crate::models::team_slots(&setup.players.iter().map(|p| p.faction).collect::<Vec<_>>());
    for (i, row) in rows.iter().enumerate() {
        let y = 119.0 + 24.0 * i as f32;
        let at = |x0: f32, x1: f32| rect(x0, y, x1, y + 24.0);
        // The accept mark: green once accepted, as Generals' ButtonAccept.
        let mark = if row.ready { Color::srgb_u8(1, 255, 1) } else { BOX };
        frame(c, rect(58.0, y + 3.0, 74.0, y + 19.0), if row.waves { Color::NONE } else { mark }, RUST);
        fixed(c, kit, &row.name, at(79.0, 223.0), if row.own { GOLD } else { WHITE });
        frame(c, at(231.0, 315.0), Look::Cell.fill(false), RUST.with_alpha(0.6));
        c.spawn((rect(236.0, y + 6.0, 310.0, y + 18.0), BackgroundColor(crate::models::TEAM_PALETTE[colours[i]])));
        let side = side_name(row.faction, row.general, true);
        if row.own {
            cell(c, kit, &side, Act::NetSide, at(323.0, 431.0));
        } else {
            fixed(c, kit, &side, at(323.0, 431.0), WHITE);
        }
        fixed(c, kit, &team_name(setup.players[i].team), at(439.0, 515.0), WHITE);
    }
    // The map: preview, name, the host's buttons.
    heading(c, kit, "Map Preview", rect(547.0, 94.0, 711.0, 118.0));
    frame(c, rect(549.0, 121.0, 711.0, 257.0), Color::BLACK, RUST);
    if let Some(img) = preview {
        c.spawn((ImageNode::new(img), rect(562.0, 122.0, 698.0, 256.0)));
    }
    label(c, kit, &map_title(&setup.map), rect(539.0, 259.0, 723.0, 287.0), 12.0, WHITE, true);
    if host {
        button(c, kit, "\u{25c0}", Act::NetMap(-1), Look::Button, rect(546.0, 285.0, 576.0, 311.0), 12.0);
        button(c, kit, "SELECT MAP", Act::NetMap(1), Look::Button, rect(580.0, 285.0, 714.0, 311.0), 13.0);
    }
    // The mode where the skirmish page has its match options: the host changes it.
    heading(c, kit, "Mode", rect(79.0, 200.0, 223.0, 224.0));
    let mode = if coop { "Co-op Onslaught" } else { "Versus" };
    if host {
        cell(c, kit, mode, Act::NetMode, rect(79.0, 225.0, 315.0, 247.0));
    } else {
        fixed(c, kit, mode, rect(79.0, 225.0, 315.0, 247.0), WHITE);
    }
    let about = if coop {
        "Co-op Onslaught: both players on one team against ten waves from the far side of the map, split \
         between your bases; the first at 2:30, then one every 75 seconds. Outlast the last one together."
    } else {
        "Versus: destroy every building and unit of the other player."
    };
    label(c, kit, about, rect(323.0, 200.0, 535.0, 300.0), 11.0, WHITE, false);
    // The chat window's place: where the game can be joined from (the host's), who came and
    // went, and the rank the local general plays at. Lines wrap; the addresses always show, the
    // rest from the newest back as far as there is room.
    frame(c, rect(78.0, 324.0, 714.0, 469.0), BOX, RUST);
    let own = rows.iter().find(|r| r.own);
    let addresses = match net {
        crate::lobby::Net::Hosting(h) => h.addresses(),
        _ => Vec::new(),
    };
    let mut lines = net.log();
    if let Some(g) = own.and_then(|r| r.general) {
        let p = profile.progress(g);
        lines.push(format!(
            "You play {} at rank {} with {} perks slotted.",
            g2_rules::rules().general(g).name,
            p.rank,
            p.slotted.len()
        ));
    }
    lines.push(
        (if host {
            "START GAME begins once the other player has accepted; any change here clears their accept."
        } else {
            "ACCEPT when you are ready; the host starts the game. A change of the host's clears your accept."
        })
        .to_owned(),
    );
    lines.extend(ui.lobby.note.as_ref().map(|n| format!("Can't do that: {n}")));
    // A warning about the forward takes two rows.
    let room = 7usize.saturating_sub(addresses.iter().map(|&(_, warn)| 1 + warn as usize).sum());
    let shown = lines.len().saturating_sub(room.max(2));
    c.spawn(Node {
        flex_direction: FlexDirection::Column,
        row_gap: px(3),
        padding: UiRect::axes(px(6), px(4)),
        overflow: Overflow::clip(),
        ..rect(79.0, 325.0, 713.0, 468.0)
    })
    .with_children(|t| {
        for (l, warn) in &addresses {
            let ink = if *warn {
                INK_HOT
            } else if l.starts_with("Internet") {
                GOLD
            } else {
                WHITE
            };
            t.spawn(kit.text(l, 12.0, ink, false));
        }
        for l in &lines[shown..] {
            t.spawn(kit.text(l, 12.0, if l.starts_with("Can't") { INK_HOT } else { WHITE }, false));
        }
    });
    if host {
        button(c, kit, "START GAME", Act::NetStart, Look::Button, rect(94.0, 509.0, 268.0, 545.0), 16.0);
    } else {
        let accepted = rows.get(1).is_some_and(|r| r.ready);
        button(
            c,
            kit,
            if accepted { "NOT READY" } else { "ACCEPT" },
            Act::NetAccept,
            Look::Button,
            rect(94.0, 509.0, 268.0, 545.0),
            16.0,
        );
    }
    button(c, kit, "BACK", Act::LeaveGame, Look::Button, rect(530.0, 509.0, 701.0, 545.0), 16.0);
}

/// The score screen, as Generals' `ScoreScreen.wnd`: the outcome across the top band, a row
/// of totals per player under gold column heads (a green mark for the winner), the match's
/// details in the box below, and the buttons along the bottom.
fn end_screen(c: &mut ChildSpawnerCommands, kit: &Kit, sim: &Sim, ui: &Ui) {
    let world = &sim.world;
    let winner = world.winner();
    // Watching two AIs play (the local player is one of them) has no victory to claim.
    let watched = world.is_ai(sim.local);
    // A side wins together: an ally who fell before the end won too, as in Generals.
    let headline = match winner {
        // A mission's result is the mission's, whoever played the seat.
        _ if let Some(won) = world.mission().and_then(|r| r.result) => {
            if won {
                "MISSION ACCOMPLISHED"
            } else {
                "MISSION FAILED"
            }
        }
        _ if watched => "MATCH OVER",
        Some(_) if world.won(sim.local) => "VICTORY",
        Some(_) => "DEFEAT",
        None => "MATCH OVER",
    };
    frame(c, rect(37.0, 29.0, 767.0, 546.0), DIALOG, Color::NONE);
    let columns = [
        ("Player", 72.0, 187.0),
        ("Units Built", 187.0, 258.0),
        ("Units Lost", 258.0, 330.0),
        ("Buildings Built", 330.0, 402.0),
        ("Buildings Lost", 402.0, 490.0),
        ("Enemies Destroyed", 490.0, 574.0),
        ("Supplies Collected", 574.0, 658.0),
        ("Result", 658.0, 756.0),
    ];
    for (s, x0, x1) in columns {
        label(c, kit, s, rect(x0, 33.0, x1, 65.0), 11.0, GOLD, true);
    }
    frame(c, rect(36.0, 68.0, 767.0, 125.0), Color::NONE, GOLD);
    c.spawn(Node {
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        ..rect(36.0, 68.0, 767.0, 125.0)
    })
    .with_child((kit.text(headline, 36.0, GOLD, true), TextShadow { offset: Vec2::splat(2.0), color: RUST }));
    for i in 0..world.players().len() {
        let id = PlayerId(i as u8);
        let s = world.stats(id);
        let y = 129.0 + 32.0 * i as f32;
        let won = world.won(id);
        frame(c, rect(43.0, y, 68.0, y + 22.0), if won { Color::srgb_u8(1, 255, 1) } else { BOX }, RUST);
        let name = match world.player(id).general {
            Some(g) => format!("{} {}", faction_tag(world.player(id).faction), world.rules.general(g).name),
            None => faction_tag(world.player(id).faction).to_owned(),
        };
        let result = match s.defeated_at {
            Some(t) => format!("Out {}", clock(t)),
            None if won => "Winner".to_owned(),
            None => String::new(),
        };
        let values = [
            name,
            s.units_built.to_string(),
            s.units_lost.to_string(),
            s.buildings_built.to_string(),
            s.buildings_lost.to_string(),
            s.kills.to_string(),
            format!("${}", thousands(s.earned)),
            result,
        ];
        for (k, (v, (_, x0, x1))) in values.iter().zip(columns).enumerate() {
            let color = if k == 0 { sim.team_color(id) } else { WHITE };
            label(c, kit, v, rect(x0, y, x1, y + 22.0), 12.0, color, k > 0);
        }
    }
    frame(c, rect(69.0, 387.0, 735.0, 508.0), BOX, RUST);
    c.spawn(Node {
        flex_direction: FlexDirection::Column,
        row_gap: px(6),
        padding: UiRect::all(px(8)),
        ..rect(71.0, 389.0, 733.0, 506.0)
    })
    .with_children(|info| {
        let map = world.mission().map_or(ui.config.map.as_str(), |r| r.def.map);
        info.spawn(kit.text(
            &format!("{}  |  seed {}  |  {}", map_title(map), sim.setup.seed, clock(world.tick())),
            13.0,
            GOLD,
            true,
        ));
        if let Some((g, xp)) = ui.earned {
            info.spawn(kit.text(&format!("{} earned {xp} XP.", world.rules.general(g).name), 13.0, WHITE, false));
        }
        if let Some(run) = world.mission() {
            let outcome = match run.result {
                Some(true) => "accomplished",
                Some(false) => "failed",
                None => "abandoned",
            };
            info.spawn(kit.text(&format!("Mission {}: {outcome}.", run.def.name), 13.0, WHITE, true));
            for (o, s) in run.def.objectives.iter().zip(&run.objectives) {
                let (mark, ink) = objective_mark(*s);
                info.spawn(kit.text(&format!("{mark}  {}", o.text), 12.0, ink, false));
            }
        }
    });
    let (again, other, other_label) = if ui.in_mission {
        (Act::StartMission, Act::OpenMissions, "MISSIONS")
    } else {
        (Act::Start, Act::ChangeSetup, "CHANGE SETUP")
    };
    button(c, kit, "PLAY AGAIN", again, Look::Button, rect(68.0, 552.0, 241.0, 588.0), 16.0);
    button(c, kit, other_label, other, Look::Button, rect(252.0, 552.0, 425.0, 588.0), 16.0);
    button(c, kit, "MAIN MENU", Act::QuitToMenu, Look::Button, rect(620.0, 552.0, 748.0, 588.0), 16.0);
}

/// The Generals page, in the skirmish page's frame: one general's portrait and career box on
/// the left, the eight perks down the right with rank, price and the one thing each can do now,
/// PROMOTE and BACK where the skirmish page has START GAME and BACK.
fn generals(c: &mut ChildSpawnerCommands, kit: &Kit, ui: &Ui, profile: &Profile) {
    let rules = g2_rules::rules();
    let g = ui.shown_general;
    let general = rules.general(g);
    let me = profile.progress(g);
    page(c, kit, "GENERALS");
    heading(c, kit, &format!("{} {}", faction_tag(general.faction), general.name), rect(58.0, 87.0, 291.0, 111.0));
    frame(c, rect(58.0, 114.0, 258.0, 284.0), Color::BLACK, RUST);
    portrait(c, kit, general.faction, Some(g), rect(59.0, 115.0, 257.0, 283.0));
    button(c, kit, "<", Act::CycleShownGeneral(-1), Look::Button, rect(58.0, 290.0, 98.0, 314.0), 14.0);
    label(c, kit, faction_name(general.faction), rect(98.0, 290.0, 218.0, 314.0), 11.0, WHITE, true);
    button(c, kit, ">", Act::CycleShownGeneral(1), Look::Button, rect(218.0, 290.0, 258.0, 314.0), 14.0);
    frame(c, rect(58.0, 330.0, 291.0, 493.0), BOX, RUST);
    let promotion =
        if me.rank < MAX_RANK { format!("{} XP", promotion_cost(me.rank)) } else { "highest rank".to_owned() };
    let lines = [
        ("Rank", format!("{} of {MAX_RANK}", me.rank)),
        ("Experience", format!("{} XP", me.xp)),
        ("Perk slots", format!("{} of {}", me.slotted.len(), me.rank)),
        ("Promotion", promotion),
        ("Power", general.power.map_or("None".to_owned(), |p| p.name.to_owned())),
    ];
    stats(c, kit, (64.0, 160.0, 287.0), 338.0, &lines);
    for (s, x0, x1) in
        [("Perk", 303.0, 440.0), ("Effect", 443.0, 600.0), ("Rank", 603.0, 636.0), ("Cost", 644.0, 690.0)]
    {
        heading(c, kit, s, rect(x0, 87.0, x1, 111.0));
    }
    for (k, &perk_id) in general.perks.iter().enumerate() {
        let perk = rules.perk(perk_id);
        let y = 113.0 + 33.0 * k as f32;
        let (color, act, verb) = if me.slotted.contains(&perk_id) {
            (GOLD, Act::UnslotPerk(perk_id), "UNSLOT")
        } else if me.bought.contains(&perk_id) {
            (WHITE, Act::SlotPerk(perk_id), "SLOT")
        } else {
            (INK_OFF, Act::BuyPerk(perk_id), "BUY")
        };
        label(c, kit, perk.name, rect(303.0, y, 440.0, y + 31.0), 12.0, color, false);
        label(c, kit, perk.description, rect(443.0, y, 600.0, y + 31.0), 10.0, WHITE, false);
        label(c, kit, &perk.rank.to_string(), rect(603.0, y, 640.0, y + 31.0), 12.0, WHITE, true);
        label(c, kit, &perk.price.to_string(), rect(640.0, y, 690.0, y + 31.0), 12.0, WHITE, true);
        button(c, kit, verb, act, Look::Button, rect(692.0, y + 4.0, 745.0, y + 27.0), 11.0);
    }
    frame(c, rect(303.0, 380.0, 745.0, 493.0), BOX, RUST);
    c.spawn(Node {
        flex_direction: FlexDirection::Column,
        row_gap: px(6),
        padding: UiRect::all(px(6)),
        ..rect(305.0, 382.0, 743.0, 491.0)
    })
    .with_children(|info| {
        if let Some(why) = ui.refused {
            info.spawn(kit.text(refusal(why), 13.0, INK_HOT, true));
        }
        info.spawn(kit.text(
            "Play a skirmish as this general to earn XP: 100 for a match, 400 more for a win, 1 a kill. \
                 Promotions open perk slots; slotted perks apply in every match.",
            12.0,
            WHITE,
            false,
        ));
    });
    if me.rank < MAX_RANK {
        button(c, kit, "PROMOTE", Act::Promote, Look::Button, rect(94.0, 509.0, 268.0, 545.0), 16.0);
    }
    button(c, kit, "BACK", Act::BackToMain, Look::Button, rect(530.0, 509.0, 701.0, 545.0), 16.0);
}

/// The campaign page, as Zero Hour's Generals' Challenge (`ChallengeMenu.wnd`): its frame,
/// title and rule; where it has the generals on a world map, the missions down the left and
/// the picked one's map and briefing; its biography box below, holding the mission's
/// objectives and the player's side with its portrait where Generals has the general's; and
/// START MISSION and BACK at its Play and Back buttons. A mission opens once the one before it
/// is won.
fn missions(c: &mut ChildSpawnerCommands, kit: &Kit, ui: &Ui, profile: &Profile, preview: Option<Handle<Image>>) {
    let rules = g2_rules::rules();
    let picked = rules.mission(ui.mission);
    frame(c, rect(41.0, 40.0, 760.0, 561.0), PANEL, GOLD);
    c.spawn(Node { align_items: AlignItems::Center, ..rect(63.0, 43.0, 496.0, 78.0) })
        .with_child(kit.text("MISSIONS", 26.0, GOLD, true));
    c.spawn((rect(42.0, 78.0, 760.0, 79.0), BackgroundColor(GOLD)));
    // The campaign, in order.
    heading(c, kit, "Campaign", rect(58.0, 87.0, 290.0, 111.0));
    for (i, m) in rules.missions.iter().enumerate() {
        let y = 112.0 + 62.0 * i as f32;
        let open = profile.unlocked(rules, i);
        let state = if profile.won(m.key) {
            "Won"
        } else if open {
            "Open"
        } else {
            "Locked"
        };
        // The map's own name, without the game it comes from, so a row fits two lines.
        let map = m.map.split_once(':').map_or(m.map, |(_, n)| n);
        let row =
            format!("{}. {}  -  {state}\n{}, {map}", i + 1, m.name.to_uppercase(), faction_tag(m.sides[0].faction));
        let on = i == ui.mission.0 as usize;
        button(c, kit, &row, Act::PickMission(i as u16), Look::Row { on }, rect(58.0, y, 290.0, y + 56.0), 13.0);
        if !open {
            c.spawn((rect(58.0, y, 290.0, y + 56.0), BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45))));
        }
    }
    // The picked mission: its map and briefing.
    heading(c, kit, picked.name, rect(306.0, 87.0, 590.0, 111.0));
    c.spawn(Node { padding: UiRect::all(px(4)), ..rect(302.0, 112.0, 594.0, 370.0) }).with_child(kit.text(
        picked.briefing,
        13.0,
        WHITE,
        false,
    ));
    frame(c, rect(604.0, 114.0, 745.0, 255.0), Color::BLACK, RUST);
    if let Some(img) = preview {
        c.spawn((ImageNode::new(img), rect(605.0, 115.0, 744.0, 254.0)));
    }
    label(c, kit, &map_title(picked.map), rect(598.0, 256.0, 751.0, 280.0), 12.0, WHITE, true);
    let won = rules.missions.iter().filter(|m| profile.won(m.key)).count();
    stats(
        c,
        kit,
        (606.0, 676.0, 751.0),
        290.0,
        &[
            ("Enemy", faction_tag(picked.sides[1].faction).to_owned()),
            ("Won", format!("{won} of {}", rules.missions.len())),
        ],
    );
    // The biography box: objectives, and the side the player commands.
    frame(c, rect(199.0, 379.0, 747.0, 493.0), BOX, RUST);
    label(c, kit, "Objectives", rect(203.0, 383.0, 400.0, 406.0), 14.0, GOLD, false);
    for (k, o) in picked.objectives.iter().enumerate() {
        let y = 405.0 + 17.0 * k as f32;
        label(c, kit, &format!("\u{2022}  {}", o.text), rect(210.0, y, 635.0, y + 17.0), 12.0, WHITE, false);
    }
    frame(c, rect(641.0, 386.0, 738.0, 486.0), Color::BLACK, RUST);
    portrait(c, kit, picked.sides[0].faction, picked.sides[0].general, rect(642.0, 387.0, 737.0, 485.0));
    label(c, kit, faction_name(picked.sides[0].faction), rect(48.0, 383.0, 195.0, 406.0), 12.0, GOLD, false);
    label(
        c,
        kit,
        &format!("${} to start", thousands(picked.sides[0].credits)),
        rect(48.0, 405.0, 195.0, 422.0),
        12.0,
        WHITE,
        false,
    );
    if profile.unlocked(rules, ui.mission.0 as usize) {
        button(c, kit, "START MISSION", Act::StartMission, Look::Button, rect(382.0, 505.0, 554.0, 541.0), 16.0);
    } else {
        label(c, kit, "Win the mission before this one first.", rect(320.0, 505.0, 570.0, 541.0), 13.0, INK_OFF, true);
    }
    button(c, kit, "BACK", Act::BackToMain, Look::Button, rect(576.0, 505.0, 748.0, 541.0), 16.0);
}

/// An objective's mark and colour: open, done (green) or failed (red).
pub(crate) fn objective_mark(s: g2_sim::mission::Status) -> (&'static str, Color) {
    match s {
        g2_sim::mission::Status::Active => ("\u{25a1}", WHITE),
        g2_sim::mission::Status::Done => ("\u{2713}", Color::srgb_u8(90, 230, 90)),
        g2_sim::mission::Status::Failed => ("\u{2717}", Color::srgb_u8(240, 70, 50)),
    }
}

/// A general's picture: the alpha portrait when imported, else a tile in the faction's colour
/// with its tag.
fn portrait(c: &mut ChildSpawnerCommands, kit: &Kit, faction: Faction, general: Option<GeneralId>, r: Node) {
    match kit.art(&crate::front::portrait(faction, general)) {
        Some(img) => {
            c.spawn((ImageNode::new(img), r));
        }
        None => {
            let colour = crate::models::TEAM_PALETTE[faction as usize].with_alpha(0.55);
            c.spawn((
                Node { align_items: AlignItems::Center, justify_content: JustifyContent::Center, ..r },
                BackgroundColor(colour),
            ))
            .with_child(kit.text(faction_tag(faction), 40.0, WHITE, true));
        }
    }
}

/// An AI seat's name in the Player column, as Generals' "Easy Army" and so on.
fn ai_name(level: AiLevel) -> &'static str {
    match level {
        AiLevel::Easy => "Easy AI",
        AiLevel::Normal => "Normal AI",
        AiLevel::Onslaught => "Onslaught",
    }
}

/// A map's name as the menus show it: the original game's maps without their `gen:` prefix,
/// Zero Hour's marked as such.
fn map_title(name: &str) -> String {
    match name.split_once(':') {
        Some(("zh", n)) => format!("{n} (Zero Hour)"),
        Some((_, n)) => n.to_owned(),
        None => name.to_owned(),
    }
}

/// `12500` as `12,500`.
fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if n < 0 {
        out.push('-');
    }
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn faction_name(f: Faction) -> &'static str {
    match f {
        Faction::Eu => "European Union",
        Faction::Apa => "Asia-Pacific Alliance",
        Faction::Gla => "Global Liberation Army",
    }
}

fn refusal(r: Refused) -> &'static str {
    match r {
        Refused::MaxRank => "Already at the highest rank.",
        Refused::NotEnoughXp => "Not enough XP.",
        Refused::RankTooLow => "This perk needs a higher rank.",
        Refused::NeedsLowerTier => "Buy the lower tier first.",
        Refused::NotThisGenerals => "That perk belongs to another general.",
        Refused::NotBought => "Buy the perk before slotting it.",
        Refused::NoFreeSlot => "No free slot: unslot a perk or promote.",
        Refused::AlreadySlotted => "Already slotted.",
    }
}

fn faction_tag(f: Faction) -> &'static str {
    match f {
        Faction::Eu => "EU",
        Faction::Apa => "APA",
        Faction::Gla => "GLA",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    /// A minimal app with the menu logic and a bare entity standing in for the window, which
    /// the real app must never lose when a match is swapped.
    fn app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(Sim::new(
                g2_rules::crater_ridge(),
                None,
                1,
                Sim::players([Faction::Eu, Faction::Gla], true),
            ))
            .init_resource::<crate::input::Selection>()
            .init_resource::<crate::input::Placement>()
            .init_resource::<crate::input::PowerAim>()
            .init_resource::<crate::sim::Previous>()
            .init_resource::<crate::sim::Hits>()
            .init_resource::<crate::sim::Deliveries>()
            .init_resource::<crate::view::Mirror>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Profile>()
            .init_resource::<crate::settings::Settings>()
            .add_plugins(MenuPlugin { blocking: true });
        let window = app.world_mut().spawn_empty().id();
        (app, window)
    }

    fn act(app: &mut App, a: Act) {
        app.world_mut().resource_mut::<Pending>().0.push(a);
        app.update();
    }

    fn screen(app: &App) -> Screen {
        app.world().resource::<Ui>().screen
    }

    fn scene_count(app: &mut App) -> usize {
        app.world_mut().query_filtered::<(), With<MatchEntity>>().iter(app.world()).count()
    }

    /// Starting a skirmish builds the configured match, replaces the old world and scene, and
    /// leaves everything that isn't the match (the window) alone.
    #[test]
    fn starting_a_skirmish_swaps_the_world_and_only_the_scene() {
        let (mut app, window) = app();
        let old_scene = app.world_mut().spawn(MatchEntity).id();
        act(&mut app, Act::OpenSetup);
        assert_eq!(screen(&app), Screen::Setup);
        act(&mut app, Act::CycleSide(1));
        act(&mut app, Act::CycleCredits);
        act(&mut app, Act::Start);
        assert_eq!(screen(&app), Screen::Match);
        let w = app.world();
        assert!(w.get_entity(old_scene).is_err(), "the old scene is gone");
        assert!(w.get_entity(window).is_ok(), "the window survives a swap");
        let sim = w.resource::<Sim>();
        let first_gla = next_general(Faction::Gla, None);
        assert_eq!((sim.world.players()[1].faction, sim.world.players()[1].general), (Faction::Gla, first_gla));
        assert_eq!(sim.world.player(PlayerId(0)).credits, 10_000, "credits from the setup screen");
        assert_eq!(sim.world.tick(), 0, "a fresh world");
        assert_eq!(sim.speed, 1);
        assert_eq!(w.resource::<MatchEpoch>().0, 1);
    }

    /// Each Start despawns the previous match's scene, so restarting never stacks scenes.
    #[test]
    fn restarting_never_stacks_scenes() {
        let (mut app, _) = app();
        for _ in 0..3 {
            act(&mut app, Act::Start);
            app.world_mut().spawn(MatchEntity);
            act(&mut app, Act::QuitToMenu);
        }
        act(&mut app, Act::Start);
        assert_eq!(scene_count(&mut app), 0, "the start despawned the previous scene");
    }

    /// Pause and resume freeze and thaw the match; surrender ends it and the end screen shows.
    #[test]
    fn pause_freezes_and_surrender_reaches_the_end_screen() {
        let (mut app, _) = app();
        act(&mut app, Act::Start);
        act(&mut app, Act::Pause);
        assert_eq!(app.world().resource::<Sim>().speed, 0);
        assert!(app.world().resource::<Ui>().paused);
        act(&mut app, Act::Resume);
        assert_eq!(app.world().resource::<Sim>().speed, 1);
        act(&mut app, Act::Surrender);
        for _ in 0..8 {
            app.world_mut().resource_mut::<Sim>().world.step();
        }
        app.update();
        assert_eq!(screen(&app), Screen::End);
        let sim = app.world().resource::<Sim>();
        assert_eq!(sim.world.winner(), Some(PlayerId(1)));
        assert!(sim.world.stats(PlayerId(0)).defeated_at.is_some());
    }

    /// Setup edits respect the map: seats stay within its starts and at least two, the
    /// player's own row has no AI level, an AI steps Easy, Normal, Onslaught and back, a
    /// general only fits its own faction, and a Team cell steps no team, 1 to 4, and back.
    #[test]
    fn setup_edits_stay_valid() {
        let (mut app, _) = app();
        let slots = |app: &App| app.world().resource::<Ui>().config.slots.clone();
        act(&mut app, Act::OpenSetup);
        act(&mut app, Act::CyclePlayer(2));
        assert_eq!(slots(&app).len(), 2, "Crater Ridge has two starts");
        act(&mut app, Act::CyclePlayer(0));
        assert_eq!(slots(&app)[0].ai, None, "the human has no level");
        let levels: Vec<_> = (0..4)
            .map(|_| {
                act(&mut app, Act::CyclePlayer(1));
                slots(&app)[1].ai
            })
            .collect();
        let (easy, normal, onslaught) = (Some(AiLevel::Easy), Some(AiLevel::Normal), Some(AiLevel::Onslaught));
        assert_eq!(levels, [onslaught, easy, normal, onslaught], "two players stay: Onslaught wraps to Easy");
        // The Faction cell steps through each faction and then its generals, as the LAN page's.
        let mut generals = Vec::new();
        loop {
            act(&mut app, Act::CycleSide(0));
            match slots(&app)[0].clone() {
                Slot { faction: Faction::Eu, general: Some(g), .. } => generals.push(g),
                s => {
                    assert_eq!((s.faction, s.general), (Faction::Apa, None), "EU's generals, then APA");
                    break;
                }
            }
        }
        assert!(!generals.is_empty() && generals.iter().all(|&g| g2_rules::rules().general(g).faction == Faction::Eu));
        let teams: Vec<u8> = (0..5)
            .map(|_| {
                act(&mut app, Act::CycleTeam(1));
                slots(&app)[1].team
            })
            .collect();
        assert_eq!(teams, [1, 2, 3, 4, 0], "the Team cell steps 1-4 and back to none");
    }

    /// A many-start map takes more players through its Open seats, the last one closes again
    /// past Onslaught, and the match that starts has exactly that many. The map selection's
    /// BACK puts the map from before it opened back, with its seats.
    #[test]
    fn a_big_map_hosts_more_players() {
        let (mut app, _) = app();
        let maps = g2_maps::skirmish_maps(&g2_maps::vendored_root());
        let i = maps.iter().position(|m| m.name == "gen:Tournament Tundra").expect("listed");
        let config = |app: &App| app.world().resource::<Ui>().config.clone();
        act(&mut app, Act::OpenSetup);
        act(&mut app, Act::ToggleMapList);
        act(&mut app, Act::MapPick(i));
        act(&mut app, Act::ToggleMapList);
        for seat in 2..6 {
            act(&mut app, Act::CyclePlayer(seat));
        }
        assert_eq!(config(&app).slots.len(), 4, "four starts, four seats");
        act(&mut app, Act::ToggleMapList);
        act(&mut app, Act::MapPick(maps.iter().position(|m| m.starts == 2).expect("a 1v1 map")));
        assert_eq!(config(&app).slots.len(), 2, "a smaller pick drops the extra seats");
        act(&mut app, Act::CancelMapList);
        assert_eq!((config(&app).map, config(&app).slots.len()), ("gen:Tournament Tundra".to_owned(), 4));
        // A new seat starts at Normal: Onslaught, then closed.
        for _ in 0..2 {
            act(&mut app, Act::CyclePlayer(3));
        }
        assert_eq!(config(&app).slots.len(), 3, "the last seat closes after Onslaught");
        act(&mut app, Act::Start);
        assert_eq!(app.world().resource::<Sim>().world.players().len(), 3);
    }

    /// Teams set on the skirmish page play as set: a 2v2 starts with them, and a match with
    /// every player on one team is refused (shown, nothing starts) until a team changes.
    #[test]
    fn teams_reach_the_match_and_one_team_alone_is_refused() {
        let (mut app, _) = app();
        let maps = g2_maps::skirmish_maps(&g2_maps::vendored_root());
        let i = maps.iter().position(|m| m.name == "gen:Tournament Tundra").expect("listed");
        act(&mut app, Act::OpenSetup);
        act(&mut app, Act::ToggleMapList);
        act(&mut app, Act::MapPick(i));
        act(&mut app, Act::ToggleMapList);
        act(&mut app, Act::CyclePlayer(2));
        act(&mut app, Act::CyclePlayer(3));
        for seat in 0..4 {
            act(&mut app, Act::CycleTeam(seat));
        }
        act(&mut app, Act::Start);
        assert_eq!(screen(&app), Screen::Setup, "four allies have no one to fight");
        assert!(app.world().resource::<Ui>().start_refused.is_some(), "and the page says why");
        assert_eq!(app.world().resource::<Sim>().world.players().len(), 2, "the old world stays");
        for seat in [2, 3] {
            act(&mut app, Act::CycleTeam(seat));
        }
        assert!(app.world().resource::<Ui>().start_refused.is_none(), "the next action clears it");
        act(&mut app, Act::Start);
        assert_eq!(screen(&app), Screen::Match);
        let w = &app.world().resource::<Sim>().world;
        assert_eq!(w.players().iter().map(|p| p.team).collect::<Vec<_>>(), [1, 1, 2, 2]);
        assert!(w.allied(PlayerId(0), PlayerId(1)) && !w.allied(PlayerId(1), PlayerId(2)));
    }

    /// Orders and camera input are accepted only while a match runs unpaused.
    #[test]
    fn gameplay_input_follows_the_screen() {
        let (mut app, _) = app();
        let playing_now = |app: &mut App| app.world_mut().run_system_once(playing).unwrap();
        assert!(!playing_now(&mut app), "the main menu covers the battlefield");
        act(&mut app, Act::Start);
        assert!(playing_now(&mut app));
        act(&mut app, Act::Pause);
        assert!(!playing_now(&mut app));
    }

    /// Esc with the general's power armed (or a building being placed) cancels that and opens
    /// no menu, whichever order the input and menu systems run in; a new match never starts
    /// with a power still armed from the last one.
    #[test]
    fn esc_disarms_a_power_and_a_new_match_starts_unarmed() {
        for input_first in [false, true] {
            let (mut app, _) = app();
            // The input system that reads Esc, ordered before or after the menu (the real app
            // leaves them unordered).
            let keyboard = crate::input::keyboard.in_set(crate::input::InputSet).run_if(playing);
            app.add_systems(Update, keyboard);
            if input_first {
                app.configure_sets(Update, MenuSet.after(crate::input::InputSet));
            } else {
                app.configure_sets(Update, MenuSet.before(crate::input::InputSet));
            }
            act(&mut app, Act::Start);
            let esc = |app: &mut App| {
                app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
                app.update();
                app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
            };
            let paused = |app: &App| app.world().resource::<Ui>().paused;
            app.world_mut().resource_mut::<crate::input::PowerAim>().0 = Some(0);
            esc(&mut app);
            assert_eq!(app.world().resource::<crate::input::PowerAim>().0, None, "Esc disarms ({input_first})");
            assert!(!paused(&app), "and opens no menu ({input_first})");
            app.world_mut().resource_mut::<crate::input::Placement>().0 = Some(g2_rules::ids::EU_BARRACKS);
            esc(&mut app);
            assert!(app.world().resource::<crate::input::Placement>().0.is_none());
            assert!(!paused(&app), "a placement too ({input_first})");
            esc(&mut app);
            assert!(paused(&app), "the next Esc pauses ({input_first})");
        }
        let (mut app, _) = app();
        act(&mut app, Act::Start);
        app.world_mut().resource_mut::<crate::input::PowerAim>().0 = Some(0);
        act(&mut app, Act::Surrender);
        act(&mut app, Act::Start);
        assert_eq!(app.world().resource::<crate::input::PowerAim>().0, None, "the new match starts unarmed");
    }

    /// The Generals screen promotes, buys and slots on the general it shows; refusals are
    /// shown and change nothing.
    #[test]
    fn the_generals_screen_spends_experience() {
        use g2_rules::generals::{BULLDOG, JUNKYARD};
        use g2_rules::perks::*;
        let (mut app, _) = app();
        app.world_mut().resource_mut::<Profile>().earn(JUNKYARD, 2_000);
        act(&mut app, Act::OpenGenerals);
        assert_eq!(screen(&app), Screen::Generals);
        assert_eq!(app.world().resource::<Ui>().shown_general, BULLDOG);
        act(&mut app, Act::CycleShownGeneral(-1));
        let last = GeneralId(g2_rules::rules().generals.len() as u16 - 1);
        assert_eq!(app.world().resource::<Ui>().shown_general, last, "wraps around");
        act(&mut app, Act::CycleShownGeneral(JUNKYARD.0 as i32 + 1));
        assert_eq!(app.world().resource::<Ui>().shown_general, JUNKYARD);
        act(&mut app, Act::BuyPerk(JY_LEADERSHIP));
        assert_eq!(app.world().resource::<Ui>().refused, Some(Refused::RankTooLow));
        act(&mut app, Act::BuyPerk(JY_DISCIPLINE));
        act(&mut app, Act::SlotPerk(JY_DISCIPLINE));
        act(&mut app, Act::Promote);
        assert_eq!(app.world().resource::<Ui>().refused, None);
        let p = app.world().resource::<Profile>().progress(JUNKYARD);
        let price = g2_rules::rules().perk(JY_DISCIPLINE).price;
        assert_eq!((p.rank, p.xp, p.slotted), (2, 2_000 - price - promotion_cost(1), vec![JY_DISCIPLINE]));
    }

    /// A match started as a general carries the profile's slotted perks for the human and the
    /// AI's own for an AI general; finishing it earns the human's general experience once.
    #[test]
    fn matches_use_slotted_perks_and_earn_experience() {
        use g2_rules::generals::{BULLDOG, JUNKYARD};
        use g2_rules::perks::*;
        let (mut app, _) = app();
        {
            let mut profile = app.world_mut().resource_mut::<Profile>();
            profile.earn(BULLDOG, 1_000);
            profile.buy(g2_rules::rules(), BULLDOG, BD_INSOURCED).unwrap_err();
            profile.buy(g2_rules::rules(), BULLDOG, BD_PROTECTOR_I).unwrap();
            profile.slot(BULLDOG, BD_PROTECTOR_I).unwrap();
        }
        let xp = app.world().resource::<Profile>().progress(BULLDOG).xp;
        act(&mut app, Act::CycleSide(0));
        act(&mut app, Act::CycleSide(1));
        act(&mut app, Act::Start);
        let w = &app.world().resource::<Sim>().world;
        assert_eq!(w.player(PlayerId(0)).perks, vec![BD_PROTECTOR_I]);
        assert_eq!(w.player(PlayerId(1)).general, Some(JUNKYARD));
        assert_eq!(w.player(PlayerId(1)).perks, ai_perks(g2_rules::rules(), Some(JUNKYARD), AiLevel::Normal));
        assert!(!w.player(PlayerId(1)).perks.is_empty());
        act(&mut app, Act::Surrender);
        for _ in 0..8 {
            app.world_mut().resource_mut::<Sim>().world.step();
        }
        app.update();
        app.update();
        assert_eq!(screen(&app), Screen::End);
        let earned = match_experience(false, 0);
        assert_eq!(app.world().resource::<Ui>().earned, Some((BULLDOG, earned)));
        assert_eq!(app.world().resource::<Profile>().progress(BULLDOG).xp, xp + earned, "earned once");
    }

    /// Updates both apps, with a pause for the socket threads, until `done` holds.
    fn both_until(apps: &mut [App; 2], what: &str, done: impl Fn(&[App; 2]) -> bool) {
        let started = std::time::Instant::now();
        while !done(apps) {
            assert!(started.elapsed() < std::time::Duration::from_secs(20), "never {what}");
            for a in apps.iter_mut() {
                a.update();
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    /// The NETWORK pages through their acts, two apps on this machine: one types its name and
    /// hosts co-op Onslaught, the other types the address and connects, picks its side and
    /// accepts; the host's START GAME then starts the same three-player match on both, the
    /// host as player 0 and the joiner as player 1 on one team.
    #[test]
    fn hosting_and_joining_a_coop_game_through_the_lobby() {
        use bevy::input::keyboard::{Key, KeyboardInput};
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let mut apps = [app().0, app().0];
        let type_in = |app: &mut App, s: &str| {
            for c in s.chars() {
                app.world_mut().write_message(KeyboardInput {
                    key_code: KeyCode::KeyA,
                    logical_key: Key::Character(c.to_string().into()),
                    state: bevy::input::ButtonState::Pressed,
                    text: None,
                    repeat: false,
                    window: Entity::PLACEHOLDER,
                });
            }
            app.update();
        };
        let lobby = |app: &App| app.world().resource::<Ui>().lobby.clone();
        for a in apps.iter_mut() {
            act(a, Act::OpenLobby);
            assert_eq!(screen(a), Screen::Lobby);
        }
        // The host: a name typed (Backspace takes the last letter back), the port, co-op.
        act(&mut apps[0], Act::Type(Some(crate::lobby::Entry::Name)));
        apps[0].world_mut().resource_mut::<Ui>().lobby.name.clear();
        type_in(&mut apps[0], "Hostx");
        apps[0].world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Backspace,
            logical_key: Key::Backspace,
            state: bevy::input::ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        apps[0].update();
        act(&mut apps[0], Act::Type(Some(crate::lobby::Entry::Port)));
        apps[0].world_mut().resource_mut::<Ui>().lobby.port.clear();
        type_in(&mut apps[0], &format!("{port}x"));
        assert_eq!((lobby(&apps[0]).name.as_str(), lobby(&apps[0]).port), ("Host", port.to_string()), "digits only");
        act(&mut apps[0], Act::HostGame);
        assert_eq!(screen(&apps[0]), Screen::NetGame, "{:?}", lobby(&apps[0]).note);
        act(&mut apps[0], Act::NetMode);
        assert!(lobby(&apps[0]).coop);
        // The joiner: no address yet is refused; then typed in, connect.
        act(&mut apps[1], Act::DirectConnect);
        assert_eq!((screen(&apps[1]), lobby(&apps[1]).note.is_some()), (Screen::Lobby, true));
        act(&mut apps[1], Act::Type(Some(crate::lobby::Entry::Address)));
        type_in(&mut apps[1], &format!("127.0.0.1:{port}"));
        act(&mut apps[1], Act::DirectConnect);
        assert_eq!(screen(&apps[1]), Screen::NetGame);
        let joined = |apps: &[App; 2]| match apps[0].world().resource::<crate::lobby::Net>() {
            crate::lobby::Net::Hosting(h) => h.guest().is_some(),
            _ => false,
        };
        both_until(&mut apps, "seated", |a| {
            joined(a) && a[1].world().resource::<crate::lobby::Net>().game().is_some_and(|g| g.0.players.len() == 3)
        });
        act(&mut apps[1], Act::NetSide);
        act(&mut apps[1], Act::NetAccept);
        both_until(&mut apps, "accepted", |a| match a[0].world().resource::<crate::lobby::Net>() {
            crate::lobby::Net::Hosting(h) => h.guest_ready(),
            _ => false,
        });
        act(&mut apps[0], Act::NetStart);
        both_until(&mut apps, "started", |a| a.iter().all(|a| screen(a) == Screen::Match));
        let [h, j] = &apps;
        let (h, j) = (h.world().resource::<Sim>(), j.world().resource::<Sim>());
        assert_eq!(h.setup.players, j.setup.players);
        assert_eq!(h.setup.players.len(), 3, "co-op: the waves are the third player");
        assert_eq!(j.setup.players[1].faction, lobby(&apps[1]).faction, "the joiner's pick");
        assert_eq!((h.local, j.local), (PlayerId(0), PlayerId(1)));
        assert!(h.world.allied(PlayerId(0), PlayerId(1)));
        assert!(h.online.is_some() && j.online.is_some());
        assert_eq!(h.world.state_hash(), j.world.state_hash());
    }

    /// The options page edits the live settings: BACK puts back what was there when it opened,
    /// ACCEPT keeps the changes and writes the file, and either returns to where the page was
    /// opened, the in-game menu still paused over the match.
    #[test]
    fn options_apply_at_once_and_back_undoes_them() {
        use crate::settings::{Battlefield, Settings};
        let (mut app, _) = app();
        let path = std::env::temp_dir().join(format!("g2-options-{}", std::process::id())).join("settings");
        app.world_mut().resource_mut::<Settings>().path = Some(path.clone());
        act(&mut app, Act::OpenOptions);
        assert_eq!(screen(&app), Screen::Options);
        act(&mut app, Act::CycleOption(Opt::Camera));
        assert_eq!(app.world().resource::<Settings>().camera, Battlefield::Medium, "applies at once");
        act(&mut app, Act::CancelOptions);
        assert_eq!((screen(&app), app.world().resource::<Settings>().camera), (Screen::Main, Battlefield::Close));
        assert!(!path.exists(), "BACK saves nothing");

        act(&mut app, Act::Start);
        act(&mut app, Act::Pause);
        act(&mut app, Act::OpenOptions);
        act(&mut app, Act::CycleOption(Opt::Camera));
        act(&mut app, Act::CycleOption(Opt::Camera));
        act(&mut app, Act::AcceptOptions);
        let ui = app.world().resource::<Ui>();
        assert_eq!((ui.screen, ui.paused), (Screen::Match, true), "back to the in-game menu");
        assert_eq!(Settings::load(Some(path.clone())).camera, Battlefield::Far, "ACCEPT saves");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// The campaign: only the first mission opens at first; starting one plays it on its own
    /// map and forces; winning it is saved and opens the next, and PLAY AGAIN replays the
    /// mission, not a skirmish.
    #[test]
    fn missions_start_and_a_win_opens_the_next() {
        let (mut app, _) = app();
        let rules = g2_rules::rules();
        act(&mut app, Act::OpenMissions);
        assert_eq!(screen(&app), Screen::Missions);
        act(&mut app, Act::PickMission(1));
        act(&mut app, Act::StartMission);
        assert_eq!(screen(&app), Screen::Missions, "the second mission is locked");
        act(&mut app, Act::PickMission(0));
        act(&mut app, Act::StartMission);
        assert_eq!(screen(&app), Screen::Match);
        let sim = app.world().resource::<Sim>();
        let run = sim.world.mission().expect("a mission is running");
        assert_eq!(run.def.key, rules.missions[0].key);
        assert_eq!(sim.setup.map.name, rules.missions[0].map);
        assert!(!sim.world.is_ai(sim.local), "the player plays their seat");
        assert!(app.world().resource::<Ui>().in_mission);

        // The enemy gives up: its headquarters goes with the rest of it, which is the
        // mission's objective.
        let hq = run.tagged(2).unwrap();
        let mut sim = app.world_mut().resource_mut::<Sim>();
        sim.world.submit(PlayerId(1), CommandKind::Surrender);
        while sim.world.winner().is_none() && sim.world.tick() < 30 {
            sim.world.step();
        }
        assert!(sim.world.entity(hq).is_none());
        assert_eq!(sim.world.winner(), Some(PlayerId(0)));
        app.update();
        assert_eq!(screen(&app), Screen::End);
        assert!(app.world().resource::<Profile>().won(rules.missions[0].key));
        act(&mut app, Act::StartMission);
        assert_eq!(app.world().resource::<Sim>().world.tick(), 0, "PLAY AGAIN: a fresh run of the mission");
        assert!(app.world().resource::<Sim>().world.mission().is_some());
        act(&mut app, Act::Pause);
        act(&mut app, Act::QuitToMenu);
        act(&mut app, Act::OpenMissions);
        assert_eq!(app.world().resource::<Ui>().mission, g2_sim::MissionId(1), "opens on the next mission");
        act(&mut app, Act::StartMission);
        assert_eq!(app.world().resource::<Sim>().world.mission().unwrap().def.key, rules.missions[1].key);

        // A skirmish after a mission is a skirmish again.
        act(&mut app, Act::Pause);
        act(&mut app, Act::QuitToMenu);
        act(&mut app, Act::OpenSetup);
        act(&mut app, Act::Start);
        assert!(app.world().resource::<Sim>().world.mission().is_none());
        assert!(!app.world().resource::<Ui>().in_mission);
    }
}
