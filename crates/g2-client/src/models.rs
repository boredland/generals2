//! Procedural models (docs/STYLE.md): chunky buildings with busy roofs and layered walls,
//! EU grey/white panels with hazard-striped edges, APA dark steel with red accents, GLA scrap
//! walls, sandbags and crate yards. Units are small; constructors are yellow-black; the EU
//! collector is a ducted-fan lifter. Every shape is original and built from primitives.
//!
//! A model is a list of parts. Each part is a primitive, a material slot and a local transform
//! relative to the entity's ground footprint (origin on the ground, Y up). The sim radius sets the
//! scale, so the footprint matches what the sim collides with.
//!
//! The camera looks down at about 57 degrees, so roofs and the front wall carry the silhouette:
//! railings, catwalks, pipe runs, ribs and greebles. Every part is its own entity, so the helpers
//! stay coarse and the tests cap the counts (120 per building, 30 per unit).

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;
use g2_sim::{BuildingRole, Class, Def, Faction};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Slot {
    /// Main hull/wall colour of the faction.
    Hull,
    /// Secondary panel colour (roofs, inner walls).
    Panel,
    /// Roof decking: plates and grilles laid on a flat roof, darker than the walls.
    Deck,
    /// Dark metal: vents, frames, tracks, rotor hubs.
    Dark,
    /// Saturated team colour (stripes, lights, flags).
    Team,
    /// Yellow/black construction and hazard colour.
    Hazard,
    /// Concrete pads and foundations.
    Concrete,
    /// Glass/emissive panels.
    Glow,
    /// Crate colours for GLA yards and supply piles (index picks one of several).
    Crate(u8),
    /// Oil: black, glossy.
    Oil,
    /// Wood pallets and planks.
    Wood,
}

#[derive(Clone, Copy, Debug)]
pub enum Prim {
    Box,
    Cylinder,
    Sphere,
}

#[derive(Clone, Copy, Debug)]
pub struct Part {
    pub prim: Prim,
    pub slot: Slot,
    /// Centre of the part.
    pub at: Vec3,
    /// Full size for boxes; (diameter, height, diameter) for cylinders; diameters for spheres.
    pub size: Vec3,
    pub yaw: f32,
    /// Spins about its local Y when set (rotors).
    pub spin: bool,
}

fn part(prim: Prim, slot: Slot, at: Vec3, size: Vec3) -> Part {
    Part { prim, slot, at, size, yaw: 0.0, spin: false }
}

fn bx(slot: Slot, at: Vec3, size: Vec3) -> Part {
    part(Prim::Box, slot, at, size)
}

fn cyl(slot: Slot, at: Vec3, size: Vec3) -> Part {
    part(Prim::Cylinder, slot, at, size)
}

/// Box resting on `y0`, centred at `(x, z)`.
fn block(slot: Slot, x: f32, z: f32, y0: f32, w: f32, h: f32, d: f32) -> Part {
    bx(slot, Vec3::new(x, y0 + h / 2.0, z), Vec3::new(w, h, d))
}

/// Deterministic small hash for greeble placement (no RNG needed, just variety).
fn h(n: u32) -> f32 {
    let mut x = n.wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    (x & 0xffff) as f32 / 65535.0
}

// ---- Detail helpers ----------------------------------------------------------------------
// The camera sees roofs and the front wall, so every building gets its silhouette from these.

/// Yellow/black chevron edging along the four top edges of a `w × d` roof at height `y`.
/// Segments are coarse: at game zoom a striped band reads the same and costs a quarter of the
/// parts that fine chevrons would.
fn hazard_trim(parts: &mut Vec<Part>, w: f32, d: f32, y: f32) {
    hazard_trim_at(parts, 0.0, 0.0, w, d, y);
}

/// `hazard_trim` around a roof patch centred at `(x, z)`, e.g. a landing pad.
fn hazard_trim_at(parts: &mut Vec<Part>, x: f32, z: f32, w: f32, d: f32, y: f32) {
    let t = 0.42;
    let per = 2.0 * (w + d);
    let n = (per / 3.2) as i32;
    let seg = per / n as f32 * 1.15;
    for i in 0..n {
        let slot = if i % 2 == 0 { Slot::Hazard } else { Slot::Dark };
        // Walk the perimeter from the front-left corner, front edge first.
        let s = (i as f32 + 0.5) * per / n as f32;
        let (px, pz, yaw) = if s < w {
            (x - w / 2.0 + s, z + d / 2.0, 0.0)
        } else if s < w + d {
            (x + w / 2.0, z + d / 2.0 - (s - w), FRAC_PI_2)
        } else if s < 2.0 * w + d {
            (x + w / 2.0 - (s - w - d), z - d / 2.0, 0.0)
        } else {
            (x - w / 2.0, z - d / 2.0 + (s - 2.0 * w - d), FRAC_PI_2)
        };
        let mut p = bx(slot, Vec3::new(px, y, pz), Vec3::new(seg, t, t));
        p.yaw = yaw;
        parts.push(p);
    }
}

/// Railing along the roof edges of a `w × d` patch centred on the origin: posts plus a top rail.
/// `sides` picks the edges: 1 front, 2 right, 4 back, 8 left.
fn roof_rail(parts: &mut Vec<Part>, w: f32, d: f32, y: f32, h: f32, sides: u8) {
    let t = 0.15;
    let edges = [(true, w, d / 2.0), (false, d, w / 2.0), (true, w, -d / 2.0), (false, d, -w / 2.0)];
    for (i, &(along_x, len, off)) in edges.iter().enumerate() {
        if sides & (1u8 << i) == 0 {
            continue;
        }
        parts.push(if along_x {
            bx(Slot::Dark, Vec3::new(0.0, y + h, off), Vec3::new(len, t, t))
        } else {
            bx(Slot::Dark, Vec3::new(off, y + h, 0.0), Vec3::new(t, t, len))
        });
        let n = (len / 4.2).max(1.0) as i32;
        for k in 0..=n {
            let s = -len / 2.0 + len * k as f32 / n as f32;
            let (px, pz) = if along_x { (s, off) } else { (off, s) };
            parts.push(block(Slot::Dark, px, pz, y, t, h, t));
        }
    }
}

/// A catwalk over a roof edge or between blocks: deck, legs and an outer rail.
fn catwalk(parts: &mut Vec<Part>, x: f32, z: f32, len: f32, y: f32, along_x: bool) {
    parts.push(if along_x {
        bx(Slot::Panel, Vec3::new(x, y, z), Vec3::new(len, 0.18, 1.4))
    } else {
        bx(Slot::Panel, Vec3::new(x, y, z), Vec3::new(1.4, 0.18, len))
    });
    for k in 0..2 {
        let s = (k as f32 - 0.5) * len * 0.75;
        let (px, pz) = if along_x { (x + s, z) } else { (x, z + s) };
        parts.push(block(Slot::Dark, px, pz, y - 1.1, 0.3, 1.1, 0.3));
    }
    let (rx, rz) = if along_x { (x, z + 0.65) } else { (x + 0.65, z) };
    parts.push(if along_x {
        bx(Slot::Dark, Vec3::new(rx, y + 0.75, rz), Vec3::new(len, 0.16, 0.16))
    } else {
        bx(Slot::Dark, Vec3::new(rx, y + 0.75, rz), Vec3::new(0.16, 0.16, len))
    });
}

/// A pipe run along a wall or roof: the pipe, its brackets and a riser at the start. The pipe is
/// a box, since cylinders only stand upright.
fn pipe_run(parts: &mut Vec<Part>, x: f32, z: f32, len: f32, y: f32, along_x: bool) {
    let t = 0.34;
    parts.push(if along_x {
        bx(Slot::Panel, Vec3::new(x, y, z), Vec3::new(len, t, t))
    } else {
        bx(Slot::Panel, Vec3::new(x, y, z), Vec3::new(t, t, len))
    });
    for k in 0..2 {
        let s = (k as f32 - 0.5) * len * 0.8;
        let (px, pz) = if along_x { (x + s, z) } else { (x, z + s) };
        parts.push(block(Slot::Dark, px, pz, y - 0.45, 0.5, 0.22, 0.5));
    }
    let (rx, rz) = if along_x { (x - len / 2.0, z) } else { (x, z - len / 2.0) };
    parts.push(cyl(Slot::Panel, Vec3::new(rx, y / 2.0, rz), Vec3::new(t, y, t)));
}

/// Thin ribs standing proud of a wall face: the panel seams of painted metal siding. The face is
/// centred at `(x, z)` and spans `span` along `along_x`, from `y0` up to `y1`.
#[allow(clippy::too_many_arguments)]
fn wall_ribs(parts: &mut Vec<Part>, x: f32, z: f32, y0: f32, y1: f32, span: f32, n: u32, along_x: bool) {
    for k in 0..n {
        let s = -span / 2.0 + (k as f32 + 0.5) * span / n as f32;
        let (px, pz) = if along_x { (x + s, z) } else { (x, z + s) };
        let (w, d) = if along_x { (0.35, 0.18) } else { (0.18, 0.35) };
        parts.push(block(Slot::Panel, px, pz, y0, w, y1 - y0, d));
    }
}

/// Horizontal panel seams across a wall face: thin dark strips that break up big flat walls the
/// way the alpha's panelled siding does.
fn wall_seams(parts: &mut Vec<Part>, x: f32, z: f32, y0: f32, span: f32, n: u32, along_x: bool) {
    for k in 1..=n {
        let (w, d) = if along_x { (span, 0.16) } else { (0.16, span) };
        parts.push(bx(Slot::Dark, Vec3::new(x, y0 + k as f32, z), Vec3::new(w, 0.18, d)));
    }
}

/// Vertical corner trim on a `w × d` hall: the painted edge strips that give the alpha's
/// buildings a bright outline against their darker walls.
#[allow(clippy::too_many_arguments)]
fn corner_trim(parts: &mut Vec<Part>, x: f32, z: f32, w: f32, d: f32, y0: f32, h: f32, slot: Slot) {
    for (sx, sz) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        parts.push(block(slot, x + sx * w / 2.0, z + sz * d / 2.0, y0, 0.5, h, 0.5));
    }
}

/// A stepped roof tier: the block plus a trim lip along its front edge. Returns the new top `y`,
/// so callers can stack tiers.
#[allow(clippy::too_many_arguments)]
fn tier(parts: &mut Vec<Part>, x: f32, z: f32, y0: f32, w: f32, d: f32, h: f32, slot: Slot) -> f32 {
    parts.push(block(slot, x, z, y0, w, h, d));
    parts.push(bx(Slot::Dark, Vec3::new(x, y0 + h * 0.2, z + d / 2.0 + 0.06), Vec3::new(w, 0.22, 0.12)));
    y0 + h
}

/// Antenna mast: pole, crossbar and a warning light on top. Sized to read at the Close zoom
/// (about 12 pixels a metre): a pole under 0.3 m was a pixel or two wide and vanished.
fn mast(parts: &mut Vec<Part>, x: f32, z: f32, y: f32) {
    parts.push(cyl(Slot::Dark, Vec3::new(x, y + 2.4, z), Vec3::new(0.45, 4.8, 0.45)));
    parts.push(bx(Slot::Dark, Vec3::new(x, y + 3.6, z), Vec3::new(3.0, 0.28, 0.28)));
    parts.push(part(Prim::Sphere, Slot::Team, Vec3::new(x, y + 5.0, z), Vec3::splat(0.75)));
}

/// A radar dish on a pedestal: a wide bowl with a dark face inside its rim, so it reads as a
/// dish from above rather than a lid, and the feed horn standing out of it.
fn dish(parts: &mut Vec<Part>, x: f32, z: f32, y: f32) {
    parts.push(cyl(Slot::Dark, Vec3::new(x, y + 0.7, z), Vec3::new(0.9, 1.4, 0.9)));
    parts.push(cyl(Slot::Panel, Vec3::new(x, y + 1.65, z), Vec3::new(3.6, 0.5, 3.6)));
    parts.push(cyl(Slot::Dark, Vec3::new(x, y + 1.9, z), Vec3::new(2.9, 0.06, 2.9)));
    parts.push(cyl(Slot::Dark, Vec3::new(x, y + 2.5, z), Vec3::new(0.45, 1.2, 0.45)));
}

/// An EU roof as the footage's EU base has every roof and tier: a darker deck of plates and
/// grilles (`Slot::Deck`) inset in a solid orange frame along the four edges, over the `w × d`
/// block centred at `(x, z)` whose top is at `y`. It stands in for the chevron `hazard_trim`
/// the other factions keep, whose roofs show their own plates (`surface.rs`) uncovered.
#[allow(clippy::too_many_arguments)]
fn roof(parts: &mut Vec<Part>, faction: Faction, x: f32, z: f32, w: f32, d: f32, y: f32) {
    if faction != Faction::Eu {
        return;
    }
    parts.push(block(Slot::Deck, x, z, y, w - 0.8, 0.12, d - 0.8));
    let t = 0.45;
    for s in [-1.0, 1.0] {
        parts.push(block(Slot::Hazard, x, z + s * (d - t) / 2.0, y - 0.15, w + 0.1, 0.4, t));
        parts.push(block(Slot::Hazard, x + s * (w - t) / 2.0, z, y - 0.15, t, 0.4, d - 2.0 * t));
    }
}

/// Roof clutter centred at `(x, z)`: louvered vents, AC units with fan discs, pipe clusters and
/// small tanks. `seed` varies the layout per building type; every item is two parts or fewer.
#[allow(clippy::too_many_arguments)]
fn greebles(parts: &mut Vec<Part>, x: f32, z: f32, w: f32, d: f32, y: f32, seed: u32, count: u32) {
    for k in 0..count {
        let (u, v) = (h(seed + k * 7) - 0.5, h(seed + k * 7 + 1) - 0.5);
        let (x, z) = (x + u * w * 0.75, z + v * d * 0.75);
        let s = 1.0 + h(seed + k * 7 + 2) * 1.7;
        match k % 4 {
            0 => {
                parts.push(block(Slot::Dark, x, z, y, s * 1.5, s * 0.8, s * 1.1));
                parts.push(bx(Slot::Panel, Vec3::new(x, y + s * 0.45, z), Vec3::new(s * 1.3, 0.16, s * 0.9)));
            }
            1 => {
                parts.push(block(Slot::Panel, x, z, y, s * 1.6, s * 0.7, s * 1.3));
                parts.push(cyl(Slot::Dark, Vec3::new(x, y + s * 0.72, z), Vec3::new(s, 0.12, s)));
            }
            2 => {
                parts.push(bx(Slot::Panel, Vec3::new(x, y + s * 0.5, z), Vec3::new(s * 2.2, s * 0.35, s * 0.35)));
                parts.push(block(Slot::Dark, x, z, y, s * 0.5, s * 0.5, s * 0.5));
            }
            _ => {
                parts.push(cyl(Slot::Panel, Vec3::new(x, y + s * 0.75, z), Vec3::new(s * 1.1, s * 1.3, s * 1.1)));
                parts.push(cyl(Slot::Dark, Vec3::new(x, y + s * 1.45, z), Vec3::new(s * 1.15, 0.14, s * 1.15)));
            }
        }
    }
}

/// A sandbag wall along a line: alternating bags, each nudged so the row is not a rail.
fn sandbags(parts: &mut Vec<Part>, x0: f32, z0: f32, x1: f32, z1: f32, y: f32, n: u32) {
    for k in 0..n {
        let t = (k as f32 + 0.5) / n as f32;
        let mut bag = block(Slot::Crate(2), x0 + (x1 - x0) * t, z0 + (z1 - z0) * t, y, 1.15, 0.55, 0.8);
        bag.yaw = if k % 2 == 0 { 0.14 } else { -0.14 };
        parts.push(bag);
    }
}

/// Leaning scrap sheets against a GLA wall: mismatched panels in mixed colours.
fn scrap(parts: &mut Vec<Part>, x: f32, z: f32, y: f32, w: f32, d: f32, seed: u32) {
    for k in 0..4 {
        let mut sheet = block(
            Slot::Crate((k % 5) as u8),
            x + (h(seed + k + 9) - 0.5) * w,
            z + (h(seed + k + 17) - 0.5) * d,
            y,
            2.3 + h(seed + k + 3) * 1.5,
            0.22,
            1.9,
        );
        sheet.yaw = k as f32 * 1.6 + (h(seed + k) - 0.5) * 0.8;
        parts.push(sheet);
    }
}

/// A field of stacked crates and barrels, the GLA look (and supply piles).
fn crate_yard(parts: &mut Vec<Part>, w: f32, d: f32, y: f32, seed: u32, rows: i32) {
    let cell = 2.4;
    let (nx, nz) = ((w / cell) as i32, (d / cell) as i32);
    for i in 0..nx {
        for j in 0..nz {
            let n = seed + (i * 31 + j * 17) as u32;
            let x = -w / 2.0 + (i as f32 + 0.5) * cell;
            let z = -d / 2.0 + (j as f32 + 0.5) * cell;
            let stack = 1 + (h(n) * rows as f32) as i32;
            for s in 0..stack {
                let c = (h(n + 3 + s as u32) * 5.0) as u8;
                if h(n + 9) < 0.25 {
                    parts.push(cyl(
                        Slot::Crate(c),
                        Vec3::new(x, y + 0.8 + s as f32 * 1.6, z),
                        Vec3::new(1.3, 1.5, 1.3),
                    ));
                } else {
                    let sz = 1.6 + h(n + 5) * 0.3;
                    let mut p = block(Slot::Crate(c), x, z, y + s as f32 * sz, sz, sz, sz);
                    p.yaw = (h(n + 7) - 0.5) * 0.5;
                    parts.push(p);
                }
            }
        }
    }
}

// ---- Buildings ---------------------------------------------------------------------------

/// A building of `role`, `r` its footprint radius; `trains_infantry`: a factory drawn as a
/// barracks or GLA camp rather than a war factory.
pub fn building(faction: Faction, role: BuildingRole, r: f32, trains_infantry: bool) -> Vec<Part> {
    let mut p = Vec::new();
    let w = r * 1.75;
    match (faction, role) {
        (_, BuildingRole::Civilian) => civilian(&mut p, r),
        (_, BuildingRole::Bunker) => battle_bunker(&mut p, r),
        (_, BuildingRole::Defense) => turret(&mut p, faction, r),
        (_, BuildingRole::Superweapon) => scud_storm(&mut p, w),
        (_, BuildingRole::OilDerrick { .. }) => derrick(&mut p, faction, r),
        (Faction::Gla, BuildingRole::SupplyCenter { .. }) => stash(&mut p, w),
        (Faction::Gla, BuildingRole::CommandCenter) => fortress(&mut p, w),
        (Faction::Gla, BuildingRole::Factory) if trains_infantry => camp(&mut p, w),
        (Faction::Gla, BuildingRole::Factory) => workshop(&mut p, w),
        (Faction::Gla, BuildingRole::Lab) => salvage_bay(&mut p, w),
        (Faction::Gla, _) => {
            let top = tier(&mut p, 0.0, 0.0, 0.0, w * 0.8, w * 0.8, 3.6, Slot::Hull);
            sandbags(&mut p, -w * 0.4, w * 0.42, w * 0.4, w * 0.42, 0.0, 6);
            scrap(&mut p, -w * 0.4, -w * 0.1, 0.0, w * 0.18, w * 0.3, 91);
            crate_yard(&mut p, w, w * 0.28, 0.0, 91, 2);
            roof_rail(&mut p, w * 0.8, w * 0.8, 3.6, 0.8, 0b1011);
            mast(&mut p, w * 0.2, -w * 0.2, top);
        }
        (_, BuildingRole::CommandCenter) => command_center(&mut p, faction, w),
        (_, BuildingRole::SupplyCenter { .. }) => depot(&mut p, faction, w),
        (_, BuildingRole::PowerPlant) => plant(&mut p, faction, w),
        (_, BuildingRole::Factory) if trains_infantry => barracks(&mut p, faction, w),
        (_, BuildingRole::Factory) => war_factory(&mut p, faction, w),
        (_, BuildingRole::Lab) => lab(&mut p, faction, w),
    }
    p
}

/// The Battle Bus dug in: its hull sunk on its axles with sheet armour over the windows, a
/// ring of sandbags and scrap around it, and firing slits for the squads inside.
fn battle_bunker(p: &mut Vec<Part>, r: f32) {
    let (w, d) = (r * 1.5, r * 0.7);
    p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, r * 1.8, 0.2, r * 1.8));
    let top = tier(p, 0.0, 0.0, 0.2, w, d, 2.0, Slot::Hull);
    p.push(block(Slot::Team, 0.0, 0.0, top, w * 0.9, 0.3, d * 0.9));
    for x in [-0.3, 0.0, 0.3] {
        p.push(bx(Slot::Dark, Vec3::new(x * w, 1.4, d / 2.0 + 0.05), Vec3::new(w * 0.18, 0.25, 0.1)));
    }
    scrap(p, 0.0, 0.0, top + 0.3, w * 0.6, d * 0.6, 57);
    let ring = r * 0.85;
    sandbags(p, -ring, -ring, ring, -ring, 0.2, 5);
    sandbags(p, -ring, ring, ring, ring, 0.2, 5);
    sandbags(p, -ring, -ring * 0.6, -ring, ring * 0.6, 0.2, 3);
    sandbags(p, ring, -ring * 0.6, ring, ring * 0.6, 0.2, 3);
}

/// A base defence on a concrete pad: EU a squat armoured box with a long twin autocannon, APA
/// a round bunker with a stubby flame nozzle and fuel tanks, GLA a sandbag ring around a
/// rocket rack on a scrap frame. Team colour on the gun housing.
fn turret(p: &mut Vec<Part>, faction: Faction, r: f32) {
    let w = r * 1.6;
    p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w, 0.3, w));
    match faction {
        Faction::Eu => {
            p.push(block(Slot::Hull, 0.0, 0.0, 0.3, w * 0.75, 1.4, w * 0.75));
            p.push(cyl(Slot::Dark, Vec3::new(0.0, 1.85, 0.0), Vec3::new(w * 0.55, 0.3, w * 0.55)));
            p.push(block(Slot::Panel, 0.0, 0.0, 2.0, w * 0.5, 0.8, w * 0.6));
            p.push(block(Slot::Team, 0.0, -w * 0.05, 2.8, w * 0.45, 0.12, w * 0.3));
            for x in [-0.15, 0.15] {
                p.push(block(Slot::Dark, x * w, w * 0.45, 2.25, 0.18, 0.18, w * 0.65));
            }
            hazard_trim(p, w * 0.75, w * 0.75, 1.68);
        }
        Faction::Apa => {
            p.push(cyl(Slot::Hull, Vec3::new(0.0, 1.0, 0.0), Vec3::new(w * 0.8, 1.4, w * 0.8)));
            p.push(cyl(Slot::Panel, Vec3::new(0.0, 1.95, 0.0), Vec3::new(w * 0.6, 0.5, w * 0.6)));
            p.push(block(Slot::Team, 0.0, 0.0, 2.2, w * 0.4, 0.12, w * 0.4));
            p.push(block(Slot::Dark, 0.0, w * 0.4, 1.75, 0.45, 0.45, w * 0.35));
            for x in [-0.28, 0.28] {
                p.push(cyl(Slot::Crate(4), Vec3::new(x * w, 0.9, -w * 0.32), Vec3::new(0.7, 1.2, 0.7)));
            }
        }
        Faction::Gla => {
            sandbags(p, -w * 0.42, w * 0.42, w * 0.42, w * 0.42, 0.3, 5);
            sandbags(p, -w * 0.42, -w * 0.42, -w * 0.42, w * 0.3, 0.3, 4);
            sandbags(p, w * 0.42, -w * 0.42, w * 0.42, w * 0.3, 0.3, 4);
            p.push(block(Slot::Wood, 0.0, 0.0, 0.3, w * 0.5, 1.0, w * 0.5));
            p.push(block(Slot::Crate(2), 0.0, 0.0, 1.3, w * 0.4, 0.5, w * 0.6));
            for x in [-0.12, 0.0, 0.12] {
                p.push(cyl(Slot::Dark, Vec3::new(x * w, 1.95, w * 0.12), Vec3::new(0.32, 0.8, 0.32)));
            }
            p.push(block(Slot::Team, 0.0, -w * 0.25, 1.8, w * 0.3, 0.12, 0.3));
            scrap(p, -w * 0.25, -w * 0.25, 0.3, w * 0.15, w * 0.15, 17);
        }
    }
}

/// Command center: a stepped mass with a recessed entry, a landing pad on the roof and the
/// busiest roof of any building.
fn command_center(p: &mut Vec<Part>, faction: Faction, w: f32) {
    let accent = if faction == Faction::Eu { Slot::Hazard } else { Slot::Team };
    p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w * 1.08, 0.6, w * 1.08));
    p.push(block(Slot::Hull, 0.0, 0.0, 0.6, w, 3.4, w * 0.82));
    corner_trim(p, 0.0, 0.0, w, w * 0.82, 0.6, 3.4, accent);
    wall_ribs(p, 0.0, w * 0.415, 0.6, 4.0, w * 0.88, 7, true);
    wall_ribs(p, -w * 0.505, 0.0, 0.6, 4.0, w * 0.62, 5, false);
    wall_seams(p, 0.0, w * 0.415, 1.1, w * 0.88, 2, true);
    p.push(block(Slot::Panel, 0.0, -w * 0.06, 4.0, w * 0.86, 2.8, w * 0.7));
    // Recessed entry bay on the front face.
    p.push(block(Slot::Dark, 0.0, w * 0.41, 0.6, w * 0.3, 2.8, 0.35));
    p.push(block(Slot::Glow, 0.0, w * 0.44, 1.9, w * 0.26, 0.7, 0.12));
    p.push(block(accent, 0.0, w * 0.41, 3.5, w * 0.34, 0.35, 0.3));
    // Command tier at the back-left, then a penthouse and mast on top of it.
    let tier1 = tier(p, -w * 0.2, -w * 0.12, 6.8, w * 0.52, w * 0.5, 2.6, Slot::Panel);
    roof(p, faction, -w * 0.2, -w * 0.12, w * 0.52, w * 0.5, tier1);
    let top = tier(p, -w * 0.22, -w * 0.14, tier1, w * 0.32, w * 0.3, 1.7, Slot::Hull);
    roof(p, faction, -w * 0.22, -w * 0.14, w * 0.32, w * 0.3, top);
    mast(p, -w * 0.22, -w * 0.14, top);
    dish(p, w * 0.32, w * 0.24, 6.8);
    // Landing pad on the open side of the roof: hazard deck markings, team flash at the centre.
    p.push(cyl(Slot::Dark, Vec3::new(w * 0.26, 6.95, 0.0), Vec3::new(w * 0.34, 0.3, w * 0.34)));
    p.push(cyl(Slot::Hazard, Vec3::new(w * 0.26, 7.15, 0.0), Vec3::new(w * 0.26, 0.12, w * 0.26)));
    p.push(cyl(Slot::Team, Vec3::new(w * 0.26, 7.24, 0.0), Vec3::new(w * 0.1, 0.1, w * 0.1)));
    hazard_trim_at(p, w * 0.26, 0.0, w * 0.42, w * 0.42, 7.2);
    roof(p, faction, 0.0, -w * 0.06, w * 0.86, w * 0.7, 6.8);
    if faction != Faction::Eu {
        hazard_trim(p, w, w * 0.82, 6.8);
    }
    roof_rail(p, w, w * 0.82, 6.8, 0.9, 0b0011);
    catwalk(p, -w * 0.36, w * 0.3, w * 0.34, 4.6, true);
    pipe_run(p, w * 0.5, -w * 0.18, w * 0.42, 5.0, false);
    greebles(p, -w * 0.22, w * 0.22, w * 0.26, w * 0.14, 6.8, 3, 4);
    if faction == Faction::Apa {
        let top = tier(p, w * 0.22, -w * 0.3, 6.8, w * 0.22, w * 0.18, 1.6, Slot::Panel);
        p.push(bx(
            Slot::Team,
            Vec3::new(w * 0.22, top - 0.45, -w * 0.3 + w * 0.09 + 0.08),
            Vec3::new(w * 0.22, 0.32, 0.14),
        ));
    }
}

/// Supply depot: a long hall with an unloading bay, a collector pad and yard crates.
fn depot(p: &mut Vec<Part>, faction: Faction, w: f32) {
    let accent = if faction == Faction::Eu { Slot::Hazard } else { Slot::Team };
    p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w * 1.08, 0.5, w * 1.08));
    p.push(block(Slot::Hull, 0.0, 0.0, 0.5, w * 0.78, 3.6, w * 0.86));
    corner_trim(p, 0.0, 0.0, w * 0.78, w * 0.86, 0.5, 3.6, accent);
    wall_ribs(p, 0.0, w * 0.435, 0.5, 4.1, w * 0.72, 6, true);
    wall_seams(p, 0.0, w * 0.435, 1.0, w * 0.72, 2, true);
    // Unloading bay: recessed door, lintel and apron.
    p.push(block(Slot::Dark, 0.0, w * 0.43, 0.5, w * 0.4, 2.8, 0.32));
    p.push(block(Slot::Glow, 0.0, w * 0.46, 1.4, w * 0.34, 0.6, 0.12));
    p.push(block(accent, 0.0, w * 0.43, 3.5, w * 0.46, 0.4, 0.3));
    p.push(block(Slot::Concrete, 0.0, w * 0.62, 0.5, w * 0.5, 0.18, w * 0.22));
    roof(p, faction, 0.0, 0.0, w * 0.78, w * 0.86, 4.1);
    let top = tier(p, -w * 0.08, -w * 0.14, 4.1, w * 0.58, w * 0.54, 2.2, Slot::Panel);
    roof(p, faction, -w * 0.08, -w * 0.14, w * 0.58, w * 0.54, top);
    mast(p, -w * 0.28, -w * 0.3, top);
    // Pad for the airborne collectors, off the hall's side.
    p.push(cyl(Slot::Concrete, Vec3::new(w * 0.56, 0.7, -w * 0.18), Vec3::new(w * 0.3, 0.4, w * 0.3)));
    p.push(cyl(Slot::Hazard, Vec3::new(w * 0.56, 0.95, -w * 0.18), Vec3::new(w * 0.22, 0.1, w * 0.22)));
    p.push(cyl(Slot::Team, Vec3::new(w * 0.56, 1.03, -w * 0.18), Vec3::new(w * 0.08, 0.1, w * 0.08)));
    catwalk(p, 0.0, w * 0.5, w * 0.52, 3.4, true);
    crate_yard(p, w * 0.26, w * 0.28, 0.5, 13, 2);
    if faction != Faction::Eu {
        hazard_trim(p, w * 0.78, w * 0.86, 4.1);
    }
    roof_rail(p, w * 0.78, w * 0.86, 4.1, 0.85, 0b1011);
    greebles(p, w * 0.3, w * 0.3, w * 0.16, w * 0.18, 4.1, 21, 4);
    pipe_run(p, w * 0.4, -w * 0.05, w * 0.5, 2.8, false);
}

/// Power plant: a turbine hall between two tall cooling stacks, pipework between them.
fn plant(p: &mut Vec<Part>, faction: Faction, w: f32) {
    let accent = if faction == Faction::Eu { Slot::Hazard } else { Slot::Team };
    p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w * 1.05, 0.5, w * 1.05));
    p.push(block(Slot::Hull, 0.0, 0.0, 0.5, w * 0.9, 3.2, w * 0.5));
    corner_trim(p, 0.0, 0.0, w * 0.9, w * 0.5, 0.5, 3.2, accent);
    wall_ribs(p, 0.0, w * 0.255, 0.5, 3.7, w * 0.84, 6, true);
    wall_seams(p, 0.0, w * 0.255, 1.0, w * 0.84, 2, true);
    roof(p, faction, 0.0, 0.0, w * 0.9, w * 0.5, 3.7);
    let top = tier(p, 0.0, 0.0, 3.7, w * 0.68, w * 0.38, 1.6, Slot::Panel);
    roof(p, faction, 0.0, 0.0, w * 0.68, w * 0.38, top);
    p.push(block(Slot::Glow, 0.0, w * 0.255, 1.6, w * 0.6, 0.6, 0.12));
    // Twin cooling stacks: shell, rim cap, accent band and a plinth each.
    for x in [-0.22, 0.22] {
        p.push(cyl(Slot::Panel, Vec3::new(w * x, 4.6, -w * 0.36), Vec3::new(w * 0.3, 8.2, w * 0.3)));
        p.push(cyl(Slot::Dark, Vec3::new(w * x, 8.85, -w * 0.36), Vec3::new(w * 0.34, 0.35, w * 0.34)));
        p.push(cyl(accent, Vec3::new(w * x, 6.6, -w * 0.36), Vec3::new(w * 0.31, 0.5, w * 0.31)));
        p.push(block(Slot::Dark, w * x, -w * 0.36, 0.5, w * 0.36, 0.7, w * 0.36));
    }
    pipe_run(p, 0.0, -w * 0.36, w * 0.44, 2.6, true);
    pipe_run(p, 0.0, w * 0.32, w * 0.7, 3.2, true);
    if faction != Faction::Eu {
        hazard_trim(p, w * 0.68, w * 0.38, 3.7);
    }
    roof_rail(p, w * 0.9, w * 0.5, 3.7, 0.85, 0b1011);
    greebles(p, 0.0, 0.0, w * 0.4, w * 0.22, top, 31, 3);
    p.push(block(accent, 0.0, w * 0.28, 3.1, w * 0.5, 0.4, 0.25));
}

/// Barracks: a low block with lit door bays, a stepped roof and a striped yard curb.
fn barracks(p: &mut Vec<Part>, faction: Faction, w: f32) {
    let accent = if faction == Faction::Eu { Slot::Hazard } else { Slot::Team };
    p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w * 1.05, 0.4, w * 1.05));
    p.push(block(Slot::Hull, 0.0, -w * 0.1, 0.4, w * 0.9, 3.2, w * 0.62));
    corner_trim(p, 0.0, -w * 0.1, w * 0.9, w * 0.62, 0.4, 3.2, accent);
    wall_ribs(p, 0.0, w * 0.215, 0.4, 3.6, w * 0.84, 6, true);
    wall_seams(p, 0.0, w * 0.215, 2.5, w * 0.84, 1, true);
    // Door bays along the front face.
    for x in [-0.26, 0.0, 0.26] {
        p.push(block(Slot::Dark, w * x, w * 0.215, 0.4, w * 0.16, 2.2, 0.28));
        p.push(block(Slot::Glow, w * x, w * 0.24, 2.0, w * 0.13, 0.4, 0.1));
    }
    roof(p, faction, 0.0, -w * 0.1, w * 0.9, w * 0.62, 3.6);
    let top = tier(p, 0.0, -w * 0.1, 3.6, w * 0.68, w * 0.42, 1.5, Slot::Panel);
    roof(p, faction, 0.0, -w * 0.1, w * 0.68, w * 0.42, top);
    p.push(block(accent, 0.0, w * 0.38, 0.4, w * 0.9, 0.7, 0.22));
    // Flag on the yard corner.
    p.push(block(Slot::Dark, w * 0.42, w * 0.36, 0.4, 0.26, 3.2, 0.26));
    p.push(block(Slot::Team, w * 0.42 + 0.8, w * 0.36, 2.9, 1.5, 0.85, 0.1));
    if faction != Faction::Eu {
        hazard_trim(p, w * 0.68, w * 0.42, 3.6);
    }
    roof_rail(p, w * 0.9, w * 0.62, 3.6, 0.85, 0b1011);
    greebles(p, 0.0, -w * 0.12, w * 0.4, w * 0.22, top, 71, 3);
    pipe_run(p, -w * 0.46, -w * 0.08, w * 0.45, 2.6, false);
}

/// War factory: a tall assembly hall with a wide vehicle door, a gantry over it and stepped roofs.
fn war_factory(p: &mut Vec<Part>, faction: Faction, w: f32) {
    let accent = if faction == Faction::Eu { Slot::Hazard } else { Slot::Team };
    p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w * 1.05, 0.5, w * 1.05));
    p.push(block(Slot::Hull, 0.0, 0.0, 0.5, w * 0.92, 5.6, w * 0.72));
    corner_trim(p, 0.0, 0.0, w * 0.92, w * 0.72, 0.5, 5.6, accent);
    wall_ribs(p, -w * 0.465, 0.0, 0.5, 6.1, w * 0.62, 5, false);
    wall_ribs(p, w * 0.465, 0.0, 0.5, 6.1, w * 0.62, 5, false);
    wall_seams(p, -w * 0.465, 0.0, 1.4, w * 0.62, 3, false);
    wall_seams(p, w * 0.465, 0.0, 1.4, w * 0.62, 3, false);
    // Vehicle door: dark opening, glow strip, columns and a hazard lintel.
    p.push(block(Slot::Dark, 0.0, w * 0.365, 0.5, w * 0.52, 4.0, 0.32));
    p.push(block(Slot::Glow, 0.0, w * 0.39, 1.6, w * 0.44, 0.8, 0.12));
    for x in [-0.34, 0.34] {
        p.push(block(Slot::Panel, w * x, w * 0.365, 0.5, 0.5, 5.6, 0.5));
    }
    p.push(block(accent, 0.0, w * 0.365, 4.7, w * 0.62, 0.45, 0.35));
    catwalk(p, 0.0, w * 0.44, w * 0.7, 5.3, true);
    roof(p, faction, 0.0, 0.0, w * 0.92, w * 0.72, 6.1);
    let tier1 = tier(p, 0.0, -w * 0.1, 6.1, w * 0.78, w * 0.52, 1.6, Slot::Panel);
    roof(p, faction, 0.0, -w * 0.1, w * 0.78, w * 0.52, tier1);
    let top = tier(p, -w * 0.16, -w * 0.16, tier1, w * 0.4, w * 0.32, 1.5, Slot::Hull);
    roof(p, faction, -w * 0.16, -w * 0.16, w * 0.4, w * 0.32, top);
    mast(p, -w * 0.16, -w * 0.16, top);
    dish(p, w * 0.24, -w * 0.12, tier1);
    if faction != Faction::Eu {
        hazard_trim(p, w * 0.78, w * 0.52, 6.1);
    }
    roof_rail(p, w * 0.92, w * 0.72, 6.1, 0.9, 0b1011);
    pipe_run(p, -w * 0.475, -w * 0.05, w * 0.5, 3.6, false);
    pipe_run(p, w * 0.475, -w * 0.15, w * 0.4, 2.6, false);
    greebles(p, 0.0, w * 0.27, w * 0.68, w * 0.14, 6.1, 81, 4);
    // Side annex and a team banner on the opposite wall.
    p.push(block(Slot::Panel, -w * 0.52, w * 0.1, 0.5, w * 0.16, 2.8, w * 0.4));
    p.push(block(accent, w * 0.48, 0.0, 3.5, 0.28, 2.2, w * 0.36));
}

/// Tech lab: a compact research block under a dome, with a lit window band, sensor mast and
/// dish, and a cooling unit at the back.
fn lab(p: &mut Vec<Part>, faction: Faction, w: f32) {
    let accent = if faction == Faction::Eu { Slot::Hazard } else { Slot::Team };
    p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w * 1.02, 0.5, w * 1.02));
    p.push(block(Slot::Hull, 0.0, 0.0, 0.5, w * 0.8, 3.4, w * 0.7));
    corner_trim(p, 0.0, 0.0, w * 0.8, w * 0.7, 0.5, 3.4, accent);
    wall_ribs(p, 0.0, w * 0.355, 0.5, 3.9, w * 0.74, 5, true);
    p.push(block(Slot::Glow, 0.0, w * 0.36, 2.4, w * 0.66, 0.5, 0.12));
    p.push(block(Slot::Dark, 0.0, w * 0.36, 0.5, w * 0.2, 1.8, 0.28));
    p.push(block(accent, 0.0, w * 0.36, 2.0, w * 0.28, 0.3, 0.3));
    p.push(cyl(Slot::Panel, Vec3::new(0.0, 4.3, -w * 0.05), Vec3::new(w * 0.5, 0.8, w * 0.5)));
    p.push(part(Prim::Sphere, Slot::Panel, Vec3::new(0.0, 4.7, -w * 0.05), Vec3::new(w * 0.46, w * 0.3, w * 0.46)));
    p.push(cyl(accent, Vec3::new(0.0, 4.75, -w * 0.05), Vec3::new(w * 0.52, 0.2, w * 0.52)));
    roof(p, faction, 0.0, 0.0, w * 0.8, w * 0.7, 3.9);
    mast(p, w * 0.3, w * 0.2, 3.9);
    dish(p, -w * 0.26, w * 0.16, 3.9);
    p.push(block(Slot::Dark, w * 0.2, -w * 0.42, 0.5, w * 0.3, 1.4, w * 0.14));
    p.push(block(Slot::Panel, -w * 0.2, -w * 0.42, 0.5, w * 0.26, 1.0, w * 0.14));
    if faction != Faction::Eu {
        hazard_trim(p, w * 0.8, w * 0.7, 3.9);
    }
    roof_rail(p, w * 0.8, w * 0.7, 3.9, 0.8, 0b1011);
    greebles(p, -w * 0.25, -w * 0.25, w * 0.16, w * 0.14, 3.9, 57, 3);
}

/// GLA supply stash: a wooden platform of mixed crates with a lookout post and sandbagged edges.
fn stash(p: &mut Vec<Part>, w: f32) {
    p.push(block(Slot::Wood, 0.0, 0.0, 0.0, w * 1.02, 0.7, w * 1.02));
    for k in 0..3 {
        // Plank seams across the platform.
        let z = -w * 0.3 + k as f32 * w * 0.3;
        p.push(bx(Slot::Dark, Vec3::new(0.0, 0.72, z), Vec3::new(w * 1.02, 0.08, 0.12)));
    }
    crate_yard(p, w * 0.88, w * 0.88, 0.7, 41, 2);
    // Lookout post with a flag at the back-right corner.
    let (tx, tz) = (w * 0.42, -w * 0.42);
    for (dx, dz) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        p.push(block(Slot::Wood, tx + dx * 0.9, tz + dz * 0.9, 0.7, 0.32, 5.4, 0.32));
    }
    p.push(block(Slot::Wood, tx, tz, 6.1, 3.0, 0.35, 3.0));
    p.push(block(Slot::Team, tx, tz, 6.45, 2.4, 0.12, 2.4));
    mast(p, tx, tz, 6.45);
    sandbags(p, -w * 0.45, w * 0.42, w * 0.45, w * 0.42, 0.7, 6);
    sandbags(p, w * 0.42, -w * 0.3, w * 0.42, w * 0.32, 0.7, 5);
    scrap(p, -w * 0.42, -w * 0.18, 0.7, w * 0.16, w * 0.4, 3);
}

/// GLA command center: a sandbag-and-sheet ring around a squat stepped bunker.
fn fortress(p: &mut Vec<Part>, w: f32) {
    p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w * 1.05, 0.5, w * 1.05));
    p.push(block(Slot::Hull, 0.0, 0.0, 0.5, w * 0.6, 4.2, w * 0.6));
    let mid = tier(p, 0.0, 0.0, 4.7, w * 0.38, w * 0.38, 1.9, Slot::Panel);
    let top = tier(p, 0.0, 0.0, mid, w * 0.22, w * 0.22, 1.6, Slot::Hull);
    mast(p, w * 0.1, w * 0.1, top);
    dish(p, -w * 0.26, -w * 0.26, 4.7);
    // Uneven scrap wall ring: alternating sheets and posts around the bunker.
    for k in 0..10 {
        let a = k as f32 / 10.0 * TAU;
        let (x, z) = (a.cos() * w * 0.46, a.sin() * w * 0.46);
        let slot = if k % 2 == 0 { Slot::Panel } else { Slot::Crate((k % 5) as u8) };
        let mut seg = block(slot, x, z, 0.5, w * 0.28, 1.8 + h(k * 5) * 1.8, 0.85);
        seg.yaw = -a + FRAC_PI_2;
        p.push(seg);
    }
    sandbags(p, -w * 0.38, w * 0.38, w * 0.38, w * 0.38, 0.5, 6);
    scrap(p, -w * 0.42, -w * 0.1, 0.5, w * 0.2, w * 0.3, 5);
    crate_yard(p, w * 0.86, w * 0.22, 0.5, 77, 2);
    roof_rail(p, w * 0.6, w * 0.6, 4.7, 0.8, 0b1010);
    greebles(p, w * 0.26, w * 0.26, w * 0.16, w * 0.16, 4.7, 11, 3);
    p.push(block(Slot::Team, 0.0, w * 0.3, 4.4, w * 0.36, 1.3, 0.3));
}

/// GLA war factory: a salvage hall with an open bay, scrap roofing and chimney pipes.
fn workshop(p: &mut Vec<Part>, w: f32) {
    p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w * 1.02, 0.4, w * 1.02));
    p.push(block(Slot::Hull, 0.0, 0.0, 0.4, w * 0.82, 4.2, w * 0.62));
    wall_ribs(p, 0.0, w * 0.315, 0.4, 4.6, w * 0.76, 7, true);
    wall_seams(p, -w * 0.415, 0.0, 1.2, w * 0.62, 2, false);
    // Open bay: dark interior, lintel and a raised scrap roof over it.
    p.push(block(Slot::Dark, 0.0, w * 0.315, 0.4, w * 0.5, 3.2, 0.3));
    p.push(block(Slot::Hazard, 0.0, w * 0.315, 3.6, w * 0.56, 0.4, 0.35));
    p.push(block(Slot::Panel, 0.0, 0.0, 4.6, w * 0.88, 0.5, w * 0.68));
    let top = tier(p, -w * 0.16, -w * 0.16, 5.1, w * 0.5, w * 0.4, 1.7, Slot::Panel);
    mast(p, -w * 0.3, -w * 0.3, top);
    // Salvage kept on the roofs, the way the alpha fills every flat surface.
    scrap(p, w * 0.26, w * 0.14, 5.1, w * 0.3, w * 0.3, 43);
    crate_yard(p, w * 0.28, w * 0.2, top, 29, 1);
    // Chimney pipes clear of the roof tier.
    for x in [0.18, 0.36] {
        p.push(cyl(Slot::Dark, Vec3::new(w * x, 6.2, -w * 0.32), Vec3::new(0.75, 4.0, 0.75)));
        p.push(cyl(Slot::Panel, Vec3::new(w * x, 8.35, -w * 0.32), Vec3::new(0.95, 0.3, 0.95)));
    }
    scrap(p, -w * 0.42, w * 0.05, 0.4, w * 0.16, w * 0.4, 61);
    sandbags(p, -w * 0.42, w * 0.42, w * 0.42, w * 0.42, 0.4, 7);
    crate_yard(p, w * 0.9, w * 0.2, 0.4, 61, 2);
    roof_rail(p, w * 0.82, w * 0.62, 4.6, 0.85, 0b1011);
    pipe_run(p, w * 0.42, -w * 0.1, w * 0.5, 3.0, false);
    greebles(p, -w * 0.32, w * 0.02, w * 0.26, w * 0.2, top, 51, 4);
    p.push(block(Slot::Team, w * 0.3, 0.0, 5.1, 0.3, 1.8, 0.2));
}

/// GLA barracks: a walled camp of tent ridges between sandbag lines.
fn camp(p: &mut Vec<Part>, w: f32) {
    p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w * 1.02, 0.4, w * 1.02));
    p.push(block(Slot::Hull, 0.0, -w * 0.1, 0.4, w * 0.8, 2.8, w * 0.52));
    let top = tier(p, 0.0, -w * 0.1, 3.2, w * 0.6, w * 0.38, 1.4, Slot::Panel);
    for (k, x) in [-0.28, 0.0, 0.28].into_iter().enumerate() {
        let mut ridge = block(Slot::Crate(k as u8 + 1), w * x, w * 0.32, 0.4, w * 0.2, 1.7, w * 0.26);
        ridge.yaw = (h(k as u32 * 7) - 0.5) * 0.3;
        p.push(ridge);
    }
    sandbags(p, -w * 0.46, w * 0.44, w * 0.46, w * 0.44, 0.4, 7);
    sandbags(p, -w * 0.44, -w * 0.3, -w * 0.44, w * 0.3, 0.4, 5);
    scrap(p, w * 0.36, -w * 0.2, 0.4, w * 0.16, w * 0.3, 71);
    crate_yard(p, w * 0.3, w * 0.22, 0.4, 91, 2);
    roof_rail(p, w * 0.8, w * 0.52, 3.2, 0.8, 0b1010);
    mast(p, -w * 0.24, -w * 0.22, top);
    scrap(p, w * 0.28, -w * 0.05, top, w * 0.22, w * 0.18, 53);
    greebles(p, 0.0, -w * 0.1, w * 0.24, w * 0.16, top, 27, 4);
}

/// GLA salvage bay: a scrap shed beside two heaps of salvage, a crane arm over them.
fn salvage_bay(p: &mut Vec<Part>, w: f32) {
    p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w * 1.02, 0.4, w * 1.02));
    p.push(block(Slot::Hull, -w * 0.15, -w * 0.1, 0.4, w * 0.6, 3.0, w * 0.6));
    wall_seams(p, -w * 0.15, w * 0.2, 0.8, w * 0.56, 3, true);
    p.push(block(Slot::Dark, -w * 0.15, w * 0.2, 0.4, w * 0.34, 2.2, 0.26));
    p.push(block(Slot::Team, -w * 0.15, w * 0.21, 2.7, w * 0.4, 0.35, 0.12));
    let top = tier(p, -w * 0.15, -w * 0.1, 3.4, w * 0.5, w * 0.5, 0.6, Slot::Panel);
    scrap(p, w * 0.3, w * 0.22, 0.4, w * 0.3, w * 0.34, 41);
    scrap(p, w * 0.32, -w * 0.25, 0.4, w * 0.26, w * 0.26, 43);
    p.push(block(Slot::Dark, w * 0.15, -w * 0.02, 0.4, 0.5, 5.0, 0.5));
    p.push(block(Slot::Hazard, w * 0.28, -w * 0.02, 5.0, w * 0.3, 0.35, 0.35));
    sandbags(p, -w * 0.45, w * 0.44, w * 0.05, w * 0.44, 0.4, 5);
    mast(p, -w * 0.32, -w * 0.3, top);
}

/// The GLA Scud Storm: a concrete pad with a sandbagged rim and an upright rack of eight
/// missiles on a scrap launcher frame, their warheads in the toxin glow.
fn scud_storm(p: &mut Vec<Part>, w: f32) {
    p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w * 1.02, 0.4, w * 1.02));
    sandbags(p, -w * 0.48, w * 0.48, w * 0.48, w * 0.48, 0.4, 7);
    sandbags(p, -w * 0.48, -w * 0.48, w * 0.48, -w * 0.48, 0.4, 7);
    p.push(block(Slot::Hull, 0.0, -w * 0.05, 0.4, w * 0.62, 1.2, w * 0.5));
    p.push(block(Slot::Team, 0.0, w * 0.21, 1.2, w * 0.5, 0.3, 0.12));
    for k in 0..8 {
        let x = (k % 4) as f32 * w * 0.13 - w * 0.2;
        let z = if k < 4 { -w * 0.1 } else { w * 0.06 };
        p.push(cyl(Slot::Panel, Vec3::new(x, 3.2, z), Vec3::new(0.45, 3.6, 0.45)));
        p.push(cyl(Slot::Glow, Vec3::new(x, 5.3, z), Vec3::new(0.38, 0.6, 0.38)));
    }
    scrap(p, w * 0.32, w * 0.3, 0.4, w * 0.22, w * 0.2, 57);
    p.push(block(Slot::Dark, -w * 0.32, w * 0.3, 0.4, 0.5, 3.4, 0.5));
}

/// A civilian building, in no faction's colours: concrete walls, a tin or flat roof, dark
/// windows. Shaped by size: a one-room shanty under a tin sheet, a two-storey house with a
/// stepped tile roof, or a flat-roofed block with a roof terrace and water tanks.
fn civilian(p: &mut Vec<Part>, r: f32) {
    let w = r * 1.5;
    let window = |p: &mut Vec<Part>, x: f32, z: f32, y: f32, along_x: bool| {
        let (sx, sz) = if along_x { (0.9, 0.08) } else { (0.08, 0.9) };
        p.push(bx(Slot::Dark, Vec3::new(x, y, z), Vec3::new(sx, 0.9, sz)));
    };
    if r < 2.5 {
        p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w, 2.4, w * 0.9));
        let mut roof = block(Slot::Crate(3), 0.0, 0.0, 2.4, w * 1.1, 0.18, w);
        roof.yaw = 0.06;
        p.push(roof);
        p.push(block(Slot::Wood, w * 0.15, w * 0.46, 0.0, 0.9, 1.8, 0.1));
        window(p, -w * 0.25, w * 0.46, 1.4, true);
        p.push(block(Slot::Crate(0), -w * 0.42, -w * 0.3, 0.0, 0.7, 0.6, 0.7));
    } else if r < 4.0 {
        p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w, 5.0, w * 0.85));
        p.push(block(Slot::Panel, 0.0, 0.0, 2.45, w * 1.02, 0.18, w * 0.87));
        // Stepped roof: two tile slabs either side of a dark ridge beam.
        for s in [-1.0, 1.0] {
            p.push(bx(Slot::Crate(4), Vec3::new(0.0, 5.3 - s * 0.2, s * w * 0.22), Vec3::new(w * 1.05, 0.2, w * 0.5)));
        }
        p.push(block(Slot::Dark, 0.0, 0.0, 5.5, w * 1.05, 0.25, 0.3));
        for y in [1.3, 3.8] {
            for x in [-0.28, 0.28] {
                window(p, x * w, w * 0.43, y, true);
                window(p, w * 0.5, x * w * 0.8, y, false);
            }
        }
        p.push(block(Slot::Wood, 0.0, w * 0.43, 0.0, 1.0, 2.0, 0.12));
        p.push(block(Slot::Dark, -w * 0.38, -w * 0.32, 5.6, 0.5, 1.6, 0.5));
    } else {
        p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w, 7.5, w * 0.9));
        p.push(block(Slot::Panel, 0.0, 0.0, 7.5, w * 1.02, 0.4, w * 0.92));
        for y in [1.3, 3.8, 6.2] {
            for x in [-0.33, 0.0, 0.33] {
                window(p, x * w, w * 0.455, y, true);
                window(p, w * 0.505, x * w * 0.85, y, false);
            }
        }
        // Roof terrace: low parapet, two water tanks, an awning over the stairwell.
        p.push(block(Slot::Concrete, 0.0, -w * 0.43, 7.9, w, 0.6, 0.15));
        p.push(block(Slot::Concrete, -w * 0.49, 0.0, 7.9, 0.15, 0.6, w * 0.9));
        for x in [0.25, 0.4] {
            p.push(cyl(Slot::Crate(3), Vec3::new(x * w, 8.6, -w * 0.25), Vec3::new(1.2, 1.4, 1.2)));
        }
        p.push(block(Slot::Concrete, -w * 0.25, w * 0.2, 7.9, 2.0, 2.0, 2.0));
        p.push(block(Slot::Crate(1), -w * 0.25, w * 0.2 + 1.3, 9.0, 2.4, 0.12, 1.2));
        p.push(block(Slot::Wood, w * 0.1, w * 0.45, 0.0, 1.4, 2.2, 0.12));
    }
}

fn derrick(p: &mut Vec<Part>, faction: Faction, r: f32) {
    let w = r * 1.6;
    p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w, 0.4, w));
    // Pumpjack: A-frame, walking beam, horse head, counterweight and the wellhead it pumps.
    let frame = if faction == Faction::Gla { Slot::Wood } else { Slot::Dark };
    p.push(block(frame, 0.0, 0.0, 0.4, 0.7, 4.4, 0.7));
    p.push(block(frame, -w * 0.16, 0.0, 0.4, 0.5, 3.2, 0.5));
    p.push(block(Slot::Hull, 0.0, 0.0, 4.8, w * 0.95, 0.55, 0.65));
    p.push(block(Slot::Hazard, w * 0.48, 0.0, 3.6, 0.65, 1.9, 0.85));
    p.push(cyl(Slot::Dark, Vec3::new(-w * 0.42, 1.3, 0.0), Vec3::new(1.25, 1.8, 1.25)));
    p.push(cyl(Slot::Dark, Vec3::new(w * 0.2, 0.7, 0.0), Vec3::new(0.6, 1.4, 0.6)));
    p.push(bx(Slot::Hazard, Vec3::new(w * 0.2, 1.5, 0.0), Vec3::new(0.9, 0.25, 0.9)));
    // Storage tanks with accent bands, tied to the wellhead by a pipe run.
    p.push(cyl(Slot::Hull, Vec3::new(-w * 0.22, 1.8, w * 0.3), Vec3::new(1.9, 2.9, 1.9)));
    p.push(cyl(Slot::Team, Vec3::new(-w * 0.22, 2.7, w * 0.3), Vec3::new(1.95, 0.3, 1.95)));
    p.push(cyl(Slot::Hull, Vec3::new(-w * 0.02, 1.5, w * 0.36), Vec3::new(1.5, 2.3, 1.5)));
    pipe_run(p, -w * 0.12, w * 0.32, w * 0.5, 2.6, true);
    roof_rail(p, w * 0.86, w * 0.86, 0.4, 0.7, 0b1001);
    sandbags(p, -w * 0.42, -w * 0.42, w * 0.42, -w * 0.42, 0.4, 6);
}

// ---- Units -------------------------------------------------------------------------------

pub fn unit(faction: Faction, def: &Def, r: f32) -> Vec<Part> {
    let mut p = Vec::new();
    let Class::Unit { constructor, collector, weapon, .. } = def.class else { return p };
    if weapon.is_some() {
        combat_unit(&mut p, faction, def, r);
        return p;
    }
    match (faction, constructor, collector.is_some()) {
        (Faction::Gla, _, _) => {
            // Worker: a small figure with a hard hat; the GLA has no construction vehicles.
            p.push(cyl(Slot::Panel, Vec3::new(0.0, 0.55, 0.0), Vec3::new(0.7, 1.1, 0.5)));
            p.push(part(Prim::Sphere, Slot::Hazard, Vec3::new(0.0, 1.35, 0.0), Vec3::splat(0.55)));
            p.push(block(Slot::Crate(2), 0.0, -0.35, 0.7, 0.6, 0.5, 0.4));
        }
        (_, true, _) => {
            // Construction vehicle: yellow-black tracked body, cab, crane arm and blade.
            let (l, wd) = (r * 2.2, r * 1.3);
            for side in [-1.0, 1.0] {
                track(&mut p, side * wd * 0.42, l, 0.0, Slot::Hazard);
            }
            p.push(block(Slot::Hazard, 0.0, -l * 0.08, 0.75, wd * 0.78, 1.05, l * 0.72));
            p.push(block(Slot::Glow, 0.0, l * 0.26, 0.75, wd * 0.72, 1.15, l * 0.3));
            p.push(block(Slot::Dark, 0.0, l * 0.42, 1.1, wd * 0.5, 0.4, 0.25));
            let mut arm = block(Slot::Hazard, 0.0, -l * 0.18, 1.8, 0.38, 0.38, l * 0.95);
            arm.yaw = 0.12;
            p.push(arm);
            p.push(block(Slot::Dark, 0.0, -l * 0.62, 1.1, wd * 0.95, 0.75, 0.28));
            p.push(block(Slot::Team, wd * 0.5, 0.0, 1.9, 0.12, 0.35, l * 0.5));
            p.push(block(Slot::Crate(2), -wd * 0.3, -l * 0.35, 1.85, 0.65, 0.5, 0.55));
        }
        (Faction::Eu, _, true) => {
            // Airborne collector (sourced: the EU harvester flies). Our design: a flat lifting
            // body with four ducted fans and a cargo cradle underneath.
            let l = r * 2.2;
            let y = 7.0;
            p.push(block(Slot::Hull, 0.0, 0.0, y, l * 0.55, 1.0, l));
            p.push(block(Slot::Panel, 0.0, l * 0.3, y + 1.0, l * 0.4, 0.6, l * 0.3));
            p.push(block(Slot::Glow, 0.0, l * 0.46, y + 1.2, l * 0.3, 0.3, 0.1));
            p.push(block(Slot::Team, 0.0, -l * 0.1, y + 1.0, l * 0.5, 0.15, l * 0.35));
            for (x, z) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let at = Vec3::new(x * l * 0.42, y + 0.4, z * l * 0.38);
                p.push(cyl(Slot::Dark, at, Vec3::new(1.9, 0.7, 1.9)));
                let mut fan = bx(Slot::Panel, at + Vec3::Y * 0.1, Vec3::new(1.5, 0.06, 0.25));
                fan.spin = true;
                p.push(fan);
            }
            p.push(block(Slot::Dark, 0.0, 0.0, y - 1.0, 0.25, 1.0, 0.25));
            p.push(block(Slot::Crate(0), 0.0, 0.0, y - 2.1, 1.5, 1.1, 1.8));
        }
        (_, _, true) => {
            // Ground supply truck: wheels under a flatbed, cab forward, covered load.
            let (l, wd) = (r * 2.3, r * 1.2);
            for (x, z) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                p.push(cyl(Slot::Dark, Vec3::new(x * wd * 0.47, 0.5, z * l * 0.33), Vec3::new(1.0, 0.55, 1.0)));
            }
            p.push(block(Slot::Dark, 0.0, 0.0, 0.35, wd * 0.85, 0.35, l * 0.95));
            p.push(block(Slot::Hull, 0.0, l * 0.3, 0.7, wd * 0.95, 1.35, l * 0.32));
            p.push(block(Slot::Glow, 0.0, l * 0.46, 1.25, wd * 0.7, 0.55, 0.1));
            p.push(block(Slot::Panel, 0.0, -l * 0.18, 0.7, wd * 0.95, 1.15, l * 0.6));
            p.push(block(Slot::Crate(3), 0.0, -l * 0.18, 1.85, wd * 0.78, 0.55, l * 0.5));
            p.push(block(Slot::Team, 0.0, l * 0.3, 2.1, wd * 0.85, 0.14, l * 0.22));
            p.push(block(Slot::Crate(1), wd * 0.35, -l * 0.42, 1.6, 0.6, 0.5, 0.5));
        }
        _ => {
            let (l, wd) = (r * 2.0, r * 1.3);
            p.push(block(Slot::Hull, 0.0, 0.0, 0.0, wd, 1.0, l));
            p.push(block(Slot::Team, 0.0, 0.0, 1.0, wd * 0.8, 0.2, l * 0.4));
        }
    }
    p
}

/// One side of tracked running gear: the track band, three road-wheel discs sitting proud on it
/// and an armour skirt hanging on the outer face, leaving the wheel tops visible.
fn track(parts: &mut Vec<Part>, x: f32, l: f32, y0: f32, skirt: Slot) {
    parts.push(block(Slot::Dark, x, 0.0, y0, 0.66, 0.8, l));
    for k in 0..3 {
        let z = (k as f32 - 1.0) * l * 0.33;
        parts.push(cyl(Slot::Panel, Vec3::new(x, y0 + 0.82, z), Vec3::new(0.8, 0.18, 0.8)));
    }
    parts.push(block(skirt, x + x.signum() * 0.45, 0.0, y0 + 0.35, 0.18, 0.75, l * 0.95));
}

/// Helicopter: cockpit glazing, fuselage, tail boom with a fin and a tail rotor, stub wings
/// with weapon pods, and a spinning main rotor. Built around its own origin (the unit is
/// lifted to flight height), skids hanging below. EU: hazard-striped, twin pods; APA: a chin
/// gun; GLA: a scrap patch and a single pod.
fn helicopter(p: &mut Vec<Part>, faction: Faction, r: f32) {
    let l = r * 2.0;
    p.push(block(Slot::Hull, 0.0, 0.0, -0.6, r * 0.75, 1.3, l * 0.55));
    p.push(block(Slot::Glow, 0.0, l * 0.28, -0.4, r * 0.6, 0.9, l * 0.18));
    p.push(block(Slot::Panel, 0.0, -l * 0.36, -0.15, 0.45, 0.45, l * 0.5));
    p.push(block(Slot::Team, 0.0, -l * 0.6, 0.2, 0.12, 1.0, 0.7));
    p.push(bx(Slot::Dark, Vec3::new(0.3, 0.55, -l * 0.6), Vec3::new(0.06, 1.0, 0.18)));
    p.push(cyl(Slot::Dark, Vec3::new(0.0, 0.85, 0.0), Vec3::new(0.4, 0.3, 0.4)));
    let mut rotor = bx(Slot::Dark, Vec3::new(0.0, 1.05, 0.0), Vec3::new(l * 1.1, 0.06, 0.3));
    rotor.spin = true;
    p.push(rotor);
    for x in [-1.0, 1.0] {
        p.push(block(Slot::Dark, x * r * 0.6, 0.1, -1.55, 0.12, 0.12, l * 0.5));
    }
    match faction {
        Faction::Eu => {
            for x in [-1.0, 1.0] {
                p.push(block(Slot::Panel, x * r * 0.62, 0.0, -0.4, r * 0.45, 0.18, 0.5));
                p.push(cyl(Slot::Dark, Vec3::new(x * r * 0.85, -0.5, 0.1), Vec3::new(0.45, 0.6, 0.45)));
            }
            p.push(block(Slot::Hazard, 0.0, 0.0, 0.1, r * 0.76, 0.12, l * 0.2));
        }
        Faction::Apa => {
            p.push(block(Slot::Dark, 0.0, l * 0.36, -1.05, 0.3, 0.3, 0.7));
            for x in [-1.0, 1.0] {
                p.push(block(Slot::Panel, x * r * 0.62, 0.0, -0.45, r * 0.4, 0.15, 0.45));
            }
        }
        Faction::Gla => {
            p.push(block(Slot::Crate(2), r * 0.2, -l * 0.05, 0.05, r * 0.35, 0.12, l * 0.2));
            p.push(cyl(Slot::Dark, Vec3::new(r * 0.7, -0.5, 0.05), Vec3::new(0.4, 0.55, 0.4)));
        }
    }
}

/// A support power's fixed-wing jet: a long fuselage, swept wings (forward-swept for APA, as
/// the alpha's fighter), twin tail fins carrying the team colour and a glowing exhaust.
fn jet(p: &mut Vec<Part>, faction: Faction, r: f32) {
    let l = r * 3.0;
    p.push(block(Slot::Hull, 0.0, 0.0, -0.4, r * 0.45, 0.7, l));
    p.push(block(Slot::Glow, 0.0, l * 0.32, -0.05, r * 0.3, 0.35, l * 0.16));
    p.push(block(Slot::Glow, 0.0, -l * 0.52, -0.3, r * 0.3, 0.35, 0.3));
    let sweep = if faction == Faction::Apa { 0.12 } else { -0.12 };
    for x in [-1.0, 1.0] {
        p.push(block(Slot::Panel, x * r * 0.75, l * sweep, -0.35, r * 1.1, 0.12, l * 0.28));
        p.push(block(Slot::Team, x * r * 0.18, -l * 0.42, 0.1, 0.1, 0.8, l * 0.14));
    }
    p.push(block(Slot::Dark, 0.0, -l * 0.42, -0.25, r * 1.2, 0.1, l * 0.1));
}

/// Infantry squads (several small figures) and tanks (hull, tracks, turret, barrel). Our own
/// silhouettes: helmets and a roof band carry the team colour so sides read at game zoom; EU
/// tanks are long and flat, APA tanks twin-barrelled, GLA tanks small and boxy.
fn combat_unit(p: &mut Vec<Part>, faction: Faction, def: &Def, r: f32) {
    let weapon = def.weapon();
    if def.armour == g2_sim::Armour::Infantry {
        // Rocket infantry (strongest against Heavy) shoulders a fat launcher tube instead of
        // a rifle, so an anti-armour squad reads apart from a rifle squad.
        let rockets = weapon.is_some_and(|w| w.bonus[2] > w.bonus[0].max(w.bonus[1]));
        // Squad of three figures in a loose wedge.
        for (k, (x, z)) in [(0.0, 0.7), (-0.8, -0.4), (0.8, -0.4)].into_iter().enumerate() {
            let body = if faction == Faction::Gla { Slot::Crate(k as u8 + 1) } else { Slot::Hull };
            p.push(cyl(body, Vec3::new(x, 0.55, z), Vec3::new(0.55, 1.1, 0.45)));
            p.push(part(Prim::Sphere, Slot::Team, Vec3::new(x, 1.3, z), Vec3::splat(0.5)));
            p.push(block(Slot::Panel, x, z, 0.85, 0.6, 0.2, 0.5));
            if rockets {
                p.push(block(Slot::Dark, x + 0.3, z + 0.1, 1.35, 0.28, 0.28, 1.3));
            } else {
                p.push(block(Slot::Dark, x + 0.25, z + 0.3, 0.75, 0.12, 0.12, 0.8));
            }
        }
        return;
    }
    if def.is_support_jet() {
        jet(p, faction, r);
        return;
    }
    if def.flies() {
        helicopter(p, faction, r);
        return;
    }
    if weapon.is_some_and(|w| w.shell.is_some()) || def.deploy.is_some() {
        artillery(p, faction, r);
        return;
    }
    // Fast wheeled vehicles: anything quicker than a tank's 12 per second.
    if def.speed() > g2_sim::Fx::from_ratio(12, g2_sim::TICKS_PER_SECOND as i64) {
        light_vehicle(p, faction, r);
        return;
    }
    // Tracked main battle tanks.
    let (l, wd) = (r * 2.0, r * 1.3);
    let skirt = if faction == Faction::Eu { Slot::Panel } else { Slot::Hull };
    for side in [-1.0, 1.0] {
        track(p, side * wd * 0.44, l, 0.0, skirt);
    }
    p.push(block(Slot::Hull, 0.0, 0.0, 0.55, wd * 0.72, 0.85, l * 0.92));
    p.push(block(Slot::Hull, 0.0, -l * 0.34, 1.4, wd * 0.66, 0.5, l * 0.22));
    p.push(block(Slot::Dark, 0.0, l * 0.4, 0.85, wd * 0.5, 0.5, l * 0.12));
    match faction {
        Faction::Eu => {
            // Long flat turret: ring, mantlet, single long gun, cupola and deck stowage.
            p.push(cyl(Slot::Dark, Vec3::new(0.0, 1.5, -l * 0.02), Vec3::new(wd * 0.62, 0.22, wd * 0.62)));
            p.push(block(Slot::Panel, 0.0, -l * 0.02, 1.65, wd * 0.58, 0.75, l * 0.46));
            p.push(block(Slot::Dark, 0.0, l * 0.24, 1.95, wd * 0.3, 0.5, 0.5));
            p.push(block(Slot::Dark, 0.0, l * 0.42, 1.95, 0.24, 0.24, l * 0.5));
            p.push(cyl(Slot::Dark, Vec3::new(0.0, 2.15, -l * 0.12), Vec3::new(0.5, 0.35, 0.5)));
            p.push(block(Slot::Team, 0.0, -l * 0.18, 2.05, wd * 0.5, 0.14, l * 0.14));
            p.push(block(Slot::Crate(3), wd * 0.22, -l * 0.3, 2.1, 0.7, 0.45, 0.6));
            p.push(block(Slot::Crate(1), -wd * 0.22, -l * 0.3, 2.1, 0.7, 0.45, 0.6));
            hazard_trim(p, wd * 0.72, l * 0.92, 1.42);
        }
        Faction::Apa => {
            // Twin-barrelled turret on a round mount.
            p.push(cyl(Slot::Panel, Vec3::new(0.0, 1.85, -l * 0.02), Vec3::new(wd * 0.62, 0.85, wd * 0.62)));
            for x in [-0.2, 0.2] {
                p.push(block(Slot::Dark, x, l * 0.16, 2.15, 0.42, 0.55, 0.5));
                p.push(block(Slot::Dark, x, l * 0.36, 2.15, 0.2, 0.2, l * 0.46));
            }
            p.push(cyl(Slot::Dark, Vec3::new(0.0, 2.45, -l * 0.14), Vec3::new(0.55, 0.4, 0.55)));
            p.push(block(Slot::Team, 0.0, -l * 0.14, 2.3, wd * 0.5, 0.15, l * 0.16));
            p.push(block(Slot::Crate(2), -wd * 0.24, -l * 0.32, 2.15, 0.75, 0.5, 0.65));
        }
        Faction::Gla => {
            // Small boxy turret with a scrap plate and salvaged stowage.
            p.push(cyl(Slot::Dark, Vec3::new(0.0, 1.5, -l * 0.06), Vec3::new(wd * 0.55, 0.2, wd * 0.55)));
            p.push(block(Slot::Panel, 0.0, -l * 0.08, 1.6, wd * 0.5, 0.85, l * 0.34));
            p.push(block(Slot::Dark, 0.0, l * 0.14, 1.95, wd * 0.28, 0.5, 0.45));
            p.push(block(Slot::Dark, 0.0, l * 0.32, 1.95, 0.2, 0.2, l * 0.42));
            p.push(block(Slot::Crate(2), wd * 0.2, -l * 0.32, 1.95, 0.85, 0.6, 0.7));
            p.push(block(Slot::Crate(4), -wd * 0.22, -l * 0.3, 1.95, 0.7, 0.5, 0.6));
            p.push(block(Slot::Team, 0.0, -l * 0.16, 2.08, wd * 0.44, 0.14, l * 0.12));
        }
    }
}

/// Fast light vehicles on four wheels, gun on a pintle or a small turret: the EU APC is a
/// boxy carrier with a roof gun, the APA Gatling tank a low hull with a rotary barrel cluster,
/// the GLA technical a pickup with a machine gun in the bed.
fn light_vehicle(p: &mut Vec<Part>, faction: Faction, r: f32) {
    let (l, wd) = (r * 2.1, r * 1.2);
    for (x, z) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        p.push(cyl(Slot::Dark, Vec3::new(x * wd * 0.46, 0.5, z * l * 0.32), Vec3::new(1.05, 0.6, 1.05)));
    }
    // Fenders over the wheels leave the wheel tops showing.
    for side in [-1.0, 1.0] {
        p.push(block(Slot::Panel, side * wd * 0.46, 0.0, 0.72, wd * 0.18, 0.22, l * 0.92));
    }
    match faction {
        Faction::Eu => {
            p.push(block(Slot::Hull, 0.0, 0.0, 0.6, wd * 0.82, 1.3, l * 0.95));
            p.push(block(Slot::Glow, 0.0, l * 0.34, 1.35, wd * 0.62, 0.55, 0.1));
            p.push(cyl(Slot::Dark, Vec3::new(0.0, 2.05, -l * 0.05), Vec3::new(wd * 0.42, 0.35, wd * 0.42)));
            p.push(block(Slot::Panel, 0.0, -l * 0.05, 2.25, wd * 0.38, 0.5, l * 0.24));
            p.push(block(Slot::Dark, 0.0, l * 0.16, 2.35, 0.18, 0.18, l * 0.36));
            p.push(block(Slot::Team, 0.0, -l * 0.22, 1.95, wd * 0.7, 0.15, l * 0.3));
            p.push(block(Slot::Crate(3), -wd * 0.28, -l * 0.36, 1.95, 0.7, 0.5, 0.6));
            hazard_trim(p, wd * 0.82, l * 0.95, 1.9);
        }
        Faction::Apa => {
            p.push(block(Slot::Hull, 0.0, 0.0, 0.55, wd * 0.82, 0.9, l * 0.9));
            p.push(block(Slot::Team, 0.0, -l * 0.3, 1.45, wd * 0.6, 0.15, l * 0.3));
            p.push(cyl(Slot::Panel, Vec3::new(0.0, 1.75, -l * 0.02), Vec3::new(wd * 0.5, 0.6, wd * 0.5)));
            for (x, y) in [(-0.15, 1.75), (0.15, 1.75), (0.0, 2.0)] {
                p.push(block(Slot::Dark, x, l * 0.3, y, 0.13, 0.13, l * 0.42));
            }
            p.push(block(Slot::Dark, 0.0, l * 0.06, 1.95, wd * 0.34, 0.45, 0.4));
            p.push(block(Slot::Crate(2), wd * 0.3, -l * 0.36, 1.5, 0.7, 0.5, 0.6));
        }
        Faction::Gla => {
            p.push(block(Slot::Hull, 0.0, l * 0.22, 0.55, wd * 0.9, 1.2, l * 0.35));
            p.push(block(Slot::Team, 0.0, l * 0.22, 1.75, wd * 0.8, 0.15, l * 0.3));
            p.push(block(Slot::Crate(1), 0.0, -l * 0.18, 0.55, wd * 0.9, 0.6, l * 0.5));
            p.push(block(Slot::Dark, 0.0, -l * 0.2, 1.15, 0.2, 0.9, 0.2));
            p.push(block(Slot::Dark, 0.0, -l * 0.05, 1.95, 0.14, 0.14, l * 0.4));
            p.push(block(Slot::Crate(3), wd * 0.3, -l * 0.38, 1.35, 0.7, 0.55, 0.6));
            p.push(block(Slot::Crate(2), -wd * 0.3, -l * 0.38, 1.35, 0.7, 0.55, 0.6));
        }
    }
}

/// Self-propelled guns: a long chassis with a big barrel raised for indirect fire. EU: a flat
/// casemate with hazard trim; APA (Thunderfist): a squat turret on a heavy hull; GLA: an open
/// truck bed with a scrap-plated gun and three short tubes (the "tri-artillery cannon").
fn artillery(p: &mut Vec<Part>, faction: Faction, r: f32) {
    let (l, wd) = (r * 2.4, r * 1.35);
    for side in [-1.0, 1.0] {
        track(p, side * wd * 0.43, l, 0.0, Slot::Panel);
    }
    p.push(block(Slot::Hull, 0.0, 0.0, 0.55, wd * 0.74, 0.85, l * 0.95));
    p.push(block(Slot::Team, 0.0, l * 0.32, 1.4, wd * 0.74, 0.15, l * 0.22));
    // Spade dug in at the rear.
    p.push(block(Slot::Dark, 0.0, -l * 0.5, 0.6, wd * 0.9, 0.55, 0.3));
    let raised = |slot: Slot, x: f32, len: f32, thick: f32| {
        // Parts have no pitch, so the raised barrel is a stack of short segments, each a little
        // further forward and higher than the last.
        (0..3).map(move |k| {
            let t = k as f32 / 2.0;
            block(slot, x, -l * 0.05 + len * t, 2.1 + len * t * 0.55, thick, thick, len * 0.4)
        })
    };
    match faction {
        Faction::Eu => {
            p.push(block(Slot::Panel, 0.0, -l * 0.14, 1.4, wd * 0.64, 0.95, l * 0.52));
            p.push(block(Slot::Dark, 0.0, l * 0.12, 2.15, wd * 0.34, 0.55, 0.5));
            p.extend(raised(Slot::Dark, 0.0, l * 0.52, 0.34));
            p.push(block(Slot::Crate(3), wd * 0.24, -l * 0.36, 2.45, 0.7, 0.5, 0.6));
            p.push(block(Slot::Crate(1), -wd * 0.24, -l * 0.36, 2.45, 0.7, 0.5, 0.6));
            hazard_trim(p, wd * 0.74, l * 0.95, 1.42);
        }
        Faction::Apa => {
            p.push(cyl(Slot::Panel, Vec3::new(0.0, 1.95, -l * 0.1), Vec3::new(wd * 0.7, 0.9, wd * 0.7)));
            p.push(block(Slot::Dark, 0.0, l * 0.1, 2.55, wd * 0.38, 0.6, 0.5));
            p.extend(raised(Slot::Dark, 0.0, l * 0.55, 0.4));
            p.push(cyl(Slot::Dark, Vec3::new(0.0, 2.65, -l * 0.18), Vec3::new(0.55, 0.4, 0.55)));
            p.push(block(Slot::Crate(2), -wd * 0.26, -l * 0.34, 2.35, 0.8, 0.55, 0.7));
        }
        Faction::Gla => {
            p.push(block(Slot::Crate(1), 0.0, -l * 0.2, 1.4, wd * 0.7, 0.5, l * 0.5));
            for x in [-0.35, 0.0, 0.35] {
                p.extend(raised(Slot::Dark, x, l * 0.42, 0.2));
            }
            p.push(block(Slot::Crate(3), wd * 0.25, l * 0.3, 1.4, 0.8, 0.6, 0.7));
            p.push(block(Slot::Crate(4), -wd * 0.25, l * 0.3, 1.4, 0.8, 0.6, 0.7));
        }
    }
}

/// The Thunderfist's stabiliser legs, fully out; the view scales them across while it sets up
/// or packs up and hides them while packed.
pub fn outriggers(r: f32) -> Vec<Part> {
    let (l, wd) = (r * 2.4, r * 1.35);
    let mut p = Vec::new();
    for (x, z) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        p.push(block(Slot::Dark, x * wd * 0.85, z * l * 0.32, 0.15, 0.5, 0.3, 0.5));
        p.push(block(Slot::Panel, x * wd * 0.63, z * l * 0.32, 0.45, wd * 0.45, 0.2, 0.25));
    }
    p
}

pub fn supply_pile(r: f32) -> Vec<Part> {
    let mut p = Vec::new();
    p.push(block(Slot::Wood, 0.0, 0.0, 0.0, r * 1.9, 0.3, r * 1.9));
    crate_yard(&mut p, r * 1.8, r * 1.8, 0.3, 5, 3);
    p
}

pub fn oil_well(r: f32) -> Vec<Part> {
    vec![
        cyl(Slot::Oil, Vec3::new(0.0, 0.08, 0.0), Vec3::new(r * 1.6, 0.16, r * 1.4)),
        cyl(Slot::Dark, Vec3::new(0.0, 0.6, 0.0), Vec3::new(0.7, 1.2, 0.7)),
        cyl(Slot::Hazard, Vec3::new(0.0, 1.25, 0.0), Vec3::new(1.0, 0.2, 1.0)),
    ]
}

/// Construction site: grey steel scaffolding with team-coloured safety netting on two sides and a
/// single tower crane. `w` is the footprint. Steel parts use `Slot::Panel`; netting `Slot::Team`.
pub fn scaffold(w: f32, height: f32) -> Vec<Part> {
    let mut p = Vec::new();
    let t = 0.3;
    for (x, z) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        p.push(block(Slot::Panel, x * w / 2.0, z * w / 2.0, 0.0, t, height, t));
    }
    for k in 1..=2 {
        let y = height * k as f32 / 2.0;
        p.push(bx(Slot::Panel, Vec3::new(0.0, y, -w / 2.0), Vec3::new(w, t, t)));
        p.push(bx(Slot::Panel, Vec3::new(0.0, y, w / 2.0), Vec3::new(w, t, t)));
        p.push(bx(Slot::Panel, Vec3::new(-w / 2.0, y, 0.0), Vec3::new(t, t, w)));
        p.push(bx(Slot::Panel, Vec3::new(w / 2.0, y, 0.0), Vec3::new(t, t, w)));
    }
    // Netting hangs on the two far sides only, so the rising building stays visible.
    p.push(bx(Slot::Team, Vec3::new(0.0, height * 0.6, -w / 2.0 - 0.1), Vec3::new(w * 0.95, height * 0.7, 0.08)));
    p.push(bx(Slot::Team, Vec3::new(-w / 2.0 - 0.1, height * 0.6, 0.0), Vec3::new(0.08, height * 0.7, w * 0.95)));
    // Tower crane: lattice mast at one corner, counter-jib and hook block.
    let (cx, cz) = (w * 0.42, -w * 0.42);
    p.push(block(Slot::Dark, cx, cz, 0.0, 0.7, height + 4.0, 0.7));
    let mut jib = block(Slot::Team, cx, cz, height + 3.6, 0.5, 0.5, w * 0.8);
    jib.at.z += w * 0.3;
    p.push(jib);
    p.push(block(Slot::Dark, cx, cz - w * 0.15, height + 3.4, 1.0, 0.8, 1.0));
    p.push(block(Slot::Dark, cx, cz + w * 0.55, height - 1.0, 0.12, 4.2, 0.12));
    p
}

/// Team colours, in the order players claim them: the three factions' own (EU teal, APA red,
/// GLA sand) first, then colours that read apart from those on the ground and under fog.
pub const TEAM_PALETTE: [Color; 8] = [
    Color::srgb(0.10, 0.62, 0.78),
    Color::srgb(0.80, 0.14, 0.10),
    Color::srgb(0.80, 0.62, 0.20),
    Color::srgb(0.30, 0.72, 0.22),
    Color::srgb(0.58, 0.30, 0.78),
    Color::srgb(0.92, 0.45, 0.08),
    Color::srgb(0.88, 0.88, 0.86),
    Color::srgb(0.16, 0.28, 0.80),
];

/// Each player's index into `TEAM_PALETTE`: their faction's own colour unless an earlier
/// player already holds it, else the first colour nobody holds. Two players never share one.
pub fn team_slots(factions: &[Faction]) -> Vec<usize> {
    let mut taken = [false; TEAM_PALETTE.len()];
    factions
        .iter()
        .map(|&f| {
            let own = f as usize;
            let i = if taken[own] { taken.iter().position(|&t| !t).unwrap_or(own) } else { own };
            taken[i] = true;
            i
        })
        .collect()
}

/// Material colours per faction (sRGB); the hulls carry the faction's character. `Slot::Team`
/// here is the faction's own colour, for previews; in a match each player's comes from
/// `team_slots`.
pub fn color(faction: Option<Faction>, slot: Slot) -> Color {
    let f = faction.unwrap_or(Faction::Gla);
    match slot {
        // Tuned against the 2013 footage under our grade: EU a warm mid grey (near-white blew
        // out in the sun), APA dark steel, GLA rust-brown scrap; hazard trim leans orange.
        Slot::Hull => match f {
            Faction::Eu => Color::srgb(0.60, 0.60, 0.58),
            Faction::Apa => Color::srgb(0.32, 0.33, 0.34),
            Faction::Gla => Color::srgb(0.46, 0.37, 0.27),
        },
        Slot::Panel => match f {
            Faction::Eu => Color::srgb(0.44, 0.45, 0.45),
            Faction::Apa => Color::srgb(0.21, 0.22, 0.23),
            Faction::Gla => Color::srgb(0.36, 0.30, 0.23),
        },
        // The footage's EU roofs are darker than their walls: grating and deck plates measure
        // about 96-103 sRGB luminance in [dtl2Q_tUDDQ] at 270 s, against 124 on the lit wall tops.
        Slot::Deck => match f {
            Faction::Eu => Color::srgb(0.31, 0.31, 0.32),
            Faction::Apa => Color::srgb(0.17, 0.18, 0.19),
            Faction::Gla => Color::srgb(0.30, 0.25, 0.19),
        },
        Slot::Dark => Color::srgb(0.11, 0.11, 0.12),
        Slot::Team => TEAM_PALETTE[f as usize],
        Slot::Hazard => Color::srgb(0.90, 0.52, 0.08),
        Slot::Concrete => Color::srgb(0.42, 0.40, 0.36),
        Slot::Glow => Color::srgb(0.35, 0.75, 0.85),
        Slot::Crate(i) => [
            Color::srgb(0.50, 0.31, 0.25),
            Color::srgb(0.35, 0.40, 0.28),
            Color::srgb(0.58, 0.50, 0.36),
            Color::srgb(0.34, 0.38, 0.41),
            Color::srgb(0.56, 0.38, 0.26),
        ][i as usize % 5],
        Slot::Oil => Color::srgb(0.02, 0.02, 0.02),
        Slot::Wood => Color::srgb(0.38, 0.28, 0.17),
    }
}

/// The procedural vehicles are drawn about 2 × their radius long; the alpha's models are a
/// median 1.5 times that next to the same `EntityRadius` (measured over its 26 vehicles we
/// use), so ours are scaled up to match its proportions to the buildings.
const VEHICLE_SCALE: f32 = 1.5;

pub fn parts_for(def: &Def, faction: Option<Faction>) -> Vec<Part> {
    let r = def.radius.to_f32();
    match (def.class, faction) {
        (Class::SupplyPile, _) => supply_pile(r),
        (Class::OilWell, _) => oil_well(r),
        // Neutral or held, a civilian building keeps its look.
        (Class::Building { role: BuildingRole::Civilian, .. }, _) => {
            let mut p = Vec::new();
            civilian(&mut p, r);
            p
        }
        (Class::Building { role, .. }, Some(f)) => {
            let rules = g2_rules::rules();
            let infantry = def.produces.iter().any(|&u| rules.def(u).armour == g2_sim::Armour::Infantry);
            building(f, role, r, infantry)
        }
        // Vehicles at the alpha's proportions: its models are about 1.5 times as long as
        // their footprint (`EntityRadius`) is wide, so they reach past it front and back.
        (Class::Unit { .. }, Some(f)) if def.armour != g2_sim::Armour::Infantry && !def.is_support_jet() => {
            let mut p = unit(f, def, r);
            for part in &mut p {
                part.at *= VEHICLE_SCALE;
                part.size *= VEHICLE_SCALE;
            }
            p
        }
        (Class::Unit { .. }, Some(f)) => unit(f, def, r),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_def_has_a_model_that_stays_inside_its_footprint() {
        let rules = g2_rules::rules();
        for def in rules.defs.iter() {
            let parts = parts_for(def, def.faction);
            assert!(!parts.is_empty(), "{} has no model", def.name);
            let r = def.radius.to_f32();
            let reach_limit = if def.is_unit() && def.armour != g2_sim::Armour::Infantry { 2.0 } else { 1.3 };
            for p in &parts {
                let reach = Vec2::new(p.at.x, p.at.z).length() - p.size.x.max(p.size.z) / 2.0;
                assert!(reach <= r * reach_limit, "{} part at {:?} pokes out of its {r} footprint", def.name, p.at);
            }
        }
    }

    /// Every part is a mesh and an entity, so the counts are a performance budget, not a
    /// style choice.
    #[test]
    fn every_model_stays_inside_its_part_budget() {
        let rules = g2_rules::rules();
        for def in rules.defs.iter() {
            let parts = parts_for(def, def.faction);
            let cap = if def.is_building() { 120 } else { 30 };
            assert!(parts.len() <= cap, "{} needs {} parts, its budget is {cap}", def.name, parts.len());
        }
    }
}
