//! Menus: main menu, controls, skirmish page, in-match pause and the score screen, laid out as
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
    CycleFaction(usize),
    CycleGeneral(usize),
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
}

/// One skirmish player: slot 0 is the human at the keyboard, the rest are AI.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Slot {
    pub faction: Faction,
    pub general: Option<GeneralId>,
    pub ai: Option<AiLevel>,
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
                Slot { faction: Faction::Eu, general: None, ai: None },
                Slot { faction: Faction::Gla, general: None, ai: Some(AiLevel::Normal) },
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
    /// their own.
    pub fn build(&self, perks: &[PerkId], rank: u8) -> Result<Sim, String> {
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
        self.slots.push(Slot { faction, general: None, ai: Some(AiLevel::Normal) });
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
    /// The experience the finished match earned, for the end screen; set once per match.
    pub earned: Option<(GeneralId, u32)>,
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
            earned: None,
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
/// run without a menu count as playing.
pub fn playing(ui: Option<Res<Ui>>) -> bool {
    ui.is_none_or(|u| u.screen == Screen::Match && !u.paused)
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
        app.init_resource::<Ui>()
            .init_resource::<Pending>()
            .init_resource::<MatchEpoch>()
            .insert_resource(MapListChannel(std::sync::Mutex::new(rx)))
            .add_systems(Startup, load_fonts)
            .add_systems(
                Update,
                (poll_maps, interact, apply, watch_winner, award_experience, rebuild, hover, hide_hud)
                    .chain()
                    .in_set(MenuSet)
                    .run_if(resource_exists::<Sim>),
            );
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
    if keys.just_pressed(KeyCode::Escape) {
        // Esc closes whatever is open: a placement or an armed power first, then a menu.
        let placed = placing.as_mut().is_some_and(|p| p.0.take().is_some());
        let aimed = aim.as_mut().is_some_and(|a| a.0.take().is_some());
        if !placed && !aimed {
            pending.0.push(match (ui.screen, ui.paused) {
                (Screen::Setup, _) if ui.map_select.is_some() => Act::CancelMapList,
                (Screen::Setup | Screen::Controls | Screen::Generals, _) => Act::BackToMain,
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

/// The match HUD is hidden behind the full-screen menus (main, controls, setup, generals, the
/// score screen); the in-game menu keeps it, dimmed, as context.
fn hide_hud(ui: Res<Ui>, mut roots: Query<&mut Visibility, HudRoots>) {
    let hidden = matches!(ui.screen, Screen::Main | Screen::Controls | Screen::Setup | Screen::Generals | Screen::End);
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
    match act {
        Act::QuitGame => {
            world.write_message(AppExit::Success);
        }
        // Starting always rolls a fresh world, so "start" after quitting a match is a new game
        // and never a silent resume of the frozen one.
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
                    ui.config = config;
                    ui.earned = None;
                    ui.screen = Screen::Match;
                    ui.paused = false;
                    world.resource_mut::<Sim>().speed = 1;
                }
                Err(e) => error!("cannot start match: {e}"),
            }
        }
        // Everything below only edits `Ui` and the live `Sim`. The two borrow the world
        // mutably, so arms touch them one at a time.
        _ => match act {
            Act::ShowControls => world.resource_mut::<Ui>().screen = Screen::Controls,
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
                // Leaving a network match ends it: the other player sees the link close.
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
            Act::CycleFaction(i) => {
                if let Some(s) = world.resource_mut::<Ui>().config.slots.get_mut(i) {
                    let f = s.faction as usize;
                    s.faction = Faction::ALL[(f + 1) % Faction::ALL.len()];
                    s.general = None;
                }
            }
            Act::CycleGeneral(i) => {
                if let Some(s) = world.resource_mut::<Ui>().config.slots.get_mut(i) {
                    s.general = next_general(s.faction, s.general);
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
            Act::Start | Act::QuitGame => unreachable!("handled above"),
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
    world.insert_resource(crate::input::Selection::default());
    world.insert_resource(crate::input::Placement::default());
    world.insert_resource(crate::input::PowerAim::default());
    world.resource_mut::<MatchEpoch>().0 += 1;
}

/// The match is over: freeze it and show the results.
fn watch_winner(sim: Res<Sim>, mut ui: ResMut<Ui>) {
    if ui.screen == Screen::Match && sim.world.winner().is_some() {
        ui.screen = Screen::End;
        ui.paused = false;
    }
}

/// A finished match earns the local human's general experience in their profile, once.
/// Watching AIs, or playing without a general, earns nothing.
fn award_experience(sim: Res<Sim>, mut ui: ResMut<Ui>, mut profile: ResMut<Profile>) {
    if ui.screen != Screen::End || ui.earned.is_some() || sim.world.is_ai(sim.local) {
        return;
    }
    let Some(g) = sim.world.player(sim.local).general else { return };
    let xp = match_experience(sim.world.winner() == Some(sim.local), sim.world.stats(sim.local).kills);
    profile.earn(g, xp);
    profile.save(g2_rules::rules());
    ui.earned = Some((g, xp));
}

const MAPS_PER_PAGE: usize = 12;

/// Generals' skirmish page has eight player rows.
const MAX_PLAYERS: usize = 8;

fn map_list(world: &World) -> Option<&Vec<g2_maps::SkirmishMap>> {
    world.get_resource::<MapList>().map(|m| &m.0)
}

fn map_starts(world: &World, map: &str) -> Option<usize> {
    map_list(world).and_then(|maps| maps.iter().find(|m| m.name == map).map(|m| m.starts))
}

fn next_general(faction: Faction, current: Option<GeneralId>) -> Option<GeneralId> {
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

/// The menus' typeface: DejaVu Sans Condensed, bundled (Bitstream Vera licence), in place of
/// the condensed faces Generals' shell is set in.
#[derive(Resource)]
pub struct MenuFonts {
    regular: Handle<Font>,
    bold: Handle<Font>,
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
    maps: Option<Res<MapList>>,
    fonts: Option<Res<MenuFonts>>,
    cache: Option<Res<crate::alpha::Cache>>,
    server: Option<Res<AssetServer>>,
    mut images: Option<ResMut<Assets<Image>>>,
    mut preview: Local<Option<(String, Option<Handle<Image>>)>>,
    old: Query<Entity, With<MenuUi>>,
) {
    if !ui.is_changed() && !profile.is_changed() && !fonts.as_ref().is_some_and(|f| f.is_added()) {
        return;
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    // A running match shows the HUD alone: no overlay, no dimming.
    if ui.screen == Screen::Match && !ui.paused {
        return;
    }
    // The map preview is decoded once per map, not on every rebuild.
    if ui.screen == Screen::Setup && preview.as_ref().is_none_or(|(m, _)| *m != ui.config.map) {
        let handle = crate::front::map_preview(&ui.config.map).and_then(|img| images.as_mut().map(|i| i.add(img)));
        *preview = Some((ui.config.map.clone(), handle));
    }
    let map_image = preview.as_ref().and_then(|(_, h)| h.clone());
    let maps = maps.as_ref().map(|m| m.0.as_slice());
    let kit = Kit { fonts: fonts.as_deref(), server: server.as_deref(), alpha: cache.is_some() };
    // The in-game menu sits on the match as Generals' does; every other screen is framed by
    // the ruler, the battlefield behind it standing in for Generals' shell map.
    let ruled = ui.screen != Screen::Match;
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
        ("SKIRMISH", Act::OpenSetup),
        ("ONSLAUGHT", Act::OpenOnslaught),
        ("GENERALS", Act::OpenGenerals),
        ("CONTROLS", Act::ShowControls),
        ("EXIT GAME", Act::QuitGame),
    ];
    frame(c, rect(532.0, 108.0, 756.0, 120.0 + 40.0 * items.len() as f32), BOX, GOLD);
    for (k, (s, act)) in items.into_iter().enumerate() {
        let y = 116.0 + 40.0 * k as f32;
        button(c, kit, s, act, Look::Button, rect(540.0, y, 748.0, y + 36.0), 16.0);
    }
}

fn controls(c: &mut ChildSpawnerCommands, kit: &Kit) {
    page(c, kit, "CONTROLS");
    for (k, (keys, what)) in [
        ("Left click, drag", "Select units and buildings"),
        ("Right click", "Context order: attack, gather, build, move"),
        ("Ctrl + right click", "Attack-move"),
        ("B, then 1-9", "Build; 1-9 on a factory queues units"),
        ("Backspace", "Cancel the queued item"),
        ("T", "Deploy or pack up artillery; turn a Battle Bus into its bunker"),
        ("R, then click an enemy", "Particle Accelerator (Shadow Tank)"),
        ("G, F1-F3", "General's power, faction support powers"),
        ("Tab", "Select the next collector"),
        ("Space, + / -, Esc", "Pause, game speed, menu"),
        ("M", "Mute or unmute sound"),
    ]
    .into_iter()
    .enumerate()
    {
        let y = 96.0 + 34.0 * k as f32;
        label(c, kit, keys, rect(72.0, y, 290.0, y + 30.0), 14.0, GOLD, false);
        label(c, kit, what, rect(300.0, y, 745.0, y + 30.0), 14.0, WHITE, false);
    }
    button(c, kit, "BACK", Act::BackToMain, Look::Button, rect(530.0, 509.0, 701.0, 545.0), 16.0);
}

/// The in-game menu, as Generals' `QuitMenu.wnd`: the logo over a column of buttons.
fn pause(c: &mut ChildSpawnerCommands, kit: &Kit) {
    frame(c, rect(252.0, 100.0, 552.0, 332.0), DIALOG, GOLD);
    logo(c, kit, rect(262.0, 101.0, 550.0, 180.0));
    for (k, (s, act)) in
        [("SURRENDER", Act::Surrender), ("EXIT TO MAIN MENU", Act::QuitToMenu), ("RETURN TO GAME", Act::Resume)]
            .into_iter()
            .enumerate()
    {
        let y = 183.0 + 48.0 * k as f32;
        button(c, kit, s, act, Look::Button, rect(288.0, y, 512.0, y + 40.0), 16.0);
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
        [("Players", 72.0, 216.0), ("Color", 224.0, 284.0), ("Faction", 292.0, 372.0), ("General", 380.0, 520.0)]
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
                frame(c, at(224.0, 284.0), Look::Cell.fill(false), RUST.with_alpha(0.6));
                c.spawn((rect(229.0, y + 6.0, 279.0, y + 18.0), BackgroundColor(crate::models::TEAM_PALETTE[colour])));
                cell(c, kit, faction_tag(slot.faction), Act::CycleFaction(i), at(292.0, 372.0));
                let general = slot.general.map_or("No General", |g| rules.general(g).name);
                cell(c, kit, general, Act::CycleGeneral(i), at(380.0, 520.0));
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
        "Skirmish: destroy every enemy building and unit. Click an AI player to set it Easy, Normal or \
         Onslaught; an Open seat adds one."
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

/// The score screen, as Generals' `ScoreScreen.wnd`: the outcome across the top band, a row
/// of totals per player under gold column heads (a green mark for the winner), the match's
/// details in the box below, and the buttons along the bottom.
fn end_screen(c: &mut ChildSpawnerCommands, kit: &Kit, sim: &Sim, ui: &Ui) {
    let world = &sim.world;
    let winner = world.winner();
    // Watching two AIs play (the local player is one of them) has no victory to claim.
    let watched = world.is_ai(sim.local);
    let headline = match winner {
        _ if watched => "MATCH OVER",
        Some(w) if w == sim.local => "VICTORY",
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
        let won = winner == Some(id);
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
        info.spawn(kit.text(
            &format!("{}  |  seed {}  |  {}", map_title(&ui.config.map), ui.config.seed, clock(world.tick())),
            13.0,
            GOLD,
            true,
        ));
        if let Some((g, xp)) = ui.earned {
            info.spawn(kit.text(&format!("{} earned {xp} XP.", world.rules.general(g).name), 13.0, WHITE, false));
        }
    });
    button(c, kit, "PLAY AGAIN", Act::Start, Look::Button, rect(68.0, 552.0, 241.0, 588.0), 16.0);
    button(c, kit, "CHANGE SETUP", Act::ChangeSetup, Look::Button, rect(252.0, 552.0, 425.0, 588.0), 16.0);
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
        act(&mut app, Act::CycleFaction(1));
        act(&mut app, Act::CycleCredits);
        act(&mut app, Act::Start);
        assert_eq!(screen(&app), Screen::Match);
        let w = app.world();
        assert!(w.get_entity(old_scene).is_err(), "the old scene is gone");
        assert!(w.get_entity(window).is_ok(), "the window survives a swap");
        let sim = w.resource::<Sim>();
        assert_eq!(sim.world.players()[1].faction, Faction::Eu, "GLA cycles on to EU (Faction::ALL wraps)");
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
    /// player's own row has no AI level, an AI steps Easy, Normal, Onslaught and back, and a
    /// general only fits its own faction.
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
        act(&mut app, Act::CycleGeneral(0));
        let cfg = slots(&app);
        let g = cfg[0].general.expect("EU has a general");
        assert_eq!(g2_rules::rules().general(g).faction, cfg[0].faction);
        act(&mut app, Act::CycleFaction(0));
        assert_eq!(slots(&app)[0].general, None, "a new faction drops the general");
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
        act(&mut app, Act::CycleGeneral(0));
        act(&mut app, Act::CycleGeneral(1));
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
}
