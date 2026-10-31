//! Procedural surface textures for the procedural models (docs/STYLE.md, Models): painted
//! panels, deck plates, scrap, concrete and wood, generated at startup like `fx.rs`'s puff
//! sprite. Primitives alone read flat next to the alpha's footage, whose buildings show panel
//! seams, plate variation and grime; these maps put that on every part.
//!
//! Each style is three tiling maps over `TILE` metres: an albedo multiplier (its mean is divided
//! back out of the material colour, so the calibrated palette in `models::color` keeps its
//! average brightness), a normal map from a height field (seams, rivets, corrugation catch the
//! sun), and roughness/metallic. Part meshes carry UVs in metres (`part_mesh`), so a texel is the
//! same size on a railing as on a war factory wall, and a vertex-colour gradient that darkens the
//! foot of every wall where dirt collects.

use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use g2_sim::Faction;

use crate::models::{Part, Prim, Slot};

/// World metres per repeat of every surface map. 2 m panels fit twice, and at the Close zoom
/// (about 12 screen pixels a metre: a 21 m command center spans a fifth of a 1280-pixel frame)
/// a repeat spans some 50 pixels.
pub const TILE: f32 = 4.0;
/// Texels per side: 64 a metre, so a 4 cm seam is 2-3 texels before mipmapping.
const SIZE: usize = 256;

/// How a material slot's surface looks. Hull walls and panel roofs carry the faction's
/// character; vehicles get the subtler `Armour`; the rest are shared.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Style {
    Siding(Faction),
    Roof(Faction),
    Armour(Faction),
    Dark,
    Tread,
    Concrete,
    Wood,
    Crate,
    Hazard,
    Team,
}

/// The style for `slot` in `faction`'s colours (`None`, neutral, draws as the GLA palette does);
/// glow panels and oil stay plain. On a `vehicle`, hull, panel and dark metal are `Armour`.
pub fn style(faction: Option<Faction>, slot: Slot, vehicle: bool) -> Option<Style> {
    let f = faction.unwrap_or(Faction::Gla);
    Some(match slot {
        Slot::Hull | Slot::Panel | Slot::Dark if vehicle => Style::Armour(f),
        // EU panels are wall pieces (tiers, ribs, stacks) whose roofs `Slot::Deck` covers
        // (`models::roof`); the APA's and GLA's are their roof tiers, plated or tin.
        Slot::Hull => Style::Siding(f),
        Slot::Panel if f == Faction::Eu => Style::Siding(f),
        Slot::Panel | Slot::Deck => Style::Roof(f),
        Slot::Dark => Style::Dark,
        Slot::Tread => Style::Tread,
        Slot::Team => Style::Team,
        Slot::Hazard => Style::Hazard,
        Slot::Concrete => Style::Concrete,
        Slot::Wood => Style::Wood,
        Slot::Crate(_) => Style::Crate,
        Slot::Glow | Slot::Oil => return None,
    })
}

/// One style's maps and the linear mean of its albedo, which the material colour is divided by.
pub struct Maps {
    pub albedo: Image,
    pub normal: Image,
    /// Roughness in green, metallic in blue (glTF's layout, which `StandardMaterial` reads).
    pub surface: Image,
    pub mean: Vec3,
}

/// What one texel of a style is: an albedo multiplier, a height in metres for the normal map,
/// and multipliers on the slot's roughness and metallic.
struct Texel {
    shade: Vec3,
    height: f32,
    rough: f32,
    metal: f32,
}

impl Texel {
    fn grey(tone: f32, height: f32, rough: f32, metal: f32) -> Texel {
        Texel { shade: Vec3::splat(tone), height, rough, metal }
    }
}

/// Builds the maps of `style` around the slot's `roughness` and `metallic`.
pub fn maps(style: Style, roughness: f32, metallic: f32) -> Maps {
    let texels: Vec<Texel> = (0..SIZE * SIZE)
        .map(|i| {
            let (x, y) = ((i % SIZE) as f32 + 0.5, (i / SIZE) as f32 + 0.5);
            texel(style, x / SIZE as f32 * TILE, y / SIZE as f32 * TILE)
        })
        .collect();
    let top = texels.iter().map(|t| t.shade.max_element()).fold(1e-3, f32::max);
    let mut albedo = Vec::with_capacity(SIZE * SIZE * 4);
    let mut sum = Vec3::ZERO;
    for t in &texels {
        // Stored linear (`Rgba8Unorm`): the maps only ever multiply, and box-filtered mips of
        // linear values keep the mean the material is calibrated against.
        let s = (t.shade / top).clamp(Vec3::ZERO, Vec3::ONE);
        let q = (s * 255.0).round();
        sum += q / 255.0;
        albedo.extend_from_slice(&[q.x as u8, q.y as u8, q.z as u8, 255]);
    }
    let mean = sum / (SIZE * SIZE) as f32;
    let texel_m = TILE / SIZE as f32;
    let h = |x: usize, y: usize| texels[(y % SIZE) * SIZE + x % SIZE].height;
    let mut normal = Vec::with_capacity(SIZE * SIZE * 4);
    let mut surface = Vec::with_capacity(SIZE * SIZE * 4);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = (h(x + 1, y) - h(x + SIZE - 1, y)) / (2.0 * texel_m);
            let dy = (h(x, y + 1) - h(x, y + SIZE - 1)) / (2.0 * texel_m);
            let n = Vec3::new(-dx, -dy, 1.0).normalize() * 0.5 + 0.5;
            normal.extend_from_slice(&[(n.x * 255.0) as u8, (n.y * 255.0) as u8, (n.z * 255.0) as u8, 255]);
            let t = &texels[y * SIZE + x];
            let r = (roughness * t.rough).clamp(0.05, 1.0);
            let m = (metallic * t.metal).clamp(0.0, 1.0);
            surface.extend_from_slice(&[255, (r * 255.0) as u8, (m * 255.0) as u8, 255]);
        }
    }
    Maps { albedo: image(albedo), normal: image(normal), surface: image(surface), mean }
}

/// A repeating, mipmapped, anisotropically filtered map: walls are seen at grazing angles and
/// shrink to a few pixels a metre at the far zoom, where unfiltered seams would shimmer.
fn image(data: Vec<u8>) -> Image {
    let mut image = Image::new(
        Extent3d { width: SIZE as u32, height: SIZE as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    crate::terrain::mip_chain(&mut image);
    let mut sampler = ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    };
    sampler.set_anisotropic_filter(16);
    image.sampler = ImageSampler::Descriptor(sampler);
    image
}

/// One texel of `style` at `(x, y)` metres into the tile; `y` runs down a wall.
fn texel(style: Style, x: f32, y: f32) -> Texel {
    let (u, v) = (x / TILE, y / TILE);
    // Rain and dirt streaks: thin vertical runs, strongest just under the joint above.
    let streak = |cells: i32, seed: u32| smoothstep(0.55, 0.9, noise(u, v, cells, 2, seed));
    match style {
        // Painted siding in tall 1 m panels with a joint every 2 m. The alpha's EU halls are
        // kept clean: only faint rain streaks under each joint.
        Style::Siding(Faction::Eu) => {
            let seam = groove(x, 1.0, 0.025, 0.03).min(groove(y, 2.0, 0.03, 0.035));
            let panel = hash((x / 1.0) as i32, (y / 2.0) as i32, 11);
            let drip = streak(48, 12) * (-y.rem_euclid(2.0) / 0.8).exp();
            let tone =
                1.0 + (panel - 0.5) * 0.06 + (fbm(u, v, 4, 3, 13) - 0.5) * 0.06 - drip * 0.1 - (1.0 - seam) * 0.25;
            Texel::grey(tone, seam * 0.012, 1.0 + (1.0 - seam) * 0.3 + drip * 0.15, 1.0)
        }
        // Dark steel: staggered 2 × 1 m plates with deep joints and a row of rivets along each,
        // some plates shinier than others.
        Style::Siding(Faction::Apa) => {
            let row = (y / 1.0).floor();
            let xo = (x + if row as i32 % 2 == 1 { 1.0 } else { 0.0 }).rem_euclid(TILE);
            let seam = groove(xo, 2.0, 0.035, 0.04).min(groove(y, 1.0, 0.035, 0.04));
            let plate = hash((xo / 2.0) as i32, row as i32, 21);
            let rivet = rivets(xo, y, 1.0, 0.12, 0.2);
            let drip = streak(40, 22) * (-y.rem_euclid(1.0) / 0.6).exp();
            let tone =
                1.0 + (plate - 0.5) * 0.14 + (fbm(u, v, 4, 3, 23) - 0.5) * 0.06 - drip * 0.14 - (1.0 - seam) * 0.35
                    + rivet * 0.08;
            Texel::grey(
                tone,
                seam * 0.015 + rivet * 0.006,
                1.0 + (plate - 0.5) * 0.3 + (1.0 - seam) * 0.35 + drip * 0.2,
                1.0 + (plate - 0.5) * 0.3,
            )
        }
        // Scrap: mismatched sheets in rows, each row shifted, rust spreading in blotches that
        // are worse on some sheets, and heavy grime run down the whole face.
        Style::Siding(_) => {
            let row = (y / 1.0).floor() as i32;
            let xo = (x + hash(row, 0, 31) * TILE).rem_euclid(TILE);
            let w = TILE / 3.0;
            let col = (xo / w) as i32;
            let seam = groove(xo, w, 0.02, 0.03).min(groove(y, 1.0, 0.02, 0.03));
            let sheet = hash(col, row, 33);
            let rust = smoothstep(0.42, 0.68, fbm(u, v, 4, 4, 35) * 0.8 + sheet * 0.35 - 0.1);
            let grime = streak(40, 36) * 0.16 + streak(24, 38) * (-y.rem_euclid(1.0) / 0.5).exp() * 0.2;
            let rivet = rivets(xo, y, 1.0, 0.08, 0.3);
            let tone = 1.0 + (sheet - 0.5) * 0.22 - grime - (1.0 - seam) * 0.3 + rivet * 0.05;
            Texel {
                shade: tone * Vec3::new(0.97, 0.99, 1.03).lerp(Vec3::new(1.06, 0.8, 0.62), rust),
                height: seam * 0.01 + hash(col, row, 34) * 0.006 + fbm(u, v, 8, 2, 37) * 0.01 + rivet * 0.005,
                rough: 1.1 + rust * 0.35,
                metal: 1.0 - rust * 0.8,
            }
        }
        // Deck plates 2 × 1 m; about two in five a dark grille of slats, picked per plate so the
        // roof isn't a chessboard. The footage's EU roofs are runs of dark grating between
        // lighter plates, framed in orange trim (`models::roof`).
        Style::Roof(Faction::Eu) => {
            let seam = groove(x, 2.0, 0.04, 0.04).min(groove(y, 1.0, 0.04, 0.04));
            let (px, py) = ((x / 2.0) as i32, y as i32);
            let (tone, height, rough) = if hash(px, py, 43) < 0.4 {
                let slat = groove(x, 0.25, 0.04, 0.03);
                (0.62 + slat * 0.14, slat * 0.012, 1.2 - slat * 0.2)
            } else {
                (1.0 + (hash(px, py, 41) - 0.5) * 0.1 + (fbm(u, v, 4, 3, 42) - 0.5) * 0.06, 0.012, 1.0)
            };
            Texel::grey(tone - (1.0 - seam) * 0.3, height * seam, rough + (1.0 - seam) * 0.3, 1.0)
        }
        // Heavy deck plates bolted along every joint, with a low diamond tread.
        Style::Roof(Faction::Apa) => {
            let seam = groove(x, 2.0, 0.04, 0.04).min(groove(y, 2.0, 0.04, 0.04));
            let plate = hash((x / 2.0) as i32, (y / 2.0) as i32, 51);
            let bolt = rivets(x, y, 2.0, 0.14, 0.25).max(rivets(y, x, 2.0, 0.14, 0.25));
            // Diamond tread: raised dashes on a 0.25 m lattice, alternating in direction.
            let (a, b) = ((x / 0.25).fract() - 0.5, (y / 0.25).fract() - 0.5);
            let flip = if ((x / 0.25) as i32 + (y / 0.25) as i32) % 2 == 0 { 1.0 } else { -1.0 };
            let along = (a + flip * b) * std::f32::consts::FRAC_1_SQRT_2;
            let across = (a - flip * b) * std::f32::consts::FRAC_1_SQRT_2;
            let dash = (1.0 - smoothstep(0.03, 0.06, across.abs())) * (1.0 - smoothstep(0.12, 0.16, along.abs()));
            let tone =
                1.0 + (plate - 0.5) * 0.1 + (fbm(u, v, 4, 3, 52) - 0.5) * 0.08 - (1.0 - seam) * 0.35 + bolt * 0.08;
            Texel::grey(
                tone,
                seam * 0.015 + dash * 0.003 + bolt * 0.006,
                1.0 + (plate - 0.5) * 0.25 - dash * 0.1 + (1.0 - seam) * 0.35,
                1.0 + (plate - 0.5) * 0.2,
            )
        }
        // Corrugated sheet laid in overlapping strips, ridges every 0.25 m, dirt in the troughs
        // and rust like the walls.
        Style::Roof(_) => {
            let w = TILE / 3.0;
            let strip = (y / w).floor() as i32;
            let lap = groove(y, w, 0.02, 0.03);
            let ridge = 0.5 + 0.5 * (TAU * x / 0.25).cos();
            let sheet = hash(strip, (x / 2.0) as i32, 61);
            let rust = smoothstep(0.45, 0.7, fbm(u, v, 4, 4, 62) * 0.8 + sheet * 0.3 - 0.08);
            let tone = 1.0 + (sheet - 0.5) * 0.16 - (1.0 - ridge) * 0.1 - streak(32, 63) * 0.12 - (1.0 - lap) * 0.3;
            Texel {
                shade: tone * Vec3::new(0.97, 0.99, 1.03).lerp(Vec3::new(1.06, 0.8, 0.62), rust),
                height: ridge * 0.01 + lap * 0.008,
                rough: 1.1 + rust * 0.35,
                metal: 1.0 - rust * 0.8,
            }
        }
        // Vehicle armour: a vehicle is a few metres long, so building-scale plates would chop it
        // into bricks. Thin panel lines every 1.33 m, plate-to-plate tone, a mottle of wear and
        // dust; the GLA's with rust patches.
        Style::Armour(f) => {
            let w = TILE / 3.0;
            let seam = groove(x, w, 0.015, 0.02).min(groove(y, w, 0.015, 0.02));
            let plate = hash((x / w) as i32, (y / w) as i32, 131);
            let mottle = fbm(u, v, 8, 3, 132);
            let dust = streak(48, 133) * 0.1;
            let rust = if f == Faction::Gla { smoothstep(0.55, 0.7, fbm(u, v, 8, 3, 134)) } else { 0.0 };
            let tone = 1.0 + (plate - 0.5) * 0.12 + (mottle - 0.5) * 0.18 - dust - (1.0 - seam) * 0.3;
            Texel {
                shade: tone * Vec3::ONE.lerp(Vec3::new(1.06, 0.8, 0.62), rust),
                height: seam * 0.008 + mottle * 0.004,
                rough: 1.0 + (mottle - 0.5) * 0.4 + (1.0 - seam) * 0.3 + rust * 0.35,
                metal: 1.0 - rust * 0.8,
            }
        }
        // Frames, vents and tracks: dark plates with faint joints and patchy wear.
        Style::Dark => {
            let seam = groove(x, 2.0, 0.03, 0.04).min(groove(y, 1.0, 0.03, 0.04));
            let plate = hash((x / 2.0) as i32, (y / 1.0) as i32, 71);
            let tone = 1.0 + (plate - 0.5) * 0.1 + (fbm(u, v, 6, 3, 72) - 0.5) * 0.12 - (1.0 - seam) * 0.25;
            Texel::grey(tone, seam * 0.01, 1.0 + (fbm(u, v, 5, 3, 73) - 0.5) * 0.4, 1.0)
        }
        // Track links `TREAD_PITCH` long, each with a raised cleat across its middle, so the
        // run shows as the band scrolls (`tread_mesh`).
        Style::Tread => {
            let joint = groove(y, TREAD_PITCH, 0.03, 0.03);
            let cleat = 1.0 - smoothstep(0.12, 0.18, (y.rem_euclid(TREAD_PITCH) - TREAD_PITCH / 2.0).abs());
            let wear = fbm(u, v, 8, 3, 141);
            let tone = 0.8 + cleat * 0.7 + (wear - 0.5) * 0.15 - (1.0 - joint) * 0.4;
            Texel::grey(tone, joint * 0.02 + cleat * 0.02, 1.0 - cleat * 0.3, 1.0 + cleat * 0.8)
        }
        // Cast slabs: a joint every 2 m, broad water stains, darker oil and dirt blotches, grit.
        Style::Concrete => {
            let joint = groove(x, 2.0, 0.02, 0.02).min(groove(y, 2.0, 0.02, 0.02));
            let slab = hash((x / 2.0) as i32, (y / 2.0) as i32, 81);
            let blotch = smoothstep(0.58, 0.75, fbm(u, v, 5, 3, 82));
            let grit = noise(u, v, 128, 128, 83);
            let tone = 1.0 + (fbm(u, v, 3, 4, 84) - 0.5) * 0.18 + (slab - 0.5) * 0.06 - blotch * 0.14
                + (grit - 0.5) * 0.08
                - (1.0 - joint) * 0.25;
            Texel::grey(tone, joint * 0.008 + grit * 0.002, 1.0 + (grit - 0.5) * 0.1 - blotch * 0.15, 1.0)
        }
        // Boards 0.25 m wide with dark gaps, each its own tone, grain running along them.
        Style::Wood => {
            let board = (y / 0.25) as i32;
            let gap = groove(y, 0.25, 0.012, 0.015);
            let tint = hash(board, 0, 91);
            let grain = noise(u + tint, v, 6, 128, 92);
            let tone = 1.0 + (tint - 0.5) * 0.2 + (grain - 0.5) * 0.16 - (1.0 - gap) * 0.4;
            Texel::grey(tone, gap * 0.006 + grain * 0.002, 1.0 + (grain - 0.5) * 0.1, 1.0)
        }
        // Crates, tarps, sandbags and tin: blotchy wear and a little dirt run down.
        Style::Crate => {
            let tone = 1.0 + (fbm(u, v, 4, 4, 101) - 0.5) * 0.16 - streak(32, 102) * 0.08
                + (noise(u, v, 64, 64, 103) - 0.5) * 0.06;
            Texel::grey(tone, fbm(u, v, 8, 3, 104) * 0.01, 1.0, 1.0)
        }
        // Chipped paint: small dark flakes where it has worn through, and faint streaks. The
        // flakes stay under 10 cm, since trim strips are only 0.4 m wide.
        Style::Hazard => {
            let chip = smoothstep(0.7, 0.76, fbm(u, v, 32, 3, 111));
            let tone = (1.0 + (fbm(u, v, 3, 3, 112) - 0.5) * 0.06 - streak(40, 113) * 0.06) * (1.0 - chip * 0.4);
            Texel::grey(tone, (1.0 - chip) * 0.003, 1.0 + chip * 0.3, 1.0)
        }
        // Team colour stays nearly flat so sides read at a glance: panel joints in the normal
        // map and a faint mottle only.
        Style::Team => {
            let seam = groove(x, 1.0, 0.02, 0.03).min(groove(y, 2.0, 0.02, 0.03));
            let tone = 1.0 + (fbm(u, v, 4, 3, 121) - 0.5) * 0.04 - (1.0 - seam) * 0.12;
            Texel::grey(tone, seam * 0.008, 1.0 + (1.0 - seam) * 0.2, 1.0)
        }
    }
}

/// Height across joints repeating every `period` metres: 0 in a groove `half` either side of
/// the line, rising over `bevel` to 1 on the plate.
fn groove(t: f32, period: f32, half: f32, bevel: f32) -> f32 {
    let d = (t - (t / period).round() * period).abs();
    smoothstep(half, half + bevel, d)
}

/// Rivet bumps (0..1) in a row `inset` metres inside each joint that repeats every `period`
/// along `y`, one every `pitch` along `x`.
fn rivets(x: f32, y: f32, period: f32, inset: f32, pitch: f32) -> f32 {
    let dy = (y - (y / period).round() * period).abs() - inset;
    let dx = x - (x / pitch).round() * pitch;
    1.0 - smoothstep(0.015, 0.03, (dx * dx + dy * dy).sqrt())
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hash(a: i32, b: i32, seed: u32) -> f32 {
    let mut x =
        (a as u32).wrapping_mul(0x8da6_b343) ^ (b as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    x ^= x >> 13;
    x = x.wrapping_mul(0x5bd1_e995);
    x ^= x >> 15;
    (x & 0xffff) as f32 / 65535.0
}

/// Value noise over the tile (`u`, `v` in 0..1) on a `cx × cy` lattice that wraps, so every map
/// repeats without a seam. Unequal cell counts stretch it, as streaks are.
fn noise(u: f32, v: f32, cx: i32, cy: i32, seed: u32) -> f32 {
    let (fx, fy) = (u * cx as f32, v * cy as f32);
    let (xi, yi) = (fx.floor() as i32, fy.floor() as i32);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (tx, ty) = (s(fx - xi as f32), s(fy - yi as f32));
    let g = |a: i32, b: i32| hash(a.rem_euclid(cx), b.rem_euclid(cy), seed);
    let top = g(xi, yi) + (g(xi + 1, yi) - g(xi, yi)) * tx;
    let bot = g(xi, yi + 1) + (g(xi + 1, yi + 1) - g(xi, yi + 1)) * tx;
    top + (bot - top) * ty
}

/// `octaves` of `noise`, from `cells` per tile up, normalised to 0..1.
fn fbm(u: f32, v: f32, cells: i32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut norm) = (0.0, 1.0, 0.0);
    for o in 0..octaves {
        let c = cells << o;
        sum += noise(u, v, c, c, seed + o) * amp;
        norm += amp;
        amp *= 0.5;
    }
    sum / norm
}

/// Parts are drawn at their size by scaling a unit mesh; parts whose sizes round to the same
/// `QUANTUM` share one, so a map is stretched by at most 12 cm over a part's length (noticeable
/// only on trim thinner than that, where no joint is visible anyway).
const QUANTUM: f32 = 0.25;

/// The mesh a part is drawn with: its primitive and its size in `QUANTUM` steps.
pub fn key(p: &Part) -> (u8, [i32; 3]) {
    let q = |s: f32| ((s / QUANTUM).round() as i32).max(1);
    (p.prim as u8, [q(p.size.x), q(p.size.y), q(p.size.z)])
}

/// Metres from one track link's cleat to the next; a whole number of them fit `TILE`, so the
/// map repeats cleanly. Coarser than a real track's, so the run reads at a tank's speed: a
/// Scorpion covers 0.37 m a sim tick, and a pattern that moves more than half its repeat
/// between frames seems to run backwards.
pub const TREAD_PITCH: f32 = 1.0;
/// Steps a track band's run is drawn in, each a mesh with its map moved on by a share of a
/// link (`tread_mesh`): moving the UVs, not the material, keeps every tread on one material.
pub const TREAD_PHASES: usize = 8;

/// A track band's mesh at `phase` of its run: a unit box like `box_mesh`, its map laid along
/// the band's length (local Z) on every face, moved on by `phase` steps of `TREAD_PITCH /
/// TREAD_PHASES`. Driving forward, the top run moves forward over the hull and the rest (the
/// run on the ground, seen on the sides and underneath) backward, as a track's links do.
pub fn tread_mesh(key: (u8, [i32; 3]), phase: usize) -> Mesh {
    let size = Vec3::new(key.1[0] as f32, key.1[1] as f32, key.1[2] as f32) * QUANTUM;
    let shift = phase as f32 * TREAD_PITCH / TREAD_PHASES as f32;
    let (mut positions, mut normals, mut uvs) = (Vec::new(), Vec::new(), Vec::new());
    let mut indices = Vec::<u32>::new();
    // Each face: its normal and the axis across the band in it (`across × n` runs along +Z on
    // the top), its width along that axis, and which way its links run.
    for (n, across, width, run) in [
        (Vec3::Y, Vec3::NEG_X, size.x, -1.0),
        (Vec3::NEG_Y, Vec3::X, size.x, 1.0),
        (Vec3::X, Vec3::Y, size.y, 1.0),
        (Vec3::NEG_X, Vec3::NEG_Y, size.y, 1.0),
    ] {
        let along = n.cross(across);
        let i = positions.len() as u32;
        for (s, a) in [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)] {
            positions.push((n * 0.5 + across * s + along * a).to_array());
            normals.push(n.to_array());
            uvs.push([(s + 0.5) * width / TILE, ((a + 0.5) * size.z + run * shift) / TILE]);
        }
        indices.extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
    }
    // The ends: plain, the links seen edge on.
    for n in [Vec3::Z, Vec3::NEG_Z] {
        let (across, up) = (Vec3::Y.cross(n), Vec3::Y);
        let i = positions.len() as u32;
        for (s, y) in [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)] {
            positions.push((n * 0.5 + across * s + up * y).to_array());
            normals.push(n.to_array());
            uvs.push([(s + 0.5) * size.x / TILE, (0.5 - y) * size.y / TILE]);
        }
        indices.extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
    }
    let colors = vec![[1.0f32; 4]; positions.len()];
    let mut mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(bevy::mesh::Indices::U32(indices));
    mesh.generate_tangents().expect("a triangle list with normals and UVs");
    mesh
}

/// The unit mesh of `key`'s primitive, with UVs in `TILE`s for its size and tangents for the
/// normal maps. Wall faces run `v` downward from their top edge, so streaks hang from the roof
/// line. Vertex colours darken the foot of every wall, fading out over its lowest metre: the
/// dirt line along the ground the footage's buildings and vehicles show.
pub fn part_mesh(key: (u8, [i32; 3])) -> Mesh {
    let size = Vec3::new(key.1[0] as f32, key.1[1] as f32, key.1[2] as f32) * QUANTUM;
    if key.0 == Prim::Box as u8 {
        return box_mesh(size);
    }
    let mut mesh = if key.0 == Prim::Cylinder as u8 {
        // Rings every half metre or so (8 at most), so the dirt line has vertices to fade over.
        let rings = (size.y / 0.5).ceil().clamp(1.0, 8.0) as u32;
        Cylinder::new(0.5, 1.0).mesh().segments(rings).build()
    } else {
        Sphere::new(0.5).mesh().ico(2).expect("ico 2 is in range")
    };
    let positions =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION).and_then(VertexAttributeValues::as_float3).unwrap().to_vec();
    let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL).cloned() else {
        unreachable!("primitive meshes have normals")
    };
    let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0).cloned() else {
        unreachable!("primitive meshes have UVs")
    };
    // Whole tiles around anything wider than one, so the wrap seam lines up; a thin pole keeps
    // the true scale, where a seam can't be seen.
    let around = std::f32::consts::PI * size.x / TILE;
    let around = if around > 1.0 { around.round() } else { around };
    let mut out_uv = Vec::with_capacity(positions.len());
    let mut colors = Vec::with_capacity(positions.len());
    for ((p, n), uv) in positions.iter().zip(&normals).zip(&uvs) {
        let (p, n) = (Vec3::from(*p), Vec3::from(*n));
        let cap = key.0 == Prim::Cylinder as u8 && n.y.abs() > 0.5;
        out_uv.push(if cap {
            [p.x * size.x / TILE, p.z * size.z / TILE]
        } else if key.0 == Prim::Cylinder as u8 {
            [uv[0] * around, (0.5 - p.y) * size.y / TILE]
        } else {
            [uv[0] * around, uv[1] * around / 2.0]
        });
        colors.push(if cap { [1.0; 4] } else { dirt((p.y + 0.5) * size.y) });
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, out_uv);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.generate_tangents().expect("primitive meshes are triangle lists with normals and UVs");
    mesh
}

/// The vertex colour `above` metres over a part's base: darkest at the ground.
fn dirt(above: f32) -> [f32; 4] {
    let c = 1.0 - 0.2 * (-above / 0.6).exp();
    [c, c, c, 1.0]
}

/// A unit cube whose side faces are split into rows near the base (0.3 m and 1 m up, where the
/// part is that tall), so the dirt line fades over its lowest metre instead of the whole wall.
fn box_mesh(size: Vec3) -> Mesh {
    let mut levels = vec![-0.5];
    levels.extend([0.3, 1.0].into_iter().filter(|&m| m < size.y * 0.9).map(|m| m / size.y - 0.5));
    levels.push(0.5);
    let (mut positions, mut normals, mut uvs, mut colors, mut indices) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::<u32>::new());
    let mut quad = |corners: [Vec3; 4], n: Vec3, uv: [[f32; 2]; 4], c: [[f32; 4]; 4]| {
        let i = positions.len() as u32;
        for k in 0..4 {
            positions.push(corners[k].to_array());
            normals.push(n.to_array());
            uvs.push(uv[k]);
            colors.push(c[k]);
        }
        indices.extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
    };
    // Each side: its normal and the horizontal axis that runs left to right seen from outside
    // (`t × Y = n`), with the face's width along it.
    for (n, t, width) in [
        (Vec3::Z, Vec3::X, size.x),
        (Vec3::NEG_Z, Vec3::NEG_X, size.x),
        (Vec3::X, Vec3::NEG_Z, size.z),
        (Vec3::NEG_X, Vec3::Z, size.z),
    ] {
        for w in levels.windows(2) {
            let (y0, y1) = (w[0], w[1]);
            let at = |s: f32, y: f32| n * 0.5 + t * s + Vec3::Y * y;
            let uv = |s: f32, y: f32| [(s + 0.5) * width / TILE, (0.5 - y) * size.y / TILE];
            let (c0, c1) = (dirt((y0 + 0.5) * size.y), dirt((y1 + 0.5) * size.y));
            quad(
                [at(-0.5, y0), at(0.5, y0), at(0.5, y1), at(-0.5, y1)],
                n,
                [uv(-0.5, y0), uv(0.5, y0), uv(0.5, y1), uv(-0.5, y1)],
                [c0, c0, c1, c1],
            );
        }
    }
    let flat = |x: f32, z: f32| [(x + 0.5) * size.x / TILE, (z + 0.5) * size.z / TILE];
    let (a, b, c, d) = ((-0.5, 0.5), (0.5, 0.5), (0.5, -0.5), (-0.5, -0.5));
    let top = |(x, z): (f32, f32)| Vec3::new(x, 0.5, z);
    let bottom = |(x, z): (f32, f32)| Vec3::new(x, -0.5, z);
    quad(
        [top(a), top(b), top(c), top(d)],
        Vec3::Y,
        [flat(a.0, a.1), flat(b.0, b.1), flat(c.0, c.1), flat(d.0, d.1)],
        [[1.0; 4]; 4],
    );
    quad(
        [bottom(d), bottom(c), bottom(b), bottom(a)],
        Vec3::NEG_Y,
        [flat(d.0, d.1), flat(c.0, c.1), flat(b.0, b.1), flat(a.0, a.1)],
        [[1.0; 4]; 4],
    );
    let mut mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(bevy::mesh::Indices::U32(indices));
    mesh.generate_tangents().expect("a triangle list with normals and UVs");
    mesh
}
