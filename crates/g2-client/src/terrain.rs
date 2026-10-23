//! Procedural savanna terrain (docs/STYLE.md): ochre and olive grass
//! over tan dirt, worn light dirt roads linking bases, piles and wells, dark rocky ridges at the
//! map edge, acacia-like trees and bushes. All generated here; no textures or game assets.
//!
//! Terrain is purely visual: the sim's ground is flat, so the playable area stays (nearly) flat
//! and relief is confined to the border ridges.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::sim::Sim;

/// Vertex spacing in world units.
const CELL: f32 = 2.0;
/// Extra ground beyond the playable area, so the camera never sees the edge of the world.
const APRON: f32 = 120.0;
const ROAD_HALF_WIDTH: f32 = 5.0;

pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_terrain);
    }
}

// ---- deterministic value noise ------------------------------------------------------------

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h =
        (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0x00ff_ffff) as f32 / 0x00ff_ffff as f32
}

fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - xi as f32, y - yi as f32);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (s(fx), s(fy));
    let a = hash(xi, yi, seed);
    let b = hash(xi + 1, yi, seed);
    let c = hash(xi, yi + 1, seed);
    let d = hash(xi + 1, yi + 1, seed);
    (a + (b - a) * sx) + ((c + (d - c) * sx) - (a + (b - a) * sx)) * sy
}

/// Fractal noise in 0..1.
pub fn fbm(x: f32, y: f32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 0.5, 1.0, 0.0);
    for o in 0..octaves {
        sum += value_noise(x * freq, y * freq, seed + o * 101) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.03;
    }
    sum / norm
}

// ---- layout ---------------------------------------------------------------------------------

/// A road is a quadratic Bézier between two points, bent by a deterministic sideways offset.
struct Road {
    a: Vec2,
    ctrl: Vec2,
    b: Vec2,
}

impl Road {
    fn new(a: Vec2, b: Vec2, bend: f32) -> Road {
        let mid = (a + b) / 2.0;
        let normal = (b - a).perp().normalize_or_zero();
        Road { a, ctrl: mid + normal * bend, b }
    }

    fn point(&self, t: f32) -> Vec2 {
        let u = 1.0 - t;
        self.a * u * u + self.ctrl * 2.0 * u * t + self.b * t * t
    }

    /// Distance from `p` to the curve, sampled; plenty accurate at road widths.
    fn distance(&self, p: Vec2) -> f32 {
        (0..=48).map(|i| self.point(i as f32 / 48.0).distance(p)).fold(f32::MAX, f32::min)
    }
}

struct Layout {
    width: f32,
    height: f32,
    roads: Vec<Road>,
    /// Bases and resources: kept clear of trees and rocks.
    clearings: Vec<(Vec2, f32)>,
}

impl Layout {
    fn from_sim(sim: &Sim) -> Layout {
        let map = &sim.world.map;
        let p = |v: g2_sim::Vec2| Vec2::new(v.x.to_f32(), v.y.to_f32());
        let spawns: Vec<Vec2> = map.spawns.iter().map(|&s| p(s)).collect();
        let mut roads = Vec::new();
        // Room for the opening buildings; the AI and players build within ~35 units of the spawn.
        let mut clearings: Vec<(Vec2, f32)> = spawns.iter().map(|&s| (s, 38.0)).collect();
        // Main road between the bases, plus a spur from each resource to the nearest base.
        if let [a, b, ..] = spawns.as_slice() {
            roads.push(Road::new(*a, *b, 60.0));
        }
        for (i, &(pos, _)) in map.supply_piles.iter().chain(map.oil_wells.iter()).enumerate() {
            let q = p(pos);
            clearings.push((q, 16.0));
            let nearest = spawns.iter().copied().min_by(|x, y| x.distance(q).total_cmp(&y.distance(q))).unwrap_or(q);
            let bend = if i % 2 == 0 { 18.0 } else { -18.0 };
            roads.push(Road::new(nearest, q, bend));
        }
        Layout { width: map.width.to_f32(), height: map.height.to_f32(), roads, clearings }
    }

    fn road(&self, p: Vec2) -> f32 {
        self.roads.iter().map(|r| r.distance(p)).fold(f32::MAX, f32::min)
    }

    /// 0 inside the playable area, rising to 1 a little past its edge.
    fn border(&self, p: Vec2) -> f32 {
        let out = (-p.x).max(-p.y).max(p.x - self.width).max(p.y - self.height);
        let wobble = (fbm(p.x * 0.03, p.y * 0.03, 7, 3) - 0.5) * 30.0;
        ((out + 18.0 + wobble) / 40.0).clamp(0.0, 1.0)
    }

    fn height(&self, p: Vec2) -> f32 {
        let b = self.border(p);
        let ridge = b * b * (14.0 + fbm(p.x * 0.05, p.y * 0.05, 4, 11) * 22.0);
        // Gentle undulation only; the sim ground is flat, so keep it within ±0.3.
        let undulate = (fbm(p.x * 0.02, p.y * 0.02, 3, 5) - 0.5) * 0.6 * (1.0 - b);
        ridge + undulate
    }

    fn color(&self, p: Vec2) -> [f32; 4] {
        let dirt = Vec3::new(0.60, 0.48, 0.31);
        let road = Vec3::new(0.74, 0.63, 0.45);
        let ochre = Vec3::new(0.52, 0.44, 0.20);
        let olive = Vec3::new(0.34, 0.33, 0.13);
        let rock = Vec3::new(0.20, 0.18, 0.15);
        // Mostly dry grass, with large dirt patches and fine breakup.
        let big = fbm(p.x * 0.012, p.y * 0.012, 4, 21);
        let fine = fbm(p.x * 0.25, p.y * 0.25, 3, 31);
        let grass_amt = ((big - 0.18) * 4.0 + (fine - 0.5) * 1.4).clamp(0.0, 1.0);
        let grass = ochre.lerp(olive, fbm(p.x * 0.04, p.y * 0.04, 3, 41).powf(0.8));
        let mut c = dirt.lerp(grass, grass_amt);
        // Worn road: bright centre, soft shoulders, tyre-track streaks.
        let d = self.road(p);
        let edge = fbm(p.x * 0.15, p.y * 0.15, 2, 51) * 3.0;
        let on_road = (1.0 - ((d - ROAD_HALF_WIDTH + edge) / 4.0)).clamp(0.0, 1.0);
        let tracks = if (d - 2.5).abs() < 0.6 { 0.06 } else { 0.0 };
        c = c.lerp(road - Vec3::splat(tracks), on_road);
        // Only a soft trampled patch at the very centre of a base; grass runs up to the buildings.
        for &(centre, r) in &self.clearings {
            let k = (1.0 - (centre.distance(p) - r * 0.25) / (r * 0.35)).clamp(0.0, 1.0);
            c = c.lerp(dirt * 1.05, k * 0.45);
        }
        // Ridges go dark and rocky.
        let b = self.border(p);
        c = c.lerp(rock * (0.7 + fine * 0.6), (b * 1.4).clamp(0.0, 1.0));
        // Fine luminance grain everywhere.
        let grain = 0.88 + fbm(p.x * 0.7, p.y * 0.7, 2, 61) * 0.24;
        let c = c * grain;
        // Authored in sRGB; vertex colours are linear.
        Color::srgb(c.x, c.y, c.z).to_linear().to_f32_array()
    }

    fn is_clear(&self, p: Vec2, margin: f32) -> bool {
        self.road(p) > ROAD_HALF_WIDTH + margin
            && self.clearings.iter().all(|&(c, r)| c.distance(p) > r + margin)
            && self.border(p) < 0.05
    }
}

// ---- meshes ---------------------------------------------------------------------------------

fn ground_mesh(layout: &Layout) -> Mesh {
    let (x0, z0) = (-APRON, -APRON);
    let nx = ((layout.width + 2.0 * APRON) / CELL) as usize + 1;
    let nz = ((layout.height + 2.0 * APRON) / CELL) as usize + 1;
    let mut positions = Vec::with_capacity(nx * nz);
    let mut colors = Vec::with_capacity(nx * nz);
    let mut uvs = Vec::with_capacity(nx * nz);
    for j in 0..nz {
        for i in 0..nx {
            let p = Vec2::new(x0 + i as f32 * CELL, z0 + j as f32 * CELL);
            positions.push([p.x, layout.height(p), p.y]);
            colors.push(layout.color(p));
            uvs.push([p.x / DETAIL_TILE, p.y / DETAIL_TILE]);
        }
    }
    let mut indices = Vec::with_capacity((nx - 1) * (nz - 1) * 6);
    for j in 0..nz - 1 {
        for i in 0..nx - 1 {
            let a = (j * nx + i) as u32;
            let (b, c, d) = (a + 1, a + nx as u32, a + nx as u32 + 1);
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh.compute_smooth_normals();
    mesh
}

/// World units per repeat of the detail texture.
const DETAIL_TILE: f32 = 14.0;
const DETAIL_SIZE: u32 = 256;

/// Tiling grayscale detail (grass tufts, pebbles, cracks) multiplied over the vertex colours, so
/// the ground has a fine, busy texture at close zoom. Mean is ~1.0 so it
/// modulates without darkening. Periodic noise keeps the tile seamless.
fn detail_image() -> Image {
    let n = DETAIL_SIZE as usize;
    let period = 16;
    let tiled = |x: f32, y: f32, scale: i32, seed: u32| {
        // Value noise on a lattice that wraps every `scale` cells.
        let (fx, fy) = (x * scale as f32, y * scale as f32);
        let (xi, yi) = (fx.floor() as i32, fy.floor() as i32);
        let (tx, ty) = (fx - xi as f32, fy - yi as f32);
        let s = |t: f32| t * t * (3.0 - 2.0 * t);
        let g = |a: i32, b: i32| hash(a.rem_euclid(scale), b.rem_euclid(scale), seed);
        let (a, b, c, d) = (g(xi, yi), g(xi + 1, yi), g(xi, yi + 1), g(xi + 1, yi + 1));
        let top = a + (b - a) * s(tx);
        let bot = c + (d - c) * s(tx);
        top + (bot - top) * s(ty)
    };
    let mut data = Vec::with_capacity(n * n * 4);
    for y in 0..n {
        for x in 0..n {
            let (u, v) = (x as f32 / n as f32, y as f32 / n as f32);
            let tufts = tiled(u, v, period * 4, 3);
            let blades = tiled(u, v, period * 8, 5);
            let pebbles = tiled(u, v, period * 2, 7);
            let tuft = ((tufts - 0.55) * 4.0).clamp(0.0, 1.0);
            let shade = 0.82 + blades * 0.3 - tuft * 0.18 + (pebbles - 0.5) * 0.12;
            let l = (shade.clamp(0.5, 1.3) / 1.3 * 255.0) as u8;
            data.extend_from_slice(&[l, l, l, 255]);
        }
    }
    let mut image = Image::new(
        Extent3d { width: DETAIL_SIZE, height: DETAIL_SIZE, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    image
}
#[derive(Clone, Copy)]
enum Prop {
    Acacia,
    Bush,
    Rock,
}

/// Deterministic scatter on a jittered grid, avoiding roads, bases and resources.
fn scatter(layout: &Layout) -> Vec<(Prop, Vec2, f32, f32)> {
    let mut out = Vec::new();
    let step = 6.5;
    let (nx, nz) = ((layout.width / step) as i32, (layout.height / step) as i32);
    for j in 0..nz {
        for i in 0..nx {
            let r = |k: u32| hash(i, j, 900 + k);
            let p = Vec2::new((i as f32 + r(0)) * step, (j as f32 + r(1)) * step);
            // Trees cluster where the big grass noise is high, forming groves.
            let density = fbm(p.x * 0.015, p.y * 0.015, 3, 77);
            let roll = r(2);
            let kind = if roll < (density - 0.40).max(0.0) * 0.7 {
                Prop::Acacia
            } else if roll < (density - 0.28).max(0.0) * 1.3 {
                Prop::Bush
            } else if roll > 0.98 {
                Prop::Rock
            } else {
                continue;
            };
            if layout.is_clear(p, 3.0) {
                out.push((kind, p, 0.75 + r(3) * 0.6, r(4) * std::f32::consts::TAU));
            }
        }
    }
    out
}

fn spawn_terrain(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    sim: Res<Sim>,
) {
    let layout = Layout::from_sim(&sim);
    let ground = materials.add(StandardMaterial {
        // Detail is stored at /1.3 to fit 8 bits; scale back so it averages out near 1.
        base_color: Color::linear_rgb(1.3, 1.3, 1.3),
        base_color_texture: Some(images.add(detail_image())),
        perceptual_roughness: 0.95,
        reflectance: 0.2,
        ..default()
    });
    commands.spawn((Mesh3d(meshes.add(ground_mesh(&layout))), MeshMaterial3d(ground)));

    let mat = |m: &mut Assets<StandardMaterial>, c: Color| {
        m.add(StandardMaterial { base_color: c, perceptual_roughness: 0.9, ..default() })
    };
    let canopy = mat(&mut materials, Color::srgb(0.22, 0.27, 0.10));
    let canopy_dark = mat(&mut materials, Color::srgb(0.15, 0.19, 0.08));
    let bark = mat(&mut materials, Color::srgb(0.25, 0.19, 0.13));
    let stone = mat(&mut materials, Color::srgb(0.30, 0.27, 0.23));
    let trunk = meshes.add(Cylinder::new(0.35, 1.0));
    let crown = meshes.add(Sphere::new(1.0).mesh().ico(2).unwrap());
    let rock = meshes.add(Sphere::new(1.0).mesh().ico(1).unwrap());

    for (kind, p, scale, yaw) in scatter(&layout) {
        let base = Vec3::new(p.x, layout.height(p), p.y);
        let rot = Quat::from_rotation_y(yaw);
        match kind {
            Prop::Acacia => {
                let h = 4.0 * scale;
                commands
                    .spawn((Transform::from_translation(base).with_rotation(rot), Visibility::default()))
                    .with_children(|t| {
                        t.spawn((
                            Mesh3d(trunk.clone()),
                            MeshMaterial3d(bark.clone()),
                            Transform::from_xyz(0.0, h / 2.0, 0.0).with_scale(Vec3::new(1.0, h, 1.0)),
                        ));
                        // Flat, wide canopy: savanna acacia silhouette.
                        t.spawn((
                            Mesh3d(crown.clone()),
                            MeshMaterial3d(canopy.clone()),
                            Transform::from_xyz(0.0, h, 0.0).with_scale(Vec3::new(3.6, 1.1, 3.2) * scale),
                        ));
                        t.spawn((
                            Mesh3d(crown.clone()),
                            MeshMaterial3d(canopy_dark.clone()),
                            Transform::from_xyz(1.4 * scale, h - 0.4, 0.8).with_scale(Vec3::new(2.0, 0.8, 1.9) * scale),
                        ));
                    });
            }
            Prop::Bush => {
                commands.spawn((
                    Mesh3d(crown.clone()),
                    MeshMaterial3d(canopy_dark.clone()),
                    Transform::from_translation(base + Vec3::Y * 0.6 * scale)
                        .with_rotation(rot)
                        .with_scale(Vec3::new(1.8, 1.0, 1.5) * scale),
                ));
            }
            Prop::Rock => {
                commands.spawn((
                    Mesh3d(rock.clone()),
                    MeshMaterial3d(stone.clone()),
                    Transform::from_translation(base + Vec3::Y * 0.3)
                        .with_rotation(rot)
                        .with_scale(Vec3::new(2.4, 1.2, 1.8) * scale),
                ));
            }
        }
    }
    // Boulders along the ridges: dark rocky rims at the map edges.
    let (w, h) = (layout.width, layout.height);
    let perimeter = 2.0 * (w + h);
    let count = (perimeter / 7.0) as i32;
    for k in 0..count {
        let t = k as f32 / count as f32 * perimeter;
        let (edge_p, outward) = if t < w {
            (Vec2::new(t, 0.0), Vec2::NEG_Y)
        } else if t < w + h {
            (Vec2::new(w, t - w), Vec2::X)
        } else if t < 2.0 * w + h {
            (Vec2::new(2.0 * w + h - t, h), Vec2::Y)
        } else {
            (Vec2::new(0.0, perimeter - t), Vec2::NEG_X)
        };
        let p = edge_p + outward * (4.0 + hash(k, 0, 1700) * 14.0);
        let s = 2.0 + hash(k, 1, 1700) * 4.5;
        commands.spawn((
            Mesh3d(rock.clone()),
            MeshMaterial3d(stone.clone()),
            Transform::from_translation(Vec3::new(p.x, layout.height(p) + s * 0.25, p.y))
                .with_rotation(Quat::from_rotation_y(hash(k, 2, 1700) * std::f32::consts::TAU))
                .with_scale(Vec3::new(s * 1.3, s * 0.7, s)),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_deterministic_and_bounded() {
        for i in 0..500 {
            let (x, y) = (i as f32 * 1.37, i as f32 * -0.71);
            let a = fbm(x, y, 4, 9);
            assert_eq!(a, fbm(x, y, 4, 9));
            assert!((0.0..=1.0).contains(&a));
        }
    }

    #[test]
    fn playable_area_stays_flat_enough_for_the_flat_sim_ground() {
        let sim = Sim::new(1, [g2_sim::Faction::Eu, g2_sim::Faction::Gla], true);
        let layout = Layout::from_sim(&sim);
        for j in 0..40 {
            for i in 0..40 {
                let p = Vec2::new(30.0 + i as f32 * 8.5, 30.0 + j as f32 * 8.5);
                assert!(layout.height(p).abs() < 0.5, "height {} at {p}", layout.height(p));
            }
        }
    }

    #[test]
    fn props_never_block_roads_bases_or_resources() {
        let sim = Sim::new(1, [g2_sim::Faction::Eu, g2_sim::Faction::Gla], true);
        let layout = Layout::from_sim(&sim);
        let props = scatter(&layout);
        assert!(props.len() > 50, "{} props", props.len());
        for (_, p, _, _) in props {
            assert!(layout.road(p) > ROAD_HALF_WIDTH);
            for &(c, r) in &layout.clearings {
                assert!(c.distance(p) > r);
            }
        }
    }
}
