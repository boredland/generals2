//! Grid pathfinding around structures and resources.
//!
//! The map is divided into square cells of `CELL` world units. A cell is blocked when it lies
//! inside any non-unit entity grown by `CLEARANCE` (every unit has radius 2, so a path through
//! free cells keeps units out of buildings). The grid is derived state: it is rebuilt from the
//! entities whenever the set of statics changes, so it needs no place in the state hash.
//!
//! A* uses integer octile costs (10 straight, 14 diagonal) and breaks ties by cell index, so
//! every machine finds the same path. Diagonal steps may not cut a blocked corner. The raw
//! cell path is shortened with a grid line-of-sight check, leaving only the turning points.
//! The heuristic is consistent, so each cell is expanded at most once and a search is bounded
//! by the grid size; an unreachable goal yields a route to the reachable cell closest to it.

use std::collections::BinaryHeap;

use crate::fixed::{Fx, Vec2};

/// Cell edge in world units.
pub const CELL: i64 = 2;
/// Extra margin around statics, the radius of every unit.
pub const CLEARANCE: i64 = 2;

/// Blocked cells of the map. `w`×`h` cells, row-major.
#[derive(Clone, Debug, Default)]
pub struct Grid {
    pub w: i32,
    pub h: i32,
    blocked: Vec<bool>,
}

impl Grid {
    /// Builds the grid for a `width`×`height` map with circular obstacles `(centre, radius)`
    /// plus the map's impassable `terrain` cells (same layout, may be empty).
    pub fn new(width: Fx, height: Fx, obstacles: &[(Vec2, Fx)], terrain: &[bool]) -> Grid {
        let (w, h) = Self::dims(width, height);
        let (w, h) = (w as i32, h as i32);
        let mut blocked =
            if terrain.len() == (w * h) as usize { terrain.to_vec() } else { vec![false; (w * h) as usize] };
        let clearance = Fx::from_int(CLEARANCE);
        for &(c, r) in obstacles {
            let reach = r + clearance;
            let (x0, x1) = (Self::cell_of(c.x - reach).max(0), Self::cell_of(c.x + reach).min(w - 1));
            let (y0, y1) = (Self::cell_of(c.y - reach).max(0), Self::cell_of(c.y + reach).min(h - 1));
            for cy in y0..=y1 {
                for cx in x0..=x1 {
                    // Blocked when the cell centre is inside the grown circle.
                    if Self::centre(cx, cy).distance(c) < reach {
                        blocked[(cy * w + cx) as usize] = true;
                    }
                }
            }
        }
        Grid { w, h, blocked }
    }

    fn cell_of(v: Fx) -> i32 {
        (v.floor_int().max(0) / CELL) as i32
    }

    /// Cells across and down for a `width`×`height` map.
    pub fn dims(width: Fx, height: Fx) -> (usize, usize) {
        ((width.floor_int() / CELL + 1) as usize, (height.floor_int() / CELL + 1) as usize)
    }

    pub fn centre(cx: i32, cy: i32) -> Vec2 {
        let half = Fx::from_ratio(CELL, 2);
        Vec2::new(Fx::from_int(cx as i64 * CELL) + half, Fx::from_int(cy as i64 * CELL) + half)
    }

    pub fn cell(&self, p: Vec2) -> (i32, i32) {
        (Self::cell_of(p.x).min(self.w - 1), Self::cell_of(p.y).min(self.h - 1))
    }

    pub fn blocked(&self, cx: i32, cy: i32) -> bool {
        cx < 0 || cy < 0 || cx >= self.w || cy >= self.h || self.blocked[(cy * self.w + cx) as usize]
    }

    /// Whether a straight walk between two cells crosses only free cells (supercover line, so
    /// a diagonal that grazes a blocked corner counts as blocked).
    fn line_free(&self, a: (i32, i32), b: (i32, i32)) -> bool {
        let (mut x, mut y) = a;
        let (dx, dy) = ((b.0 - a.0).abs(), (b.1 - a.1).abs());
        let (sx, sy) = ((b.0 - a.0).signum(), (b.1 - a.1).signum());
        let mut err = dx - dy;
        loop {
            if self.blocked(x, y) {
                return false;
            }
            if (x, y) == b {
                return true;
            }
            let e2 = 2 * err;
            match (e2 > -dy, e2 < dx) {
                (true, true) => {
                    // Exact corner: both side cells must be free too.
                    if self.blocked(x + sx, y) || self.blocked(x, y + sy) {
                        return false;
                    }
                    err += dx - dy;
                    x += sx;
                    y += sy;
                }
                (true, false) => {
                    err -= dy;
                    x += sx;
                }
                (false, _) => {
                    err += dx;
                    y += sy;
                }
            }
        }
    }

    /// Path from `from` to the nearest free cell that satisfies `goal`, as world-space turning
    /// points (excluding the start, ending at `end` when it is in sight). `goal(cell_centre)`
    /// decides arrival; `end` is where the unit finally steers. When no goal cell is
    /// reachable, the route ends at the reachable cell closest to `end`. `None` only when the
    /// unit stands deep inside blocked cells.
    pub fn find(&self, from: Vec2, end: Vec2, goal: impl Fn(Vec2) -> bool) -> Option<Vec<Vec2>> {
        let start = self.nearest_free(self.cell(from))?;
        let idx = |(x, y): (i32, i32)| (y * self.w + x) as usize;
        let target = self.cell(end);
        let h = |(x, y): (i32, i32)| {
            let (dx, dy) = ((x - target.0).abs(), (y - target.1).abs());
            10 * dx.max(dy) + 4 * dx.min(dy)
        };
        let n = (self.w * self.h) as usize;
        let mut g = vec![i32::MAX; n];
        let mut closed = vec![false; n];
        let mut parent = vec![u32::MAX; n];
        let mut open = BinaryHeap::new();
        g[idx(start)] = 0;
        // Closest expanded cell to `end` (lowest h, then index), the fallback destination.
        let mut best = (h(start), idx(start));
        // Max-heap of (−f, −h, −index): lowest f, then lowest h, then lowest index first.
        open.push((-h(start), -h(start), -(idx(start) as i64)));
        while let Some((_, _, neg_i)) = open.pop() {
            let i = (-neg_i) as usize;
            if closed[i] {
                continue;
            }
            closed[i] = true;
            let c = ((i as i32) % self.w, (i as i32) / self.w);
            if goal(Self::centre(c.0, c.1)) {
                return Some(self.smooth(start, c, &parent, Some(end)));
            }
            best = best.min((h(c), i));
            for (dx, dy, cost) in
                [(1, 0, 10), (-1, 0, 10), (0, 1, 10), (0, -1, 10), (1, 1, 14), (1, -1, 14), (-1, 1, 14), (-1, -1, 14)]
            {
                let nb = (c.0 + dx, c.1 + dy);
                if self.blocked(nb.0, nb.1) {
                    continue;
                }
                if dx != 0 && dy != 0 && (self.blocked(c.0 + dx, c.1) || self.blocked(c.0, c.1 + dy)) {
                    continue;
                }
                let ng = g[i] + cost;
                let j = idx(nb);
                if ng < g[j] {
                    g[j] = ng;
                    parent[j] = i as u32;
                    open.push((-(ng + h(nb)), -h(nb), -(j as i64)));
                }
            }
        }
        let b = best.1 as i32;
        Some(self.smooth(start, (b % self.w, b / self.w), &parent, None))
    }

    /// Nearest free cell to `c` (ring search), for starts inside a grown obstacle.
    fn nearest_free(&self, c: (i32, i32)) -> Option<(i32, i32)> {
        if !self.blocked(c.0, c.1) {
            return Some(c);
        }
        for r in 1..16_i32 {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dy.abs()) == r && !self.blocked(c.0 + dx, c.1 + dy) {
                        return Some((c.0 + dx, c.1 + dy));
                    }
                }
            }
        }
        None
    }

    /// Walks the parent chain back to `start` and keeps only the cells needed to stay in
    /// line of sight, then appends `end` (or the goal cell's centre when there is none).
    fn smooth(&self, start: (i32, i32), goal: (i32, i32), parent: &[u32], end: Option<Vec2>) -> Vec<Vec2> {
        let mut cells = vec![goal];
        let mut i = (goal.1 * self.w + goal.0) as usize;
        while parent[i] != u32::MAX {
            i = parent[i] as usize;
            cells.push(((i as i32) % self.w, (i as i32) / self.w));
        }
        cells.reverse();
        debug_assert_eq!(cells[0], start);
        let mut out = Vec::new();
        let mut anchor = 0;
        let mut k = 1;
        while k < cells.len() {
            if !self.line_free(cells[anchor], cells[k]) {
                anchor = k - 1;
                out.push(Self::centre(cells[anchor].0, cells[anchor].1));
            }
            k += 1;
        }
        // Steer to `end` only if it's in sight of the last turning point; else stop at the goal cell.
        let last = cells[anchor];
        match end {
            Some(end) if self.line_free(last, self.cell(end)) => out.push(end),
            _ => out.push(Self::centre(goal.0, goal.1)),
        }
        out
    }
}

/// Smallest-first priority helper kept for clarity in tests.
#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: i64, y: i64) -> Vec2 {
        Vec2::from_int(x, y)
    }

    #[test]
    fn walks_around_a_wall_of_buildings_and_is_deterministic() {
        // Three overlapping buildings form a wall across x = 50 between y = 20 and y = 80.
        let wall = [(v(50, 30), Fx::from_int(10)), (v(50, 50), Fx::from_int(10)), (v(50, 70), Fx::from_int(10))];
        let grid = Grid::new(Fx::from_int(100), Fx::from_int(100), &wall, &[]);
        let (from, to) = (v(20, 50), v(80, 50));
        let path = grid.find(from, to, |c| c.distance(to) <= Fx::from_int(2)).expect("a way around");
        assert_eq!(*path.last().unwrap(), to);
        // Every leg stays clear of every building (radius + unit radius).
        let mut prev = from;
        for &p in &path {
            for k in 0..=20 {
                let q = prev + (p - prev).scale(Fx::from_ratio(k, 20));
                for &(c, r) in &wall {
                    assert!(q.distance(c) >= r + Fx::from_int(1), "leg {prev:?}->{p:?} enters {c:?} at {q:?}");
                }
            }
            prev = p;
        }
        assert!(path.len() >= 2, "goes around, not through: {path:?}");
        assert_eq!(grid.find(from, to, |c| c.distance(to) <= Fx::from_int(2)), Some(path));
    }

    #[test]
    fn open_ground_is_a_single_straight_leg() {
        let grid = Grid::new(Fx::from_int(100), Fx::from_int(100), &[], &[]);
        let to = v(90, 10);
        assert_eq!(grid.find(v(5, 95), to, |c| c.distance(to) <= Fx::from_int(2)), Some(vec![to]));
    }

    #[test]
    fn enclosed_goals_route_to_the_closest_reachable_cell() {
        // A ring of buildings around (50,50) with the goal in its centre.
        let ring: Vec<_> = (0..16)
            .map(|k| {
                let a = k as f64 * std::f64::consts::TAU / 16.0;
                (v(50 + (a.cos() * 20.0) as i64, 50 + (a.sin() * 20.0) as i64), Fx::from_int(6))
            })
            .collect();
        let grid = Grid::new(Fx::from_int(100), Fx::from_int(100), &ring, &[]);
        let to = v(50, 50);
        let path = grid.find(v(5, 5), to, |c| c.distance(to) <= Fx::from_int(2)).expect("a partial route");
        let stop = *path.last().unwrap();
        let c = grid.cell(stop);
        assert!(!grid.blocked(c.0, c.1), "stops on a free cell: {stop:?}");
        // Outside the ring (radius 20 + 6 + clearance), as close to it as the free cells allow.
        let d = stop.distance(to);
        assert!(d > Fx::from_int(26) && d < Fx::from_int(32), "stops just outside the ring, {d:?} from the goal");
    }

    #[test]
    fn impassable_terrain_is_walked_around() {
        // A river of terrain cells across the whole map at x = 48..52 except a ford at the top.
        let (w, h) = Grid::dims(Fx::from_int(100), Fx::from_int(100));
        let terrain: Vec<bool> = (0..w * h).map(|i| (24..=26).contains(&(i % w)) && i / w > 5).collect();
        let grid = Grid::new(Fx::from_int(100), Fx::from_int(100), &[], &terrain);
        let (from, to) = (v(20, 80), v(80, 80));
        let path = grid.find(from, to, |c| c.distance(to) <= Fx::from_int(2)).unwrap();
        assert_eq!(*path.last().unwrap(), to);
        assert!(path.iter().any(|p| p.y < Fx::from_int(14)), "crosses at the ford: {path:?}");
    }
}
