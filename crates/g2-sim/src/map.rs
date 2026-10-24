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
    /// Cells on a bridge deck, same layout: walkable, but nothing may be built there, or a
    /// building could close the only crossing. Empty means no bridges.
    pub decks: Vec<bool>,
}

impl MapDef {
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= Fx::ZERO && p.y >= Fx::ZERO && p.x <= self.width && p.y <= self.height
    }

    pub fn clamp(&self, p: Vec2) -> Vec2 {
        Vec2::new(p.x.max(Fx::ZERO).min(self.width), p.y.max(Fx::ZERO).min(self.height))
    }

    /// Whether ground units can stand at `p`: inside the map and not on impassable terrain.
    pub fn walkable(&self, p: Vec2) -> bool {
        if !self.contains(p) {
            return false;
        }
        if self.terrain.is_empty() {
            return true;
        }
        let (w, h) = crate::nav::Grid::dims(self.width, self.height);
        let cell = |v: Fx, n: usize| (v.floor_int().max(0) / crate::nav::CELL).min(n as i64 - 1) as usize;
        !self.terrain[cell(p.y, h) * w + cell(p.x, w)]
    }

    /// Whether any cell where buildings may not stand (impassable terrain, bridge decks) has its
    /// centre within `r` of `c`.
    pub fn terrain_within(&self, c: Vec2, r: Fx) -> bool {
        if self.terrain.is_empty() && self.decks.is_empty() {
            return false;
        }
        let (w, h) = crate::nav::Grid::dims(self.width, self.height);
        let cell = |v: Fx, n: usize| (v.floor_int().max(0) / crate::nav::CELL).min(n as i64 - 1) as i32;
        let (x0, x1) = (cell(c.x - r, w), cell(c.x + r, w));
        let (y0, y1) = (cell(c.y - r, h), cell(c.y + r, h));
        let no_build = |i: usize| self.terrain.get(i) == Some(&true) || self.decks.get(i) == Some(&true);
        (y0..=y1).any(|y| {
            (x0..=x1).any(|x| no_build(y as usize * w + x as usize) && crate::nav::Grid::centre(x, y).distance(c) < r)
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerSetup {
    pub faction: Faction,
    pub starting_credits: i64,
    /// Played by the built-in AI at this level; `None` for a human.
    pub ai: Option<AiLevel>,
    /// The chosen general of `faction`, or `None` to play the plain faction.
    pub general: Option<crate::defs::GeneralId>,
}

/// How strong the built-in AI plays. `Easy` is the original wave AI. `Normal` judges the battle
/// (army strength seen, threats at home, its own fights) before attacking, retreats from losing
/// fights, guards its constructors and expands when its base's supply runs low. `Onslaught`
/// runs no economy: its waves appear on a schedule (`crate::onslaught`) and attack.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub enum AiLevel {
    #[default]
    Easy,
    Normal,
    Onslaught,
}

impl AiLevel {
    /// The levels that play a skirmish; `Onslaught` is its own mode.
    pub const ALL: [AiLevel; 2] = [AiLevel::Easy, AiLevel::Normal];

    pub fn name(self) -> &'static str {
        match self {
            AiLevel::Easy => "Easy",
            AiLevel::Normal => "Normal",
            AiLevel::Onslaught => "Onslaught",
        }
    }
}

#[derive(Clone, Debug)]
pub struct MatchSetup {
    pub map: MapDef,
    pub seed: u64,
    pub players: Vec<PlayerSetup>,
}
