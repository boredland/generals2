//! Scenery shapes merged into vertex-coloured meshes: a tree is one mesh (a few prebuilt
//! variants, so the many trees of a map instance a handful of meshes), and the clutter of a
//! patch of ground is one mesh. All drawn with one plain white material each.
//!
//! Sizes come from the retail objects' geometry in GeneralsGamePatch2's INI (`Object/Faction/
//! Civilian/Prop`, SAGE units / 6): a `Barrel` is a cylinder of radius 0.5 and height 1.0, a
//! `CarScrap` a 3.7 × 1.3 × 1.2 box, a `PicketFence01` 5.3 long and 1.7 tall, a
//! `SandBagWall01` 4.3 long. Fence panels are 4.6 long, the spacing measured between the
//! `ChainLinkFence05` posts of `zh:Red Rock`.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::math::Affine3A;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use super::hash;

/// A unit primitive's triangles, copied into merged shapes.
struct Prim {
    pos: Vec<Vec3>,
    nrm: Vec<Vec3>,
    idx: Vec<u32>,
}

impl Prim {
    fn of(mesh: Mesh) -> Prim {
        let attr = |id| mesh.attribute(id).and_then(|a| a.as_float3()).expect("built-in primitive").to_vec();
        Prim {
            pos: attr(Mesh::ATTRIBUTE_POSITION).into_iter().map(Vec3::from).collect(),
            nrm: attr(Mesh::ATTRIBUTE_NORMAL).into_iter().map(Vec3::from).collect(),
            idx: mesh.indices().expect("indexed").iter().map(|i| i as u32).collect(),
        }
    }
}

/// The unit primitives every shape is built from: a 1×1×1 cube and a cylinder (radius 0.5,
/// height 1) centred on the origin, spheres of radius 1, and a tyre-like torus lying flat.
pub(super) struct Prims {
    cube: Prim,
    cylinder: Prim,
    lump: Prim,
    pebble: Prim,
    torus: Prim,
}

impl Prims {
    pub fn new() -> Prims {
        Prims {
            cube: Prim::of(Cuboid::new(1.0, 1.0, 1.0).mesh().build()),
            cylinder: Prim::of(Cylinder::new(0.5, 1.0).mesh().resolution(10).build()),
            lump: Prim::of(Sphere::new(1.0).mesh().ico(2).expect("small subdivision")),
            pebble: Prim::of(Sphere::new(1.0).mesh().ico(0).expect("small subdivision")),
            torus: Prim::of(Torus::new(0.35, 1.0).mesh().minor_resolution(6).major_resolution(12).build()),
        }
    }
}

/// Vertex data being merged into one mesh.
#[derive(Default)]
pub(super) struct Shape {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

impl Shape {
    fn add(&mut self, prim: &Prim, tf: Affine3A, color: [f32; 4]) {
        let base = self.positions.len() as u32;
        let normal = Mat3::from(tf.matrix3).inverse().transpose();
        for (&p, &n) in prim.pos.iter().zip(&prim.nrm) {
            self.positions.push(tf.transform_point3(p).into());
            self.normals.push((normal * n).normalize_or_zero().into());
            self.colors.push(color);
        }
        self.indices.extend(prim.idx.iter().map(|i| i + base));
    }

    /// A cylinder of `radius` from `a` to `b`.
    fn rod(&mut self, prims: &Prims, place: Affine3A, a: Vec3, b: Vec3, radius: f32, color: [f32; 4]) {
        let d = b - a;
        let tf = Affine3A::from_scale_rotation_translation(
            Vec3::new(radius * 2.0, d.length(), radius * 2.0),
            Quat::from_rotation_arc(Vec3::Y, d.normalize_or(Vec3::Y)),
            (a + b) / 2.0,
        );
        self.add(&prims.cylinder, place * tf, color);
    }

    /// A two-sided leaf blade along `line`, `widths` across at each point, flat along `side`.
    fn blade(&mut self, line: &[Vec3], widths: &[f32], side: Vec3, color: [f32; 4]) {
        for flip in [false, true] {
            let base = self.positions.len() as u32;
            for (i, (&p, &w)) in line.iter().zip(widths).enumerate() {
                let along = line[(i + 1).min(line.len() - 1)] - line[i.saturating_sub(1)];
                let n = side.cross(along).normalize_or(Vec3::Y);
                let n = if flip { -n } else { n };
                for q in [p - side * w, p + side * w] {
                    self.positions.push(q.into());
                    self.normals.push(n.into());
                    self.colors.push(color);
                }
            }
            for i in 0..line.len() as u32 - 1 {
                let (r0, l0, r1, l1) = (base + 2 * i, base + 2 * i + 1, base + 2 * i + 2, base + 2 * i + 3);
                if flip {
                    self.indices.extend_from_slice(&[r0, r1, l0, l0, r1, l1]);
                } else {
                    self.indices.extend_from_slice(&[r0, l0, r1, l0, l1, r1]);
                }
            }
        }
    }

    pub fn mesh(self) -> Mesh {
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
        mesh.insert_indices(Indices::U32(self.indices));
        mesh
    }
}

/// An sRGB colour as a linear vertex colour.
fn lin(r: f32, g: f32, b: f32) -> [f32; 4] {
    Color::srgb(r, g, b).to_linear().to_f32_array()
}

fn at(scale: Vec3, rot: Quat, pos: Vec3) -> Affine3A {
    Affine3A::from_scale_rotation_translation(scale, rot, pos)
}

const CANOPY: (f32, f32, f32) = (0.20, 0.24, 0.09);
const CANOPY_DARK: (f32, f32, f32) = (0.13, 0.16, 0.07);
const CANOPY_DRY: (f32, f32, f32) = (0.30, 0.28, 0.12);
const BARK: (f32, f32, f32) = (0.22, 0.17, 0.12);

fn rgb(c: (f32, f32, f32)) -> [f32; 4] {
    lin(c.0, c.1, c.2)
}

/// Acacia `variant`, about 5 tall: a slender trunk forking into two leaning limbs, under flat
/// crowns of small lit and shaded lumps (a broken, leafy edge instead of a solid plate).
pub(super) fn acacia(prims: &Prims, variant: u32) -> Mesh {
    let r = |k: u32| hash(variant as i32, 7, 1300 + k);
    let mut s = Shape::default();
    let h = 4.4 + r(90) * 1.2;
    let lean = Vec3::new(r(0) - 0.5, 0.0, r(1) - 0.5) * 0.5;
    let fork = Vec3::Y * h * 0.55 + lean * h * 0.3;
    let one = Affine3A::IDENTITY;
    s.rod(prims, one, Vec3::ZERO, fork, 0.25, rgb(BARK));
    let tips = [Vec3::new(1.6, h, 0.6), Vec3::new(-1.3, h * 0.92, -0.9)];
    for tip in tips {
        s.rod(prims, one, fork, tip + lean, 0.15, rgb(BARK));
    }
    let crowns = [(tips[0] + lean, 2.4), (tips[1] + lean, 2.0)];
    for (c, (centre, spread)) in crowns.into_iter().enumerate() {
        for k in 0..6u32 {
            let n = c as u32 * 16 + k;
            let a = k as f32 / 6.0 * TAU + r(10 + n);
            let d = if k == 0 { 0.0 } else { spread * (0.45 + r(30 + n) * 0.35) };
            let off = Vec3::new(a.cos() * d, (r(50 + n) - 0.5) * 0.5, a.sin() * d);
            let size = 1.1 + r(70 + n) * 0.6;
            let colour = [CANOPY, CANOPY_DARK, CANOPY_DRY][((k + c as u32) % 3) as usize];
            s.add(&prims.lump, at(Vec3::new(size, size * 0.32, size), Quat::IDENTITY, centre + off), rgb(colour));
        }
    }
    s.mesh()
}

/// Bush `variant`: a low clump of three lumps, about 1.8 across.
pub(super) fn bush(prims: &Prims, variant: u32) -> Mesh {
    let r = |k: u32| hash(variant as i32, 9, 1400 + k);
    let mut s = Shape::default();
    for k in 0..3 {
        let colour = match k {
            1 => CANOPY,
            2 if r(20) < 0.4 => CANOPY_DRY,
            _ => CANOPY_DARK,
        };
        let off = Vec3::new(r(k * 2) - 0.5, 0.0, r(k * 2 + 1) - 0.5) * 1.8;
        let size = 0.7 + r(10 + k) * 0.6;
        s.add(
            &prims.lump,
            at(Vec3::new(1.1, 0.75, 1.0) * size, Quat::IDENTITY, off + Vec3::Y * 0.45 * size),
            rgb(colour),
        );
    }
    s.mesh()
}

/// Palm `variant`: a ringed trunk curving up 6-8, a crown of drooping fronds about 3.5 long
/// with a few dead ones hanging under it. What the footage's desert roadsides have.
pub(super) fn palm(prims: &Prims, variant: u32) -> Mesh {
    let r = |k: u32| hash(variant as i32, 11, 1500 + k);
    let mut s = Shape::default();
    let one = Affine3A::IDENTITY;
    let h = 6.0 + r(0) * 2.0;
    let bend = Vec3::new(0.6 + r(1) * 1.2, 0.0, (r(2) - 0.5) * 0.6);
    let trunk = |t: f32| Vec3::Y * h * t + bend * t * t;
    const SEGMENTS: u32 = 6;
    for k in 0..SEGMENTS {
        let (t0, t1) = (k as f32 / SEGMENTS as f32, (k + 1) as f32 / SEGMENTS as f32);
        let colour = if k % 2 == 0 { lin(0.36, 0.30, 0.22) } else { lin(0.29, 0.24, 0.18) };
        s.rod(prims, one, trunk(t0), trunk(t1), 0.3 - 0.12 * t0, colour);
    }
    let top = trunk(1.0);
    for k in 0..3 {
        let a = k as f32 / 3.0 * TAU;
        let nut = top + Vec3::new(a.cos() * 0.25, -0.25, a.sin() * 0.25);
        s.add(&prims.pebble, at(Vec3::splat(0.2), Quat::IDENTITY, nut), lin(0.25, 0.20, 0.10));
    }
    let fronds = 8 + (r(3) * 3.0) as u32;
    // Living fronds arch out and droop at the tips; three dead ones hang under the crown.
    let living = [lin(0.24, 0.30, 0.10), lin(0.16, 0.22, 0.08), lin(0.34, 0.33, 0.14)];
    for f in 0..fronds + 3 {
        let dead = f >= fronds;
        let a = f as f32 / fronds as f32 * TAU + r(10 + f) * 0.5;
        let (dir, side) = (Vec3::new(a.cos(), 0.0, a.sin()), Vec3::new(-a.sin(), 0.0, a.cos()));
        let (len, lift, droop) = if dead {
            (1.8 + r(30 + f) * 0.6, -0.6, 0.6)
        } else {
            let lift = 0.5 + r(50 + f) * 0.4;
            (3.0 + r(30 + f) * 1.0, lift, lift + 0.4 + r(70 + f) * 0.3)
        };
        let colour = if dead { lin(0.42, 0.34, 0.20) } else { living[(f % 3) as usize] };
        let line: Vec<Vec3> = (0..6)
            .map(|i| {
                let u = i as f32 / 5.0;
                top + dir * len * u + Vec3::Y * len * (lift * u - droop * u * u)
            })
            .collect();
        s.blade(&line, &[0.08, 0.42, 0.5, 0.4, 0.25, 0.04], side, colour);
    }
    s.mesh()
}

/// Small junk lying about, after the props of the footage's bases and roadsides.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Clutter {
    /// That many wooden or olive crates (1-3), stacked.
    Crates(u8),
    /// That many oil drums (1-4), one sometimes tipped over.
    Barrels(u8),
    /// A short curved wall of sandbags, two rows.
    Sandbags,
    /// That many fence panels in a row, posts and two rails.
    Fence(u8),
    /// A wooden utility pole with a crossarm.
    Pole,
    /// That many old tyres (1-3), stacked or leaning.
    Tyres(u8),
    /// A low pile of broken concrete, brick and charred bits.
    Rubble,
    /// A dusty civilian car.
    Car,
    /// A burnt-out civilian car, wheels missing.
    Wreck,
}

/// Fence panel length (see the module doc).
const PANEL: f32 = 4.6;

impl Clutter {
    /// What stands at the roadside in a town: on an imported map these may stand inside our
    /// road band.
    pub fn kerbside(self) -> bool {
        matches!(self, Clutter::Car | Clutter::Wreck | Clutter::Pole)
    }

    /// Ground footprint reach from the item's position, for keeping it off sites and for
    /// hiding it under a building.
    pub fn radius(self) -> f32 {
        match self {
            Clutter::Crates(_) => 1.4,
            Clutter::Barrels(_) => 1.1,
            Clutter::Sandbags => 2.3,
            Clutter::Fence(n) => PANEL / 2.0 * n as f32 + 0.2,
            Clutter::Pole => 1.0,
            Clutter::Tyres(_) => 0.9,
            Clutter::Rubble => 1.6,
            Clutter::Car | Clutter::Wreck => 2.0,
        }
    }

    /// The junk a retail map object stands for, by template name (lower case), if any.
    /// Money and level-up crates are pickups in the original, not scenery.
    pub fn of_template(t: &str) -> Option<Clutter> {
        let starts = |keys: &[&str]| keys.iter().any(|k| t.starts_with(k));
        Some(if t.contains("dollarcrate") || t.contains("levelupcrate") || t.starts_with("cine") {
            return None;
        } else if starts(&["chainlinkfence", "picketfence"]) {
            Clutter::Fence(1)
        } else if t.starts_with("sandbag") {
            Clutter::Sandbags
        } else if starts(&["barrel", "55gallondrum", "bigwoodenbarrels"]) {
            Clutter::Barrels(1)
        } else if t.contains("crate") || t.starts_with("cargonet") {
            Clutter::Crates(1)
        } else if t.starts_with("tire") {
            Clutter::Tyres(2)
        } else if t.contains("rubble") {
            Clutter::Rubble
        } else if starts(&["carscrap", "carjunk"]) || (t.starts_with("burnt") && t.contains("hulk")) {
            Clutter::Wreck
        } else if t.starts_with("car") && !starts(&["cargo", "cart"]) {
            Clutter::Car
        } else if ["streetlamp", "streetlight", "telephonepole", "powerpole"].iter().any(|k| t.contains(k)) {
            Clutter::Pole
        } else {
            return None;
        })
    }
}

/// Appends `kind` to `s`, placed by `place` (origin on the ground, local X along its length);
/// `seed` varies colours and arrangement.
pub(super) fn clutter(prims: &Prims, s: &mut Shape, kind: Clutter, place: Affine3A, seed: u32) {
    let r = |k: u32| hash(seed as i32, k as i32, 2100);
    let pick = |k: u32, colours: &[[f32; 4]]| colours[(r(k) * colours.len() as f32) as usize % colours.len()];
    let tilt = |k: u32, amount: f32| Quat::from_euler(EulerRot::YXZ, r(k) * TAU, (r(k + 1) - 0.5) * amount, 0.0);
    match kind {
        Clutter::Crates(n) => {
            let woods = [lin(0.46, 0.37, 0.25), lin(0.36, 0.36, 0.23), lin(0.52, 0.45, 0.33), lin(0.40, 0.33, 0.24)];
            let spots = [Vec3::ZERO, Vec3::new(1.1, 0.0, (r(9) - 0.5) * 0.4), Vec3::new(0.1, 1.0, 0.05)];
            for (k, spot) in spots.into_iter().take(n as usize).enumerate() {
                let k = k as u32;
                let size = if k == 2 { 0.75 } else { 0.85 + r(10 + k) * 0.3 };
                let rot = Quat::from_rotation_y((r(20 + k) - 0.5) * 0.5);
                let centre = spot + Vec3::Y * size / 2.0;
                let wood = pick(30 + k, &woods);
                s.add(&prims.cube, place * at(Vec3::splat(size), rot, centre), wood);
                // A darker band around the middle: the crate's battens.
                let band = [wood[0] * 0.6, wood[1] * 0.6, wood[2] * 0.6, 1.0];
                s.add(&prims.cube, place * at(Vec3::new(size * 1.03, size * 0.14, size * 1.03), rot, centre), band);
            }
        }
        Clutter::Barrels(n) => {
            let paints = [lin(0.44, 0.25, 0.14), lin(0.30, 0.32, 0.20), lin(0.48, 0.46, 0.40), lin(0.22, 0.22, 0.21)];
            for k in 0..n as u32 {
                let a = k as f32 * 2.1 + r(40) * TAU;
                let spot = if k == 0 { Vec3::ZERO } else { Vec3::new(a.cos(), 0.0, a.sin()) * 0.8 };
                let paint = pick(50 + k, &paints);
                let dark = [paint[0] * 0.55, paint[1] * 0.55, paint[2] * 0.55, 1.0];
                // The last of several sometimes lies on its side.
                let (rot, lift) = if n > 1 && k == n as u32 - 1 && r(60) < 0.4 {
                    (Quat::from_rotation_y(r(61) * TAU) * Quat::from_rotation_x(FRAC_PI_2), 0.4)
                } else {
                    (Quat::IDENTITY, 0.5)
                };
                let centre = spot + Vec3::Y * lift;
                s.add(&prims.cylinder, place * at(Vec3::new(0.8, 1.0, 0.8), rot, centre), paint);
                for ring in [-0.2, 0.2] {
                    let c = centre + rot * Vec3::Y * ring;
                    s.add(&prims.cylinder, place * at(Vec3::new(0.84, 0.06, 0.84), rot, c), dark);
                }
            }
        }
        Clutter::Sandbags => {
            let sand = [lin(0.56, 0.48, 0.34), lin(0.50, 0.43, 0.30), lin(0.60, 0.53, 0.38)];
            for (row, (count, y)) in [(5, 0.17), (4, 0.47)].into_iter().enumerate() {
                for k in 0..count {
                    let x = (k as f32 - (count - 1) as f32 / 2.0) * 0.82;
                    let z = x * x / 8.0;
                    let n = row as u32 * 8 + k;
                    let rot = Quat::from_rotation_y(-(x / 4.0).atan() + (r(70 + n) - 0.5) * 0.3);
                    let bag = at(Vec3::new(0.46, 0.18, 0.3), rot, Vec3::new(x, y, z));
                    s.add(&prims.lump, place * bag, pick(80 + n, &sand));
                }
            }
        }
        Clutter::Fence(n) => {
            let wood = [lin(0.40, 0.35, 0.28), lin(0.34, 0.31, 0.26)];
            let post = lin(0.28, 0.24, 0.19);
            let start = -PANEL * n as f32 / 2.0;
            for k in 0..=2 * n as u32 {
                let x = start + k as f32 * PANEL / 2.0;
                let lean = Quat::from_rotation_z((r(90 + k) - 0.5) * 0.12);
                s.add(&prims.cube, place * at(Vec3::new(0.14, 1.6, 0.14), lean, Vec3::new(x, 0.8, 0.0)), post);
            }
            for k in 0..n as u32 {
                let x = start + (k as f32 + 0.5) * PANEL;
                for (j, y) in [0.55, 1.25].into_iter().enumerate() {
                    // A rail missing here and there: the fence is old.
                    if r(110 + k * 2 + j as u32) < 0.15 {
                        continue;
                    }
                    let rail = at(Vec3::new(PANEL, 0.09, 0.05), Quat::IDENTITY, Vec3::new(x, y, 0.04));
                    s.add(&prims.cube, place * rail, pick(130 + k, &wood));
                }
            }
        }
        Clutter::Pole => {
            let wood = lin(0.28, 0.22, 0.16);
            let lean = Quat::from_rotation_x((r(140) - 0.5) * 0.06);
            s.rod(prims, place, Vec3::ZERO, lean * Vec3::Y * 6.6, 0.13, wood);
            let top = lean * Vec3::Y * 6.1;
            s.add(&prims.cube, place * at(Vec3::new(1.8, 0.14, 0.14), lean, top), wood);
            for side in [-0.75, 0.75] {
                let insulator = at(Vec3::new(0.12, 0.2, 0.12), lean, top + lean * Vec3::new(side, 0.16, 0.0));
                s.add(&prims.cylinder, place * insulator, lin(0.62, 0.60, 0.55));
            }
        }
        Clutter::Tyres(n) => {
            let rubber = lin(0.10, 0.09, 0.09);
            for k in 0..n as u32 {
                // Stacked, or the top one leaning against the stack.
                let (rot, centre) = if k == 2 {
                    (Quat::from_rotation_y(r(150) * TAU) * Quat::from_rotation_x(1.2), Vec3::new(0.0, 0.4, 0.55))
                } else {
                    (tilt(152 + k * 2, 0.15), Vec3::new((r(156 + k) - 0.5) * 0.2, 0.15 + k as f32 * 0.28, 0.0))
                };
                s.add(&prims.torus, place * at(Vec3::new(0.45, 0.5, 0.45), rot, centre), rubber);
            }
        }
        Clutter::Rubble => {
            let bits = [lin(0.50, 0.48, 0.44), lin(0.48, 0.32, 0.24), lin(0.18, 0.16, 0.15), lin(0.55, 0.48, 0.38)];
            let count = 5 + (r(160) * 5.0) as u32;
            for k in 0..count {
                let a = r(170 + k) * TAU;
                let d = r(190 + k).sqrt() * 1.2;
                let size = Vec3::new(0.3 + r(210 + k) * 0.6, 0.2 + r(230 + k) * 0.3, 0.3 + r(250 + k) * 0.5);
                let centre = Vec3::new(a.cos() * d, size.y * 0.2, a.sin() * d);
                let prim = if k % 3 == 0 { &prims.cube } else { &prims.pebble };
                let scale = if k % 3 == 0 { size } else { size * 0.6 };
                s.add(prim, place * at(scale, tilt(270 + k * 2, 0.7), centre), pick(300 + k, &bits));
            }
        }
        Clutter::Car | Clutter::Wreck => {
            let wreck = kind == Clutter::Wreck;
            let paints = [lin(0.66, 0.63, 0.56), lin(0.60, 0.52, 0.38), lin(0.36, 0.40, 0.32), lin(0.48, 0.32, 0.25)];
            let (body, roof, glass) = if wreck {
                (lin(0.19, 0.16, 0.14), lin(0.42, 0.26, 0.16), lin(0.08, 0.07, 0.07))
            } else {
                let paint = pick(320, &paints);
                (paint, paint, lin(0.12, 0.13, 0.14))
            };
            // A wreck sits low and askew on its rims.
            let (sink, roll) = if wreck { (0.12, (r(321) - 0.5) * 0.2) } else { (0.0, 0.0) };
            let lean = Quat::from_rotation_x(roll);
            let car = |v: Vec3| lean * (v - Vec3::Y * sink);
            s.add(&prims.cube, place * at(Vec3::new(3.6, 0.55, 1.6), lean, car(Vec3::new(0.0, 0.56, 0.0))), body);
            let cabin_h = if wreck { 0.36 } else { 0.5 };
            let cabin = car(Vec3::new(-0.25, 0.83 + cabin_h / 2.0, 0.0));
            s.add(&prims.cube, place * at(Vec3::new(1.9, cabin_h, 1.45), lean, cabin), roof);
            s.add(&prims.cube, place * at(Vec3::new(1.94, cabin_h * 0.55, 1.49), lean, cabin), glass);
            let bumper = lin(0.15, 0.14, 0.13);
            for x in [-1.82, 1.82] {
                s.add(&prims.cube, place * at(Vec3::new(0.1, 0.18, 1.62), lean, car(Vec3::new(x, 0.4, 0.0))), bumper);
            }
            for (k, (x, z)) in [(-1.15, -0.72), (-1.15, 0.72), (1.15, -0.72), (1.15, 0.72)].into_iter().enumerate() {
                if wreck && r(330 + k as u32) < 0.5 {
                    continue;
                }
                let wheel = at(
                    Vec3::new(0.62, 0.22, 0.62),
                    lean * Quat::from_rotation_x(FRAC_PI_2),
                    car(Vec3::new(x, 0.31, z)),
                );
                s.add(&prims.cylinder, place * wheel, lin(0.09, 0.08, 0.08));
            }
        }
    }
}
