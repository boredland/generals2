//! The build grid's tooltip: hovering a button shows what it makes in one of our panels above
//! the grid (both HUDs: the alpha's bottom bar has a tooltip proxy over its ability grid, but
//! its tooltip layouts draw abilities this game doesn't have). Name, price and build time
//! after the player's modifiers, armour and hit points, the weapon and what it is strong or
//! weak against (its per-armour bonus, as the archived unit cards state it), power, and why
//! the button would do nothing right now.

use bevy::prelude::*;
use g2_sim::{Armour, Class, Weapon};

use crate::hud::{Slot, TEXT};
use crate::input::Selection;
use crate::sim::Sim;

const WARN: Color = Color::srgb(1.0, 0.4, 0.3);

/// Headless only (`--hover-slot N`): show the tooltip of grid button N as if hovered.
#[derive(Resource)]
pub struct HoverSlot(pub usize);

/// One line of a tooltip, by how it is drawn.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Line {
    Title(String),
    Info(String),
    /// Why the button would do nothing now.
    Warn(String),
}

#[derive(Component)]
struct Tip;

pub struct TooltipPlugin;

impl Plugin for TooltipPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, show.after(crate::menu::MenuSet));
    }
}

fn armour_name(a: Armour) -> &'static str {
    match a {
        Armour::Infantry => "infantry",
        Armour::Light => "light",
        Armour::Heavy => "heavy",
        Armour::Structure => "structures",
    }
}

fn seconds(ticks: u32) -> String {
    let tenths = (ticks * 10).div_ceil(g2_sim::TICKS_PER_SECOND);
    if tenths.is_multiple_of(10) { format!("{} s", tenths / 10) } else { format!("{}.{} s", tenths / 10, tenths % 10) }
}

/// "Gun 40, range 35, every 1.5 s, ground only" and "Strong vs light (+30)".
fn weapon_lines(label: &str, w: Weapon) -> Vec<Line> {
    let reach = match w.targets {
        g2_sim::defs::Targets::Ground => ", ground only",
        g2_sim::defs::Targets::Air => ", air only",
        g2_sim::defs::Targets::Both => ", ground and air",
    };
    let shell = if w.shell.is_some() { " shells" } else { "" };
    let mut out = vec![Line::Info(format!(
        "{label}{shell} {}, range {}, every {}{reach}",
        w.damage,
        w.range.to_f32().round(),
        seconds(w.cooldown_ticks)
    ))];
    let by = |pick: fn(i32) -> bool| -> Vec<String> {
        [Armour::Infantry, Armour::Light, Armour::Heavy, Armour::Structure]
            .into_iter()
            .filter(|&a| pick(w.bonus[a as usize]))
            .map(|a| format!("{} ({:+})", armour_name(a), w.bonus[a as usize]))
            .collect()
    };
    let (strong, weak) = (by(|b| b > 0), by(|b| b < 0));
    if !strong.is_empty() {
        out.push(Line::Info(format!("Strong vs {}", strong.join(", "))));
    }
    if !weak.is_empty() {
        out.push(Line::Info(format!("Weak vs {}", weak.join(", "))));
    }
    out
}

/// What a unit's special shot, ability and parting blast do, a line each: "R: Nitro, speed
/// 150% for 5 s, every 30 s".
pub fn ability_lines(def: &g2_sim::Def) -> Vec<String> {
    use g2_sim::defs::{AbilityKind, StatusKind};
    let mut out = Vec::new();
    if let Some(s) = def.special {
        let what = match s.blast {
            Some(b) => format!("bursts for {} within {}", b.damage, b.radius.to_f32().round()),
            None => format!("{} damage", s.weapon.damage),
        };
        out.push(format!(
            "R: {}, {what} at range {}, every {}",
            s.name,
            s.weapon.range.to_f32().round(),
            seconds(s.cooldown_ticks)
        ));
    }
    if let Some(a) = def.ability {
        let what = match a.kind {
            AbilityKind::Boost(st) => {
                let kind = match st.kind {
                    StatusKind::Toughness => "damage taken",
                    StatusKind::Damage => "damage",
                    StatusKind::Speed => "speed",
                    StatusKind::Sight => "sight",
                    StatusKind::Stun => "stunned",
                };
                format!("{kind} {}% for {}, every {}", st.percent, seconds(st.ticks), seconds(a.cooldown_ticks))
            }
            AbilityKind::Detonate => "self-destruct".to_owned(),
        };
        out.push(format!("R: {}, {what}", a.name));
    }
    if let Some(b) = def.blast {
        let mut s = format!("Blows up for {} within {} when destroyed", b.damage, b.radius.to_f32().round());
        if b.cloud_ticks > 0 {
            s += &format!(", leaving {} a second for {}", b.cloud_damage, seconds(b.cloud_ticks));
        }
        out.push(s);
    }
    out
}

/// The tooltip of build-grid `slot` for the local player and the selection.
pub fn lines(sim: &Sim, sel: &Selection, slot: Slot) -> Vec<Line> {
    let world = &sim.world;
    let rules = world.rules;
    let me = sim.local;
    let mut out = vec![Line::Title(slot.name(rules).to_owned())];
    let (cost, ticks) = match slot {
        Slot::Def(d) => (world.cost_for(me, d).credits, world.build_ticks_for(me, d)),
        Slot::Upgrade(u) => (rules.upgrade(u).cost.credits, rules.upgrade(u).research_ticks.max(1)),
    };
    out.push(Line::Info(format!("${cost}  -  {}", seconds(ticks))));
    let focus = sel.0.first().and_then(|&id| world.entity(id));
    match slot {
        Slot::Def(d) => {
            let def = rules.def(d);
            let armour = match def.armour {
                Armour::Structure => "Structure".to_owned(),
                a => format!("{}{} armour", armour_name(a)[..1].to_uppercase(), &armour_name(a)[1..]),
            };
            // The def's hit points plus the health upgrades and perks the player already has,
            // as a fresh one would get (`World::max_hp_of`).
            let extra: i32 = world
                .effects(me, d)
                .map(|x| match x {
                    g2_sim::defs::Effect::Health { add } => add,
                    _ => 0,
                })
                .sum();
            out.push(Line::Info(format!("{armour}, {} hp", def.hp + extra)));
            if let Some(w) = def.weapon() {
                out.extend(weapon_lines(if def.is_building() { "Turret" } else { "Weapon" }, w));
            }
            if let Some(dep) = def.deploy {
                out.extend(weapon_lines("Deployed", dep.weapon));
            }
            out.extend(ability_lines(def).into_iter().map(Line::Info));
            if let Some(c) = def.collector() {
                out.push(Line::Info(format!("Carries ${} a load", c.capacity)));
            }
            if let Class::Building { power, .. } = def.class
                && world.player(me).faction.uses_power()
            {
                match power {
                    p if p > 0 => out.push(Line::Info(format!("Supplies {p} power"))),
                    p if p < 0 => out.push(Line::Info(format!("Uses {} power", -p))),
                    _ => {}
                }
            }
            if let (Some(_), Some(focus)) = (def.collector(), focus)
                && let Some(g2_sim::BuildingRole::SupplyCenter { collector_cap }) = rules.def(focus.def).role()
                && world.depot_load(focus.id) >= collector_cap
            {
                out.push(Line::Warn(format!("This supply center has its {collector_cap} collectors")));
            }
        }
        Slot::Upgrade(u) => {
            let mut names: Vec<&str> = rules.upgrade(u).effects.iter().map(|&(d, _)| rules.def(d).name).collect();
            names.dedup();
            out.push(Line::Info(format!("Improves {}", names.join(", "))));
        }
    }
    if world.player(me).credits < cost {
        out.push(Line::Warn(format!("Not enough money: ${} short", cost - world.player(me).credits)));
    }
    let builds = focus.is_some_and(|e| rules.def(e.def).is_constructor());
    if !builds && focus.is_some_and(|e| e.queue.len() >= g2_sim::world::MAX_QUEUE) {
        out.push(Line::Warn(format!("Queue full ({} items)", g2_sim::world::MAX_QUEUE)));
    }
    if !builds && focus.is_some_and(|e| !e.powered) && world.player(me).faction.uses_power() {
        out.push(Line::Warn("Unpowered: half speed".into()));
    }
    out
}

/// Rebuilds the tooltip every frame: over the hovered grid button (or `HoverSlot`), above the
/// HUD panel that holds the button (so it never covers the selection's name over the grid),
/// its right edge in line with the button's, kept on screen.
#[allow(clippy::too_many_arguments)]
fn show(
    mut commands: Commands,
    sim: Res<Sim>,
    sel: Option<Res<Selection>>,
    forced: Option<Res<HoverSlot>>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    headless: Option<Res<crate::hud::HeadlessScreen>>,
    ui: Option<Res<crate::menu::Ui>>,
    buttons: Query<(&crate::hud::GridButton, &ComputedNode, &bevy::ui::UiGlobalTransform, &InheritedVisibility)>,
    panels: crate::hud::Panels,
    old: Query<Entity, With<Tip>>,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    let Some(sel) = sel else { return };
    if ui.is_some_and(|u| u.paused || u.screen != crate::menu::Screen::Match) {
        return;
    }
    let screen = match (window.single(), &headless) {
        (Ok(w), _) => w.size(),
        (Err(_), Some(h)) => h.0,
        _ => return,
    };
    let cursor = window.single().ok().and_then(Window::cursor_position);
    let hovered = buttons.iter().find(|(b, n, tf, v)| {
        v.get()
            && match (&forced, cursor) {
                (Some(f), _) => b.0 == f.0,
                (None, Some(c)) => n.contains_point(**tf, c / n.inverse_scale_factor()),
                _ => false,
            }
    });
    let Some((button, node, tf, _)) = hovered else { return };
    let Some(&slot) = crate::hud::build_menu(&sim, &sel).get(button.0) else { return };
    // Boxes in logical pixels: the button's, and the top of the HUD panel under its top edge
    // (our bottom-right panel, or the alpha bar's art around its grid).
    let rect = |n: &ComputedNode, tf: &bevy::ui::UiGlobalTransform| {
        let s = n.inverse_scale_factor();
        (tf.translation * s - n.size() * s / 2.0, tf.translation * s + n.size() * s / 2.0)
    };
    let (lo, hi) = rect(node, tf);
    let probe = Vec2::new((lo.x + hi.x) / 2.0, lo.y + 1.0);
    let top = panels
        .iter()
        .filter(|(n, t, v)| v.get() && n.contains_point(**t, probe / n.inverse_scale_factor()))
        .map(|(n, t, _)| rect(n, t).0.y)
        .fold(lo.y, f32::min);
    let mut tip = crate::hud::abs(auto(), px((screen.x - hi.x).max(4.0)), auto(), px(screen.y - top + 6.0));
    tip.flex_direction = FlexDirection::Column;
    tip.row_gap = px(2);
    tip.min_width = px(300);
    tip.max_width = px(360);
    commands.spawn((crate::menu::MatchEntity, crate::hud::panel_node(tip), GlobalZIndex(20), Tip)).with_children(|p| {
        for line in lines(&sim, &sel, slot) {
            let (s, size, color) = match line {
                Line::Title(s) => (s.to_uppercase(), 14.0, crate::hud::TEAL),
                Line::Info(s) => (s, 12.0, TEXT),
                Line::Warn(s) => (s, 12.0, WARN),
            };
            p.spawn(crate::hud::text(&s, size, color));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The APA war factory's Warlord: the card's price, time, armour and gun (40, +30 vs.
    /// light, 1.5 s, range 35), what it is strong against, and the money it lacks.
    #[test]
    fn warlord_tooltip_states_its_card() {
        use g2_rules::ids::*;
        let mut players = Sim::players([g2_sim::Faction::Apa, g2_sim::Faction::Eu], false);
        players[0].starting_credits = 500;
        let sim = Sim::new(g2_rules::crater_ridge(), None, 1, players);
        let cc = sim.world.entities().find(|e| e.owner == Some(sim.local)).unwrap().id;
        let text: Vec<Line> = lines(&sim, &Selection(vec![cc]), Slot::Def(APA_WARLORD));
        let def = g2_rules::rules().def(APA_WARLORD);
        let want = [
            Line::Title("APA Warlord Tank".into()),
            Line::Info(format!("${}  -  {}", def.cost.credits, seconds(def.build_ticks))),
            Line::Info("Heavy armour, 610 hp".into()),
            Line::Info("Weapon 40, range 35, every 1.5 s, ground only".into()),
            Line::Info("Strong vs light (+30)".into()),
            Line::Warn(format!("Not enough money: ${} short", def.cost.credits - 500)),
        ];
        assert_eq!(text, want);
    }
}
