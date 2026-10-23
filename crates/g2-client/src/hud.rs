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
use crate::input::{Placement, Selection};
use crate::sim::{LOCAL, Sim};

const PANEL: Color = Color::srgba(0.07, 0.08, 0.09, 0.92);
const PANEL_EDGE: Color = Color::srgb(0.24, 0.27, 0.29);
const SLOT: Color = Color::srgba(0.13, 0.15, 0.17, 0.95);
const TEAL: Color = Color::srgb(0.18, 0.78, 0.86);
const MONEY: Color = Color::srgb(0.35, 0.95, 0.35);
const XP: Color = Color::srgb(0.98, 0.86, 0.12);
const TEXT: Color = Color::srgb(0.88, 0.90, 0.90);
const DIM: Color = Color::srgb(0.55, 0.58, 0.60);
const MINIMAP: f32 = 196.0;

#[derive(Component)]
enum Field {
    Clock,
    Power,
    Money,
    Oil,
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

#[derive(Component)]
struct HealthBar;

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
        app.add_systems(Startup, spawn).add_systems(Update, (update_fields, minimap, overlays, popups));
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

fn text(s: &str, size: f32, color: Color) -> impl Bundle {
    (Text::new(s), TextFont { font_size: FontSize::Px(size), ..default() }, TextColor(color))
}

fn abs(left: Val, right: Val, top: Val, bottom: Val) -> Node {
    Node { position_type: PositionType::Absolute, left, right, top, bottom, ..default() }
}

fn panel_node(mut node: Node) -> impl Bundle {
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
    let me = world.player(LOCAL);
    let enemy = world.players().iter().enumerate().find(|(i, _)| *i != LOCAL.0 as usize).map(|(_, p)| p);

    // Top left: menu; top right: neutral project label (no EA trademarks in the UI).
    commands.spawn((abs(px(14), auto(), px(8), auto()), text("MENU", 14.0, DIM)));
    commands.spawn((abs(auto(), px(14), px(8), auto()), text("OPEN RTS - M1", 12.0, DIM)));

    // Top centre: one compact match bar: own tag, clock, enemy tag; a general shows after its faction.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(6),
                left: percent(50),
                margin: UiRect::left(px(-150)),
                width: px(300),
                padding: UiRect::axes(px(8), px(3)),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
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
            let team = |p: &g2_sim::Player| crate::models::color(Some(p.faction), crate::models::Slot::Team);
            t.spawn(text(&label(me), 12.0, team(me)));
            t.spawn((text("0:00:00", 15.0, TEXT), Field::Clock));
            if let Some(e) = enemy {
                t.spawn(text(&label(e), 12.0, team(e)));
            }
        });

    // Bottom left: power/money strip above the minimap.
    let mut corner = abs(px(0), auto(), auto(), px(0));
    corner.flex_direction = FlexDirection::Column;
    commands.spawn(panel_node(corner)).with_children(|p| {
        p.spawn(Node {
            width: px(MINIMAP),
            justify_content: JustifyContent::SpaceBetween,
            column_gap: px(6),
            ..default()
        })
        .with_children(|s| {
            s.spawn((text("PWR 0/0", 12.0, XP), Field::Power));
            s.spawn((text("OIL 0", 12.0, TEXT), Field::Oil));
            s.spawn((text("$0", 13.0, MONEY), Field::Money));
        });
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
        .spawn(panel_node(Node {
            position_type: PositionType::Absolute,
            right: px(0),
            bottom: px(0),
            width: px(360),
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            ..default()
        }))
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
        .spawn((Node {
            position_type: PositionType::Absolute,
            top: px(58),
            left: percent(50),
            margin: UiRect::left(px(-200)),
            width: px(400),
            justify_content: JustifyContent::Center,
            ..default()
        },))
        .with_children(|s| {
            s.spawn((text("", 14.0, XP), Field::Status));
        });
}

fn clock(ticks: u32) -> String {
    let s = ticks / TICKS_PER_SECOND;
    format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

fn update_fields(
    sim: Res<Sim>,
    sel: Res<Selection>,
    placing: Res<Placement>,
    mut fields: Query<(&Field, &mut Text, Option<&mut Node>), Without<QueueRow>>,
    mut queue_row: Query<&mut Node, With<QueueRow>>,
) {
    let world = &sim.world;
    let rules = world.rules;
    let me = world.player(LOCAL);
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
            Field::Power => {
                t.0 = if me.faction.uses_power() {
                    format!("PWR {}/{}", me.power_drain, me.power_supply)
                } else {
                    "PWR -".into()
                }
            }
            Field::Money => t.0 = format!("${}", me.credits),
            Field::Oil => t.0 = format!("OIL {}", me.oil),
            Field::Status => {
                t.0 = match (world.winner(), sim.speed, placing.0) {
                    (Some(w), _, _) if w == LOCAL => "VICTORY".into(),
                    (Some(_), _, _) => "DEFEAT".into(),
                    (_, _, Some(def)) => format!("Place {}  -  Esc cancels", rules.def(def).name),
                    (_, 0, _) => "PAUSED".into(),
                    (_, 1, _) => String::new(),
                    (_, s, _) => format!("×{s}"),
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
                        format!("{} {}  ${}", i + 1, short, world.cost_for(LOCAL, def).credits)
                    }
                    None => format!("{}", i + 1),
                }
            }
        }
    }
}

/// Minimap: one dot per entity, team-coloured; resources neutral; refreshed every frame.
fn minimap(
    mut commands: Commands,
    sim: Res<Sim>,
    view: Query<Entity, With<MinimapView>>,
    dots: Query<Entity, With<MinimapDot>>,
) {
    let Ok(map_node) = view.single() else { return };
    for d in &dots {
        commands.entity(d).despawn();
    }
    let world = &sim.world;
    let (w, h) = (world.map.width.to_f32(), world.map.height.to_f32());
    commands.entity(map_node).with_children(|m| {
        for e in world.entities() {
            let def = world.rules.def(e.def);
            let (color, size) = match (def.class, e.owner) {
                // Resources in neutral greys, so they never read as a faction.
                (Class::SupplyPile, _) => (Color::srgb(0.80, 0.80, 0.78), 4.0),
                (Class::OilWell, _) => (Color::srgb(0.35, 0.35, 0.35), 4.0),
                (_, Some(o)) => {
                    // Same team colours as the match bar and the models.
                    let f = world.player(o).faction;
                    let c = crate::models::color(Some(f), crate::models::Slot::Team);
                    (c, if def.is_building() { 6.0 } else { 3.0 })
                }
                _ => continue,
            };
            let (x, y) = (e.pos.x.to_f32() / w * MINIMAP, e.pos.y.to_f32() / h * MINIMAP);
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
    bars: Query<Entity, With<HealthBar>>,
) {
    for e in labels.iter().chain(bars.iter()) {
        commands.entity(e).despawn();
    }
    let Ok((camera, cam_tf)) = cam.single() else { return };
    let world = &sim.world;
    for e in world.entities() {
        let anchor = crate::view::overlay_anchor(&sim, e);
        let Ok(screen) = camera.world_to_viewport(cam_tf, anchor) else { continue };
        if let Some((n, cap)) = crate::view::depot_label(&sim, e) {
            let color = if n >= cap { Color::srgb(0.95, 0.25, 0.2) } else { Color::srgb(0.3, 0.95, 0.3) };
            commands.spawn((
                abs(px(screen.x - 50.0), auto(), px(screen.y - 44.0), auto()),
                text(&format!("COLLECTORS {n}/{cap}"), 12.0, color),
                // A tight shadow for contrast; the 4 px default reads as a doubled label at 12 px.
                TextShadow { offset: Vec2::splat(1.0), ..default() },
                WorldLabel,
            ));
        }
        let def = world.rules.def(e.def);
        // Cargo pips over own collectors: five segments, filled by load.
        if e.owner == Some(LOCAL)
            && let Some(spec) = def.collector()
        {
            let filled = cargo_pips(e.cargo, spec.capacity);
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
                    HealthBar,
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
                    HealthBar,
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
