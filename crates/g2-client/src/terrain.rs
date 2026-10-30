//! Terrain in the look of docs/STYLE.md: ochre and olive grass over tan dirt, worn light dirt
//! roads, dark rocky ridges past the map edge, acacia-like trees and bushes. All generated
//! here; no textures or game assets.
//!
//! On the built-in map the layout is procedural and the playable area stays flat. On an
//! imported Generals / Zero Hour map, the original's relief, painted ground classes, roads,
//! water areas and tree positions drive the same procedural look. Terrain is visual only: the
//! sim stays flat and models are lifted onto the ground (`Sim::to_world`).

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use g2_maps::MapFile;

use crate::sim::{Sim, to_world};

/// Vertex spacing in world units. Imported maps are larger, so they use a coarser grid; the
/// source height map has one sample per 10/6 world units anyway.
fn cell_size(layout: &Layout) -> f32 {
    if layout.source.is_some() { 3.0 } else { 2.0 }
}
/// Extra ground beyond the playable area, so the camera never sees the edge of the world.
const APRON: f32 = 120.0;
const ROAD_HALF_WIDTH: f32 = 5.0;

pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, spawn_terrain.run_if(crate::menu::scene_start).after(crate::menu::MenuSet))
            .add_systems(Update, (clear_under_buildings, update_fog).after(spawn_terrain));
    }
}

/// A scatter prop with its ground footprint, so buildings placed later can hide it.
#[derive(Component)]
struct Scenery {
    at: Vec2,
    radius: f32,
}

/// Props are placed once from the map; players build anywhere. Hide every prop that overlaps a
/// shown building's footprint (an enemy building in the fog must not clear its trees, or the
/// gap would give it away) or a ghost's (a remembered building destroyed out of sight: its
/// trees growing back would give the kill away), and (under fog) every prop on ground the
/// local player hasn't explored, since props stand taller than the fog sheet. Props never
/// move, so this only reruns when the footprints or the explored area change.
fn clear_under_buildings(
    sim: Res<Sim>,
    mut seen: Local<(Vec<(Vec2, f32)>, usize)>,
    mut props: Query<(&Scenery, &mut Visibility)>,
    fresh: Query<(), Added<Scenery>>,
) {
    if !fresh.is_empty() {
        *seen = Default::default();
    }
    let world = &sim.world;
    let ghosts = world
        .vision(sim.local)
        .memory
        .iter()
        .filter(|(id, _)| sim.fog && world.entity(**id).is_none())
        .map(|(_, s)| (s.pos, s.def));
    let buildings: Vec<(Vec2, f32)> = world
        .entities()
        .filter(|e| world.rules.def(e.def).is_building() && sim.shows(e))
        .map(|e| (e.pos, e.def))
        .chain(ghosts)
        .map(|(pos, def)| (to_world(pos).xz(), world.rules.def(def).radius.to_f32()))
        .collect();
    let v = world.vision(sim.local);
    let explored = (0..v.h).flat_map(|y| (0..v.w).map(move |x| (x, y))).filter(|&(x, y)| v.cell_state(x, y).0).count();
    if (buildings.as_slice(), explored) == (seen.0.as_slice(), seen.1) {
        return;
    }
    for (prop, mut vis) in &mut props {
        let covered = buildings.iter().any(|&(c, r)| c.distance(prop.at) < r + prop.radius);
        let hidden = covered || !sim.explored_at(prop.at.x, prop.at.y);
        vis.set_if_neq(if hidden { Visibility::Hidden } else { Visibility::Inherited });
    }
    *seen = (buildings, explored);
}

/// Darkness drawn over the playable ground: unexplored, explored but not seen, and visible.
const FOG_ALPHA: [f32; 3] = [0.88, 0.5, 0.0];

/// The fog overlay: a sheet just above the ground whose vertex alpha follows the local
/// player's fog cells, interpolated between cell centres so edges are soft.
#[derive(Component)]
struct FogSheet {
    mesh: Handle<Mesh>,
    /// Vertex positions in sim units (x, y), in mesh order.
    points: Vec<Vec2>,
}

fn fog_sheet(layout: &Layout) -> (Mesh, Vec<Vec2>) {
    let cell = cell_size(layout);
    let nx = (layout.width / cell) as usize + 1;
    let nz = (layout.height / cell) as usize + 1;
    let mut points = Vec::with_capacity(nx * nz);
    let mut positions = Vec::with_capacity(nx * nz);
    for j in 0..nz {
        for i in 0..nx {
            let p = Vec2::new((i as f32 * cell).min(layout.width), (j as f32 * cell).min(layout.height));
            points.push(p);
            // Over water the sheet lies on the surface, not the bed, or the water would cover it.
            let top = layout.height(p) + layout.water_depth(p).unwrap_or(0.0);
            positions.push([p.x, top + 0.25, p.y]);
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
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32, 0.0, 0.0, FOG_ALPHA[0]]; nx * nz]);
    mesh.insert_indices(Indices::U32(indices));
    (mesh, points)
}

/// Fog darkness at sim point `p`: bilinear between the centres of the fog cells around it.
fn fog_alpha(states: &[u8], w: i32, h: i32, p: Vec2) -> f32 {
    let c = g2_sim::fog::CELL as f32;
    let (fx, fy) = (p.x / c - 0.5, p.y / c - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let at = |x: i32, y: i32| FOG_ALPHA[states[(y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as usize] as usize];
    let (x0, y0) = (x0 as i32, y0 as i32);
    let top = at(x0, y0) + (at(x0 + 1, y0) - at(x0, y0)) * tx;
    let bot = at(x0, y0 + 1) + (at(x0 + 1, y0 + 1) - at(x0, y0 + 1)) * tx;
    top + (bot - top) * ty
}

/// Rewrites the fog sheet's alpha when the local player's fog cells change.
fn update_fog(
    sim: Res<Sim>,
    mut last: Local<Vec<u8>>,
    mut now: Local<Vec<u8>>,
    sheets: Query<&FogSheet>,
    fresh: Query<(), Added<FogSheet>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    if !fresh.is_empty() {
        last.clear();
    }
    sim.fog_states(&mut now);
    if *now == *last {
        return;
    }
    std::mem::swap(&mut *now, &mut *last);
    let v = sim.world.vision(sim.local);
    for sheet in &sheets {
        let Some(mut mesh) = meshes.get_mut(&sheet.mesh) else { continue };
        let colors: Vec<[f32; 4]> =
            sheet.points.iter().map(|&p| [0.0, 0.0, 0.0, fog_alpha(&last, v.w, v.h, p)]).collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
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

struct Layout<'a> {
    width: f32,
    height: f32,
    roads: Vec<Road>,
    /// Command centers and resources: kept clear of trees and rocks. Small on purpose, so groves
    /// grow right up to the bases; later buildings hide what they cover (`clear_under_buildings`).
    clearings: Vec<(Vec2, f32)>,
    /// Trampled dirt painted around bases and resources; larger than the scenery clearings.
    pads: Vec<(Vec2, f32)>,
    /// The original map when one is imported: heights, painted ground and water come from it.
    source: Option<&'a MapFile>,
}

/// What a retail terrain texture class reads as, for our own palette.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Ground {
    Grass,
    Dirt,
    Sand,
    Rock,
    Snow,
    Paved,
}

impl Ground {
    /// Classifies by the texture class name (`GrassMediumType3`, `CliffLargeType1snow`, ...).
    pub(crate) fn of(class: &str) -> Ground {
        let c = class.to_ascii_lowercase();
        if c.contains("snow") {
            Ground::Snow
        } else if c.contains("cliff") || c.contains("rock") || c.contains("mountain") {
            Ground::Rock
        } else if ["asphalt", "concrete", "parking", "cobble", "wood", "bridge"].iter().any(|k| c.contains(k)) {
            Ground::Paved
        } else if c.contains("grass") || c.contains("crops") || c.contains("field") {
            Ground::Grass
        } else if c.contains("sand") || c.contains("desert") || c.contains("beach") {
            Ground::Sand
        } else {
            Ground::Dirt
        }
    }
}

impl<'a> Layout<'a> {
    fn from_sim(sim: &'a Sim) -> Layout<'a> {
        let map = &sim.world.map;
        let p = |v: g2_sim::Vec2| Vec2::new(v.x.to_f32(), v.y.to_f32());
        let spawns: Vec<Vec2> = map.spawns.iter().map(|&s| p(s)).collect();
        let mut roads = Vec::new();
        // Just past the command center (radius 10); everything else at a base is built later.
        let mut clearings: Vec<(Vec2, f32)> = spawns.iter().map(|&s| (s, 13.0)).collect();
        let mut pads: Vec<(Vec2, f32)> = spawns.iter().map(|&s| (s, 38.0)).collect();
        for &(pos, _) in map.supply_piles.iter().chain(map.oil_wells.iter()) {
            clearings.push((p(pos), 8.0));
            pads.push((p(pos), 16.0));
        }
        match &sim.source {
            // The original road network, as straight segments between road points.
            Some(file) => {
                // Real roads only; markings (arrows, words, parking lines, caution areas)
                // are decals in the original and would read as dirt tracks here.
                const ROADS: [&str; 7] = ["road", "lane", "path", "cobble", "gravel", "sidewalk", "track"];
                for r in file.roads() {
                    let n = r.name.to_ascii_lowercase();
                    if !ROADS.iter().any(|k| n.contains(k)) {
                        continue;
                    }
                    let a = Vec2::from(file.to_world(r.a.0, r.a.1));
                    let b = Vec2::from(file.to_world(r.b.0, r.b.1));
                    roads.push(Road::new(a, b, 0.0));
                }
            }
            // Built-in map: a main road between the bases, plus a spur from each resource.
            None => {
                if let [a, b, ..] = spawns.as_slice() {
                    roads.push(Road::new(*a, *b, 60.0));
                }
                for (i, &(pos, _)) in map.supply_piles.iter().chain(map.oil_wells.iter()).enumerate() {
                    let q = p(pos);
                    let nearest =
                        spawns.iter().copied().min_by(|x, y| x.distance(q).total_cmp(&y.distance(q))).unwrap_or(q);
                    let bend = if i % 2 == 0 { 18.0 } else { -18.0 };
                    roads.push(Road::new(nearest, q, bend));
                }
            }
        }
        Layout {
            width: map.width.to_f32(),
            height: map.height.to_f32(),
            roads,
            clearings,
            pads,
            source: sim.source.as_ref(),
        }
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
        if let Some(file) = self.source {
            // Inside: exactly the original relief, which models are placed on. Outside: the
            // relief around the nearest edge point, averaged over a window that widens with
            // distance (5×5 samples, clamped into the map), plus our ridge. Continuous at the
            // edge and corners; the source's border cliffs soften instead of streaking outward.
            let size = Vec2::new(self.width, self.height);
            let edge = p.clamp(Vec2::ZERO, size);
            let out = p.distance(edge);
            if out == 0.0 {
                return file.ground_y(p.x, p.y);
            }
            let spread = out * 0.5;
            let mut sum = 0.0;
            for j in -2..=2 {
                for i in -2..=2 {
                    let q = (edge + Vec2::new(i as f32, j as f32) * spread).clamp(Vec2::ZERO, size);
                    sum += file.ground_y(q.x, q.y);
                }
            }
            // Ridge rises from zero at the edge (`border` already ramps up inside the edge).
            let rise = (out / 40.0).min(1.0);
            return sum / 25.0 + ridge * rise;
        }
        // Gentle undulation only; the sim ground is flat, so keep it within ±0.3.
        let undulate = (fbm(p.x * 0.02, p.y * 0.02, 3, 5) - 0.5) * 0.6 * (1.0 - b);
        ridge + undulate
    }

    /// Painted ground around `p` as weights per `Ground` (Grass, Dirt, Sand, Rock, Snow, Paved).
    /// Four noise-warped samples ±1.5 units apart are averaged, so class boundaries are ragged
    /// and soft instead of following the source's square cells (we don't read its blend tiles).
    fn ground_mix(&self, p: Vec2) -> Option<[f32; 6]> {
        let file = self.source?;
        let warp = Vec2::new(fbm(p.x * 0.09, p.y * 0.09, 3, 71) - 0.5, fbm(p.x * 0.09, p.y * 0.09, 3, 73) - 0.5) * 9.0;
        let mut mix = [0.0; 6];
        for o in [Vec2::new(-1.5, -1.5), Vec2::new(1.5, -1.5), Vec2::new(-1.5, 1.5), Vec2::new(1.5, 1.5)] {
            let q = p + warp + o;
            let g = file.class_at(q.x, q.y).map_or(Ground::Dirt, Ground::of);
            mix[g as usize] += 0.25;
        }
        Some(mix)
    }

    /// Steepness of the rendered ground at `p` (rise per unit run).
    fn slope(&self, p: Vec2) -> f32 {
        let e = 1.0;
        let dx = self.height(p + Vec2::X * e) - self.height(p - Vec2::X * e);
        let dz = self.height(p + Vec2::Y * e) - self.height(p - Vec2::Y * e);
        Vec2::new(dx, dz).length() / (2.0 * e)
    }

    fn color(&self, p: Vec2) -> [f32; 4] {
        // Measured against the 2013 alpha frames: its ground is warm brown-ochre dry grass
        // (about sRGB 135/103/62), with darker olive only in scattered tufts and patches.
        let dirt = Vec3::new(0.56, 0.41, 0.24);
        let road = Vec3::new(0.68, 0.53, 0.34);
        let ochre = Vec3::new(0.53, 0.40, 0.21);
        let olive = Vec3::new(0.38, 0.33, 0.16);
        let rock = Vec3::new(0.24, 0.20, 0.16);
        let sand = Vec3::new(0.72, 0.58, 0.38);
        let snow = Vec3::new(0.86, 0.88, 0.90);
        let paved = Vec3::new(0.42, 0.41, 0.39);
        let big = fbm(p.x * 0.012, p.y * 0.012, 4, 21);
        let fine = fbm(p.x * 0.25, p.y * 0.25, 3, 31);
        let grass = ochre.lerp(olive, ((fbm(p.x * 0.04, p.y * 0.04, 3, 41) - 0.35) * 1.6).clamp(0.0, 0.7));
        let mut c = match self.ground_mix(p) {
            // Mostly dry grass, with large dirt patches and fine breakup.
            None => {
                let grass_amt = ((big - 0.18) * 4.0 + (fine - 0.5) * 1.4).clamp(0.0, 1.0);
                dirt.lerp(grass, grass_amt)
            }
            // Imported map: the original painting, re-coloured in our palette with our breakup.
            // Sand sits close to dirt so desert/grass edges read as savanna, not camouflage.
            Some(mix) => {
                let colours = [
                    grass,
                    dirt.lerp(grass, ((big - 0.45) * 2.0).clamp(0.0, 0.5)),
                    sand.lerp(dirt, 0.35),
                    rock * (0.8 + fine * 0.5),
                    snow,
                    paved,
                ];
                let base = mix.iter().zip(colours).fold(Vec3::ZERO, |acc, (&w, col)| acc + col * w);
                // Steep faces read as cliffs whatever was painted on them.
                let cliff = ((self.slope(p) - 0.6) * 2.0).clamp(0.0, 1.0);
                base.lerp(rock * (0.7 + fine * 0.6), cliff)
            }
        };
        // Worn road: bright centre, soft shoulders, tyre-track streaks.
        let d = self.road(p);
        let edge = fbm(p.x * 0.15, p.y * 0.15, 2, 51) * 3.0;
        let on_road = (1.0 - ((d - ROAD_HALF_WIDTH + edge) / 4.0)).clamp(0.0, 1.0);
        let tracks = if (d - 2.5).abs() < 0.6 { 0.06 } else { 0.0 };
        c = c.lerp(road - Vec3::splat(tracks), on_road);
        // Only a soft trampled patch at the very centre of a base; grass runs up to the buildings.
        for &(centre, r) in &self.pads {
            let k = (1.0 - (centre.distance(p) - r * 0.25) / (r * 0.35)).clamp(0.0, 1.0);
            c = c.lerp(dirt * 1.05, k * 0.45);
        }
        // Water: wet dark shore and deep blue-green beds; the surface is a separate mesh.
        if let Some(depth) = self.water_depth(p) {
            let bed = Vec3::new(0.18, 0.20, 0.16);
            c = c.lerp(bed, (depth / 3.0 + 0.4).clamp(0.0, 1.0));
        }
        // Ridges go dark and rocky.
        let b = self.border(p);
        c = c.lerp(rock * (0.7 + fine * 0.6), (b * 1.4).clamp(0.0, 1.0));
        // Fine luminance grain everywhere, and dark speckle (stones, burnt tufts) as in the
        // footage's busy ground.
        let grain = 0.86 + fbm(p.x * 0.7, p.y * 0.7, 2, 61) * 0.28;
        let speckle = if fbm(p.x * 1.9, p.y * 1.9, 1, 67) > 0.78 { 0.8 } else { 1.0 };
        let c = c * grain * speckle;
        // Authored in sRGB; vertex colours are linear.
        Color::srgb(c.x, c.y, c.z).to_linear().to_f32_array()
    }

    fn is_clear(&self, p: Vec2, margin: f32) -> bool {
        self.road(p) > ROAD_HALF_WIDTH + margin
            && self.clearings.iter().all(|&(c, r)| c.distance(p) > r + margin)
            && self.border(p) < 0.05
            && self.water_depth(p).is_none()
            && self.slope(p) < 0.5
    }

    /// Depth below the nearest water surface, if `p` is inside a water polygon.
    fn water_depth(&self, p: Vec2) -> Option<f32> {
        let file = self.source?;
        let (sx, sy) = file.to_sage(p.x, p.y);
        file.water.iter().find(|w| inside(&w.outline, sx, sy)).map(|w| {
            let surface = (w.z - file.base_height) / g2_maps::SAGE_PER_WORLD as f32;
            (surface - self.height(p)).max(0.0)
        })
    }
}

/// Even-odd point-in-polygon test.
fn inside(poly: &[(f32, f32)], x: f32, y: f32) -> bool {
    let mut hit = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            hit = !hit;
        }
        j = i;
    }
    hit
}

// ---- meshes ---------------------------------------------------------------------------------

fn ground_mesh(layout: &Layout) -> Mesh {
    let cell = cell_size(layout);
    let (x0, z0) = (-APRON, -APRON);
    let nx = ((layout.width + 2.0 * APRON) / cell) as usize + 1;
    let nz = ((layout.height + 2.0 * APRON) / cell) as usize + 1;
    let mut positions = Vec::with_capacity(nx * nz);
    let mut colors = Vec::with_capacity(nx * nz);
    let mut uvs = Vec::with_capacity(nx * nz);
    for j in 0..nz {
        for i in 0..nx {
            let p = Vec2::new(x0 + i as f32 * cell, z0 + j as f32 * cell);
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
            // Two texels per cell at most: finer grit would alias, and average to grey in the
            // mips anyway.
            let grit = tiled(u, v, period * 8, 9);
            // Dark tuft clumps with lit blade tips and a fine grit: the footage's ground is
            // busy at every scale, not smooth. Centred so the texture averages 1.0.
            let tuft = ((tufts - 0.5) * 4.0).clamp(0.0, 1.0);
            let shade = 0.91 + blades * 0.38 - tuft * 0.3 + (pebbles - 0.5) * 0.16 + (grit - 0.5) * 0.2;
            let l = (shade.clamp(0.4, 1.3) / 1.3 * 255.0) as u8;
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
    mip_chain(&mut image);
    // The ground is seen at a steep angle across a wide depth range: without mips the detail
    // rings and shimmers in the distance, and without anisotropy it smears near the camera.
    let mut sampler = ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    };
    sampler.set_anisotropic_filter(16);
    image.sampler = ImageSampler::Descriptor(sampler);
    image
}

/// Appends a box-filtered mip chain to a level-0 image (wgpu wants the levels concatenated).
fn mip_chain(image: &mut Image) {
    let mut data = image.data.clone().expect("level 0");
    let (mut w, mut h) = (image.texture_descriptor.size.width as usize, image.texture_descriptor.size.height as usize);
    let mut prev = data.clone();
    let mut levels = 1;
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = Vec::with_capacity(nw * nh * 4);
        for y in 0..nh {
            for x in 0..nw {
                let mut acc = [0u32; 4];
                let mut count = 0u32;
                for dy in 0..2 {
                    for dx in 0..2 {
                        let (sx, sy) = ((x * 2 + dx).min(w - 1), (y * 2 + dy).min(h - 1));
                        let i = (sy * w + sx) * 4;
                        for c in 0..4 {
                            acc[c] += prev[i + c] as u32;
                        }
                        count += 1;
                    }
                }
                next.extend(acc.iter().map(|&a| (a / count) as u8));
            }
        }
        data.extend_from_slice(&next);
        prev = next;
        w = nw;
        h = nh;
        levels += 1;
    }
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
}
#[derive(Clone, Copy)]
enum Prop {
    Acacia,
    Bush,
    Rock,
}

/// Scenery placement. Imported maps keep the original positions of every tree, bush and rock
/// (drawn as our own shapes); the built-in map uses a deterministic jittered-grid scatter.
fn scatter(layout: &Layout) -> Vec<(Prop, Vec2, f32, f32)> {
    if let Some(file) = layout.source {
        return imported_scenery(layout, file);
    }
    let mut out = Vec::new();
    let step = 6.5;
    let (nx, nz) = ((layout.width / step) as i32, (layout.height / step) as i32);
    for j in 0..nz {
        for i in 0..nx {
            let r = |k: u32| hash(i, j, 900 + k);
            let p = Vec2::new((i as f32 + r(0)) * step, (j as f32 + r(1)) * step);
            // Trees cluster where the big grass noise is high, forming groves, but stay sparse:
            // the footage's savanna is open ground with trees standing apart.
            let density = fbm(p.x * 0.015, p.y * 0.015, 3, 77);
            let roll = r(2);
            let kind = if roll < (density - 0.42).max(0.0) * 0.45 + 0.012 {
                Prop::Acacia
            } else if roll < (density - 0.30).max(0.0) * 0.9 + 0.03 {
                Prop::Bush
            } else if roll > 0.985 {
                Prop::Rock
            } else {
                continue;
            };
            if layout.is_clear(p, 3.0) {
                out.push((kind, p, 0.6 + r(3) * 0.45, r(4) * std::f32::consts::TAU));
            }
        }
    }
    out
}

fn imported_scenery(layout: &Layout, file: &MapFile) -> Vec<(Prop, Vec2, f32, f32)> {
    let mut out = Vec::new();
    for (k, o) in file.objects.iter().enumerate() {
        let template = o.name.rsplit('/').next().unwrap_or(&o.name).to_ascii_lowercase();
        let kind = if template.starts_with("tree") || template.starts_with("cinetree") {
            Prop::Acacia
        } else if template.starts_with("bush") {
            Prop::Bush
        } else if template.starts_with("rock") {
            Prop::Rock
        } else {
            continue;
        };
        let p = Vec2::from(file.to_world(o.x, o.y));
        // The original map already keeps scenery off roads and slopes; only bases and
        // resources (our footprints differ) and the outer ridge need clearing.
        let clear = layout.clearings.iter().all(|&(c, r)| c.distance(p) > r + 2.0)
            && layout.border(p) < 0.05
            && layout.water_depth(p).is_none();
        if clear {
            let r = |n: u32| hash(k as i32, 0, 950 + n);
            out.push((kind, p, 0.75 + r(0) * 0.6, -o.angle));
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
    commands.spawn((crate::menu::MatchEntity, Mesh3d(meshes.add(ground_mesh(&layout))), MeshMaterial3d(ground)));
    let (sheet, points) = fog_sheet(&layout);
    let sheet = meshes.add(sheet);
    let fog = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        // Distance haze would tint the black sheet; it sits on the ground and needs none.
        fog_enabled: false,
        ..default()
    });
    commands.spawn((
        crate::menu::MatchEntity,
        Mesh3d(sheet.clone()),
        MeshMaterial3d(fog),
        FogSheet { mesh: sheet, points },
    ));
    if let Some(file) = layout.source {
        spawn_bridges(&mut commands, &mut meshes, &mut materials, file);
        // Opaque: blended water would be sorted against the blended fog sheet per mesh, and
        // draw over it wherever its centre lies nearer the camera.
        let water = materials.add(StandardMaterial {
            base_color: Color::srgb(0.16, 0.30, 0.32),
            perceptual_roughness: 0.08,
            reflectance: 0.6,
            ..default()
        });
        for w in &file.water {
            if let Some(mesh) = water_mesh(file, w) {
                commands.spawn((crate::menu::MatchEntity, Mesh3d(meshes.add(mesh)), MeshMaterial3d(water.clone())));
            }
        }
    }

    let mat = |m: &mut Assets<StandardMaterial>, c: Color| {
        m.add(StandardMaterial { base_color: c, perceptual_roughness: 0.9, ..default() })
    };
    let canopy = mat(&mut materials, Color::srgb(0.20, 0.24, 0.09));
    let canopy_dark = mat(&mut materials, Color::srgb(0.13, 0.16, 0.07));
    let canopy_dry = mat(&mut materials, Color::srgb(0.30, 0.28, 0.12));
    let bark = mat(&mut materials, Color::srgb(0.22, 0.17, 0.12));
    let stone = mat(&mut materials, Color::srgb(0.30, 0.27, 0.23));
    let trunk = meshes.add(Cylinder::new(0.25, 1.0));
    let crown = meshes.add(Sphere::new(1.0).mesh().ico(2).unwrap());
    let rock = meshes.add(Sphere::new(1.0).mesh().ico(1).unwrap());

    for (kind, p, scale, yaw) in scatter(&layout) {
        let base = Vec3::new(p.x, layout.height(p), p.y);
        let rot = Quat::from_rotation_y(yaw);
        match kind {
            Prop::Acacia => {
                let h = 5.0 * scale;
                // Per-tree variety from its position, so neighbours differ.
                let r = |k: u32| hash(p.x as i32, p.y as i32, 1300 + k);
                let lean = Vec3::new(r(0) - 0.5, 0.0, r(1) - 0.5) * 0.5;
                commands
                    .spawn((
                        crate::menu::MatchEntity,
                        // Reach of the farthest crown lump: limb tip (~1.7) + spread (~1.9) + lump
                        // radius (~1.1), all × scale. Buildings clear props by this radius.
                        Scenery { at: p, radius: 4.7 * scale },
                        Transform::from_translation(base).with_rotation(rot),
                        Visibility::default(),
                    ))
                    .with_children(|t| {
                        // A slender trunk forking into two leaning limbs.
                        let fork = Vec3::Y * h * 0.55 + lean * h * 0.3;
                        let limb = |t: &mut ChildSpawnerCommands, from: Vec3, to: Vec3, thick: f32| {
                            let d = to - from;
                            t.spawn((
                                Mesh3d(trunk.clone()),
                                MeshMaterial3d(bark.clone()),
                                Transform::from_translation((from + to) / 2.0)
                                    .with_rotation(Quat::from_rotation_arc(Vec3::Y, d.normalize()))
                                    .with_scale(Vec3::new(thick, d.length(), thick)),
                            ));
                        };
                        limb(t, Vec3::ZERO, fork, 1.0);
                        let tips = [Vec3::new(1.6, h, 0.6) * scale, Vec3::new(-1.3, h * 0.92, -0.9) * scale];
                        for tip in tips {
                            limb(t, fork, tip + lean, 0.6);
                        }
                        // Each crown is a flat cluster of small lumps around a limb tip: a broken,
                        // leafy edge instead of a solid plate, with lit and shaded lumps mixed.
                        let crowns = [(tips[0] + lean, 2.4), (tips[1] + lean, 2.0)];
                        for (c, (at, spread)) in crowns.into_iter().enumerate() {
                            for k in 0..6u32 {
                                let n = c as u32 * 16 + k;
                                let a = k as f32 / 6.0 * std::f32::consts::TAU + r(10 + n);
                                let d = if k == 0 { 0.0 } else { spread * (0.45 + r(30 + n) * 0.35) };
                                let off = Vec3::new(a.cos() * d, (r(50 + n) - 0.5) * 0.5, a.sin() * d) * scale;
                                let s = (1.1 + r(70 + n) * 0.6) * scale;
                                let m = match (k + c as u32) % 3 {
                                    0 => &canopy,
                                    1 => &canopy_dark,
                                    _ => &canopy_dry,
                                };
                                t.spawn((
                                    Mesh3d(crown.clone()),
                                    MeshMaterial3d(m.clone()),
                                    Transform::from_translation(at + off).with_scale(Vec3::new(s, s * 0.32, s)),
                                ));
                            }
                        }
                    });
            }
            Prop::Bush => {
                // A low clump of two or three lumps rather than one ball.
                let r = |k: u32| hash(p.x as i32, p.y as i32, 1400 + k);
                commands
                    .spawn((
                        crate::menu::MatchEntity,
                        Scenery { at: p, radius: 1.8 * scale },
                        Transform::from_translation(base).with_rotation(rot),
                        Visibility::default(),
                    ))
                    .with_children(|t| {
                        for k in 0..3 {
                            let m = if k == 1 { &canopy } else { &canopy_dark };
                            let off = Vec3::new(r(k * 2) - 0.5, 0.0, r(k * 2 + 1) - 0.5) * 1.8 * scale;
                            let s = (0.7 + r(10 + k) * 0.6) * scale;
                            t.spawn((
                                Mesh3d(crown.clone()),
                                MeshMaterial3d(m.clone()),
                                Transform::from_translation(off + Vec3::Y * 0.45 * s)
                                    .with_scale(Vec3::new(1.1, 0.75, 1.0) * s),
                            ));
                        }
                    });
            }
            Prop::Rock => {
                commands.spawn((
                    crate::menu::MatchEntity,
                    Scenery { at: p, radius: 2.4 * scale },
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
            crate::menu::MatchEntity,
            Mesh3d(rock.clone()),
            MeshMaterial3d(stone.clone()),
            Transform::from_translation(Vec3::new(p.x, layout.height(p) + s * 0.25, p.y))
                .with_rotation(Quat::from_rotation_y(hash(k, 2, 1700) * std::f32::consts::TAU))
                .with_scale(Vec3::new(s * 1.3, s * 0.7, s)),
        ));
    }
}

/// Every bridge of an imported map as a concrete deck: a slab along the walkable deck
/// (`MapFile::bridge_deck_at`, `BRIDGE_DECK_WIDTH` wide), a low parapet on each side and piers
/// down to the ground. Our own shapes; the original bridge models aren't used.
fn spawn_bridges(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    file: &MapFile,
) {
    if file.bridges.is_empty() {
        return;
    }
    let deck = materials.add(StandardMaterial {
        base_color: Color::srgb(0.46, 0.43, 0.38),
        perceptual_roughness: 0.9,
        ..default()
    });
    let rail = materials.add(StandardMaterial {
        base_color: Color::srgb(0.36, 0.34, 0.30),
        perceptual_roughness: 0.9,
        ..default()
    });
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let per = g2_maps::SAGE_PER_WORLD as f32;
    let width = g2_maps::BRIDGE_DECK_WIDTH / per;
    for b in &file.bridges {
        let a = Vec2::from(file.to_world(b.a.0, b.a.1));
        let z = Vec2::from(file.to_world(b.b.0, b.b.1));
        let len = a.distance(z);
        if len < 1.0 {
            continue;
        }
        let dir = (z - a) / len;
        // The walkable deck reaches one height cell past each end so it meets the banks.
        let reach = g2_maps::SAGE_PER_CELL as f32 / per;
        let (ya, yz) = (file.surface_y(a.x, a.y), file.surface_y(z.x, z.y));
        let mid = (a + z) / 2.0;
        let pitch = ((yz - ya) / len).atan();
        let rot = Quat::from_rotation_y(-dir.y.atan2(dir.x)) * Quat::from_rotation_z(pitch);
        let thick = 0.8;
        let centre = Vec3::new(mid.x, (ya + yz) / 2.0 - thick / 2.0, mid.y);
        commands
            .spawn((
                crate::menu::MatchEntity,
                Transform::from_translation(centre).with_rotation(rot),
                Visibility::default(),
            ))
            .with_children(|c| {
                c.spawn((
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(deck.clone()),
                    Transform::from_scale(Vec3::new(len + 2.0 * reach, thick, width)),
                ));
                for side in [-1.0, 1.0] {
                    c.spawn((
                        Mesh3d(cube.clone()),
                        MeshMaterial3d(rail.clone()),
                        Transform::from_xyz(0.0, thick / 2.0 + 0.5, side * (width / 2.0 - 0.35)).with_scale(Vec3::new(
                            len + 2.0 * reach,
                            1.0,
                            0.7,
                        )),
                    ));
                }
            });
        // Piers every ~12 units, from the deck down to the ground below (bed or bank).
        let piers = (len / 12.0).floor() as i32;
        for k in 1..piers {
            let t = k as f32 / piers as f32;
            let p = a.lerp(z, t);
            let top = ya + (yz - ya) * t - thick;
            let bottom = file.ground_y(p.x, p.y) - 1.0;
            let h = top - bottom;
            if h < 0.5 {
                continue;
            }
            commands.spawn((
                crate::menu::MatchEntity,
                Mesh3d(cube.clone()),
                MeshMaterial3d(rail.clone()),
                Transform::from_translation(Vec3::new(p.x, bottom + h / 2.0, p.y))
                    .with_rotation(Quat::from_rotation_y(-dir.y.atan2(dir.x)))
                    .with_scale(Vec3::new(1.6, h, width * 0.8)),
            ));
        }
    }
}

/// Flat water surface over a polygon, clipped to the playable area (fan from the centroid; the
/// retail water areas are close enough to convex for this, and the ground under any overhang is
/// dark bed colour).
fn water_mesh(file: &MapFile, w: &g2_maps::Water) -> Option<Mesh> {
    let y = (w.z - file.base_height) / g2_maps::SAGE_PER_WORLD as f32;
    let pts: Vec<Vec2> = w.outline.iter().map(|&(x, z)| Vec2::from(file.to_world(x, z))).collect();
    let pts = clip_to_rect(&pts, Vec2::ZERO, Vec2::from(file.world_size()));
    if pts.len() < 3 {
        return None;
    }
    let centre = pts.iter().copied().sum::<Vec2>() / pts.len() as f32;
    let mut positions = vec![[centre.x, y, centre.y]];
    positions.extend(pts.iter().map(|p| [p.x, y, p.y]));
    let n = pts.len() as u32;
    let mut indices = Vec::with_capacity(pts.len() * 3);
    for i in 0..n {
        let (a, b) = (1 + i, 1 + (i + 1) % n);
        // Both windings, so the surface shows whatever direction the outline runs.
        indices.extend_from_slice(&[0, a, b, 0, b, a]);
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_indices(Indices::U32(indices));
    Some(mesh)
}

/// Sutherland–Hodgman clip of a polygon against an axis-aligned rectangle.
fn clip_to_rect(poly: &[Vec2], lo: Vec2, hi: Vec2) -> Vec<Vec2> {
    // Each edge: (inside test, intersection of segment a→b with the edge line).
    type Edge<'f> = (&'f dyn Fn(Vec2) -> bool, &'f dyn Fn(Vec2, Vec2) -> Vec2);
    let edges: [Edge; 4] = [
        (&|p| p.x >= lo.x, &|a, b| a.lerp(b, (lo.x - a.x) / (b.x - a.x))),
        (&|p| p.x <= hi.x, &|a, b| a.lerp(b, (hi.x - a.x) / (b.x - a.x))),
        (&|p| p.y >= lo.y, &|a, b| a.lerp(b, (lo.y - a.y) / (b.y - a.y))),
        (&|p| p.y <= hi.y, &|a, b| a.lerp(b, (hi.y - a.y) / (b.y - a.y))),
    ];
    let mut out = poly.to_vec();
    for (inside, cross) in edges {
        let input = std::mem::take(&mut out);
        for (i, &b) in input.iter().enumerate() {
            let a = input[(i + input.len() - 1) % input.len()];
            match (inside(a), inside(b)) {
                (true, true) => out.push(b),
                (true, false) => out.push(cross(a, b)),
                (false, true) => out.extend([cross(a, b), b]),
                (false, false) => {}
            }
        }
    }
    out
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

    fn built_in() -> Sim {
        Sim::new(g2_rules::crater_ridge(), None, 1, Sim::players([g2_sim::Faction::Eu, g2_sim::Faction::Gla], true))
    }

    #[test]
    fn playable_area_stays_flat_enough_for_the_flat_sim_ground() {
        let sim = built_in();
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
        let sim = built_in();
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

    #[test]
    fn props_under_a_building_are_hidden_and_return_when_it_is_gone() {
        let mut sim = built_in();
        // Cover only: with the fog on, the far prop would stay hidden as unexplored.
        sim.fog = false;
        let cc = sim.world.entities().find(|e| sim.world.rules.def(e.def).is_building()).unwrap();
        let (at, owner) = (to_world(cc.pos).xz(), cc.owner.unwrap());
        let mut app = App::new();
        app.insert_resource(sim).add_systems(Update, clear_under_buildings);
        let under = app.world_mut().spawn((Scenery { at, radius: 1.0 }, Visibility::default())).id();
        let far =
            app.world_mut().spawn((Scenery { at: at + Vec2::splat(200.0), radius: 1.0 }, Visibility::default())).id();
        app.update();
        assert_eq!(app.world().get::<Visibility>(under), Some(&Visibility::Hidden));
        assert_eq!(app.world().get::<Visibility>(far), Some(&Visibility::Inherited));
        // Surrender removes all of that player's buildings at the next cleanup.
        let mut sim = app.world_mut().resource_mut::<Sim>();
        sim.world.submit(owner, g2_sim::CommandKind::Surrender);
        for _ in 0..10 {
            sim.world.step();
        }
        assert!(sim.world.entities().all(|e| e.owner != Some(owner)));
        app.update();
        assert_eq!(app.world().get::<Visibility>(under), Some(&Visibility::Inherited));
    }

    /// Under fog, props stand taller than the fog sheet, so those on unexplored ground are
    /// hidden; they appear once a unit has seen the spot.
    #[test]
    fn props_on_unexplored_ground_are_hidden_until_explored() {
        // A human local player and a passive opponent, so only our dozer explores.
        let players = Sim::players([g2_sim::Faction::Eu, g2_sim::Faction::Gla], false)
            .map(|p| g2_sim::PlayerSetup { ai: None, ..p });
        let sim = Sim::new(g2_rules::crater_ridge(), None, 1, players);
        let dozer = sim
            .world
            .entities()
            .find(|e| e.owner == Some(sim.local) && sim.world.rules.def(e.def).is_constructor())
            .unwrap();
        let (home, dozer) = (to_world(dozer.pos).xz(), dozer.id);
        let spot = home + Vec2::new(70.0, 0.0);
        let mut app = App::new();
        app.insert_resource(sim).add_systems(Update, clear_under_buildings);
        // Beside the dozer, clear of the command center's footprint: seen from the start.
        let (near, out) = (home + Vec2::new(8.0, 4.0), spot);
        let near = app.world_mut().spawn((Scenery { at: near, radius: 0.5 }, Visibility::default())).id();
        let out = app.world_mut().spawn((Scenery { at: out, radius: 0.5 }, Visibility::default())).id();
        app.update();
        assert_eq!(app.world().get::<Visibility>(near), Some(&Visibility::Inherited), "seen from the start");
        assert_eq!(app.world().get::<Visibility>(out), Some(&Visibility::Hidden), "not explored yet");
        let mut sim = app.world_mut().resource_mut::<Sim>();
        sim.order(g2_sim::CommandKind::Move {
            units: vec![dozer],
            target: crate::sim::to_sim(Vec3::new(spot.x, 0.0, spot.y)),
        });
        // 70 units at the dozer's speed is well under a minute.
        for _ in 0..30 * 60 {
            if sim.explored_at(spot.x, spot.y) {
                break;
            }
            sim.world.step();
        }
        assert!(sim.explored_at(spot.x, spot.y), "the dozer reached the spot");
        app.update();
        assert_eq!(app.world().get::<Visibility>(out), Some(&Visibility::Inherited), "explored now");
    }

    /// An enemy building raised in the fog leaves the props on its footprint alone: a clearing
    /// in an explored grove would give it away. The spot is explored (our dozer drove there and
    /// back) but out of sight when the enemy builds on it.
    #[test]
    fn an_unseen_enemy_building_does_not_clear_props() {
        let players = Sim::players([g2_sim::Faction::Eu, g2_sim::Faction::Gla], false)
            .map(|p| g2_sim::PlayerSetup { ai: None, ..p });
        let mut sim = Sim::new(g2_rules::crater_ridge(), None, 1, players);
        let mine = |sim: &Sim, owner| {
            sim.world
                .entities()
                .find(|e| e.owner == Some(owner) && sim.world.rules.def(e.def).is_constructor())
                .unwrap()
                .id
        };
        let (ours, theirs) = (mine(&sim, sim.local), mine(&sim, g2_sim::PlayerId(1)));
        let home = sim.world.entity(ours).unwrap().pos;
        let spot = g2_sim::Vec2::from_int(200, 140) + g2_sim::Vec2::from_int(30, 30);
        sim.order(g2_sim::CommandKind::Move { units: vec![ours], target: spot });
        for _ in 0..30 * 120 {
            if sim.world.vision(sim.local).is_visible(spot) {
                break;
            }
            sim.world.step();
        }
        sim.order(g2_sim::CommandKind::Move { units: vec![ours], target: home });
        for _ in 0..30 * 120 {
            if !sim.world.vision(sim.local).is_visible(spot) {
                break;
            }
            sim.world.step();
        }
        let explored_out_of_sight = |s: &Sim| {
            let v = s.world.vision(s.local);
            v.is_explored(spot) && !v.is_visible(spot)
        };
        assert!(explored_out_of_sight(&sim), "the spot is explored, then left");
        let barracks = sim.world.rules.find("GLA Barracks").unwrap();
        sim.world.submit(g2_sim::PlayerId(1), g2_sim::CommandKind::Build { builder: theirs, def: barracks, pos: spot });
        let built = |s: &Sim| s.world.entities().any(|e| e.def == barracks && e.pos == spot);
        for _ in 0..30 * 120 {
            if built(&sim) {
                break;
            }
            sim.world.step();
        }
        assert!(built(&sim) && explored_out_of_sight(&sim), "a foundation stands on the spot, unseen");
        let at = to_world(spot).xz();
        let mut app = App::new();
        app.insert_resource(sim).add_systems(Update, clear_under_buildings);
        let prop = app.world_mut().spawn((Scenery { at, radius: 1.0 }, Visibility::default())).id();
        app.update();
        assert_eq!(app.world().get::<Visibility>(prop), Some(&Visibility::Inherited), "the grove gives nothing away");
        app.world_mut().resource_mut::<Sim>().fog = false;
        app.update();
        assert_eq!(app.world().get::<Visibility>(prop), Some(&Visibility::Hidden), "shown, the foundation clears it");
    }

    /// A remembered building destroyed out of sight keeps the props on its footprint hidden
    /// (trees growing back would give the kill away) until its spot is seen again.
    #[test]
    fn a_ghost_keeps_its_props_hidden_until_seen() {
        let players = Sim::players([g2_sim::Faction::Eu, g2_sim::Faction::Gla], false)
            .map(|p| g2_sim::PlayerSetup { ai: None, ..p });
        let sim = Sim::new(g2_rules::crater_ridge(), None, 1, players);
        let enemy = g2_sim::PlayerId(1);
        let rules = sim.world.rules;
        let dozer =
            sim.world.entities().find(|e| e.owner == Some(sim.local) && rules.def(e.def).is_constructor()).unwrap().id;
        let cc = sim.world.entities().find(|e| e.owner == Some(enemy) && rules.def(e.def).is_building()).unwrap();
        let (cc, cc_pos) = (cc.id, cc.pos);
        let home = sim.world.entity(dozer).unwrap().pos;
        let mut app = App::new();
        app.insert_resource(sim).add_systems(Update, clear_under_buildings);
        let prop =
            app.world_mut().spawn((Scenery { at: to_world(cc_pos).xz(), radius: 1.0 }, Visibility::default())).id();
        let step_until = |app: &mut App, done: &dyn Fn(&Sim) -> bool, what: &str| {
            for _ in 0..30 * 120 {
                if done(app.world().resource::<Sim>()) {
                    return;
                }
                app.world_mut().resource_mut::<Sim>().world.step();
            }
            panic!("timed out: {what}");
        };
        let order = |app: &mut App, target| {
            app.world_mut().resource_mut::<Sim>().order(g2_sim::CommandKind::Move { units: vec![dozer], target });
        };
        let near = cc_pos - g2_sim::Vec2::from_int(14, 14);
        order(&mut app, near);
        step_until(&mut app, &|s| s.remembers(cc), "scouted");
        order(&mut app, home);
        step_until(&mut app, &|s| !s.sees(s.world.entity(cc).unwrap()), "out of sight");
        app.world_mut().resource_mut::<Sim>().world.submit(enemy, g2_sim::CommandKind::Surrender);
        step_until(&mut app, &|s| s.world.entity(cc).is_none(), "destroyed");
        app.update();
        assert_eq!(app.world().get::<Visibility>(prop), Some(&Visibility::Hidden), "the ghost keeps its clearing");
        order(&mut app, near);
        step_until(&mut app, &|s| !s.remembers(cc), "seen again");
        app.update();
        assert_eq!(app.world().get::<Visibility>(prop), Some(&Visibility::Inherited), "trees back once seen");
    }

    fn imported(name: &str) -> Sim {
        let file = g2_maps::load_file_named(&g2_maps::vendored_root(), name).unwrap();
        let map = file.to_map_def(name);
        Sim::new(map, Some(file), 1, Sim::players([g2_sim::Faction::Eu, g2_sim::Faction::Gla], true))
    }

    /// Imported terrain: bases sit on the ground they were designed for, original trees are
    /// placed but never on a base or in water, and the edge ridge continues smoothly.
    #[test]
    fn imported_terrain_keeps_the_original_layout() {
        let sim = imported("zh:Tournament Desert");
        let layout = Layout::from_sim(&sim);
        // Start positions are on the battlefield level (base_height), not on a hill or in a pit.
        for &s in &sim.world.map.spawns {
            let p = to_world(s).xz();
            assert!(layout.height(p).abs() < 4.0, "start at {p} is at height {}", layout.height(p));
        }
        let props = scatter(&layout);
        assert!(props.len() > 100, "{} props from the original placements", props.len());
        for (_, p, _, _) in &props {
            assert!(layout.water_depth(*p).is_none() && layout.border(*p) < 0.05);
            assert!(layout.clearings.iter().all(|&(c, r)| c.distance(*p) > r));
        }
        // Past the edge the ridge rises smoothly: no steps between neighbouring vertices.
        for i in 0..200 {
            let x = i as f32 * layout.width / 200.0;
            let (a, b) = (layout.height(Vec2::new(x, -30.0)), layout.height(Vec2::new(x + 1.5, -30.0)));
            assert!((a - b).abs() < 3.0, "apron step {a} -> {b} at x {x}");
        }
    }

    #[test]
    fn water_surfaces_stay_inside_the_map() {
        let sim = imported("gen:Golden Oasis");
        let file = sim.source.as_ref().unwrap();
        assert!(!file.water.is_empty());
        let (w, h) = file.world_size();
        for water in &file.water {
            let Some(mesh) = water_mesh(file, water) else { continue };
            let Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                panic!("no positions")
            };
            for p in pos {
                assert!((-0.01..=w + 0.01).contains(&p[0]) && (-0.01..=h + 0.01).contains(&p[2]), "{p:?}");
            }
        }
    }
}
