//! Procedural models (docs/STYLE.md): chunky buildings with busy roofs, EU grey/white panels with
//! hazard-striped edges, APA dark steel with red accents, GLA scrap walls and crate yards. Units are
//! small; constructors are yellow-black; the EU collector is a ducted-fan lifter. Every shape is
//! original and built from primitives.
//!
//! A model is a list of parts. Each part is a primitive, a material slot and a local transform
//! relative to the entity's ground footprint (origin on the ground, Y up). The sim radius sets the
//! scale, so the footprint matches what the sim collides with.

use bevy::prelude::*;
use g2_sim::{BuildingRole, Class, Def, Faction};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Slot {
    /// Main hull/wall colour of the faction.
    Hull,
    /// Secondary panel colour (roofs, inner walls).
    Panel,
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

/// Yellow/black chevron trim along the four top edges of a `w × d` roof at height `y`.
fn hazard_trim(parts: &mut Vec<Part>, w: f32, d: f32, y: f32) {
    let t = 0.5;
    let n = ((w + d) / 1.6) as i32;
    for i in 0..n {
        let slot = if i % 2 == 0 { Slot::Hazard } else { Slot::Dark };
        let f = (i as f32 + 0.5) / n as f32;
        let (seg_w, seg_d) = (w / n as f32 * 2.0, d / n as f32 * 2.0);
        parts.push(bx(slot, Vec3::new(-w / 2.0 + f * w, y, -d / 2.0), Vec3::new(seg_w * 0.5, t, t)));
        parts.push(bx(slot, Vec3::new(-w / 2.0 + f * w, y, d / 2.0), Vec3::new(seg_w * 0.5, t, t)));
        parts.push(bx(slot, Vec3::new(-w / 2.0, y, -d / 2.0 + f * d), Vec3::new(t, t, seg_d * 0.5)));
        parts.push(bx(slot, Vec3::new(w / 2.0, y, -d / 2.0 + f * d), Vec3::new(t, t, seg_d * 0.5)));
    }
}

/// Roof clutter: vents, AC boxes, pipes, an antenna. `seed` varies the layout per building type.
fn greebles(parts: &mut Vec<Part>, w: f32, d: f32, y: f32, seed: u32, count: u32) {
    for k in 0..count {
        let (u, v) = (h(seed + k * 7) - 0.5, h(seed + k * 7 + 1) - 0.5);
        let (x, z) = (u * w * 0.75, v * d * 0.75);
        let s = 0.8 + h(seed + k * 7 + 2) * 1.8;
        match k % 4 {
            0 => parts.push(block(Slot::Dark, x, z, y, s * 1.4, s * 0.6, s)),
            1 => parts.push(cyl(Slot::Panel, Vec3::new(x, y + s * 0.35, z), Vec3::new(s, s * 0.7, s))),
            2 => parts.push(block(Slot::Panel, x, z, y, s * 0.9, s * 0.9, s * 0.9)),
            _ => parts.push(block(Slot::Dark, x, z, y, s * 2.4, 0.35, 0.35)),
        }
    }
    let (ax, az) = ((h(seed + 99) - 0.5) * w * 0.6, (h(seed + 98) - 0.5) * d * 0.6);
    parts.push(cyl(Slot::Dark, Vec3::new(ax, y + 2.5, az), Vec3::new(0.18, 5.0, 0.18)));
    parts.push(cyl(Slot::Team, Vec3::new(ax, y + 5.1, az), Vec3::new(0.45, 0.3, 0.45)));
}

/// A field of stacked crates and barrels, the GLA look (and supply piles).
fn crate_yard(parts: &mut Vec<Part>, w: f32, d: f32, y: f32, seed: u32, rows: i32) {
    let cell = 2.1;
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

pub fn building(faction: Faction, role: BuildingRole, r: f32) -> Vec<Part> {
    let mut p = Vec::new();
    let w = r * 1.75;
    match (faction, role) {
        (_, BuildingRole::OilDerrick { .. }) => derrick(&mut p, faction, r),
        (Faction::Gla, BuildingRole::SupplyCenter { .. }) => {
            // The GLA stash: a crate yard on a wooden platform with a lookout post.
            p.push(block(Slot::Wood, 0.0, 0.0, 0.0, w * 1.05, 0.7, w * 1.05));
            crate_yard(&mut p, w * 0.95, w * 0.95, 0.7, 41, 3);
            p.push(cyl(Slot::Dark, Vec3::new(w * 0.42, 4.0, -w * 0.42), Vec3::new(0.4, 8.0, 0.4)));
            p.push(block(Slot::Team, w * 0.42, -w * 0.42, 7.8, 2.6, 0.4, 2.6));
        }
        (Faction::Gla, BuildingRole::CommandCenter) => {
            // Scrap fortress: uneven sandbag-and-sheet walls around a squat bunker.
            p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w * 1.05, 0.5, w * 1.05));
            p.push(block(Slot::Hull, 0.0, 0.0, 0.5, w * 0.62, 5.5, w * 0.62));
            p.push(block(Slot::Panel, 0.0, 0.0, 6.0, w * 0.5, 2.2, w * 0.5));
            for k in 0..12 {
                let a = k as f32 / 12.0 * std::f32::consts::TAU;
                let (x, z) = (a.cos() * w * 0.46, a.sin() * w * 0.46);
                let mut seg = block(Slot::Panel, x, z, 0.5, w * 0.26, 2.0 + h(k * 5) * 1.6, 0.9);
                seg.yaw = -a + std::f32::consts::FRAC_PI_2;
                p.push(seg);
            }
            crate_yard(&mut p, w * 0.9, w * 0.25, 0.5, 77, 2);
            greebles(&mut p, w * 0.5, w * 0.5, 8.2, 11, 6);
            p.push(block(Slot::Team, 0.0, w * 0.32, 5.0, w * 0.4, 1.4, 0.3));
        }
        (Faction::Gla, BuildingRole::Factory) => {
            // Salvage shed: a corrugated hall with an open front and scrap piled along a side.
            p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w * 1.02, 0.4, w * 1.02));
            p.push(block(Slot::Hull, 0.0, -w * 0.12, 0.4, w * 0.82, 4.4, w * 0.62));
            p.push(block(Slot::Dark, 0.0, w * 0.2, 0.4, w * 0.5, 3.4, 0.25));
            p.push(block(Slot::Panel, 0.0, -w * 0.12, 4.8, w * 0.86, 0.5, w * 0.66));
            crate_yard(&mut p, w * 0.95, w * 0.25, 0.4, 61, 2);
            p.push(block(Slot::Team, w * 0.3, -w * 0.12, 5.3, 0.3, 1.8, 0.2));
        }
        (Faction::Gla, _) => {
            p.push(block(Slot::Hull, 0.0, 0.0, 0.0, w * 0.8, 4.0, w * 0.8));
            crate_yard(&mut p, w, w * 0.3, 0.0, 91, 2);
        }
        (_, BuildingRole::CommandCenter) => {
            let accent = if faction == Faction::Eu { Slot::Hazard } else { Slot::Team };
            p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w * 1.08, 0.6, w * 1.08));
            p.push(block(Slot::Hull, 0.0, 0.0, 0.6, w, 6.5, w * 0.82));
            p.push(block(Slot::Panel, -w * 0.18, 0.0, 7.1, w * 0.58, 2.6, w * 0.6));
            p.push(block(Slot::Glow, -w * 0.18, w * 0.302, 8.0, w * 0.5, 0.9, 0.12));
            // Landing pad on the open side of the roof.
            p.push(cyl(Slot::Dark, Vec3::new(w * 0.27, 7.25, 0.0), Vec3::new(w * 0.36, 0.3, w * 0.36)));
            p.push(cyl(accent, Vec3::new(w * 0.27, 7.45, 0.0), Vec3::new(w * 0.26, 0.12, w * 0.26)));
            p.push(block(Slot::Team, -w * 0.5, 0.0, 3.0, 0.3, 2.5, w * 0.5));
            greebles(&mut p, w * 0.55, w * 0.6, 9.7, 3, 7);
            if faction == Faction::Eu {
                hazard_trim(&mut p, w, w * 0.82, 7.1);
            }
            if faction == Faction::Apa {
                p.push(block(Slot::Panel, -w * 0.18, 0.0, 9.7, w * 0.4, 1.4, w * 0.42));
            }
        }
        (_, BuildingRole::SupplyCenter { .. }) => {
            // Long depot with an unloading bay and a pad for the airborne harvesters.
            p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w * 1.08, 0.5, w * 1.08));
            p.push(block(Slot::Hull, -w * 0.12, 0.0, 0.5, w * 0.72, 4.2, w * 0.86));
            p.push(block(Slot::Dark, -w * 0.12, w * 0.44, 0.5, w * 0.4, 3.2, 0.3));
            p.push(cyl(Slot::Concrete, Vec3::new(w * 0.36, 0.7, -w * 0.18), Vec3::new(w * 0.34, 0.4, w * 0.34)));
            p.push(cyl(Slot::Team, Vec3::new(w * 0.36, 0.95, -w * 0.18), Vec3::new(w * 0.22, 0.08, w * 0.22)));
            crate_yard(&mut p, w * 0.28, w * 0.3, 0.5, 13, 2);
            greebles(&mut p, w * 0.6, w * 0.7, 4.7, 21, 6);
            if faction == Faction::Eu {
                hazard_trim(&mut p, w * 0.72, w * 0.86, 4.7);
            }
        }
        (_, BuildingRole::PowerPlant) => {
            // Twin cooling stacks over a turbine hall.
            p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w * 1.05, 0.5, w * 1.05));
            p.push(block(Slot::Hull, 0.0, w * 0.15, 0.5, w * 0.9, 3.5, w * 0.5));
            for x in [-0.22, 0.22] {
                p.push(cyl(Slot::Panel, Vec3::new(w * x, 4.0, -w * 0.22), Vec3::new(w * 0.36, 8.0, w * 0.36)));
                p.push(cyl(Slot::Dark, Vec3::new(w * x, 8.1, -w * 0.22), Vec3::new(w * 0.3, 0.3, w * 0.3)));
                p.push(cyl(Slot::Team, Vec3::new(w * x, 6.0, -w * 0.22), Vec3::new(w * 0.37, 0.5, w * 0.37)));
            }
            p.push(block(Slot::Glow, 0.0, w * 0.405, 1.5, w * 0.6, 0.6, 0.1));
            greebles(&mut p, w * 0.8, w * 0.4, 4.0, 31, 4);
        }
        (_, BuildingRole::Factory) if r < 8.0 => {
            // Barracks: a low block with a lit front, a striped curb along the yard side and a flag.
            let accent = if faction == Faction::Eu { Slot::Hazard } else { Slot::Team };
            p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w * 1.05, 0.4, w * 1.05));
            p.push(block(Slot::Hull, 0.0, -w * 0.18, 0.4, w * 0.9, 3.4, w * 0.55));
            p.push(block(Slot::Panel, 0.0, -w * 0.18, 3.8, w * 0.7, 0.9, w * 0.4));
            p.push(block(Slot::Glow, 0.0, w * 0.1, 1.6, w * 0.6, 0.5, 0.1));
            p.push(block(accent, 0.0, w * 0.48, 0.4, w * 0.9, 0.7, 0.2));
            p.push(cyl(Slot::Dark, Vec3::new(w * 0.38, 3.0, w * 0.38), Vec3::new(0.25, 6.0, 0.25)));
            p.push(block(Slot::Team, w * 0.38 + 0.8, w * 0.38, 5.2, 1.6, 0.9, 0.08));
            greebles(&mut p, w * 0.7, w * 0.4, 4.7, 71, 4);
        }
        (_, BuildingRole::Factory) => {
            // War factory: a tall assembly hall with a wide vehicle door and a hazard-striped roof beam.
            p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w * 1.05, 0.5, w * 1.05));
            p.push(block(Slot::Hull, 0.0, -w * 0.1, 0.5, w * 0.92, 6.0, w * 0.72));
            p.push(block(Slot::Dark, 0.0, w * 0.27, 0.5, w * 0.5, 4.2, 0.2));
            p.push(block(Slot::Panel, 0.0, -w * 0.1, 6.5, w * 0.8, 1.0, w * 0.6));
            p.push(block(Slot::Hazard, 0.0, -w * 0.1, 7.5, w * 0.95, 0.4, 0.5));
            p.push(block(Slot::Team, -w * 0.47, -w * 0.1, 3.5, 0.3, 2.0, w * 0.4));
            greebles(&mut p, w * 0.75, w * 0.55, 7.5, 81, 6);
            if faction == Faction::Eu {
                hazard_trim(&mut p, w * 0.92, w * 0.72, 6.5);
            }
        }
    }
    p
}

fn derrick(p: &mut Vec<Part>, faction: Faction, r: f32) {
    let w = r * 1.6;
    p.push(block(Slot::Concrete, 0.0, 0.0, 0.0, w, 0.4, w));
    // Pumpjack: frame, walking beam, horse head, counterweight; plus storage tanks.
    let frame = if faction == Faction::Gla { Slot::Wood } else { Slot::Dark };
    p.push(block(frame, 0.0, 0.0, 0.4, 0.6, 4.2, 0.6));
    let mut beam = block(Slot::Hull, 0.0, 0.0, 4.6, w * 0.95, 0.5, 0.6);
    beam.yaw = 0.0;
    p.push(beam);
    p.push(block(Slot::Hazard, w * 0.48, 0.0, 3.4, 0.6, 1.8, 0.8));
    p.push(block(Slot::Dark, -w * 0.4, 0.0, 2.0, 1.2, 1.4, 1.0));
    p.push(cyl(Slot::Hull, Vec3::new(-w * 0.25, 1.6, w * 0.3), Vec3::new(1.8, 2.8, 1.8)));
    p.push(cyl(Slot::Team, Vec3::new(-w * 0.25, 2.4, w * 0.3), Vec3::new(1.85, 0.3, 1.85)));
}

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
            // Construction vehicle: yellow/black body, crane arm, cab.
            let (l, wd) = (r * 2.2, r * 1.3);
            p.push(block(Slot::Dark, 0.0, 0.0, 0.0, wd, 0.6, l));
            p.push(block(Slot::Hazard, 0.0, -l * 0.1, 0.6, wd * 0.95, 1.0, l * 0.7));
            p.push(block(Slot::Glow, 0.0, l * 0.25, 0.6, wd * 0.7, 1.2, l * 0.3));
            let mut arm = block(Slot::Hazard, 0.0, -l * 0.15, 1.6, 0.35, 0.35, l * 0.9);
            arm.yaw = 0.15;
            p.push(arm);
            p.push(block(Slot::Team, wd * 0.5, 0.0, 0.7, 0.08, 0.4, l * 0.5));
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
            // Ground supply truck.
            let (l, wd) = (r * 2.3, r * 1.2);
            p.push(block(Slot::Dark, 0.0, 0.0, 0.0, wd, 0.6, l));
            p.push(block(Slot::Hull, 0.0, l * 0.32, 0.6, wd, 1.3, l * 0.3));
            p.push(block(Slot::Team, 0.0, l * 0.32, 1.9, wd * 0.9, 0.15, l * 0.25));
            p.push(block(Slot::Panel, 0.0, -l * 0.15, 0.6, wd, 1.1, l * 0.62));
            p.push(block(Slot::Crate(3), 0.0, -l * 0.15, 1.7, wd * 0.8, 0.6, l * 0.5));
        }
        _ => {
            let (l, wd) = (r * 2.0, r * 1.3);
            p.push(block(Slot::Hull, 0.0, 0.0, 0.0, wd, 1.0, l));
            p.push(block(Slot::Team, 0.0, 0.0, 1.0, wd * 0.8, 0.2, l * 0.4));
        }
    }
    p
}

/// Infantry squads (several small figures) and tanks (hull, tracks, turret, barrel). Our own
/// silhouettes: helmets and a roof band carry the team colour so sides read at game zoom; EU
/// tanks are long and flat, APA tanks twin-barrelled, GLA tanks small and boxy.
fn combat_unit(p: &mut Vec<Part>, faction: Faction, def: &Def, r: f32) {
    if def.armour == g2_sim::Armour::Infantry {
        // Squad of three figures in a loose wedge.
        for (k, (x, z)) in [(0.0, 0.7), (-0.8, -0.4), (0.8, -0.4)].into_iter().enumerate() {
            let body = if faction == Faction::Gla { Slot::Crate(k as u8 + 1) } else { Slot::Hull };
            p.push(cyl(body, Vec3::new(x, 0.55, z), Vec3::new(0.55, 1.1, 0.45)));
            p.push(part(Prim::Sphere, Slot::Team, Vec3::new(x, 1.3, z), Vec3::splat(0.5)));
            p.push(block(Slot::Panel, x, z, 0.85, 0.6, 0.2, 0.5));
            p.push(block(Slot::Dark, x + 0.25, z + 0.3, 0.75, 0.12, 0.12, 0.8));
        }
        return;
    }
    if def.weapon().is_some_and(|w| w.shell.is_some()) || def.deploy.is_some() {
        artillery(p, faction, r);
        return;
    }
    let (l, wd) = (r * 2.0, r * 1.3);
    for side in [-1.0, 1.0] {
        p.push(block(Slot::Dark, side * wd * 0.42, 0.0, 0.0, wd * 0.22, 0.8, l));
    }
    p.push(block(Slot::Hull, 0.0, 0.0, 0.5, wd * 0.7, 0.9, l * 0.9));
    p.push(block(Slot::Team, 0.0, -l * 0.3, 1.4, wd * 0.72, 0.15, l * 0.3));
    let (turret_y, barrel_y) = (1.4, 1.9);
    match faction {
        Faction::Eu => {
            p.push(block(Slot::Panel, 0.0, -l * 0.05, turret_y, wd * 0.55, 0.7, l * 0.42));
            p.push(block(Slot::Dark, 0.0, l * 0.4, barrel_y - 0.15, 0.22, 0.22, l * 0.55));
            hazard_trim(p, wd * 0.7, l * 0.9, 1.4);
        }
        Faction::Apa => {
            p.push(cyl(Slot::Panel, Vec3::new(0.0, turret_y + 0.4, 0.0), Vec3::new(wd * 0.6, 0.8, wd * 0.6)));
            for x in [-0.2, 0.2] {
                p.push(block(Slot::Dark, x, l * 0.38, barrel_y, 0.18, 0.18, l * 0.5));
            }
        }
        Faction::Gla => {
            p.push(block(Slot::Panel, 0.0, -l * 0.1, turret_y, wd * 0.5, 0.9, l * 0.35));
            p.push(block(Slot::Dark, 0.0, l * 0.3, barrel_y, 0.2, 0.2, l * 0.45));
            p.push(block(Slot::Crate(2), wd * 0.2, -l * 0.38, 1.4, 0.8, 0.6, 0.7));
        }
    }
}

/// Self-propelled guns: a long chassis with a big barrel raised for indirect fire. EU: a flat
/// casemate with hazard trim; APA (Thunderfist): a squat turret on a heavy hull; GLA: an open
/// truck bed with a scrap-plated gun and three short tubes (the "tri-artillery cannon").
fn artillery(p: &mut Vec<Part>, faction: Faction, r: f32) {
    let (l, wd) = (r * 2.4, r * 1.35);
    for side in [-1.0, 1.0] {
        p.push(block(Slot::Dark, side * wd * 0.42, 0.0, 0.0, wd * 0.22, 0.8, l));
    }
    p.push(block(Slot::Hull, 0.0, 0.0, 0.5, wd * 0.72, 0.8, l * 0.95));
    p.push(block(Slot::Team, 0.0, l * 0.32, 1.3, wd * 0.72, 0.15, l * 0.22));
    let raised = |slot: Slot, x: f32, len: f32, thick: f32| {
        // Parts have no pitch, so the raised barrel is a stack of short segments, each a little
        // further forward and higher than the last.
        (0..4).map(move |k| {
            let t = k as f32 / 3.0;
            block(slot, x, -l * 0.05 + len * t, 2.0 + len * t * 0.55, thick, thick, len * 0.36)
        })
    };
    match faction {
        Faction::Eu => {
            p.push(block(Slot::Panel, 0.0, -l * 0.12, 1.3, wd * 0.62, 0.9, l * 0.5));
            p.extend(raised(Slot::Dark, 0.0, l * 0.5, 0.32));
            hazard_trim(p, wd * 0.72, l * 0.95, 1.3);
        }
        Faction::Apa => {
            p.push(cyl(Slot::Panel, Vec3::new(0.0, 1.9, -l * 0.1), Vec3::new(wd * 0.7, 0.9, wd * 0.7)));
            p.extend(raised(Slot::Dark, 0.0, l * 0.55, 0.4));
        }
        Faction::Gla => {
            p.push(block(Slot::Crate(1), 0.0, -l * 0.2, 1.3, wd * 0.7, 0.5, l * 0.5));
            for x in [-0.35, 0.0, 0.35] {
                p.extend(raised(Slot::Dark, x, l * 0.42, 0.2));
            }
            p.push(block(Slot::Crate(3), wd * 0.25, l * 0.3, 1.3, 0.8, 0.6, 0.7));
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
        Slot::Hull => match f {
            Faction::Eu => Color::srgb(0.78, 0.79, 0.78),
            Faction::Apa => Color::srgb(0.36, 0.37, 0.38),
            Faction::Gla => Color::srgb(0.56, 0.49, 0.38),
        },
        Slot::Panel => match f {
            Faction::Eu => Color::srgb(0.58, 0.60, 0.61),
            Faction::Apa => Color::srgb(0.24, 0.25, 0.26),
            Faction::Gla => Color::srgb(0.43, 0.38, 0.31),
        },
        Slot::Dark => Color::srgb(0.13, 0.13, 0.14),
        Slot::Team => TEAM_PALETTE[f as usize],
        Slot::Hazard => Color::srgb(0.92, 0.70, 0.08),
        Slot::Concrete => Color::srgb(0.50, 0.48, 0.44),
        Slot::Glow => Color::srgb(0.35, 0.75, 0.85),
        Slot::Crate(i) => [
            Color::srgb(0.55, 0.30, 0.25),
            Color::srgb(0.32, 0.42, 0.30),
            Color::srgb(0.62, 0.55, 0.38),
            Color::srgb(0.30, 0.38, 0.48),
            Color::srgb(0.66, 0.36, 0.30),
        ][i as usize % 5],
        Slot::Oil => Color::srgb(0.02, 0.02, 0.02),
        Slot::Wood => Color::srgb(0.38, 0.28, 0.17),
    }
}

pub fn parts_for(def: &Def, faction: Option<Faction>) -> Vec<Part> {
    let r = def.radius.to_f32();
    match (def.class, faction) {
        (Class::SupplyPile, _) => supply_pile(r),
        (Class::OilWell, _) => oil_well(r),
        (Class::Building { role, .. }, Some(f)) => building(f, role, r),
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
            for p in &parts {
                let reach = Vec2::new(p.at.x, p.at.z).length() - p.size.x.max(p.size.z) / 2.0;
                assert!(reach <= r * 1.3, "{} part at {:?} pokes out of its {r} footprint", def.name, p.at);
            }
        }
    }
}
