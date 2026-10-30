//! Control groups and selecting many at once, as OpenRA does them (`ControlGroups.cs`,
//! `ControlGroupsWidget.cs`, `SelectionUtils.cs`, the `control-groups.yaml` and `game.yaml`
//! hotkeys). Client-side only: groups never reach the sim, as in OpenRA.
//!
//! `0`-`9` select a group (`ControlGroupSelect`); pressed twice within OpenRA's 250 ms
//! double-tap window it also centres the camera on the group. Ctrl + number makes the group
//! from the selection (`ControlGroupCreate`), Ctrl + Shift + number adds the selection to it
//! (`ControlGroupAddTo`), Shift + number adds the group to the selection
//! (`ControlGroupCombineWith`), Alt + number centres the camera on it without selecting
//! (`ControlGroupJumpTo`). A unit belongs to one group at most; dead units and units that
//! changed hands drop out every frame.
//!
//! OpenRA's `SelectAllUnits` (Q) and `SelectUnitsByType` (W) would rotate and pan the camera
//! here, so they sit on Z and X: Z selects every own unit on screen, or, pressed again with
//! nothing more on screen, every own unit on the map; X does the same for the kinds of unit
//! already selected, added to the selection. A double click on an own unit or building selects
//! every one of its kind on screen (`input.rs`).

use bevy::prelude::*;
use g2_sim::EntityId;

use crate::camera::RtsCamera;
use crate::input::Selection;
use crate::sim::Sim;

/// The number keys in group order: `1` is group 0 ... `0` is group 9, as OpenRA numbers them.
pub const GROUP_KEYS: [KeyCode; 10] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
    KeyCode::Digit0,
];

/// OpenRA's multi-tap window (`MultiTapDetection`): a second press of the same key with the
/// same modifiers within this many seconds is a double tap.
pub const DOUBLE_TAP: f64 = 0.25;

pub const SELECT_ALL_KEY: KeyCode = KeyCode::KeyZ;
pub const SELECT_BY_TYPE_KEY: KeyCode = KeyCode::KeyX;

/// The local player's control groups, for this match.
#[derive(Resource, Default)]
pub struct Groups {
    pub groups: [Vec<EntityId>; 10],
    /// The last group key: which, with which modifiers, and when (`Time` seconds).
    last_tap: Option<(usize, Mods, f64)>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Mods {
    ctrl: bool,
    shift: bool,
    alt: bool,
}

impl Mods {
    fn of(keys: &ButtonInput<KeyCode>) -> Mods {
        // OpenRA's create and add-to keys use Meta on macOS in place of Ctrl.
        let ctrl = if cfg!(target_os = "macos") {
            [KeyCode::SuperLeft, KeyCode::SuperRight]
        } else {
            [KeyCode::ControlLeft, KeyCode::ControlRight]
        };
        Mods {
            ctrl: keys.any_pressed(ctrl),
            shift: keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
            alt: keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]),
        }
    }
}

impl Groups {
    /// `ControlGroups.CreateControlGroup`: the group becomes the selection's own entities, which
    /// leave any other group. An empty selection leaves the group as it was.
    fn create(&mut self, group: usize, sel: &[EntityId]) {
        if sel.is_empty() {
            return;
        }
        self.groups[group].clear();
        self.add_to(group, sel);
    }

    /// `ControlGroups.AddSelectionToControlGroup`.
    fn add_to(&mut self, group: usize, sel: &[EntityId]) {
        for g in &mut self.groups {
            g.retain(|id| !sel.contains(id));
        }
        self.groups[group].extend_from_slice(sel);
    }

    /// Drops every entity that is gone or no longer the local player's (`ControlGroups.Tick`).
    fn forget(&mut self, sim: &Sim) {
        for g in &mut self.groups {
            g.retain(|&id| sim.world.entity(id).is_some_and(|e| e.owner == Some(sim.local)));
        }
    }
}

pub struct SelectPlugin;

// Groups are per match: `menu::swap_world` puts in fresh ones.
impl Plugin for SelectPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Groups>().add_systems(
            Update,
            (group_keys, select_keys)
                .chain()
                .after(crate::input::keyboard)
                .in_set(crate::input::InputSet)
                .run_if(crate::menu::playing),
        );
    }
}

/// The number keys: select, create, add to, combine with, or jump to a control group.
pub(crate) fn group_keys(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    sim: Res<Sim>,
    mut groups: ResMut<Groups>,
    mut sel: ResMut<Selection>,
    mut cam: Query<&mut RtsCamera>,
) {
    groups.forget(&sim);
    let Some(group) = GROUP_KEYS.iter().position(|&k| keys.just_pressed(k)) else { return };
    let mods = Mods::of(&keys);
    let now = time.elapsed_secs_f64();
    // A third quick tap counts too: OpenRA's `MultiTapCount >= 2`.
    let double = groups.last_tap.is_some_and(|(g, m, at)| g == group && m == mods && now - at < DOUBLE_TAP);
    groups.last_tap = Some((group, mods, now));
    let mut centre = false;
    match (mods.ctrl, mods.shift, mods.alt) {
        (true, true, _) => groups.add_to(group, &sel.0),
        (true, false, _) => groups.create(group, &sel.0),
        (false, true, _) => {
            for &id in &groups.groups[group] {
                if !sel.0.contains(&id) {
                    sel.0.push(id);
                }
            }
        }
        (false, false, true) => centre = true,
        (false, false, false) => {
            // OpenRA's `Selection.Combine` with an empty group leaves the selection empty.
            sel.0 = groups.groups[group].clone();
            centre = double;
        }
    }
    if centre && let Ok(mut cam) = cam.single_mut() {
        centre_on(&sim, &mut cam, &groups.groups[group]);
    }
}

/// OpenRA's `Viewport.Center(actors)`: the camera's focus to the average of their positions.
pub(crate) fn centre_on(sim: &Sim, cam: &mut RtsCamera, ids: &[EntityId]) {
    let at: Vec<Vec3> = ids.iter().filter_map(|&id| sim.world.entity(id)).map(|e| sim.to_world(e.pos)).collect();
    if at.is_empty() {
        return;
    }
    let mid = at.iter().sum::<Vec3>() / at.len() as f32;
    cam.pivot = Vec3::new(mid.x, cam.pivot.y, mid.z);
}

/// Own entities that `keep` accepts and whose centre is in the camera's view, in entity order.
/// Units inside something and support jets (which can't be selected) are left out.
pub(crate) fn on_screen(
    sim: &Sim,
    cam: (&Camera, &GlobalTransform),
    keep: impl Fn(&g2_sim::Entity) -> bool,
) -> Vec<EntityId> {
    let Some(view) = cam.0.logical_viewport_size() else { return Vec::new() };
    own(sim, keep)
        .into_iter()
        .filter(|&id| {
            let e = sim.world.entity(id).unwrap();
            cam.0
                .world_to_viewport(cam.1, sim.to_world(e.pos))
                .is_ok_and(|p| p.cmpge(Vec2::ZERO).all() && p.cmple(view).all())
        })
        .collect()
}

/// Every own entity that `keep` accepts, the selectable ones only, in entity order.
fn own(sim: &Sim, keep: impl Fn(&g2_sim::Entity) -> bool) -> Vec<EntityId> {
    let rules = sim.world.rules;
    sim.world
        .entities()
        .filter(|e| e.owner == Some(sim.local) && e.inside.is_none() && !rules.def(e.def).is_support_jet() && keep(e))
        .map(|e| e.id)
        .collect()
}

/// Z and X: OpenRA's `SelectAllUnits` and `SelectUnitsByType`.
pub(crate) fn select_keys(
    keys: Res<ButtonInput<KeyCode>>,
    sim: Res<Sim>,
    mut sel: ResMut<Selection>,
    cam: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
) {
    let Ok((camera, cam_tf)) = cam.single() else { return };
    let rules = sim.world.rules;
    let unit = |e: &g2_sim::Entity| rules.def(e.def).is_unit();
    if keys.just_pressed(SELECT_ALL_KEY) {
        // Units only: OpenRA picks the highest selection priority, and units outrank buildings.
        let screen = on_screen(&sim, (camera, cam_tf), unit);
        sel.0 = if screen.len() > sel.0.len() { screen } else { own(&sim, unit) };
    }
    if keys.just_pressed(SELECT_BY_TYPE_KEY) && !sel.0.is_empty() {
        let kinds: Vec<g2_sim::DefId> = sel.0.iter().filter_map(|&id| sim.world.entity(id)).map(|e| e.def).collect();
        let of_kind = |e: &g2_sim::Entity| kinds.contains(&e.def);
        let screen = on_screen(&sim, (camera, cam_tf), of_kind);
        let pick = if screen.len() > sel.0.len() { screen } else { own(&sim, of_kind) };
        for id in pick {
            if !sel.0.contains(&id) {
                sel.0.push(id);
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use bevy::time::TimeUpdateStrategy;
    use bevy::window::PrimaryWindow;

    /// A 1280 × 720 window and the game's camera over the local base, with the selection and
    /// group systems running frame by frame (16 ms each).
    pub(crate) struct Rig {
        pub app: App,
    }

    impl Rig {
        pub fn new(sim: Sim) -> Rig {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins)
                .insert_resource(TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(16)))
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<Selection>()
                .init_resource::<Groups>()
                .add_systems(Update, (group_keys, select_keys).chain());
            let home = sim.world.entities().find(|e| e.owner == Some(sim.local)).map(|e| sim.to_world(e.pos)).unwrap();
            app.insert_resource(sim);
            let mut window = Window::default();
            window.resolution.set(1280.0, 720.0);
            app.world_mut().spawn((window, PrimaryWindow));
            spawn_camera(app.world_mut(), home);
            app.update();
            Rig { app }
        }

        /// One frame with `keys` held down (pressed this frame).
        pub fn frame(&mut self, keys: &[KeyCode]) {
            let mut k = self.app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            k.clear();
            k.release_all();
            keys.iter().for_each(|&key| k.press(key));
            self.app.update();
        }

        pub fn idle(&mut self, frames: usize) {
            for _ in 0..frames {
                self.frame(&[]);
            }
        }

        pub fn sel(&self) -> Vec<EntityId> {
            self.app.world().resource::<Selection>().0.clone()
        }

        pub fn set_sel(&mut self, ids: &[EntityId]) {
            self.app.world_mut().resource_mut::<Selection>().0 = ids.to_vec();
        }

        pub fn pivot(&mut self) -> Vec3 {
            self.app.world_mut().query::<&RtsCamera>().single(self.app.world()).unwrap().pivot
        }

        pub fn set_pivot(&mut self, p: Vec3) {
            let mut q = self.app.world_mut().query::<(&mut RtsCamera, &mut GlobalTransform)>();
            let (mut cam, mut tf) = q.single_mut(self.app.world_mut()).unwrap();
            cam.pivot = p;
            *tf = GlobalTransform::from(cam.transform());
        }

        pub fn sim(&mut self) -> Mut<'_, Sim> {
            self.app.world_mut().resource_mut::<Sim>()
        }
    }

    /// The in-game camera at `pivot` with its projection worked out for a 1280 × 720 target,
    /// as the render world would leave it.
    pub(crate) fn spawn_camera(world: &mut World, pivot: Vec3) {
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
    }

    /// A match whose local player has, besides the start, a few units near its base, given
    /// by stepping the sim with training orders.
    pub(crate) fn sim_with_units() -> (Sim, Vec<EntityId>) {
        use g2_rules::ids::*;
        let mut players = Sim::players([g2_sim::Faction::Gla, g2_sim::Faction::Eu], false);
        players[0].starting_credits = 100_000;
        let mut sim = Sim::new(g2_rules::crater_ridge(), None, 1, players);
        let me = sim.local;
        let worker = sim.world.entities().find(|e| e.owner == Some(me) && e.def == GLA_WORKER).unwrap().id;
        let cc = sim.world.entities().find(|e| e.owner == Some(me) && e.def == GLA_CC).unwrap().pos;
        let at = cc + g2_sim::Vec2::from_int(30, if cc.y.to_f32() > 100.0 { -20 } else { 20 });
        sim.world.submit(me, g2_sim::CommandKind::Build { builder: worker, def: GLA_BARRACKS, pos: at });
        let barracks = loop {
            sim.world.step();
            if let Some(b) = sim.world.entities().find(|e| e.def == GLA_BARRACKS && e.is_complete()) {
                break b.id;
            }
        };
        for _ in 0..3 {
            sim.world.submit(me, g2_sim::CommandKind::Train { producer: barracks, def: GLA_RABBLE });
        }
        while sim.world.entities().filter(|e| e.def == GLA_RABBLE).count() < 3 {
            sim.world.step();
        }
        let squads = sim.world.entities().filter(|e| e.def == GLA_RABBLE).map(|e| e.id).collect();
        (sim, squads)
    }

    /// OpenRA's control groups through the real system: Ctrl + 1 makes group 1, 1 selects it,
    /// Shift + 2 adds group 2 to the selection, Ctrl + Shift + 2 moves the selection into
    /// group 2, a unit is in one group at most, 1 twice within 250 ms centres the camera on the
    /// group (twice slower does not), Alt + 1 centres without selecting, an empty selection
    /// doesn't wipe a group, and the units of a defeated player drop out.
    #[test]
    fn control_groups_as_openra() {
        let (sim, squads) = sim_with_units();
        let worker = sim.world.entities().find(|e| e.owner == Some(sim.local) && e.def == g2_rules::ids::GLA_WORKER);
        let worker = worker.unwrap().id;
        let mut rig = Rig::new(sim);
        let (ctrl, shift, alt) = (KeyCode::ControlLeft, KeyCode::ShiftLeft, KeyCode::AltLeft);
        let ctrl = if cfg!(target_os = "macos") { KeyCode::SuperLeft } else { ctrl };
        let (one, two) = (KeyCode::Digit1, KeyCode::Digit2);

        rig.set_sel(&squads[..2]);
        rig.frame(&[ctrl, one]);
        rig.set_sel(&[]);
        rig.frame(&[ctrl, one]);
        rig.idle(30);
        rig.frame(&[one]);
        assert_eq!(rig.sel(), squads[..2], "1 selects the group; an empty Ctrl + 1 left it alone");

        rig.set_sel(&[worker]);
        rig.frame(&[ctrl, two]);
        rig.set_sel(&squads[..1]);
        rig.frame(&[shift, two]);
        assert_eq!(rig.sel(), vec![squads[0], worker], "Shift + 2 adds group 2 to the selection");

        rig.set_sel(&[squads[1], squads[2]]);
        rig.frame(&[ctrl, shift, two]);
        rig.idle(30);
        rig.frame(&[two]);
        assert_eq!(rig.sel(), vec![worker, squads[1], squads[2]], "Ctrl + Shift + 2 adds the selection to group 2");
        rig.idle(30);
        rig.frame(&[one]);
        assert_eq!(rig.sel(), vec![squads[0]], "a unit moved to group 2 left group 1");

        let far = Vec3::new(5.0, 0.0, 5.0);
        rig.set_pivot(far);
        rig.idle(30);
        rig.frame(&[one]);
        rig.idle(20);
        rig.frame(&[one]);
        assert_eq!(rig.pivot(), far, "two presses 336 ms apart are not a double tap");
        rig.idle(30);
        rig.frame(&[one]);
        rig.idle(5);
        rig.frame(&[one]);
        let sim = rig.app.world().resource::<Sim>();
        let want = sim.to_world(sim.world.entity(squads[0]).unwrap().pos);
        let at = rig.pivot();
        assert!(
            Vec2::new(at.x - want.x, at.z - want.z).length() < 0.01,
            "double tap centres on the group: {at} vs {want}"
        );

        rig.set_pivot(far);
        rig.set_sel(&[]);
        rig.idle(30);
        rig.frame(&[alt, two]);
        assert!(rig.pivot().distance(far) > 10.0 && rig.sel().is_empty(), "Alt + 2 jumps there, selects nothing");

        let me = rig.app.world().resource::<Sim>().local;
        rig.sim().world.submit(me, g2_sim::CommandKind::Surrender);
        for _ in 0..=g2_sim::world::INPUT_DELAY {
            rig.sim().world.step();
        }
        rig.idle(1);
        let groups = rig.app.world().resource::<Groups>();
        assert!(groups.groups.iter().all(Vec::is_empty), "a defeated player's units left every group");
    }

    /// Z selects every own unit on screen, then, pressed again with no more on screen, every
    /// own unit on the map; X adds the selected kinds, on screen first. Buildings are left out
    /// of Z, as OpenRA's selection priority leaves them out.
    #[test]
    fn select_all_and_by_type() {
        let (sim, squads) = sim_with_units();
        let me = sim.local;
        let units: Vec<EntityId> = sim
            .world
            .entities()
            .filter(|e| e.owner == Some(me) && sim.world.rules.def(e.def).is_unit())
            .map(|e| e.id)
            .collect();
        let mut rig = Rig::new(sim);
        rig.frame(&[SELECT_ALL_KEY]);
        assert_eq!(rig.sel(), units, "every own unit is near the base, so on screen");

        // With the camera far away, Z finds nothing on screen and takes the whole map.
        rig.set_pivot(Vec3::new(5.0, 0.0, 5.0));
        rig.set_sel(&[]);
        rig.frame(&[SELECT_ALL_KEY]);
        assert_eq!(rig.sel(), units, "nothing on screen: every own unit on the map");

        rig.set_sel(&squads[..1]);
        rig.frame(&[SELECT_BY_TYPE_KEY]);
        assert_eq!(rig.sel(), squads, "X: every squad of the selected kind, none of the others");
    }
}
