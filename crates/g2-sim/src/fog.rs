//! Fog of war: what each player can see and has seen.
//!
//! The map is divided into square cells of `CELL` world units. Every tick a player's *visible*
//! cells are recomputed from the sight radius of each of their units and finished buildings;
//! *explored* cells are the union of everything ever visible. Enemy units count only where
//! visible; enemy buildings are remembered as last seen on explored cells (`Memory`).
//!
//! Sight ranges are not in any archived Generals 2 source. They follow Zero Hour's data (EA's
//! GPL release, `vendor/superhackers/GeneralsGamePatch2`): armed units see as far as their
//! weapon reaches (Battlemaster 150/150, Red Guard 100/100), widened here by the acquisition
//! margin so a guard can see what it would engage; buildings see further than units.

use std::collections::BTreeMap;

use crate::defs::{DefId, Rules};
use crate::entity::{Entities, EntityId, PlayerId};
use crate::fixed::{Fx, Vec2};

/// Fog cell edge in world units.
pub const CELL: i64 = 8;
/// Unarmed units (constructors, collectors).
pub const UNIT_SIGHT: Fx = Fx::from_int(25);
/// Finished buildings; foundations see as far as an unarmed unit.
pub const BUILDING_SIGHT: Fx = Fx::from_int(30);

/// An enemy building as a player last saw it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Seen {
    pub def: DefId,
    pub owner: PlayerId,
    pub pos: Vec2,
}

/// One player's view of the map.
#[derive(Clone, Debug, Default)]
pub struct Vision {
    pub w: i32,
    pub h: i32,
    visible: Vec<bool>,
    explored: Vec<bool>,
    /// Enemy buildings last seen on explored cells, keyed by entity. A building seen destroyed
    /// (its cell visible, the building gone) is forgotten.
    pub memory: BTreeMap<EntityId, Seen>,
}

impl Vision {
    pub fn new(width: Fx, height: Fx) -> Vision {
        let w = (width.floor_int() / CELL + 1) as i32;
        let h = (height.floor_int() / CELL + 1) as i32;
        let n = (w * h) as usize;
        Vision { w, h, visible: vec![false; n], explored: vec![false; n], memory: BTreeMap::new() }
    }

    fn cell(&self, p: Vec2) -> Option<usize> {
        cell_index(self.w, self.h, p)
    }

    pub fn is_visible(&self, p: Vec2) -> bool {
        self.cell(p).is_some_and(|i| self.visible[i])
    }

    pub fn is_explored(&self, p: Vec2) -> bool {
        self.cell(p).is_some_and(|i| self.explored[i])
    }

    /// Cell `(x, y)` state for drawing: `(explored, visible)`.
    pub fn cell_state(&self, x: i32, y: i32) -> (bool, bool) {
        let i = (y * self.w + x) as usize;
        (self.explored[i], self.visible[i])
    }

    /// Centres of the cells never explored, in row order.
    pub fn unexplored_centres(&self) -> impl Iterator<Item = Vec2> + '_ {
        let w = self.w;
        self.explored.iter().enumerate().filter(|&(_, &e)| !e).map(move |(i, _)| centre(i as i32 % w, i as i32 / w))
    }

    /// Marks every cell whose centre lies within `r` of `c` as visible and explored. The test
    /// is exact on raw fixed-point values: squared sight distances stay far below `i64::MAX`.
    fn reveal(&mut self, c: Vec2, r: Fx) {
        let to_cell = |v: Fx| (v.floor_int().max(0) / CELL) as i32;
        let (x0, x1) = (to_cell(c.x - r), to_cell(c.x + r).min(self.w - 1));
        let (y0, y1) = (to_cell(c.y - r), to_cell(c.y + r).min(self.h - 1));
        let r2 = r.0 * r.0;
        for y in y0..=y1 {
            let dy = (centre(0, y).y - c.y).0;
            let rem = r2 - dy * dy;
            if rem < 0 {
                continue;
            }
            let row = (y * self.w) as usize;
            for x in x0..=x1 {
                let dx = (centre(x, 0).x - c.x).0;
                if dx * dx <= rem {
                    self.visible[row + x as usize] = true;
                    self.explored[row + x as usize] = true;
                }
            }
        }
    }

    /// Hash input: the explored cells (visibility follows from positions) and the memory.
    pub(crate) fn hash_into(&self, mut put: impl FnMut(u64)) {
        let explored = self.explored.iter().filter(|&&e| e).count();
        put(explored as u64);
        for (i, _) in self.explored.iter().enumerate().filter(|&(_, &e)| e) {
            put(i as u64);
        }
        put(self.memory.len() as u64);
        for (id, s) in &self.memory {
            put(((id.index as u64) << 32) | id.generation as u64);
            put(s.def.0 as u64);
            put(s.pos.x.0 as u64);
            put(s.pos.y.0 as u64);
        }
    }
}

/// How far `def` sees, for an entity that is (`complete`) or isn't finished yet. A deployable
/// unit sees as far as its longest weapon reaches, so the howitzer has targets to set up for.
pub fn sight(rules: &Rules, def: DefId, complete: bool, acquire: Fx) -> Fx {
    let d = rules.def(def);
    let weapon = d.deploy.map(|dep| dep.weapon).into_iter().chain(d.weapon()).map(|w| w.range).max();
    match weapon {
        Some(range) => range + acquire + d.radius,
        None if d.is_building() && complete => BUILDING_SIGHT + d.radius,
        None if d.is_unit() || d.is_building() => UNIT_SIGHT + d.radius,
        None => Fx::ZERO,
    }
}

/// Recomputes every player's visible cells and updates explored cells and building memory.
pub fn update(visions: &mut [Vision], entities: &Entities, rules: &Rules, acquire: Fx) {
    for v in visions.iter_mut() {
        v.visible.fill(false);
    }
    for e in entities.iter() {
        let Some(owner) = e.owner else { continue };
        if e.hp <= 0 {
            continue;
        }
        let r = sight(rules, e.def, e.is_complete(), acquire);
        if let Some(v) = visions.get_mut(owner.0 as usize) {
            v.reveal(e.pos, r);
        }
    }
    for (p, v) in visions.iter_mut().enumerate() {
        // Forget remembered buildings whose spot is in view again: if still there, they are
        // re-added below with their current state.
        let (w, h, visible) = (v.w, v.h, &v.visible);
        v.memory.retain(|_, s| !cell_index(w, h, s.pos).is_some_and(|i| visible[i]));
        for e in entities.iter() {
            let Some(owner) = e.owner else { continue };
            if owner.0 as usize == p || e.hp <= 0 || !rules.def(e.def).is_building() {
                continue;
            }
            if v.is_visible(e.pos) {
                v.memory.insert(e.id, Seen { def: e.def, owner, pos: e.pos });
            }
        }
    }
}

/// Index of the fog cell holding `p` on a `w`×`h` grid, if `p` is on the map.
fn cell_index(w: i32, h: i32, p: Vec2) -> Option<usize> {
    let (x, y) = ((p.x.floor_int() / CELL) as i32, (p.y.floor_int() / CELL) as i32);
    (p.x >= Fx::ZERO && p.y >= Fx::ZERO && x < w && y < h).then(|| (y * w + x) as usize)
}

/// Centre of fog cell `(x, y)`.
fn centre(x: i32, y: i32) -> Vec2 {
    let half = Fx::from_ratio(CELL, 2);
    Vec2::new(Fx::from_int(x as i64 * CELL) + half, Fx::from_int(y as i64 * CELL) + half)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_marks_a_disc_and_explored_persists() {
        let mut v = Vision::new(Fx::from_int(100), Fx::from_int(100));
        v.reveal(Vec2::from_int(50, 50), Fx::from_int(20));
        assert!(v.is_visible(Vec2::from_int(50, 50)));
        assert!(v.is_visible(Vec2::from_int(65, 50)));
        assert!(!v.is_visible(Vec2::from_int(75, 50)), "outside the radius");
        assert!(!v.is_visible(Vec2::from_int(64, 64)), "the corner of the box is outside the disc");
        v.visible.fill(false);
        assert!(!v.is_visible(Vec2::from_int(50, 50)));
        assert!(v.is_explored(Vec2::from_int(50, 50)), "explored stays");
    }
}
