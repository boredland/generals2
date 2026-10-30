//! A mission's HUD: its objectives top left, under the menu key, each marked open, done or
//! failed, and the mission's messages under the match bar, the newest at the bottom, each
//! for `MESSAGE_SECS` of match time. Kept apart from `hud.rs`, which draws every match; this
//! only spawns in a mission. Both read the sim's `Run` every frame, so a replay shows the
//! same.

use bevy::prelude::*;
use g2_sim::TICKS_PER_SECOND;

use crate::hud::{TEXT, abs, text};
use crate::sim::Sim;

/// How long a message stays up, in seconds of match time: long enough to read two lines.
const MESSAGE_SECS: u32 = 8;
/// At most this many messages show at once; older ones make way.
const MESSAGES: usize = 3;

#[derive(Component)]
struct ObjectiveLine(usize);

#[derive(Component)]
struct MessageLines;

pub struct MissionHudPlugin;

impl Plugin for MissionHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, spawn.run_if(crate::menu::scene_start).after(crate::menu::MenuSet))
            .add_systems(Update, update.after(spawn));
    }
}

fn shadow() -> TextShadow {
    TextShadow { offset: Vec2::splat(1.0), ..default() }
}

fn spawn(mut commands: Commands, sim: Res<Sim>) {
    let Some(run) = sim.world.mission() else { return };
    let mut list = abs(px(14), auto(), px(34), auto());
    list.flex_direction = FlexDirection::Column;
    list.row_gap = px(2);
    list.max_width = px(360);
    list.padding = UiRect::all(px(6));
    commands.spawn((crate::menu::MatchEntity, list, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)))).with_children(
        |l| {
            l.spawn((text(&run.def.name.to_uppercase(), 13.0, crate::hud::TEAL), shadow()));
            for i in 0..run.def.objectives.len() {
                l.spawn((text("", 12.0, TEXT), shadow(), ObjectiveLine(i)));
            }
        },
    );
    let mut messages = abs(percent(50), auto(), px(84), auto());
    messages.margin = UiRect::left(px(-300));
    messages.width = px(600);
    messages.justify_content = JustifyContent::Center;
    commands.spawn((crate::menu::MatchEntity, messages)).with_child((
        text("", 15.0, Color::srgb(1.0, 0.86, 0.45)),
        TextLayout { justify: Justify::Center, ..default() },
        shadow(),
        MessageLines,
    ));
}

fn update(
    sim: Res<Sim>,
    mut lines: Query<(&ObjectiveLine, &mut Text, &mut TextColor), Without<MessageLines>>,
    mut messages: Query<&mut Text, With<MessageLines>>,
) {
    let Some(run) = sim.world.mission() else { return };
    for (line, mut t, mut colour) in &mut lines {
        let (Some(o), Some(&s)) = (run.def.objectives.get(line.0), run.objectives.get(line.0)) else { continue };
        // ASCII marks: the HUD's monospace face has no box or check glyphs.
        let (_, ink) = crate::menu::objective_mark(s);
        let mark = match s {
            g2_sim::mission::Status::Active => "[ ]",
            g2_sim::mission::Status::Done => "[x]",
            g2_sim::mission::Status::Failed => "[!]",
        };
        let wanted = format!("{mark} {}", o.text);
        if t.0 != wanted {
            t.0 = wanted;
        }
        colour.set_if_neq(TextColor(if s == g2_sim::mission::Status::Active { TEXT } else { ink }));
    }
    let now = sim.world.tick();
    let recent: Vec<&str> =
        run.said.iter().filter(|&&(t, _)| now < t + MESSAGE_SECS * TICKS_PER_SECOND).map(|&(_, s)| s).collect();
    let shown = recent[recent.len().saturating_sub(MESSAGES)..].join("\n");
    for mut t in &mut messages {
        if t.0 != shown {
            t.0.clone_from(&shown);
        }
    }
}
