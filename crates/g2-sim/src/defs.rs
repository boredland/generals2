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

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Cost {
    pub credits: i64,
    pub oil: i64,
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

/// An instant-hit weapon: every `cooldown_ticks` it deals `damage_vs(armour)` to one target
/// whose edge is within `range` of the shooter's centre.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Weapon {
    pub damage: i32,
    /// Extra damage per armour class, indexed by `Armour as usize`; may be negative.
    pub bonus: [i32; 4],
    pub range: Fx,
    pub cooldown_ticks: u32,
}

impl Weapon {
    /// At least 1, so every hit counts.
    pub fn damage_vs(&self, armour: Armour) -> i32 {
        (self.damage + self.bonus[armour as usize]).max(1)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuildingRole {
    CommandCenter,
    SupplyCenter {
        collector_cap: u32,
    },
    PowerPlant,
    /// Must be placed on a free oil well; yields `oil_per_second` until the well runs dry.
    OilDerrick {
        oil_per_second: i64,
    },
    /// Trains combat units (barracks, war factory).
    Factory,
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

    pub fn is_constructor(&self) -> bool {
        matches!(self.class, Class::Unit { constructor: true, .. })
    }

    pub fn speed(&self) -> Fx {
        match self.class {
            Class::Unit { speed, .. } => speed,
            _ => Fx::ZERO,
        }
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

/// A general a player picks within a faction. Generals 2 had several per faction (docs/
/// field notes), each with "unique advantages … balanced with disadvantages" (2012 developer
/// blog in archive/wayback). Their exact rules are unknown; this models them as data, the way
/// such designs do (per-unit cost and build-time changes).
#[derive(Clone, Debug)]
pub struct General {
    pub name: &'static str,
    pub faction: Faction,
    pub modifiers: &'static [Modifier],
}

/// A per-def change, in whole percent; several for one def multiply.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Modifier {
    /// `cost_percent` of the normal credit and oil cost.
    Cost { def: DefId, percent: i64 },
    /// `percent` of the normal build time.
    BuildTime { def: DefId, percent: u32 },
}

pub struct Rules {
    pub defs: &'static [Def],
    pub generals: &'static [General],
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
        Cost { credits: base.credits * percent / 100, oil: base.oil * percent / 100 }
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
