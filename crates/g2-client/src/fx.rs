//! Combat effects: muzzle flashes, tracers, rockets and shells in flight with smoke trails,
//! impacts, blasts and the fireballs of kills. Decoration only: nothing here feeds back into
//! the sim, and every random choice comes from a hash of the spot, so a replay draws the same.
//!
//! There is no bloom (the colour pipeline is tuned to the 2013 footage without HDR), so glow
//! comes from layering: a hot additive core inside a wider, fainter additive halo, plus a short
//! point light at big blasts that lights the ground and models around it.
//!
//! Each weapon is drawn by its `Look`, derived from the firing def's weapon (shell, range,
//! damage, rate, what it can hit) and, for the few weapons the numbers can't tell apart
//! (flames, toxins, lasers, railguns), from the def's name as the archived cards describe it.

use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use g2_sim::Vec2 as SimVec2;
use g2_sim::world::ShellSource;

use crate::sim::{Shot, Sim};

/// How a weapon is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// Rifles and machine guns: short fast streaks, a small flash, dust puffs where they land.
    Bullets { heavy: bool },
    /// A tank or cannon round: a bright slug, a big flash with a smoke puff, a spark burst.
    Cannon { heavy: bool },
    /// A rocket or missile fired straight: a glowing body with a smoke trail.
    Rocket,
    /// A flamethrower: a cone of fire licking toward the target.
    Flame,
    /// Toxin: a green stream, green bursts and clouds.
    Toxin,
    /// A beam weapon: a laser (`rail: false`) or a railgun's blue line.
    Beam { rail: bool },
    /// An artillery shell lobbed on an arc; `toxic` for GLA toxin shells, `napalm` for fire.
    Shell { toxic: bool, napalm: bool },
    /// A power's bombs and missiles: a long fall or flight and a big blast; `toxic` for the
    /// toxin and Scud strikes, `nuke` for the neutron bomb's white flash.
    Strike { toxic: bool, nuke: bool, missile: bool },
}

impl Look {
    /// The look of `e`'s weapon as it fires now (deployed or not).
    pub fn of_unit(world: &g2_sim::World, e: &g2_sim::Entity) -> Look {
        let deployed = e.deploy == g2_sim::entity::DeployState::Deployed;
        Look::of_def(world.rules.def(e.def), deployed)
    }

    pub fn of_def(d: &g2_sim::Def, deployed: bool) -> Look {
        let Some(w) = d.weapon_for(deployed) else { return Look::Bullets { heavy: false } };
        let name = d.name;
        let has = |s: &str| name.contains(s);
        if w.shell.is_some() {
            return Look::Shell { toxic: has("Toxin"), napalm: has("Napalm") };
        }
        if has("Flame") || has("Inferno") || has("Dragon") {
            return Look::Flame;
        }
        if has("Toxin") || has("Chemist") {
            return Look::Toxin;
        }
        if has("Railgun") {
            return Look::Beam { rail: true };
        }
        if has("Laser") {
            return Look::Beam { rail: false };
        }
        // Rockets: anything that can fire at aircraft and hits hard per shot, or a rocket unit.
        if has("Rocket") || has("RPG") || has("Hunter") || has("Longbow") || has("Valkyrie") {
            return Look::Rocket;
        }
        // Machine guns fire often for little; guns fire seldom for a lot.
        let per_shot = w.damage;
        if per_shot >= 35 {
            Look::Cannon { heavy: per_shot >= 80 }
        } else {
            Look::Bullets { heavy: per_shot >= 20 || matches!(d.class, g2_sim::Class::Building { .. }) }
        }
    }

    /// A shell's look from what sent it.
    pub fn of_shell(world: &g2_sim::World, source: ShellSource) -> Look {
        match source {
            ShellSource::Gun { def, deployed } => Look::of_def(world.rules.def(def), deployed),
            ShellSource::Power { player, slot } => {
                let name = world.power_in(player, slot).map_or("", |p| p.name);
                let missile = matches!(
                    world.power_in(player, slot).map(|p| p.kind),
                    Some(g2_sim::defs::PowerKind::Missiles { .. })
                );
                Look::Strike {
                    toxic: name.contains("Toxin") || name.contains("Scud"),
                    nuke: name.contains("Neutron") || name.contains("Plasma"),
                    missile,
                }
            }
        }
    }
}

/// A short-lived visual: it moves, slows, falls, grows and fades over `life` seconds, then goes.
#[derive(Component, Clone, Copy)]
pub struct Particle {
    pub age: f32,
    pub life: f32,
    pub vel: Vec3,
    /// Fraction of velocity lost per second (air drag): 0 keeps speed.
    pub drag: f32,
    /// Downward acceleration (debris); negative rises (smoke).
    pub gravity: f32,
    /// Scale at birth and at death, interpolated with an ease-out.
    pub size: (Vec3, Vec3),
    /// Which material set it fades through (see `Palette`), and its stage: the material is
    /// swapped as the particle ages, so fire cools to smoke without per-particle materials.
    pub fade: Fade,
    /// Spins about its own up axis, radians a second (debris tumbling, smoke curling).
    pub spin: f32,
    /// Where debris comes to rest (the ground height under the blast).
    pub ground: f32,
}

/// The wind every haze drifts with once its launch speed has died down, so plumes lean one way
/// across the whole field as they do in the footage. Our own value, slow enough that a smoke
/// column stays a column. Blowing sand runs before it and flags fly from it.
pub const WIND: Vec3 = Vec3::new(0.9, 0.0, -0.35);

/// How hard the wind blows `t` seconds into the match, 0 (a lull) to 1 (a gust): two slow
/// waves out of step, so gusts come every half minute or so and never repeat exactly. Only
/// time goes in, so every flag and sand gust on the field rises and falls together.
pub fn gust(t: f32) -> f32 {
    (0.5 + 0.32 * (t * 0.23).sin() + 0.22 * (t * 0.61 + 1.3).sin()).clamp(0.0, 1.0)
}

/// Downward pull of the shells' drawn arcs, world units per second squared. The sim flies a
/// shell at its weapon's speed across the ground (`Shell::speed`), so a longer shot is longer
/// in the air and, under a constant pull, climbs higher and leaves the barrel steeper, as a
/// gun's does. Ours, picked so the Thunderfist's longest shot (65 away) leaves at 50 degrees
/// and peaks 20 high, about as high as the arcs drawn before (a third of the range).
pub const SHELL_GRAVITY: f32 = 60.0;

/// How high a shell `secs` in the air rises over the straight line between its ends.
pub fn shell_peak(secs: f32) -> f32 {
    SHELL_GRAVITY * secs * secs / 8.0
}

/// The slope (rise over run) a shell leaves the barrel at, flying `run` across the ground in
/// `secs` and landing `rise` above the muzzle: the start of `Flight::at`'s arc.
pub fn launch_slope(run: f32, secs: f32, rise: f32) -> f32 {
    (4.0 * shell_peak(secs) + rise) / run.max(1.0)
}

/// Heights above the ground a shell is drawn leaving the gun and landing at, and a direct
/// shot's tracer leaving the gun and striking; the guns lay along these lines (`procanim`).
pub const SHELL_MUZZLE: f32 = 2.5;
pub const SHELL_LAND: f32 = 0.5;
pub const TRACER_MUZZLE: f32 = 1.6;
pub const TRACER_HIT: f32 = 1.2;

impl Particle {
    /// Moves, slows, grows and turns the particle over `dt` seconds of its life (already counted
    /// into `age`). Debris lands on the ground it was thrown from and stays, shrinking away at
    /// the end.
    fn advance(&mut self, tf: &mut Transform, dt: f32) {
        let t = self.age / self.life;
        self.vel.y -= self.gravity * dt;
        // Drag pulls toward the wind for haze and toward rest for everything else.
        let keep = (1.0 - self.drag * dt).max(0.0);
        let wind = if self.fade.drifts() { WIND } else { Vec3::ZERO };
        self.vel = self.vel * keep + wind * (1.0 - keep);
        tf.translation += self.vel * dt;
        // Ease-out growth: fast at first, settling as it fades.
        let ease = 1.0 - (1.0 - t) * (1.0 - t);
        tf.scale = self.size.0.lerp(self.size.1, ease);
        // Debris tumbles; puffs roll in `face_camera` instead.
        if self.fade == Fade::Debris {
            if self.spin != 0.0 {
                tf.rotate_local_y(self.spin * dt);
            }
            if tf.translation.y < self.ground {
                tf.translation.y = self.ground;
                self.vel = Vec3::ZERO;
                self.spin = 0.0;
            }
            tf.scale = self.size.0 * (1.0 - ((t - 0.7) / 0.3).clamp(0.0, 1.0));
        }
    }
}

/// A fading material track: the particle uses `ramp[i]` while it is in the i-th equal part of
/// its life.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fade {
    /// Additive fire: white-hot, yellow, orange, deep red, then gone.
    Fire,
    /// Additive burning fuel (flamethrowers, napalm): orange from the start, so many
    /// overlapping puffs add up to fire rather than to white.
    Flame,
    /// Additive green toxin glow.
    Toxic,
    /// Additive blue-white (railgun, nuke).
    Plasma,
    /// Additive muzzle/tracer yellow, brief.
    Spark,
    /// Alpha-blended smoke: dark grey thinning out.
    Smoke,
    /// Alpha-blended pale dust thrown up by impacts.
    Dust,
    /// Alpha-blended green toxin haze.
    Fumes,
    /// Opaque dark debris: never fades, falls under gravity.
    Debris,
    /// Alpha-blended thick black smoke of things burning (damaged buildings, wrecks).
    Soot,
    /// Alpha-blended pale grey steam from stacks; also powder thrown up on snow.
    Steam,
    /// Alpha-blended pale sand the wind lifts off open ground: faint, swelling then thinning.
    Sand,
    /// Alpha-blended dark brown dust thrown up on grass and rock.
    Earth,
    /// Ground decal: a tracked vehicle's tread marks, fading out.
    Tread,
    /// Ground decal: a blast's charred crater, fading out.
    Scar,
}

impl Fade {
    /// Haze that drifts with the `WIND`.
    fn drifts(self) -> bool {
        matches!(self, Fade::Smoke | Fade::Dust | Fade::Fumes | Fade::Soot | Fade::Steam | Fade::Earth | Fade::Sand)
    }
}

/// A light that flares and dies over `life` seconds, from `peak` intensity.
#[derive(Component)]
pub struct Flare {
    pub age: f32,
    pub life: f32,
    pub peak: f32,
}

/// A projectile flying from `from` to `to` over `life` seconds, `peak` high at mid-flight (0
/// for a straight shot). It leaves smoke behind every `trail` seconds (0: none).
#[derive(Component)]
pub struct Flight {
    pub age: f32,
    pub life: f32,
    pub from: Vec3,
    pub to: Vec3,
    pub peak: f32,
    pub trail: f32,
    pub since_puff: f32,
    pub smoke: Fade,
    pub puff: f32,
}

impl Flight {
    fn at(&self, t: f32) -> Vec3 {
        self.from.lerp(self.to, t) + Vec3::Y * self.peak * 4.0 * t * (1.0 - t)
    }
}

/// Number of material stages per fade track.
const STAGES: usize = 6;

/// Shared meshes and materials, so thousands of particles cost two handle clones each.
#[derive(Resource)]
pub struct Palette {
    /// A camera-facing square (see `face_camera`) with a soft round sprite: fire, smoke, dust.
    pub card: Handle<Mesh>,
    pub cube: Handle<Mesh>,
    pub ring: Handle<Mesh>,
    /// A flat unit square lying on the ground, and the soft charred mark it carries: what a
    /// kill leaves (`view.rs` wrecks). Blast craters and tread marks fade through their own
    /// tracks on the same square (`Fade::Scar`, `Fade::Tread`).
    pub decal: Handle<Mesh>,
    pub scorch: Handle<StandardMaterial>,
    tracks: [(Fade, [Handle<StandardMaterial>; STAGES]); 15],
}

impl Palette {
    fn stage(&self, fade: Fade, t: f32) -> Handle<StandardMaterial> {
        let ramp = &self.tracks.iter().find(|(f, _)| *f == fade).unwrap().1;
        ramp[((t * STAGES as f32) as usize).min(STAGES - 1)].clone()
    }
}

/// A puff sprite: white, fully opaque at the centre, falling off softly to clear at the rim,
/// with a little lumpy noise so stacked puffs don't read as discs.
fn puff_sprite() -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    const N: u32 = 64;
    let mut data = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let (u, v) = ((x as f32 + 0.5) / N as f32 * 2.0 - 1.0, (y as f32 + 0.5) / N as f32 * 2.0 - 1.0);
            let a = v.atan2(u);
            // Lumps around the rim: a few low harmonics, fixed, so every puff shares one sprite.
            let rim = 0.82 + 0.08 * (a * 3.0).sin() + 0.05 * (a * 5.0 + 1.3).sin() + 0.03 * (a * 9.0 + 0.4).sin();
            let r = (u * u + v * v).sqrt() / rim;
            let alpha = (1.0 - r).clamp(0.0, 1.0);
            // Smoothstep for a soft edge, a little denser in the middle.
            let alpha = alpha * alpha * (3.0 - 2.0 * alpha);
            data.extend_from_slice(&[255, 255, 255, (alpha * 255.0) as u8]);
        }
    }
    Image::new(
        Extent3d { width: N, height: N, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// A tread-mark sprite, laid across a vehicle's width (u) and along its run (v): two belts
/// near the edges, where the models' tracks run (`models::track`, at 0.44 of the hull width
/// from the centre), with a dark grouser bar every quarter, so marks laid end to end read as
/// one track.
fn tread_sprite() -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    const W: u32 = 32;
    const H: u32 = 32;
    let mut data = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        for x in 0..W {
            let u = (x as f32 + 0.5) / W as f32;
            let v = (y as f32 + 0.5) / H as f32;
            // Each belt about a seventh of the width, as the models' track bands are.
            let belt = ((0.07 - (u - 0.08).abs().min((u - 0.92).abs())) / 0.02).clamp(0.0, 1.0);
            let grouser = if (v * 4.0).fract() < 0.35 { 1.0 } else { 0.6 };
            data.extend_from_slice(&[255, 255, 255, (belt * grouser * 255.0) as u8]);
        }
    }
    Image::new(
        Extent3d { width: W, height: H, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let sprite = images.add(puff_sprite());
    // Additive: the sprite's alpha scales what is added, so the edges fade to nothing.
    let glow = |c: Color, k: f32| StandardMaterial {
        base_color: c,
        base_color_texture: Some(sprite.clone()),
        emissive: c.to_linear() * k,
        emissive_texture: Some(sprite.clone()),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        fog_enabled: false,
        double_sided: true,
        cull_mode: None,
        ..default()
    };
    // Smoke and dust: unlit, so a plume reads by its own tone rather than turning black on its
    // shaded side. Tones are picked against the graded ground (mid browns at ~120 luminance).
    let haze = |c: Color| StandardMaterial {
        base_color: c,
        base_color_texture: Some(sprite.clone()),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        double_sided: true,
        cull_mode: None,
        ..default()
    };
    let mut track = |stops: [Color; STAGES], f: &dyn Fn(Color) -> StandardMaterial| stops.map(|c| materials.add(f(c)));
    // Additive colours fade by darkening: black adds nothing.
    let fire = track(
        [
            Color::srgb(1.0, 0.95, 0.8),
            Color::srgb(1.0, 0.78, 0.35),
            Color::srgb(1.0, 0.5, 0.12),
            Color::srgb(0.75, 0.25, 0.05),
            Color::srgb(0.4, 0.1, 0.02),
            Color::srgb(0.15, 0.04, 0.01),
        ],
        &|c| glow(c, 2.0),
    );
    let toxic = track(
        [
            Color::srgb(0.85, 1.0, 0.5),
            Color::srgb(0.55, 1.0, 0.2),
            Color::srgb(0.35, 0.8, 0.1),
            Color::srgb(0.2, 0.5, 0.05),
            Color::srgb(0.1, 0.25, 0.02),
            Color::srgb(0.04, 0.1, 0.01),
        ],
        &|c| glow(c, 1.6),
    );
    let plasma = track(
        [
            Color::srgb(1.0, 1.0, 1.0),
            Color::srgb(0.8, 0.92, 1.0),
            Color::srgb(0.5, 0.75, 1.0),
            Color::srgb(0.3, 0.5, 0.9),
            Color::srgb(0.15, 0.25, 0.5),
            Color::srgb(0.05, 0.08, 0.2),
        ],
        &|c| glow(c, 2.2),
    );
    let spark = track(
        [
            Color::srgb(1.0, 0.95, 0.75),
            Color::srgb(1.0, 0.85, 0.5),
            Color::srgb(1.0, 0.7, 0.3),
            Color::srgb(0.7, 0.4, 0.12),
            Color::srgb(0.35, 0.15, 0.04),
            Color::srgb(0.1, 0.04, 0.01),
        ],
        &|c| glow(c, 2.5),
    );
    let flame = track(
        [
            Color::srgb(0.9, 0.5, 0.12),
            Color::srgb(0.85, 0.38, 0.08),
            Color::srgb(0.7, 0.25, 0.05),
            Color::srgb(0.5, 0.14, 0.03),
            Color::srgb(0.28, 0.07, 0.01),
            Color::srgb(0.1, 0.02, 0.0),
        ],
        &|c| glow(c, 1.5),
    );
    let smoke = track(
        [
            Color::srgba(0.24, 0.21, 0.18, 0.8),
            Color::srgba(0.3, 0.27, 0.24, 0.62),
            Color::srgba(0.36, 0.33, 0.29, 0.46),
            Color::srgba(0.41, 0.38, 0.34, 0.32),
            Color::srgba(0.45, 0.42, 0.38, 0.19),
            Color::srgba(0.48, 0.45, 0.41, 0.07),
        ],
        &haze,
    );
    let dust = track(
        [
            Color::srgba(0.62, 0.52, 0.36, 0.7),
            Color::srgba(0.64, 0.55, 0.4, 0.55),
            Color::srgba(0.66, 0.58, 0.43, 0.4),
            Color::srgba(0.68, 0.6, 0.46, 0.27),
            Color::srgba(0.7, 0.62, 0.48, 0.15),
            Color::srgba(0.72, 0.64, 0.5, 0.05),
        ],
        &haze,
    );
    let fumes = track(
        [
            Color::srgba(0.45, 0.7, 0.15, 0.55),
            Color::srgba(0.42, 0.65, 0.16, 0.45),
            Color::srgba(0.4, 0.6, 0.18, 0.35),
            Color::srgba(0.38, 0.55, 0.2, 0.25),
            Color::srgba(0.36, 0.5, 0.22, 0.14),
            Color::srgba(0.35, 0.45, 0.24, 0.05),
        ],
        &haze,
    );
    // Burning: near black at the fire, thinning to the smoke track's grey. The footage's
    // burning wrecks and buildings send up plumes far darker than the ground around them.
    let soot = track(
        [
            Color::srgba(0.07, 0.06, 0.05, 0.92),
            Color::srgba(0.1, 0.09, 0.08, 0.82),
            Color::srgba(0.15, 0.14, 0.13, 0.66),
            Color::srgba(0.21, 0.2, 0.18, 0.46),
            Color::srgba(0.28, 0.26, 0.24, 0.26),
            Color::srgba(0.34, 0.32, 0.3, 0.08),
        ],
        &haze,
    );
    // Stack steam: the pale grey the footage's power plant stacks give off, faint from the start.
    let steam = track(
        [
            Color::srgba(0.8, 0.79, 0.76, 0.42),
            Color::srgba(0.8, 0.79, 0.76, 0.36),
            Color::srgba(0.78, 0.77, 0.74, 0.28),
            Color::srgba(0.76, 0.75, 0.72, 0.19),
            Color::srgba(0.74, 0.73, 0.7, 0.11),
            Color::srgba(0.72, 0.71, 0.68, 0.04),
        ],
        &haze,
    );
    // Dark loam kicked up off grass and rock, where the pale dust track would glow.
    let earth = track(
        [
            Color::srgba(0.4, 0.33, 0.22, 0.65),
            Color::srgba(0.43, 0.36, 0.25, 0.5),
            Color::srgba(0.46, 0.39, 0.28, 0.37),
            Color::srgba(0.48, 0.41, 0.3, 0.25),
            Color::srgba(0.5, 0.43, 0.32, 0.14),
            Color::srgba(0.52, 0.45, 0.34, 0.04),
        ],
        &haze,
    );
    // Wind-blown sand: a veil a little paler than the sand under it (fine dust in the air
    // catches the light), never as dense as dust thrown up by a tank, thickest a third of the
    // way through its drift.
    let sand = track(
        [
            Color::srgba(0.8, 0.7, 0.53, 0.2),
            Color::srgba(0.8, 0.7, 0.53, 0.42),
            Color::srgba(0.81, 0.71, 0.54, 0.44),
            Color::srgba(0.82, 0.72, 0.56, 0.34),
            Color::srgba(0.83, 0.73, 0.57, 0.19),
            Color::srgba(0.84, 0.74, 0.58, 0.04),
        ],
        &haze,
    );
    // Ground decals are lit like the ground they lie on, and fade by alpha alone.
    let treads = images.add(tread_sprite());
    let mark = |c: Color, sprite: &Handle<Image>| StandardMaterial {
        base_color: c,
        base_color_texture: Some(sprite.clone()),
        perceptual_roughness: 1.0,
        alpha_mode: AlphaMode::Blend,
        depth_bias: 1.0,
        ..default()
    };
    // Full strength for the first half of their life, then fading out over the rest.
    let fading = |rgb: (f32, f32, f32), a: f32| -> [Color; STAGES] {
        std::array::from_fn(|i| Color::srgba(rgb.0, rgb.1, rgb.2, a * ((STAGES - i) as f32 / 3.0).min(1.0)))
    };
    // Treads: dark churned earth, as the footage's tracks cut through its pale dry grass.
    let tread = track(fading((0.12, 0.09, 0.06), 0.7), &|c| mark(c, &treads));
    let scar = track(fading((0.06, 0.05, 0.04), 0.8), &|c| mark(c, &sprite));
    let debris_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.12, 0.1, 0.08),
        perceptual_roughness: 0.9,
        ..default()
    });
    let debris = std::array::from_fn(|_| debris_mat.clone());
    let scorch = materials.add(StandardMaterial {
        base_color: Color::srgba(0.06, 0.05, 0.04, 0.85),
        base_color_texture: Some(sprite.clone()),
        perceptual_roughness: 1.0,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    commands.insert_resource(Living::default());
    commands.insert_resource(Palette {
        card: meshes.add(Rectangle::new(1.0, 1.0)),
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        ring: meshes.add(Torus::new(0.85, 1.0)),
        decal: meshes.add(Plane3d::new(Vec3::Y, Vec2::splat(0.5))),
        scorch,
        tracks: [
            (Fade::Fire, fire),
            (Fade::Flame, flame),
            (Fade::Toxic, toxic),
            (Fade::Plasma, plasma),
            (Fade::Spark, spark),
            (Fade::Smoke, smoke),
            (Fade::Dust, dust),
            (Fade::Fumes, fumes),
            (Fade::Debris, debris),
            (Fade::Soot, soot),
            (Fade::Steam, steam),
            (Fade::Earth, earth),
            (Fade::Sand, sand),
            (Fade::Tread, tread),
            (Fade::Scar, scar),
        ],
    });
}

/// Deterministic noise in 0..1 from a spot and a counter.
#[derive(Clone, Copy)]
struct Rng(u32);

impl Rng {
    fn at(p: SimVec2, salt: u32) -> Rng {
        Rng((p.x.0 as u32).wrapping_mul(0x9e37_79b1)
            ^ (p.y.0 as u32).wrapping_mul(0x85eb_ca6b)
            ^ salt.wrapping_mul(0xc2b2_ae35))
    }
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let mut h = self.0;
        h = (h ^ (h >> 15)).wrapping_mul(h | 1);
        h ^= h.wrapping_add((h ^ (h >> 7)).wrapping_mul(h | 61));
        ((h ^ (h >> 14)) & 0xffff) as f32 / 65_535.0
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
    /// A random direction in the upper hemisphere, `lift` 0 (flat) to 1 (straight up).
    fn dir(&mut self, lift: f32) -> Vec3 {
        let a = self.next() * std::f32::consts::TAU;
        let up = self.range(lift * 0.3, lift);
        Vec3::new(a.cos(), 0.0, a.sin()) * (1.0 - up * up).sqrt() + Vec3::Y * up
    }
}

/// Spawns effects. Borrowed per system run; `p` the palette, `c` the commands.
struct Spawner<'a, 'w, 's> {
    c: &'a mut Commands<'w, 's>,
    p: &'a Palette,
}

impl Spawner<'_, '_, '_> {
    fn particle(&mut self, mesh: &Handle<Mesh>, at: Vec3, rot: Quat, p: Particle) {
        self.c.spawn((
            crate::menu::MatchEntity,
            Mesh3d(mesh.clone()),
            MeshMaterial3d(self.p.stage(p.fade, p.age / p.life)),
            Transform { translation: at, rotation: rot, scale: p.size.0 },
            p,
            NotShadowCaster,
            NotShadowReceiver,
        ));
    }

    /// A soft round puff that always faces the camera, rolled by `spin` as it ages.
    fn puff(&mut self, at: Vec3, vel: Vec3, life: f32, size: (f32, f32), fade: Fade, gravity: f32) {
        let p = Particle {
            age: 0.0,
            life,
            vel,
            drag: 2.2,
            gravity,
            size: (Vec3::splat(size.0), Vec3::splat(size.1)),
            fade,
            spin: if fade.drifts() { 0.6 } else { 1.5 },
            ground: 0.0,
        };
        self.card(at, p, 0.0);
    }

    /// A camera-facing puff born at `at`, `age` seconds into its life: moved, grown and faded
    /// as it would have been by now, so a frozen screenshot shows a standing column of smoke
    /// rather than a clump at its source.
    fn card(&mut self, at: Vec3, mut p: Particle, age: f32) -> EntityCommands<'_> {
        // Each puff starts at its own roll, so a cluster of them doesn't repeat one shape.
        let roll = (at.x * 12.9898 + at.z * 78.233).sin() * 43_758.547;
        let mut tf = Transform::from_translation(at).with_scale(p.size.0);
        let mut lived = 0.0;
        while lived < age {
            let dt = (age - lived).min(TICK_SECS);
            lived += dt;
            p.age += dt;
            p.advance(&mut tf, dt);
        }
        self.c.spawn((
            crate::menu::MatchEntity,
            Mesh3d(self.p.card.clone()),
            MeshMaterial3d(self.p.stage(p.fade, p.age / p.life)),
            tf,
            p,
            Billboard(roll.fract() * std::f32::consts::TAU + p.spin * age),
            NotShadowCaster,
            NotShadowReceiver,
        ))
    }

    /// A flat mark on the ground, `size` across (x) and along (z) after turning by `yaw`,
    /// `age` seconds into a `life` over which it fades through `fade`. It takes the light
    /// and shadows of the ground it lies on.
    fn mark(&mut self, at: Vec3, yaw: f32, size: Vec2, fade: Fade, life: f32, age: f32) -> Entity {
        let scale = Vec3::new(size.x, 1.0, size.y);
        let p = Particle {
            age,
            life,
            vel: Vec3::ZERO,
            drag: 0.0,
            gravity: 0.0,
            size: (scale, scale),
            fade,
            spin: 0.0,
            ground: 0.0,
        };
        self.c
            .spawn((
                crate::menu::MatchEntity,
                Mesh3d(self.p.decal.clone()),
                MeshMaterial3d(self.p.stage(fade, age / life)),
                Transform::from_translation(at).with_rotation(Quat::from_rotation_y(yaw)).with_scale(scale),
                p,
                NotShadowCaster,
            ))
            .id()
    }

    /// A glowing bar from `a` to `b`, `width` thick, gone in `life`.
    fn streak(&mut self, a: Vec3, b: Vec3, width: f32, life: f32, fade: Fade) {
        let len = a.distance(b);
        if len < 0.05 {
            return;
        }
        let mid = (a + b) / 2.0;
        let rot = Transform::from_translation(mid).looking_at(b, Vec3::Y).rotation;
        let p = Particle {
            age: 0.0,
            life,
            vel: Vec3::ZERO,
            drag: 0.0,
            gravity: 0.0,
            size: (Vec3::new(width, width, len), Vec3::new(width * 0.4, width * 0.4, len)),
            fade,
            spin: 0.0,
            ground: 0.0,
        };
        self.particle(&self.p.cube.clone(), mid, rot, p);
    }

    /// A round in flight: a short glowing dash, `width` thick, travelling from `a` to `b` in
    /// `secs`. Its length is a fixed part of the line, so long shots read as long streaks.
    fn tracer(&mut self, a: Vec3, b: Vec3, width: f32, secs: f32) {
        let len = a.distance(b);
        if len < 0.05 || secs <= 0.0 {
            return;
        }
        let dash = (len * 0.18).clamp(1.2, 5.0);
        let dir = (b - a) / len;
        let start = a + dir * dash / 2.0;
        let rot = Transform::from_translation(start).looking_at(b, Vec3::Y).rotation;
        let p = Particle {
            age: 0.0,
            life: secs,
            vel: dir * (len - dash) / secs,
            drag: 0.0,
            gravity: 0.0,
            size: (Vec3::new(width, width, dash), Vec3::new(width, width, dash)),
            fade: Fade::Spark,
            spin: 0.0,
            ground: 0.0,
        };
        self.particle(&self.p.cube.clone(), start, rot, p);
    }

    fn flare(&mut self, at: Vec3, color: Color, peak: f32, range: f32, life: f32) {
        self.c.spawn((
            crate::menu::MatchEntity,
            PointLight { color, intensity: peak, range, shadow_maps_enabled: false, ..default() },
            Transform::from_translation(at),
            Flare { age: 0.0, life, peak },
        ));
    }

    /// A muzzle flash at `at`, facing `dir`: a hot core, a wider halo, a forward tongue.
    fn muzzle(&mut self, at: Vec3, dir: Vec3, size: f32, fade: Fade) {
        self.puff(at, Vec3::ZERO, 0.07, (size * 0.9, size * 1.4), fade, 0.0);
        self.puff(at, Vec3::ZERO, 0.11, (size * 1.8, size * 2.6), Fade::Spark, 0.0);
        self.streak(at, at + dir * size * 2.2, size * 0.55, 0.06, fade);
    }

    /// Dust and sparks where a round lands.
    fn impact(&mut self, at: Vec3, size: f32, rng: &mut Rng, sparks: usize, fade: Fade) {
        for _ in 0..sparks {
            let d = rng.dir(0.8);
            let speed = rng.range(8.0, 18.0) * size;
            self.streak(at, at + d * size * 0.9, size * 0.12, rng.range(0.08, 0.16), fade);
            self.puff(at, d * speed * 0.5, rng.range(0.12, 0.25), (size * 0.25, size * 0.08), fade, 30.0);
        }
        for _ in 0..2 {
            let d = rng.dir(0.6);
            self.puff(
                at + Vec3::Y * 0.2,
                d * rng.range(1.0, 3.0) * size + Vec3::Y * size,
                rng.range(0.6, 1.1),
                (size * 0.6, size * 2.2),
                Fade::Dust,
                -0.5,
            );
        }
    }

    /// A blast `r` in radius at `at` on the ground: a white flash, a fireball rolling up into
    /// smoke, a dust ring racing out along the ground, debris and a flare of light.
    fn explosion(&mut self, at: Vec3, r: f32, rng: &mut Rng, kind: Blast) {
        let (fire, smoke, light) = match kind {
            Blast::Fire => (Fade::Fire, Fade::Smoke, Color::srgb(1.0, 0.6, 0.25)),
            Blast::Toxic => (Fade::Toxic, Fade::Fumes, Color::srgb(0.5, 1.0, 0.3)),
            Blast::Plasma => (Fade::Plasma, Fade::Smoke, Color::srgb(0.75, 0.85, 1.0)),
        };
        let r = r.max(1.0);
        // Flash: brief and big, the moment of detonation.
        self.puff(at + Vec3::Y * r * 0.4, Vec3::ZERO, 0.12, (r * 1.2, r * 2.4), fire, 0.0);
        // Fireball: a cluster of hot balls rising and cooling.
        let balls = (4.0 + r * 1.2).min(14.0) as usize;
        for _ in 0..balls {
            let d = rng.dir(0.7);
            let s = r * rng.range(0.45, 0.8);
            self.puff(
                at + d * r * 0.35 + Vec3::Y * r * 0.3,
                d * r * rng.range(2.0, 4.0) + Vec3::Y * r * 1.2,
                rng.range(0.45, 0.8),
                (s, s * 1.7),
                fire,
                -r * 0.6,
            );
        }
        // Smoke: darker, slower, rising and spreading long after the fire is out.
        let puffs = (3.0 + r * 0.9).min(10.0) as usize;
        for k in 0..puffs {
            let d = rng.dir(0.8);
            let s = r * rng.range(0.6, 1.0);
            let lag = k as f32 * 0.04;
            self.puff(
                at + d * r * 0.4 + Vec3::Y * (r * 0.6 + lag),
                d * r * rng.range(0.8, 1.6) + Vec3::Y * r * rng.range(0.8, 1.4),
                rng.range(2.0, 3.6),
                (s, s * 2.8),
                smoke,
                -r * 0.25,
            );
        }
        // Dust ring: a flat torus racing outward along the ground, to about twice the blast.
        let ring = Particle {
            age: 0.0,
            life: 0.45,
            vel: Vec3::ZERO,
            drag: 0.0,
            gravity: 0.0,
            size: (Vec3::new(r * 0.5, r * 0.2, r * 0.5), Vec3::new(r * 1.8, r * 0.08, r * 1.8)),
            fade: Fade::Dust,
            spin: 0.0,
            ground: 0.0,
        };
        self.particle(&self.p.ring.clone(), at + Vec3::Y * 0.3, Quat::IDENTITY, ring);
        // Debris: small dark chunks thrown up in arcs, tumbling, settling on the ground.
        let chunks = (2.0 + r * 0.5).min(8.0) as usize;
        for _ in 0..chunks {
            let d = rng.dir(0.85);
            let s = rng.range(0.15, 0.35) * r.sqrt();
            let p = Particle {
                age: 0.0,
                life: rng.range(1.6, 2.6),
                vel: d * r.sqrt() * rng.range(5.0, 9.0) + Vec3::Y * r.sqrt() * 6.0,
                drag: 0.3,
                gravity: 40.0,
                size: (Vec3::new(s, s * 0.6, s * 1.2), Vec3::new(s, s * 0.6, s * 1.2)),
                fade: Fade::Debris,
                spin: rng.range(-12.0, 12.0),
                ground: at.y,
            };
            let rot = Quat::from_euler(EulerRot::YXZ, rng.range(0.0, 6.3), rng.range(-1.0, 1.0), rng.range(-1.0, 1.0));
            self.particle(&self.p.cube.clone(), at + Vec3::Y * r * 0.3, rot, p);
        }
        // Light: lights the ground, the models and the smoke around the blast.
        self.flare(at + Vec3::Y * r * 1.2, light, 6.0e6 * r.min(12.0), r * 9.0, 0.35);
    }
}

/// What burns in a blast.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Blast {
    Fire,
    Toxic,
    Plasma,
}

/// Seconds a flying shell or rocket takes per sim tick of flight.
const TICK_SECS: f32 = 1.0 / g2_sim::TICKS_PER_SECOND as f32;

/// Draws every shot gathered since the last frame. Under fog, only what has an end the local
/// player sees. Kill fireballs and the wrecks they leave come back as `(at, target)` for
/// `view.rs`, which owns wrecks; blasts leave their craters in `living`'s pool.
pub fn spawn_shots(
    c: &mut Commands,
    p: &Palette,
    sim: &Sim,
    living: &mut Living,
    shots: impl Iterator<Item = Shot>,
) -> Vec<(SimVec2, g2_sim::DefId)> {
    let vision = sim.world.vision(sim.local);
    let seen = |at: SimVec2| !sim.fog || vision.is_visible(at);
    let mut s = Spawner { c, p };
    let mut kills = Vec::new();
    let height =
        |at: SimVec2, air: bool| sim.to_world(at) + Vec3::Y * if air { crate::view::FLIGHT_HEIGHT } else { 0.0 };
    for (k, shot) in shots.enumerate() {
        match shot {
            Shot::Hit { from, at, killed, tracer, target, look, air } => {
                if !seen(from) && !seen(at) {
                    continue;
                }
                let mut rng = Rng::at(at, k as u32 ^ sim.world.tick());
                let (a, b) = (height(from, air.0) + Vec3::Y * TRACER_MUZZLE, height(at, air.1) + Vec3::Y * TRACER_HIT);
                if tracer && let Some(look) = look {
                    direct_fire(&mut s, a, b, look, &mut rng);
                }
                if killed
                    && seen(at)
                    && let Some(def) = target
                {
                    kills.push((at, def));
                    let d = sim.world.rules.def(def);
                    let r = 1.0 + d.radius.to_f32() * if d.is_building() { 0.8 } else { 0.6 };
                    // What dies burns, whatever killed it; green is a toxin shell's own blast.
                    s.explosion(b - Vec3::Y * 0.8, r, &mut rng, Blast::Fire);
                    if d.is_building() {
                        // A building burns on: a second, later wave of fire and a tall plume.
                        for _ in 0..4 {
                            let off = Vec3::new(rng.range(-1.0, 1.0), 0.0, rng.range(-1.0, 1.0)) * r * 0.6;
                            s.puff(
                                b + off,
                                Vec3::Y * r * 0.8,
                                rng.range(4.0, 6.0),
                                (r * 0.6, r * 2.4),
                                Fade::Smoke,
                                -r * 0.2,
                            );
                        }
                    }
                }
            }
            Shot::Shell { from, at, flight, done, look } => {
                if !seen(from) && !seen(at) {
                    continue;
                }
                shell(&mut s, sim, from, at, flight, done, look);
            }
            Shot::Blast { at, radius, look } => {
                if !seen(at) {
                    continue;
                }
                let mut rng = Rng::at(at, 0xb1a5 ^ sim.world.tick());
                let kind = match look {
                    Look::Shell { toxic: true, .. } | Look::Strike { toxic: true, .. } | Look::Toxin => Blast::Toxic,
                    Look::Strike { nuke: true, .. } | Look::Beam { rail: true } => Blast::Plasma,
                    _ => Blast::Fire,
                };
                let r = radius.to_f32() * if matches!(look, Look::Strike { .. }) { 0.85 } else { 0.7 };
                s.explosion(sim.to_world(at) + Vec3::Y * 0.4, r, &mut rng, kind);
                crater(&mut s, living, sim.to_world(at), radius.to_f32(), 0.0, &mut rng);
                if let Look::Shell { napalm: true, .. } = look {
                    // Napalm leaves fire on the ground for a few seconds.
                    for _ in 0..6 {
                        let off = Vec3::new(rng.range(-1.0, 1.0), 0.0, rng.range(-1.0, 1.0)) * radius.to_f32() * 0.7;
                        s.puff(
                            sim.to_world(at) + off + Vec3::Y * 0.5,
                            Vec3::Y * 2.0,
                            rng.range(1.5, 2.5),
                            (1.2, 2.4),
                            Fade::Flame,
                            -1.0,
                        );
                    }
                }
            }
            Shot::DigIn { at } => {
                if !seen(at) {
                    continue;
                }
                // A ring of dust thrown up as the bus settles into its bunker.
                let mut rng = Rng::at(at, 0xd16 ^ sim.world.tick());
                let base = sim.to_world(at) + Vec3::Y * 0.4;
                for k in 0..10 {
                    let a = k as f32 / 10.0 * std::f32::consts::TAU + rng.range(-0.2, 0.2);
                    let out = Vec3::new(a.cos(), 0.0, a.sin());
                    s.puff(base + out * 3.5, out * 4.0 + Vec3::Y, rng.range(1.2, 2.0), (1.4, 3.2), Fade::Dust, -0.3);
                }
            }
        }
    }
    kills
}

/// One direct-fire shot from `a` to `b`.
fn direct_fire(s: &mut Spawner, a: Vec3, b: Vec3, look: Look, rng: &mut Rng) {
    let dir = (b - a).normalize_or_zero();
    match look {
        Look::Bullets { heavy } => {
            s.muzzle(a, dir, if heavy { 1.0 } else { 0.7 }, Fade::Spark);
            // A burst: each round a short glowing dash flying down the line, staggered so the
            // burst reads as a stream rather than one bar.
            let len = a.distance(b);
            for k in 0..if heavy { 3 } else { 2 } {
                let lead = k as f32 * 0.08 + rng.range(0.0, 0.04);
                let start = a - dir * len * lead;
                s.tracer(start, b, if heavy { 0.3 } else { 0.22 }, (len / 160.0).clamp(0.05, 0.16) * (1.0 + lead));
            }
            s.impact(b, if heavy { 0.7 } else { 0.5 }, rng, if heavy { 3 } else { 2 }, Fade::Spark);
        }
        Look::Cannon { heavy } => {
            let size = if heavy { 1.7 } else { 1.3 };
            s.muzzle(a, dir, size, Fade::Fire);
            // Barrel smoke drifting off the muzzle.
            s.puff(a + dir * size, dir * 2.0 + Vec3::Y * 1.5, 1.2, (size * 0.6, size * 2.2), Fade::Smoke, -1.0);
            // The round itself: a brief hot slug along the whole line.
            s.streak(a, b, if heavy { 0.4 } else { 0.32 }, 0.06, Fade::Spark);
            s.impact(b, if heavy { 1.2 } else { 0.9 }, rng, 5, Fade::Fire);
            s.puff(b, Vec3::ZERO, 0.15, (size * 0.8, size * 1.6), Fade::Fire, 0.0);
        }
        Look::Rocket => {
            s.muzzle(a, dir, 0.9, Fade::Fire);
            s.puff(a - dir * 0.8, -dir * 3.0 + Vec3::Y, 1.0, (0.8, 2.4), Fade::Smoke, -1.0);
            // Rockets fly visibly: a glowing body along a straight line, with a smoke trail.
            let len = a.distance(b);
            let life = (len / 90.0).clamp(0.12, 0.45);
            s.c.spawn((
                crate::menu::MatchEntity,
                Mesh3d(s.p.card.clone()),
                MeshMaterial3d(s.p.stage(Fade::Spark, 0.0)),
                Transform::from_translation(a).with_scale(Vec3::splat(0.55)),
                Billboard(0.0),
                Flight {
                    age: 0.0,
                    life,
                    from: a,
                    to: b,
                    peak: 0.0,
                    trail: 0.02,
                    since_puff: 0.0,
                    smoke: Fade::Smoke,
                    puff: 0.5,
                },
                NotShadowCaster,
                NotShadowReceiver,
            ));
            s.impact(b, 0.9, rng, 4, Fade::Fire);
            s.puff(b, Vec3::ZERO, 0.18, (1.4, 2.6), Fade::Fire, 0.0);
        }
        Look::Flame => {
            // A cone of fire: overlapping hot puffs streaming from the nozzle, widening and
            // reddening to a burst on the target, with smoke curling off the far end.
            let len = a.distance(b);
            let n = (len / 0.8).clamp(10.0, 26.0) as usize;
            for k in 0..n {
                let t = (k as f32 + rng.next()) / n as f32;
                let spread = 0.3 + t * 1.6;
                let p =
                    a.lerp(b, t) + Vec3::new(rng.range(-1.0, 1.0), rng.range(-0.2, 0.6), rng.range(-1.0, 1.0)) * spread;
                let size = 1.3 + t * 2.8;
                s.puff(p, dir * 5.0 + Vec3::Y * 1.5, rng.range(0.45, 0.7), (size, size * 1.7), Fade::Flame, -2.0);
            }
            s.muzzle(a, dir, 1.2, Fade::Fire);
            s.puff(b, Vec3::Y * 1.0, 0.6, (2.0, 3.6), Fade::Flame, -1.5);
            s.puff(b + Vec3::Y, Vec3::Y * 2.5, 1.8, (1.2, 3.4), Fade::Smoke, -1.0);
        }
        Look::Toxin => {
            let len = a.distance(b);
            let n = (len / 3.0).clamp(3.0, 8.0) as usize;
            for k in 0..n {
                let t = (k as f32 + rng.next()) / n as f32;
                let p = a.lerp(b, t);
                s.puff(p, dir * 4.0, rng.range(0.25, 0.4), (0.4, 0.9 + t), Fade::Toxic, 0.0);
            }
            s.puff(b, Vec3::Y * 1.0, 1.6, (1.2, 3.4), Fade::Fumes, -0.4);
            s.puff(b, Vec3::ZERO, 0.2, (1.0, 2.0), Fade::Toxic, 0.0);
        }
        Look::Beam { rail } => {
            let fade = if rail { Fade::Plasma } else { Fade::Fire };
            // A beam: a hot thin core and a wide faint sheath along the whole line.
            s.streak(a, b, 0.35, 0.18, fade);
            s.streak(a, b, 1.1, 0.12, if rail { Fade::Plasma } else { Fade::Spark });
            s.muzzle(a, dir, 1.4, fade);
            s.impact(b, 1.2, rng, 6, fade);
            s.puff(b, Vec3::ZERO, 0.2, (1.6, 3.0), fade, 0.0);
            if rail {
                // A railgun's slug leaves a fading spiral of vapour along its path.
                let n = (a.distance(b) / 3.0) as usize;
                for k in 0..n {
                    let p = a.lerp(b, k as f32 / n.max(1) as f32);
                    s.puff(p, Vec3::Y * 0.5, rng.range(0.6, 1.0), (0.35, 0.9), Fade::Dust, 0.0);
                }
            }
        }
        // Shells and strikes are drawn by `shell` and their blasts; a direct hit from one
        // (splash) has no line of its own.
        Look::Shell { .. } | Look::Strike { .. } => {}
    }
}

/// A shell, bomb or missile in flight from `from` to `at`, with `flight` ticks left and `done`
/// of its whole flight behind it (a screenshot of one fired earlier starts partway).
fn shell(s: &mut Spawner, sim: &Sim, from: SimVec2, at: SimVec2, flight: u32, done: f32, look: Look) {
    let (mut a, b) = (sim.to_world(from) + Vec3::Y * SHELL_MUZZLE, sim.to_world(at) + Vec3::Y * SHELL_LAND);
    let whole = flight as f32 * TICK_SECS / (1.0 - done).max(0.05);
    // A gun's shell climbs by its whole time in the air, as the gun that fired it is laid for
    // (`procanim::elevation`); `flight` counts from the firing tick, so `whole` is that time.
    let arc = shell_peak(whole);
    let (peak, size, trail, fade, puff) = match look {
        // A power's bombs and barrage shells fall from high above where each lands (a barrage's
        // outer shells start at its aim point in the sim, but come in from the sky): steep,
        // fast, a thin wisp of trail.
        Look::Strike { missile: false, .. } => {
            a = b + Vec3::new(-6.0, 70.0, 6.0);
            (0.0, 1.0, 0.05, Fade::Dust, 0.4)
        }
        Look::Strike { missile: true, toxic, .. } => {
            let peak = (a.distance(b) * 0.3).max(40.0);
            (peak, 1.4, 0.015, if toxic { Fade::Fumes } else { Fade::Smoke }, 1.4)
        }
        Look::Shell { toxic, .. } => (arc, 0.75, 0.035, if toxic { Fade::Fumes } else { Fade::Smoke }, 0.55),
        _ => (arc, 0.75, 0.035, Fade::Smoke, 0.55),
    };
    let glow = match look {
        Look::Shell { toxic: true, .. } | Look::Strike { toxic: true, .. } => Fade::Toxic,
        Look::Strike { nuke: true, .. } => Fade::Plasma,
        _ => Fade::Spark,
    };
    let f = Flight { age: whole * done, life: whole, from: a, to: b, peak, trail, since_puff: 0.0, smoke: fade, puff };
    let start = f.at(done);
    if done == 0.0 && !matches!(look, Look::Strike { .. }) {
        let dir = (b - a).normalize_or_zero() + Vec3::Y;
        s.muzzle(a, dir.normalize_or_zero(), 1.6, Fade::Fire);
        s.puff(a, Vec3::Y * 2.0, 1.6, (1.2, 3.6), Fade::Smoke, -1.0);
    }
    s.c.spawn((
        crate::menu::MatchEntity,
        Mesh3d(s.p.card.clone()),
        MeshMaterial3d(s.p.stage(glow, 0.0)),
        Transform::from_translation(start).with_scale(Vec3::splat(size)),
        f,
        Billboard(0.0),
        NotShadowCaster,
        NotShadowReceiver,
    ));
}

/// Moves particles, swaps their material as they age, and removes them at the end of life.
/// Frozen in screenshot mode, so a captured frame shows each effect as it was spawned.
pub fn age(
    mut commands: Commands,
    time: Res<Time>,
    palette: Option<Res<Palette>>,
    frozen: Option<Res<crate::view::FreezeEffects>>,
    mut particles: Query<
        (Entity, &mut Particle, &mut Transform, &mut MeshMaterial3d<StandardMaterial>),
        Without<Flight>,
    >,
    mut flights: Query<(Entity, &mut Flight, &mut Transform), Without<Particle>>,
    mut flares: Query<(Entity, &mut Flare, &mut PointLight)>,
) {
    let Some(palette) = palette else { return };
    if frozen.is_some() {
        return;
    }
    let dt = time.delta_secs();
    for (id, mut p, mut tf, mut mat) in &mut particles {
        let stage_before = (p.age / p.life * STAGES as f32) as usize;
        p.age += dt;
        if p.age >= p.life {
            commands.entity(id).despawn();
            continue;
        }
        let t = p.age / p.life;
        p.advance(&mut tf, dt);
        if (t * STAGES as f32) as usize != stage_before && p.fade != Fade::Debris {
            mat.0 = palette.stage(p.fade, t);
        }
    }
    let mut puffs = Vec::new();
    for (id, mut f, mut tf) in &mut flights {
        f.age += dt;
        if f.age >= f.life {
            commands.entity(id).despawn();
            continue;
        }
        let t = f.age / f.life;
        let at = f.at(t);
        if f.trail > 0.0 {
            f.since_puff += dt;
            while f.since_puff >= f.trail {
                f.since_puff -= f.trail;
                puffs.push((at, f.smoke, f.puff));
            }
        }
        tf.translation = at;
    }
    let mut s = Spawner { c: &mut commands, p: &palette };
    for (at, fade, size) in puffs {
        s.puff(at, Vec3::Y * 0.6, 0.9, (size, size * 2.4), fade, -0.3);
        s.puff(at, Vec3::ZERO, 0.08, (size * 0.8, size * 0.4), Fade::Spark, 0.0);
    }
    for (id, mut f, mut light) in &mut flares {
        f.age += dt;
        if f.age >= f.life {
            commands.entity(id).despawn();
            continue;
        }
        let t = f.age / f.life;
        light.intensity = f.peak * (1.0 - t) * (1.0 - t);
    }
}

/// A card turned to face the camera every frame, rolled by its `.0` (radians) about the view
/// axis. Its `Particle::spin` turns the roll as it ages.
#[derive(Component)]
pub struct Billboard(pub f32);

/// Turns every billboard to face the camera. Runs after `age`, so a puff's roll follows its
/// spin. Also runs frozen: a screenshot's camera is only placed after the effects spawn.
pub fn face_camera(
    time: Res<Time>,
    frozen: Option<Res<crate::view::FreezeEffects>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut cards: Query<(&mut Billboard, &mut Transform, Option<&Particle>)>,
) {
    let Some(cam) = camera.iter().next() else { return };
    let view = cam.rotation();
    let dt = if frozen.is_some() { 0.0 } else { time.delta_secs() };
    for (mut b, mut tf, p) in &mut cards {
        b.0 += p.map_or(0.0, |p| p.spin) * dt;
        tf.rotation = view * Quat::from_rotation_z(b.0);
    }
}

// ---- The living battlefield ----------------------------------------------------------------
//
// Effects that come from the state of things rather than from shots: damaged buildings smoke
// and burn, damaged vehicles trail smoke, wrecks smoulder, vehicles throw dust and lay tread
// marks, blasts leave craters, stacks steam. Like the shots, they never feed back into the sim,
// and every random choice is a hash of the entity or spot and the tick.

/// Below this share of its maximum hit points a building smokes and a vehicle starts to: the
/// Generals/ZH `UnitDamagedThreshold` (GameData.ini) is 0.7; the alpha's smoking buildings in
/// the footage are about two-thirds down, so we use 2/3.
pub const DAMAGED: (i32, i32) = (2, 3);
/// Below this share it burns: the Generals/ZH `UnitReallyDamagedThreshold` (0.35); 1/3 here.
pub const BURNING: (i32, i32) = (1, 3);

/// How hurt something looks.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Harm {
    Whole,
    Damaged,
    Burning,
}

impl Harm {
    /// `hp` of `max`, in integers so the threshold is exact.
    pub fn of(hp: i32, max: i32) -> Harm {
        let below = |(n, d): (i32, i32)| (hp as i64) * (d as i64) < (max as i64) * (n as i64);
        if hp <= 0 || max <= 0 {
            Harm::Whole
        } else if below(BURNING) {
            Harm::Burning
        } else if below(DAMAGED) {
            Harm::Damaged
        } else {
            Harm::Whole
        }
    }
}

/// What a source of ambient effects gives off, and how often.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Emitter {
    /// A thin dark wisp from a damaged roof.
    Wisp,
    /// A burning roof: a thick soot column, fire licking at its foot, the odd spark.
    Blaze,
    /// A burning vehicle's trail of soot.
    Trail,
    /// A stack's pale steam plume.
    Plume,
    /// A wreck smouldering: soot thinning out over its life, and fire at first for a building.
    Smoulder { fire: bool },
}

impl Emitter {
    /// Puffs a second: Generals/ZH's `SmokeFactionSmall` bursts every 1-5 s and its
    /// `SmokeFactionMedium` every 2-8 s, but each of its puffs lives a minute; ours live a few
    /// seconds, so more of them keep a column standing. Picked by eye against the footage's
    /// burning buildings at 266-325 s, and kept low: these run for every damaged thing in view.
    pub fn rate(self) -> f32 {
        match self {
            Emitter::Wisp => 1.6,
            Emitter::Blaze => 4.0,
            Emitter::Trail => 3.0,
            Emitter::Plume => 1.2,
            Emitter::Smoulder { .. } => 2.4,
        }
    }

    /// How long one of its puffs lives, seconds: it is drawn this far back in time when primed.
    fn life(self) -> f32 {
        match self {
            Emitter::Wisp => 4.5,
            Emitter::Blaze => 5.0,
            Emitter::Trail => 2.2,
            Emitter::Plume => 5.5,
            Emitter::Smoulder { .. } => 5.0,
        }
    }

    /// Puff size for a source of radius `r`: building fires grow with the building (a war
    /// factory's column is about twice a turret's, as in the footage at 300-310 s), a burning
    /// vehicle's trail with the vehicle; stacks keep their own size.
    fn scale(self, r: f32) -> f32 {
        match self {
            Emitter::Wisp | Emitter::Blaze => (r / 4.0).clamp(1.0, 2.2),
            Emitter::Trail => (r * 0.5).max(1.0),
            Emitter::Plume | Emitter::Smoulder { .. } => 1.0,
        }
    }
}

/// The emitters a damaged entity carries: none whole; a building one wisp per roof point when
/// damaged and a blaze at each when burning; a unit a smoke trail only when burning.
pub fn harm_emitters(harm: Harm, building: bool) -> &'static [Emitter] {
    match (harm, building) {
        (Harm::Whole, _) | (Harm::Damaged, false) => &[],
        (Harm::Damaged, true) => &[Emitter::Wisp],
        (Harm::Burning, true) => &[Emitter::Blaze],
        (Harm::Burning, false) => &[Emitter::Trail],
    }
}

/// Points on a building's roof for its damage smoke, as offsets from its centre: one or two,
/// from a hash of its id so they stay put and differ between buildings of a kind. `height` is
/// the roof's top, so the fire sits on it rather than inside the model.
pub fn roof_points(id: g2_sim::EntityId, radius: f32, height: f32) -> Vec<Vec3> {
    let mut rng = Rng(id.index.wrapping_mul(0x2c1b_3c6d) ^ 0x51ed_270b);
    let n = if radius >= 5.0 && rng.next() < 0.6 { 2 } else { 1 };
    (0..n)
        .map(|_| {
            let a = rng.next() * std::f32::consts::TAU;
            let d = rng.range(0.15, 0.45) * radius;
            Vec3::new(a.cos() * d, height, a.sin() * d)
        })
        .collect()
}

/// Stacks that steam, by def name: offsets from the entity's centre to each stack's mouth.
/// The alpha's models (when imported) and our procedural ones put their stacks in different
/// places, so each look has its own list. Alpha points are measured from its meshes' height
/// fields (the tallest narrow peaks, then checked in the meshes); procedural ones are where
/// `models.rs` puts its stacks (`plant`'s cooling towers, `workshop`'s chimney pipes). The
/// alpha's derricks have flare stacks; ours are pumpjacks without one.
pub fn stacks(def: &g2_sim::Def, alpha: bool) -> Vec<Vec3> {
    use g2_sim::{BuildingRole, Faction};
    let Some(role) = def.role() else { return Vec::new() };
    if alpha {
        // Alpha models are centred on their footprint (`view::alpha_models`): the points are in
        // that frame, from `_rts/art/*/<model>_mesh` with the bounding-box centre at the origin.
        return match def.name {
            // es_powerplant: a round reactor whose four vent tops stand 8.5 high.
            "EU Power Plant" => vec![Vec3::new(2.4, 8.5, 0.0), Vec3::new(-2.4, 8.5, 0.0)],
            // as_powerplant: the tall stack in its corner (8.9).
            "APA Power Plant" => vec![Vec3::new(4.8, 8.9, 4.8)],
            // es_warfactory: the paired exhausts at its back corner (7.0).
            "EU War Factory" => vec![Vec3::new(-2.2, 7.0, -9.8), Vec3::new(-4.3, 7.0, -7.8)],
            // gs_warfactory: the chimney on its rear corner (7.9).
            "GLA War Factory" => vec![Vec3::new(7.6, 7.9, -5.6)],
            // The derricks' flare stacks.
            "EU Oil Derrick" => vec![Vec3::new(0.2, 8.4, -5.6)],
            "APA Oil Derrick" => vec![Vec3::new(-4.4, 10.2, 5.0)],
            "GLA Oil Derrick" => vec![Vec3::new(-3.6, 10.9, 4.2)],
            _ => Vec::new(),
        };
    }
    let w = def.radius.to_f32() * 1.75;
    match (def.faction, role) {
        // `plant`: twin cooling stacks 8.7 high at (±0.22 w, -0.36 w).
        (_, BuildingRole::PowerPlant) => {
            vec![Vec3::new(-0.22 * w, 9.0, -0.36 * w), Vec3::new(0.22 * w, 9.0, -0.36 * w)]
        }
        // `workshop`: chimney pipes 8.5 high at (0.18 w and 0.36 w, -0.32 w).
        (Some(Faction::Gla), BuildingRole::Factory) if !trains_infantry(def) => {
            vec![Vec3::new(0.18 * w, 8.6, -0.32 * w), Vec3::new(0.36 * w, 8.6, -0.32 * w)]
        }
        _ => Vec::new(),
    }
}

/// A factory drawn as a barracks or camp (as `models::parts_for` decides).
fn trains_infantry(def: &g2_sim::Def) -> bool {
    let rules = g2_rules::rules();
    def.produces.iter().any(|&u| rules.def(u).armour == g2_sim::Armour::Infantry)
}

/// How a ground unit moves over the ground, for dust and tread marks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gait {
    /// Lays tread marks and throws a broad dust plume.
    Tracked,
    /// Throws dust, no tread marks.
    Wheeled,
    /// A squad's feet: tiny puffs.
    Foot,
    /// Aircraft: nothing on the ground.
    Airborne,
}

impl Gait {
    /// Following `models.rs`: infantry walk, aircraft fly; constructors, artillery and armed
    /// vehicles up to 12 a second (`combat_unit`'s cut) run on tracks; supply trucks, the bus
    /// and fast vehicles roll on wheels.
    pub fn of(def: &g2_sim::Def) -> Gait {
        let fast = def.speed() > g2_sim::Fx::from_ratio(12, g2_sim::TICKS_PER_SECOND as i64);
        let artillery = def.deploy.is_some() || def.weapon().is_some_and(|w| w.shell.is_some());
        if def.flies() {
            Gait::Airborne
        } else if def.armour == g2_sim::Armour::Infantry {
            Gait::Foot
        } else if def.is_constructor() || artillery || (def.weapon().is_some() && !fast) {
            Gait::Tracked
        } else {
            Gait::Wheeled
        }
    }
}

/// Dust puffs a second for a ground unit moving at `speed` world units a second: none standing,
/// rising with speed. A tank at 9 a second throws about four and a half, a squad one or two.
pub fn dust_rate(gait: Gait, speed: f32) -> f32 {
    if speed < 0.5 {
        return 0.0;
    }
    match gait {
        Gait::Tracked => (speed * 0.5).min(8.0),
        Gait::Wheeled => (speed * 0.4).min(8.0),
        Gait::Foot => (speed * 0.2).min(2.0),
        Gait::Airborne => 0.0,
    }
}

/// Live haze particles the ambient effects may keep at once: about what the Inferno barrage
/// clip peaked at (228, see STYLE.md) plus room for a burning base; beyond it, new puffs are
/// skipped until old ones go. Shots and blasts are not counted and never skipped.
pub const AMBIENT_PUFFS: usize = 600;
/// Tread mark segments alive at once (Generals/ZH's high-detail `MaxTankTrackEdges` is 100
/// per vehicle; we share one pool among all vehicles, and drop the oldest first).
pub const TREAD_MARKS: usize = 400;
/// Seconds a tread mark takes to fade (Generals/ZH's `MaxTankTrackFadeDelay` is 30 s at high
/// detail; 20 s keeps the pool from running out in a big tank battle).
pub const TREAD_SECS: f32 = 20.0;
/// World units between tread marks along a track's path.
const TREAD_STEP: f32 = 2.2;
/// Blast craters alive at once; the oldest goes first.
pub const CRATERS: usize = 60;
/// Seconds a blast crater takes to fade.
pub const CRATER_SECS: f32 = 45.0;

/// A puff given off by an ambient emitter: counts against `AMBIENT_PUFFS`.
#[derive(Component)]
pub struct Ambient;

/// A flat ground mark that fades on its own (tread marks, craters), oldest first when over the
/// cap.
#[derive(Component)]
pub struct Mark;

/// Per-entity emitter state: where it was, how far its tracks have run since the last mark,
/// and the fraction of a puff each emitter owes (up to four emitters, then its dust).
#[derive(Default)]
struct Trail {
    at: Option<Vec3>,
    since_mark: f32,
    owed: [f32; 5],
    /// Last frame this entity was seen; entries not seen for a frame are dropped.
    frame: u32,
}

const DUST_SLOT: usize = 4;

/// What the ambient effects remember between frames; reset with each new scene (`setup`).
#[derive(Resource, Default)]
pub struct Living {
    trails: std::collections::BTreeMap<g2_sim::EntityId, Trail>,
    /// Wrecks smouldering: where, how big, a building's or not, seconds since the kill, and
    /// seconds it smoulders.
    wrecks: Vec<(Vec3, f32, bool, f32, f32)>,
    /// Ground marks, oldest first. Faded ones (despawned by `age`) stay listed until pushed out;
    /// despawning them again is a no-op.
    treads: std::collections::VecDeque<Entity>,
    craters: std::collections::VecDeque<Entity>,
    frame: u32,
    /// The match tick the scene was last primed for (see `prime`).
    primed: Option<u32>,
    /// The match tick whose sand gusts are up (`sand_gusts`).
    sanded: Option<u32>,
}

impl Living {
    /// A kill's wreck at `at`, radius `r`, `age` seconds old, starts smouldering for `life`
    /// seconds (as long as `view.rs` keeps the wreck itself).
    pub fn wreck(&mut self, at: Vec3, r: f32, building: bool, age: f32, life: f32) {
        self.wrecks.push((at, r, building, age, life));
    }
}

/// The most particles one `emit` spawns (a blaze's smoke, flame, core and spark): an emitter
/// fires only with that much of the cap left, so a burst never runs past it.
const EMIT_MAX: usize = 4;

/// One puff from emitter `e` at `at`, `age` seconds old (0 for a fresh one), `scale` the size
/// of what gives it off; `rng` picks its jitter. Returns how many particles it spawned.
fn emit(s: &mut Spawner, e: Emitter, at: Vec3, scale: f32, age: f32, rng: &mut Rng, fade_out: f32) -> usize {
    let haze = |life: f32, vel: Vec3, size: (f32, f32), fade: Fade, rise: f32| Particle {
        age: 0.0,
        life,
        vel,
        drag: 1.2,
        gravity: -rise,
        size: (Vec3::splat(size.0), Vec3::splat(size.1)),
        fade,
        spin: 0.6,
        ground: 0.0,
    };
    let jitter = Vec3::new(rng.range(-0.5, 0.5), 0.0, rng.range(-0.5, 0.5)) * scale;
    let mut n = 0;
    let mut card = |s: &mut Spawner, at: Vec3, p: Particle| {
        if age < p.life {
            s.card(at, p, age).insert(Ambient);
            n += 1;
        }
    };
    match e {
        Emitter::Wisp => {
            // Soot, not the grey smoke track: grey vanishes against the light roofs.
            let p = haze(e.life(), Vec3::Y * 1.8, (0.9 * scale, 3.6 * scale), Fade::Soot, 0.4);
            card(s, at + jitter * 0.4, p);
        }
        Emitter::Blaze => {
            let p = haze(e.life(), Vec3::Y * 3.0, (1.8 * scale, 6.0 * scale), Fade::Soot, 1.0);
            card(s, at + jitter * 0.6, p);
            // Fire at the foot of the column, licking up: the alpha's burning buildings show
            // flames on the roof under the smoke. Each puff brings one tongue of flame and,
            // often, a hotter core.
            let f = haze(1.0, Vec3::Y * 2.5 + jitter, (1.8 * scale, 0.7 * scale), Fade::Flame, 1.5);
            card(s, at + jitter * 0.5, f);
            if rng.next() < 0.5 {
                let core = haze(0.5, Vec3::Y * 1.5, (1.0 * scale, 0.4 * scale), Fade::Fire, 1.0);
                card(s, at + jitter * 0.3, core);
            }
            if rng.next() < 0.12 {
                let d = rng.dir(0.8);
                let spark = Particle { drag: 0.3, gravity: 9.0, ..haze(0.6, d * 7.0, (0.25, 0.1), Fade::Spark, 0.0) };
                card(s, at, spark);
            }
        }
        Emitter::Trail => {
            let p = haze(e.life(), Vec3::Y * 1.5, (0.6 * scale, 2.6 * scale), Fade::Soot, 0.5);
            card(s, at + jitter * 0.3, p);
        }
        Emitter::Plume => {
            let p = haze(e.life(), Vec3::Y * 2.2, (0.7 * scale, 3.4 * scale), Fade::Steam, 0.3);
            card(s, at + jitter * 0.15, p);
        }
        Emitter::Smoulder { fire } => {
            // Thick and dark at first, thinning as the wreck cools: `fade_out` 1 to 0.
            let size = scale * (0.5 + 0.5 * fade_out);
            let p = haze(e.life(), Vec3::Y * 2.0, (0.9 * size, 3.6 * size), Fade::Soot, 0.5);
            card(s, at + jitter * 0.5, p);
            if fire && fade_out > 0.75 && rng.next() < 0.8 {
                let f = haze(0.8, Vec3::Y * 2.0 + jitter, (1.3 * scale, 0.6 * scale), Fade::Flame, 1.2);
                card(s, at + jitter * 0.4, f);
            }
        }
    }
    n
}

/// One dust puff at `at` behind a unit of size `r` moving along `dir`, in the ground's own
/// tone (`fade`), `age` seconds old.
#[allow(clippy::too_many_arguments)]
fn kick(s: &mut Spawner, gait: Gait, at: Vec3, dir: Vec3, r: f32, fade: Fade, age: f32, rng: &mut Rng) -> usize {
    let (size, life, rise) = match gait {
        Gait::Foot => (0.6, 1.0, 0.2),
        Gait::Wheeled => (r * 0.8, 2.6, 0.4),
        _ => (r * 1.0, 3.0, 0.35),
    };
    let side = Vec3::new(-dir.z, 0.0, dir.x) * rng.range(-0.6, 0.6) * r;
    let p = Particle {
        age: 0.0,
        life,
        vel: -dir * rng.range(0.5, 1.5) + Vec3::Y * 0.8 + side * 0.4,
        drag: 1.6,
        gravity: -rise,
        size: (Vec3::splat(size * 0.6), Vec3::splat(size * 2.4)),
        fade,
        spin: 0.6,
        ground: 0.0,
    };
    if age >= life {
        return 0;
    }
    s.card(at + side + Vec3::Y * 0.4, p, age).insert(Ambient);
    1
}

/// The dust track for the ground under `(x, z)`: the painted ground class on an imported map
/// (grass and rock throw dark loam, snow powder), the pale dust track elsewhere (the built-in
/// map and sand, dirt, roads).
fn ground_dust(sim: &Sim, x: f32, z: f32) -> Fade {
    use crate::terrain::Ground;
    match sim.source.as_ref().and_then(|m| m.class_at(x, z)).map(Ground::of) {
        Some(Ground::Grass | Ground::Rock) => Fade::Earth,
        Some(Ground::Snow) => Fade::Steam,
        _ => Fade::Dust,
    }
}

/// Something in view that gives off ambient effects: its emitters (offsets from its centre, in
/// its own frame) and, for a ground unit, how it moves.
struct Source {
    id: g2_sim::EntityId,
    at: Vec3,
    emitters: Vec<(Emitter, Vec3)>,
    gait: Option<Gait>,
    r: f32,
}

/// The ambient sources the local player sees: damaged buildings and vehicles, steaming stacks,
/// moving ground units. `alpha`: the imported models are drawn (their stacks sit elsewhere).
fn sources(sim: &Sim, alpha: &dyn Fn(&str) -> bool) -> Vec<Source> {
    let world = &sim.world;
    let mut out = Vec::new();
    for e in world.entities() {
        if !sim.sees(e) || e.inside.is_some() {
            continue;
        }
        let def = world.rules.def(e.def);
        let building = def.is_building();
        if !building && !def.is_unit() {
            continue;
        }
        let r = def.radius.to_f32();
        let at = sim.to_world(e.pos);
        let harm = if e.is_complete() { Harm::of(e.hp, world.max_hp(e)) } else { Harm::Whole };
        let mut emitters = Vec::new();
        for &em in harm_emitters(harm, building) {
            if building {
                // Just under the overlay anchor, which floats over the roofs (8 for most
                // buildings; the procedural and alpha roofs top out at 5-8.5).
                let roof = crate::view::overlay_anchor(sim, e).y - at.y - 1.0;
                emitters.extend(roof_points(e.id, r, roof.max(2.0)).into_iter().map(|p| (em, p)));
            } else {
                emitters.push((em, Vec3::Y * 1.5));
            }
        }
        // Stacks steam while the building stands finished and powered (a derrick always pumps).
        let derrick = matches!(def.role(), Some(g2_sim::BuildingRole::OilDerrick { .. }));
        let running = e.is_complete() && (e.powered || derrick);
        if building && running && harm != Harm::Burning {
            emitters.extend(stacks(def, alpha(def.name)).into_iter().map(|p| (Emitter::Plume, p)));
        }
        let gait = (!building).then(|| Gait::of(def)).filter(|&g| g != Gait::Airborne);
        if emitters.is_empty() && gait.is_none() {
            continue;
        }
        out.push(Source { id: e.id, at, emitters, gait, r });
    }
    out
}

/// Spawns the ambient effects: called every frame by `view.rs` after the shots. `dt` is the
/// match time that passed (0 while paused); `alpha` names the defs drawn with imported models;
/// `focus` is the ground the camera looks at, where sand blows.
#[allow(clippy::too_many_arguments)]
pub fn live(
    c: &mut Commands,
    p: &Palette,
    sim: &Sim,
    living: &mut Living,
    dt: f32,
    frozen: bool,
    alpha: &dyn Fn(&str) -> bool,
    roots: &dyn Fn(g2_sim::EntityId) -> Option<(Vec3, Quat)>,
    ambient: usize,
    focus: Option<Vec3>,
) {
    let mut s = Spawner { c, p };
    let tick = sim.world.tick();
    living.frame = living.frame.wrapping_add(1);
    let frame = living.frame;
    // A fresh scene (or a screenshot) starts with every emitter's last few seconds drawn, so a
    // frozen frame shows standing columns and a running one doesn't start bare.
    if living.primed != Some(tick) && (frozen || living.primed.is_none()) {
        living.primed = Some(tick);
        living.sanded = Some(tick);
        prime(&mut s, sim, living, alpha, roots, focus);
        return;
    }
    if frozen || dt <= 0.0 {
        return;
    }
    let mut budget = AMBIENT_PUFFS.saturating_sub(ambient);
    for src in sources(sim, alpha) {
        let (at, rot) = roots(src.id).unwrap_or((src.at, Quat::IDENTITY));
        let trail = living.trails.entry(src.id).or_default();
        trail.frame = frame;
        let moved = trail.at.map_or(Vec3::ZERO, |prev| (at - prev).with_y(0.0));
        trail.at = Some(at);
        let mut rng = Rng(src.id.index.wrapping_mul(0x9e37_79b1) ^ tick.wrapping_mul(0x85eb_ca6b) ^ frame);
        for (k, &(em, off)) in src.emitters.iter().enumerate() {
            let slot = k.min(DUST_SLOT - 1);
            trail.owed[slot] += em.rate() * dt;
            while trail.owed[slot] >= 1.0 {
                trail.owed[slot] -= 1.0;
                if budget < EMIT_MAX {
                    continue;
                }
                let scale = em.scale(src.r);
                budget = budget.saturating_sub(emit(&mut s, em, at + rot * off, scale, 0.0, &mut rng, 1.0));
            }
        }
        let Some(gait) = src.gait else { continue };
        let dist = moved.length();
        // A unit that jumped (spawned, teleported, the scene reloaded) leaves nothing behind.
        if dist > 8.0 || dist < 1e-4 {
            continue;
        }
        let speed = dist / dt;
        let dir = moved / dist;
        trail.owed[DUST_SLOT] += dust_rate(gait, speed) * dt;
        let back = at - dir * src.r * 0.9;
        while trail.owed[DUST_SLOT] >= 1.0 {
            trail.owed[DUST_SLOT] -= 1.0;
            if budget > 0 {
                let fade = ground_dust(sim, back.x, back.z);
                budget -= kick(&mut s, gait, back, dir, src.r, fade, 0.0, &mut rng);
            }
        }
        if gait == Gait::Tracked {
            trail.since_mark += dist;
            if trail.since_mark >= TREAD_STEP {
                trail.since_mark = 0.0;
                let yaw = dir.x.atan2(dir.z);
                lay_tread(&mut s, living, sim, at, yaw, src.r, 0.0);
            }
        }
    }
    living.trails.retain(|_, t| t.frame == frame);
    // Wrecks smoulder for their life, thinning out.
    let mut rng = Rng(tick.wrapping_mul(0x2545_f491) ^ frame);
    for w in &mut living.wrecks {
        w.3 += dt;
        let (at, r, building, age, life) = *w;
        let left = 1.0 - age / life;
        let em = Emitter::Smoulder { fire: building };
        // A small wreck sends up one column, a building's a few, at its own rate each.
        let columns = if building { 2 } else { 1 };
        if rng.next() < em.rate() * columns as f32 * dt * left.max(0.3) && budget >= EMIT_MAX {
            let off = Vec3::new(rng.range(-0.5, 0.5), 0.0, rng.range(-0.5, 0.5)) * r;
            let scale = (r * 0.45).clamp(1.0, 3.0);
            budget = budget.saturating_sub(emit(&mut s, em, at + off + Vec3::Y * 0.8, scale, 0.0, &mut rng, left));
        }
    }
    living.wrecks.retain(|w| w.3 < w.4);
    // Blowing sand: this tick's gusts, once per tick however many frames show it.
    if let Some(focus) = focus
        && living.sanded != Some(tick)
    {
        living.sanded = Some(tick);
        for (at, _, strength, mut rng) in sand_gusts(sim, focus, tick, 0.0) {
            if budget >= SAND_PUFFS {
                budget -= sand_gust(&mut s, at, strength, 0.0, &mut rng);
            }
        }
    }
}

/// Gusts of blowing sand a second over the ground in view, at full wind on open sand: our own
/// pick, so a desert map shows a streamer or two at a time and a green one hardly any.
pub const SAND_RATE: f32 = 2.5;
/// How far from the camera's focus a gust may rise: about the ground a play-zoom view shows.
const SAND_REACH: f32 = 70.0;
/// Seconds a sand puff drifts, and how many puffs string out along one gust.
const SAND_LIFE: f32 = 5.0;
const SAND_PUFFS: usize = 5;

/// How readily the wind lifts sand off the ground under `(x, z)`, 0 to 1: open sand most, dry
/// dirt and the built-in savanna less, grass a little; never rock, pavement, snow or water.
pub fn sandiness(sim: &Sim, x: f32, z: f32) -> f32 {
    let Some(file) = sim.source.as_ref() else { return lift(None) };
    if crate::terrain::over_water(file, x, z) {
        return 0.0;
    }
    lift(file.class_at(x, z).map(crate::terrain::Ground::of))
}

/// `sandiness` by the ground's class; `None` is the built-in savanna or an unpainted cell.
fn lift(ground: Option<crate::terrain::Ground>) -> f32 {
    use crate::terrain::Ground;
    match ground {
        Some(Ground::Sand) => 1.0,
        Some(Ground::Dirt) | None => 0.5,
        Some(Ground::Grass) => 0.15,
        Some(Ground::Rock | Ground::Paved | Ground::Snow) => 0.0,
    }
}

/// One gust of blowing sand rising at `at` (on the ground), `age` seconds ago: a low streamer
/// of wide, faint puffs strung out downwind, running before the wind faster than smoke drifts
/// and faster in a gust. Each puff is stretched across the screen, which the wind mostly runs
/// along at the game camera's bearing. Returns the puffs spawned.
fn sand_gust(s: &mut Spawner, at: Vec3, strength: f32, age: f32, rng: &mut Rng) -> usize {
    let dir = WIND.normalize();
    let speed = 4.0 + 5.0 * strength;
    let mut n = 0;
    for k in 0..SAND_PUFFS {
        let lag = k as f32 * 0.35;
        let life = SAND_LIFE * rng.range(0.8, 1.1);
        if age - lag >= life || age < lag {
            continue;
        }
        let side = Vec3::new(-dir.z, 0.0, dir.x) * rng.range(-1.5, 1.5) + dir * k as f32 * 3.5;
        let size = rng.range(2.5, 3.5);
        let p = Particle {
            age: 0.0,
            life,
            vel: dir * speed * rng.range(0.85, 1.15) + Vec3::Y * 0.15,
            drag: 0.0,
            gravity: 0.0,
            size: (Vec3::new(size * 1.6, size * 0.6, size), Vec3::new(size * 4.5, size * 1.3, size)),
            fade: Fade::Sand,
            spin: 0.0,
            ground: 0.0,
        };
        // Unrolled, so the streamer lies along the ground rather than at a random tilt.
        s.card(at + side + Vec3::Y * rng.range(0.5, 1.2), p, age - lag).insert((Ambient, Billboard(0.0)));
        n += 1;
    }
    n
}

/// Gusts of sand that rose over the last `secs` seconds (up to `now`), around `focus`: each
/// with its age and the wind's strength when it rose. A gust rises in each tick's slot with a chance by the wind (`gust`) and the
/// ground under it, so a still frame and a running match draw the same gusts for a tick.
fn sand_gusts(sim: &Sim, focus: Vec3, now: u32, secs: f32) -> Vec<(Vec3, f32, f32, Rng)> {
    let tps = g2_sim::TICKS_PER_SECOND;
    let span = (secs * tps as f32) as u32;
    let mut out = Vec::new();
    for tick in now.saturating_sub(span)..=now {
        let t = tick as f32 / tps as f32;
        let strength = gust(t);
        let mut rng = Rng(tick.wrapping_mul(0x632b_e5ab) ^ 0x5a4d);
        // The chance of a gust this tick at full sandiness; the ground decides the rest.
        if rng.next() >= SAND_RATE * (0.25 + strength) / tps as f32 {
            continue;
        }
        // Around the focus snapped to a grid, so the same tick picks the same ground however the
        // camera sits within a cell.
        let cell = 16.0;
        let x = (focus.x / cell).round() * cell + rng.range(-SAND_REACH, SAND_REACH);
        let z = (focus.z / cell).round() * cell + rng.range(-SAND_REACH, SAND_REACH);
        let (w, h) = (sim.world.map.width.to_f32(), sim.world.map.height.to_f32());
        // Not over unexplored ground, which the fog sheet hides and a puff above it wouldn't.
        let open = (0.0..w).contains(&x) && (0.0..h).contains(&z) && sim.explored_at(x, z);
        if !open || rng.next() >= sandiness(sim, x, z) {
            continue;
        }
        out.push((Vec3::new(x, sim.ground_y(x, z), z), (now - tick) as f32 / tps as f32, strength, rng));
    }
    out
}

/// Draws the last seconds of every emitter at once, each puff already as old as it would be
/// now: the screenshot's still frame and the first frame of a match show the plumes as they
/// stand, and the sand gusts still blowing. Tread marks are laid back along each tracked
/// vehicle's facing, as if it had driven in; craters and wrecks come from the shots and kills
/// (`spawn_shots`, `view.rs`).
fn prime(
    s: &mut Spawner,
    sim: &Sim,
    living: &mut Living,
    alpha: &dyn Fn(&str) -> bool,
    roots: &dyn Fn(g2_sim::EntityId) -> Option<(Vec3, Quat)>,
    focus: Option<Vec3>,
) {
    let tick = sim.world.tick();
    let mut budget = AMBIENT_PUFFS;
    // The sand gusts still drifting, each as far along as it is by now.
    for (at, age, strength, mut rng) in focus.map_or(Vec::new(), |f| sand_gusts(sim, f, tick, SAND_LIFE + 1.5)) {
        if budget >= SAND_PUFFS {
            budget -= sand_gust(s, at, strength, age, &mut rng);
        }
    }
    for src in sources(sim, alpha) {
        let (at, rot) = roots(src.id).unwrap_or((src.at, Quat::IDENTITY));
        let mut rng = Rng(src.id.index.wrapping_mul(0x9e37_79b1) ^ tick.wrapping_mul(0x85eb_ca6b));
        for &(em, off) in &src.emitters {
            let n = (em.rate() * em.life()) as usize;
            let scale = em.scale(src.r);
            for k in 0..n {
                if budget < EMIT_MAX {
                    break;
                }
                let age = (k as f32 + rng.next()) / em.rate();
                budget = budget.saturating_sub(emit(s, em, at + rot * off, scale, age, &mut rng, 1.0));
            }
        }
        let Some(gait) = src.gait else { continue };
        let Some((dir, speed)) = heading(sim, src.id) else { continue };
        let rate = dust_rate(gait, speed);
        let back = at - dir * src.r * 0.9;
        let fade = ground_dust(sim, back.x, back.z);
        for k in 0..(rate * 2.0) as usize {
            if budget == 0 {
                break;
            }
            let age = (k as f32 + rng.next()) / rate;
            // Older puffs were thrown further back along the path.
            budget -= kick(s, gait, back - dir * speed * age, dir, src.r, fade, age, &mut rng);
        }
        if gait == Gait::Tracked {
            let yaw = dir.x.atan2(dir.z);
            for k in 0..((speed * 6.0 / TREAD_STEP) as usize).min(30) {
                let behind = k as f32 * TREAD_STEP;
                let age = behind / speed.max(1.0);
                let p = at - dir * behind;
                lay_tread(s, living, sim, p.with_y(sim.ground_y(p.x, p.z)), yaw, src.r, age);
            }
        }
    }
    // Wrecks: each column's last seconds, thinning as the wreck has aged.
    let mut rng = Rng(tick.wrapping_mul(0x2545_f491));
    for &(at, r, building, age, life) in &living.wrecks {
        let em = Emitter::Smoulder { fire: building };
        let columns = if building { 2 } else { 1 };
        let scale = (r * 0.45).clamp(1.0, 3.0);
        for k in 0..(em.rate() * em.life() * columns as f32) as usize {
            if budget < EMIT_MAX {
                break;
            }
            let back = (k as f32 + rng.next()) / (em.rate() * columns as f32);
            // Puffs given off before the kill never were.
            if back > age.max(1.5) {
                break;
            }
            let off = Vec3::new(rng.range(-0.5, 0.5), 0.0, rng.range(-0.5, 0.5)) * r;
            let left = 1.0 - (age - back).max(0.0) / life;
            budget = budget.saturating_sub(emit(s, em, at + off + Vec3::Y * 0.8, scale, back, &mut rng, left));
        }
    }
}

/// A tread mark under a tracked vehicle of radius `r` at `at`, facing `yaw`, `age` seconds old.
fn lay_tread(s: &mut Spawner, living: &mut Living, sim: &Sim, at: Vec3, yaw: f32, r: f32, age: f32) {
    // As wide as the procedural tank's track span (1.3 r, `models::combat_unit`, times the
    // vehicle scale 1.5), a little longer than a step so the marks overlap.
    let width = r * 1.3 * 1.5;
    let y = sim.ground_y(at.x, at.z) + 0.05;
    let e = s.mark(at.with_y(y), yaw, Vec2::new(width, TREAD_STEP * 1.15), Fade::Tread, TREAD_SECS, age);
    s.c.entity(e).insert(Mark);
    living.treads.push_back(e);
    while living.treads.len() > TREAD_MARKS {
        if let Some(old) = living.treads.pop_front() {
            s.c.entity(old).try_despawn();
        }
    }
}

/// A blast's crater: a charred decal sized by its radius, `age` seconds into fading over
/// `CRATER_SECS`. Kills keep their own scorch (`view.rs` wrecks).
fn crater(s: &mut Spawner, living: &mut Living, at: Vec3, radius: f32, age: f32, rng: &mut Rng) {
    let size = (radius * 1.6).clamp(2.0, 18.0);
    let yaw = rng.next() * std::f32::consts::TAU;
    let e = s.mark(at + Vec3::Y * 0.07, yaw, Vec2::splat(size), Fade::Scar, CRATER_SECS, age);
    s.c.entity(e).insert(Mark);
    living.craters.push_back(e);
    while living.craters.len() > CRATERS {
        if let Some(old) = living.craters.pop_front() {
            s.c.entity(old).try_despawn();
        }
    }
}

/// The last stretch of a match before a screenshot (`main.rs`'s headless warm-up): every blast
/// and kill with how many seconds ago it happened, so the still frame shows the craters,
/// wrecks and smoke the fighting left, not only its last few ticks.
#[derive(Resource, Default)]
pub struct Aftermath(pub Vec<(Shot, f32)>);

/// How far back `Aftermath` reaches: the longest any of it stays (a crater's fade).
pub const AFTERMATH_SECS: f32 = CRATER_SECS;

/// Lays the craters of `aftermath`'s blasts, aged, and returns its kills `(at, def, age)` for
/// `view.rs` to leave aged wrecks. Fog hides what the local player can't see now.
pub fn replay_aftermath(
    c: &mut Commands,
    p: &Palette,
    sim: &Sim,
    living: &mut Living,
    aftermath: Vec<(Shot, f32)>,
) -> Vec<(SimVec2, g2_sim::DefId, f32)> {
    let vision = sim.world.vision(sim.local);
    let seen = |at: SimVec2| !sim.fog || vision.is_visible(at);
    let mut s = Spawner { c, p };
    let mut kills = Vec::new();
    for (shot, age) in aftermath {
        match shot {
            Shot::Blast { at, radius, .. } if seen(at) && age < CRATER_SECS => {
                let mut rng = Rng::at(at, 0xc4a7);
                crater(&mut s, living, sim.to_world(at), radius.to_f32(), age, &mut rng);
            }
            Shot::Hit { at, killed: true, target: Some(def), .. } if seen(at) => kills.push((at, def, age)),
            _ => {}
        }
    }
    kills
}

/// Where a ground unit is driving and how fast, from its sim state: toward its next waypoint
/// (or straight at its order's goal), at its pace; `None` standing still. For a still frame,
/// which has no previous position to compare with.
fn heading(sim: &Sim, id: g2_sim::EntityId) -> Option<(Vec3, f32)> {
    use g2_sim::Order;
    let e = sim.world.entity(id)?;
    let def = sim.world.rules.def(e.def);
    let (goal, pace) = match e.order {
        Order::Move { target, pace } => (e.route.waypoints.last().copied().unwrap_or(target), pace),
        Order::AttackMove { dest, pace, engaged: None } => (e.route.waypoints.last().copied().unwrap_or(dest), pace),
        Order::Idle => return None,
        _ => (*e.route.waypoints.last()?, def.speed()),
    };
    let to = crate::sim::to_world(goal) - crate::sim::to_world(e.pos);
    let dist = to.length();
    (dist > 1.5).then(|| (to / dist, pace.to_f32() * g2_sim::TICKS_PER_SECOND as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(name: &str) -> &'static g2_sim::Def {
        let rules = g2_rules::rules();
        rules.def(rules.find(name).unwrap_or_else(|| panic!("no {name}")))
    }

    /// Two-thirds and one-third of maximum hit points pick the damaged and burning sets, exact
    /// at the boundary (integers, no rounding), and only buildings smoke before they burn.
    #[test]
    fn damage_thresholds_pick_the_emitter_set() {
        assert_eq!(Harm::of(900, 900), Harm::Whole);
        assert_eq!(Harm::of(600, 900), Harm::Whole, "exactly two-thirds is still whole");
        assert_eq!(Harm::of(599, 900), Harm::Damaged);
        assert_eq!(Harm::of(300, 900), Harm::Damaged, "exactly one-third is still only damaged");
        assert_eq!(Harm::of(299, 900), Harm::Burning);
        assert_eq!(Harm::of(0, 900), Harm::Whole, "the dead are wrecks, not burning buildings");
        assert_eq!(harm_emitters(Harm::Whole, true), &[] as &[Emitter]);
        assert_eq!(harm_emitters(Harm::Damaged, true), &[Emitter::Wisp]);
        assert_eq!(harm_emitters(Harm::Burning, true), &[Emitter::Blaze]);
        assert_eq!(harm_emitters(Harm::Damaged, false), &[] as &[Emitter]);
        assert_eq!(harm_emitters(Harm::Burning, false), &[Emitter::Trail]);
    }

    /// A building's smoke comes from the same one or two roof points every time, inside its
    /// footprint, and buildings differ.
    #[test]
    fn roof_points_are_stable_and_on_the_roof() {
        let id = |index| g2_sim::EntityId { index, generation: 0 };
        for k in 0..50 {
            let a = roof_points(id(k), 8.0, 6.0);
            assert_eq!(a, roof_points(id(k), 8.0, 6.0));
            assert!((1..=2).contains(&a.len()));
            for p in &a {
                assert!(p.with_y(0.0).length() <= 8.0 * 0.45 + 1e-4 && p.y == 6.0, "{p}");
            }
        }
        let distinct: std::collections::BTreeSet<_> =
            (0..50).map(|k| (roof_points(id(k), 8.0, 6.0)[0].x * 100.0) as i32).collect();
        assert!(distinct.len() > 25, "points vary between buildings: {}", distinct.len());
    }

    /// Tanks and constructors lay tread marks, trucks and fast vehicles only dust, squads a
    /// little dust, aircraft nothing; standing still throws none.
    #[test]
    fn gaits_follow_the_models() {
        assert_eq!(Gait::of(def("EU Aegis Tank")), Gait::Tracked);
        assert_eq!(Gait::of(def("EU Bulldozer")), Gait::Tracked);
        assert_eq!(Gait::of(def("EU Howitzer")), Gait::Tracked);
        assert_eq!(Gait::of(def("GLA Technical")), Gait::Wheeled);
        assert_eq!(Gait::of(def("APA Harvester")), Gait::Wheeled);
        assert_eq!(Gait::of(def("EU Recon Squad")), Gait::Foot);
        assert_eq!(Gait::of(def("EU Valkyrie")), Gait::Airborne);
        for g in [Gait::Tracked, Gait::Wheeled, Gait::Foot, Gait::Airborne] {
            assert_eq!(dust_rate(g, 0.0), 0.0);
            assert!(dust_rate(g, 1e6) <= 8.0, "capped");
        }
        assert!(dust_rate(Gait::Tracked, 9.0) > dust_rate(Gait::Foot, 9.0));
        assert_eq!(dust_rate(Gait::Airborne, 20.0), 0.0);
    }

    /// Power plants and the GLA workshop steam in both looks; the alpha's derricks have flare
    /// stacks, ours none; barracks and command centers never steam.
    #[test]
    fn stacks_steam_where_the_models_have_them() {
        assert_eq!(stacks(def("EU Power Plant"), false).len(), 2);
        assert_eq!(stacks(def("EU Power Plant"), true).len(), 2);
        assert_eq!(stacks(def("APA Power Plant"), true).len(), 1);
        assert_eq!(stacks(def("GLA War Factory"), false).len(), 2);
        assert_eq!(stacks(def("GLA War Factory"), true).len(), 1);
        assert!(stacks(def("EU Oil Derrick"), false).is_empty());
        assert_eq!(stacks(def("EU Oil Derrick"), true).len(), 1);
        for name in ["EU Barracks", "EU Command Center", "GLA Barracks", "EU Aegis Tank"] {
            assert!(stacks(def(name), false).is_empty() && stacks(def(name), true).is_empty(), "{name}");
        }
    }

    fn app() -> App {
        let players = crate::sim::Sim::players([g2_sim::Faction::Eu, g2_sim::Faction::Gla], true);
        let mut sim = crate::sim::Sim::new(g2_rules::crater_ridge(), None, 1, players);
        sim.fog = false;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Image>()
            .insert_resource(sim)
            .add_systems(Startup, setup);
        app.update();
        app
    }

    /// However many craters and tread marks are laid, the pools hold at their caps and drop
    /// the oldest first.
    #[test]
    fn mark_pools_hold_their_caps() {
        let mut app = app();
        let world = app.world_mut();
        let mut living = world.remove_resource::<Living>().unwrap();
        let palette = world.remove_resource::<Palette>().unwrap();
        let sim = world.remove_resource::<crate::sim::Sim>().unwrap();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut queue, world);
        let mut s = Spawner { c: &mut commands, p: &palette };
        let mut rng = Rng(7);
        for k in 0..CRATERS * 2 {
            crater(&mut s, &mut living, Vec3::new(k as f32, 0.0, 0.0), 4.0, 0.0, &mut rng);
        }
        for k in 0..TREAD_MARKS + 50 {
            lay_tread(&mut s, &mut living, &sim, Vec3::new(0.0, 0.0, k as f32), 0.0, 3.0, 0.0);
        }
        queue.apply(world);
        assert_eq!(living.craters.len(), CRATERS);
        assert_eq!(living.treads.len(), TREAD_MARKS);
        let marks = world.query_filtered::<&Transform, With<Mark>>().iter(world).count();
        assert_eq!(marks, CRATERS + TREAD_MARKS, "older marks were despawned");
        // The newest stay: the last crater laid is at x = 2 * CRATERS - 1.
        let far =
            world.query_filtered::<&Transform, With<Mark>>().iter(world).fold(0.0f32, |m, t| m.max(t.translation.x));
        assert_eq!(far, (CRATERS * 2 - 1) as f32);
    }

    /// The ambient effects never keep more than `AMBIENT_PUFFS` alive, however much is burning:
    /// a whole army of burning buildings primes and then runs at the cap, sand blowing too.
    #[test]
    fn ambient_puffs_hold_their_cap() {
        let mut app = app();
        let world = app.world_mut();
        let mut living = world.remove_resource::<Living>().unwrap();
        let palette = world.remove_resource::<Palette>().unwrap();
        let mut sim = world.remove_resource::<crate::sim::Sim>().unwrap();
        // Every wreck a burning building's, twice the cap's worth.
        for k in 0..AMBIENT_PUFFS {
            living.wreck(Vec3::new(k as f32, 0.0, 0.0), 6.0, true, 1.0, 70.0);
        }
        sim.speed = 1;
        let focus = Vec3::new(150.0, 0.0, 150.0);
        let count = |world: &mut World| world.query_filtered::<(), With<Ambient>>().iter(world).count();
        for frame in 0..20 {
            sim.world.step();
            let ambient = count(world);
            let mut queue = bevy::ecs::world::CommandQueue::default();
            let mut commands = Commands::new(&mut queue, world);
            live(&mut commands, &palette, &sim, &mut living, 0.1, false, &|_| false, &|_| None, ambient, Some(focus));
            queue.apply(world);
            assert!(count(world) <= AMBIENT_PUFFS, "frame {frame}: {} alive", count(world));
        }
        assert!(count(world) > AMBIENT_PUFFS / 2, "the cap is reached, not starved: {}", count(world));
    }

    /// Sand blows only off loose ground (most off sand, never off snow, rock or pavement), its
    /// gusts come from the tick alone, and over a minute of the built-in savanna a few rise
    /// around the camera, each inside the map and near the focus.
    #[test]
    fn sand_blows_off_loose_ground_by_the_tick() {
        use crate::terrain::Ground;
        assert_eq!(lift(Some(Ground::Snow)), 0.0);
        assert_eq!(lift(Some(Ground::Rock)), 0.0);
        assert_eq!(lift(Some(Ground::Paved)), 0.0);
        assert!(lift(Some(Ground::Sand)) > lift(Some(Ground::Dirt)));
        assert!(lift(Some(Ground::Dirt)) > lift(Some(Ground::Grass)));
        let mut app = app();
        let sim = app.world_mut().remove_resource::<crate::sim::Sim>().unwrap();
        let focus = Vec3::new(200.0, 0.0, 200.0);
        let minute = 60 * g2_sim::TICKS_PER_SECOND;
        let gusts = sand_gusts(&sim, focus, minute, 60.0);
        let again = sand_gusts(&sim, focus, minute, 60.0);
        assert_eq!(gusts.len(), again.len());
        assert!(gusts.iter().zip(&again).all(|(a, b)| a.0 == b.0 && a.1 == b.1), "the same gusts for the same ticks");
        assert!((5..=120).contains(&gusts.len()), "a few gusts a minute: {}", gusts.len());
        for (at, age, strength, _) in &gusts {
            let off = (*at - focus).abs();
            assert!(off.x <= SAND_REACH + 8.0 && off.z <= SAND_REACH + 8.0, "{at}");
            assert!((0.0..=60.0).contains(age) && (0.0..=1.0).contains(strength));
        }
    }

    /// A puff primed `age` seconds into its life is where a fresh one would be after that long.
    #[test]
    fn a_primed_puff_matches_one_that_aged() {
        let mut app = app();
        let world = app.world_mut();
        let palette = world.remove_resource::<Palette>().unwrap();
        let p = Particle {
            age: 0.0,
            life: 5.0,
            vel: Vec3::Y * 3.0,
            drag: 1.2,
            gravity: -0.8,
            size: (Vec3::ONE, Vec3::splat(4.0)),
            fade: Fade::Soot,
            spin: 0.6,
            ground: 0.0,
        };
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut queue, world);
        let mut s = Spawner { c: &mut commands, p: &palette };
        let primed = s.card(Vec3::ZERO, p, 2.0).id();
        queue.apply(world);
        let (mut q, mut tf) = (p, Transform::from_translation(Vec3::ZERO));
        for _ in 0..60 {
            q.age += TICK_SECS;
            q.advance(&mut tf, TICK_SECS);
        }
        let got = world.get::<Transform>(primed).unwrap();
        assert!(got.translation.distance(tf.translation) < 1e-3, "{} vs {}", got.translation, tf.translation);
        assert!((world.get::<Particle>(primed).unwrap().age - 2.0).abs() < 1e-4);
    }
}
