use crate::fixed::Fx;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Faction {
    Eu,
    Apa,
    Gla,
}

impl Faction {
    pub const ALL: [Faction; 3] = [Faction::Eu, Faction::Apa, Faction::Gla];

    /// The GLA runs without a power grid (its command center supplies 0 power).
    pub fn uses_power(self) -> bool {
        !matches!(self, Faction::Gla)
    }

    pub fn name(self) -> &'static str {
        match self {
            Faction::Eu => "European Union",
            Faction::Apa => "Asia-Pacific Alliance",
            Faction::Gla => "Global Liberation Army",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct DefId(pub u16);

/// What a unit or building costs. Credits only: the 2013 game dropped oil as a second
/// currency (its derricks paid credits, Fandom `Oil_derrick_(Generals_2)`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Cost {
    pub credits: i64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CollectorSpec {
    pub capacity: i64,
    pub load_ticks: u32,
    pub unload_ticks: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    Unit {
        /// World units per tick.
        speed: Fx,
        constructor: bool,
        collector: Option<CollectorSpec>,
        weapon: Option<Weapon>,
        /// Aircraft fly straight over everything, push apart only from other aircraft, and
        /// can be hit only by weapons that target the air.
        flies: bool,
    },
    Building {
        /// Positive: power source with this capacity. Negative: consumer drawing this much.
        power: i32,
        /// Sources power consumers whose centre lies within this distance.
        power_radius: Fx,
        role: BuildingRole,
    },
    SupplyPile,
    OilWell,
}

/// Armour classes from the archived unit cards ("Infantry", "Light", "Heavy, Mechanical",
/// "Structure"). A weapon's damage against each is its base damage plus a per-class bonus,
/// the way the cards state it ("40 (Bonus: 30 vs. Light)").
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Armour {
    Infantry,
    Light,
    Heavy,
    Structure,
}

/// What a weapon can fire at: the alpha's `TargetType` (0 both, 1 ground, 2 air).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Targets {
    Ground,
    Air,
    Both,
}

impl Targets {
    pub fn hits(self, flying: bool) -> bool {
        match self {
            Targets::Ground => !flying,
            Targets::Air => flying,
            Targets::Both => true,
        }
    }
}

/// A weapon: every `cooldown_ticks` it fires at one target whose edge is within `range` of the
/// shooter's centre (and, for a shell, at least `min_range` away). Without a `shell` the hit is
/// instant and deals `damage_vs(armour)` to that target. With one it lobs a shell at the spot
/// the target stood on when it fired; the shell lands after its flight time and damages every
/// ground enemy within `radius` of that spot, so a unit that moved away is missed. `air_range`,
/// when set, is the range against aircraft (the alpha's rocket squads reach further up).
/// `garrison_bonus` is extra damage against a building with units inside (the Thunderfist
/// howitzer's card: "+100 vs. garrisoned buildings").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Weapon {
    pub damage: i32,
    /// Extra damage per armour class, indexed by `Armour as usize`; may be negative.
    pub bonus: [i32; 4],
    pub range: Fx,
    pub cooldown_ticks: u32,
    pub shell: Option<Shell>,
    pub targets: Targets,
    pub air_range: Option<Fx>,
    pub garrison_bonus: i32,
}

impl Weapon {
    /// At least 1, so every hit counts.
    pub fn damage_vs(&self, armour: Armour) -> i32 {
        (self.damage + self.bonus[armour as usize]).max(1)
    }

    /// Its reach against a target that flies or not; `None` if it can't fire at one.
    pub fn range_vs(&self, flying: bool) -> Option<Fx> {
        self.targets.hits(flying).then(|| if flying { self.air_range.unwrap_or(self.range) } else { self.range })
    }
}

/// An artillery shell. Splash hurts enemies only: friendly fire would need an AI that avoids it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shell {
    /// Units within this distance of the impact point (edge, not centre) are hit.
    pub radius: Fx,
    /// World units per tick; flight time is the distance over this, at least one tick.
    pub speed: Fx,
    /// No firing at targets whose edge is closer than this.
    pub min_range: Fx,
}

/// A unit that must stop and set up to use its main gun (the Thunderfist's howitzer). It moves
/// and fights with its class weapon; deployed it can't move, fires `weapon`, and setting up or
/// packing up takes `ticks`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Deploy {
    pub weapon: Weapon,
    pub ticks: u32,
}

/// Room for infantry inside a building or vehicle, after the alpha's `RtsGarrisonComponentData`:
/// up to `capacity` units (a squad takes one place). With `fires`, they shoot out from its edge
/// with `range_bonus` added to their weapons' reach; inside they can't be targeted, and die
/// with it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Garrison {
    pub capacity: u32,
    pub fires: bool,
    pub range_bonus: Fx,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuildingRole {
    CommandCenter,
    SupplyCenter {
        collector_cap: u32,
    },
    PowerPlant,
    /// Must be placed on a free oil well; pays `credits_per_second` until the well runs dry.
    OilDerrick {
        credits_per_second: i64,
    },
    /// Trains combat units (barracks, war factory).
    Factory,
    /// Researches upgrades only (tech labs).
    Lab,
    /// A neutral building on the map (houses, shops): no production and no power. Infantry may
    /// garrison it; while they do, it belongs to their player.
    Civilian,
}

#[derive(Clone, Debug)]
pub struct Def {
    pub name: &'static str,
    pub faction: Option<Faction>,
    pub class: Class,
    pub hp: i32,
    pub armour: Armour,
    pub cost: Cost,
    pub build_ticks: u32,
    /// Collision/placement radius in world units.
    pub radius: Fx,
    pub produces: &'static [DefId],
    /// Buildings a constructor of this def may place.
    pub builds: &'static [DefId],
    /// Deployable units only: the set-up time and the deployed weapon.
    pub deploy: Option<Deploy>,
    /// Upgrades this building researches, after the units it trains in its build grid.
    pub researches: &'static [UpgradeId],
    /// Experience its killer gains when it is destroyed.
    pub experience: u32,
    /// Units that earn ranks; `None` for units that never do (and every building).
    pub veterancy: Option<Veterancy>,
    /// Units may enter it (`Garrison`); `None` for everything else.
    pub garrison: Option<Garrison>,
}

/// What a unit gains by rank, after the alpha's `RtsVeterancyComponentData`: three ranks at
/// `experience[i]` total experience from kills. Rank 1 adds `health` maximum hit points, rank 2
/// regains `idle_regen` hit points a second while the unit has no order, rank 3 fires faster:
/// `cooldown_percent` of the normal time between shots. A rank keeps every lower rank's gains.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Veterancy {
    pub experience: [u32; 3],
    pub health: i32,
    pub idle_regen: i32,
    pub cooldown_percent: u32,
}

impl Def {
    pub fn is_building(&self) -> bool {
        matches!(self.class, Class::Building { .. })
    }

    pub fn is_unit(&self) -> bool {
        matches!(self.class, Class::Unit { .. })
    }

    pub fn weapon(&self) -> Option<Weapon> {
        match self.class {
            Class::Unit { weapon, .. } => weapon,
            _ => None,
        }
    }

    pub fn collector(&self) -> Option<CollectorSpec> {
        match self.class {
            Class::Unit { collector, .. } => collector,
            _ => None,
        }
    }

    /// The weapon in use: the deployed one while `deployed`, else the class weapon.
    pub fn weapon_for(&self, deployed: bool) -> Option<Weapon> {
        match (deployed, self.deploy) {
            (true, Some(d)) => Some(d.weapon),
            _ => self.weapon(),
        }
    }

    pub fn is_constructor(&self) -> bool {
        matches!(self.class, Class::Unit { constructor: true, .. })
    }

    pub fn speed(&self) -> Fx {
        match self.class {
            Class::Unit { speed, .. } => speed,
            _ => Fx::ZERO,
        }
    }

    pub fn flies(&self) -> bool {
        matches!(self.class, Class::Unit { flies: true, .. })
    }

    /// Armed infantry, the only units that enter garrisons (the alpha's `IncludeInfantry`;
    /// constructors and collectors stay out).
    pub fn can_garrison(&self) -> bool {
        self.is_unit() && !self.flies() && self.armour == Armour::Infantry && self.weapon().is_some()
    }

    pub fn role(&self) -> Option<BuildingRole> {
        match self.class {
            Class::Building { role, .. } => Some(role),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct GeneralId(pub u16);

/// A general a player picks within a faction. Generals 2 had several per faction, each with
/// "unique advantages … balanced with disadvantages" (2012 developer
/// blog in archive/wayback). Their exact rules are unknown; this models them as data, the way
/// such designs do (per-unit cost and build-time changes), plus one active power.
#[derive(Clone, Debug)]
pub struct General {
    pub name: &'static str,
    pub faction: Faction,
    pub modifiers: &'static [Modifier],
    pub power: Option<Power>,
}

/// A general's active power: aimed at a point the player has explored, then unavailable for
/// `cooldown_ticks`. The first use is possible `first_ticks` into the match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Power {
    pub name: &'static str,
    pub kind: PowerKind,
    pub first_ticks: u32,
    pub cooldown_ticks: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerKind {
    /// `shells` artillery shells land around the point, one every `every_ticks` after
    /// `delay_ticks`, each like an artillery weapon's (damage, bonus, blast radius).
    Barrage { shells: u32, delay_ticks: u32, every_ticks: u32, damage: i32, bonus: [i32; 4], radius: Fx },
    /// Own units within `radius` of the point regain up to `hp` hit points.
    Repair { radius: Fx, hp: i32 },
    /// `count` units of `def` appear at and around the point, each on open ground (else the
    /// power is refused).
    Ambush { def: DefId, count: u32 },
}

/// A per-def change, in whole percent; several for one def multiply.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Modifier {
    /// `percent` of the normal cost.
    Cost { def: DefId, percent: i64 },
    /// `percent` of the normal build time.
    BuildTime { def: DefId, percent: u32 },
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct UpgradeId(pub u16);

/// A one-time research: once done, `effects` apply to every current and future entity of each
/// listed def the player owns. Researched in a building's production queue, like Zero Hour's
/// player upgrades; the buildings that offer it list it in `Def::researches`.
#[derive(Clone, Debug)]
pub struct Upgrade {
    pub name: &'static str,
    pub cost: Cost,
    pub research_ticks: u32,
    pub effects: &'static [(DefId, Effect)],
}

/// What an upgrade changes for one def. Percentages of the same kind add up rather than
/// multiply (two +25% damage upgrades make +50%), the way Zero Hour stacks weapon bonuses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Effect {
    /// Weapon damage, in percent of normal: against `against` only, or every armour class.
    Damage { percent: i32, against: Option<Armour> },
    /// Artillery shell damage only, in percent of normal; a direct-fire gun is unchanged.
    ShellDamage { percent: i32 },
    /// Time between shots, in percent of normal.
    Cooldown { percent: u32 },
    /// Damage taken, in percent of normal.
    Toughness { percent: i32 },
    /// More maximum hit points; existing entities gain the same amount of current ones.
    Health { add: i32 },
    /// Regains `hp` every second once `calm_ticks` have passed without being hit.
    Repair { hp: i32, calm_ticks: u32 },
}

pub struct Rules {
    pub defs: &'static [Def],
    pub generals: &'static [General],
    pub upgrades: &'static [Upgrade],
}

impl Rules {
    pub fn def(&self, id: DefId) -> &Def {
        &self.defs[id.0 as usize]
    }

    pub fn find(&self, name: &str) -> Option<DefId> {
        self.defs.iter().position(|d| d.name == name).map(|i| DefId(i as u16))
    }

    pub fn general(&self, id: GeneralId) -> &General {
        &self.generals[id.0 as usize]
    }

    pub fn upgrade(&self, id: UpgradeId) -> &Upgrade {
        &self.upgrades[id.0 as usize]
    }

    pub fn find_general(&self, name: &str) -> Option<GeneralId> {
        self.generals.iter().position(|g| g.name.eq_ignore_ascii_case(name)).map(|i| GeneralId(i as u16))
    }

    /// Cost of `def` for a player with `general`: integer percent, rounded down.
    pub fn cost(&self, general: Option<GeneralId>, def: DefId) -> Cost {
        let base = self.def(def).cost;
        let percent = self.modifiers(general).fold(100_i64, |p, m| match m {
            Modifier::Cost { def: d, percent } if d == def => p * percent / 100,
            _ => p,
        });
        Cost { credits: base.credits * percent / 100 }
    }

    /// Build time of `def` in ticks for a player with `general`; at least one tick.
    pub fn build_ticks(&self, general: Option<GeneralId>, def: DefId) -> u32 {
        let percent = self.modifiers(general).fold(100_u32, |p, m| match m {
            Modifier::BuildTime { def: d, percent } if d == def => p * percent / 100,
            _ => p,
        });
        (self.def(def).build_ticks * percent / 100).max(1)
    }

    fn modifiers(&self, general: Option<GeneralId>) -> impl Iterator<Item = Modifier> + '_ {
        general.into_iter().flat_map(|g| self.general(g).modifiers.iter().copied())
    }
}
