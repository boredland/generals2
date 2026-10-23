//! Mouse and keyboard → sim commands.
//!
//! Left click/drag: select own units (click on a building selects it).
//! Right click: context order (supply pile → gather, own foundation → construct, ground → move).
//! With a constructor selected: B opens build mode, 1-9 pick a building, left click places, Esc cancels.
//! With a building selected: 1-9 queue production, Backspace cancels the last item.
//! Space pauses, +/- change speed (×1, ×2, ×4, ×8). F10 surrenders.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use g2_sim::{Class, CommandKind, DefId, EntityId};

use crate::camera::RtsCamera;
use crate::sim::{LOCAL, Sim, to_sim, to_world};

#[derive(Resource, Default)]
pub struct Selection(pub Vec<EntityId>);

#[derive(Resource, Default)]
pub struct Placement(pub Option<DefId>);

#[derive(Resource, Default)]
struct Drag(Option<Vec2>);

const DIGITS: [KeyCode; 9] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Selection>()
            .init_resource::<Placement>()
            .init_resource::<Drag>()
            .add_systems(Update, (prune_selection, keyboard, mouse, draw_overlays).chain());
    }
}

/// Cursor ray → ground plane (Y = 0).
fn cursor_ground(window: &Window, cam: (&Camera, &GlobalTransform)) -> Option<Vec3> {
    let ray = cam.0.viewport_to_world(cam.1, window.cursor_position()?).ok()?;
    let denom = ray.direction.y;
    if denom.abs() < 1e-6 {
        return None;
    }
    let t = -ray.origin.y / denom;
    (t > 0.0).then(|| ray.origin + *ray.direction * t)
}

fn prune_selection(sim: Res<Sim>, mut sel: ResMut<Selection>) {
    sel.0.retain(|&id| sim.world.entity(id).is_some_and(|e| e.owner == Some(LOCAL)));
}

/// The first selected entity whose def is a constructor, if any.
fn selected_constructor(sim: &Sim, sel: &Selection) -> Option<EntityId> {
    sel.0.iter().copied().find(|&id| sim.world.entity(id).is_some_and(|e| sim.world.rules.def(e.def).is_constructor()))
}

fn selected_building(sim: &Sim, sel: &Selection) -> Option<EntityId> {
    match sel.0.as_slice() {
        [id] => sim.world.entity(*id).filter(|e| sim.world.rules.def(e.def).is_building()).map(|e| e.id),
        _ => None,
    }
}

fn keyboard(keys: Res<ButtonInput<KeyCode>>, mut sim: ResMut<Sim>, sel: Res<Selection>, mut placing: ResMut<Placement>) {
    if keys.just_pressed(KeyCode::Space) {
        sim.speed = if sim.speed == 0 { 1 } else { 0 };
    }
    if keys.any_just_pressed([KeyCode::Equal, KeyCode::NumpadAdd]) {
        sim.speed = (sim.speed * 2).clamp(1, 8);
    }
    if keys.any_just_pressed([KeyCode::Minus, KeyCode::NumpadSubtract]) {
        sim.speed = (sim.speed / 2).max(1);
    }
    if keys.just_pressed(KeyCode::F10) {
        sim.order(CommandKind::Surrender);
    }
    if keys.just_pressed(KeyCode::Escape) {
        placing.0 = None;
    }
    let rules = sim.world.rules;
    if let Some(builder) = selected_constructor(&sim, &sel) {
        let builds = rules.def(sim.world.entity(builder).unwrap().def).builds;
        for (i, key) in DIGITS.iter().enumerate() {
            if keys.just_pressed(*key)
                && let Some(&def) = builds.get(i)
            {
                placing.0 = Some(def);
            }
        }
        return;
    }
    if let Some(producer) = selected_building(&sim, &sel) {
        let produces = rules.def(sim.world.entity(producer).unwrap().def).produces;
        for (i, key) in DIGITS.iter().enumerate() {
            if keys.just_pressed(*key)
                && let Some(&def) = produces.get(i)
            {
                sim.order(CommandKind::Train { producer, def });
            }
        }
        if keys.just_pressed(KeyCode::Backspace) {
            sim.order(CommandKind::CancelTrain { producer });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn mouse(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Query<&Window, With<PrimaryWindow>>,
    cam: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
    mut sim: ResMut<Sim>,
    mut sel: ResMut<Selection>,
    mut placing: ResMut<Placement>,
    mut drag: ResMut<Drag>,
) {
    let (Ok(window), Ok(cam)) = (window.single(), cam.single()) else { return };
    let Some(ground) = cursor_ground(window, cam) else { return };
    let here = to_sim(ground);

    if let Some(def) = placing.0 {
        if buttons.just_pressed(MouseButton::Left)
            && let Some(builder) = selected_constructor(&sim, &sel)
        {
            sim.order(CommandKind::Build { builder, def, pos: here });
            if !keys.pressed(KeyCode::ShiftLeft) {
                placing.0 = None;
            }
        }
        if buttons.just_pressed(MouseButton::Right) {
            placing.0 = None;
        }
        return;
    }

    if buttons.just_pressed(MouseButton::Left) {
        drag.0 = window.cursor_position();
    }
    if buttons.just_released(MouseButton::Left)
        && let (Some(start), Some(end)) = (drag.0.take(), window.cursor_position())
    {
        let additive = keys.pressed(KeyCode::ShiftLeft);
        if !additive {
            sel.0.clear();
        }
        if start.distance(end) < 6.0 {
            if let Some(id) = pick(&sim, here, |e| e.owner == Some(LOCAL)) {
                sel.0.push(id);
            }
        } else {
            let (lo, hi) = (start.min(end), start.max(end));
            for e in sim.world.entities().filter(|e| e.owner == Some(LOCAL) && sim.world.rules.def(e.def).is_unit()) {
                if let Ok(p) = cam.0.world_to_viewport(cam.1, to_world(e.pos))
                    && p.cmpge(lo).all()
                    && p.cmple(hi).all()
                {
                    sel.0.push(e.id);
                }
            }
        }
        sel.0.dedup();
    }

    if buttons.just_pressed(MouseButton::Right) && !sel.0.is_empty() {
        let units: Vec<EntityId> =
            sel.0.iter().copied().filter(|&id| sim.world.entity(id).is_some_and(|e| sim.world.rules.def(e.def).is_unit())).collect();
        if units.is_empty() {
            return;
        }
        let rules = sim.world.rules;
        let target = pick(&sim, here, |e| e.owner.is_none() || e.owner == Some(LOCAL));
        let kind = match target.and_then(|t| sim.world.entity(t)) {
            Some(t) if matches!(rules.def(t.def).class, Class::SupplyPile) => CommandKind::Gather { units, pile: t.id },
            Some(t) if t.owner == Some(LOCAL) && t.construction.is_some() => CommandKind::Construct { units, site: t.id },
            _ => CommandKind::Move { units, target: here },
        };
        sim.order(kind);
    }
}

/// Topmost entity under the cursor: the one whose footprint contains `at`, smallest first, so a
/// unit standing next to a building is still clickable.
fn pick(sim: &Sim, at: g2_sim::Vec2, filter: impl Fn(&g2_sim::Entity) -> bool) -> Option<EntityId> {
    sim.world
        .entities()
        .filter(|e| filter(e))
        .filter(|e| e.pos.distance(at) <= sim.world.rules.def(e.def).radius + g2_sim::Fx::ONE)
        .min_by_key(|e| sim.world.rules.def(e.def).radius)
        .map(|e| e.id)
}

fn draw_overlays(
    mut gizmos: Gizmos,
    sim: Res<Sim>,
    sel: Res<Selection>,
    placing: Res<Placement>,
    window: Query<&Window, With<PrimaryWindow>>,
    cam: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
) {
    let flat = |p: Vec3| Isometry3d::new(p + Vec3::Y * 0.2, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2));
    for &id in &sel.0 {
        let Some(e) = sim.world.entity(id) else { continue };
        let def = sim.world.rules.def(e.def);
        gizmos.circle(flat(to_world(e.pos)), def.radius.to_f32() + 1.0, Color::srgb(0.3, 1.0, 0.4));
        if let Class::Building { power, power_radius, .. } = def.class
            && power > 0
        {
            gizmos.circle(flat(to_world(e.pos)), power_radius.to_f32(), Color::srgba(1.0, 0.9, 0.2, 0.5));
        }
    }
    if let (Some(def), Ok(window), Ok(cam)) = (placing.0, window.single(), cam.single())
        && let Some(ground) = cursor_ground(window, cam)
    {
        let ok = sim.world.check_placement(def, to_sim(ground));
        let (pos, color) = match ok {
            Ok((snapped, _)) => (to_world(snapped), Color::srgb(0.3, 1.0, 0.4)),
            Err(_) => (ground, Color::srgb(1.0, 0.25, 0.2)),
        };
        gizmos.circle(flat(pos), sim.world.rules.def(def).radius.to_f32(), color);
    }
}
