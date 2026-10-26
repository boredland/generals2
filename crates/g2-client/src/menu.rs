//! Menus: main menu, controls, skirmish setup, in-match pause and the end screen.
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

use crate::hud::{DIM, PANEL, PANEL_EDGE, SLOT, TEAL, TEXT, clock, text};
use g2_sim::ai::ai_perks;

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
    CycleAi(usize),
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
    /// slots `perks` (from their profile) on their general; AI slots pick their own.
    pub fn build(&self, perks: &[PerkId]) -> Result<Sim, String> {
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
fn hover(mut buttons: Query<(&Btn, &RelativeCursorPosition, &mut BackgroundColor)>) {
    for (_, cur, mut bg) in &mut buttons {
        bg.set_if_neq(BackgroundColor(if cur.cursor_over() { Color::srgba(0.16, 0.34, 0.38, 0.98) } else { SLOT }));
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
            let perks = world.resource::<Profile>().slotted(config.slots[0].general);
            match config.build(&perks) {
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
            Act::BackToMain => world.resource_mut::<Ui>().screen = Screen::Main,
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
            Act::CycleAi(i) => {
                // Slot 0 is the player at the keyboard and has no level to set.
                if let Some(s) = world.resource_mut::<Ui>().config.slots.get_mut(i).filter(|_| i > 0) {
                    s.ai = Some(match s.ai {
                        Some(AiLevel::Easy) => AiLevel::Normal,
                        Some(AiLevel::Normal) => AiLevel::Onslaught,
                        _ => AiLevel::Easy,
                    });
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
// `rebuild` replaces the whole menu overlay whenever `Ui` changes. Screens are centred columns
// over the frozen battlefield, in the HUD's gunmetal-and-teal panel language (docs/STYLE.md).

fn rebuild(
    mut commands: Commands,
    ui: Res<Ui>,
    sim: Res<Sim>,
    profile: Res<Profile>,
    maps: Option<Res<MapList>>,
    old: Query<Entity, With<MenuUi>>,
) {
    if !ui.is_changed() && !profile.is_changed() {
        return;
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    // A running match shows the HUD alone: no overlay, no dimming.
    if ui.screen == Screen::Match && !ui.paused {
        return;
    }
    commands
        .spawn((
            MenuUi,
            GlobalZIndex(10),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, if ui.screen == Screen::Match { 0.45 } else { 0.62 })),
            Node {
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_direction: FlexDirection::Column,
                row_gap: px(14),
                ..default()
            },
        ))
        .with_children(|root| match ui.screen {
            Screen::Main => main_menu(root),
            Screen::Controls => controls(root),
            Screen::Setup => setup(root, &ui, maps.as_ref().map(|m| m.0.as_slice())),
            Screen::Match => pause(root),
            Screen::End => end_screen(root, &sim, &ui),
            Screen::Generals => generals(root, &ui, &profile),
        });
}

/// A clickable button: `hover` lights it up, a click queues its `Act`.
fn button(parent: &mut ChildSpawnerCommands, label: &str, act: Act, width: f32) {
    pick(parent, label, act, width, false);
}

/// As `button`, but drawn selected: the current map in the list.
fn pick(parent: &mut ChildSpawnerCommands, label: &str, act: Act, width: f32, selected: bool) {
    parent
        .spawn((
            Btn(act),
            RelativeCursorPosition::default(),
            Node {
                width: px(width),
                height: px(34),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(SLOT),
            BorderColor::all(if selected { TEAL } else { PANEL_EDGE }),
        ))
        .with_children(|b| {
            b.spawn(text(label, 14.0, if selected { TEAL } else { TEXT }));
        });
}

fn title(parent: &mut ChildSpawnerCommands, s: &str) {
    parent.spawn(text(s, 28.0, TEAL));
}

fn caption(parent: &mut ChildSpawnerCommands, s: &str) {
    parent.spawn(text(s, 13.0, DIM));
}

/// A panel body the caller fills through `fill`.
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
            BackgroundColor(PANEL),
            BorderColor::all(PANEL_EDGE),
        ))
        .with_children(fill);
}

fn main_menu(root: &mut ChildSpawnerCommands) {
    panel(root, 380.0, |p| {
        title(p, "OPEN RTS");
        caption(p, "an original RTS engine");
        button(p, "SKIRMISH", Act::OpenSetup, 260.0);
        button(p, "GENERALS", Act::OpenGenerals, 260.0);
        button(p, "CONTROLS", Act::ShowControls, 260.0);
        button(p, "QUIT", Act::QuitGame, 260.0);
    });
}

fn controls(root: &mut ChildSpawnerCommands) {
    panel(root, 560.0, |p| {
        title(p, "CONTROLS");
        for line in [
            "Left click / drag - select units and buildings",
            "Right click - context order: attack, gather, build, move",
            "Ctrl + right click - attack-move",
            "B, then 1-9 - build; 1-9 on a factory - queue units",
            "Backspace - cancel the queued item",
            "T - deploy / pack up artillery",
            "G - general's power; F1-F3 - faction support powers",
            "Tab - select the next collector",
            "Space - pause; + / - - game speed; Esc - menu",
            "M - mute / unmute sound",
        ] {
            p.spawn(text(line, 14.0, TEXT));
        }
        button(p, "BACK", Act::BackToMain, 200.0);
    });
}

fn pause(root: &mut ChildSpawnerCommands) {
    panel(root, 320.0, |p| {
        title(p, "PAUSED");
        button(p, "RESUME", Act::Resume, 220.0);
        button(p, "SURRENDER", Act::Surrender, 220.0);
        button(p, "QUIT TO MENU", Act::QuitToMenu, 220.0);
    });
}

/// Skirmish setup: map list on the left, players and match options on the right.
fn setup(root: &mut ChildSpawnerCommands, ui: &Ui, maps: Option<&[g2_maps::SkirmishMap]>) {
    root.spawn(Node {
        flex_direction: FlexDirection::Row,
        column_gap: px(16),
        align_items: AlignItems::FlexStart,
        ..default()
    })
    .with_children(|row| {
        panel(row, 380.0, |p| map_list_panel(p, ui, maps));
        panel(row, 440.0, |p| options_panel(p, ui));
    });
}

fn map_list_panel(p: &mut ChildSpawnerCommands, ui: &Ui, maps: Option<&[g2_maps::SkirmishMap]>) {
    title(p, "MAP");
    let Some(maps) = maps else {
        caption(p, "loading map list...");
        return;
    };
    let pages = maps.len().div_ceil(MAPS_PER_PAGE).max(1);
    let page = ui.map_page.min(pages - 1);
    for (i, m) in maps.iter().enumerate().skip(page * MAPS_PER_PAGE).take(MAPS_PER_PAGE) {
        pick(p, &format!("{}  ({} starts)", m.name, m.starts), Act::MapPick(i), 340.0, m.name == ui.config.map);
    }
    p.spawn(Node { column_gap: px(8), align_items: AlignItems::Center, ..default() }).with_children(|nav| {
        button(nav, "<", Act::MapPage(-1), 44.0);
        nav.spawn(text(&format!("{}/{}", page + 1, pages), 13.0, DIM));
        button(nav, ">", Act::MapPage(1), 44.0);
    });
}

fn options_panel(p: &mut ChildSpawnerCommands, ui: &Ui) {
    title(p, "PLAYERS");
    let teams = crate::models::team_slots(&ui.config.slots.iter().map(|s| s.faction).collect::<Vec<_>>());
    for (i, slot) in ui.config.slots.iter().enumerate() {
        let general = slot.general.map_or("No general".to_owned(), |g| g2_rules::rules().general(g).name.to_owned());
        let level = if i == 0 { "YOU".to_owned() } else { slot.ai.unwrap_or_default().name().to_owned() };
        p.spawn(Node { column_gap: px(6), align_items: AlignItems::Center, ..default() }).with_children(|r| {
            r.spawn((
                Node { width: px(12), height: px(12), ..default() },
                BackgroundColor(crate::models::TEAM_PALETTE[teams[i]]),
            ));
            button(r, faction_tag(slot.faction), Act::CycleFaction(i), 70.0);
            button(r, &general, Act::CycleGeneral(i), 130.0);
            button(r, &level, Act::CycleAi(i), 90.0);
        });
    }
    p.spawn(Node { column_gap: px(8), ..default() }).with_children(|r| {
        button(r, "+ ADD", Act::AddSlot, 110.0);
        button(r, "REMOVE", Act::RemoveSlot, 110.0);
    });
    title(p, "MATCH");
    p.spawn(Node { column_gap: px(8), ..default() }).with_children(|r| {
        button(r, &format!("${}", ui.config.credits), Act::CycleCredits, 110.0);
        button(r, if ui.config.fog { "Fog: on" } else { "Fog: off" }, Act::ToggleFog, 110.0);
        button(r, &format!("Seed {}", ui.config.seed), Act::NextSeed, 130.0);
    });
    p.spawn(Node { column_gap: px(8), ..default() }).with_children(|r| {
        button(r, "BACK", Act::BackToMain, 150.0);
        button(r, "START", Act::Start, 150.0);
    });
}

fn end_screen(root: &mut ChildSpawnerCommands, sim: &Sim, ui: &Ui) {
    let world = &sim.world;
    let winner = world.winner();
    panel(root, 660.0, |p| {
        // Watching two AIs play (the local player is one of them) has no victory to claim.
        let headline = match winner {
            _ if world.is_ai(sim.local) => "MATCH OVER",
            Some(w) if w == sim.local => "VICTORY",
            Some(_) => "DEFEAT",
            None => "MATCH OVER",
        };
        title(p, headline);
        caption(p, &format!("{} | seed {} | {}", ui.config.map, ui.config.seed, clock(world.tick())));
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
                width: px(610),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(10),
                ..default()
            })
            .with_children(|r| {
                r.spawn((Node { width: px(10), height: px(10), ..default() }, BackgroundColor(sim.team_color(id))));
                r.spawn((text(&name, 14.0, TEXT), Node { width: px(160), ..default() }));
                let totals = format!(
                    "built {} | lost {} | kills {}",
                    s.units_built + s.buildings_built,
                    s.units_lost + s.buildings_lost,
                    s.kills
                );
                r.spawn((text(&totals, 13.0, DIM), Node { width: px(240), ..default() }));
                r.spawn((text(&format!("${}", s.earned), 13.0, DIM), Node { width: px(70), ..default() }));
                r.spawn((text(&out, 13.0, TEAL), Node { width: px(110), ..default() }));
            });
        }
        if let Some((g, xp)) = ui.earned {
            caption(p, &format!("{} earned {xp} XP", world.rules.general(g).name));
        }
        button(p, "PLAY AGAIN", Act::Start, 320.0);
        button(p, "CHANGE SETUP", Act::ChangeSetup, 320.0);
        button(p, "MAIN MENU", Act::QuitToMenu, 320.0);
    });
}

/// The Generals screen: one general's rank, experience, perk slots and perks. Each perk shows
/// its rank, price and state, and its button the one thing it can do now.
fn generals(root: &mut ChildSpawnerCommands, ui: &Ui, profile: &Profile) {
    let rules = g2_rules::rules();
    let g = ui.shown_general;
    let general = rules.general(g);
    let me = profile.progress(g);
    panel(root, 720.0, |p| {
        p.spawn(Node { column_gap: px(8), align_items: AlignItems::Center, ..default() }).with_children(|r| {
            button(r, "<", Act::CycleShownGeneral(-1), 44.0);
            r.spawn((
                text(&format!("{} {}", faction_tag(general.faction), general.name), 28.0, TEAL),
                Node { width: px(340), justify_content: JustifyContent::Center, ..default() },
            ));
            button(r, ">", Act::CycleShownGeneral(1), 44.0);
        });
        let next = if me.rank < MAX_RANK {
            format!("promote: {} XP", promotion_cost(me.rank))
        } else {
            "highest rank".to_owned()
        };
        p.spawn(Node { column_gap: px(10), align_items: AlignItems::Center, ..default() }).with_children(|r| {
            r.spawn(text(
                &format!(
                    "Rank {} of {MAX_RANK} | {} XP | slots {}/{} | {next}",
                    me.rank,
                    me.xp,
                    me.slotted.len(),
                    me.rank
                ),
                13.0,
                DIM,
            ));
            if me.rank < MAX_RANK {
                button(r, "PROMOTE", Act::Promote, 110.0);
            }
        });
        if let Some(power) = general.power {
            caption(p, &format!("Power: {}", power.name));
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
                    r.spawn((text(perk.name, 14.0, TEXT), Node { width: px(150), ..default() }));
                    r.spawn((text(perk.description, 12.0, DIM), Node { width: px(300), ..default() }));
                    r.spawn((
                        text(&format!("R{} {} XP", perk.rank, perk.price), 12.0, DIM),
                        Node { width: px(90), ..default() },
                    ));
                    r.spawn((text(state, 12.0, TEAL), Node { width: px(50), ..default() }));
                    button(r, label, act, 70.0);
                });
        }
        if let Some(why) = ui.refused {
            p.spawn(text(refusal(why), 13.0, TEAL));
        }
        caption(p, "Play a skirmish as this general to earn XP; slotted perks apply in every match.");
        button(p, "BACK", Act::BackToMain, 200.0);
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
        act(&mut app, Act::CycleAi(0));
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
        assert_eq!(app.world().resource::<Ui>().shown_general, JUNKYARD, "wraps around");
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
