//! Camera, light and grade for the target mood (docs/STYLE.md): steep narrow-FOV camera, strong
//! warm afternoon sun with crisp shadows to the lower right, warm ambient, warm grade, distance
//! haze and a heavy vignette.

use bevy::camera::{Exposure, PerspectiveProjection, ShadowLodOrigin};
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
        // The 2013 footage is contrasty and warm: deep brown shadows, bright dusty highlights.
        // Its lit centre measures about 140-150 sRGB luminance; exposure is set to match.
        ColorGrading {
            global: ColorGradingGlobal { exposure: 1.15, temperature: 0.3, post_saturation: 1.22, ..default() },
            shadows: ColorGradingSection { contrast: 1.3, saturation: 1.15, gamma: 0.9, ..default() },
            midtones: ColorGradingSection { contrast: 1.18, saturation: 1.15, ..default() },
            highlights: ColorGradingSection { contrast: 1.1, ..default() },
        },
        DistanceFog {
            color: Color::srgb(0.32, 0.27, 0.18),
            directional_light_color: Color::srgba(1.0, 0.85, 0.6, 0.25),
            directional_light_exponent: 20.0,
            falloff: FogFalloff::Linear { start: 330.0, end: 900.0 },
        },
        // The footage darkens the top corners hard but keeps the left and right middle lit.
        Vignette { intensity: 1.15, radius: 0.74, smoothness: 2.4, roundness: 1.2, ..default() },
        // Contact shadows where buildings and units meet the ground, and in the creases of the
        // models: what makes the footage's bases look heavy and grounded. Needs MSAA off; FXAA
        // takes over the edges (SMAA would need the `smaa_luts` feature's lookup tables, which
        // our feature list leaves out, and runs on 1×1 placeholders without them).
        bevy::pbr::ScreenSpaceAmbientOcclusion { radius: 2.0, constant_object_thickness: 1.0, ..default() },
        Msaa::Off,
        bevy::anti_alias::fxaa::Fxaa::default(),
        // Effect flares are point lights; headless renders go to an image, not a window, so
        // Bevy can't pick this camera for their shadow LOD by itself.
        ShadowLodOrigin,
    )
}

pub fn spawn_lights(commands: &mut Commands) {
    commands.spawn((
        crate::menu::MatchEntity,
        DirectionalLight {
            color: Color::srgb(1.0, 0.93, 0.80),
            illuminance: 110_000.0,
            shadow_maps_enabled: true,
            // The nearly flat ground under this sun shadows itself at the engine defaults,
            // which reads as contour rings in the distance and as brown-black sand up close.
            // These biases are large enough to stop it; watch that shadows still meet their
            // casters rather than lifting off the bases.
            shadow_depth_bias: 0.25,
            shadow_normal_bias: 9.0,
            ..default()
        },
        // Afternoon sun from the upper left of the default view (camera looks toward -Z, screen
        // right is +X): light travels toward +X/+Z, so shadows fall to the lower right.
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, -2.35, -0.85, 0.0)),
        CascadeShadowConfigBuilder {
            // Fine splits near the camera, where coarse shadow texels band the ground into
            // contour rings, over a short fixed range: everything the camera can see is
            // inside 300 units, and tighter cascades buy texel density over map_size.
            num_cascades: 4,
            minimum_distance: 1.0,
            maximum_distance: 300.0,
            first_cascade_far_bound: 60.0,
            overlap_proportion: 0.15,
        }
        .build(),
    ));
    // Twice the default texel density per cascade: the sun is the only shadow caster, so the
    // extra memory buys clean shadows across the whole map.
    commands.insert_resource(bevy::light::DirectionalLightShadowMap { size: 4096 });
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.95, 0.85, 0.70),
        brightness: 9_000.0,
        ..default()
    });
    commands.insert_resource(ClearColor(Color::BLACK));
}
