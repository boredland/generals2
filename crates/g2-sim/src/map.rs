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
    /// Neutral buildings infantry can garrison: centre and footprint radius.
    pub civilians: Vec<(Vec2, Fx)>,
    /// The bridges whose decks `decks` marks, in map order (`World::bridges` follows it).
    pub bridges: Vec<BridgeDef>,
    /// Ground heights (`crate::sight`); empty on flat maps.
    pub heights: Heights,
}

/// Ground height samples on a square grid with `step` world units between samples, sample
/// `(0, 0)` at the map's origin, `w` across and `h` down, row-major. Heights are in world
/// units. Empty means flat ground.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Heights {
    pub step: Fx,
    pub w: u32,
    pub h: u32,
    pub z: Vec<Fx>,
}

/// One bridge: its deck runs straight from `a` to `b` and is `width` wide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeDef {
    pub a: Vec2,
    pub b: Vec2,
    pub width: Fx,
    /// Hit points; `None` for a bridge the map makes indestructible.
    pub hp: Option<i32>,
    /// Nav cells (`nav::Grid` layout) on this deck, where ground units die when it falls.
    pub cells: Vec<u32>,
    /// The subset of `cells` that is water or cliff below the deck: closed while it is down.
    pub gap: Vec<u32>,
}

impl BridgeDef {
    /// The two points weapons aim at, as Zero Hour's `getBridgeAttackPoints`: each end moved
    /// half the deck's width in along it.
    pub fn attack_points(&self) -> [Vec2; 2] {
        let d = self.b - self.a;
        let len = d.length();
        if len <= self.width {
            let mid = self.a + d.scale(Fx::from_ratio(1, 2));
            return [mid, mid];
        }
        let step = d.scale(self.width / Fx::from_int(2) / len);
        [self.a + step, self.b - step]
    }

    /// Distance from `p` to the line between the attack points.
    pub fn distance(&self, p: Vec2) -> Fx {
        let [s, e] = self.attack_points();
        let d = e - s;
        let len_sq = d.length_sq();
        if len_sq == Fx::ZERO {
            return p.distance(s);
        }
        let t = ((p.x - s.x) * d.x + (p.y - s.y) * d.y) / len_sq;
        let t = t.max(Fx::ZERO).min(Fx::ONE);
        p.distance(s + d.scale(t))
    }
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
    /// Perks slotted on that general for this match (from the player's profile); empty
    /// without a general.
    pub perks: Vec<crate::defs::PerkId>,
    /// The player's rank with that general, 1-5 (from their profile): sets the level of its
    /// power (`defs::POWER_LEVEL_RANKS`). 1 without a general.
    pub rank: u8,
    /// The player's side: players with the same non-zero team are allies (they never shoot or
    /// splash each other, share their vision and win together); 0, the default, plays alone,
    /// as Generals' LAN lobby's "-" team.
    pub team: u8,
}

/// How strong the built-in AI plays. `Easy` is the original wave AI. `Normal` and `Hard`
/// judge the battle (army strength seen, threats at home, their own fights) before attacking,
/// retreat from losing fights, guard their constructors and expand when their base's supply
/// runs low. `Onslaught` runs no economy: its waves appear on a schedule (`crate::onslaught`)
/// and attack.
///
/// In a skirmish the levels also differ in pace (`AiLevel::pace`), and Easy plays with the
/// handicaps Zero Hour gives its campaign's Easy (there only in single-player, here borrowed
/// for the skirmish): its weapons fire at 80% of their rate (`SOLO_AI_EASY RATE_OF_FIRE 80%`)
/// and a human's units and buildings take two thirds of the damage it deals (their health at
/// 150%, `HumanSoloPlayerHealthBonus_Easy`). In a mission, whose own difficulty sets the pace,
/// Normal plays as Hard and Easy without handicaps.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub enum AiLevel {
    #[default]
    Easy,
    Normal,
    Hard,
    Onslaught,
}

/// A skirmish AI's pace: when its first attack may leave, how fast its army may grow, and
/// whether it adds factories beyond its core barracks and war factory when credits pile up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pace {
    /// No wave leaves and no support power is used before this tick; the base is still
    /// defended.
    pub first_attack: u32,
    /// Its factories train no armed unit while it owns (or trains) one per this many ticks of
    /// play so far; 0 for no limit.
    pub unit_every: u32,
    pub extra_factories: bool,
}

impl Pace {
    /// How many armed units it may own (or have in training) at `tick`.
    pub fn army_cap(&self, tick: u32) -> usize {
        tick.checked_div(self.unit_every).map_or(usize::MAX, |n| n as usize)
    }
}

impl AiLevel {
    /// The levels that play a skirmish; `Onslaught` is its own mode.
    pub const ALL: [AiLevel; 3] = [AiLevel::Easy, AiLevel::Normal, AiLevel::Hard];

    pub fn name(self) -> &'static str {
        match self {
            AiLevel::Easy => "Easy",
            AiLevel::Normal => "Normal",
            AiLevel::Hard => "Hard",
            AiLevel::Onslaught => "Onslaught",
        }
    }

    /// Its skirmish pace. Hard plays at full speed. Normal's first attack waits four minutes
    /// and Easy's six, time for a new player to stand up a base and a first army, and their
    /// armies grow by one armed unit per 25 s and 45 s of play (losses are replaced), so that
    /// first wave is about ten units, not thirty. Measured against a player who builds nothing
    /// (see `crates/g2-maps/tests/ai_levels.rs`).
    pub fn pace(self) -> Pace {
        let second = crate::world::TICKS_PER_SECOND;
        match self {
            AiLevel::Easy => Pace { first_attack: 360 * second, unit_every: 45 * second, extra_factories: false },
            AiLevel::Normal => Pace { first_attack: 240 * second, unit_every: 25 * second, extra_factories: false },
            AiLevel::Hard | AiLevel::Onslaught => Pace { first_attack: 0, unit_every: 0, extra_factories: true },
        }
    }

    /// Whether it judges the battle (`Normal`, `Hard`) rather than sending waves (`Easy`).
    pub fn judges(self) -> bool {
        matches!(self, AiLevel::Normal | AiLevel::Hard)
    }
}

#[derive(Clone, Debug)]
pub struct MatchSetup {
    pub map: MapDef,
    pub seed: u64,
    pub players: Vec<PlayerSetup>,
}
