//! HUD (docs/STYLE.md): conventional RTS zones (minimap bottom-left, selection
//! and production bottom-right, clock top-centre) in our own panel design, gunmetal with teal
//! accents. Every element shows live sim data; nothing is decorative.
//!
//! - Top centre: a compact match bar with both factions' tags around the clock.
//! - Top left: menu label; top right: neutral project label (no EA trademarks in the UI).
//! - Bottom left: minimap with team and resource dots; power and money above it.
//! - Bottom right: selection panel (name, health, status, training queue with progress) and the
//!   numbered 3×3 build/production grid for the selection's hotkeys.
//! - World overlays: collector count over own supply centers (red at the faction cap), health bars
//!   over the selection and damaged entities.
//!
//! Not shown until the systems exist: generals/perks, abilities, experience.

use bevy::prelude::*;
use g2_sim::{Class, Faction, TICKS_PER_SECOND};

use crate::camera::RtsCamera;
use crate::input::{Placement, PowerAim, Selection};
use crate::sim::Sim;

pub(crate) const PANEL: Color = Color::srgba(0.07, 0.08, 0.09, 0.92);
pub(crate) const PANEL_EDGE: Color = Color::srgb(0.24, 0.27, 0.29);
pub(crate) const SLOT: Color = Color::srgba(0.13, 0.15, 0.17, 0.95);
pub(crate) const TEAL: Color = Color::srgb(0.18, 0.78, 0.86);
const MONEY: Color = Color::srgb(0.35, 0.95, 0.35);
const XP: Color = Color::srgb(0.98, 0.86, 0.12);
pub(crate) const TEXT: Color = Color::srgb(0.88, 0.90, 0.90);
pub(crate) const DIM: Color = Color::srgb(0.55, 0.58, 0.60);
const MINIMAP: f32 = 196.0;

#[derive(Component)]
pub(crate) enum Field {
    Clock,
    /// A player's tag in the match bar, dimmed once that player is defeated.
    Tag(g2_sim::PlayerId),
    Power,
    /// Power as bare figures ("20/180"), for the alpha's power slot beside its own icon.
    PowerFigures,
    Money,
    /// The general's power: "G <name>" when ready, else its countdown; hidden without one.
    GeneralPower,
    /// Countdown to the next Onslaught wave; empty without an Onslaught opponent.
    Waves,
    Status,
    SelName,
    SelInfo,
    Queue,
    QueueBar,
    GridCell(usize),
    /// An alpha build button's corner text: hotkey and price (the icon names the def).
    GridTag(usize),
}

#[derive(Component)]
struct MinimapDot;

#[derive(Component)]
pub(crate) struct MinimapView;

/// A build-grid button's icon in the alpha HUD, shown when its cell has something to build.
#[derive(Component)]
struct GridIcon(usize);

#[derive(Component)]
struct QueueRow;

/// Floating world-space labels, positioned each frame by projecting their anchor to the screen.
/// Overlays are rebuilt every frame; these markers find last frame's to despawn.
#[derive(Component)]
struct WorldLabel;

/// Per-frame world overlays (health bars, cargo pips), despawned and rebuilt every frame.
#[derive(Component)]
struct Overlay;

/// A "+credits" pop-up over a supply center: rises and fades over `POPUP_SECS`.
#[derive(Component)]
struct Popup {
    depot: g2_sim::EntityId,
    age: f32,
}

const POPUP_SECS: f32 = 1.5;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_alpha_layout)
            .add_systems(
                Update,
                spawn.run_if(crate::menu::scene_start.or_eager(resized_in_match)).after(crate::menu::MenuSet),
            )
            .add_systems(
                Update,
                (update_fields, tags, minimap, overlays, popups, grid_icons, selection_portrait).after(spawn),
            );
    }
}

/// The window changed size while a match HUD exists: rebuild it for the new size.
fn resized_in_match(mut resized: MessageReader<bevy::window::WindowResized>, hud: Query<(), With<HudRoot>>) -> bool {
    resized.read().count() > 0 && !hud.is_empty()
}

/// Every node the HUD spawns at the root, so a resize can rebuild the HUD (the alpha's is
/// placed for the window size it was spawned at).
#[derive(Component)]
pub(crate) struct HudRoot;

fn root() -> (crate::menu::MatchEntity, HudRoot) {
    (crate::menu::MatchEntity, HudRoot)
}

/// A HUD area that owns the screen under it: clicks there are the HUD's, and world labels
/// anchored there are not drawn.
#[derive(Component)]
pub(crate) struct HudPanel;

/// A build-grid cell in either HUD: clicking it is pressing its hotkey.
#[derive(Component)]
pub(crate) struct GridButton(pub usize);

pub(crate) type Panels<'w, 's> = Query<
    'w,
    's,
    (&'static ComputedNode, &'static bevy::ui::UiGlobalTransform, &'static InheritedVisibility),
    With<HudPanel>,
>;

/// Whether `point` (logical window pixels) is over a visible HUD panel.
pub(crate) fn over_hud(panels: &Panels, point: Vec2) -> bool {
    panels.iter().any(|(node, tf, vis)| vis.get() && node.contains_point(*tf, point / node.inverse_scale_factor()))
}

/// The side of the square the minimap draws the map in. Our panel is MINIMAP square; the
/// alpha's slot is whatever its layout says, so the map fills its smaller laid-out side
/// (before the first layout pass, the node's own width).
pub(crate) fn minimap_side(computed: &ComputedNode, node: &Node) -> f32 {
    let laid_out = computed.size() * computed.inverse_scale_factor();
    match (laid_out.min_element(), node.width) {
        (s, _) if s > 0.0 => s,
        (_, Val::Px(w)) => w,
        _ => MINIMAP,
    }
}

/// With an imported alpha, reads its HUD layout once; `spawn` then builds the alpha's HUD.
fn load_alpha_layout(mut commands: Commands, cache: Option<Res<crate::alpha::Cache>>) {
    if let Some(layout) = cache.and_then(|c| crate::alpha_hud::Layout::load(&c)) {
        commands.insert_resource(layout);
    }
}

/// Dims a defeated player's tag in the match bar.
fn tags(sim: Res<Sim>, mut q: Query<(&Field, &mut TextColor)>) {
    for (f, mut c) in &mut q {
        if let Field::Tag(id) = *f {
            let team = sim.team_color(id);
            c.set_if_neq(TextColor(if sim.world.player(id).defeated { team.with_alpha(0.3) } else { team }));
        }
    }
}

/// Spawns a pop-up per local delivery and moves the live ones; they track the depot on screen.
fn popups(
    mut commands: Commands,
    sim: Res<Sim>,
    time: Res<Time>,
    mut deliveries: ResMut<crate::sim::Deliveries>,
    cam: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
    mut live: Query<(Entity, &mut Popup, &mut Node, &mut TextColor)>,
) {
    for (depot, credits) in deliveries.0.drain(..) {
        commands.spawn((
            abs(px(-100.0), auto(), px(-100.0), auto()),
            text(&format!("+{credits}"), 13.0, MONEY),
            TextShadow { offset: Vec2::splat(1.0), ..default() },
            Popup { depot, age: 0.0 },
        ));
    }
    let Ok((camera, cam_tf)) = cam.single() else { return };
    for (id, mut p, mut node, mut color) in &mut live {
        p.age += time.delta_secs();
        let anchor = sim.world.entity(p.depot).map(|e| crate::view::overlay_anchor(&sim, e));
        let screen = anchor.and_then(|a| camera.world_to_viewport(cam_tf, a).ok());
        let (Some(screen), true) = (screen, p.age < POPUP_SECS) else {
            commands.entity(id).despawn();
            continue;
        };
        let t = p.age / POPUP_SECS;
        node.left = px(screen.x - 14.0);
        node.top = px(screen.y - 60.0 - t * 24.0);
        color.0 = MONEY.with_alpha(1.0 - t * t);
    }
}

pub(crate) fn text(s: &str, size: f32, color: Color) -> impl Bundle {
    (Text::new(s), TextFont { font_size: FontSize::Px(size), ..default() }, TextColor(color))
}

pub(crate) fn abs(left: Val, right: Val, top: Val, bottom: Val) -> Node {
    Node { position_type: PositionType::Absolute, left, right, top, bottom, ..default() }
}

pub(crate) fn panel_node(mut node: Node) -> impl Bundle {
    node.border = UiRect::all(px(1));
    node.padding = UiRect::all(px(6));
    (node, BackgroundColor(PANEL), BorderColor::all(PANEL_EDGE))
}

fn faction_initial(f: Faction) -> &'static str {
    match f {
        Faction::Eu => "EU",
        Faction::Apa => "APA",
        Faction::Gla => "GLA",
    }
}

/// The render target's size where there is no window (`--screenshot`), so the alpha HUD is
/// laid out for the frame it will be drawn in.
#[derive(Resource)]
pub struct HeadlessScreen(pub Vec2);

fn spawn(
    mut commands: Commands,
    sim: Res<Sim>,
    layout: Option<Res<crate::alpha_hud::Layout>>,
    server: Option<Res<AssetServer>>,
    window: Query<&Window>,
    headless: Option<Res<HeadlessScreen>>,
    old: Query<Entity, With<HudRoot>>,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    let world = &sim.world;
    let screen = match (window.iter().next(), headless) {
        (Some(w), _) => w.size(),
        (None, Some(h)) => h.0,
        (None, None) => Vec2::new(1280.0, 720.0),
    };
    if let (Some(layout), Some(server)) = (layout, server) {
        let faction = world.player(sim.local).faction;
        if crate::alpha_hud::spawn(&mut commands, &layout, &server, faction, screen) {
            spawn_alpha_extras(&mut commands);
            return;
        }
    }

    // Top left: the in-match menu key; top right: neutral project label (no EA trademarks).
    commands.spawn((root(), abs(px(14), auto(), px(8), auto()), text("ESC  MENU", 14.0, DIM)));
    commands.spawn((root(), abs(auto(), px(14), px(8), auto()), text("OPEN RTS", 12.0, DIM)));

    // Top centre: one compact match bar: own tag, clock, then every opponent's tag in its
    // team colour (struck out once defeated); a general shows after its faction.
    commands
        .spawn((
            root(),
            Node {
                position_type: PositionType::Absolute,
                top: px(6),
                left: percent(50),
                margin: UiRect::left(px(-210)),
                width: px(420),
                padding: UiRect::axes(px(8), px(3)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                column_gap: px(12),
                border: UiRect::bottom(px(2)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
            BorderColor::all(TEAL),
            HudPanel,
        ))
        .with_children(|t| {
            let label = |p: &g2_sim::Player| match p.general {
                Some(g) => format!("{} {}", faction_initial(p.faction), world.rules.general(g).name.to_uppercase()),
                None => faction_initial(p.faction).to_owned(),
            };
            for (i, p) in world.players().iter().enumerate() {
                let id = g2_sim::PlayerId(i as u8);
                t.spawn((text(&label(p), 12.0, sim.team_color(id)), Field::Tag(id)));
                if id == sim.local {
                    t.spawn((text("0:00:00", 15.0, TEXT), Field::Clock));
                }
            }
        });

    // Bottom left: power/money strip above the minimap.
    let mut corner = abs(px(0), auto(), auto(), px(0));
    corner.flex_direction = FlexDirection::Column;
    commands.spawn((root(), panel_node(corner), HudPanel)).with_children(|p| {
        p.spawn(Node {
            width: px(MINIMAP),
            justify_content: JustifyContent::SpaceBetween,
            column_gap: px(6),
            ..default()
        })
        .with_children(|s| {
            s.spawn((text("PWR 0/0", 12.0, XP), Field::Power));
            s.spawn((text("$0", 13.0, MONEY), Field::Money));
        });
        p.spawn((text("", 12.0, TEXT), Field::GeneralPower));
        p.spawn((text("", 12.0, XP), Field::Waves));
        p.spawn((
            Node {
                width: px(MINIMAP),
                height: px(MINIMAP),
                margin: UiRect::top(px(4)),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.10, 0.11, 0.08)),
            BorderColor::all(PANEL_EDGE),
            MinimapView,
        ));
    });

    // Bottom right: selection panel with queue and the production grid.
    commands
        .spawn((
            root(),
            panel_node(Node {
                position_type: PositionType::Absolute,
                right: px(0),
                bottom: px(0),
                width: px(360),
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            }),
            HudPanel,
        ))
        .with_children(|p| {
            p.spawn((text("", 14.0, TEXT), Field::SelName));
            p.spawn((text("", 12.0, DIM), Field::SelInfo));
            // Training row: hidden (display none) until the selection has something queued.
            p.spawn((
                Node { align_items: AlignItems::Center, column_gap: px(6), display: Display::None, ..default() },
                QueueRow,
            ))
            .with_children(|q| {
                q.spawn((text("", 12.0, TEXT), Field::Queue));
                q.spawn((
                    Node { width: px(90), height: px(8), ..default() },
                    BackgroundColor(Color::srgb(0.05, 0.05, 0.05)),
                ))
                .with_children(|bar| {
                    bar.spawn((
                        Node { width: percent(0), height: percent(100), ..default() },
                        BackgroundColor(TEAL),
                        Field::QueueBar,
                    ));
                });
            });
            p.spawn(Node {
                display: Display::Grid,
                grid_template_columns: RepeatedGridTrack::flex(3, 1.0),
                row_gap: px(3),
                column_gap: px(3),
                ..default()
            })
            .with_children(|g| {
                for i in 0..9 {
                    g.spawn((
                        Node {
                            height: px(30),
                            border: UiRect::all(px(1)),
                            padding: UiRect::horizontal(px(4)),
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(SLOT),
                        BorderColor::all(PANEL_EDGE),
                        GridButton(i),
                    ))
                    .with_children(|c| {
                        c.spawn((text(&format!("{}", i + 1), 11.0, DIM), Field::GridCell(i)));
                    });
                }
            });
        });

    // Status line (pause/speed/victory), centred above the bottom panels.
    commands
        .spawn((
            root(),
            Node {
                position_type: PositionType::Absolute,
                top: px(58),
                left: percent(50),
                margin: UiRect::left(px(-200)),
                width: px(400),
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|s| {
            s.spawn((text("", 14.0, XP), Field::Status));
        });
}

/// What the alpha bar has no slot for, placed where its own debug and message windows sat:
/// the menu key top-left, the general's power and Onslaught waves above the minimap, the
/// status line top-centre, and the selection's name, state and training queue in the
/// bottom-right panel's unit area.
fn spawn_alpha_extras(commands: &mut Commands) {
    let shadow = || TextShadow { offset: Vec2::splat(1.0), ..default() };
    commands.spawn((root(), abs(px(14), auto(), px(8), auto()), text("ESC  MENU", 14.0, TEXT), shadow()));
    let mut corner = abs(px(8), auto(), auto(), percent(36));
    corner.flex_direction = FlexDirection::Column;
    commands.spawn((root(), corner)).with_children(|p| {
        p.spawn((text("", 13.0, TEXT), shadow(), Field::GeneralPower));
        p.spawn((text("", 13.0, XP), shadow(), Field::Waves));
    });
    let mut status = abs(percent(50), auto(), px(58), auto());
    status.margin = UiRect::left(px(-200));
    status.width = px(400);
    status.justify_content = JustifyContent::Center;
    commands.spawn((root(), status)).with_children(|s| {
        s.spawn((text("", 14.0, XP), shadow(), Field::Status));
    });
    // The selection's text sits over the bottom-right panel's unit area (the alpha drew its
    // own unit card there, which this game doesn't have).
    let mut sel = abs(auto(), percent(23), auto(), percent(2));
    sel.flex_direction = FlexDirection::Column;
    sel.width = percent(13);
    sel.row_gap = px(2);
    commands.spawn((root(), sel)).with_children(|p| {
        p.spawn((text("", 13.0, TEXT), shadow(), Field::SelName));
        p.spawn((text("", 11.0, DIM), shadow(), Field::SelInfo));
        p.spawn((
            Node { align_items: AlignItems::Center, column_gap: px(6), display: Display::None, ..default() },
            QueueRow,
        ))
        .with_children(|q| {
            q.spawn((text("", 11.0, TEXT), Field::Queue));
            q.spawn((
                Node { width: px(60), height: px(6), ..default() },
                BackgroundColor(Color::srgb(0.05, 0.05, 0.05)),
            ))
            .with_children(|bar| {
                bar.spawn((
                    Node { width: percent(0), height: percent(100), ..default() },
                    BackgroundColor(Color::srgb(0.98, 0.89, 0.02)),
                    Field::QueueBar,
                ));
            });
        });
    });
}

/// The alpha's 6×3 ability grid, filled with this game's first nine build slots: the top
/// row and the first three of the second, as the alpha's own buttons were numbered. Each
/// button is the alpha's build icon for that def, with the hotkey and price in its corner.
pub(crate) fn alpha_grid(g: &mut ChildSpawnerCommands, w: f32, h: f32) {
    let (cw, ch) = (w / 6.0, h / 3.0);
    for i in 0..9 {
        let (col, row) = (i % 6, i / 6);
        g.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(col as f32 * cw + 1.0),
                top: px(row as f32 * ch + 1.0),
                width: px(cw - 2.0),
                height: px(ch - 2.0),
                align_items: AlignItems::FlexEnd,
                justify_content: JustifyContent::FlexStart,
                padding: UiRect::all(px(2)),
                ..default()
            },
            GridIcon(i),
            GridButton(i),
            Visibility::Hidden,
        ))
        .with_children(|c| {
            c.spawn((text("", 10.0, TEXT), TextShadow { offset: Vec2::splat(1.0), ..default() }, Field::GridTag(i)));
        });
    }
}

/// Shows each alpha build button that has something to build, with that def's icon.
fn grid_icons(
    mut commands: Commands,
    sim: Res<Sim>,
    sel: Res<Selection>,
    server: Option<Res<AssetServer>>,
    mut cells: Query<(Entity, &GridIcon, &mut Visibility, Option<&ImageNode>)>,
) {
    let Some(server) = server else { return };
    let world = &sim.world;
    let menu = build_menu(&sim, &sel);
    for (e, cell, mut vis, img) in &mut cells {
        let want =
            menu.get(cell.0).and_then(|&d| crate::alpha_hud::icon_for(world.rules.def(d).name)).map(|(icon, _)| icon);
        match want {
            Some(icon) => {
                let handle = crate::alpha_hud::image(&server, icon);
                if img.is_none_or(|i| i.image != handle) {
                    commands.entity(e).insert(ImageNode::new(handle));
                }
                vis.set_if_neq(Visibility::Inherited);
            }
            None => {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }
}

/// The bottom bar's portrait: the selected unit's or building's own portrait, else the
/// faction's general.
fn selection_portrait(
    sim: Res<Sim>,
    sel: Res<Selection>,
    server: Option<Res<AssetServer>>,
    mut slot: Query<(&crate::alpha_hud::PortraitSlot, &mut ImageNode)>,
) {
    let (Some(server), Ok((general, mut img))) = (server, slot.single_mut()) else { return };
    let world = &sim.world;
    let focus = sel.0.first().and_then(|&id| world.entity(id));
    let want = focus
        .and_then(|e| crate::alpha_hud::icon_for(world.rules.def(e.def).name))
        .map_or_else(|| general.0.clone(), |(_, portrait)| crate::alpha_hud::image(&server, portrait));
    if img.image != want {
        img.image = want;
    }
}

/// What the selection can build or train right now, in grid order.
fn build_menu(sim: &Sim, sel: &Selection) -> Vec<g2_sim::DefId> {
    let world = &sim.world;
    let focus = sel.0.first().and_then(|&id| world.entity(id));
    focus.map_or_else(Vec::new, |e| {
        let d = world.rules.def(e.def);
        if !e.is_complete() {
            Vec::new()
        } else if d.is_constructor() {
            d.builds.to_vec()
        } else {
            d.produces.to_vec()
        }
    })
}

pub(crate) fn clock(ticks: u32) -> String {
    let s = ticks / TICKS_PER_SECOND;
    format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

/// "Wave 3/10 in 0:00:41", or "Last wave" once every wave is out; `None` without an
/// `Onslaught` opponent.
fn onslaught_status(world: &g2_sim::World) -> Option<String> {
    use g2_sim::onslaught::{WAVES, next_wave};
    let onslaught = (0..world.players().len()).any(|i| {
        let p = g2_sim::PlayerId(i as u8);
        world.ai_level(p) == Some(g2_sim::AiLevel::Onslaught) && !world.player(p).defeated
    });
    onslaught.then(|| match next_wave(world.tick()) {
        Some((k, at)) => format!("Wave {}/{WAVES} in {}", k + 1, clock(at - world.tick())),
        None => "Last wave: hold out".into(),
    })
}

#[allow(clippy::too_many_arguments)]
fn update_fields(
    sim: Res<Sim>,
    ui: Option<Res<crate::menu::Ui>>,
    frozen: Option<Res<crate::view::FreezeEffects>>,
    sel: Res<Selection>,
    placing: Res<Placement>,
    aim: Option<Res<PowerAim>>,
    mut fields: Query<(&Field, &mut Text, Option<&mut Node>), Without<QueueRow>>,
    mut queue_row: Query<&mut Node, With<QueueRow>>,
) {
    let aim = aim.is_some_and(|a| a.0);
    let world = &sim.world;
    let rules = world.rules;
    let me = world.player(sim.local);
    let focus = sel.0.first().and_then(|&id| world.entity(id));
    if let Ok(mut row) = queue_row.single_mut() {
        let training = focus.is_some_and(|e| !e.queue.is_empty());
        row.display = if training { Display::Flex } else { Display::None };
    }
    let menu: &[g2_sim::DefId] = focus.map_or(&[], |e| {
        let d = rules.def(e.def);
        if !e.is_complete() {
            &[]
        } else if d.is_constructor() {
            d.builds
        } else {
            d.produces
        }
    });
    for (field, mut t, node) in &mut fields {
        match field {
            Field::Clock => t.0 = clock(world.tick()),
            Field::Tag(_) => {}
            Field::Power => {
                t.0 = if me.faction.uses_power() {
                    format!("PWR {}/{}", me.power_drain, me.power_supply)
                } else {
                    "PWR -".into()
                }
            }
            Field::PowerFigures => {
                t.0 =
                    if me.faction.uses_power() { format!("{}/{}", me.power_drain, me.power_supply) } else { "-".into() }
            }
            Field::Money => t.0 = format!("${}", me.credits),
            Field::GeneralPower => {
                t.0 = match (sim.power(), sim.power_ready_in()) {
                    (Some(p), _) if aim => format!("{}: click to aim", p.name.to_uppercase()),
                    (Some(p), Some(0)) => format!("G  {}  READY", p.name.to_uppercase()),
                    (Some(p), Some(t)) => format!("{}  {}", p.name.to_uppercase(), clock(t)),
                    _ => String::new(),
                }
            }
            Field::Waves => t.0 = onslaught_status(world).unwrap_or_default(),
            Field::Status => {
                // The end screen and the Esc pause menu announce themselves; Space pauses
                // with no menu, so that one needs its label here. A network problem comes
                // first: the match is stuck until it clears, or for good.
                let menu_open = ui.as_ref().is_some_and(|u| u.paused || u.screen != crate::menu::Screen::Match);
                let link = sim.online.as_ref().and_then(|n| n.status());
                t.0 = match (link, world.winner(), sim.speed, placing.0) {
                    (Some(s), _, _, _) => s,
                    (_, Some(_), _, _) => String::new(),
                    (_, _, _, Some(def)) => format!("Place {}  -  Esc cancels", rules.def(def).name),
                    // Screenshot mode freezes the frame; a "PAUSED" label there would be a lie.
                    (_, _, 0, _) if !menu_open && frozen.is_none() => "PAUSED".into(),
                    (_, _, 0 | 1, _) => String::new(),
                    (_, _, s, _) => format!("x{s}"),
                }
            }
            Field::SelName => {
                t.0 = match (sel.0.len(), focus) {
                    (0, _) | (_, None) => "No selection".into(),
                    (1, Some(e)) => rules.def(e.def).name.to_uppercase(),
                    (n, Some(_)) => format!("{n} UNITS"),
                }
            }
            Field::SelInfo => {
                t.0 = focus.map_or(String::new(), |e| {
                    let d = rules.def(e.def);
                    let mut s = format!("{}/{}", e.hp, d.hp);
                    if let (Some(done), Some(owner)) = (e.construction, e.owner) {
                        s += &format!("  -  building {}%", done * 100 / world.build_ticks_for(owner, e.def));
                    }
                    if let Class::Building { power, .. } = d.class
                        && power < 0
                        && e.is_complete()
                    {
                        s += if e.powered { "  -  powered" } else { "  -  UNPOWERED" };
                    }
                    if e.cargo > 0 {
                        s += &format!("  -  carrying ${}", e.cargo);
                    }
                    if d.deploy.is_some() {
                        use g2_sim::entity::DeployState;
                        s += match e.deploy {
                            DeployState::Packed => "  -  mobile (T: deploy)",
                            DeployState::Deploying(_) => "  -  setting up",
                            DeployState::Deployed => "  -  DEPLOYED (T: pack up)",
                            DeployState::Packing(_) => "  -  packing up",
                        };
                    }
                    s
                })
            }
            Field::Queue => {
                t.0 = focus.and_then(|e| e.queue.first()).map_or(String::new(), |p| {
                    format!("Training {} ({})", rules.def(p.def).name, focus.unwrap().queue.len())
                })
            }
            Field::QueueBar => {
                if let Some(mut n) = node {
                    let pct = focus.and_then(|e| e.queue.first().zip(e.owner)).map_or(0.0, |(p, owner)| {
                        p.progress_halves as f32 / (world.build_ticks_for(owner, p.def) * 2) as f32 * 100.0
                    });
                    n.width = percent(pct);
                }
            }
            Field::GridCell(i) => {
                t.0 = match menu.get(*i) {
                    Some(&def) => {
                        let d = rules.def(def);
                        let short = d.name.split_once(' ').map_or(d.name, |(_, rest)| rest);
                        format!("{} {}  ${}", i + 1, short, world.cost_for(sim.local, def).credits)
                    }
                    None => format!("{}", i + 1),
                }
            }
            Field::GridTag(i) => {
                t.0 = menu
                    .get(*i)
                    .map_or(String::new(), |&def| format!("{} ${}", i + 1, world.cost_for(sim.local, def).credits))
            }
        }
    }
}

/// Minimap: a fog layer (the local player's cells: black unexplored, dimmed out of sight),
/// then one dot per shown entity, team-coloured; resources neutral; refreshed every frame.
#[allow(clippy::too_many_arguments)]
fn minimap(
    mut commands: Commands,
    sim: Res<Sim>,
    view: Query<(Entity, &ComputedNode, &Node), With<MinimapView>>,
    dots: Query<Entity, With<MinimapDot>>,
    mut fog_image: Local<Option<Handle<Image>>>,
    mut states: Local<Vec<u8>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Ok((map_node, computed, node)) = view.single() else { return };
    let side = minimap_side(computed, node);
    for d in &dots {
        commands.entity(d).despawn();
    }
    let world = &sim.world;
    let (w, h) = (world.map.width.to_f32(), world.map.height.to_f32());
    let v = world.vision(sim.local);
    sim.fog_states(&mut states);
    let shade = [[0u8, 0, 0, 255], [0, 0, 0, 140], [0, 0, 0, 0]];
    let pixels: Vec<u8> = states.iter().flat_map(|&s| shade[s as usize]).collect();
    let handle = fog_image.get_or_insert_with(|| {
        let size =
            bevy::render::render_resource::Extent3d { width: v.w as u32, height: v.h as u32, depth_or_array_layers: 1 };
        let mut image = Image::new(
            size,
            bevy::render::render_resource::TextureDimension::D2,
            pixels.clone(),
            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
            bevy::asset::RenderAssetUsages::default(),
        );
        image.sampler = bevy::image::ImageSampler::linear();
        images.add(image)
    });
    if let Some(mut image) = images.get_mut(&*handle)
        && image.data.as_deref() != Some(&pixels[..])
    {
        image.data = Some(pixels);
    }
    // The fog grid overshoots the map by part of a cell; scale it so cell edges line up.
    let cell = g2_sim::fog::CELL as f32;
    let (fw, fh) = (v.w as f32 * cell / w * side, v.h as f32 * cell / h * side);
    commands.entity(map_node).with_children(|m| {
        m.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: px(fw),
                height: px(fh),
                ..default()
            },
            ImageNode::new(handle.clone()),
            MinimapDot,
        ));
        // Shown entities, plus ghosts: remembered buildings destroyed out of sight, drawn where
        // they were seen.
        let ghosts = world
            .vision(sim.local)
            .memory
            .iter()
            .filter(|(id, _)| sim.fog && world.entity(**id).is_none())
            .map(|(_, s)| (s.def, Some(s.owner), s.pos));
        let dots = world.entities().filter(|e| sim.shows(e)).map(|e| (e.def, e.owner, e.pos)).chain(ghosts);
        for (def_id, owner, pos) in dots {
            let def = world.rules.def(def_id);
            let (color, size) = match (def.class, owner) {
                // Resources in neutral greys, so they never read as a faction.
                (Class::SupplyPile, _) => (Color::srgb(0.80, 0.80, 0.78), 4.0),
                (Class::OilWell, _) => (Color::srgb(0.35, 0.35, 0.35), 4.0),
                (_, Some(o)) => {
                    // Same team colours as the match bar and the models.
                    (sim.team_color(o), if def.is_building() { 6.0 } else { 3.0 })
                }
                _ => continue,
            };
            let (x, y) = (pos.x.to_f32() / w * side, pos.y.to_f32() / h * side);
            m.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(x - size / 2.0),
                    top: px(y - size / 2.0),
                    width: px(size),
                    height: px(size),
                    ..default()
                },
                BackgroundColor(color),
                MinimapDot,
            ));
        }
    });
}

/// World-anchored overlays: collector counts over own supply centers and health bars.
#[allow(clippy::too_many_arguments)]
fn overlays(
    mut commands: Commands,
    sim: Res<Sim>,
    sel: Res<Selection>,
    cam: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
    labels: Query<Entity, With<WorldLabel>>,
    bars: Query<Entity, With<Overlay>>,
    panels: Panels,
) {
    for e in labels.iter().chain(bars.iter()) {
        commands.entity(e).despawn();
    }
    let Ok((camera, cam_tf)) = cam.single() else { return };
    let view = camera.logical_viewport_size().unwrap_or(Vec2::ZERO);
    // An anchor outside the viewport draws no overlay: clamping one to the edge would detach it
    // from what it labels. For an anchor on screen, only the width is kept inside, and a label
    // that would run off the top goes below its anchor instead of pinning to the frame.
    let place = |screen: Vec2, half_w: f32| -> Option<Vec2> {
        if screen.x < -half_w || screen.y < -20.0 || screen.x > view.x + half_w || screen.y > view.y + 20.0 {
            return None;
        }
        let lo = (half_w + 2.0).min(view.x / 2.0);
        let hi = (view.x - half_w - 2.0).max(view.x / 2.0);
        Some(Vec2::new(screen.x.clamp(lo, hi), screen.y))
    };
    let world = &sim.world;
    for e in world.entities().filter(|e| sim.shows(e)) {
        let anchor = crate::view::overlay_anchor(&sim, e);
        // Raw placement is what decides whether an overlay is drawn at all: the HUD corners
        // (menu and clock along the top, minimap and selection panel at the bottom) would
        // cover a label here, and clamping one in from off-screen would pin it to the chrome.
        let raw = match camera.world_to_viewport(cam_tf, anchor) {
            Ok(p) if p.x >= 8.0 && p.x <= view.x - 8.0 && p.y >= 8.0 && p.y <= view.y - 8.0 => p,
            _ => continue,
        };
        // The HUD's panels own the screen under them: nothing anchored there is drawn.
        if over_hud(&panels, raw) {
            continue;
        }
        let Some(screen) = place(raw, 52.0) else { continue };
        // The label sits 44 px above its anchor: it needs that much clear sky, or it goes
        // below the anchor instead of into the HUD strip along the top.
        let label_y = if screen.y >= 88.0 { screen.y - 44.0 } else { screen.y + 10.0 };
        if let Some((n, cap)) = crate::view::depot_label(&sim, e) {
            let color = if n >= cap { Color::srgb(0.95, 0.25, 0.2) } else { Color::srgb(0.3, 0.95, 0.3) };
            commands.spawn((
                abs(px(screen.x - 50.0), auto(), px(label_y), auto()),
                text(&format!("COLLECTORS {n}/{cap}"), 12.0, color),
                // A tight shadow for contrast; the 4 px default reads as a doubled label at 12 px.
                TextShadow { offset: Vec2::splat(1.0), ..default() },
                WorldLabel,
            ));
        }
        let def = world.rules.def(e.def);
        // Cargo pips over own collectors: five segments, filled by load.
        if e.owner == Some(sim.local)
            && let Some(spec) = def.collector()
        {
            let filled = cargo_pips(e.cargo, spec.capacity);
            let Some(screen) = place(screen, 22.0) else { continue };
            commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(screen.x - 20.0),
                        top: px(screen.y - 18.0),
                        column_gap: px(2),
                        padding: UiRect::all(px(1)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.75)),
                    Overlay,
                ))
                .with_children(|row| {
                    for i in 0..5 {
                        let c = if i < filled { MONEY } else { Color::srgb(0.18, 0.20, 0.20) };
                        row.spawn((Node { width: px(6), height: px(5), ..default() }, BackgroundColor(c)));
                    }
                });
        }
        let selected = sel.0.contains(&e.id);
        let damaged = e.hp < def.hp;
        if e.owner.is_some() && (selected || damaged) {
            let w = if def.is_building() { 46.0 } else { 22.0 };
            let frac = (e.hp as f32 / def.hp.max(1) as f32).clamp(0.0, 1.0);
            let color = if frac > 0.5 {
                Color::srgb(0.3, 0.95, 0.3)
            } else if frac > 0.25 {
                Color::srgb(0.95, 0.85, 0.2)
            } else {
                Color::srgb(0.95, 0.25, 0.2)
            };
            let Some(screen) = place(screen, w / 2.0) else { continue };
            commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(screen.x - w / 2.0),
                        top: px(screen.y - 8.0),
                        width: px(w),
                        height: px(4),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
                    Overlay,
                ))
                .with_children(|b| {
                    b.spawn((
                        Node { width: percent(frac * 100.0), height: percent(100), ..default() },
                        BackgroundColor(color),
                    ));
                });
        }
    }
}

/// Filled cargo pips out of five: any cargo lights the first, a full load lights all five.
fn cargo_pips(cargo: i64, capacity: i64) -> i64 {
    ((cargo * 5 + capacity - 1) / capacity.max(1)).clamp(0, 5)
}

#[cfg(test)]
mod tests {
    #[test]
    fn cargo_pips_round_up_and_cap_at_five() {
        let pips = [0, 1, 20, 21, 99, 100, 140].map(|c| super::cargo_pips(c, 100));
        assert_eq!(pips, [0, 1, 1, 2, 5, 5, 5]);
        assert_eq!(super::cargo_pips(40, 40), 5, "a GLA worker's full 40-credit load");
    }
}
