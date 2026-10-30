//! Where the junk goes. The footage's ground is littered: fences, poles and dumped tyres along
//! the roads, sandbags, crates and drums around the bases and resources, rubble and the odd
//! wrecked car out in the field. Placement is deterministic for a map (`Layout::seed`); on an
//! imported map the original's own civilian props (fences, drums, crates, cars...) are kept
//! where its author put them, and ours are added along its roads and around its bases.
//!
//! Props are visual only (the sim doesn't know them), so every one stays off the build sites
//! (`Layout::sites`: base areas, resources, civilian buildings), the roads, water, slopes and
//! the trees, and they don't overlap each other.

use std::f32::consts::TAU;

use bevy::math::Affine3A;
use bevy::prelude::*;

use super::shapes::{self, Clutter, Prims, Shape};
use super::{BASE_SITE, Layout, MapFile, RESOURCE_SITE, ROAD_HALF_WIDTH, hash};

/// One piece of junk on the ground.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct Item {
    pub kind: Clutter,
    pub at: Vec2,
    /// Turn about the vertical; a fence runs along its local X, which `yaw` 0 points along +x.
    pub yaw: f32,
    pub seed: u32,
}

impl Item {
    /// Ground circles the item covers: one for most, a row along a fence.
    pub fn footprint(&self) -> Vec<(Vec2, f32)> {
        match self.kind {
            Clutter::Fence(n) => {
                let along = Vec2::new(self.yaw.cos(), -self.yaw.sin());
                let half = self.kind.radius() - 0.2;
                let steps = 2 * n as i32;
                (0..=steps).map(|k| (self.at + along * (k as f32 / steps as f32 * 2.0 - 1.0) * half, 0.5)).collect()
            }
            kind => vec![(self.at, kind.radius())],
        }
    }

    /// Where the item stands, origin on the ground.
    pub fn place(&self, layout: &Layout) -> Affine3A {
        let ground = layout.height(self.at) - 0.05;
        Affine3A::from_rotation_translation(Quat::from_rotation_y(self.yaw), Vec3::new(self.at.x, ground, self.at.y))
    }
}

/// Grows the item list, rejecting any that break the placement rules.
struct Placer<'a> {
    layout: &'a Layout<'a>,
    /// Tree trunks and bushes to keep clear of: position and radius.
    trees: Vec<(Vec2, f32)>,
    taken: Vec<(Vec2, f32)>,
    items: Vec<Item>,
}

impl Placer<'_> {
    fn fits(&self, item: &Item, on_road_ok: bool) -> bool {
        let layout = self.layout;
        // A long fence must stand on nearly level ground: it is one rigid piece.
        let steep = if matches!(item.kind, Clutter::Fence(_)) { 0.15 } else { 0.3 };
        item.footprint().iter().all(|&(p, r)| {
            layout.sites.iter().all(|&(c, s)| c.distance(p) > s + r)
                && (on_road_ok || layout.road(p) > ROAD_HALF_WIDTH + r + 0.3)
                && layout.border(p) < 0.05
                && layout.water_depth(p).is_none()
                && layout.slope(p) < steep
                && self.trees.iter().all(|&(c, t)| c.distance(p) > t + r)
                && self.taken.iter().all(|&(c, t)| c.distance(p) > t + r)
        })
    }

    fn try_add(&mut self, item: Item, on_road_ok: bool) {
        if self.fits(&item, on_road_ok) {
            self.taken.extend(item.footprint());
            self.items.push(item);
        }
    }
}

/// Picks from `table` (weights need not sum to 1) by `roll` in 0..1.
fn pick<T: Copy>(table: &[(f32, T)], roll: f32) -> T {
    let total: f32 = table.iter().map(|(w, _)| w).sum();
    let mut left = roll * total;
    for &(w, t) in table {
        if left < w {
            return t;
        }
        left -= w;
    }
    table[table.len() - 1].1
}

/// Every piece of junk on the map, in placement order. `trees` are the scatter's trees and
/// bushes (position, trunk clearance).
pub(super) fn place(layout: &Layout, trees: Vec<(Vec2, f32)>) -> Vec<Item> {
    let mut placer = Placer { layout, trees, taken: Vec::new(), items: Vec::new() };
    let seed = layout.seed;
    if let Some(file) = layout.source {
        imported(&mut placer, file);
    }
    // Along the roads: a line of utility poles down one side of each long road, and junk
    // dumped on both shoulders.
    for (k, road) in layout.roads.iter().enumerate() {
        let len = road.length();
        let r = |slot: u32, q: u32| hash(k as i32, slot as i32, seed.wrapping_add(4000 + q));
        if len > 60.0 {
            let side = if r(0, 0) < 0.5 { -1.0 } else { 1.0 };
            let poles = (len / 32.0) as u32;
            for n in 1..poles {
                let t = n as f32 / poles as f32;
                let at = road.point(t) + road.normal(t) * side * (ROAD_HALF_WIDTH + 2.2);
                placer.try_add(
                    Item { kind: Clutter::Pole, at, yaw: r(n, 1) * TAU, seed: seed ^ (k as u32 * 977 + n) },
                    false,
                );
            }
        }
        let steps = (len / 7.0) as u32;
        for n in 0..steps {
            if r(n, 2) > 0.6 {
                continue;
            }
            let t = (n as f32 + r(n, 3)) / steps.max(1) as f32;
            let side = if r(n, 4) < 0.5 { -1.0 } else { 1.0 };
            let kind = pick(
                &[
                    (0.22, Clutter::Fence(1 + (r(n, 5) * 2.0) as u8)),
                    (0.14, Clutter::Barrels(1 + (r(n, 5) * 3.0) as u8)),
                    (0.12, Clutter::Crates(1 + (r(n, 5) * 2.0) as u8)),
                    (0.16, Clutter::Tyres(1 + (r(n, 5) * 3.0) as u8)),
                    (0.12, Clutter::Rubble),
                    (0.06, Clutter::Sandbags),
                    (0.08, Clutter::Wreck),
                    (0.05, Clutter::Car),
                ],
                r(n, 6),
            );
            let gap = ROAD_HALF_WIDTH + 0.8 + kind.radius() * if matches!(kind, Clutter::Fence(_)) { 0.0 } else { 1.0 };
            let normal = road.normal(t);
            let at = road.point(t) + normal * side * (gap + r(n, 7) * 3.0);
            // Fences and cars line up with the road; the rest lie any way.
            let along = normal.perp();
            let yaw = match kind {
                Clutter::Fence(_) | Clutter::Car | Clutter::Wreck => (-along.y).atan2(along.x) + (r(n, 8) - 0.5) * 0.3,
                _ => r(n, 8) * TAU,
            };
            placer.try_add(Item { kind, at, yaw, seed: seed ^ (k as u32 * 7919 + n * 31) }, false);
        }
    }
    // Around the bases, just outside their build areas: sandbag walls, stacks of supplies and
    // stretches of fence, as at the footage's bases.
    for (k, &base) in layout.bases.iter().enumerate() {
        let r = |slot: u32, q: u32| hash(k as i32, slot as i32, seed.wrapping_add(4100 + q));
        for n in 0..18 {
            let a = (n as f32 + r(n, 0) * 0.7) / 18.0 * TAU;
            let kind = pick(
                &[
                    (0.28, Clutter::Sandbags),
                    (0.2, Clutter::Crates(1 + (r(n, 1) * 3.0) as u8)),
                    (0.18, Clutter::Barrels(1 + (r(n, 1) * 4.0) as u8)),
                    (0.16, Clutter::Fence(1 + (r(n, 1) * 2.0) as u8)),
                    (0.1, Clutter::Rubble),
                    (0.08, Clutter::Tyres(2)),
                ],
                r(n, 2),
            );
            let out = Vec2::from_angle(a);
            let at = base + out * (BASE_SITE + kind.radius() + 1.0 + r(n, 3) * 7.0);
            // Walls and fences run along the base's edge, as if guarding it.
            let yaw = match kind {
                Clutter::Sandbags | Clutter::Fence(_) => (-out.x).atan2(-out.y) + (r(n, 4) - 0.5) * 0.4,
                _ => r(n, 4) * TAU,
            };
            placer.try_add(Item { kind, at, yaw, seed: seed ^ (0x5151 + k as u32 * 131 + n) }, false);
        }
    }
    // Beside each resource: drums by the oil wells, crates by the supply piles.
    for (k, &(pos, oil)) in layout.resources.iter().enumerate() {
        let r = |slot: u32, q: u32| hash(k as i32, slot as i32, seed.wrapping_add(4200 + q));
        for n in 0..4 {
            let kind = if oil {
                Clutter::Barrels(2 + (r(n, 0) * 3.0) as u8)
            } else {
                Clutter::Crates(2 + (r(n, 0) * 2.0) as u8)
            };
            let a = (n as f32 + r(n, 1) * 0.6) / 4.0 * TAU;
            let at = pos + Vec2::from_angle(a) * (RESOURCE_SITE + kind.radius() + 0.5 + r(n, 2) * 3.0);
            placer.try_add(Item { kind, at, yaw: r(n, 3) * TAU, seed: seed ^ (0x7272 + k as u32 * 97 + n) }, false);
        }
    }
    // Out in the field: rubble, tyres, drums and wrecks, one every few hundred square units.
    let step = 26.0;
    let (nx, nz) = ((layout.width / step) as i32, (layout.height / step) as i32);
    for j in 0..nz {
        for i in 0..nx {
            let r = |q: u32| hash(i, j, seed.wrapping_add(4300 + q));
            if r(0) > 0.16 {
                continue;
            }
            let kind = pick(
                &[
                    (0.32, Clutter::Rubble),
                    (0.18, Clutter::Tyres(1 + (r(1) * 2.0) as u8)),
                    (0.22, Clutter::Wreck),
                    (0.14, Clutter::Barrels(1 + (r(1) * 2.0) as u8)),
                    (0.14, Clutter::Crates(1)),
                ],
                r(2),
            );
            let at = Vec2::new((i as f32 + r(3)) * step, (j as f32 + r(4)) * step);
            placer.try_add(Item { kind, at, yaw: r(5) * TAU, seed: seed ^ (0x9393 + (j * nx + i) as u32) }, false);
        }
    }
    placer.items
}

/// The original map's civilian props that we have a shape for, where its author put them.
/// Parked cars and street lamps may stand on a road (our road band is wider than the
/// original's painted road, so its kerbs fall inside it); nothing stands on a build site or in
/// water.
fn imported(placer: &mut Placer, file: &MapFile) {
    for (k, o) in file.objects.iter().enumerate() {
        let template = o.name.rsplit('/').next().unwrap_or(&o.name).to_ascii_lowercase();
        let Some(kind) = Clutter::of_template(&template) else { continue };
        let at = Vec2::from(file.to_world(o.x, o.y));
        // The original's angle turns its local x axis, in its own (y-down) frame.
        let item = Item { kind, at, yaw: o.angle, seed: k as u32 };
        placer.try_add(item, kind.kerbside());
    }
}

/// World units per clutter chunk side: each chunk's junk is one mesh.
const CHUNK: f32 = 48.0;

/// One item in a merged chunk: its footprint (centre, reach) and its run of the chunk's
/// indices.
pub(super) type Run = (Vec2, f32, std::ops::Range<usize>);

/// The items grouped into chunks, each merged into one shape, with a run per item.
pub(super) fn chunks(layout: &Layout, prims: &Prims, items: &[Item]) -> Vec<(Shape, Vec<Run>)> {
    let mut groups: std::collections::BTreeMap<(i32, i32), Vec<&Item>> = Default::default();
    for item in items {
        let cell = ((item.at.x / CHUNK).floor() as i32, (item.at.y / CHUNK).floor() as i32);
        groups.entry(cell).or_default().push(item);
    }
    groups
        .into_values()
        .map(|group| {
            let mut shape = Shape::default();
            let mut runs = Vec::with_capacity(group.len());
            for item in group {
                let start = shape.indices.len();
                shapes::clutter(prims, &mut shape, item.kind, item.place(layout), item.seed);
                runs.push((item.at, item.kind.radius(), start..shape.indices.len()));
            }
            (shape, runs)
        })
        .collect()
}
