//! HUD (docs/STYLE.md): conventional RTS zones (minimap bottom-left, selection and production
//! bottom-right, clock top-centre) in our own framed design: bevelled gunmetal plates with a
//! trim line in the local player's faction colour (`art.rs`). Every element shows live sim
//! data; the frames are the only decoration.
//!
//! - Top centre: a compact match bar with both factions' tags around the clock.
//! - Top left: menu label; top right: neutral project label (no EA trademarks in the UI).
//! - Bottom left: a header strip with power (figures and a load meter) and credits over the
//!   minimap, which sits in a recessed bezel, with team and resource dots.
//! - Bottom centre: the support powers as framed tiles with a cooldown sweep.
//! - Bottom right: the selection card (a picture of the selected model in a portrait bezel,
//!   name, health, status, training queue with progress) and the 3×3 build/production tray,
//!   each button with its picture, name, price and the progress of what it is training.
//! - World overlays: collector count over own supply centers (red at the faction cap), health bars
//!   over the selection and damaged entities.
//!
//! Everything scales with the window's height over 720 lines, so the HUD keeps its share of the
//! screen (and its text stays legible) at 1080p and above.

mod art;

use bevy::color::Mix;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use g2_sim::entity::Product;
use g2_sim::{Class, Faction, TICKS_PER_SECOND};

use crate::camera::RtsCamera;
use crate::input::{Placement, PowerAim, Selection};
use crate::sim::Sim;
use art::{Face, Icon};

pub(crate) const PANEL: Color = Color::srgba(0.07, 0.08, 0.09, 0.92);
pub(crate) const PANEL_EDGE: Color = Color::srgb(0.24, 0.27, 0.29);
pub(crate) const TEAL: Color = Color::srgb(0.18, 0.78, 0.86);
const MONEY: Color = Color::srgb(0.35, 0.95, 0.35);
const XP: Color = Color::srgb(0.98, 0.86, 0.12);
/// Prices the player can't pay, and a power drain over supply.
const SHORT: Color = Color::srgb(0.95, 0.32, 0.25);
pub(crate) const TEXT: Color = Color::srgb(0.88, 0.90, 0.90);
pub(crate) const DIM: Color = Color::srgb(0.55, 0.58, 0.60);
/// Status lines on the plates: lighter than `DIM`, which is too faint on gunmetal at 12 px.
const INFO: Color = Color::srgb(0.70, 0.74, 0.76);
const MINIMAP: f32 = 196.0;
/// The keys that arm each power slot (`input::POWER_KEYS`), as the tiles label them.
const POWER_LABELS: [&str; crate::input::POWER_KEYS.len()] = ["G", "F1", "F2", "F3", "F4", "F5", "F6"];

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
    /// What an armed power or special shot waits for ("click to aim"); empty otherwise. The
    /// power tiles show the rest of what `GeneralPower` says.
    Aim,
    /// Countdown to the next Onslaught wave; empty without an Onslaught opponent.
    Waves,
    Status,
    SelName,
    SelInfo,
    Queue,
    /// A build-tray button's name.
    GridCell(usize),
    /// A build-tray button's price, red when the player can't pay it.
    GridCost(usize),
    /// An alpha build button's corner text: its price (the icon names the def).
    GridTag(usize),
}

/// A bar that fills to a live fraction: power drawn of supplied, the selection's health, the
/// training item's progress, and each build button's share of it.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Meter {
    Power,
    Health,
    Queue,
    Grid(usize),
}

#[derive(Component)]
struct MinimapDot;

#[derive(Component)]
pub(crate) struct MinimapView;

/// A build-grid button's icon in the alpha HUD, shown when its cell has something to build.
#[derive(Component)]
struct GridIcon(usize);

/// A build-tray button's class silhouette in our HUD.
#[derive(Component)]
struct GridGlyph(usize);

/// How many of a build-tray button's item the selection has queued.
#[derive(Component)]
struct GridCount(usize);

/// The selection card's silhouette, hidden without a selection.
#[derive(Component)]
struct Portrait;

/// The selection card's health bar's track, hidden without a selection.
#[derive(Component)]
struct HealthTrack;

/// A support-power tile's face, by power slot (as `input::POWER_KEYS`).
#[derive(Component)]
pub(crate) struct PowerTile(pub u8);

/// The shade over a recharging power's tile, swept away clockwise as it charges.
#[derive(Component)]
struct PowerSweep(u8);

/// A power tile's countdown, READY, or AIM.
#[derive(Component)]
struct PowerTimer(u8);

/// An alpha power frame's icon, by power slot: the alpha's own button art for the power in
/// that slot, hidden while the slot is empty.
#[derive(Component)]
struct AlphaPowerIcon(u8);

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
        app.add_systems(Startup, (load_alpha_layout, art::make))
            .add_systems(
                Update,
                spawn.run_if(crate::menu::scene_start.or_eager(resized_in_match)).after(crate::menu::MenuSet),
            )
            .add_systems(
                Update,
                (update_fields, tags, minimap, overlays, popups, grid_icons, selection_portrait, alpha_power_icons)
                    .after(spawn),
            )
            .add_systems(Update, (meters, grid_faces, power_tiles, portrait).after(spawn));
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

/// A small plain panel for transient pop-ups (tooltips and the like), which stay lighter than
/// the framed plates.
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

/// Sizes and type for one HUD build: everything is laid out for 1280×720 and scaled by `k`, so
/// the HUD keeps its share of the screen at any size.
struct Pen {
    k: f32,
    regular: Option<Handle<Font>>,
    bold: Option<Handle<Font>>,
}

impl Pen {
    fn new(screen: Vec2, fonts: Option<&crate::menu::MenuFonts>) -> Self {
        Pen {
            // The bottom plates span about 1130 of 1280 columns, so a narrower window scales
            // by its width; below 0.85 the 11 px labels would drop under 10 px.
            k: (screen.y / 720.0).min(screen.x / 1280.0).clamp(0.85, 3.0),
            regular: fonts.map(|f| f.regular.clone()),
            bold: fonts.map(|f| f.bold.clone()),
        }
    }

    fn px(&self, v: f32) -> Val {
        px(v * self.k)
    }

    fn text(&self, s: &str, size: f32, color: Color, bold: bool) -> impl Bundle {
        let mut font = TextFont { font_size: FontSize::Px(size * self.k), ..default() };
        if let Some(f) = if bold { &self.bold } else { &self.regular } {
            font.font = f.clone().into();
        }
        (Text::new(s), font, TextColor(color))
    }
}

/// A tight drop shadow for text on the world or on bright trim; the 4 px default reads as a
/// doubled label at these sizes.
fn shadow() -> TextShadow {
    TextShadow { offset: Vec2::splat(1.0), ..default() }
}

/// The bottom-left plate's width: the minimap, its bezel (5 a side) and the plate's padding.
const LEFT_PLATE: f32 = MINIMAP + 10.0 + 24.0;

#[allow(clippy::too_many_arguments)]
fn spawn(
    mut commands: Commands,
    sim: Res<Sim>,
    layout: Option<Res<crate::alpha_hud::Layout>>,
    server: Option<Res<AssetServer>>,
    art: Option<Res<art::HudArt>>,
    fonts: Option<Res<crate::menu::MenuFonts>>,
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
    let faction = world.player(sim.local).faction;
    if let (Some(layout), Some(server)) = (layout, server)
        && crate::alpha_hud::spawn(&mut commands, &layout, &server, faction, screen)
    {
        spawn_alpha_extras(&mut commands);
        return;
    }
    let Some(art) = art else { return };
    let kit = art.kit(faction);
    let trim = art::trim(faction);
    let pen = Pen::new(screen, fonts.as_deref());
    let k = pen.k;
    let plate = || art::sliced(kit.plate.clone(), art::PLATE_EDGE, k);
    let well = || art::sliced(kit.well.clone(), art::WELL_EDGE, k);
    // Inside a bottom plate: clear of its bevel and trim, and of the cut corners on top.
    let plate_pad = UiRect::new(pen.px(12.0), pen.px(12.0), pen.px(13.0), pen.px(9.0));

    // Top left: the in-match menu key; top right: neutral project label (no EA trademarks).
    commands.spawn((
        root(),
        abs(pen.px(14.0), auto(), pen.px(8.0), auto()),
        pen.text("ESC  MENU", 13.0, TEXT, true),
        shadow(),
    ));
    commands.spawn((
        root(),
        abs(auto(), pen.px(14.0), pen.px(8.0), auto()),
        pen.text("OPEN RTS", 12.0, DIM, true),
        shadow(),
    ));

    // Top centre: one compact match bar hanging from the top edge, its top rim above the
    // screen: own tag, clock, then every opponent's tag in its team colour (dimmed once
    // defeated); a general shows after its faction.
    commands
        .spawn((
            root(),
            Node {
                position_type: PositionType::Absolute,
                top: pen.px(-7.0),
                left: percent(50),
                margin: UiRect::left(pen.px(-220.0)),
                width: pen.px(440.0),
                height: pen.px(38.0),
                padding: UiRect::new(pen.px(14.0), pen.px(14.0), pen.px(8.0), pen.px(6.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                column_gap: pen.px(14.0),
                ..default()
            },
            art::sliced(kit.bar.clone(), art::BAR_EDGE, k),
            HudPanel,
        ))
        .with_children(|t| {
            let label = |p: &g2_sim::Player| match p.general {
                Some(g) => format!("{} {}", faction_initial(p.faction), world.rules.general(g).name.to_uppercase()),
                None => faction_initial(p.faction).to_owned(),
            };
            for (i, p) in world.players().iter().enumerate() {
                let id = g2_sim::PlayerId(i as u8);
                t.spawn((pen.text(&label(p), 12.0, sim.team_color(id), true), Field::Tag(id)));
                if id == sim.local {
                    t.spawn((pen.text("0:00:00", 16.0, TEXT, true), Field::Clock));
                }
            }
        });
    // Onslaught's wave countdown under the bar, and the status line (link, placement, pause,
    // speed) under that.
    for (top, field) in [(36.0, Field::Waves), (58.0, Field::Status)] {
        let mut line = abs(percent(50), auto(), pen.px(top), auto());
        line.margin = UiRect::left(pen.px(-220.0));
        line.width = pen.px(440.0);
        line.justify_content = JustifyContent::Center;
        commands.spawn((root(), line)).with_children(|s| {
            s.spawn((pen.text("", 14.0, XP, true), shadow(), field));
        });
    }

    // Bottom left: the header strip (power with its load meter, credits) over the minimap in
    // its bezel.
    let mut corner = abs(px(0), auto(), auto(), px(0));
    corner.flex_direction = FlexDirection::Column;
    corner.width = pen.px(LEFT_PLATE);
    corner.padding = plate_pad;
    commands.spawn((root(), corner, plate(), HudPanel)).with_children(|p| {
        p.spawn((
            Node {
                height: pen.px(22.0),
                padding: UiRect::horizontal(pen.px(6.0)),
                margin: UiRect::bottom(pen.px(5.0)),
                border: UiRect::all(px(1)),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundGradient::from(LinearGradient::to_bottom(vec![
                ColorStop::auto(Color::srgba(0.0, 0.0, 0.0, 0.8)),
                ColorStop::auto(Color::srgba(0.06, 0.07, 0.08, 0.8)),
            ])),
            BorderColor::all(trim.with_alpha(0.4)),
        ))
        .with_children(|h| {
            h.spawn(Node { align_items: AlignItems::Center, column_gap: pen.px(6.0), ..default() }).with_children(
                |pw| {
                    pw.spawn((pen.text("PWR 0/0", 12.0, XP, true), Field::Power));
                    // GLA runs without power: no meter to fill.
                    if faction.uses_power() {
                        pw.spawn((
                            Node { width: pen.px(44.0), height: pen.px(6.0), ..default() },
                            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
                        ))
                        .with_child((
                            Node { width: percent(0), height: percent(100), ..default() },
                            BackgroundColor(XP),
                            Meter::Power,
                        ));
                    }
                },
            );
            h.spawn((pen.text("$0", 14.0, MONEY, true), Field::Money));
        });
        p.spawn((Node { padding: UiRect::all(pen.px(5.0)), ..default() }, well())).with_children(|w| {
            w.spawn((
                // The fog layer overshoots the map by part of a cell; the bezel's edge cuts it.
                Node { width: pen.px(MINIMAP), height: pen.px(MINIMAP), overflow: Overflow::clip(), ..default() },
                BackgroundColor(Color::srgb(0.10, 0.11, 0.08)),
                MinimapView,
            ));
        });
    });

    // Bottom centre, beside the minimap: the general's and the faction's support powers as
    // tiles, with the line that says what an armed power waits for above them.
    let powers: Vec<(usize, g2_sim::defs::Power)> =
        sim.powers().into_iter().enumerate().filter_map(|(slot, p)| Some((slot, p?))).collect();
    let tray_left = LEFT_PLATE + 4.0;
    let mut hint = abs(pen.px(tray_left + 8.0), auto(), auto(), pen.px(118.0));
    hint.width = pen.px(420.0);
    commands.spawn((root(), hint)).with_children(|h| {
        h.spawn((pen.text("", 13.0, XP, true), shadow(), Field::Aim));
    });
    if !powers.is_empty() {
        let mut tray = abs(pen.px(tray_left), auto(), auto(), px(0));
        tray.column_gap = pen.px(4.0);
        tray.padding = plate_pad;
        commands.spawn((root(), tray, plate(), HudPanel)).with_children(|t| {
            for (slot, p) in powers {
                power_tile(t, &pen, &art, kit, slot, &p);
            }
        });
    }

    // Bottom right: the selection card (portrait, name, health, status, training) beside the
    // 3×3 build tray.
    let mut right = abs(auto(), px(0), auto(), px(0));
    right.column_gap = pen.px(10.0);
    right.padding = plate_pad;
    commands.spawn((root(), right, plate(), HudPanel)).with_children(|p| {
        p.spawn(Node { width: pen.px(236.0), column_gap: pen.px(8.0), ..default() }).with_children(|c| {
            c.spawn((
                Node {
                    width: pen.px(78.0),
                    height: pen.px(78.0),
                    padding: UiRect::all(pen.px(5.0)),
                    flex_shrink: 0.0,
                    ..default()
                },
                well(),
            ))
            .with_children(|w| {
                w.spawn((
                    Node {
                        width: percent(100),
                        height: percent(100),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundGradient::from(RadialGradient::new(
                        UiPosition::CENTER,
                        RadialGradientShape::FarthestCorner,
                        vec![
                            ColorStop::auto(trim.mix(&Color::BLACK, 0.72)),
                            ColorStop::auto(Color::srgb(0.02, 0.02, 0.03)),
                        ],
                    )),
                ))
                .with_child((
                    Node { width: percent(PORTRAIT_GLYPH), height: percent(PORTRAIT_GLYPH), ..default() },
                    ImageNode::new(art.icon(Icon::Building)),
                    Visibility::Hidden,
                    Portrait,
                ));
            });
            c.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, row_gap: pen.px(3.0), ..default() })
                .with_children(|col| {
                    col.spawn((pen.text("", 14.0, TEXT, true), Field::SelName));
                    col.spawn((
                        Node { height: pen.px(5.0), ..default() },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
                        Visibility::Hidden,
                        HealthTrack,
                    ))
                    .with_child((
                        Node { width: percent(0), height: percent(100), ..default() },
                        BackgroundColor(MONEY),
                        Meter::Health,
                    ));
                    col.spawn((pen.text("", 11.0, INFO, false), Field::SelInfo));
                    // Training: hidden (display none) until the selection has something queued.
                    col.spawn((
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: pen.px(2.0),
                            display: Display::None,
                            ..default()
                        },
                        QueueRow,
                    ))
                    .with_children(|q| {
                        q.spawn((pen.text("", 11.0, TEXT, false), Field::Queue));
                        q.spawn((
                            Node { height: pen.px(6.0), ..default() },
                            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
                        ))
                        .with_child((
                            Node { width: percent(0), height: percent(100), ..default() },
                            BackgroundColor(trim),
                            Meter::Queue,
                        ));
                    });
                });
        });
        p.spawn((
            Node {
                display: Display::Grid,
                grid_template_columns: RepeatedGridTrack::px(3, 96.0 * k),
                row_gap: pen.px(3.0),
                column_gap: pen.px(3.0),
                padding: UiRect::all(pen.px(3.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
        ))
        .with_children(|g| {
            for i in 0..9 {
                grid_button(g, &pen, &art, kit, trim, i);
            }
        });
    });
}

/// One build-tray button: the slot's picture (its class icon until the picture is taken), name
/// and price on a framed face, the progress of what the selection is training from it along
/// the bottom edge, and the count queued.
fn grid_button(g: &mut ChildSpawnerCommands, pen: &Pen, art: &art::HudArt, kit: &art::Kit, trim: Color, i: usize) {
    g.spawn((
        Node {
            height: pen.px(48.0),
            padding: UiRect::new(pen.px(5.0), pen.px(4.0), pen.px(3.0), pen.px(5.0)),
            align_items: AlignItems::Center,
            overflow: Overflow::clip(),
            ..default()
        },
        art::sliced(kit.face(Face::Off), art::FACE_EDGE, pen.k),
        RelativeCursorPosition::default(),
        GridButton(i),
    ))
    .with_children(|b| {
        // The picture sits to the right, under the name and price where a long name runs
        // over it; a square as tall as the face inside its bevel.
        let mut picture = abs(auto(), pen.px(3.0), pen.px(3.0), auto());
        (picture.width, picture.height) = (pen.px(GLYPH), pen.px(GLYPH));
        b.spawn((picture, ImageNode::new(art.icon(Icon::Vehicle)), Visibility::Hidden, GridGlyph(i)));
        b.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, flex_shrink: 1.0, ..default() })
            .with_children(|t| {
                t.spawn((pen.text("", 10.5, TEXT, true), shadow(), Field::GridCell(i)));
                t.spawn((pen.text("", 11.0, MONEY, false), shadow(), Field::GridCost(i)));
            });
        let mut track = abs(pen.px(4.0), pen.px(4.0), auto(), pen.px(3.0));
        track.height = pen.px(3.0);
        b.spawn(track).with_child((
            Node { width: percent(0), height: percent(100), ..default() },
            BackgroundColor(trim),
            Meter::Grid(i),
        ));
        b.spawn((abs(auto(), pen.px(5.0), pen.px(2.0), auto()), pen.text("", 11.0, XP, true), shadow(), GridCount(i)));
    });
}

/// One support-power tile: its icon on a framed face with its key, a sweep that uncovers the
/// face as the power recharges, the time left (or READY), and its name under it.
fn power_tile(
    t: &mut ChildSpawnerCommands,
    pen: &Pen,
    art: &art::HudArt,
    kit: &art::Kit,
    slot: usize,
    power: &g2_sim::defs::Power,
) {
    t.spawn(Node {
        width: pen.px(72.0),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        row_gap: pen.px(3.0),
        ..default()
    })
    .with_children(|c| {
        c.spawn((
            Node {
                width: pen.px(56.0),
                height: pen.px(56.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            art::sliced(kit.face(Face::Off), art::FACE_EDGE, pen.k),
            PowerTile(slot as u8),
        ))
        .with_children(|f| {
            f.spawn((
                Node { width: pen.px(38.0), height: pen.px(38.0), ..default() },
                ImageNode::new(art.icon(Icon::of_power(&power.kind))),
            ));
            f.spawn((
                abs(pen.px(2.0), pen.px(2.0), pen.px(2.0), pen.px(2.0)),
                BackgroundGradient::default(),
                PowerSweep(slot as u8),
            ));
            f.spawn((
                abs(pen.px(5.0), auto(), pen.px(2.0), auto()),
                pen.text(POWER_LABELS[slot], 10.0, TEXT, true),
                shadow(),
            ));
            let mut foot = abs(px(0), px(0), auto(), pen.px(3.0));
            foot.justify_content = JustifyContent::Center;
            f.spawn(foot).with_child((pen.text("", 11.0, TEXT, true), shadow(), PowerTimer(slot as u8)));
        });
        c.spawn((
            pen.text(&power.name.to_uppercase(), 10.0, INFO, true),
            TextLayout::default().with_justify(Justify::Center),
        ));
    });
}

/// One of the alpha bar's power frames, filled like our tray's tiles: the alpha's button art
/// for the power in `slot`, the charging sweep over it, its key in the corner and the time
/// left (or READY, AIM) along the bottom. Hidden while the slot is empty.
pub(crate) fn alpha_power(t: &mut ChildSpawnerCommands, slot: u8, font: TextFont) {
    let shadow = || TextShadow { offset: Vec2::splat(1.0), ..default() };
    t.spawn((
        Node { width: percent(100), height: percent(100), ..default() },
        ImageNode::default(),
        AlphaPowerIcon(slot),
        Visibility::Hidden,
    ))
    .with_children(|f| {
        f.spawn((abs(px(0), px(0), px(0), px(0)), BackgroundGradient::default(), PowerSweep(slot)));
        f.spawn((
            abs(px(3), auto(), px(1), auto()),
            Text::new(POWER_LABELS[slot as usize]),
            font.clone(),
            TextColor(TEXT),
            shadow(),
        ));
        let mut foot = abs(px(0), px(0), auto(), px(2));
        foot.justify_content = JustifyContent::Center;
        f.spawn(foot).with_child((Text::new(""), font, TextColor(TEXT), shadow(), PowerTimer(slot)));
    });
}

/// Puts each alpha power frame's button art up for the power now in its slot (a perk power
/// fills the frame after the faction's, a level change keeps the name) and hides empty ones.
fn alpha_power_icons(
    mut commands: Commands,
    sim: Res<Sim>,
    server: Option<Res<AssetServer>>,
    mut cells: Query<(Entity, &AlphaPowerIcon, &mut Visibility, &ImageNode)>,
) {
    let Some(server) = server else { return };
    let powers = sim.powers();
    for (e, cell, mut vis, img) in &mut cells {
        let icon = powers.get(cell.0 as usize).copied().flatten().and_then(|p| crate::alpha_hud::power_icon(p.name));
        match icon {
            Some(icon) => {
                let handle = crate::alpha_hud::image(&server, icon);
                if img.image != handle {
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
                    Meter::Queue,
                ));
            });
        });
    });
}

/// The alpha's 6×3 ability grid, filled with this game's first nine build slots: the top
/// row and the first three of the second, as the alpha's own buttons were numbered. Each
/// button is the alpha's build icon for that def, with the price in its corner.
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

/// Shows each alpha build button that has something to build, with that slot's icon.
fn grid_icons(
    mut commands: Commands,
    sim: Res<Sim>,
    sel: Res<Selection>,
    server: Option<Res<AssetServer>>,
    mut cells: Query<(Entity, &GridIcon, &mut Visibility, Option<&ImageNode>)>,
) {
    let Some(server) = server else { return };
    let menu = build_menu(&sim, &sel);
    for (e, cell, mut vis, img) in &mut cells {
        let want = menu.get(cell.0).and_then(|slot| slot.icon(sim.world.rules));
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

/// Fills the meters: power drawn of supplied (red over supply), the selection's health (green,
/// yellow, red as the world bars), the first training item's progress, and that progress again
/// along the build button it came from.
fn meters(sim: Res<Sim>, sel: Res<Selection>, mut bars: Query<(&Meter, &mut Node, &mut BackgroundColor)>) {
    let world = &sim.world;
    let me = world.player(sim.local);
    let focus = sel.0.first().and_then(|&id| world.entity(id));
    let training = focus
        .and_then(|e| e.queue.first().zip(e.owner))
        .map(|(p, owner)| (p.item, p.progress_halves as f32 / (world.item_ticks(owner, p.item) * 2).max(1) as f32));
    let menu = build_menu(&sim, &sel);
    for (meter, mut node, mut color) in &mut bars {
        let frac = match *meter {
            Meter::Power => {
                let over = me.power_drain > me.power_supply;
                color.set_if_neq(BackgroundColor(if over { SHORT } else { XP }));
                me.power_drain as f32 / me.power_supply.max(1) as f32
            }
            Meter::Health => focus.map_or(0.0, |e| {
                let f = e.hp as f32 / world.max_hp(e).max(1) as f32;
                color.set_if_neq(BackgroundColor(health_color(f)));
                f
            }),
            Meter::Queue => training.map_or(0.0, |(_, f)| f),
            Meter::Grid(i) => match (training, menu.get(i)) {
                (Some((item, f)), Some(&slot)) if slot.is(item) => f,
                _ => 0.0,
            },
        };
        let width = percent(frac.clamp(0.0, 1.0) * 100.0);
        if node.width != width {
            node.width = width;
        }
    }
}

/// Health bar colour: green above half, yellow above a quarter, red below.
fn health_color(frac: f32) -> Color {
    if frac > 0.5 {
        Color::srgb(0.3, 0.95, 0.3)
    } else if frac > 0.25 {
        Color::srgb(0.95, 0.85, 0.2)
    } else {
        Color::srgb(0.95, 0.25, 0.2)
    }
}

/// Sets each build-tray button's face and picture from its slot: off when empty or
/// unaffordable, pressed while held or while its building is being placed, lit under the
/// cursor; and the count of its item in the selection's queue. A def's picture is its model
/// (`thumbs.rs`), its class icon until that is taken; an upgrade's is the upgrade icon.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn grid_faces(
    sim: Res<Sim>,
    sel: Res<Selection>,
    placing: Res<Placement>,
    art: Option<Res<art::HudArt>>,
    mut thumbs: Option<ResMut<crate::thumbs::Thumbs>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    forced: Option<Res<crate::tooltip::HoverSlot>>,
    mut faces: Query<(&GridButton, &RelativeCursorPosition, &mut ImageNode), (Without<GridIcon>, Without<GridGlyph>)>,
    mut glyphs: Query<(&GridGlyph, &mut ImageNode, &mut Visibility), Without<GridButton>>,
    mut counts: Query<(&GridCount, &mut Text)>,
) {
    let Some(art) = art else { return };
    let world = &sim.world;
    let kit = art.kit(world.player(sim.local).faction);
    let credits = world.player(sim.local).credits;
    let menu = build_menu(&sim, &sel);
    let held = mouse.is_some_and(|m| m.pressed(MouseButton::Left));
    let queue = sel.0.first().and_then(|&id| world.entity(id)).map_or(&[][..], |e| &e.queue[..]);
    let usable = |i: usize| menu.get(i).is_some_and(|&slot| slot_cost(&sim, slot) <= credits);
    let mut states = [Face::Off; 9];
    for (button, cursor, mut img) in &mut faces {
        let slot = menu.get(button.0).copied();
        // A headless shot's `--hover-slot` lights its button as the cursor would.
        let over = cursor.cursor_over() || forced.as_ref().is_some_and(|f| f.0 == button.0);
        let face = match slot {
            Some(Slot::Def(d)) if placing.0 == Some(d) => Face::Pressed,
            _ if !usable(button.0) => Face::Off,
            _ if over && held => Face::Pressed,
            _ if over => Face::Hover,
            _ => Face::Idle,
        };
        if let Some(s) = states.get_mut(button.0) {
            *s = face;
        }
        let want = kit.face(face);
        if img.image != want {
            img.image = want;
        }
    }
    for (glyph, mut img, mut vis) in &mut glyphs {
        let Some(&slot) = menu.get(glyph.0) else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        let picture = match (slot, thumbs.as_deref_mut()) {
            (Slot::Def(d), Some(t)) => t.get(crate::thumbs::Key::of(&sim, d, Some(sim.local))),
            _ => None,
        };
        let tint = match picture {
            Some(_) => picture_tint(states.get(glyph.0).copied().unwrap_or(Face::Off)),
            // An unaffordable button's icon fades with its face.
            None if usable(glyph.0) => Color::WHITE,
            None => Color::srgba(1.0, 1.0, 1.0, 0.35),
        };
        let want = picture.unwrap_or_else(|| {
            art.icon(match slot {
                Slot::Def(d) => Icon::of_def(world.rules.def(d)),
                Slot::Upgrade(_) => Icon::Upgrade,
            })
        });
        if img.image != want {
            img.image = want;
        }
        if img.color != tint {
            img.color = tint;
        }
        vis.set_if_neq(Visibility::Inherited);
    }
    for (count, mut t) in &mut counts {
        let n = menu.get(count.0).map_or(0, |&slot| queue.iter().filter(|p| slot.is(p.item)).count());
        let want = if n > 0 { format!("{n}") } else { String::new() };
        if t.0 != want {
            t.0 = want;
        }
    }
}

/// Each support-power tile: lit when ready, pressed while being aimed, dim while recharging
/// or unavailable; a dark sweep over the part of the charge still to come (clockwise from
/// the top, as a clock hand); and the time left, READY, or AIM.
#[allow(clippy::type_complexity)]
fn power_tiles(
    sim: Res<Sim>,
    art: Option<Res<art::HudArt>>,
    aim: Option<Res<PowerAim>>,
    mut faces: Query<(&PowerTile, &mut ImageNode)>,
    mut sweeps: Query<(&PowerSweep, &mut BackgroundGradient)>,
    mut timers: Query<(&PowerTimer, &mut Text, &mut TextColor)>,
) {
    let Some(art) = art else { return };
    let faction = sim.world.player(sim.local).faction;
    let (kit, trim) = (art.kit(faction), art::trim(faction));
    let aim = aim.and_then(|a| a.0);
    let powers = sim.powers();
    let wait = |slot: u8| sim.power_wait(slot);
    for (tile, mut img) in &mut faces {
        let face = match wait(tile.0) {
            _ if aim == Some(tile.0) => Face::Pressed,
            Some(0) => Face::Hover,
            Some(_) => Face::Idle,
            None => Face::Off,
        };
        let want = kit.face(face);
        if img.image != want {
            img.image = want;
        }
    }
    for (sweep, mut grad) in &mut sweeps {
        // A charge runs `first_ticks` at the start and `cooldown_ticks` after each use (the
        // same figure for every power so far); the longer one keeps the sweep from jumping.
        let total =
            powers.get(sweep.0 as usize).copied().flatten().map_or(1, |p| p.cooldown_ticks.max(p.first_ticks).max(1));
        let left = match wait(sweep.0) {
            Some(t) => (t as f32 / total as f32).min(1.0),
            None => 1.0,
        };
        let shade = Color::srgba(0.0, 0.0, 0.0, 0.68);
        let want = if left <= 0.0 {
            BackgroundGradient::default()
        } else {
            // The charged part is clear, with a glow in the trim colour fading in behind the
            // hand so the sweep reads on a dark face; the part still to charge is shaded.
            let done = (1.0 - left) * std::f32::consts::TAU;
            BackgroundGradient::from(ConicGradient::new(
                UiPosition::CENTER,
                vec![
                    AngularColorStop::new(Color::NONE, 0.0),
                    AngularColorStop::new(trim.with_alpha(0.0), (done - 0.9).max(0.0)),
                    AngularColorStop::new(trim.with_alpha(0.4), done),
                    AngularColorStop::new(shade, done),
                    AngularColorStop::new(shade, std::f32::consts::TAU),
                ],
            ))
        };
        if *grad != want {
            *grad = want;
        }
    }
    for (timer, mut t, mut color) in &mut timers {
        let (want, c) = match wait(timer.0) {
            _ if aim == Some(timer.0) => ("AIM".to_owned(), XP),
            Some(0) => ("READY".to_owned(), MONEY),
            Some(w) => {
                let s = w.div_ceil(TICKS_PER_SECOND);
                (format!("{}:{:02}", s / 60, s % 60), TEXT)
            }
            None => ("-".to_owned(), DIM),
        };
        if t.0 != want {
            t.0 = want;
        }
        color.set_if_neq(TextColor(c));
    }
}

/// The selection card's picture of the selected entity's model in its owner's colours
/// (`thumbs.rs`), filling the bezel; until that is taken, the silhouette of its class, tinted
/// toward the faction trim. Its health bar too; both hidden without a selection.
#[allow(clippy::type_complexity)]
fn portrait(
    sim: Res<Sim>,
    sel: Res<Selection>,
    art: Option<Res<art::HudArt>>,
    mut thumbs: Option<ResMut<crate::thumbs::Thumbs>>,
    mut icon: Query<(&mut ImageNode, &mut Node, &mut Visibility), With<Portrait>>,
    mut track: Query<&mut Visibility, (With<HealthTrack>, Without<Portrait>)>,
) {
    let Some(art) = art else { return };
    let world = &sim.world;
    let focus = sel.0.first().and_then(|&id| world.entity(id));
    let shown = if focus.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in &mut track {
        v.set_if_neq(shown);
    }
    let Ok((mut img, mut node, mut vis)) = icon.single_mut() else { return };
    vis.set_if_neq(shown);
    if let Some(e) = focus {
        let picture = thumbs.as_deref_mut().and_then(|t| t.get(crate::thumbs::Key::of(&sim, e.def, e.owner)));
        let (want, tint, side) = match picture {
            Some(p) => (p, Color::WHITE, percent(100)),
            None => (
                art.icon(Icon::of_def(world.rules.def(e.def))),
                Color::WHITE.mix(&art::trim(world.player(sim.local).faction), 0.3),
                percent(PORTRAIT_GLYPH),
            ),
        };
        if img.image != want {
            img.image = want;
        }
        if img.color != tint {
            img.color = tint;
        }
        if node.width != side {
            (node.width, node.height) = (side, side);
        }
    }
}

/// The build tray's picture side (layout pixels at 720p): as tall as a button face inside its
/// bevel.
const GLYPH: f32 = 42.0;
/// The portrait silhouette's share of the bezel's inside (56 of 68 px), as before pictures.
const PORTRAIT_GLYPH: f32 = 82.0;

/// A build-tray picture's tint by its button's face: as it is when idle, a little lifted under
/// the cursor, darker while held, and dim and faded when off, so the face's state still reads
/// over a colourful picture (the UI tint multiplies, so "off" can only dim, not desaturate).
/// The tint multiplies in linear light: sRGB 0.7 lets through under half of it.
fn picture_tint(face: Face) -> Color {
    match face {
        Face::Idle => Color::WHITE,
        Face::Hover => Color::srgb(1.12, 1.12, 1.12),
        Face::Pressed => Color::srgb(0.82, 0.82, 0.82),
        Face::Off => Color::srgba(0.7, 0.7, 0.7, 0.7),
    }
}

/// One entry of the build grid: a building to place, a unit to train, an upgrade to research.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Slot {
    Def(g2_sim::DefId),
    Upgrade(g2_sim::defs::UpgradeId),
}

impl Slot {
    pub(crate) fn name(self, rules: &g2_sim::Rules) -> &'static str {
        match self {
            Slot::Def(d) => rules.def(d).name,
            Slot::Upgrade(u) => rules.upgrade(u).name,
        }
    }

    /// The alpha's icon for the slot, under `ui/images/`.
    fn icon(self, rules: &g2_sim::Rules) -> Option<&'static str> {
        match self {
            Slot::Def(d) => crate::alpha_hud::icon_for(rules.def(d).name).map(|(icon, _)| icon),
            Slot::Upgrade(u) => crate::alpha_hud::upgrade_icon(rules.upgrade(u).name),
        }
    }

    /// Whether a production queue item is this slot's.
    fn is(self, item: Product) -> bool {
        match (self, item) {
            (Slot::Def(d), Product::Unit(u)) => d == u,
            (Slot::Upgrade(a), Product::Upgrade(b)) => a == b,
            _ => false,
        }
    }
}

/// What the selection can build, train or research right now, in grid order: a building's
/// units first (with the local player's general's), then its upgrades not yet researched or
/// queued anywhere.
pub(crate) fn build_menu(sim: &Sim, sel: &Selection) -> Vec<Slot> {
    let world = &sim.world;
    let focus = sel.0.first().and_then(|&id| world.entity(id));
    focus.map_or_else(Vec::new, |e| {
        let d = world.rules.def(e.def);
        let offers = world.offers_of(sim.local, e.def).into_iter().map(Slot::Def);
        if !e.is_complete() {
            Vec::new()
        } else if d.is_constructor() {
            offers.collect()
        } else {
            let research = d.researches.iter().filter(|&&u| !world.researched_or_queued(sim.local, u));
            offers.chain(research.map(|&u| Slot::Upgrade(u))).collect()
        }
    })
}

/// What a slot costs the local player.
fn slot_cost(sim: &Sim, slot: Slot) -> i64 {
    match slot {
        Slot::Def(d) => sim.world.cost_for(sim.local, d).credits,
        Slot::Upgrade(u) => sim.world.rules.upgrade(u).cost.credits,
    }
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
    mut fields: Query<(&Field, &mut Text, &mut TextColor)>,
    mut queue_row: Query<&mut Node, With<QueueRow>>,
) {
    let aim = aim.and_then(|a| a.0);
    let world = &sim.world;
    let rules = world.rules;
    let me = world.player(sim.local);
    let focus = sel.0.first().and_then(|&id| world.entity(id));
    if let Ok(mut row) = queue_row.single_mut() {
        let training = focus.is_some_and(|e| !e.queue.is_empty());
        row.display = if training { Display::Flex } else { Display::None };
    }
    let menu = build_menu(&sim, &sel);
    for (field, mut t, mut color) in &mut fields {
        match field {
            Field::Clock => t.0 = clock(world.tick()),
            Field::Tag(_) => {}
            Field::Power => {
                t.0 = if me.faction.uses_power() {
                    format!("PWR {}/{}", me.power_drain, me.power_supply)
                } else {
                    "PWR -".into()
                };
                color.set_if_neq(TextColor(if me.power_drain > me.power_supply { SHORT } else { XP }));
            }
            Field::PowerFigures => {
                t.0 =
                    if me.faction.uses_power() { format!("{}/{}", me.power_drain, me.power_supply) } else { "-".into() }
            }
            Field::Money => t.0 = format!("${}", me.credits),
            Field::GeneralPower => {
                // One line per power, its key first: ready, recharging, or being aimed.
                let lines: Vec<String> = sim
                    .powers()
                    .iter()
                    .enumerate()
                    .filter_map(|(slot, p)| {
                        let (p, key) = (p.as_ref()?, POWER_LABELS.get(slot)?);
                        let name = p.name.to_uppercase();
                        Some(match sim.power_wait(slot as u8) {
                            _ if aim == Some(slot as u8) => format!("{name}: click to aim"),
                            Some(0) => format!("{key}  {name}  READY"),
                            Some(t) => format!("{key}  {name}  {}", clock(t)),
                            None => return None,
                        })
                    })
                    .chain(
                        (aim == Some(crate::input::SPECIAL)).then(|| "PARTICLE ACCELERATOR: click an enemy".to_owned()),
                    )
                    .collect();
                t.0 = lines.join("\n");
            }
            Field::Aim => {
                let power = |slot: u8| sim.powers().get(slot as usize).copied().flatten();
                t.0 = match aim {
                    Some(crate::input::SPECIAL) => "PARTICLE ACCELERATOR: click an enemy".into(),
                    Some(slot) => {
                        power(slot).map_or(String::new(), |p| format!("{}: click to aim", p.name.to_uppercase()))
                    }
                    None => String::new(),
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
                    let mut s = format!("{}/{}", e.hp, world.max_hp(e));
                    if d.veterancy.is_some() {
                        s += &format!("  -  {}", rank_label(world.rank(e)));
                    }
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
                    if d.becomes.is_some() && e.owner == Some(sim.local) {
                        s += "  -  T: become a bunker";
                    }
                    if d.special.is_some() && e.owner == Some(sim.local) {
                        let left = e.special_at.saturating_sub(world.tick());
                        s += &if left == 0 {
                            "  -  R: particle accelerator READY".to_owned()
                        } else {
                            let secs = left.div_ceil(g2_sim::TICKS_PER_SECOND);
                            format!("  -  particle accelerator {}:{:02}", secs / 60, secs % 60)
                        };
                    }
                    if let Some(g) = d.garrison {
                        let n = world.occupants(e.id).len();
                        s += &format!("  -  garrison {n}/{}", g.capacity);
                        if n > 0 && e.owner == Some(sim.local) {
                            s += " (E: evacuate)";
                        }
                    }
                    if d.stealth {
                        s += if world.hidden(e) { "  -  stealthed" } else { "  -  REVEALED" };
                    } else if world.hidden(e) {
                        s += "  -  cloaked";
                    }
                    s
                })
            }
            Field::Queue => {
                t.0 = focus.and_then(|e| e.queue.first()).map_or(String::new(), |p| {
                    let n = focus.unwrap().queue.len();
                    match p.item {
                        Product::Unit(d) => format!("Training {} ({n})", rules.def(d).name),
                        Product::Upgrade(u) => format!("Researching {} ({n})", rules.upgrade(u).name),
                    }
                })
            }
            Field::GridCell(i) => {
                t.0 = menu.get(*i).map_or(String::new(), |&slot| {
                    let name = slot.name(rules);
                    match slot {
                        Slot::Def(_) => name.split_once(' ').map_or(name, |(_, rest)| rest),
                        Slot::Upgrade(_) => name,
                    }
                    .to_owned()
                })
            }
            Field::GridCost(i) => {
                let cost = menu.get(*i).map(|&slot| slot_cost(&sim, slot));
                t.0 = cost.map_or(String::new(), |c| format!("${c}"));
                color.set_if_neq(TextColor(if cost.is_some_and(|c| c > me.credits) { SHORT } else { MONEY }));
            }
            Field::GridTag(i) => {
                t.0 = menu.get(*i).map_or(String::new(), |&slot| format!("${}", slot_cost(&sim, slot)))
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
        // The label grows with the window as the HUD does, so it stays legible at 1080p.
        let k = (view.y / 720.0).clamp(1.0, 3.0);
        let Some(screen) = place(raw, 52.0 * k) else { continue };
        // The label sits 44 px above its anchor: it needs that much clear sky, or it goes
        // below the anchor instead of into the HUD strip along the top.
        let label_y = if screen.y >= 88.0 * k { screen.y - 44.0 * k } else { screen.y + 10.0 * k };
        if let Some((n, cap)) = crate::view::depot_label(&sim, e) {
            let color = if n >= cap { Color::srgb(0.95, 0.25, 0.2) } else { Color::srgb(0.3, 0.95, 0.3) };
            commands.spawn((
                abs(px(screen.x - 50.0 * k), auto(), px(label_y), auto()),
                text(&format!("COLLECTORS {n}/{cap}"), 12.0 * k, color),
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
        // Garrison pips over a building or vehicle with units inside: one per place, filled
        // in its holder's team colour.
        if let (Some(g), Some(owner)) = (def.garrison, e.owner) {
            let n = world.occupants(e.id).len();
            if n > 0 {
                let team = crate::models::TEAM_PALETTE[sim.teams[owner.0 as usize]];
                let Some(at) = place(screen, 30.0) else { continue };
                commands
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(at.x - 4.0 * g.capacity as f32),
                            top: px(at.y - 28.0),
                            column_gap: px(2),
                            padding: UiRect::all(px(1)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.75)),
                        Overlay,
                    ))
                    .with_children(|row| {
                        for i in 0..g.capacity as usize {
                            let c = if i < n { team } else { Color::srgb(0.18, 0.20, 0.20) };
                            row.spawn((Node { width: px(6), height: px(6), ..default() }, BackgroundColor(c)));
                        }
                    });
            }
        }
        let selected = sel.0.contains(&e.id);
        let max = world.max_hp(e);
        let damaged = e.hp < max;
        let rank = world.rank(e);
        if e.owner.is_some() && (selected || damaged || rank > 0) {
            let w = if def.is_building() { 46.0 } else { 22.0 };
            let frac = (e.hp as f32 / max.max(1) as f32).clamp(0.0, 1.0);
            let color = health_color(frac);
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
            // Rank chevrons at the bar's left end, one gold mark per rank.
            if rank > 0 {
                commands
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(screen.x - w / 2.0 - 4.0 * rank as f32 - 2.0),
                            top: px(screen.y - 10.0),
                            column_gap: px(1),
                            ..default()
                        },
                        Overlay,
                    ))
                    .with_children(|row| {
                        for _ in 0..rank {
                            row.spawn((Node { width: px(3), height: px(8), ..default() }, BackgroundColor(XP)));
                        }
                    });
            }
        }
    }
}

/// The selection panel's rank: "RECRUIT", or one chevron per rank.
fn rank_label(rank: u8) -> String {
    match rank {
        0 => "RECRUIT".into(),
        n => format!("RANK {}", "^".repeat(n as usize)),
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

    /// A window resize rebuilds the HUD in place: after the first build and after each
    /// resize there is exactly one HUD (the same root count), and frames without a resize
    /// leave it alone.
    #[test]
    fn a_resize_rebuilds_the_hud_once() {
        use bevy::prelude::*;
        use bevy::window::WindowResized;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .add_message::<WindowResized>()
            .init_asset::<Image>()
            .insert_resource(crate::sim::Sim::new(
                g2_rules::crater_ridge(),
                None,
                1,
                crate::sim::Sim::players([g2_sim::Faction::Eu, g2_sim::Faction::Gla], false),
            ))
            .init_resource::<crate::input::Selection>()
            .init_resource::<crate::input::Placement>()
            .init_resource::<crate::sim::Deliveries>()
            .add_plugins(super::HudPlugin);
        let roots = |app: &mut App| {
            app.world_mut().query_filtered::<Entity, With<super::HudRoot>>().iter(app.world()).collect::<Vec<_>>()
        };
        app.update();
        let first = roots(&mut app);
        assert!(!first.is_empty(), "the HUD spawns at the start");
        app.update();
        assert_eq!(roots(&mut app), first, "no resize: the same HUD");
        let window = app.world_mut().spawn_empty().id();
        app.world_mut().write_message(WindowResized { window, width: 1920.0, height: 1080.0 });
        app.update();
        let rebuilt = roots(&mut app);
        assert_eq!(rebuilt.len(), first.len(), "rebuilt, not stacked");
        assert!(rebuilt.iter().all(|e| !first.contains(e)), "every old root is gone");
    }
}
