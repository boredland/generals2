//! Text HUD: resources, power, clock and speed on top; selection details and hotkeys at the
//! bottom; a banner when the match ends.

use bevy::prelude::*;
use g2_sim::{BuildingRole, Class, Order, TICKS_PER_SECOND};

use crate::input::{Placement, Selection};
use crate::sim::{LOCAL, Sim};

#[derive(Component)]
struct TopBar;

#[derive(Component)]
struct BottomPanel;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn).add_systems(Update, update);
    }
}

fn panel(top: bool) -> (Node, BackgroundColor) {
    (
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: if top { px(0) } else { auto() },
            bottom: if top { auto() } else { px(0) },
            padding: UiRect::axes(px(12), px(6)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
    )
}

fn spawn(mut commands: Commands) {
    let font = TextFont { font_size: FontSize::Px(16.0), ..default() };
    commands.spawn(panel(true)).with_child((Text::new(""), font.clone(), TopBar));
    commands.spawn(panel(false)).with_child((Text::new(""), font, BottomPanel));
}

fn clock(ticks: u32) -> String {
    let s = ticks / TICKS_PER_SECOND;
    format!("{:02}:{:02}", s / 60, s % 60)
}

fn update(
    sim: Res<Sim>,
    sel: Res<Selection>,
    placing: Res<Placement>,
    mut top: Query<&mut Text, (With<TopBar>, Without<BottomPanel>)>,
    mut bottom: Query<&mut Text, (With<BottomPanel>, Without<TopBar>)>,
) {
    let world = &sim.world;
    let rules = world.rules;
    let me = world.player(LOCAL);
    let speed = match sim.speed {
        0 => "PAUSED".to_owned(),
        s => format!("x{s}"),
    };
    let power = if me.faction.uses_power() {
        format!("Power {}/{}", me.power_drain, me.power_supply)
    } else {
        "No power grid".to_owned()
    };
    if let Ok(mut t) = top.single_mut() {
        t.0 = format!(
            "{}   Credits {}   Oil {}   {}   {}  {}{}",
            me.faction.name(),
            me.credits,
            me.oil,
            power,
            clock(world.tick()),
            speed,
            if world.is_ai(LOCAL) { "   (spectating)" } else { "" },
        );
    }

    let mut lines: Vec<String> = Vec::new();
    if let Some(winner) = world.winner() {
        lines.push(if winner == LOCAL { "VICTORY".into() } else { "DEFEAT".into() });
    }
    if let Some(def) = placing.0 {
        lines.push(format!("Placing {}: left click to place, Shift to repeat, Esc cancels", rules.def(def).name));
    }
    match sel.0.as_slice() {
        [] => lines.push("Select units: left click/drag. Space pause, +/- speed, WASD/QE camera.".into()),
        [id] => {
            if let Some(e) = world.entity(*id) {
                let d = rules.def(e.def);
                let mut line = format!("{}  HP {}/{}", d.name, e.hp, d.hp);
                if let Some(done) = e.construction {
                    line += &format!("  under construction {}%", done * 100 / d.build_ticks.max(1));
                }
                if let Class::Building { power, .. } = d.class
                    && power < 0
                {
                    line += if e.powered { "  powered" } else { "  UNPOWERED (half speed)" };
                }
                if let Some(BuildingRole::SupplyCenter { collector_cap }) = d.role() {
                    line += &format!("  collectors {}/{}", world.depot_load(e.id), collector_cap);
                }
                if let Order::Gather { .. } = e.order {
                    line += &format!("  gathering (carrying {})", e.cargo);
                }
                lines.push(line);
                if !e.queue.is_empty() {
                    let q: Vec<String> = e
                        .queue
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let total = rules.def(p.def).build_ticks * 2;
                            if i == 0 {
                                format!("{} {}%", rules.def(p.def).name, p.progress_halves * 100 / total.max(1))
                            } else {
                                rules.def(p.def).name.to_owned()
                            }
                        })
                        .collect();
                    lines.push(format!("Queue: {}  (Backspace cancels last)", q.join(", ")));
                }
                let menu = if d.is_constructor() { d.builds } else { d.produces };
                if !menu.is_empty() && e.is_complete() {
                    let items: Vec<String> = menu
                        .iter()
                        .enumerate()
                        .map(|(i, &u)| {
                            let c = rules.def(u).cost;
                            format!("[{}] {} ${}", i + 1, rules.def(u).name, c.credits)
                        })
                        .collect();
                    lines.push(items.join("   "));
                }
            }
        }
        many => lines.push(format!("{} units selected. Right click: move / gather / help build", many.len())),
    }
    if let Ok(mut t) = bottom.single_mut() {
        t.0 = lines.join("\n");
    }
}
