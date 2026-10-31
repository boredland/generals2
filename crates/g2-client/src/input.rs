//! Mouse and keyboard → sim commands.
//!
//! Left click/drag: select own units (click on a building selects it); a double click on an
//! own unit or building adds every one of its kind on screen (OpenRA's `SelectActorsOnScreen`).
//! Right click: context order (enemy → attack, supply pile → gather, own foundation →
//! construct, ground → move), on release, so a right-button drag that panned the camera
//! orders nothing. Ctrl + click on the ground attack-moves. With only own production buildings
//! selected, a click on the ground sets their rally point (OpenRA's `RallyPoint`). Under the
//! Classic control scheme (`settings.rs`, as OpenRA's) a left click orders the selected units
//! instead, unless it is on one of our own units or buildings, which it selects; a right click
//! deselects. Each order shows where it went (`feedback.rs`).
//! Building and training are by clicking the build grid, as in OpenRA: a constructor's cell
//! starts placing that building (left click places, Esc cancels), a building's cell queues
//! that unit or research; Backspace cancels the selected building's last item. The number
//! keys are control groups, and Z / X select many at once (`select.rs`). Clicking the minimap
//! moves the camera there. A click on any HUD panel stays with the HUD: it never selects or
//! orders in the world.
//! Space pauses, +/- change speed (×1, ×2, ×4, ×8). F10 surrenders. Tab selects the next own
//! collector and centres the camera on it. G arms the general's power, F1-F6 the faction's
//! support powers and then those the slotted perks add: the next left click aims it (right
//! click or Esc disarms). Esc is handled by `menu` alone, so it cancels a placement or an
//! armed power before it opens the pause menu.
//! M (mute) is handled by `audio`, V (to the last alert) by `alerts`.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use g2_sim::{Class, CommandKind, DefId, EntityId};

use crate::camera::RtsCamera;
use crate::sim::{Sim, to_sim};

#[derive(Resource, Default)]
pub struct Selection(pub Vec<EntityId>);

#[derive(Resource, Default)]
pub struct Placement(pub Option<DefId>);

/// A power is armed, by slot (0 the general's, then support and perk powers), or the selected
/// units' special shot (`SPECIAL`): the next left click on the ground uses the power there, on
/// an enemy fires the special at it.
#[derive(Resource, Default)]
pub struct PowerAim(pub Option<u8>);

/// `PowerAim` for the selection's special shot (R) rather than a power slot.
pub const SPECIAL: u8 = u8::MAX;

/// The key that arms each power slot: room for a general's power, a faction's three support
/// powers and the perk powers after them.
pub const POWER_KEYS: [KeyCode; 7] =
    [KeyCode::KeyG, KeyCode::F1, KeyCode::F2, KeyCode::F3, KeyCode::F4, KeyCode::F5, KeyCode::F6];

/// The left button's drag start, whether the right button went down over the world, and the
/// last left click (where and when), for telling a double click.
#[derive(Resource, Default)]
struct Drag {
    left: Option<Vec2>,
    right: bool,
    last_click: Option<(Vec2, f64)>,
}

/// OpenRA's `MultiTapDetection` for the mouse: a second click within 250 ms and 4 pixels of
/// the first is a double click.
const DOUBLE_CLICK: (f64, f32) = (0.25, 4.0);

pub struct InputPlugin;

/// The gameplay input systems, as one unit for ordering against other plugins.
#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub struct InputSet;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Selection>()
            .init_resource::<Placement>()
            .init_resource::<PowerAim>()
            .init_resource::<Drag>()
            .add_plugins((crate::select::SelectPlugin, crate::feedback::FeedbackPlugin))
            .add_systems(
                Update,
                (prune_selection, keyboard, cycle_collectors, mouse, draw_overlays)
                    .chain()
                    .in_set(InputSet)
                    .run_if(crate::menu::playing),
            );
    }
}

/// Tab: select the next own collector (in entity order, wrapping) and centre the camera on it.
fn cycle_collectors(
    keys: Res<ButtonInput<KeyCode>>,
    sim: Res<Sim>,
    mut sel: ResMut<Selection>,
    mut cam: Query<&mut RtsCamera>,
) {
    if !keys.just_pressed(KeyCode::Tab) {
        return;
    }
    let world = &sim.world;
    let collectors: Vec<EntityId> = world
        .entities()
        .filter(|e| e.owner == Some(sim.local) && world.rules.def(e.def).collector().is_some())
        .map(|e| e.id)
        .collect();
    let Some(next) = next_after(&collectors, sel.0.first().copied()) else { return };
    sel.0 = vec![next];
    if let (Ok(mut cam), Some(e)) = (cam.single_mut(), world.entity(next)) {
        let at = sim.to_world(e.pos);
        cam.pivot = Vec3::new(at.x, cam.pivot.y, at.z);
    }
}

/// The entry after `current` in `list`, wrapping; the first entry if `current` isn't in it.
fn next_after(list: &[EntityId], current: Option<EntityId>) -> Option<EntityId> {
    let i = current.and_then(|c| list.iter().position(|&x| x == c)).map_or(0, |i| (i + 1) % list.len());
    list.get(i).copied()
}

/// Cursor ray → rendered ground.
fn cursor_ground(sim: &Sim, window: &Window, cam: (&Camera, &GlobalTransform)) -> Option<Vec3> {
    let ray = cam.0.viewport_to_world(cam.1, window.cursor_position()?).ok()?;
    ray_ground(sim, ray.origin, *ray.direction)
}

/// First crossing of a ray with the rendered ground (flat at Y = 0 on the built-in map):
/// marched in 2-unit steps, then refined by bisection.
fn ray_ground(sim: &Sim, origin: Vec3, dir: Vec3) -> Option<Vec3> {
    let above = |t: f32| {
        let p = origin + dir * t;
        p.y - sim.ground_y(p.x, p.z)
    };
    let step = 2.0;
    let (mut lo, mut t) = (0.0, step);
    while t < 4000.0 {
        if above(t) <= 0.0 {
            for _ in 0..12 {
                let mid = (lo + t) / 2.0;
                if above(mid) > 0.0 { lo = mid } else { t = mid }
            }
            let p = origin + dir * t;
            return Some(Vec3::new(p.x, sim.ground_y(p.x, p.z), p.z));
        }
        lo = t;
        t += step;
    }
    None
}

fn prune_selection(sim: Res<Sim>, mut sel: ResMut<Selection>) {
    sel.0.retain(|&id| sim.world.entity(id).is_some_and(|e| e.owner == Some(sim.local)));
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

pub(crate) fn keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    mut sim: ResMut<Sim>,
    sel: Res<Selection>,
    mut placing: ResMut<Placement>,
    mut aim: ResMut<PowerAim>,
) {
    // Online the clock is shared: pausing or speeding up one machine would only stall the
    // other, so these keys do nothing there.
    let local_clock = sim.online.is_none();
    if local_clock && keys.just_pressed(KeyCode::Space) {
        sim.speed = if sim.speed == 0 { 1 } else { 0 };
    }
    if local_clock && keys.any_just_pressed([KeyCode::Equal, KeyCode::NumpadAdd]) {
        sim.speed = (sim.speed * 2).clamp(1, 8);
    }
    if local_clock && keys.any_just_pressed([KeyCode::Minus, KeyCode::NumpadSubtract]) {
        sim.speed = (sim.speed / 2).max(1);
    }
    if keys.just_pressed(KeyCode::F10) {
        sim.order(CommandKind::Surrender);
    }
    for (slot, key) in POWER_KEYS.into_iter().enumerate() {
        if keys.just_pressed(key) && sim.power_wait(slot as u8) == Some(0) {
            aim.0 = Some(slot as u8);
            placing.0 = None;
        }
    }
    let rules = sim.world.rules;
    // T: deployable units in the selection set up, or pack up if every one is already set up.
    if keys.just_pressed(KeyCode::KeyT) {
        let deployable: Vec<EntityId> = sel
            .0
            .iter()
            .copied()
            .filter(|&id| sim.world.entity(id).is_some_and(|e| rules.def(e.def).deploy.is_some()))
            .collect();
        if !deployable.is_empty() {
            let all_set_up = deployable
                .iter()
                .all(|&id| sim.world.entity(id).unwrap().deploy == g2_sim::entity::DeployState::Deployed);
            sim.order(CommandKind::Deploy { units: deployable, deployed: !all_set_up });
        }
        // T also turns the selected Battle Buses into bunkers, for good.
        let becoming: Vec<EntityId> = sel
            .0
            .iter()
            .copied()
            .filter(|&id| sim.world.entity(id).is_some_and(|e| rules.def(e.def).becomes.is_some()))
            .collect();
        if !becoming.is_empty() {
            sim.order(CommandKind::Transform { units: becoming });
        }
    }
    // R: the selected units with a ready ability (Nitro, Radar Pulse, a self-destruct) use it at
    // once; the special shot of those that have one is armed, and the next left click on an
    // enemy fires it.
    if keys.just_pressed(KeyCode::KeyR) {
        let tick = sim.world.tick();
        let mine = |e: &&g2_sim::Entity| e.owner == Some(sim.local);
        let using: Vec<_> = sel
            .0
            .iter()
            .copied()
            .filter(|&id| {
                sim.world
                    .entity(id)
                    .filter(mine)
                    .is_some_and(|e| rules.def(e.def).ability.is_some() && e.ability_at <= tick)
            })
            .collect();
        let ready = sel.0.iter().any(|&id| {
            sim.world
                .entity(id)
                .filter(mine)
                .is_some_and(|e| rules.def(e.def).special.is_some() && e.special_at <= tick)
        });
        if !using.is_empty() {
            sim.order(CommandKind::Ability { units: using });
        }
        if ready {
            aim.0 = Some(SPECIAL);
            placing.0 = None;
        }
    }
    // E: everyone gets out of the selected buildings and vehicles that hold units.
    if keys.just_pressed(KeyCode::KeyE) {
        for &id in &sel.0 {
            if sim.world.entity(id).is_some_and(|e| e.owner == Some(sim.local)) && !sim.world.occupants(id).is_empty() {
                sim.order(CommandKind::Evacuate { container: id });
            }
        }
    }
    if selected_constructor(&sim, &sel).is_none()
        && let Some(producer) = selected_building(&sim, &sel)
        && keys.just_pressed(KeyCode::Backspace)
    {
        sim.order(CommandKind::CancelTrain { producer });
    }
}

/// A left click on the HUD: a build-grid cell builds, trains or researches its slot, the minimap moves the
/// camera there. Returns whether the HUD took the click.
fn hud_click(
    point: Vec2,
    sim: &mut Sim,
    sel: &Selection,
    placing: &mut Placement,
    hud: &HudQueries,
    cam: &mut RtsCamera,
) -> bool {
    let inside = |node: &ComputedNode, tf: &bevy::ui::UiGlobalTransform, vis: &InheritedVisibility| {
        vis.get() && node.contains_point(*tf, point / node.inverse_scale_factor())
    };
    if let Some(cell) = hud.buttons.iter().find(|(_, n, tf, v)| inside(n, tf, v)).map(|(b, ..)| b.0) {
        press_slot(sim, sel, placing, cell);
        return true;
    }
    if let Some((n, tf, v, node)) = hud.minimap.iter().next()
        && inside(n, tf, v)
        && let Some(local) = n.normalize_point(*tf, point / n.inverse_scale_factor())
    {
        // `local` runs -0.5..0.5 over the node; the map fills a `side` square from its corner.
        let size = n.size() * n.inverse_scale_factor();
        let side = crate::hud::minimap_side(n, node);
        let at = (local + 0.5) * size / side;
        if at.x <= 1.0 && at.y <= 1.0 {
            let map = &sim.world.map;
            let (x, z) = (at.x * map.width.to_f32(), at.y * map.height.to_f32());
            cam.pivot = Vec3::new(x, sim.ground_y(x, z), z);
        }
        return true;
    }
    crate::hud::over_hud(&hud.panels, point)
}

/// What a build-grid slot does for the selection: a constructor's slot starts placing that
/// building, a building's trains that unit or researches that upgrade. Read from the same
/// menu the grid shows.
fn press_slot(sim: &mut Sim, sel: &Selection, placing: &mut Placement, i: usize) {
    use crate::hud::Slot;
    let Some(&slot) = crate::hud::build_menu(sim, sel).get(i) else { return };
    match (slot, selected_constructor(sim, sel), selected_building(sim, sel)) {
        (Slot::Def(def), Some(_), _) => placing.0 = Some(def),
        (Slot::Def(def), None, Some(producer)) => sim.order(CommandKind::Train { producer, def }),
        (Slot::Upgrade(upgrade), None, Some(producer)) => sim.order(CommandKind::Research { producer, upgrade }),
        _ => {}
    }
}

#[derive(bevy::ecs::system::SystemParam)]
struct HudQueries<'w, 's> {
    panels: crate::hud::Panels<'w, 's>,
    buttons: Query<
        'w,
        's,
        (
            &'static crate::hud::GridButton,
            &'static ComputedNode,
            &'static bevy::ui::UiGlobalTransform,
            &'static InheritedVisibility,
        ),
    >,
    minimap: Query<
        'w,
        's,
        (&'static ComputedNode, &'static bevy::ui::UiGlobalTransform, &'static InheritedVisibility, &'static Node),
        With<crate::hud::MinimapView>,
    >,
}

#[allow(clippy::too_many_arguments)]
fn mouse(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut cam: Query<(&Camera, &GlobalTransform, &mut RtsCamera)>,
    mut sim: ResMut<Sim>,
    mut sel: ResMut<Selection>,
    mut placing: ResMut<Placement>,
    mut aim: ResMut<PowerAim>,
    mut drag: ResMut<Drag>,
    hud: HudQueries,
    settings: Res<crate::settings::Settings>,
    pan: Res<crate::camera::PanDrag>,
    time: Res<Time>,
    mut marks: ResMut<crate::feedback::Marks>,
) {
    let (Ok(window), Ok((camera, cam_tf, mut rts))) = (window.single(), cam.single_mut()) else { return };
    let Some(cursor) = window.cursor_position() else { return };
    // A press on the HUD is the HUD's: it never selects, orders or places in the world. A
    // drag that started in the world still ends normally over the HUD.
    let pressed = buttons.get_just_pressed().next().is_some();
    if pressed && crate::hud::over_hud(&hud.panels, cursor) {
        if buttons.just_pressed(MouseButton::Left) && aim.0.is_none() {
            hud_click(cursor, &mut sim, &sel, &mut placing, &hud, &mut rts);
        }
        return;
    }
    let cam = (camera, cam_tf);
    let Some(ground) = cursor_ground(&sim, window, cam) else { return };
    let here = to_sim(ground);
    // A right click is complete on release, unless the right button was dragging the camera.
    if buttons.just_pressed(MouseButton::Right) {
        drag.right = true;
    }
    let right_click = buttons.just_released(MouseButton::Right) && core::mem::take(&mut drag.right) && !pan.ended;
    let classic = settings.scheme == crate::settings::Scheme::Classic;
    let attack_move = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);

    if let Some(slot) = aim.0 {
        if buttons.just_pressed(MouseButton::Left) {
            if slot == SPECIAL {
                let me = sim.local;
                if let Some(target) = pick(&sim, here, |e| e.owner.is_some_and(|o| o != me)) {
                    let units = sel.0.clone();
                    sim.order(CommandKind::Special { units, target });
                }
            } else {
                sim.order(CommandKind::Power { slot, at: here });
            }
            aim.0 = None;
        }
        if right_click {
            aim.0 = None;
        }
        return;
    }

    let me = sim.local;
    if let Some(def) = placing.0 {
        if buttons.just_pressed(MouseButton::Left)
            && let Some(builder) = selected_constructor(&sim, &sel)
        {
            sim.order(CommandKind::Build { builder, def, pos: here });
            if !keys.pressed(KeyCode::ShiftLeft) {
                placing.0 = None;
            }
        }
        if right_click {
            placing.0 = None;
        }
        return;
    }

    if buttons.just_pressed(MouseButton::Left) {
        drag.left = window.cursor_position();
    }
    if buttons.just_released(MouseButton::Left)
        && let (Some(start), Some(end)) = (drag.left.take(), window.cursor_position())
    {
        let click = start.distance(end) < 6.0;
        let now = time.elapsed_secs_f64();
        let double = click
            && drag.last_click.is_some_and(|(at, t)| now - t < DOUBLE_CLICK.0 && at.distance(end) < DOUBLE_CLICK.1);
        drag.last_click = click.then_some((end, now));
        // Classic: a click that would order the selection orders it instead of selecting,
        // unless it is the second of a double click (OpenRA's `!multiClick`).
        if classic && click && !double && overrides_selection(&sim, &sel, here) {
            give_orders(&mut sim, &sel, here, attack_move, &mut marks);
            return;
        }
        // Support jets fly their pass on their own: they can't be selected.
        let selectable = |e: &g2_sim::Entity| e.owner == Some(me) && !sim.world.rules.def(e.def).is_support_jet();
        // A double click on one of ours adds every one of its kind on screen (OpenRA's
        // `SelectActorsOnScreen` with its selection class).
        if double && let Some(def) = pick(&sim, here, selectable).and_then(|id| sim.world.entity(id)).map(|e| e.def) {
            for id in crate::select::on_screen(&sim, cam, |e| e.def == def) {
                if !sel.0.contains(&id) {
                    sel.0.push(id);
                }
            }
            return;
        }
        let additive = keys.pressed(KeyCode::ShiftLeft);
        if !additive {
            sel.0.clear();
        }
        if click {
            if let Some(id) = pick(&sim, here, selectable) {
                sel.0.push(id);
            }
        } else {
            let (lo, hi) = (start.min(end), start.max(end));
            for e in sim.world.entities().filter(|e| selectable(e) && sim.world.rules.def(e.def).is_unit()) {
                if let Ok(p) = cam.0.world_to_viewport(cam.1, sim.to_world(e.pos))
                    && p.cmpge(lo).all()
                    && p.cmple(hi).all()
                {
                    sel.0.push(e.id);
                }
            }
        }
        sel.0.dedup();
        let first_unit = sel
            .0
            .iter()
            .copied()
            .find(|&id| sim.world.entity(id).is_some_and(|e| sim.world.rules.def(e.def).is_unit()));
        if let Some(unit) = first_unit {
            sim.bypass_change_detection().selected = Some(unit);
        }
    }

    if right_click {
        if classic {
            sel.0.clear();
        } else {
            give_orders(&mut sim, &sel, here, attack_move, &mut marks);
        }
    }
}

/// The selection's own finished buildings that train units: the ones a rally point applies to.
fn selected_producers(sim: &Sim, sel: &Selection) -> Vec<EntityId> {
    let rules = sim.world.rules;
    sel.0
        .iter()
        .copied()
        .filter(|&id| {
            sim.world
                .entity(id)
                .is_some_and(|e| e.owner == Some(sim.local) && e.is_complete() && !rules.def(e.def).produces.is_empty())
        })
        .collect()
}

/// The selected units' context order at `here`: attack an enemy, gather from a pile, help
/// build our foundation, get into a building with room, else move (attack-move with Ctrl).
/// Selected production buildings set their rally point at a click on open ground, as OpenRA's
/// rally targets terrain only. Each order leaves its mark (`feedback`).
fn give_orders(
    sim: &mut Sim,
    sel: &Selection,
    here: g2_sim::Vec2,
    attack_move: bool,
    marks: &mut crate::feedback::Marks,
) {
    let me = sim.local;
    let units: Vec<EntityId> = sel
        .0
        .iter()
        .copied()
        .filter(|&id| sim.world.entity(id).is_some_and(|e| sim.world.rules.def(e.def).is_unit()))
        .collect();
    let rules = sim.world.rules;
    // An enemy under the cursor wins over our own entities or a pile it stands next to.
    let enemy = pick(sim, here, |e| e.owner.is_some_and(|o| o != me));
    let target = enemy.or_else(|| pick(sim, here, |e| e.owner.is_none() || e.owner == Some(me)));
    if target.is_none() {
        for producer in selected_producers(sim, sel) {
            let kind = CommandKind::SetRally { producer, at: here };
            marks.order(&kind, here);
            sim.order(kind);
        }
    }
    if units.is_empty() {
        return;
    }
    let armed = |id: &EntityId| sim.world.entity(*id).is_some_and(|e| rules.def(e.def).weapon().is_some());
    let kinds = match target.and_then(|t| sim.world.entity(t)) {
        // Armed units attack; the rest of a mixed selection walks to the spot instead of
        // having the whole order rejected.
        Some(t) if t.owner.is_some_and(|o| o != me) => {
            let (fighters, others): (Vec<_>, Vec<_>) = units.into_iter().partition(armed);
            [
                (!fighters.is_empty()).then_some(CommandKind::Attack { units: fighters, target: t.id }),
                (!others.is_empty()).then_some(CommandKind::Move { units: others, target: here }),
            ]
        }
        Some(t) if matches!(rules.def(t.def).class, Class::SupplyPile) => {
            [Some(CommandKind::Gather { units, pile: t.id }), None]
        }
        Some(t) if t.owner == Some(me) && t.construction.is_some() => {
            [Some(CommandKind::Construct { units, site: t.id }), None]
        }
        // Infantry get into a building or vehicle with room for them; the rest move there.
        Some(t) if sim.world.garrison_room(me, t.id).is_some_and(|room| room > 0) => {
            let (infantry, others): (Vec<_>, Vec<_>) = units
                .into_iter()
                .partition(|id| sim.world.entity(*id).is_some_and(|e| rules.def(e.def).can_garrison()));
            [
                (!infantry.is_empty()).then_some(CommandKind::Garrison { units: infantry, container: t.id }),
                (!others.is_empty()).then_some(CommandKind::Move { units: others, target: here }),
            ]
        }
        _ if attack_move => {
            let (fighters, others): (Vec<_>, Vec<_>) = units.into_iter().partition(armed);
            [
                (!fighters.is_empty()).then_some(CommandKind::AttackMove { units: fighters, target: here }),
                (!others.is_empty()).then_some(CommandKind::Move { units: others, target: here }),
            ]
        }
        _ => [Some(CommandKind::Move { units, target: here }), None],
    };
    for kind in kinds.into_iter().flatten() {
        marks.order(&kind, here);
        sim.order(kind);
    }
}

/// Classic's left click orders rather than selects (OpenRA's `InputOverridesSelection`): with
/// units selected, anywhere but on one of our own entities, or on our own foundation or a
/// building with room for the selection's infantry, which the order uses; with production
/// buildings selected, on open ground, where it sets their rally point.
fn overrides_selection(sim: &Sim, sel: &Selection, here: g2_sim::Vec2) -> bool {
    let me = sim.local;
    let rules = sim.world.rules;
    let has_units = sel.0.iter().any(|&id| sim.world.entity(id).is_some_and(|e| rules.def(e.def).is_unit()));
    if !has_units {
        return !selected_producers(sim, sel).is_empty() && pick(sim, here, |_| true).is_none();
    }
    let Some(own) = pick(sim, here, |e| e.owner == Some(me)).and_then(|id| sim.world.entity(id)) else {
        return true;
    };
    let infantry = sel.0.iter().any(|&id| sim.world.entity(id).is_some_and(|e| rules.def(e.def).can_garrison()));
    own.construction.is_some() || (infantry && sim.world.garrison_room(me, own.id).is_some_and(|room| room > 0))
}

/// Topmost shown entity under the cursor: the one whose footprint contains `at`, smallest
/// first, so a unit standing next to a building is still clickable. Enemies hidden by the fog
/// can't be picked.
fn pick(sim: &Sim, at: g2_sim::Vec2, filter: impl Fn(&g2_sim::Entity) -> bool) -> Option<EntityId> {
    sim.world
        .entities()
        .filter(|e| sim.shows(e) && e.inside.is_none() && filter(e))
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
    let flat = |p: Vec3| Isometry3d::new(p + Vec3::Y * 0.4, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2));
    for &id in &sel.0 {
        let Some(e) = sim.world.entity(id) else { continue };
        let def = sim.world.rules.def(e.def);
        gizmos.circle(flat(sim.to_world(e.pos)), def.radius.to_f32() + 1.0, Color::srgb(0.3, 1.0, 0.4));
        if let Class::Building { power, power_radius, .. } = def.class
            && power > 0
        {
            gizmos.circle(flat(sim.to_world(e.pos)), power_radius.to_f32(), Color::srgba(1.0, 0.9, 0.2, 0.5));
        }
        // A selected building's rally point: a line out to a flag on a pole, as OpenRA's
        // `RallyPointIndicator` draws its path and flag, in the rally mark's colour.
        if let Some(rally) = e.rally.filter(|_| e.owner == Some(sim.local)) {
            let c = crate::feedback::color(crate::feedback::Kind::Rally);
            let (from, to) = (sim.to_world(e.pos) + Vec3::Y * 0.5, sim.to_world(rally) + Vec3::Y * 0.5);
            gizmos.line(from, to, c.with_alpha(0.6));
            let top = to + Vec3::Y * 6.0;
            gizmos.line(to, top, c);
            gizmos.linestrip([top, top + Vec3::new(2.5, -1.0, 0.0), top + Vec3::new(0.0, -2.0, 0.0)], c);
            gizmos.circle(flat(sim.to_world(rally)), 1.5, c);
        }
    }
    if let (Some(def), Ok(window), Ok(cam)) = (placing.0, window.single(), cam.single())
        && let Some(ground) = cursor_ground(&sim, window, cam)
    {
        let ok = sim.world.check_placement(def, to_sim(ground));
        let (pos, color) = match ok {
            Ok((snapped, _)) => (sim.to_world(snapped), Color::srgb(0.3, 1.0, 0.4)),
            Err(_) => (ground, Color::srgb(1.0, 0.25, 0.2)),
        };
        gizmos.circle(flat(pos), sim.world.rules.def(def).radius.to_f32(), color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_cycles_in_order_and_wraps() {
        let ids: Vec<EntityId> = (0..3).map(|i| EntityId { index: i, generation: 0 }).collect();
        assert_eq!(next_after(&ids, None), Some(ids[0]), "nothing selected: first");
        assert_eq!(next_after(&ids, Some(ids[0])), Some(ids[1]));
        assert_eq!(next_after(&ids, Some(ids[2])), Some(ids[0]), "wraps");
        let gone = EntityId { index: 9, generation: 0 };
        assert_eq!(next_after(&ids, Some(gone)), Some(ids[0]), "selection isn't a collector: first");
        assert_eq!(next_after(&[], Some(ids[0])), None);
    }

    /// A camera ray aimed at a point lands on that point, on the high and low ground of an
    /// imported map, not on the Y = 0 plane below or above it.
    #[test]
    fn cursor_ray_lands_on_imported_terrain() {
        let name = "zh:Defcon6";
        let file = g2_maps::load_file_named(&g2_maps::vendored_root(), name).unwrap();
        let map = file.to_map_def(name);
        let sim = Sim::new(map, Some(file), 1, Sim::players([g2_sim::Faction::Eu, g2_sim::Faction::Gla], true));
        let (w, h) = (sim.world.map.width.to_f32(), sim.world.map.height.to_f32());
        let mut relief = (f32::MAX, f32::MIN);
        for j in 1..20 {
            for i in 1..20 {
                let (x, z) = (w * i as f32 / 20.0, h * j as f32 / 20.0);
                let target = Vec3::new(x, sim.ground_y(x, z), z);
                relief = (relief.0.min(target.y), relief.1.max(target.y));
                // From where the in-game camera would sit: up and back along +Z, looking down.
                let origin = target + Vec3::new(0.0, 90.0, 55.0);
                let hit = ray_ground(&sim, origin, (target - origin).normalize()).unwrap();
                assert!(hit.distance(target) < 0.5, "aimed at {target}, hit {hit}");
            }
        }
        assert!(relief.1 - relief.0 > 10.0, "Defcon6 should have relief, got {relief:?}");
    }

    /// G arms the general's power only once it is ready, and never for a player without one;
    /// F1 arms the faction's first support power, on its own timer; each further key the next
    /// slot: F3 the EU's third support power, F4 the power Mercury Red's Strike Team perk adds.
    #[test]
    fn power_keys_arm_only_ready_powers() {
        use bevy::ecs::system::RunSystemOnce;
        let armed = |general: Option<g2_sim::GeneralId>, perks: &[g2_sim::defs::PerkId], key: KeyCode, ticks: u32| {
            let mut players = Sim::players([g2_sim::Faction::Eu, g2_sim::Faction::Gla], false);
            players[0].general = general;
            players[0].perks = perks.to_vec();
            let mut sim = Sim::new(g2_rules::crater_ridge(), None, 1, players);
            for _ in 0..ticks {
                sim.world.step();
            }
            let mut world = World::new();
            let mut keys = ButtonInput::<KeyCode>::default();
            keys.press(key);
            world.insert_resource(keys);
            world.insert_resource(sim);
            world.init_resource::<Selection>();
            world.init_resource::<Placement>();
            world.init_resource::<PowerAim>();
            world.run_system_once(keyboard).unwrap();
            world.resource::<PowerAim>().0
        };
        let rules = g2_rules::rules();
        let bulldog = g2_rules::generals::BULLDOG;
        let ready = rules.general(bulldog).power.unwrap().first_ticks;
        assert_eq!(armed(Some(bulldog), &[], KeyCode::KeyG, ready - 30), None, "still charging");
        assert_eq!(armed(Some(bulldog), &[], KeyCode::KeyG, ready), Some(0), "ready");
        assert_eq!(armed(None, &[], KeyCode::KeyG, ready), None, "no general, no power");
        let raid = rules.support_powers(g2_sim::Faction::Eu).next().unwrap().first_ticks;
        assert_eq!(armed(None, &[], KeyCode::F1, raid - 30), None, "support still charging");
        assert_eq!(armed(None, &[], KeyCode::F1, raid), Some(1), "support ready without a general");
        let drone = rules.support_powers(g2_sim::Faction::Eu).nth(2).unwrap();
        assert_eq!(drone.name, "UAV Spy Drone");
        assert_eq!(armed(None, &[], KeyCode::F3, drone.first_ticks), Some(3), "F3: the third support power");
        let (mercury, team) = (g2_rules::generals::MERCURY_RED, g2_rules::perks::MR_STRIKE_TEAM);
        let drop = rules.perk(team).adds_power.unwrap().first_ticks;
        assert_eq!(armed(Some(mercury), &[team], KeyCode::F4, drop), Some(4), "F4: the perk's power");
        assert_eq!(armed(Some(mercury), &[], KeyCode::F4, drop), None, "no perk, nothing on F4");
    }

    /// A click on the HUD is the HUD's: on a build-grid button it trains that slot's unit, on
    /// bare panel art it issues nothing, on the minimap it moves the camera there, and the
    /// same click off the HUD reaches the world (a click on open ground drops the selection).
    /// The nodes are laid out at scale 2 (sizes and centres in physical pixels), the cursor in
    /// logical pixels, as on a HiDPI window.
    #[test]
    fn hud_clicks_never_reach_the_world() {
        use bevy::ecs::system::RunSystemOnce;
        use bevy::ui::UiGlobalTransform;
        let click = |at: Vec2| -> (u32, usize, Vec3) {
            let mut sim = Sim::new(
                g2_rules::crater_ridge(),
                None,
                1,
                Sim::players([g2_sim::Faction::Eu, g2_sim::Faction::Gla], false),
            );
            let rules = sim.world.rules;
            let factory = sim
                .world
                .entities()
                .find(|e| e.owner == Some(sim.local) && !rules.def(e.def).produces.is_empty() && e.is_complete())
                .map(|e| e.id)
                .expect("a finished producer at the start");
            let pivot = sim.to_world(sim.world.entity(factory).unwrap().pos);
            sim.world.step();
            let mut world = World::new();
            let mut window = Window::default();
            window.resolution.set(1280.0, 720.0);
            window.set_cursor_position(Some(at));
            world.spawn((window, PrimaryWindow));
            let cam = RtsCamera {
                pivot,
                distance: crate::camera::DEFAULT_DISTANCE,
                yaw: crate::look::YAW,
                pitch: crate::look::PITCH,
            };
            let mut camera = Camera::default();
            camera.computed.target_info =
                Some(bevy::camera::RenderTargetInfo { physical_size: UVec2::new(1280, 720), scale_factor: 1.0 });
            let mut projection = PerspectiveProjection::default();
            bevy::camera::CameraProjection::update(&mut projection, 1280.0, 720.0);
            camera.computed.clip_from_view = bevy::camera::CameraProjection::get_clip_from_view(&projection);
            world.spawn((camera, GlobalTransform::from(cam.transform()), cam));
            // At scale 2, in logical pixels: a panel 0..400 x 500..720 holding a grid button at
            // 10..60 x 510..560 and a 200 x 200 minimap at 180..380 x 510..710.
            let node = |size: Vec2, centre: Vec2| {
                (
                    ComputedNode { size: size * 2.0, inverse_scale_factor: 0.5, ..ComputedNode::DEFAULT },
                    UiGlobalTransform::from_translation(centre * 2.0),
                    InheritedVisibility::VISIBLE,
                )
            };
            world.spawn((crate::hud::HudPanel, node(Vec2::new(400.0, 220.0), Vec2::new(200.0, 610.0))));
            world.spawn((crate::hud::GridButton(0), node(Vec2::new(50.0, 50.0), Vec2::new(35.0, 535.0))));
            world.spawn((
                crate::hud::MinimapView,
                Node { width: px(200), height: px(200), ..default() },
                node(Vec2::new(200.0, 200.0), Vec2::new(280.0, 610.0)),
            ));
            let mut buttons = ButtonInput::<MouseButton>::default();
            buttons.press(MouseButton::Left);
            world.insert_resource(buttons);
            world.init_resource::<ButtonInput<KeyCode>>();
            world.insert_resource(sim);
            world.insert_resource(Selection(vec![factory]));
            world.init_resource::<Placement>();
            world.init_resource::<PowerAim>();
            world.init_resource::<Drag>();
            world.init_resource::<crate::settings::Settings>();
            world.init_resource::<crate::camera::PanDrag>();
            world.init_resource::<Time>();
            world.init_resource::<crate::feedback::Marks>();
            world.run_system_once(mouse).unwrap();
            // A world click completes on release: release on the same spot.
            world.resource_mut::<ButtonInput<MouseButton>>().clear();
            world.resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
            world.run_system_once(mouse).unwrap();
            let pivot = world.query::<&RtsCamera>().single(&world).unwrap().pivot;
            (world.resource::<Sim>().unheard_orders, world.resource::<Selection>().0.len(), pivot)
        };
        let (orders, selected, home) = click(Vec2::new(35.0, 535.0));
        assert_eq!((orders, selected), (1, 1), "grid button: one Train order, selection kept");
        let (orders, selected, pivot) = click(Vec2::new(130.0, 650.0));
        assert_eq!((orders, selected), (0, 1), "bare panel: no order, selection kept");
        assert_eq!(pivot, home, "bare panel: camera stays");
        // The minimap's centre is the map's centre.
        let (orders, selected, pivot) = click(Vec2::new(280.0, 610.0));
        assert_eq!((orders, selected), (0, 1), "minimap: no order, selection kept");
        let map = g2_rules::crater_ridge();
        let centre = Vec2::new(map.width.to_f32(), map.height.to_f32()) / 2.0;
        assert!(
            Vec2::new(pivot.x, pivot.z).distance(centre) < 1.0,
            "minimap centre moves the camera to {centre}, got {pivot}"
        );
        assert_eq!(click(Vec2::new(640.0, 200.0)).1, 0, "open ground: the world takes the click");
    }

    /// `mouse` through a real camera and window over the local base, in an app so time runs.
    struct Clicks {
        app: App,
    }

    impl Clicks {
        fn new(sim: Sim, sel: Vec<EntityId>, at: Vec3) -> Clicks {
            use bevy::time::TimeUpdateStrategy;
            let mut app = App::new();
            app.add_plugins(MinimalPlugins)
                .insert_resource(TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(16)))
                .init_resource::<ButtonInput<MouseButton>>()
                .init_resource::<ButtonInput<KeyCode>>()
                .insert_resource(sim)
                .insert_resource(Selection(sel))
                .init_resource::<Placement>()
                .init_resource::<PowerAim>()
                .init_resource::<Drag>()
                .init_resource::<crate::settings::Settings>()
                .init_resource::<crate::camera::PanDrag>()
                .init_resource::<crate::feedback::Marks>()
                .add_systems(Update, mouse);
            let mut window = Window::default();
            window.resolution.set(1280.0, 720.0);
            app.world_mut().spawn((window, PrimaryWindow));
            crate::select::tests::spawn_camera(app.world_mut(), at);
            app.update();
            Clicks { app }
        }

        /// The screen point of world `p`.
        fn screen(&mut self, p: Vec3) -> Vec2 {
            let w = self.app.world_mut();
            let (c, tf) = w.query::<(&Camera, &GlobalTransform)>().single(w).unwrap();
            c.world_to_viewport(tf, p).unwrap()
        }

        /// A press and a release of `button` at `at`, a frame each.
        fn click(&mut self, button: MouseButton, at: Vec2) {
            let w = self.app.world_mut();
            w.query::<&mut Window>().single_mut(w).unwrap().set_cursor_position(Some(at));
            for press in [true, false] {
                let mut b = self.app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
                b.clear();
                if press {
                    b.press(button)
                } else {
                    b.release(button)
                }
                self.app.update();
            }
        }
    }

    /// A right click on open ground with only a factory selected sets its rally point there,
    /// with a teal mark; a right click on a unit of ours does not.
    #[test]
    fn right_click_on_the_ground_sets_the_rally_point() {
        let (mut sim, squads) = crate::select::tests::sim_with_units();
        let barracks = sim.world.entities().find(|e| e.def == g2_rules::ids::GLA_BARRACKS).unwrap().id;
        // Out of the way, so open ground in front of the barracks is clear.
        let away = sim.world.entity(barracks).unwrap().pos + g2_sim::Vec2::from_int(0, 60);
        sim.world.submit(sim.local, CommandKind::Move { units: squads.clone(), target: away });
        for _ in 0..30 * 15 {
            sim.world.step();
        }
        let b = sim.to_world(sim.world.entity(barracks).unwrap().pos);
        let unit_at = sim.to_world(sim.world.entity(squads[0]).unwrap().pos);
        let mut c = Clicks::new(sim, vec![barracks], b);
        let on_unit = c.screen(unit_at);
        c.click(MouseButton::Right, on_unit);
        assert_eq!(c.app.world().resource::<Sim>().unheard_orders, 0, "a click on a unit sets no rally");
        let ground = c.screen(b + Vec3::new(-25.0, 0.0, 0.0));
        c.click(MouseButton::Right, ground);
        let sim = &mut c.app.world_mut().resource_mut::<Sim>();
        assert_eq!(sim.unheard_orders, 1);
        for _ in 0..=g2_sim::world::INPUT_DELAY {
            sim.world.step();
        }
        let rally = sim.world.entity(barracks).unwrap().rally.expect("rally set");
        assert!((rally.x.to_f32() - (b.x - 25.0)).abs() < 1.5 && (rally.y.to_f32() - b.z).abs() < 1.5, "{rally:?}");
        let marks = &c.app.world().resource::<crate::feedback::Marks>().0;
        assert!(marks.iter().any(|m| m.kind == crate::feedback::Kind::Rally), "{marks:?}");
    }

    /// A double click on one squad selects every squad of ours on screen, not the worker; two
    /// clicks slower than OpenRA's 250 ms select just the one.
    #[test]
    fn a_double_click_selects_the_kind_on_screen() {
        let (sim, squads) = crate::select::tests::sim_with_units();
        let first = sim.to_world(sim.world.entity(squads[0]).unwrap().pos);
        let mut c = Clicks::new(sim, Vec::new(), first);
        let at = c.screen(first);
        c.click(MouseButton::Left, at);
        assert_eq!(c.app.world().resource::<Selection>().0, vec![squads[0]], "one click: one squad");
        c.click(MouseButton::Left, at);
        assert_eq!(c.app.world().resource::<Selection>().0, squads, "double click: every squad on screen");
        for _ in 0..20 {
            c.app.update();
        }
        c.click(MouseButton::Left, at);
        assert_eq!(c.app.world().resource::<Selection>().0, vec![squads[0]], "a later click selects one again");
    }
}
