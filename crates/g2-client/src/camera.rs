//! Classic RTS camera: a pivot on the ground, viewed from a fixed pitch.
//! WASD/arrows or screen edges pan, mouse wheel zooms, Q/E rotate.

use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

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
}

const PAN_SPEED: f32 = 1.2;
const EDGE: f32 = 8.0;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, control);
    }
}

pub fn spawn(commands: &mut Commands, pivot: Vec3) {
    let cam = RtsCamera { pivot, distance: 160.0, yaw: 0.0, pitch: 0.95 };
    commands.spawn((Camera3d::default(), cam.transform(), cam));
}

fn control(
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    window: Query<&Window, With<PrimaryWindow>>,
    time: Res<Time>,
    sim: Res<Sim>,
    mut q: Query<(&mut RtsCamera, &mut Transform)>,
) {
    let Ok((mut cam, mut tf)) = q.single_mut() else { return };
    let dt = time.delta_secs();
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
    if let Ok(w) = window.single()
        && let Some(c) = w.cursor_position()
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
    cam.distance = (cam.distance * (1.0 - scroll.delta.y * 0.1)).clamp(40.0, 400.0);
    if dir != Vec2::ZERO {
        let step = dir.normalize() * PAN_SPEED * cam.distance * dt;
        let rot = Quat::from_rotation_y(cam.yaw);
        cam.pivot += rot * Vec3::new(step.x, 0.0, step.y);
    }
    let map = &sim.world.map;
    cam.pivot.x = cam.pivot.x.clamp(0.0, map.width.to_f32());
    cam.pivot.z = cam.pivot.z.clamp(0.0, map.height.to_f32());
    *tf = cam.transform();
}
