//! Definitions for the three factions and the built-in maps.
//!
//! Values marked `// src:` come from archived material (see the local field notes for the file).
//! Everything else is a placeholder chosen for playability and will be tuned.

use g2_sim::defs::{BuildingRole, Class, CollectorSpec, Cost, Def, DefId, Faction, Rules};
use g2_sim::{Fx, MapDef, Vec2};

/// Stable ids into `RULES.defs`; `tests::id_constants_match_table_order` keeps them in sync.
pub mod ids {
    use g2_sim::DefId;
    pub const SUPPLY_PILE: DefId = DefId(0);
    pub const OIL_WELL: DefId = DefId(1);
    pub const EU_CC: DefId = DefId(2);
    pub const EU_DOZER: DefId = DefId(3);
    pub const EU_SUPPLY: DefId = DefId(4);
    pub const EU_HARVESTER: DefId = DefId(5);
    pub const EU_POWER: DefId = DefId(6);
    pub const EU_DERRICK: DefId = DefId(7);
    pub const APA_CC: DefId = DefId(8);
    pub const APA_CRANE: DefId = DefId(9);
    pub const APA_SUPPLY: DefId = DefId(10);
    pub const APA_HARVESTER: DefId = DefId(11);
    pub const APA_POWER: DefId = DefId(12);
    pub const APA_DERRICK: DefId = DefId(13);
    pub const GLA_CC: DefId = DefId(14);
    pub const GLA_WORKER: DefId = DefId(15);
    pub const GLA_STASH: DefId = DefId(16);
    pub const GLA_DERRICK: DefId = DefId(17);
}
use ids::*;

const fn secs(s: u32) -> u32 {
    s * g2_sim::TICKS_PER_SECOND
}

/// World units per tick from world units per second.
const fn per_sec(units: i64) -> Fx {
    Fx::from_ratio(units, g2_sim::TICKS_PER_SECOND as i64)
}

const fn credits(c: i64) -> Cost {
    Cost { credits: c, oil: 0 }
}

const NONE: &[DefId] = &[];

/// Positional on purpose: each table row then reads as a single line of stats.
#[allow(clippy::too_many_arguments)]
const fn building(
    name: &'static str,
    faction: Faction,
    role: BuildingRole,
    power: i32,
    hp: i32,
    cost: Cost,
    build_secs: u32,
    radius: i64,
    produces: &'static [DefId],
) -> Def {
    Def {
        name,
        faction: Some(faction),
        class: Class::Building { power, power_radius: Fx::from_int(if power > 0 { 60 } else { 0 }), role },
        hp,
        cost,
        build_ticks: secs(build_secs),
        radius: Fx::from_int(radius),
        produces,
        builds: NONE,
    }
}

#[allow(clippy::too_many_arguments)]
const fn unit(
    name: &'static str,
    faction: Faction,
    hp: i32,
    cost: Cost,
    build_secs: u32,
    speed: i64,
    constructor: bool,
    collector: Option<CollectorSpec>,
    builds: &'static [DefId],
) -> Def {
    Def {
        name,
        faction: Some(faction),
        class: Class::Unit { speed: per_sec(speed), constructor, collector },
        hp,
        cost,
        build_ticks: secs(build_secs),
        radius: Fx::from_int(2),
        produces: NONE,
        builds,
    }
}

const HARVEST: CollectorSpec = CollectorSpec { capacity: 100, load_ticks: secs(3), unload_ticks: secs(1) };
const DERRICK: BuildingRole = BuildingRole::OilDerrick { oil_per_second: 5 };

static DEFS: [Def; 18] = [
    Def {
        name: "Supply Pile",
        faction: None,
        class: Class::SupplyPile,
        hp: 1,
        cost: credits(0),
        build_ticks: 0,
        radius: Fx::from_int(4),
        produces: NONE,
        builds: NONE,
    },
    Def {
        name: "Oil Well",
        faction: None,
        class: Class::OilWell,
        hp: 1,
        cost: credits(0),
        build_ticks: 0,
        radius: Fx::from_int(5),
        produces: NONE,
        builds: NONE,
    },
    // ---- European Union ------------------------------------------------------------------
    // src: Command center: hp 1751, cost 1000, 28 s, power +110 (0 for GLA).
    building(
        "EU Command Center",
        Faction::Eu,
        BuildingRole::CommandCenter,
        110,
        1751,
        credits(1000),
        28,
        10,
        &[EU_DOZER],
    ),
    // src: Bulldozer hp 250, land speed 14.
    unit("EU Bulldozer", Faction::Eu, 250, credits(500), 12, 14, true, None, &[EU_SUPPLY, EU_POWER, EU_DERRICK]),
    // src: Supply center collector cap EU 3.
    building(
        "EU Supply Center",
        Faction::Eu,
        BuildingRole::SupplyCenter { collector_cap: 3 },
        -20,
        1200,
        credits(800),
        20,
        8,
        &[EU_HARVESTER, EU_DOZER],
    ),
    // src: EU harvester $450, 0:20, airborne collector (flying is not modelled yet).
    unit("EU Harvester", Faction::Eu, 300, credits(450), 20, 24, false, Some(HARVEST), NONE),
    building("EU Power Plant", Faction::Eu, BuildingRole::PowerPlant, 100, 800, credits(600), 15, 6, NONE),
    building("EU Oil Derrick", Faction::Eu, DERRICK, -10, 600, credits(500), 15, 5, NONE),
    // ---- Asia-Pacific Alliance -----------------------------------------------------------
    building(
        "APA Command Center",
        Faction::Apa,
        BuildingRole::CommandCenter,
        110,
        1751,
        credits(1000),
        28,
        10,
        &[APA_CRANE],
    ),
    // src: Crane hp 280, $150, 11 s.
    unit("APA Crane", Faction::Apa, 280, credits(150), 11, 13, true, None, &[APA_SUPPLY, APA_POWER, APA_DERRICK]),
    // src: Supply center collector cap APA 5.
    building(
        "APA Supply Center",
        Faction::Apa,
        BuildingRole::SupplyCenter { collector_cap: 5 },
        -20,
        1200,
        credits(800),
        20,
        8,
        &[APA_HARVESTER, APA_CRANE],
    ),
    // src: APA harvester $300, 0:19.
    unit("APA Harvester", Faction::Apa, 350, credits(300), 19, 18, false, Some(HARVEST), NONE),
    building("APA Power Plant", Faction::Apa, BuildingRole::PowerPlant, 100, 800, credits(600), 15, 6, NONE),
    building("APA Oil Derrick", Faction::Apa, DERRICK, -10, 600, credits(500), 15, 5, NONE),
    // ---- Global Liberation Army ----------------------------------------------------------
    // src: GLA command center gives 0 power; the GLA has no power grid.
    building(
        "GLA Command Center",
        Faction::Gla,
        BuildingRole::CommandCenter,
        0,
        1751,
        credits(1000),
        28,
        10,
        &[GLA_WORKER],
    ),
    // src: workers construct and gather supplies.
    unit("GLA Worker", Faction::Gla, 120, credits(200), 8, 16, true, Some(HARVEST_WORKER), &[GLA_STASH, GLA_DERRICK]),
    // src: Supply center collector cap GLA 8.
    building(
        "GLA Supply Stash",
        Faction::Gla,
        BuildingRole::SupplyCenter { collector_cap: 8 },
        0,
        1000,
        credits(600),
        18,
        8,
        &[GLA_WORKER],
    ),
    building("GLA Oil Derrick", Faction::Gla, DERRICK, 0, 500, credits(400), 12, 5, NONE),
];

/// Workers carry less per trip; the GLA compensates with a higher collector cap.
const HARVEST_WORKER: CollectorSpec = CollectorSpec { capacity: 40, load_ticks: secs(2), unload_ticks: secs(1) };

pub static RULES: Rules = Rules { defs: &DEFS };

pub fn rules() -> &'static Rules {
    &RULES
}

/// Original two-player test map: mirrored bases, four supply piles, four oil wells.
pub fn crater_ridge() -> MapDef {
    let v = Vec2::from_int;
    MapDef {
        name: "Crater Ridge".to_owned(),
        width: Fx::from_int(400),
        height: Fx::from_int(400),
        spawns: vec![v(80, 80), v(320, 320)],
        supply_piles: vec![(v(130, 60), 6000), (v(270, 340), 6000), (v(200, 140), 10000), (v(200, 260), 10000)],
        oil_wells: vec![(v(60, 150), 3000), (v(340, 250), 3000), (v(120, 280), 5000), (v(280, 120), 5000)],
    }
}

pub fn map_by_name(name: &str) -> Option<MapDef> {
    (name == "Crater Ridge").then(crater_ridge)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_constants_match_table_order() {
        let r = rules();
        let named = [
            (SUPPLY_PILE, "Supply Pile"),
            (OIL_WELL, "Oil Well"),
            (EU_CC, "EU Command Center"),
            (EU_DOZER, "EU Bulldozer"),
            (EU_SUPPLY, "EU Supply Center"),
            (EU_HARVESTER, "EU Harvester"),
            (EU_POWER, "EU Power Plant"),
            (EU_DERRICK, "EU Oil Derrick"),
            (APA_CC, "APA Command Center"),
            (APA_CRANE, "APA Crane"),
            (APA_SUPPLY, "APA Supply Center"),
            (APA_HARVESTER, "APA Harvester"),
            (APA_POWER, "APA Power Plant"),
            (APA_DERRICK, "APA Oil Derrick"),
            (GLA_CC, "GLA Command Center"),
            (GLA_WORKER, "GLA Worker"),
            (GLA_STASH, "GLA Supply Stash"),
            (GLA_DERRICK, "GLA Oil Derrick"),
        ];
        for (id, name) in named {
            assert_eq!(r.def(id).name, name);
        }
    }

    #[test]
    fn every_faction_has_one_command_center_with_a_constructor() {
        let r = rules();
        for f in Faction::ALL {
            let dozer = r.starting_constructor(f);
            assert_eq!(r.def(dozer).faction, Some(f));
            for &b in r.def(dozer).builds {
                assert_eq!(r.def(b).faction, Some(f), "{} builds a foreign building", r.def(dozer).name);
            }
        }
    }

    #[test]
    fn gla_buildings_neither_supply_nor_draw_power() {
        let r = rules();
        for d in r.defs.iter().filter(|d| d.faction == Some(Faction::Gla)) {
            if let Class::Building { power, .. } = d.class {
                assert_eq!(power, 0, "{}", d.name);
            }
        }
    }
}
