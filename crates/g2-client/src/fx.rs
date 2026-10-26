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
    /// kill leaves (`view.rs` wrecks).
    pub decal: Handle<Mesh>,
    pub scorch: Handle<StandardMaterial>,
    tracks: [(Fade, [Handle<StandardMaterial>; STAGES]); 9],
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
        // Each puff starts at its own roll, so a cluster of them doesn't repeat one shape.
        let roll = (at.x * 12.9898 + at.z * 78.233).sin() * 43_758.547;
        let p = Particle {
            age: 0.0,
            life,
            vel,
            drag: 2.2,
            gravity,
            size: (Vec3::splat(size.0), Vec3::splat(size.1)),
            fade,
            spin: if matches!(fade, Fade::Smoke | Fade::Dust | Fade::Fumes) { 0.6 } else { 1.5 },
            ground: 0.0,
        };
        self.c.spawn((
            crate::menu::MatchEntity,
            Mesh3d(self.p.card.clone()),
            MeshMaterial3d(self.p.stage(fade, 0.0)),
            Transform::from_translation(at).with_scale(p.size.0),
            p,
            Billboard(roll.fract() * std::f32::consts::TAU),
            NotShadowCaster,
            NotShadowReceiver,
        ));
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
/// `view.rs`, which owns wrecks.
pub fn spawn_shots(
    c: &mut Commands,
    p: &Palette,
    sim: &Sim,
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
                let (a, b) = (height(from, air.0) + Vec3::Y * 1.6, height(at, air.1) + Vec3::Y * 1.2);
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
    let (mut a, b) = (sim.to_world(from) + Vec3::Y * 2.5, sim.to_world(at) + Vec3::Y * 0.5);
    let whole = flight as f32 * TICK_SECS / (1.0 - done).max(0.05);
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
        Look::Shell { toxic, .. } => {
            ((a.distance(b) * 0.35).max(4.0), 0.75, 0.035, if toxic { Fade::Fumes } else { Fade::Smoke }, 0.55)
        }
        _ => ((a.distance(b) * 0.35).max(4.0), 0.75, 0.035, Fade::Smoke, 0.55),
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
        let gravity = p.gravity;
        let drag = p.drag;
        p.vel.y -= gravity * dt;
        p.vel *= (1.0 - drag * dt).max(0.0);
        tf.translation += p.vel * dt;
        // Ease-out growth: fast at first, settling as it fades.
        let ease = 1.0 - (1.0 - t) * (1.0 - t);
        tf.scale = p.size.0.lerp(p.size.1, ease);
        // Debris tumbles; puffs roll in `face_camera` instead.
        if p.spin != 0.0 && p.fade == Fade::Debris {
            tf.rotate_local_y(p.spin * dt);
        }
        // Debris lands on the ground it was thrown from and stays, shrinking away at the end.
        if p.fade == Fade::Debris {
            if tf.translation.y < p.ground {
                tf.translation.y = p.ground;
                p.vel = Vec3::ZERO;
                p.spin = 0.0;
            }
            tf.scale = p.size.0 * (1.0 - ((t - 0.7) / 0.3).clamp(0.0, 1.0));
        }
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
