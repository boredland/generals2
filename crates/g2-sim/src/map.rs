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
}

impl MapDef {
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= Fx::ZERO && p.y >= Fx::ZERO && p.x <= self.width && p.y <= self.height
    }

    pub fn clamp(&self, p: Vec2) -> Vec2 {
        Vec2::new(p.x.max(Fx::ZERO).min(self.width), p.y.max(Fx::ZERO).min(self.height))
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
