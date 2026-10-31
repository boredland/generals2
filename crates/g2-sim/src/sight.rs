//! Terrain line of sight for direct fire, after Zero Hour.
//!
//! Zero Hour's terrain blocks one thing: the shot of a unit whose `KindOf` has
//! `ATTACK_NEEDS_LINE_OF_SIGHT` (its tanks, infantry and other direct-fire units; not artillery,
//! aircraft or base defences: `Pathfinder::isAttackViewBlockedByObstacle` skips `IMMOBILE`
//! shooters and targets significantly above the terrain). Such a unit in range of a target it
//! can't see keeps closing in instead of firing. Vision (`doShroudReveal` is a flat disc) and
//! weapon range (`ATTACK_RANGE_IS_2D`) ignore height, so they stay flat here too.
//!
//! The test is `BaseHeightMapRenderObjClass::isClearLineOfSight`: a Bresenham walk over height
//! cells from the shooter's cell toward the target's (the target's own cell not tested), the
//! sight line falling or rising evenly from the shooter's eye to the target's centre, blocked
//! where a cell's highest corner stands more than `LOS_FUDGE` above it. The eye is the top of the
//! shooter (`getFiringLineOfSightOrigin`: position plus geometry height) and the aim point the
//! target's centre (`isClearGoalFiringLineOfSightTerrain`: half its geometry height).

use crate::defs::Def;
use crate::fixed::{Fx, Vec2};
use crate::map::Heights;

/// Zero Hour's `LOS_FUDGE`, 0.5 SAGE units, in world units (6 SAGE units each).
const LOS_FUDGE: Fx = Fx::from_ratio(1, 12);
/// Geometry heights in world units, from GeneralsGamePatch2's INI: infantry stand 12 SAGE units
/// tall (`GeometryHeight = 12.0` on every rifleman and rocket soldier) and tanks and other
/// vehicles 10-15 (Crusader 10, Overlord 15), so every unit is taken as 12.
const UNIT_HEIGHT: Fx = Fx::from_int(2);
/// Structures range from 5 to 64 SAGE units tall; 24 is the median of Zero Hour's faction
/// buildings, so a building is aimed at 12 SAGE units up.
const STRUCTURE_HEIGHT: Fx = Fx::from_int(4);

/// How tall `d` stands, in world units.
fn height_of(d: &Def) -> Fx {
    if d.is_building() { STRUCTURE_HEIGHT } else { UNIT_HEIGHT }
}

impl Heights {
    /// Highest corner of height cell `(x, y)`, `None` off the grid.
    fn cell_top(&self, x: i64, y: i64) -> Option<Fx> {
        let (w, h) = (self.w as i64, self.h as i64);
        if x < 0 || y < 0 || x >= w - 1 || y >= h - 1 {
            return None;
        }
        let at = |x: i64, y: i64| self.z[(y * w + x) as usize];
        Some(at(x, y).max(at(x + 1, y)).max(at(x, y + 1)).max(at(x + 1, y + 1)))
    }

    /// Ground height at `p` (the cell's lower-left sample; 0 on flat maps).
    pub fn ground(&self, p: Vec2) -> Fx {
        if self.z.is_empty() {
            return Fx::ZERO;
        }
        let x = (p.x.0 / self.step.0).clamp(0, self.w as i64 - 1);
        let y = (p.y.0 / self.step.0).clamp(0, self.h as i64 - 1);
        self.z[(y * self.w as i64 + x) as usize]
    }

    /// Whether the straight line from `from` at height `z0` to `to` at height `z1` clears the
    /// ground, as Zero Hour's Bresenham walk tests it.
    pub fn clear(&self, from: Vec2, z0: Fx, to: Vec2, z1: Fx) -> bool {
        if self.z.is_empty() {
            return true;
        }
        let cell = |v: Fx| v.0.div_euclid(self.step.0);
        let (mut x, mut y) = (cell(from.x), cell(from.y));
        let (ex, ey) = (cell(to.x), cell(to.y));
        let (dx, dy) = ((ex - x).abs(), (ey - y).abs());
        let (sx, sy) = ((ex - x).signum(), (ey - y).signum());
        let (major, minor) = if dx >= dy { (dx, dy) } else { (dy, dx) };
        if major == 0 {
            return true;
        }
        let mut num = major / 2;
        for k in 0..major {
            let Some(top) = self.cell_top(x, y) else { break };
            // The sight line's height over this cell: exact in fixed point.
            let z = z0 + Fx((z1 - z0).0 * k / major);
            if top > z + LOS_FUDGE {
                return false;
            }
            num += minor;
            if num >= major {
                num -= major;
                if dx >= dy { y += sy } else { x += sx }
            }
            if dx >= dy { x += sx } else { y += sy }
        }
        true
    }
}

impl crate::world::World {
    /// Whether unit `shooter` (of def `sd`, at `from`) is blocked by terrain from firing its
    /// direct-fire weapon at `target` (of def `td`, at `to`). Only ground units firing
    /// straight are ever blocked; aircraft, units inside something, shells and targets in the
    /// air never are.
    pub(crate) fn terrain_blocks(&self, sd: &Def, from: Vec2, td: &Def, to: Vec2, shell: bool) -> bool {
        let h = &self.map.heights;
        if h.z.is_empty() || shell || !sd.is_unit() || sd.flies() || td.flies() {
            return false;
        }
        let eye = h.ground(from) + height_of(sd);
        let aim = h.ground(to) + height_of(td) / Fx::from_int(2);
        !h.clear(from, eye, to, aim)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 20×20 grid, one world unit per sample, flat at 0 but for a wall 5 high on column 10.
    fn ridge() -> Heights {
        let (w, h) = (20u32, 20u32);
        let z = (0..w * h).map(|i| if i % w == 10 { Fx::from_int(5) } else { Fx::ZERO }).collect();
        Heights { step: Fx::ONE, w, h, z }
    }

    #[test]
    fn a_ridge_blocks_a_low_line_and_not_a_high_one() {
        let r = ridge();
        let (a, b) = (Vec2::from_int(3, 5), Vec2::from_int(17, 5));
        assert!(!r.clear(a, Fx::from_int(2), b, Fx::ONE), "over the ridge, low");
        assert!(r.clear(a, Fx::from_int(8), b, Fx::from_int(6)), "over the ridge, high");
        assert!(r.clear(a, Fx::from_int(2), Vec2::from_int(8, 9), Fx::ONE), "short of it");
        assert!(Heights::default().clear(a, Fx::ZERO, b, Fx::ZERO), "flat maps see everything");
    }
}
