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
enum Field {
    Clock,
    /// A player's tag in the match bar, dimmed once that player is defeated.
    Tag(g2_sim::PlayerId),
    Power,
    Money,
    /// The general's power: "G <name>" when ready, else its countdown; hidden without one.
    GeneralPower,
    Status,
    SelName,
    SelInfo,
    Queue,
    QueueBar,
    GridCell(usize),
}

#[derive(Component)]
struct MinimapDot;

#[derive(Component)]
struct MinimapView;

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
        app.add_systems(Update, spawn.run_if(crate::menu::scene_start).after(crate::menu::MenuSet))
            .add_systems(Update, (update_fields, tags, minimap, overlays, popups).after(spawn));
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

fn spawn(mut commands: Commands, sim: Res<Sim>) {
    let world = &sim.world;

    // Top left: the in-match menu key; top right: neutral project label (no EA trademarks).
    commands.spawn((crate::menu::MatchEntity, abs(px(14), auto(), px(8), auto()), text("ESC  MENU", 14.0, DIM)));
    commands.spawn((crate::menu::MatchEntity, abs(auto(), px(14), px(8), auto()), text("OPEN RTS", 12.0, DIM)));

    // Top centre: one compact match bar: own tag, clock, then every opponent's tag in its
    // team colour (struck out once defeated); a general shows after its faction.
    commands
        .spawn((
            crate::menu::MatchEntity,
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
    commands.spawn((crate::menu::MatchEntity, panel_node(corner))).with_children(|p| {
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
            crate::menu::MatchEntity,
            panel_node(Node {
                position_type: PositionType::Absolute,
                right: px(0),
                bottom: px(0),
                width: px(360),
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            }),
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
            crate::menu::MatchEntity,
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
            Field::Money => t.0 = format!("${}", me.credits),
            Field::GeneralPower => {
                t.0 = match (sim.power(), sim.power_ready_in()) {
                    (Some(p), _) if aim => format!("{}: click to aim", p.name.to_uppercase()),
                    (Some(p), Some(0)) => format!("G  {}  READY", p.name.to_uppercase()),
                    (Some(p), Some(t)) => format!("{}  {}", p.name.to_uppercase(), clock(t)),
                    _ => String::new(),
                }
            }
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
                    (_, _, s, _) => {
                        let speed = if s > 1 { format!("x{s}") } else { String::new() };
                        match onslaught_status(world) {
                            Some(waves) if speed.is_empty() => waves,
                            Some(waves) => format!("{waves}   {speed}"),
                            None => speed,
                        }
                    }
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
        }
    }
}

/// Minimap: a fog layer (the local player's cells: black unexplored, dimmed out of sight),
/// then one dot per shown entity, team-coloured; resources neutral; refreshed every frame.
#[allow(clippy::too_many_arguments)]
fn minimap(
    mut commands: Commands,
    sim: Res<Sim>,
    view: Query<Entity, With<MinimapView>>,
    dots: Query<Entity, With<MinimapDot>>,
    mut fog_image: Local<Option<Handle<Image>>>,
    mut states: Local<Vec<u8>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Ok(map_node) = view.single() else { return };
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
    let (fw, fh) = (v.w as f32 * cell / w * MINIMAP, v.h as f32 * cell / h * MINIMAP);
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
            let (x, y) = (pos.x.to_f32() / w * MINIMAP, pos.y.to_f32() / h * MINIMAP);
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
        // The HUD owns the corners: the top strip (menu, clock, project label), the minimap
        // bottom-left and the selection panel bottom-right. Nothing anchored there is drawn.
        let under_hud = raw.y < 40.0
            || (raw.x < 225.0 && raw.y > view.y - 235.0)
            || (raw.x > view.x - 375.0 && raw.y > view.y - 175.0);
        if under_hud {
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
