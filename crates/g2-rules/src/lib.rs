//! Definitions for the three factions and the built-in maps.
//!
//! Values marked `// src:` come from archived material (see the local field notes for the file).
//! Everything else is a placeholder chosen for playability and will be tuned.

use g2_sim::defs::{
    Armour, BuildingRole, Class, CollectorSpec, Cost, Def, DefId, Faction, General, Modifier, Rules, Weapon,
};
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
    pub const EU_BARRACKS: DefId = DefId(18);
    pub const EU_FACTORY: DefId = DefId(19);
    pub const EU_RECON: DefId = DefId(20);
    pub const EU_AEGIS: DefId = DefId(21);
    pub const APA_BARRACKS: DefId = DefId(22);
    pub const APA_FACTORY: DefId = DefId(23);
    pub const APA_RED_GUARD: DefId = DefId(24);
    pub const APA_WARLORD: DefId = DefId(25);
    pub const GLA_BARRACKS: DefId = DefId(26);
    pub const GLA_FACTORY: DefId = DefId(27);
    pub const GLA_RABBLE: DefId = DefId(28);
    pub const GLA_TANK: DefId = DefId(29);
}

/// Stable ids into `RULES.generals`.
pub mod generals {
    use g2_sim::defs::GeneralId;
    pub const BULLDOG: GeneralId = GeneralId(0);
    pub const TASKMASTER: GeneralId = GeneralId(1);
    pub const JUNKYARD: GeneralId = GeneralId(2);
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
    Cost { credits: c }
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
        armour: Armour::Structure,
        cost,
        build_ticks: secs(build_secs),
        radius: Fx::from_int(radius),
        produces,
        builds: NONE,
    }
}

/// An unarmed unit: constructor or collector. Without a sourced armour class, vehicles are
/// `Light` and GLA workers `Infantry` (placeholders); `with_armour` sets a sourced one.
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
        class: Class::Unit { speed: per_sec(speed), constructor, collector, weapon: None },
        hp,
        armour: if matches!(faction, Faction::Gla) { Armour::Infantry } else { Armour::Light },
        cost,
        build_ticks: secs(build_secs),
        radius: Fx::from_int(2),
        produces: NONE,
        builds,
    }
}

const fn with_armour(mut d: Def, armour: Armour) -> Def {
    d.armour = armour;
    d
}

/// A combat unit; one entity per squad, with the squad's card values. `build_ticks` rather
/// than seconds, since the cards give hundredths.
#[allow(clippy::too_many_arguments)]
const fn soldier(
    name: &'static str,
    faction: Faction,
    hp: i32,
    armour: Armour,
    cost: Cost,
    build_ticks: u32,
    speed: i64,
    radius: i64,
    weapon: Weapon,
) -> Def {
    Def {
        name,
        faction: Some(faction),
        class: Class::Unit { speed: per_sec(speed), constructor: false, collector: None, weapon: Some(weapon) },
        hp,
        armour,
        cost,
        build_ticks,
        radius: Fx::from_int(radius),
        produces: NONE,
        builds: NONE,
    }
}

/// `damage` plus `[infantry, light, heavy, structure]` bonuses, `range` in world units and
/// `cooldown` in tenths of a second, as the archived cards give them.
const fn weapon(damage: i32, bonus: [i32; 4], range: i64, cooldown_tenths: u32) -> Weapon {
    Weapon {
        damage,
        bonus,
        range: Fx::from_int(range),
        cooldown_ticks: cooldown_tenths * g2_sim::TICKS_PER_SECOND / 10,
    }
}

/// Hundredths of a second to ticks, rounded down.
const fn centis(t: u32) -> u32 {
    t * g2_sim::TICKS_PER_SECOND / 100
}

const FACTORY: BuildingRole = BuildingRole::Factory;

// src: Red Guard (APA basic infantry): 13 (+2 vs. infantry), cooldown 0.9, range 25. Recon
// squad and Rabble squad have no weapon card; they reuse it as a placeholder.
const RIFLES: Weapon = weapon(13, [2, 0, 0, 0], 25, 9);
// src: Warlord tank: 40 (bonus 30 vs. Light), cooldown 1.50, range 35. Placeholder for Aegis.
const TANK_GUN: Weapon = weapon(40, [0, 30, 0, 0], 35, 15);

const HARVEST: CollectorSpec = CollectorSpec { capacity: 100, load_ticks: secs(3), unload_ticks: secs(1) };
const DERRICK: BuildingRole = BuildingRole::OilDerrick { credits_per_second: 5 };

static DEFS: [Def; 30] = [
    Def {
        name: "Supply Pile",
        faction: None,
        class: Class::SupplyPile,
        hp: 1,
        cost: credits(0),
        armour: Armour::Structure,
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
        armour: Armour::Structure,
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
    // src: Bulldozer hp 250, armour Heavy, land speed 14.
    with_armour(
        unit(
            "EU Bulldozer",
            Faction::Eu,
            250,
            credits(500),
            12,
            14,
            true,
            None,
            &[EU_SUPPLY, EU_POWER, EU_DERRICK, EU_BARRACKS, EU_FACTORY],
        ),
        Armour::Heavy,
    ),
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
    unit(
        "APA Crane",
        Faction::Apa,
        280,
        credits(150),
        11,
        13,
        true,
        None,
        &[APA_SUPPLY, APA_POWER, APA_DERRICK, APA_BARRACKS, APA_FACTORY],
    ),
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
    unit(
        "GLA Worker",
        Faction::Gla,
        120,
        credits(200),
        8,
        16,
        true,
        Some(HARVEST_WORKER),
        &[GLA_STASH, GLA_DERRICK, GLA_BARRACKS, GLA_FACTORY],
    ),
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
    // ---- Combat ----------------------------------------------------------------------------
    // Barracks numbers are placeholders. War factories copy the GLA war factory card.
    building("EU Barracks", Faction::Eu, FACTORY, -10, 800, credits(500), 15, 7, &[EU_RECON]),
    building("EU War Factory", Faction::Eu, FACTORY, -20, 1000, credits(900), 25, 9, &[EU_AEGIS]),
    // src: Recon squad hp 200, basic infantry. Cost, time, speed and weapon: Red Guard's.
    soldier("EU Recon Squad", Faction::Eu, 200, Armour::Infantry, credits(150), centis(627), 9, 2, RIFLES),
    // src: Aegis tank hp 660, $990, 24.44 s, main battle tank. Armour, speed, gun: Warlord's.
    soldier("EU Aegis Tank", Faction::Eu, 660, Armour::Heavy, credits(990), centis(2444), 9, 3, TANK_GUN),
    building("APA Barracks", Faction::Apa, FACTORY, -10, 800, credits(500), 15, 7, &[APA_RED_GUARD]),
    building("APA War Factory", Faction::Apa, FACTORY, -20, 1000, credits(900), 25, 9, &[APA_WARLORD]),
    // src: Red Guard hp 180, Infantry, $150, 6.27 s, land speed 9.
    soldier("APA Red Guard", Faction::Apa, 180, Armour::Infantry, credits(150), centis(627), 9, 2, RIFLES),
    // src: Warlord tank hp 610, Heavy Mechanical, $990, land speed 9. Build time: Aegis's.
    soldier("APA Warlord Tank", Faction::Apa, 610, Armour::Heavy, credits(990), centis(2444), 9, 3, TANK_GUN),
    building("GLA Barracks", Faction::Gla, FACTORY, 0, 800, credits(500), 15, 7, &[GLA_RABBLE]),
    // src: GLA war factory hp 1000, $900, 25 s.
    building("GLA War Factory", Faction::Gla, FACTORY, 0, 1000, credits(900), 25, 9, &[GLA_TANK]),
    // src: Rabble squad: anti-infantry / light-armour infantry. Numbers are placeholders.
    soldier(
        "GLA Rabble Squad",
        Faction::Gla,
        160,
        Armour::Infantry,
        credits(120),
        secs(6),
        10,
        2,
        weapon(12, [4, 4, 0, 0], 20, 10),
    ),
    // src: Scorpion tanks "are lightly armored but find strength in numbers". Numbers are
    // placeholders: cheaper, faster and weaker than the Warlord.
    soldier(
        "GLA Scorpion Tank",
        Faction::Gla,
        420,
        Armour::Light,
        credits(700),
        secs(18),
        11,
        3,
        weapon(30, [0, 15, 0, 0], 30, 12),
    ),
];

/// Workers carry less per trip; the GLA compensates with a higher collector cap.
const HARVEST_WORKER: CollectorSpec = CollectorSpec { capacity: 40, load_ticks: secs(2), unload_ticks: secs(1) };

pub static RULES: Rules = Rules { defs: &DEFS, generals: &GENERALS };

/// One general per faction for now, to exercise generals-as-data. The names are documented G2
/// generals (the local field notes); what each changed is not, so these economy modifiers are
/// placeholders in the spirit of pre-release sub-general designs (e.g. a supply doctrine:
/// cheaper supply trucks, faster builds).
static GENERALS: [General; 3] = [
    General {
        name: "Bulldog",
        faction: Faction::Eu,
        modifiers: &[
            Modifier::Cost { def: EU_HARVESTER, percent: 80 },
            Modifier::BuildTime { def: EU_HARVESTER, percent: 85 },
            Modifier::Cost { def: EU_POWER, percent: 120 },
        ],
    },
    General {
        name: "Taskmaster",
        faction: Faction::Apa,
        modifiers: &[
            Modifier::BuildTime { def: APA_SUPPLY, percent: 75 },
            Modifier::BuildTime { def: APA_DERRICK, percent: 75 },
            Modifier::Cost { def: APA_HARVESTER, percent: 115 },
        ],
    },
    General {
        name: "Junkyard",
        faction: Faction::Gla,
        modifiers: &[
            Modifier::Cost { def: GLA_WORKER, percent: 75 },
            Modifier::BuildTime { def: GLA_STASH, percent: 125 },
        ],
    },
];

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
        terrain: Vec::new(),
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
            (EU_BARRACKS, "EU Barracks"),
            (EU_FACTORY, "EU War Factory"),
            (EU_RECON, "EU Recon Squad"),
            (EU_AEGIS, "EU Aegis Tank"),
            (APA_BARRACKS, "APA Barracks"),
            (APA_FACTORY, "APA War Factory"),
            (APA_RED_GUARD, "APA Red Guard"),
            (APA_WARLORD, "APA Warlord Tank"),
            (GLA_BARRACKS, "GLA Barracks"),
            (GLA_FACTORY, "GLA War Factory"),
            (GLA_RABBLE, "GLA Rabble Squad"),
            (GLA_TANK, "GLA Scorpion Tank"),
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

    #[test]
    fn generals_ids_match_and_only_modify_their_own_faction() {
        let r = rules();
        for (id, name) in
            [(generals::BULLDOG, "Bulldog"), (generals::TASKMASTER, "Taskmaster"), (generals::JUNKYARD, "Junkyard")]
        {
            assert_eq!(r.general(id).name, name);
            assert_eq!(r.find_general(&name.to_lowercase()), Some(id));
        }
        for g in r.generals {
            for m in g.modifiers {
                let (Modifier::Cost { def, .. } | Modifier::BuildTime { def, .. }) = *m;
                assert_eq!(r.def(def).faction, Some(g.faction), "{} modifies {}", g.name, r.def(def).name);
            }
        }
    }
}
