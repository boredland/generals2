//! Ground marks: tyre ruts worn into the roads, pale vehicle tracks across the grass and
//! scorched patches, painted once per map into a texture that covers the playable area (three
//! texels per world unit) and applied by the ground's shader (`ground.wesl`). The ground's
//! vertices are 2-3 units apart, far too coarse for a 0.4-unit tyre line; the tiling detail
//! texture repeats, so it can't carry anything placed.
//!
//! The footage's roads are light worn dirt (about sRGB 199/155/101 at the centre) braided with
//! tyre lines about 15% darker; tracks cross its grass as paler dirt lines; scorch marks are
//! near black (about sRGB 30/15/2 against ground of 102/75/23). Measured on frames 270 s and
//! 320 s of `[dtl2Q_tUDDQ]`.

use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::MaterialExtension;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;

use super::{Layout, ROAD_HALF_WIDTH, fbm, hash, mip_chain};

/// Texels per world unit: a tyre line is a little over one texel wide.
const PER_UNIT: f32 = 3.0;
/// Wheel track of the vehicles that wear the ruts, centre to centre.
const AXLE: f32 = 1.8;

/// The ground's material extension: samples the marks texture by world position.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub(super) struct GroundMarks {
    /// World origin (x, z) and size of the area the texture covers.
    #[uniform(100)]
    area: Vec4,
    /// What full "dirt" turns the ground into: a colour per unit of the ground's luminance,
    /// so the detail texture's grain carries through the tracks.
    #[uniform(100)]
    dirt: Vec4,
    /// What a full scorch multiplies the ground by.
    #[uniform(100)]
    scorch: Vec4,
    #[texture(101)]
    #[sampler(102)]
    texture: Handle<Image>,
}

pub(super) const SHADER: &str = "embedded://g2_client/terrain/ground.wesl";

impl MaterialExtension for GroundMarks {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}

/// Linear luminance of an sRGB colour.
fn luma(c: Color) -> f32 {
    let l = c.to_linear();
    0.2126 * l.red + 0.7152 * l.green + 0.0722 * l.blue
}

impl GroundMarks {
    pub fn new(layout: &Layout, texture: Handle<Image>) -> GroundMarks {
        // Tracks turn the ochre grass (`Layout::color`) into the pale dirt of the roads'
        // shoulders: the dirt colour over the grass's luminance.
        let grass = Color::srgb(0.53, 0.40, 0.21);
        let dirt = Color::srgb(0.64, 0.50, 0.32).to_linear();
        let k = 1.0 / luma(grass);
        GroundMarks {
            area: Vec4::new(0.0, 0.0, layout.width, layout.height),
            dirt: Vec4::new(dirt.red * k, dirt.green * k, dirt.blue * k, 1.0),
            // Charred ground: the footage's scorch over its ground (sRGB 30/15/2 over
            // 102/75/23) is about 0.05 per channel in linear; a touch lighter here, since the
            // grade deepens shadows further.
            scorch: Vec4::new(0.06, 0.05, 0.05, 1.0),
            texture,
        }
    }
}

/// The marks being painted, one value per texel and channel.
pub(super) struct Marks {
    w: usize,
    h: usize,
    /// Brightness factor: 1 leaves the ground as it is.
    shade: Vec<f32>,
    /// Blend toward pale dirt, 0..1.
    dirt: Vec<f32>,
    /// Blend toward char, 0..1.
    scorch: Vec<f32>,
}

#[derive(Clone, Copy)]
enum Channel {
    Shade(f32),
    Dirt(f32),
    Scorch(f32),
}

impl Marks {
    fn new(width: f32, height: f32) -> Marks {
        let (w, h) = ((width * PER_UNIT).ceil() as usize, (height * PER_UNIT).ceil() as usize);
        Marks { w, h, shade: vec![1.0; w * h], dirt: vec![0.0; w * h], scorch: vec![0.0; w * h] }
    }

    /// Paints `c` with soft edges within `half` of the segment `a`-`b`, at `strength` 0..1.
    /// Overlaps keep the strongest mark, so crossing tracks don't add up to black.
    fn stroke(&mut self, a: Vec2, b: Vec2, half: f32, c: Channel, strength: f32) {
        let (lo, hi) = (a.min(b) - Vec2::splat(half), a.max(b) + Vec2::splat(half));
        let (i0, j0) = ((lo.x * PER_UNIT).floor().max(0.0) as usize, (lo.y * PER_UNIT).floor().max(0.0) as usize);
        let (i1, j1) =
            (((hi.x * PER_UNIT).ceil() as usize).min(self.w), ((hi.y * PER_UNIT).ceil() as usize).min(self.h));
        let ab = b - a;
        let len2 = ab.length_squared().max(1e-6);
        for j in j0..j1 {
            for i in i0..i1 {
                let p = Vec2::new((i as f32 + 0.5) / PER_UNIT, (j as f32 + 0.5) / PER_UNIT);
                let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
                let d = p.distance(a + ab * t);
                if d >= half {
                    continue;
                }
                let x = 1.0 - d / half;
                let k = x * x * (3.0 - 2.0 * x) * strength;
                self.put(j * self.w + i, c, k);
            }
        }
    }

    fn put(&mut self, n: usize, c: Channel, k: f32) {
        match c {
            Channel::Shade(to) => self.shade[n] = self.shade[n].min(1.0 + (to - 1.0) * k),
            Channel::Dirt(to) => self.dirt[n] = self.dirt[n].max(to * k),
            Channel::Scorch(to) => self.scorch[n] = self.scorch[n].max(to * k),
        }
    }

    /// Strokes a polyline, each point carrying its own strength (0 skips the piece).
    fn path(&mut self, points: &[(Vec2, f32)], half: f32, c: Channel) {
        for w in points.windows(2) {
            let strength = w[0].1.min(w[1].1);
            if strength > 0.0 {
                self.stroke(w[0].0, w[1].0, half, c, strength);
            }
        }
    }

    /// A ragged charred patch of about `radius`, darkest in the middle.
    fn scorch(&mut self, centre: Vec2, radius: f32, seed: u32) {
        let reach = radius * 1.6;
        let (lo, hi) = (centre - Vec2::splat(reach), centre + Vec2::splat(reach));
        let (i0, j0) = ((lo.x * PER_UNIT).max(0.0) as usize, (lo.y * PER_UNIT).max(0.0) as usize);
        let (i1, j1) =
            (((hi.x * PER_UNIT).max(0.0) as usize).min(self.w), ((hi.y * PER_UNIT).max(0.0) as usize).min(self.h));
        for j in j0..j1 {
            for i in i0..i1 {
                let p = Vec2::new((i as f32 + 0.5) / PER_UNIT, (j as f32 + 0.5) / PER_UNIT);
                // Noise on the distance gives a broken, splashed edge rather than a disc.
                let rag = fbm(p.x * 0.6, p.y * 0.6, 3, seed) - 0.5;
                let d = p.distance(centre) / radius + rag * 0.9;
                let k = ((1.25 - d) / 0.6).clamp(0.0, 1.0);
                self.put(j * self.w + i, Channel::Scorch(1.0), k);
            }
        }
    }

    pub fn image(self) -> Image {
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let mut data = Vec::with_capacity(self.w * self.h * 4);
        for n in 0..self.w * self.h {
            // Shade is stored halved, so 0.5 (128) leaves the ground unchanged.
            data.extend_from_slice(&[byte(self.shade[n] / 2.0), byte(self.dirt[n]), byte(self.scorch[n]), 255]);
        }
        let mut image = Image::new(
            Extent3d { width: self.w as u32, height: self.h as u32, depth_or_array_layers: 1 },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::RENDER_WORLD,
        );
        mip_chain(&mut image);
        let mut sampler = ImageSamplerDescriptor::linear();
        sampler.set_anisotropic_filter(16);
        image.sampler = ImageSampler::Descriptor(sampler);
        image
    }

    /// Shade, dirt and scorch at world `p`, for tests.
    #[cfg(test)]
    pub fn at(&self, p: Vec2) -> (f32, f32, f32) {
        let (i, j) = ((p.x * PER_UNIT) as usize, (p.y * PER_UNIT) as usize);
        let n = j.min(self.h - 1) * self.w + i.min(self.w - 1);
        (self.shade[n], self.dirt[n], self.scorch[n])
    }
}

/// Paints every mark of the map: ruts along each road, tracks across open ground from the
/// bases to their resources and beyond, and scorched patches out in the field and under every
/// wreck in `wrecks`. Deterministic for a map: positions come from `layout.seed`.
pub(super) fn paint(layout: &Layout, wrecks: &[Vec2]) -> Marks {
    let mut m = Marks::new(layout.width, layout.height);
    let seed = layout.seed;
    // Ruts: two lanes of wheel lines down each road, each line wandering a little and worn
    // away in places, plus fainter strays between them.
    for (k, road) in layout.roads.iter().enumerate() {
        let len = road.length();
        let steps = (len / 1.5).ceil().max(1.0) as usize;
        let lanes = [-AXLE * 1.6, -AXLE * 0.6, AXLE * 0.6, AXLE * 1.6, -0.2, 0.9];
        for (l, &offset) in lanes.iter().enumerate() {
            let s = seed.wrapping_add(k as u32 * 17 + l as u32 * 3);
            let stray = l >= 4;
            let points: Vec<(Vec2, f32)> = (0..=steps)
                .map(|i| {
                    let t = i as f32 / steps as f32;
                    let along = t * len;
                    let wander = (fbm(along * 0.05, l as f32 * 7.3, 2, s) - 0.5) * 0.8;
                    let p = road.point(t) + road.normal(t) * (offset + wander);
                    let wear = fbm(along * 0.04, k as f32 * 3.1 + l as f32, 2, s + 1);
                    let strength =
                        if stray { ((wear - 0.5) * 3.0).clamp(0.0, 0.6) } else { (wear * 2.2 - 0.2).clamp(0.0, 1.0) };
                    (p, strength)
                })
                .collect();
            m.path(&points, 0.36, Channel::Shade(0.62));
        }
    }
    // Tracks: pairs of pale lines, as left by vehicles driving cross-country. From each base
    // out in several directions, and from each resource back toward its nearest base.
    let mut tracks: Vec<(Vec2, Vec2, f32)> = Vec::new();
    for (k, &base) in layout.bases.iter().enumerate() {
        for n in 0..5u32 {
            let r = |q: u32| hash(k as i32, n as i32, seed.wrapping_add(3100 + q));
            let a = (n as f32 + r(0) * 0.8) / 5.0 * TAU;
            let dir = Vec2::from_angle(a);
            let from = base + dir * 22.0;
            tracks.push((from, from + dir * (50.0 + r(1) * 60.0), (r(2) - 0.5) * 50.0));
        }
    }
    for (k, &(pile, _)) in layout.resources.iter().enumerate() {
        let Some(&base) = layout.bases.iter().min_by(|a, b| a.distance(pile).total_cmp(&b.distance(pile))) else {
            continue;
        };
        let r = |q: u32| hash(k as i32, 99, seed.wrapping_add(3200 + q));
        tracks.push((base, pile, (r(0) - 0.5) * 70.0));
    }
    for (k, &(a, b, bend)) in tracks.iter().enumerate() {
        let len = a.distance(b);
        let normal = (b - a).perp().normalize_or_zero();
        let steps = (len / 1.5).ceil().max(1.0) as usize;
        let s = seed.wrapping_add(3300 + k as u32);
        for side in [-0.5, 0.5] {
            let points: Vec<(Vec2, f32)> = (0..=steps)
                .map(|i| {
                    let t = i as f32 / steps as f32;
                    let centre = a.lerp(b, t) + normal * bend * 4.0 * t * (1.0 - t);
                    let wander = (fbm(t * len * 0.04, side, 2, s) - 0.5) * 2.0;
                    let p = centre + normal * (side * AXLE + wander);
                    // Faint at both ends, and gone on roads (worn in already), slopes and water.
                    let fade = (t * 6.0).min((1.0 - t) * 6.0).min(1.0);
                    let wear = (fbm(t * len * 0.03, 5.0, 2, s + 1) * 1.8 - 0.2).clamp(0.0, 1.0);
                    let open =
                        layout.road(p) > ROAD_HALF_WIDTH && layout.slope(p) < 0.4 && layout.water_depth(p).is_none();
                    (p, if open { fade * wear } else { 0.0 })
                })
                .collect();
            m.path(&points, 0.5, Channel::Dirt(0.85));
        }
    }
    // Scorched patches: about one per 70×70 units of open field, kept off the base areas (the
    // fighting hasn't reached them yet), and one under every wreck.
    let count = (layout.width * layout.height / 4900.0) as i32;
    for k in 0..count {
        let r = |q: u32| hash(k, 7, seed.wrapping_add(3400 + q));
        let p = Vec2::new(r(0) * layout.width, r(1) * layout.height);
        let in_base = layout.sites.iter().any(|&(c, rad)| c.distance(p) < rad + 6.0);
        if in_base || layout.water_depth(p).is_some() || layout.border(p) > 0.05 {
            continue;
        }
        m.scorch(p, 1.8 + r(2) * 3.5, seed.wrapping_add(3500 + k as u32));
    }
    for (k, &p) in wrecks.iter().enumerate() {
        m.scorch(p, 2.6, seed.wrapping_add(3600 + k as u32));
    }
    m
}
