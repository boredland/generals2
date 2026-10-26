//! Menus: main menu, controls, skirmish lobby, in-match pause and the end screen, laid out like
//! the 2013 alpha's front end (docs/STYLE.md, "Menus"; its art comes from `front.rs`).
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

use crate::hud::{DIM, TEXT, clock};
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
    OpenSetup,
    BackToMain,
    QuitGame,
    Start,
    ChangeSetup,
    MapPage(i32),
    MapPick(usize),
    AddSlot,
    RemoveSlot,
    CycleFaction(usize),
    CycleGeneral(usize),
    /// Sets an AI slot's level (the lobby's EASY / NORMAL / ONSLAUGHT pills).
    SetAi(usize, AiLevel),
    /// Opens or closes the lobby's map list over the map preview.
    ToggleMapList,
    /// The main screen's mode bar: every AI slot plays Onslaught, or back to Normal.
    ToggleMode,
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
    /// The lobby's map list is open over the preview.
    pub map_list_open: bool,
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
            map_list_open: false,
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
            .add_systems(Startup, crate::front::load_fonts)
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

/// Hover feedback: a button under the cursor lights up.
fn hover(mut buttons: Query<(&Look, &RelativeCursorPosition, &mut BackgroundColor)>) {
    for (look, cur, mut bg) in &mut buttons {
        bg.set_if_neq(BackgroundColor(look.fill(cur.cursor_over())));
    }
}

/// Top-level match spawns that are UI nodes: the HUD panels, not the 3D scene.
type HudRoots = (With<MatchEntity>, With<Node>);

/// The match HUD is hidden behind the full-screen menus (main, controls, setup, generals);
/// pause and the end screen keep it, dimmed, as context.
fn hide_hud(ui: Res<Ui>, mut roots: Query<&mut Visibility, HudRoots>) {
    let hidden = matches!(ui.screen, Screen::Main | Screen::Controls | Screen::Setup | Screen::Generals);
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
            Act::OpenSetup => world.resource_mut::<Ui>().screen = Screen::Setup,
            Act::BackToMain => {
                let mut ui = world.resource_mut::<Ui>();
                ui.screen = Screen::Main;
                ui.map_list_open = false;
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
            Act::MapPick(i) => {
                if let Some(m) = map_list(world).and_then(|m| m.get(i).cloned()) {
                    let mut ui = world.resource_mut::<Ui>();
                    ui.config.map = m.name;
                    ui.config.clamp_slots(m.starts);
                    ui.map_page = i / MAPS_PER_PAGE;
                    ui.map_list_open = false;
                }
            }
            Act::AddSlot => {
                let max = map_starts(world, &world.resource::<Ui>().config.map).unwrap_or(usize::MAX);
                let mut ui = world.resource_mut::<Ui>();
                if ui.config.slots.len() < max {
                    ui.config.add_slot();
                }
            }
            Act::RemoveSlot => {
                let mut ui = world.resource_mut::<Ui>();
                if ui.config.slots.len() > 2 {
                    ui.config.slots.pop();
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
            Act::SetAi(i, level) => {
                // Slot 0 is the player at the keyboard and has no level to set.
                if let Some(s) = world.resource_mut::<Ui>().config.slots.get_mut(i).filter(|_| i > 0) {
                    s.ai = Some(level);
                }
            }
            Act::ToggleMode => {
                let mut ui = world.resource_mut::<Ui>();
                let onslaught = ui.config.slots.iter().skip(1).any(|s| s.ai == Some(AiLevel::Onslaught));
                let level = if onslaught { AiLevel::Normal } else { AiLevel::Onslaught };
                for s in ui.config.slots.iter_mut().skip(1) {
                    s.ai = Some(level);
                }
            }
            Act::ToggleMapList => {
                let mut ui = world.resource_mut::<Ui>();
                ui.map_list_open = !ui.map_list_open;
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
// `rebuild` replaces the whole menu overlay whenever `Ui` changes. The screens follow the 2013
// alpha's front end as its footage shows it (docs/STYLE.md, "Menus"): a black screen under a
// wide silver title, a big blue PLAY with the mode under it and a tab row on the main screen,
// and a lobby of map preview, team cards and a green START BATTLE. With an imported alpha they
// use its Eurostile fonts and its general, faction and loading-screen art; without one, the
// same layout in plain fills.

/// The alpha's blues and greens, picked from its front end in the footage.
const BLUE: Color = Color::srgb(0.10, 0.38, 0.78);
const BLUE_TOP: Color = Color::srgb(0.20, 0.52, 0.92);
const GREEN: Color = Color::srgb(0.22, 0.58, 0.10);
const GREEN_TOP: Color = Color::srgb(0.40, 0.78, 0.22);
const STEEL: Color = Color::srgba(0.10, 0.11, 0.12, 0.96);
const STEEL_EDGE: Color = Color::srgb(0.22, 0.24, 0.26);
const SILVER: Color = Color::srgb(0.80, 0.82, 0.84);
const ALPHA_RED: Color = Color::srgb(0.85, 0.12, 0.10);

/// What a button looks like and how it lights up under the cursor.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Look {
    /// Steel with a thin edge; blue when hovered or `on`.
    Plain { on: bool },
    /// The big blue PLAY.
    Play,
    /// The big green START BATTLE.
    Go,
    /// A tab of the main screen's row; filled blue when `on`.
    Tab { on: bool },
}

impl Look {
    fn fill(self, hover: bool) -> Color {
        match self {
            Look::Plain { on } | Look::Tab { on } if on => BLUE,
            Look::Plain { .. } if hover => Color::srgb(0.13, 0.27, 0.45),
            Look::Tab { .. } if hover => Color::srgba(0.13, 0.27, 0.45, 0.9),
            Look::Plain { .. } => STEEL,
            Look::Tab { .. } => Color::srgba(0.06, 0.07, 0.08, 0.85),
            Look::Play if hover => BLUE_TOP,
            Look::Play => BLUE,
            Look::Go if hover => GREEN_TOP,
            Look::Go => GREEN,
        }
    }
}

/// Fonts and art for this rebuild: the alpha's when imported.
struct Kit<'a> {
    fonts: Option<&'a crate::front::Fonts>,
    server: Option<&'a AssetServer>,
    images: Option<&'a mut Assets<Image>>,
}

impl Kit<'_> {
    fn text(&self, s: &str, size: f32, color: Color, wide: bool) -> impl Bundle {
        let mut font = TextFont { font_size: FontSize::Px(size), ..default() };
        if let Some(f) = self.fonts {
            font.font = (if wide { f.wide.clone() } else { f.body.clone() }).into();
        }
        (Text::new(s), font, TextColor(color))
    }

    /// An image from the import, or `None` without one.
    fn art(&self, rel: &str) -> Option<Handle<Image>> {
        let server = self.server.filter(|_| self.fonts.is_some())?;
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
    fonts: Option<Res<crate::front::Fonts>>,
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
    let mut kit = Kit { fonts: fonts.as_deref(), server: server.as_deref(), images: images.as_deref_mut() };
    // Full-screen menus hide the battlefield behind the alpha's black; pause and the end screen
    // dim it instead, so the match stays in view.
    let over_match = matches!(ui.screen, Screen::Match | Screen::End);
    let backdrop = if over_match { Color::srgba(0.0, 0.0, 0.0, 0.55) } else { Color::BLACK };
    let mut root = commands.spawn((
        MenuUi,
        GlobalZIndex(10),
        BackgroundColor(backdrop),
        Node { width: percent(100), height: percent(100), flex_direction: FlexDirection::Column, ..default() },
    ));
    root.with_children(|root| {
        if !over_match {
            // The alpha's dark battlefield art behind its menus, faint.
            if let Some(bg) = kit.art("LoadingScreen/LoadingScreen_Background") {
                root.spawn((
                    ImageNode::new(bg).with_color(Color::srgba(1.0, 1.0, 1.0, 0.45)),
                    Node {
                        position_type: PositionType::Absolute,
                        width: percent(100),
                        height: percent(100),
                        ..default()
                    },
                ));
            }
        }
        top_bar(root, &kit);
        // The alpha's pages hang from the title rather than sitting centred; the small dialogs
        // (pause, controls, the end screen) stay in the middle.
        let dialog = matches!(ui.screen, Screen::Match | Screen::Controls | Screen::End);
        root.spawn(Node {
            width: percent(100),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: if dialog { JustifyContent::Center } else { JustifyContent::FlexStart },
            padding: UiRect::top(px(if dialog { 0.0 } else { 14.0 })),
            row_gap: px(14),
            ..default()
        })
        .with_children(|body| match ui.screen {
            Screen::Main => main_menu(body, &kit, &ui, &profile),
            Screen::Controls => controls(body, &kit),
            Screen::Setup => setup(body, &mut kit, &ui, maps.as_ref().map(|m| m.0.as_slice()), map_image),
            Screen::Match => pause(body, &kit),
            Screen::End => end_screen(body, &kit, &sim, &ui),
            Screen::Generals => generals(body, &kit, &ui, &profile),
        });
    });
}

/// The wide silver title across the top and the red build tag in the corner, as on every
/// alpha screen (ours names this project, not EA's game).
fn top_bar(root: &mut ChildSpawnerCommands, kit: &Kit) {
    root.spawn(Node {
        width: percent(100),
        height: px(64),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    })
    .with_children(|bar| {
        bar.spawn(kit.text("O P E N   R T S", 34.0, SILVER, true));
        bar.spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(14),
                top: px(6),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                ..default()
            },
            children![
                kit.text("ALPHA LOOK", 14.0, ALPHA_RED, true),
                kit.text("an open-source RTS", 11.0, ALPHA_RED, false),
            ],
        ));
    });
}

/// A clickable button: `hover` lights it up, a click queues its `Act`.
fn button(parent: &mut ChildSpawnerCommands, kit: &Kit, label: &str, act: Act, width: f32) {
    styled(parent, kit, label, act, width, 34.0, 14.0, Look::Plain { on: false });
}

/// As `button`, but drawn selected (the current map, the active AI level).
fn pick(parent: &mut ChildSpawnerCommands, kit: &Kit, label: &str, act: Act, width: f32, selected: bool) {
    styled(parent, kit, label, act, width, 34.0, 14.0, Look::Plain { on: selected });
}

#[allow(clippy::too_many_arguments)]
fn styled(
    parent: &mut ChildSpawnerCommands,
    kit: &Kit,
    label: &str,
    act: Act,
    width: f32,
    height: f32,
    size: f32,
    look: Look,
) {
    let big = matches!(look, Look::Play | Look::Go);
    let edge = match look {
        Look::Plain { on: false } => STEEL_EDGE,
        Look::Tab { .. } => Color::NONE,
        _ => SILVER.with_alpha(0.35),
    };
    parent
        .spawn((
            Btn(act),
            look,
            RelativeCursorPosition::default(),
            Node {
                width: px(width),
                height: px(height),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(look.fill(false)),
            BorderColor::all(edge),
        ))
        .with_children(|b| {
            // The big buttons' lighter upper half, as the alpha's glossy PLAY and START BATTLE.
            if big {
                b.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        top: px(0),
                        left: px(0),
                        width: percent(100),
                        height: percent(45),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.12)),
                ));
            }
            b.spawn(kit.text(label, size, Color::WHITE.with_alpha(if big { 1.0 } else { 0.92 }), big));
        });
}

fn title(parent: &mut ChildSpawnerCommands, kit: &Kit, s: &str) {
    parent.spawn(kit.text(s, 26.0, Color::WHITE, true));
}

fn caption(parent: &mut ChildSpawnerCommands, kit: &Kit, s: &str) {
    parent.spawn(kit.text(s, 13.0, DIM, false));
}

/// A dark panel body the caller fills through `fill`.
fn panel(parent: &mut ChildSpawnerCommands, width: f32, fill: impl FnOnce(&mut ChildSpawnerCommands)) {
    parent
        .spawn((
            Node {
                width: px(width),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(8),
                padding: UiRect::all(px(18)),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.035, 0.04, 0.88)),
            BorderColor::all(STEEL_EDGE),
        ))
        .with_children(fill);
}

/// A strip header like the alpha's "TEAM 1" and "Feeds / Friends / Chat" bars.
fn header(parent: &mut ChildSpawnerCommands, kit: &Kit, s: &str, width: Val) {
    parent
        .spawn((
            Node {
                width,
                height: px(22),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.12, 0.13, 0.14, 0.95)),
        ))
        .with_children(|h| {
            h.spawn(kit.text(s, 13.0, Color::WHITE, false));
        });
}

/// A general's picture: the alpha portrait when imported, else a tile in the faction's colour
/// with its tag.
fn portrait_tile(
    parent: &mut ChildSpawnerCommands,
    kit: &Kit,
    faction: Faction,
    general: Option<GeneralId>,
    w: f32,
    h: f32,
) {
    let node = Node {
        width: px(w),
        height: px(h),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    };
    match kit.art(&crate::front::portrait(faction, general)) {
        Some(img) => {
            parent.spawn((ImageNode::new(img), node, BackgroundColor(Color::srgb(0.08, 0.09, 0.10))));
        }
        None => {
            let colour = crate::models::TEAM_PALETTE[faction as usize];
            parent.spawn((node, BackgroundColor(colour.with_alpha(0.55)))).with_children(|t| {
                t.spawn(kit.text(faction_tag(faction), 20.0, Color::WHITE, true));
            });
        }
    }
}

/// The faction's small logo from the alpha, or a colour square.
fn faction_mark(parent: &mut ChildSpawnerCommands, kit: &Kit, faction: Faction) {
    match kit.art(crate::front::logo(faction)) {
        Some(img) => {
            parent.spawn((ImageNode::new(img), Node { width: px(30), height: px(23), ..default() }));
        }
        None => {
            parent.spawn((
                Node { width: px(14), height: px(14), ..default() },
                BackgroundColor(crate::models::TEAM_PALETTE[faction as usize]),
            ));
        }
    }
}

/// The main screen, laid out like the alpha's LIVE page: the player's card, the general tile
/// beside a big PLAY with the mode under it, the tab row, then the general's loading art and
/// what this build holds, with the player's generals down the right.
fn main_menu(body: &mut ChildSpawnerCommands, kit: &Kit, ui: &Ui, profile: &Profile) {
    let me = &ui.config.slots[0];
    let rules = g2_rules::rules();
    let onslaught = ui.config.slots.iter().skip(1).any(|s| s.ai == Some(AiLevel::Onslaught));
    // Player card, general tile, PLAY and the mode bar.
    body.spawn(Node { column_gap: px(18), align_items: AlignItems::Center, ..default() }).with_children(|row| {
        row.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(4), width: px(230), ..default() })
            .with_children(|card| {
                card.spawn(kit.text("COMMANDER", 14.0, Color::WHITE, true));
                let total: u32 = profile.generals.values().map(|p| p.xp).sum();
                let best = profile.generals.iter().max_by_key(|(g, p)| (p.rank, p.xp, std::cmp::Reverse(**g)));
                card.spawn(kit.text(&format!("{total} XP earned"), 12.0, DIM, false));
                if let Some((g, p)) = best {
                    card.spawn(kit.text(&format!("{} - rank {}", rules.general(*g).name, p.rank), 12.0, DIM, false));
                }
            });
        row.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, ..default() })
            .with_children(|g| {
                portrait_tile(g, kit, me.faction, me.general, 80.0, 68.0);
                styled(g, kit, "v", Act::CycleGeneral(0), 80.0, 16.0, 11.0, Look::Plain { on: true });
            });
        row.spawn(Node { flex_direction: FlexDirection::Column, ..default() }).with_children(|p| {
            styled(p, kit, "PLAY", Act::OpenSetup, 300.0, 64.0, 40.0, Look::Play);
            let mode = if onslaught { "ONSLAUGHT   v" } else { "SKIRMISH   v" };
            styled(p, kit, mode, Act::ToggleMode, 300.0, 22.0, 13.0, Look::Plain { on: true });
        });
        row.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(4), width: px(230), ..default() })
            .with_children(|side| {
                let general = me.general.map_or("No general".to_owned(), |g| rules.general(g).name.to_owned());
                side.spawn(kit.text(&format!("{}  {general}", faction_tag(me.faction)), 13.0, Color::WHITE, false));
                styled(side, kit, "FACTION   >", Act::CycleFaction(0), 150.0, 24.0, 12.0, Look::Plain { on: false });
            });
    });
    // The tab row.
    body.spawn(Node { column_gap: px(2), ..default() }).with_children(|tabs| {
        for (label, act, on) in [
            ("LIVE", Act::BackToMain, true),
            ("PROFILE", Act::OpenGenerals, false),
            ("LEARN", Act::ShowControls, false),
            ("QUIT", Act::QuitGame, false),
        ] {
            styled(tabs, kit, label, act, 150.0, 34.0, 18.0, Look::Tab { on });
        }
    });
    // The banner: loading art and what this build holds; the player's generals on the right.
    body.spawn(Node { column_gap: px(16), align_items: AlignItems::Stretch, ..default() }).with_children(|row| {
        row.spawn((
            Node {
                width: px(700),
                height: px(300),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                padding: UiRect::left(px(400)),
                row_gap: px(6),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.04, 0.05, 0.06, 0.9)),
        ))
        .with_children(|banner| {
            // The loading art is a 16:9 frame with the general on one side: shown whole at
            // the banner's height, the text over its darker side.
            if let Some(art) = kit.art(&crate::front::loading_art(me.faction, me.general)) {
                banner.spawn((
                    ImageNode::new(art),
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(-60),
                        top: px(0),
                        width: px(533),
                        height: px(300),
                        ..default()
                    },
                ));
            }
            banner.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    right: px(0),
                    top: px(0),
                    width: px(330),
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.72)),
            ));
            banner.spawn(kit.text("IN THIS BUILD", 24.0, Color::WHITE, true));
            for line in [
                format!("{} GENERALS, {} PERKS", rules.generals.len(), rules.perks.len()),
                "EU, APA AND GLA".to_owned(),
                "SKIRMISH AND ONSLAUGHT".to_owned(),
                "THE ORIGINAL MAPS".to_owned(),
                "REPLAYS, ONLINE LOCKSTEP".to_owned(),
            ] {
                banner.spawn(kit.text(&format!("- {line}"), 13.0, SILVER, false));
            }
        });
        row.spawn((
            Node { width: px(280), flex_direction: FlexDirection::Column, row_gap: px(6), ..default() },
            BackgroundColor(Color::srgba(0.04, 0.05, 0.06, 0.9)),
        ))
        .with_children(|col| {
            header(col, kit, "YOUR GENERALS", percent(100));
            for (i, g) in rules.generals.iter().enumerate().filter(|(_, g)| g.faction == me.faction) {
                let p = profile.progress(GeneralId(i as u16));
                col.spawn(Node { column_gap: px(8), padding: UiRect::horizontal(px(10)), ..default() }).with_children(
                    |r| {
                        r.spawn((kit.text(g.name, 13.0, Color::WHITE, false), Node { width: px(140), ..default() }));
                        r.spawn(kit.text(&format!("R{}  {} XP", p.rank, p.xp), 12.0, DIM, false));
                    },
                );
            }
        });
    });
}

fn controls(body: &mut ChildSpawnerCommands, kit: &Kit) {
    panel(body, 620.0, |p| {
        title(p, kit, "LEARN: CONTROLS");
        for line in [
            "Left click / drag - select units and buildings",
            "Right click - context order: attack, gather, build, move",
            "Ctrl + right click - attack-move",
            "B, then 1-9 - build; 1-9 on a factory - queue units",
            "Backspace - cancel the queued item",
            "T - deploy / pack up artillery; turn a Battle Bus into its bunker",
            "R, then click an enemy - Particle Accelerator (Shadow Tank)",
            "G - general's power; F1-F3 - faction support powers",
            "Tab - select the next collector",
            "Space - pause; + / - - game speed; Esc - menu",
            "M - mute / unmute sound",
        ] {
            p.spawn(kit.text(line, 14.0, TEXT, false));
        }
        button(p, kit, "BACK", Act::BackToMain, 200.0);
    });
}

fn pause(body: &mut ChildSpawnerCommands, kit: &Kit) {
    panel(body, 340.0, |p| {
        title(p, kit, "PAUSED");
        button(p, kit, "RESUME", Act::Resume, 240.0);
        button(p, kit, "SURRENDER", Act::Surrender, 240.0);
        button(p, kit, "QUIT TO MENU", Act::QuitToMenu, 240.0);
    });
}

/// The lobby, like the alpha's custom skirmish: the map preview with its name bar (its list
/// opens over the preview), TEAM 1 against TEAM 2, match options down the right, and a big
/// green START BATTLE.
fn setup(
    body: &mut ChildSpawnerCommands,
    kit: &mut Kit,
    ui: &Ui,
    maps: Option<&[g2_maps::SkirmishMap]>,
    preview: Option<Handle<Image>>,
) {
    let _ = &mut kit.images;
    body.spawn(Node { column_gap: px(20), align_items: AlignItems::FlexStart, ..default() }).with_children(|row| {
        row.spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(12),
            ..default()
        })
        .with_children(|left| {
            map_banner(left, kit, ui, maps, preview);
            teams(left, kit, ui);
        });
        row.spawn((
            Node {
                width: px(260),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(8),
                padding: UiRect::bottom(px(10)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.04, 0.05, 0.06, 0.9)),
        ))
        .with_children(|col| {
            header(col, kit, "MATCH", percent(100));
            button(col, kit, &format!("CREDITS  ${}", ui.config.credits), Act::CycleCredits, 220.0);
            button(col, kit, if ui.config.fog { "FOG OF WAR: ON" } else { "FOG OF WAR: OFF" }, Act::ToggleFog, 220.0);
            button(col, kit, &format!("SEED  {}", ui.config.seed), Act::NextSeed, 220.0);
            header(col, kit, "PLAYERS", percent(100));
            button(col, kit, "ADD AI", Act::AddSlot, 220.0);
            button(col, kit, "REMOVE AI", Act::RemoveSlot, 220.0);
            button(col, kit, "BACK", Act::BackToMain, 220.0);
        });
    });
    styled(body, kit, "START BATTLE", Act::Start, 330.0, 50.0, 28.0, Look::Go);
}

/// The map preview with the name bar across it; with the list open, the list replaces it.
fn map_banner(
    parent: &mut ChildSpawnerCommands,
    kit: &Kit,
    ui: &Ui,
    maps: Option<&[g2_maps::SkirmishMap]>,
    preview: Option<Handle<Image>>,
) {
    parent
        .spawn((
            Node {
                width: px(820),
                height: px(280),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(6)),
                row_gap: px(4),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgb(0.05, 0.05, 0.05)),
            BorderColor::all(Color::srgb(0.02, 0.02, 0.02)),
        ))
        .with_children(|b| {
            if ui.map_list_open {
                map_picker(b, kit, ui, maps);
                return;
            }
            // The previews are small and square: a dimmed, stretched copy fills the wide banner
            // behind the whole map shown sharp at the banner's height.
            if let Some(img) = preview {
                b.spawn((
                    ImageNode::new(img.clone())
                        .with_color(Color::srgba(0.45, 0.45, 0.45, 1.0))
                        .with_mode(bevy::ui::widget::NodeImageMode::Stretch),
                    Node {
                        position_type: PositionType::Absolute,
                        width: percent(100),
                        height: percent(100),
                        ..default()
                    },
                ));
                b.spawn((
                    ImageNode::new(img),
                    Node { position_type: PositionType::Absolute, width: px(268), height: px(268), ..default() },
                ));
            }
            // As the alpha's "Red Zone (1v1)": the map's name without its game prefix, and its
            // size as players a side.
            let starts = maps.and_then(|m| m.iter().find(|m| m.name == ui.config.map)).map(|m| m.starts);
            let name = ui.config.map.split_once(':').map_or(ui.config.map.as_str(), |(_, n)| n);
            let size = starts.map_or(String::new(), |n| format!(" ({}v{})", n / 2, n - n / 2));
            styled(
                b,
                kit,
                &format!("{name}{size}     v"),
                Act::ToggleMapList,
                420.0,
                40.0,
                16.0,
                Look::Plain { on: false },
            );
        });
}

fn map_picker(p: &mut ChildSpawnerCommands, kit: &Kit, ui: &Ui, maps: Option<&[g2_maps::SkirmishMap]>) {
    let Some(maps) = maps else {
        caption(p, kit, "loading map list...");
        return;
    };
    let pages = maps.len().div_ceil(MAPS_PER_PAGE).max(1);
    let page = ui.map_page.min(pages - 1);
    p.spawn(Node {
        flex_wrap: FlexWrap::Wrap,
        width: px(740),
        column_gap: px(6),
        row_gap: px(4),
        justify_content: JustifyContent::Center,
        ..default()
    })
    .with_children(|grid| {
        for (i, m) in maps.iter().enumerate().skip(page * MAPS_PER_PAGE).take(MAPS_PER_PAGE) {
            let label = format!("{}  ({})", m.name, m.starts);
            pick(grid, kit, &label, Act::MapPick(i), 360.0, m.name == ui.config.map);
        }
    });
    p.spawn(Node { column_gap: px(8), align_items: AlignItems::Center, ..default() }).with_children(|nav| {
        button(nav, kit, "<", Act::MapPage(-1), 44.0);
        nav.spawn(kit.text(&format!("{}/{}", page + 1, pages), 13.0, DIM, false));
        button(nav, kit, ">", Act::MapPage(1), 44.0);
        button(nav, kit, "CLOSE", Act::ToggleMapList, 100.0);
    });
}

/// TEAM 1 (the player) VS TEAM 2 (every AI), one card per player.
fn teams(parent: &mut ChildSpawnerCommands, kit: &Kit, ui: &Ui) {
    let colours = crate::models::team_slots(&ui.config.slots.iter().map(|s| s.faction).collect::<Vec<_>>());
    parent.spawn(Node { column_gap: px(20), align_items: AlignItems::Center, ..default() }).with_children(|row| {
        row.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(6), ..default() }).with_children(|team| {
            header(team, kit, "TEAM 1", px(380));
            player_card(team, kit, ui, 0, colours[0]);
        });
        row.spawn(kit.text("VS", 44.0, SILVER, true));
        row.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(6), ..default() }).with_children(|team| {
            header(team, kit, "TEAM 2", px(380));
            for (i, &colour) in colours.iter().enumerate().skip(1) {
                player_card(team, kit, ui, i, colour);
            }
        });
    });
}

/// One player's card: portrait, faction mark, general and name, then the faction and (for an
/// AI) its level as the alpha's EASY / MEDIUM / HARD pills.
fn player_card(parent: &mut ChildSpawnerCommands, kit: &Kit, ui: &Ui, i: usize, colour: usize) {
    let slot = &ui.config.slots[i];
    let rules = g2_rules::rules();
    parent
        .spawn((
            Node {
                width: px(380),
                height: px(76),
                column_gap: px(8),
                padding: UiRect::all(px(4)),
                border: UiRect::bottom(px(3)),
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.06, 0.07, 0.08, 0.95)),
            BorderColor::all(crate::models::TEAM_PALETTE[colour]),
        ))
        .with_children(|card| {
            portrait_tile(card, kit, slot.faction, slot.general, 78.0, 66.0);
            card.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(3), ..default() }).with_children(
                |info| {
                    info.spawn(Node { column_gap: px(6), align_items: AlignItems::Center, ..default() }).with_children(
                        |r| {
                            faction_mark(r, kit, slot.faction);
                            let general =
                                slot.general.map_or("NO GENERAL".to_owned(), |g| rules.general(g).name.to_uppercase());
                            styled(
                                r,
                                kit,
                                &general,
                                Act::CycleGeneral(i),
                                150.0,
                                22.0,
                                13.0,
                                Look::Plain { on: false },
                            );
                            styled(
                                r,
                                kit,
                                faction_tag(slot.faction),
                                Act::CycleFaction(i),
                                50.0,
                                22.0,
                                12.0,
                                Look::Plain { on: false },
                            );
                        },
                    );
                    if i == 0 {
                        info.spawn(kit.text("YOU", 12.0, DIM, false));
                    } else {
                        info.spawn(Node { column_gap: px(4), align_items: AlignItems::Center, ..default() })
                            .with_children(|r| {
                                r.spawn(kit.text("AI", 12.0, DIM, false));
                                // Sized to their labels: the alpha's wide font clips ONSLAUGHT in
                                // an even third.
                                for (level, w) in
                                    [(AiLevel::Easy, 56.0), (AiLevel::Normal, 72.0), (AiLevel::Onslaught, 96.0)]
                                {
                                    let on = slot.ai == Some(level);
                                    styled(
                                        r,
                                        kit,
                                        &level.name().to_uppercase(),
                                        Act::SetAi(i, level),
                                        w,
                                        20.0,
                                        10.0,
                                        Look::Plain { on },
                                    );
                                }
                            });
                    }
                },
            );
        });
}

fn end_screen(body: &mut ChildSpawnerCommands, kit: &Kit, sim: &Sim, ui: &Ui) {
    let world = &sim.world;
    let winner = world.winner();
    // Watching two AIs play (the local player is one of them) has no victory to claim.
    let watched = world.is_ai(sim.local);
    let won = winner == Some(sim.local);
    let headline = match winner {
        _ if watched => "MATCH OVER",
        Some(_) if won => "VICTORY",
        Some(_) => "DEFEAT",
        None => "MATCH OVER",
    };
    if !watched
        && winner.is_some()
        && let Some(art) = kit.art(&crate::front::outcome_art(world.player(sim.local).faction, won))
    {
        body.spawn((ImageNode::new(art), Node { width: px(390), height: px(286), ..default() }));
    }
    panel(body, 700.0, |p| {
        p.spawn(kit.text(headline, 40.0, Color::WHITE, true));
        caption(p, kit, &format!("{} | seed {} | {}", ui.config.map, ui.config.seed, clock(world.tick())));
        for i in 0..world.players().len() {
            let id = PlayerId(i as u8);
            let s = world.stats(id);
            let name = match world.player(id).general {
                Some(g) => format!("{} {}", faction_tag(world.player(id).faction), world.rules.general(g).name),
                None => faction_tag(world.player(id).faction).to_owned(),
            };
            let out = s
                .defeated_at
                .map(|t| format!("out {}", clock(t)))
                .or_else(|| (winner == Some(id)).then(|| "winner".to_owned()))
                .unwrap_or_default();
            p.spawn(Node {
                width: px(650),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(10),
                ..default()
            })
            .with_children(|r| {
                r.spawn((Node { width: px(10), height: px(10), ..default() }, BackgroundColor(sim.team_color(id))));
                r.spawn((kit.text(&name, 14.0, TEXT, false), Node { width: px(170), ..default() }));
                let totals = format!(
                    "built {} | lost {} | kills {}",
                    s.units_built + s.buildings_built,
                    s.units_lost + s.buildings_lost,
                    s.kills
                );
                r.spawn((kit.text(&totals, 13.0, DIM, false), Node { width: px(250), ..default() }));
                r.spawn((kit.text(&format!("${}", s.earned), 13.0, DIM, false), Node { width: px(70), ..default() }));
                r.spawn((kit.text(&out, 13.0, BLUE_TOP, false), Node { width: px(110), ..default() }));
            });
        }
        if let Some((g, xp)) = ui.earned {
            caption(p, kit, &format!("{} earned {xp} XP", world.rules.general(g).name));
        }
        styled(p, kit, "PLAY AGAIN", Act::Start, 320.0, 44.0, 22.0, Look::Go);
        button(p, kit, "CHANGE SETUP", Act::ChangeSetup, 320.0);
        button(p, kit, "MAIN MENU", Act::QuitToMenu, 320.0);
    });
}

/// The Generals screen (the main screen's PROFILE tab): one general's portrait, rank,
/// experience, perk slots and perks. Each perk shows its rank, price and state, and its
/// button the one thing it can do now.
fn generals(body: &mut ChildSpawnerCommands, kit: &Kit, ui: &Ui, profile: &Profile) {
    let rules = g2_rules::rules();
    let g = ui.shown_general;
    let general = rules.general(g);
    let me = profile.progress(g);
    body.spawn(Node { column_gap: px(16), align_items: AlignItems::FlexStart, ..default() }).with_children(|row| {
        row.spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(8),
            ..default()
        })
        .with_children(|left| {
            portrait_tile(left, kit, general.faction, Some(g), 247.0, 210.0);
            left.spawn(Node { column_gap: px(8), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                button(r, kit, "<", Act::CycleShownGeneral(-1), 44.0);
                faction_mark(r, kit, general.faction);
                button(r, kit, ">", Act::CycleShownGeneral(1), 44.0);
            });
        });
        panel(row, 720.0, |p| {
            p.spawn(kit.text(
                &format!("{} {}", faction_tag(general.faction), general.name.to_uppercase()),
                28.0,
                Color::WHITE,
                true,
            ));
            let next = if me.rank < MAX_RANK {
                format!("promote: {} XP", promotion_cost(me.rank))
            } else {
                "highest rank".to_owned()
            };
            p.spawn(Node { column_gap: px(10), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                r.spawn(kit.text(
                    &format!(
                        "Rank {} of {MAX_RANK} | {} XP | slots {}/{} | {next}",
                        me.rank,
                        me.xp,
                        me.slotted.len(),
                        me.rank
                    ),
                    13.0,
                    DIM,
                    false,
                ));
                if me.rank < MAX_RANK {
                    button(r, kit, "PROMOTE", Act::Promote, 110.0);
                }
            });
            if let Some(power) = general.power {
                caption(p, kit, &format!("Power: {}", power.name));
            }
            for &k in general.perks {
                let perk = rules.perk(k);
                let (state, act, label) = if me.slotted.contains(&k) {
                    ("slotted", Act::UnslotPerk(k), "UNSLOT")
                } else if me.bought.contains(&k) {
                    ("owned", Act::SlotPerk(k), "SLOT")
                } else if me.rank < perk.rank {
                    ("locked", Act::BuyPerk(k), "BUY")
                } else {
                    ("", Act::BuyPerk(k), "BUY")
                };
                p.spawn(Node { width: px(680), column_gap: px(10), align_items: AlignItems::Center, ..default() })
                    .with_children(|r| {
                        r.spawn((kit.text(perk.name, 14.0, TEXT, false), Node { width: px(150), ..default() }));
                        r.spawn((kit.text(perk.description, 12.0, DIM, false), Node { width: px(244), ..default() }));
                        r.spawn((
                            kit.text(&format!("R{} {} XP", perk.rank, perk.price), 12.0, DIM, false),
                            Node { width: px(116), ..default() },
                        ));
                        r.spawn((kit.text(state, 12.0, BLUE_TOP, false), Node { width: px(56), ..default() }));
                        button(r, kit, label, act, 70.0);
                    });
            }
            if let Some(why) = ui.refused {
                p.spawn(kit.text(refusal(why), 13.0, BLUE_TOP, false));
            }
            caption(p, kit, "Play a skirmish as this general to earn XP; slotted perks apply in every match.");
            button(p, kit, "BACK", Act::BackToMain, 200.0);
        });
    });
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

    /// Setup edits respect the map: slots stay within its starts and at least two, the player
    /// in slot 0 has no AI level, and a general only fits its own faction.
    #[test]
    fn setup_edits_stay_valid() {
        let (mut app, _) = app();
        act(&mut app, Act::OpenSetup);
        for _ in 0..5 {
            act(&mut app, Act::AddSlot);
        }
        assert_eq!(app.world().resource::<Ui>().config.slots.len(), 2, "Crater Ridge has two starts");
        act(&mut app, Act::RemoveSlot);
        assert_eq!(app.world().resource::<Ui>().config.slots.len(), 2, "never fewer than two players");
        act(&mut app, Act::SetAi(0, AiLevel::Easy));
        assert_eq!(app.world().resource::<Ui>().config.slots[0].ai, None, "the human has no level");
        act(&mut app, Act::CycleGeneral(0));
        let cfg = app.world().resource::<Ui>().config.clone();
        let g = cfg.slots[0].general.expect("EU has a general");
        assert_eq!(g2_rules::rules().general(g).faction, cfg.slots[0].faction);
        act(&mut app, Act::CycleFaction(0));
        assert_eq!(app.world().resource::<Ui>().config.slots[0].general, None, "a new faction drops the general");
    }

    /// A many-start map takes more players, and the match that starts has exactly that many.
    #[test]
    fn a_big_map_hosts_more_players() {
        let (mut app, _) = app();
        let maps = g2_maps::skirmish_maps(&g2_maps::vendored_root());
        let i = maps.iter().position(|m| m.name == "gen:Tournament Tundra").expect("listed");
        act(&mut app, Act::OpenSetup);
        act(&mut app, Act::MapPick(i));
        for _ in 0..5 {
            act(&mut app, Act::AddSlot);
        }
        assert_eq!(app.world().resource::<Ui>().config.slots.len(), 4);
        act(&mut app, Act::Start);
        assert_eq!(app.world().resource::<Sim>().world.players().len(), 4);
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
