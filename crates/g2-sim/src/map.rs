use crate::defs::Faction;
use crate::fixed::{Fx, Vec2};

#[derive(Clone, Debug)]
pub struct MapDef {
    pub name: String,
    /// World size: `0..width` × `0..height`.
    pub width: Fx,
    pub height: Fx,
    pub spawns: Vec<Vec2>,
    pub supply_piles: Vec<(Vec2, i64)>,
    pub oil_wells: Vec<(Vec2, i64)>,
    /// Cells ground units can't enter (cliffs, water), laid out like `nav::Grid`. Empty means
    /// open ground everywhere.
    pub terrain: Vec<bool>,
}

impl MapDef {
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= Fx::ZERO && p.y >= Fx::ZERO && p.x <= self.width && p.y <= self.height
    }

    pub fn clamp(&self, p: Vec2) -> Vec2 {
        Vec2::new(p.x.max(Fx::ZERO).min(self.width), p.y.max(Fx::ZERO).min(self.height))
    }

    /// Whether any impassable terrain cell has its centre within `r` of `c`: buildings may
    /// not stand on cliffs or water.
    pub fn terrain_within(&self, c: Vec2, r: Fx) -> bool {
        if self.terrain.is_empty() {
            return false;
        }
        let (w, h) = crate::nav::Grid::dims(self.width, self.height);
        let cell = |v: Fx, n: usize| (v.floor_int().max(0) / crate::nav::CELL).min(n as i64 - 1) as i32;
        let (x0, x1) = (cell(c.x - r, w), cell(c.x + r, w));
        let (y0, y1) = (cell(c.y - r, h), cell(c.y + r, h));
        (y0..=y1).any(|y| {
            (x0..=x1)
                .any(|x| self.terrain[y as usize * w + x as usize] && crate::nav::Grid::centre(x, y).distance(c) < r)
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerSetup {
    pub faction: Faction,
    pub starting_credits: i64,
    pub ai: bool,
    /// The chosen general of `faction`, or `None` to play the plain faction.
    pub general: Option<crate::defs::GeneralId>,
}

#[derive(Clone, Debug)]
pub struct MatchSetup {
    pub map: MapDef,
    pub seed: u64,
    pub players: Vec<PlayerSetup>,
}
