//! Camera, light and grade for the target mood (docs/STYLE.md): steep narrow-FOV camera, strong
//! warm afternoon sun with crisp shadows to the lower right, warm ambient, warm grade, distance
//! haze and a heavy vignette.

use bevy::camera::{Exposure, PerspectiveProjection};
use bevy::light::{CascadeShadowConfigBuilder, GlobalAmbientLight};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::post_process::effect_stack::Vignette;
use bevy::prelude::*;
use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection, Tonemapping};

/// Narrow FOV, so buildings read almost isometric.
pub const FOV_DEGREES: f32 = 32.0;
/// Camera pitch below the horizon (radians); about 57°.
pub const PITCH: f32 = 1.0;
/// Fixed default yaw: the sun sits at the upper left of the screen.
pub const YAW: f32 = 0.0;

pub fn camera_bundle() -> impl Bundle {
    (
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection { fov: FOV_DEGREES.to_radians(), ..default() }),
        // Physical units: daylight exposure with a daylight-strength sun (see `spawn_lights`).
        Exposure::SUNLIGHT,
        Tonemapping::AgX,
        ColorGrading {
            global: ColorGradingGlobal { exposure: 0.7, temperature: 0.25, post_saturation: 1.18, ..default() },
            shadows: ColorGradingSection { contrast: 1.15, saturation: 1.1, gamma: 0.95, ..default() },
            midtones: ColorGradingSection { contrast: 1.12, saturation: 1.12, ..default() },
            highlights: ColorGradingSection { contrast: 1.08, ..default() },
        },
        DistanceFog {
            color: Color::srgb(0.32, 0.27, 0.18),
            directional_light_color: Color::srgba(1.0, 0.85, 0.6, 0.25),
            directional_light_exponent: 20.0,
            falloff: FogFalloff::Linear { start: 330.0, end: 900.0 },
        },
        Vignette { intensity: 1.3, radius: 0.68, smoothness: 2.2, roundness: 1.4, ..default() },
    )
}

pub fn spawn_lights(commands: &mut Commands, map_size: f32) {
    commands.spawn((
        crate::menu::MatchEntity,
        DirectionalLight {
            color: Color::srgb(1.0, 0.93, 0.80),
            illuminance: 110_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        // Afternoon sun from the upper left of the default view (camera looks toward -Z, screen
        // right is +X): light travels toward +X/+Z, so shadows fall to the lower right.
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, -2.35, -0.85, 0.0)),
        CascadeShadowConfigBuilder {
            num_cascades: 3,
            minimum_distance: 1.0,
            maximum_distance: map_size * 1.2,
            first_cascade_far_bound: 140.0,
            overlap_proportion: 0.2,
        }
        .build(),
    ));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.95, 0.85, 0.70),
        brightness: 9_000.0,
        ..default()
    });
    commands.insert_resource(ClearColor(Color::BLACK));
}
