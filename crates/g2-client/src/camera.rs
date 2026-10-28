//! Classic RTS camera: a pivot on the ground, viewed from a fixed pitch, driven the way
//! OpenRA's viewport is (`Viewport.cs`, `ViewportControllerWidget.cs`) under the player's
//! settings (`settings.rs`). WASD/arrows or the screen edge pan; so does dragging with the pan
//! button, per Pan Behaviour. The wheel zooms toward the cursor (with the zoom modifier held),
//! `]` and `[` zoom in and out, `.` toggles between the furthest and the closest view; Q/E
//! rotate.

use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::settings::{Pan, Settings};
use crate::sim::Sim;

#[derive(Component)]
pub struct RtsCamera {
    pub pivot: Vec3,
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
}

impl RtsCamera {
    pub fn transform(&self) -> Transform {
        let offset = Quat::from_euler(EulerRot::YXZ, self.yaw, -self.pitch, 0.0) * Vec3::new(0.0, 0.0, self.distance);
        Transform::from_translation(self.pivot + offset).looking_at(self.pivot, Vec3::Y)
    }

    /// Where the ray through window point `cursor` (logical pixels, in a window of `size`) meets
    /// the pivot's level: the ground around the focus, without marching the terrain.
    fn on_pivot_plane(&self, cursor: Vec2, size: Vec2) -> Option<Vec3> {
        let tf = self.transform();
        let half = (crate::look::FOV_DEGREES.to_radians() / 2.0).tan();
        let ndc = Vec2::new(2.0 * cursor.x / size.x - 1.0, 1.0 - 2.0 * cursor.y / size.y);
        let dir = tf.rotation * Vec3::new(ndc.x * half * size.x / size.y, ndc.y * half, -1.0);
        (dir.y < -1e-4).then(|| tf.translation + dir * ((self.pivot.y - tf.translation.y) / dir.y))
    }

    /// World units per logical pixel at the pivot, in a window `height` pixels tall.
    fn units_per_pixel(&self, height: f32) -> f32 {
        2.0 * self.distance * (crate::look::FOV_DEGREES.to_radians() / 2.0).tan() / height
    }

    /// Moves the camera to `distance`, keeping the ground under `cursor` (if given) in place,
    /// as OpenRA's `AdjustZoom(dz, center)` does.
    fn zoom_to(&mut self, distance: f32, cursor: Option<(Vec2, Vec2)>) {
        let before = cursor.and_then(|(c, size)| self.on_pivot_plane(c, size));
        self.distance = distance;
        let after = cursor.and_then(|(c, size)| self.on_pivot_plane(c, size));
        if let (Some(a), Some(b)) = (before, after) {
            self.pivot += Vec3::new(a.x - b.x, 0.0, a.z - b.z);
        }
    }
}

/// Keyboard and screen-edge panning in camera distances per second at the default Pan Speed
/// (30), so a pan covers the same share of the view at every zoom.
const PAN_SPEED: f32 = 1.2;
const EDGE: f32 = 8.0;
/// A drag with the pan button shorter than this (logical pixels) is a click (OpenRA's
/// `MouseScrollDeadzone`).
pub const PAN_DEADZONE: f32 = 8.0;
/// Joystick panning moves the view by 0.01 × Pan Speed of the drag's length every frame in
/// OpenRA; at 60 frames a second that is 0.6 × Pan Speed of it a second.
const JOYSTICK_RATE: f32 = 0.6;
/// `]` and `[` zoom by this much (OpenRA's zoom keys: `AdjustZoom(±0.25)`).
const KEY_ZOOM: f32 = 0.25;
/// Pixel-unit scrolling (touchpads) counts as one wheel notch per this many pixels.
const PIXELS_PER_NOTCH: f32 = 100.0;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PanDrag>()
            .add_systems(
                Update,
                (control, fit_view).chain().run_if(crate::menu::playing).before(crate::input::InputSet),
            )
            .add_systems(Update, lock_cursor.run_if(resource_exists::<Settings>))
            .add_systems(Update, rehome.run_if(crate::menu::scene_start).after(crate::menu::MenuSet));
    }
}

/// The Close battlefield camera's default framing: a command center spans about a fifth of
/// the screen width.
pub const DEFAULT_DISTANCE: f32 = 105.0;

/// The pan button's drag: where it went down, where the cursor was last frame, whether it
/// has moved past the deadzone, and whether a pan ended this frame. `input` leaves a press
/// that panned alone, so dragging with the right button never also orders or deselects.
#[derive(Resource, Default)]
pub struct PanDrag {
    start: Option<Vec2>,
    last: Vec2,
    panning: bool,
    /// The pan button went up this frame at the end of a pan.
    pub ended: bool,
}

pub fn spawn(commands: &mut Commands, pivot: Vec3, distance: f32) {
    let cam = RtsCamera { pivot, distance, yaw: crate::look::YAW, pitch: crate::look::PITCH };
    commands.spawn((crate::look::camera_bundle(), cam.transform(), cam));
}

/// Moves the camera over the local base when a match starts, at the battlefield camera's
/// default view; the camera itself outlives matches.
pub fn rehome(sim: Res<Sim>, settings: Option<Res<Settings>>, mut cam: Query<&mut RtsCamera>) {
    let Ok(mut cam) = cam.single_mut() else { return };
    let home = sim.world.entities().find(|e| e.owner == Some(sim.local)).map_or(Vec3::ZERO, |e| sim.to_world(e.pos));
    cam.pivot = home;
    cam.distance = settings.map_or(DEFAULT_DISTANCE, |s| s.camera.distance());
}

#[allow(clippy::too_many_arguments)]
fn control(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    window: Query<&Window, With<PrimaryWindow>>,
    time: Res<Time>,
    sim: Res<Sim>,
    settings: Res<Settings>,
    panels: crate::hud::Panels,
    mut drag: ResMut<PanDrag>,
    mut q: Query<(&mut RtsCamera, &mut Transform)>,
    mut preset: Local<Option<crate::settings::Battlefield>>,
) {
    let Ok((mut cam, mut tf)) = q.single_mut() else { return };
    let dt = time.delta_secs();
    let default = settings.camera.distance();
    let (near, far) = settings.zoom_range(sim.world.is_ai(sim.local));
    // A new battlefield camera starts at its default view, as OpenRA's does.
    if preset.is_some_and(|p| p != settings.camera) {
        cam.distance = default;
    }
    *preset = Some(settings.camera);
    let window = window.single().ok();
    let size = window.map(Window::size);
    let cursor = window.and_then(Window::cursor_position);

    let mut dir = Vec2::ZERO;
    if keys.any_pressed([KeyCode::KeyW, KeyCode::ArrowUp]) {
        dir.y -= 1.0;
    }
    if keys.any_pressed([KeyCode::KeyS, KeyCode::ArrowDown]) {
        dir.y += 1.0;
    }
    if keys.any_pressed([KeyCode::KeyA, KeyCode::ArrowLeft]) {
        dir.x -= 1.0;
    }
    if keys.any_pressed([KeyCode::KeyD, KeyCode::ArrowRight]) {
        dir.x += 1.0;
    }
    // The screen edge pans only while the window has focus and no drag is panning.
    if settings.edge_pan
        && drag.start.is_none()
        && let Some(w) = window.filter(|w| w.focused)
        && let Some(c) = cursor
    {
        if c.x < EDGE {
            dir.x -= 1.0;
        } else if c.x > w.width() - EDGE {
            dir.x += 1.0;
        }
        if c.y < EDGE {
            dir.y -= 1.0;
        } else if c.y > w.height() - EDGE {
            dir.y += 1.0;
        }
    }
    if keys.pressed(KeyCode::KeyQ) {
        cam.yaw += 1.5 * dt;
    }
    if keys.pressed(KeyCode::KeyE) {
        cam.yaw -= 1.5 * dt;
    }
    let pace = settings.pan_speed / 30.0;
    let rot = Quat::from_rotation_y(cam.yaw);
    if dir != Vec2::ZERO {
        let step = dir.normalize() * PAN_SPEED * pace * cam.distance * dt;
        cam.pivot += rot * Vec3::new(step.x, 0.0, step.y);
    }

    // Dragging with the pan button. A press on the HUD is the HUD's.
    drag.ended = false;
    let button = settings.pan_button();
    if settings.pan != Pan::Disabled
        && buttons.just_pressed(button)
        && let Some(c) = cursor.filter(|&c| !crate::hud::over_hud(&panels, c))
    {
        *drag = PanDrag { start: Some(c), last: c, panning: false, ended: false };
    }
    if let (Some(start), Some(c), Some(size)) = (drag.start, cursor, size) {
        drag.panning |= start.distance(c) > PAN_DEADZONE;
        if drag.panning {
            match settings.pan {
                Pan::Standard | Pan::Inverted => {
                    if let (Some(a), Some(b)) = (cam.on_pivot_plane(drag.last, size), cam.on_pivot_plane(c, size)) {
                        // Standard: the ground under the cursor follows it, as if grabbed.
                        let d = Vec3::new(a.x - b.x, 0.0, a.z - b.z);
                        cam.pivot += if settings.pan == Pan::Inverted { -d } else { d };
                    }
                }
                Pan::Joystick => {
                    let step = (c - start) * JOYSTICK_RATE * settings.pan_speed * dt * cam.units_per_pixel(size.y);
                    cam.pivot += rot * Vec3::new(step.x, 0.0, step.y);
                }
                Pan::Disabled => {}
            }
        }
        drag.last = c;
    }
    if drag.start.is_some() && !buttons.pressed(button) {
        drag.ended = drag.panning;
        drag.start = None;
        drag.panning = false;
    }

    // Zoom: exponential, so equal steps in and out cancel (OpenRA's `AdjustZoom`).
    if settings.zoom_modifier.held(&keys) && scroll.delta.y != 0.0 {
        let notches = match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y / PIXELS_PER_NOTCH,
        };
        let to = (cam.distance / (notches * settings.zoom_speed).exp()).clamp(near, far);
        cam.zoom_to(to, cursor.zip(size));
    }
    for (key, dz) in [(KeyCode::BracketRight, KEY_ZOOM), (KeyCode::BracketLeft, -KEY_ZOOM)] {
        if keys.just_pressed(key) {
            let to = (cam.distance / dz.exp()).clamp(near, far);
            cam.zoom_to(to, None);
        }
    }
    // `.`: OpenRA's ToggleZoom. Zoomed in (or a spectator zoomed out): back to the default
    // view; at the default view: the closest one.
    if keys.just_pressed(KeyCode::Period) {
        let at_default = (cam.distance - default).abs() < 0.01;
        cam.distance = if at_default { near } else { default };
    }
    cam.distance = cam.distance.clamp(near, far);

    let map = &sim.world.map;
    cam.pivot.x = cam.pivot.x.clamp(0.0, map.width.to_f32());
    cam.pivot.z = cam.pivot.z.clamp(0.0, map.height.to_f32());
    // Ride the terrain; eased so panning over a cliff edge doesn't jolt the view.
    let ground = sim.ground_y(cam.pivot.x, cam.pivot.z);
    cam.pivot.y += (ground - cam.pivot.y) * (1.0 - (-8.0 * dt).exp());
    *tf = cam.transform();
}

/// Keeps the haze and the sun's shadow range ahead of the camera as it zooms out
/// (`look::fit_fog`, `look::fit_cascades`). Headless shots run it too, so they match play.
pub fn fit_view(
    cam: Query<&RtsCamera>,
    mut fog: Query<&mut bevy::pbr::DistanceFog, With<RtsCamera>>,
    mut sun: Query<&mut bevy::light::CascadeShadowConfig>,
) {
    let Ok(cam) = cam.single() else { return };
    for mut fog in &mut fog {
        crate::look::fit_fog(cam.distance, &mut fog);
    }
    for mut cascades in &mut sun {
        crate::look::fit_cascades(cam.distance, &mut cascades);
    }
}

/// Lock Mouse to Window: the cursor is confined to the window while the setting is on (macOS
/// can't confine a cursor and leaves it free).
fn lock_cursor(settings: Res<Settings>, mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>) {
    let mode = if settings.lock_mouse { CursorGrabMode::Confined } else { CursorGrabMode::None };
    for mut c in &mut cursor {
        if c.grab_mode != mode {
            c.grab_mode = mode;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> RtsCamera {
        RtsCamera { pivot: Vec3::new(100.0, 0.0, 100.0), distance: 105.0, yaw: 0.3, pitch: crate::look::PITCH }
    }

    /// The screen centre looks at the pivot, and zooming toward a point off-centre keeps the
    /// ground under that point where it was.
    #[test]
    fn zooming_toward_the_cursor_keeps_its_ground() {
        let size = Vec2::new(1280.0, 720.0);
        let c = cam();
        let centre = c.on_pivot_plane(size / 2.0, size).unwrap();
        assert!(centre.distance(c.pivot) < 0.01, "centre ray hits {centre}");
        let mut c = cam();
        let at = Vec2::new(1000.0, 200.0);
        let before = c.on_pivot_plane(at, size).unwrap();
        c.zoom_to(60.0, Some((at, size)));
        let after = c.on_pivot_plane(at, size).unwrap();
        assert!(before.distance(after) < 0.01, "{before} moved to {after}");
        assert!(c.pivot.distance(cam().pivot) > 1.0, "the pivot moved toward the cursor");
    }

    /// Drives `control` frame by frame on a 1280 × 720 window over Crater Ridge.
    struct Rig {
        app: App,
    }

    impl Rig {
        fn new(settings: Settings, spectate: bool) -> Rig {
            use bevy::time::TimeUpdateStrategy;
            let mut app = App::new();
            app.add_plugins(MinimalPlugins)
                .insert_resource(TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(16)))
                .insert_resource(Sim::new(
                    g2_rules::crater_ridge(),
                    None,
                    1,
                    Sim::players([g2_sim::Faction::Eu, g2_sim::Faction::Gla], spectate),
                ))
                .insert_resource(settings)
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<ButtonInput<MouseButton>>()
                .init_resource::<AccumulatedMouseScroll>()
                .init_resource::<PanDrag>()
                .add_systems(Update, control);
            let mut window = Window { focused: true, ..default() };
            window.resolution.set(1280.0, 720.0);
            window.set_cursor_position(Some(Vec2::new(640.0, 360.0)));
            app.world_mut().spawn((window, PrimaryWindow));
            let distance = app.world().resource::<Settings>().camera.distance();
            app.world_mut().spawn((
                RtsCamera { pivot: Vec3::new(200.0, 0.0, 200.0), distance, yaw: 0.0, pitch: crate::look::PITCH },
                Transform::default(),
            ));
            app.update();
            Rig { app }
        }

        fn cam(&mut self) -> (Vec3, f32) {
            let c = self.app.world_mut().query::<&RtsCamera>().single(self.app.world()).unwrap();
            (c.pivot, c.distance)
        }

        fn cursor(&mut self, at: Vec2) {
            let mut q = self.app.world_mut().query::<&mut Window>();
            q.single_mut(self.app.world_mut()).unwrap().set_cursor_position(Some(at));
        }

        /// One frame; presses and wheel notches last only for it.
        fn frame(&mut self, press: &[MouseButton], release: &[MouseButton], keys: &[KeyCode], notches: f32) {
            let w = self.app.world_mut();
            let mut b = w.resource_mut::<ButtonInput<MouseButton>>();
            b.clear();
            press.iter().for_each(|&p| b.press(p));
            release.iter().for_each(|&r| b.release(r));
            let mut k = w.resource_mut::<ButtonInput<KeyCode>>();
            k.clear();
            k.release_all();
            keys.iter().for_each(|&p| k.press(p));
            *w.resource_mut::<AccumulatedMouseScroll>() =
                AccumulatedMouseScroll { unit: MouseScrollUnit::Line, delta: Vec2::new(0.0, notches) };
            self.app.update();
        }

        /// Drags the pan button from the centre `by` logical pixels over `n` frames, then lets go.
        fn drag(&mut self, by: Vec2, n: usize) -> bool {
            let button = self.app.world().resource::<Settings>().pan_button();
            self.cursor(Vec2::new(640.0, 360.0));
            self.frame(&[button], &[], &[], 0.0);
            for k in 1..=n {
                self.cursor(Vec2::new(640.0, 360.0) + by * k as f32 / n as f32);
                self.frame(&[], &[], &[], 0.0);
            }
            self.frame(&[], &[button], &[], 0.0);
            self.app.world().resource::<PanDrag>().ended
        }
    }

    /// OpenRA's pan behaviours, through the real system: Standard drags the battlefield with
    /// the cursor, Inverted moves the view the other way, Joystick keeps the view running
    /// toward the cursor while held, Disabled does nothing; a drag inside the deadzone is a
    /// click and pans nothing.
    #[test]
    fn pan_behaviours_move_the_view_as_openra_does() {
        let moved = |pan: Pan, by: Vec2| {
            let mut rig = Rig::new(Settings { pan, edge_pan: false, ..Settings::default() }, false);
            let before = rig.cam().0;
            let ended = rig.drag(by, 10);
            (rig.cam().0 - before, ended)
        };
        let right = Vec2::new(200.0, 0.0);
        let (standard, ended) = moved(Pan::Standard, right);
        assert!(standard.x < -10.0 && ended, "grabbed ground follows the cursor right: view moves left, {standard}");
        let (inverted, _) = moved(Pan::Inverted, right);
        assert!((inverted + standard).length() < 0.5, "inverted mirrors standard: {inverted} vs {standard}");
        let (joystick, _) = moved(Pan::Joystick, right);
        assert!(joystick.x > 1.0, "joystick runs toward the cursor: {joystick}");
        let (none, ended) = moved(Pan::Disabled, right);
        assert_eq!((none, ended), (Vec3::ZERO, false));
        let (click, ended) = moved(Pan::Standard, Vec2::new(PAN_DEADZONE - 2.0, 0.0));
        assert_eq!((click, ended), (Vec3::ZERO, false), "inside the deadzone a press is a click");
    }

    /// The default view is the furthest a player can zoom out and the wheel zooms in to half of
    /// it, at the zoom speed; `]`/`[` step by OpenRA's 0.25 and `.` toggles the default and the
    /// closest view. A spectator may zoom out to twice the default. A held zoom modifier is
    /// needed when one is set. Medium and Far start further out.
    #[test]
    fn zoom_stays_in_openras_range() {
        let mut rig = Rig::new(Settings::default(), false);
        let default = rig.cam().1;
        rig.frame(&[], &[], &[], -5.0);
        assert_eq!(rig.cam().1, default, "no zooming out past the default view");
        rig.frame(&[], &[], &[], 1.0);
        assert!((rig.cam().1 - default / 0.04f32.exp()).abs() < 1e-3, "one notch at 0.04: {}", rig.cam().1);
        rig.frame(&[], &[], &[], 100.0);
        assert_eq!(rig.cam().1, default / 2.0, "closest is half the default");
        rig.frame(&[], &[], &[KeyCode::Period], 0.0);
        assert_eq!(rig.cam().1, default, "toggle back to the default view");
        rig.frame(&[], &[], &[KeyCode::Period], 0.0);
        assert_eq!(rig.cam().1, default / 2.0, "toggle to the closest view");
        rig.frame(&[], &[], &[KeyCode::BracketLeft], 0.0);
        assert!((rig.cam().1 - default / 2.0 * 0.25f32.exp()).abs() < 1e-3, "[ zooms out by 0.25");

        let mut spectator = Rig::new(Settings::default(), true);
        spectator.frame(&[], &[], &[], -100.0);
        assert_eq!(spectator.cam().1, default * 2.0, "a spectator zooms out to twice the default");

        let mut ctrl =
            Rig::new(Settings { zoom_modifier: crate::settings::Modifier::Ctrl, ..Settings::default() }, false);
        ctrl.frame(&[], &[], &[], 3.0);
        assert_eq!(ctrl.cam().1, default, "the wheel alone doesn't zoom with a modifier set");
        ctrl.frame(&[], &[], &[KeyCode::ControlLeft], 3.0);
        assert!(ctrl.cam().1 < default, "it does with the modifier held");

        for (camera, k) in [(crate::settings::Battlefield::Medium, 1.4), (crate::settings::Battlefield::Far, 2.0)] {
            let mut rig = Rig::new(Settings { camera, ..Settings::default() }, false);
            assert_eq!(rig.cam().1, default * k);
        }
    }
}
