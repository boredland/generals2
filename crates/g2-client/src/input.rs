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

/// A power is armed, by slot (0 the general's, then support and perk powers), the selected
/// units' special shot (`SPECIAL`) or one of their aimed abilities (`ABILITY` plus its slot):
/// the next left click on the ground uses the power or ability there, on an enemy fires the
/// special or the ability's shot at it.
#[derive(Resource, Default)]
pub struct PowerAim(pub Option<u8>);

/// `PowerAim` for the selection's special shot (R) rather than a power slot.
pub const SPECIAL: u8 = u8::MAX;
/// `PowerAim` for the selection's aimed ability in slot `n`: `ABILITY + n`.
pub const ABILITY: u8 = 0xf0;

/// The keys of a unit's ability slots (`Def::abilities`), in order: R, then F and C beside
/// it, none of them a camera, power or command key.
pub const ABILITY_KEYS: [KeyCode; g2_sim::defs::MAX_ABILITIES] = [KeyCode::KeyR, KeyCode::KeyF, KeyCode::KeyC];
/// How the HUD and tooltips name them.
pub const ABILITY_LABELS: [&str; g2_sim::defs::MAX_ABILITIES] = ["R", "F", "C"];

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

/// How far (in logical pixels) the cursor may move between press and release for a left
/// press to still be a click rather than a drag.
const CLICK_SLOP: f32 = 6.0;

/// The rubber band of a left drag over the world, as Generals draws it
/// (`W3DInGameUI::drawSelectionRegion`): an open rectangle 2 pixels wide in translucent green
/// (`0x9933FF33`), from the press to the cursor.
#[derive(Component)]
struct DragBox;

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
                (prune_selection, keyboard, cycle_collectors, mouse, draw_overlays, drag_box)
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
    typed: Option<Res<ButtonInput<bevy::input::keyboard::Key>>>,
    mut sim: ResMut<Sim>,
    sel: Res<Selection>,
    mut placing: ResMut<Placement>,
    mut aim: ResMut<PowerAim>,
) {
    // Online the clock is shared: pausing or speeding up one machine would only stall the
    // other, so these keys do nothing there. + and - are taken by the character typed too, so
    // they work on layouts where they aren't the US keys' places (German: + beside Enter).
    let local_clock = sim.online.is_none();
    let char =
        |c: &str| typed.as_ref().is_some_and(|t| t.just_pressed(bevy::input::keyboard::Key::Character(c.into())));
    if local_clock && keys.just_pressed(KeyCode::Space) {
        sim.speed = if sim.speed == 0 { 1 } else { 0 };
    }
    if local_clock && (keys.any_just_pressed([KeyCode::Equal, KeyCode::NumpadAdd]) || char("+")) {
        sim.speed = (sim.speed * 2).clamp(1, 8);
    }
    if local_clock && (keys.any_just_pressed([KeyCode::Minus, KeyCode::NumpadSubtract]) || char("-")) {
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
    // R, F, C: the selected units with that ability slot ready use it, at once if it needs no
    // aim, else it is armed and the next left click aims it. R also arms the special shot of
    // those that have one; the next left click on an enemy fires it.
    for (slot, key) in ABILITY_KEYS.into_iter().enumerate() {
        if !keys.just_pressed(key) {
            continue;
        }
        let (now, aimed) = ability_ready(&sim, &sel.0, slot as u8);
        if !now.is_empty() {
            sim.order(g2_sim::ability::in_place(now, slot as u8));
        }
        let special = slot == 0 && {
            let tick = sim.world.tick();
            sel.0.iter().any(|&id| {
                sim.world.entity(id).is_some_and(|e| {
                    e.owner == Some(sim.local) && rules.def(e.def).special.is_some() && e.special_at <= tick
                })
            })
        };
        if aimed {
            aim.0 = Some(ABILITY + slot as u8);
            placing.0 = None;
        } else if special {
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

/// The selected own units whose ability `slot` is ready and used where they stand, and whether
/// any has one ready there that must be aimed.
fn ability_ready(sim: &Sim, sel: &[EntityId], slot: u8) -> (Vec<EntityId>, bool) {
    use g2_sim::defs::AbilityAim;
    let world = &sim.world;
    let ready = sel.iter().filter_map(|&id| {
        let e = world.entity(id).filter(|e| e.owner == Some(sim.local))?;
        let a = world.rules.def(e.def).abilities.get(slot as usize)?;
        (world.ability_wait(e, slot) == Some(0)).then_some((id, a.kind.aim()))
    });
    let (mut now, mut aimed) = (Vec::new(), false);
    for (id, kind) in ready {
        match kind {
            AbilityAim::Here => now.push(id),
            AbilityAim::Point(_) | AbilityAim::Unit(_) => aimed = true,
        }
    }
    (now, aimed)
}

/// The command an armed ability (`ABILITY` + `slot`) gives for a left click at `here`: on an
/// enemy under the cursor, at it, for units whose ability there shoots a unit; else at the
/// ground.
fn aimed_ability(sim: &Sim, sel: &[EntityId], slot: u8, here: g2_sim::Vec2) -> CommandKind {
    use g2_sim::command::Aim;
    use g2_sim::defs::AbilityAim;
    let world = &sim.world;
    let shoots = sel.iter().any(|&id| {
        world
            .entity(id)
            .and_then(|e| world.rules.def(e.def).abilities.get(slot as usize))
            .is_some_and(|a| matches!(a.kind.aim(), AbilityAim::Unit(_)))
    });
    let me = sim.local;
    let enemy = shoots.then(|| pick(sim, here, |e| e.owner.is_some_and(|o| o != me))).flatten();
    let aim = enemy.map_or(Aim::Point(here), Aim::Unit);
    CommandKind::Ability { units: sel.to_vec(), slot, aim }
}

/// A click on the HUD: a build-grid cell builds, trains or researches its slot (a right click
/// cancels one of it instead), a speed button sets the game speed, a ready power's tile arms it
/// as its key does, the minimap moves the camera there (left click only). Returns whether the
/// HUD took the click. MENU is the menu's (`menu::interact`).
#[allow(clippy::too_many_arguments)]
fn hud_click(
    point: Vec2,
    right: bool,
    sim: &mut Sim,
    sel: &Selection,
    placing: &mut Placement,
    aim: &mut PowerAim,
    hud: &HudQueries,
    cam: &mut RtsCamera,
) -> bool {
    use crate::hud::HudButton;
    let inside = |node: &ComputedNode, tf: &bevy::ui::UiGlobalTransform, vis: &InheritedVisibility| {
        vis.get() && node.contains_point(*tf, point / node.inverse_scale_factor())
    };
    if let Some(cell) = hud.buttons.iter().find(|(_, n, tf, v)| inside(n, tf, v)).map(|(b, ..)| b.0) {
        if right {
            cancel_slot(sim, sel, cell);
        } else {
            press_slot(sim, sel, placing, cell);
        }
        return true;
    }
    if let Some(&b) = hud.hud_buttons.iter().find(|(_, n, tf, v)| inside(n, tf, v)).map(|(b, ..)| b) {
        match b {
            HudButton::Speed(s) if !right && sim.online.is_none() => sim.speed = s,
            HudButton::Power(slot) if !right && sim.power_wait(slot) == Some(0) => {
                aim.0 = Some(slot);
                placing.0 = None;
            }
            _ => {}
        }
        return true;
    }
    if right {
        return crate::hud::over_hud(&hud.panels, point);
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
/// building, a building's trains that unit or researches that upgrade, a foundation's cancels
/// it. Read from the same menu the grid shows.
fn press_slot(sim: &mut Sim, sel: &Selection, placing: &mut Placement, i: usize) {
    use crate::hud::Slot;
    let Some(&slot) = crate::hud::build_menu(sim, sel).get(i) else { return };
    match (slot, selected_constructor(sim, sel), selected_building(sim, sel)) {
        (Slot::Def(def), Some(_), _) => placing.0 = Some(def),
        (Slot::Def(def), None, Some(producer)) => sim.order(CommandKind::Train { producer, def }),
        (Slot::Upgrade(upgrade), None, Some(producer)) => sim.order(CommandKind::Research { producer, upgrade }),
        (Slot::Cancel, _, Some(site)) => sim.order(CommandKind::CancelConstruction { site }),
        _ => {}
    }
}

/// A right click on a build-grid slot: the last queued item of its kind at the selected
/// building goes, for a full refund (Generals' and OpenRA's right-click cancel).
fn cancel_slot(sim: &mut Sim, sel: &Selection, i: usize) {
    let Some(item) = crate::hud::build_menu(sim, sel).get(i).and_then(|s| s.product()) else { return };
    let Some(producer) = selected_building(sim, sel) else { return };
    if sim.world.entity(producer).is_some_and(|e| e.queue.iter().any(|p| p.item == item)) {
        sim.order(CommandKind::CancelItem { producer, item });
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
    hud_buttons: Query<
        'w,
        's,
        (
            &'static crate::hud::HudButton,
            &'static ComputedNode,
            &'static bevy::ui::UiGlobalTransform,
            &'static InheritedVisibility,
        ),
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
        let left = buttons.just_pressed(MouseButton::Left);
        if (left || buttons.just_pressed(MouseButton::Right)) && aim.0.is_none() {
            hud_click(cursor, !left, &mut sim, &sel, &mut placing, &mut aim, &hud, &mut rts);
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
            } else if slot >= ABILITY {
                let order = aimed_ability(&sim, &sel.0, slot - ABILITY, here);
                sim.order(order);
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
        let click = start.distance(end) < CLICK_SLOP;
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
/// On a bridge deck, Ctrl fires at the bridge (Zero Hour's force-attack) and constructors
/// repair a damaged one. Selected production buildings set their rally point at a click on
/// open ground, as OpenRA's rally targets terrain only. Each order leaves its mark (`feedback`).
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
        None if let Some(bridge) = sim.world.bridge_at(here).filter(|_| attack_move) => {
            let (fighters, others): (Vec<_>, Vec<_>) = units.into_iter().partition(armed);
            [
                (!fighters.is_empty()).then_some(CommandKind::AttackBridge { units: fighters, bridge }),
                (!others.is_empty()).then_some(CommandKind::Move { units: others, target: here }),
            ]
        }
        None if let Some(bridge) = sim.world.bridge_at(here).filter(|&b| {
            sim.world.bridge(b).is_some_and(|b| b.hp < b.max)
                && units.iter().any(|&u| sim.world.entity(u).is_some_and(|e| rules.def(e.def).is_constructor()))
        }) =>
        {
            let (builders, others): (Vec<_>, Vec<_>) = units
                .into_iter()
                .partition(|&u| sim.world.entity(u).is_some_and(|e| rules.def(e.def).is_constructor()));
            [
                Some(CommandKind::RepairBridge { units: builders, bridge }),
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

/// Shows `DragBox` over a left drag that has gone past `CLICK_SLOP`, and hides it otherwise
/// (the HUD's own presses never start one: `mouse` returns before recording them). It hides by
/// `Display`: its `Visibility` is `menu::hide_hud`'s, as for every top-level match node.
fn drag_box(
    mut commands: Commands,
    drag: Res<Drag>,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut boxes: Query<&mut Node, With<DragBox>>,
) {
    let Ok(mut node) = boxes.single_mut() else {
        commands.spawn((
            crate::menu::MatchEntity,
            DragBox,
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                border: UiRect::all(px(2)),
                ..default()
            },
            BorderColor::all(Color::srgba_u8(0x33, 0xff, 0x33, 0x99)),
        ));
        return;
    };
    let cursor = window.single().ok().and_then(Window::cursor_position);
    let band = drag
        .left
        .zip(cursor)
        .filter(|(start, end)| buttons.pressed(MouseButton::Left) && start.distance(*end) >= CLICK_SLOP);
    let Some((start, end)) = band else {
        if node.display != Display::None {
            node.display = Display::None;
        }
        return;
    };
    let (lo, hi) = (start.min(end), start.max(end));
    node.display = Display::Flex;
    node.left = px(lo.x);
    node.top = px(lo.y);
    node.width = px(hi.x - lo.x);
    node.height = px(hi.y - lo.y);
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

    /// A click on the HUD is the HUD's: on a build-grid button a left click trains that slot's
    /// unit and a right click cancels one of it queued (none queued: nothing), on a speed
    /// button it sets the game speed, on bare panel art it issues nothing, on the minimap it
    /// moves the camera there, and the same click off the HUD reaches the world (a click on
    /// open ground drops the selection). The nodes are laid out at scale 2 (sizes and centres
    /// in physical pixels), the cursor in logical pixels, as on a HiDPI window.
    #[test]
    fn hud_clicks_never_reach_the_world() {
        use bevy::ecs::system::RunSystemOnce;
        use bevy::ui::UiGlobalTransform;
        let click_with = |at: Vec2, button: MouseButton, queued: bool| -> (u32, usize, Vec3, usize, u32) {
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
            let first = crate::hud::build_menu(&sim, &Selection(vec![factory]))[0];
            if queued {
                let crate::hud::Slot::Def(def) = first else { panic!("a unit first: {first:?}") };
                sim.world.submit(sim.local, CommandKind::Train { producer: factory, def });
                for _ in 0..=g2_sim::world::INPUT_DELAY {
                    sim.world.step();
                }
                assert_eq!(sim.world.entity(factory).unwrap().queue.len(), 1, "one queued");
            }
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
            // 10..60 x 510..560, an x4 speed button at 85..115 x 520..550 and a 200 x 200
            // minimap at 180..380 x 510..710.
            let node = |size: Vec2, centre: Vec2| {
                (
                    ComputedNode { size: size * 2.0, inverse_scale_factor: 0.5, ..ComputedNode::DEFAULT },
                    UiGlobalTransform::from_translation(centre * 2.0),
                    InheritedVisibility::VISIBLE,
                )
            };
            world.spawn((crate::hud::HudPanel, node(Vec2::new(400.0, 220.0), Vec2::new(200.0, 610.0))));
            world.spawn((crate::hud::GridButton(0), node(Vec2::new(50.0, 50.0), Vec2::new(35.0, 535.0))));
            world.spawn((crate::hud::HudButton::Speed(4), node(Vec2::new(30.0, 30.0), Vec2::new(100.0, 535.0))));
            world.spawn((
                crate::hud::MinimapView,
                Node { width: px(200), height: px(200), ..default() },
                node(Vec2::new(200.0, 200.0), Vec2::new(280.0, 610.0)),
            ));
            let mut buttons = ButtonInput::<MouseButton>::default();
            buttons.press(button);
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
            world.resource_mut::<ButtonInput<MouseButton>>().release(button);
            world.run_system_once(mouse).unwrap();
            let pivot = world.query::<&RtsCamera>().single(&world).unwrap().pivot;
            let (orders, selected) = (world.resource::<Sim>().unheard_orders, world.resource::<Selection>().0.len());
            // What the orders did once they are due: the factory's queue.
            let mut sim = world.remove_resource::<Sim>().unwrap();
            for _ in 0..=g2_sim::world::INPUT_DELAY {
                sim.world.step();
            }
            (orders, selected, pivot, sim.world.entity(factory).unwrap().queue.len(), sim.speed)
        };
        let click = |at: Vec2| {
            let (orders, selected, pivot, ..) = click_with(at, MouseButton::Left, false);
            (orders, selected, pivot)
        };
        let (orders, selected, home) = click(Vec2::new(35.0, 535.0));
        assert_eq!((orders, selected), (1, 1), "grid button: one Train order, selection kept");
        let (orders, selected, _, queue, _) = click_with(Vec2::new(35.0, 535.0), MouseButton::Right, true);
        assert_eq!((orders, selected), (1, 1), "right click on a queued slot: one order, selection kept");
        assert_eq!(queue, 0, "and it cancels the one queued");
        let (orders, ..) = click_with(Vec2::new(35.0, 535.0), MouseButton::Right, false);
        assert_eq!(orders, 0, "right click with none queued: nothing");
        let (orders, selected, _, _, speed) = click_with(Vec2::new(100.0, 535.0), MouseButton::Left, false);
        assert_eq!((orders, selected, speed), (0, 1, 4), "speed button: x4, no order, selection kept");
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
                .add_systems(Update, (mouse, drag_box).chain());
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

    /// A left drag over the world draws Generals' green band from the press to the cursor once
    /// it is past a click's slop, and the release selects what it covers and takes the band away.
    #[test]
    fn a_left_drag_draws_the_band_until_released() {
        let (sim, squads) = crate::select::tests::sim_with_units();
        let first = sim.to_world(sim.world.entity(squads[0]).unwrap().pos);
        let mut c = Clicks::new(sim, Vec::new(), first);
        let at = c.screen(first);
        let band = |c: &mut Clicks| {
            let w = c.app.world_mut();
            let node = w.query_filtered::<&Node, With<DragBox>>().single(w).unwrap();
            (node.display != Display::None).then_some((node.left, node.top, node.width, node.height))
        };
        let (start, end) = (at - Vec2::splat(40.0), at + Vec2::splat(40.0));
        let frame = |c: &mut Clicks, cursor: Vec2, press: Option<bool>| {
            let w = c.app.world_mut();
            w.query::<&mut Window>().single_mut(w).unwrap().set_cursor_position(Some(cursor));
            let mut b = c.app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            b.clear();
            match press {
                Some(true) => b.press(MouseButton::Left),
                Some(false) => b.release(MouseButton::Left),
                None => {}
            }
            c.app.update();
        };
        frame(&mut c, start, Some(true));
        assert_eq!(band(&mut c), None, "a press alone is still a click");
        frame(&mut c, start + Vec2::splat(3.0), None);
        assert_eq!(band(&mut c), None, "within the click's slop: no band");
        frame(&mut c, end, None);
        assert_eq!(band(&mut c), Some((px(start.x), px(start.y), px(80.0), px(80.0))), "from the press to the cursor");
        frame(&mut c, end, Some(false));
        assert_eq!(band(&mut c), None, "released: gone");
        assert!(c.app.world().resource::<Selection>().0.contains(&squads[0]), "and it selected what it covered");
    }
}
