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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuildingRole {
    CommandCenter,
    SupplyCenter { collector_cap: u32 },
    PowerPlant,
    /// Must be placed on a free oil well; yields `oil_per_second` until the well runs dry.
    OilDerrick { oil_per_second: i64 },
    Factory,
}

#[derive(Clone, Debug)]
pub struct Def {
    pub name: &'static str,
    pub faction: Option<Faction>,
    pub class: Class,
    pub hp: i32,
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

pub struct Rules {
    pub defs: &'static [Def],
}

impl Rules {
    pub fn def(&self, id: DefId) -> &Def {
        &self.defs[id.0 as usize]
    }

    pub fn find(&self, name: &str) -> Option<DefId> {
        self.defs.iter().position(|d| d.name == name).map(|i| DefId(i as u16))
    }
}
