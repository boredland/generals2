//! Mouse and keyboard → sim commands.
//!
//! Left click/drag: select own units (click on a building selects it).
//! Right click: context order (enemy → attack, supply pile → gather, own foundation →
//! construct, ground → move). Ctrl + right click on the ground attack-moves.
//! With a constructor selected: B opens build mode, 1-9 pick a building, left click places, Esc cancels.
//! With a building selected: 1-9 queue production, Backspace cancels the last item.
//! Clicking a build-grid button is pressing its key; clicking the minimap moves the camera
//! there. A click on any HUD panel stays with the HUD: it never selects or orders in the world.
//! Space pauses, +/- change speed (×1, ×2, ×4, ×8). F10 surrenders. Tab selects the next own
//! collector and centres the camera on it. G arms the general's power: the next left click
//! aims it (right click or Esc disarms). Esc is handled by `menu` alone, so it cancels a
//! placement or an armed power before it opens the pause menu. M (mute) is handled by `audio`.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use g2_sim::{Class, CommandKind, DefId, EntityId};

use crate::camera::RtsCamera;
use crate::sim::{Sim, to_sim};

#[derive(Resource, Default)]
pub struct Selection(pub Vec<EntityId>);

#[derive(Resource, Default)]
pub struct Placement(pub Option<DefId>);

/// The general's power is armed: the next left click on the ground uses it there.
#[derive(Resource, Default)]
pub struct PowerAim(pub bool);

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

/// The gameplay input systems, as one unit for ordering against other plugins.
#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub struct InputSet;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Selection>()
            .init_resource::<Placement>()
            .init_resource::<PowerAim>()
            .init_resource::<Drag>()
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
    if keys.just_pressed(KeyCode::KeyG) && sim.power_ready_in().is_some_and(|t| t == 0) {
        aim.0 = true;
        placing.0 = None;
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
    }
    for (i, key) in DIGITS.iter().enumerate() {
        if keys.just_pressed(*key) {
            press_slot(&mut sim, &sel, &mut placing, i);
        }
    }
    if selected_constructor(&sim, &sel).is_none()
        && let Some(producer) = selected_building(&sim, &sel)
        && keys.just_pressed(KeyCode::Backspace)
    {
        sim.order(CommandKind::CancelTrain { producer });
    }
}

/// A left click on the HUD: a build-grid cell presses its hotkey, the minimap moves the
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
/// building, a building's queues that unit. Shared by the 1-9 keys and the grid buttons.
fn press_slot(sim: &mut Sim, sel: &Selection, placing: &mut Placement, i: usize) {
    let rules = sim.world.rules;
    if let Some(builder) = selected_constructor(sim, sel) {
        if let Some(&def) = rules.def(sim.world.entity(builder).unwrap().def).builds.get(i) {
            placing.0 = Some(def);
        }
    } else if let Some(producer) = selected_building(sim, sel)
        && let Some(&def) = rules.def(sim.world.entity(producer).unwrap().def).produces.get(i)
    {
        sim.order(CommandKind::Train { producer, def });
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
) {
    let (Ok(window), Ok((camera, cam_tf, mut rts))) = (window.single(), cam.single_mut()) else { return };
    let Some(cursor) = window.cursor_position() else { return };
    // A press on the HUD is the HUD's: it never selects, orders or places in the world. A
    // drag that started in the world still ends normally over the HUD.
    let pressed = buttons.get_just_pressed().next().is_some();
    if pressed && crate::hud::over_hud(&hud.panels, cursor) {
        if buttons.just_pressed(MouseButton::Left) && !aim.0 {
            hud_click(cursor, &mut sim, &sel, &mut placing, &hud, &mut rts);
        }
        return;
    }
    let cam = (camera, cam_tf);
    let Some(ground) = cursor_ground(&sim, window, cam) else { return };
    let here = to_sim(ground);

    if aim.0 {
        if buttons.just_pressed(MouseButton::Left) {
            sim.order(CommandKind::Power { at: here });
            aim.0 = false;
        }
        if buttons.just_pressed(MouseButton::Right) {
            aim.0 = false;
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
            if let Some(id) = pick(&sim, here, |e| e.owner == Some(me)) {
                sel.0.push(id);
            }
        } else {
            let (lo, hi) = (start.min(end), start.max(end));
            for e in sim.world.entities().filter(|e| e.owner == Some(me) && sim.world.rules.def(e.def).is_unit()) {
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

    if buttons.just_pressed(MouseButton::Right) && !sel.0.is_empty() {
        let units: Vec<EntityId> = sel
            .0
            .iter()
            .copied()
            .filter(|&id| sim.world.entity(id).is_some_and(|e| sim.world.rules.def(e.def).is_unit()))
            .collect();
        if units.is_empty() {
            return;
        }
        let rules = sim.world.rules;
        // An enemy under the cursor wins over our own entities or a pile it stands next to.
        let enemy = pick(&sim, here, |e| e.owner.is_some_and(|o| o != me));
        let target = enemy.or_else(|| pick(&sim, here, |e| e.owner.is_none() || e.owner == Some(me)));
        let attack_move = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
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
            sim.order(kind);
        }
    }
}

/// Topmost shown entity under the cursor: the one whose footprint contains `at`, smallest
/// first, so a unit standing next to a building is still clickable. Enemies hidden by the fog
/// can't be picked.
fn pick(sim: &Sim, at: g2_sim::Vec2, filter: impl Fn(&g2_sim::Entity) -> bool) -> Option<EntityId> {
    sim.world
        .entities()
        .filter(|e| sim.shows(e) && filter(e))
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

    /// G arms the general's power only once it is ready, and never for a player without one.
    #[test]
    fn g_arms_the_power_only_when_ready() {
        use bevy::ecs::system::RunSystemOnce;
        let armed = |general: Option<g2_sim::GeneralId>, ticks: u32| {
            let mut players = Sim::players([g2_sim::Faction::Eu, g2_sim::Faction::Gla], false);
            players[0].general = general;
            let mut sim = Sim::new(g2_rules::crater_ridge(), None, 1, players);
            for _ in 0..ticks {
                sim.world.step();
            }
            let mut world = World::new();
            let mut keys = ButtonInput::<KeyCode>::default();
            keys.press(KeyCode::KeyG);
            world.insert_resource(keys);
            world.insert_resource(sim);
            world.init_resource::<Selection>();
            world.init_resource::<Placement>();
            world.init_resource::<PowerAim>();
            world.run_system_once(keyboard).unwrap();
            world.resource::<PowerAim>().0
        };
        let bulldog = g2_rules::generals::BULLDOG;
        let ready = g2_rules::rules().general(bulldog).power.unwrap().first_ticks;
        assert!(!armed(Some(bulldog), ready - 30), "still charging");
        assert!(armed(Some(bulldog), ready), "ready");
        assert!(!armed(None, ready), "no general, no power");
    }

    /// A click on the HUD is the HUD's: on a build-grid button it trains that slot's unit as
    /// the hotkey would, on bare panel art it issues nothing, on the minimap it moves the
    /// camera there, and the same click off the HUD reaches the world (a click on open
    /// ground drops the selection). The nodes are laid out at scale 2 (sizes and centres in
    /// physical pixels), the cursor in logical pixels, as on a HiDPI window.
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
}
