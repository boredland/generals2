//! Definitions for the three factions and the built-in maps.
//!
//! Values marked `// src:` come from archived reference material, documented in notes kept
//! outside this repository.
//! Everything else is a placeholder chosen for playability and will be tuned.
use g2_sim::defs::{
    Armour, BuildingRole, Class, CollectorSpec, Cost, Def, DefId, Deploy, Effect, Faction, General, Modifier, Power,
    PowerKind, Rules, Shell, Targets, Upgrade, UpgradeId, Veterancy, Weapon,
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
    pub const APA_THUNDERFIST: DefId = DefId(30);
    pub const GLA_TOXIN_ARTILLERY: DefId = DefId(31);
    pub const EU_ARTILLERY: DefId = DefId(32);
    pub const EU_ROCKETS: DefId = DefId(33);
    pub const APA_TANK_HUNTER: DefId = DefId(34);
    pub const GLA_RPG: DefId = DefId(35);
    pub const EU_APC: DefId = DefId(36);
    pub const APA_GATLING: DefId = DefId(37);
    pub const GLA_TECHNICAL: DefId = DefId(38);
    pub const EU_TECH_LAB: DefId = DefId(39);
    pub const EU_ADVANCED_LAB: DefId = DefId(40);
    pub const GLA_SALVAGE_BAY: DefId = DefId(41);
    pub const EU_AIRFIELD: DefId = DefId(42);
    pub const APA_AIRFIELD: DefId = DefId(43);
    pub const EU_VALKYRIE: DefId = DefId(44);
    pub const APA_MANTIS: DefId = DefId(45);
    pub const GLA_TOXIN_CHOPPER: DefId = DefId(46);
}

/// Stable ids into `RULES.upgrades`.
pub mod upgrades {
    use g2_sim::defs::UpgradeId;
    pub const HIGHTECH_ALLOYS: UpgradeId = UpgradeId(0);
    pub const IMPROVED_BALLISTICS: UpgradeId = UpgradeId(1);
    pub const SENIOR_WARFIGHTER: UpgradeId = UpgradeId(2);
    pub const HIGH_EXPLOSIVES: UpgradeId = UpgradeId(3);
    pub const HOLLOW_POINT: UpgradeId = UpgradeId(4);
    pub const POPULATION_CONTROL: UpgradeId = UpgradeId(5);
    pub const SHORTENED_FUSES: UpgradeId = UpgradeId(6);
    pub const SCRAP_REPAIR: UpgradeId = UpgradeId(7);
    pub const FIRESTORM_SHELLS: UpgradeId = UpgradeId(8);
}

/// Stable ids into `RULES.generals`.
pub mod generals {
    use g2_sim::defs::GeneralId;
    pub const BULLDOG: GeneralId = GeneralId(0);
    pub const TASKMASTER: GeneralId = GeneralId(1);
    pub const JUNKYARD: GeneralId = GeneralId(2);
}
use ids::*;
use upgrades::*;

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
const NO_RESEARCH: &[UpgradeId] = &[];

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
        deploy: None,
        researches: NO_RESEARCH,
        experience: 0,
        veterancy: None,
    }
}

/// `d` researches `upgrades`, shown after the units it trains.
const fn researching(mut d: Def, upgrades: &'static [UpgradeId]) -> Def {
    d.researches = upgrades;
    d
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
        class: Class::Unit { speed: per_sec(speed), constructor, collector, weapon: None, flies: false },
        hp,
        armour: if matches!(faction, Faction::Gla) { Armour::Infantry } else { Armour::Light },
        cost,
        build_ticks: secs(build_secs),
        radius: Fx::from_int(2),
        produces: NONE,
        builds,
        deploy: None,
        researches: NO_RESEARCH,
        experience: 1,
        veterancy: None,
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
        class: Class::Unit {
            speed: per_sec(speed),
            constructor: false,
            collector: None,
            weapon: Some(weapon),
            flies: false,
        },
        hp,
        armour,
        cost,
        build_ticks,
        radius: Fx::from_int(radius),
        produces: NONE,
        builds: NONE,
        deploy: None,
        researches: NO_RESEARCH,
        experience: 1,
        veterancy: None,
    }
}

/// `d` earns ranks, with the alpha's rules for every unit: 1 experience per kill (buildings
/// grant none), ranks at 3, 6 and 10, rank 2 regenerating 5 hit points a second while idle.
/// `health` is its rank-1 hit points and `cooldown_percent` its rank-3 fire interval, from
/// the alpha unit's own `RtsVeterancyComponentData` (the interval cut as a share of that unit's
/// main weapon interval, since some of our guns are stand-ins with other intervals).
const fn ranked(mut d: Def, health: i32, cooldown_percent: u32) -> Def {
    d.veterancy = Some(Veterancy { experience: [3, 6, 10], health, idle_regen: 5, cooldown_percent });
    d
}

/// `d` flies: over everything, out of reach of ground-only weapons.
const fn flying(mut d: Def) -> Def {
    if let Class::Unit { speed, constructor, collector, weapon, .. } = d.class {
        d.class = Class::Unit { speed, constructor, collector, weapon, flies: true };
    }
    d
}

/// `damage` plus `[infantry, light, heavy, structure]` bonuses, `range` in world units and
/// `cooldown` in tenths of a second, as the archived cards give them. Ground targets only.
const fn weapon(damage: i32, bonus: [i32; 4], range: i64, cooldown_tenths: u32) -> Weapon {
    Weapon {
        damage,
        bonus,
        range: Fx::from_int(range),
        cooldown_ticks: cooldown_tenths * g2_sim::TICKS_PER_SECOND / 10,
        shell: None,
        targets: Targets::Ground,
        air_range: None,
    }
}

/// `w` also fires at aircraft, reaching `air_range` against them (the alpha's anti-air
/// weapon spec on the same unit).
const fn anti_air(mut w: Weapon, air_range: i64) -> Weapon {
    w.targets = Targets::Both;
    w.air_range = Some(Fx::from_int(air_range));
    w
}

/// `w` firing artillery shells: blast `radius`, flight `speed` in world units per second,
/// no targets closer than `min_range`.
const fn shelling(mut w: Weapon, radius: i64, speed: i64, min_range: i64) -> Weapon {
    w.shell = Some(Shell { radius: Fx::from_int(radius), speed: per_sec(speed), min_range: Fx::from_int(min_range) });
    w
}

/// `d` deploys for `weapon`, taking `set_up_tenths` tenths of a second each way.
const fn deploying(mut d: Def, weapon: Weapon, set_up_tenths: u32) -> Def {
    d.deploy = Some(Deploy { weapon, ticks: set_up_tenths * g2_sim::TICKS_PER_SECOND / 10 });
    d
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
// src: Thunderfist undeployed cannon 40 (+30 vs. light units), cooldown 1.65, range 30.
const THUNDERFIST_CANNON: Weapon = weapon(40, [0, 30, 0, 0], 30, 16);
// src: Thunderfist deployed howitzer 100 (+30 vs. light units, +100 vs. garrisoned buildings),
// cooldown 4, range 65. There are no garrisons, so the +100 goes to every building (a
// placeholder). Blast radius 6, shell speed 40/s and minimum range 20: placeholders, after the
// way Zero Hour's artillery (Inferno cannon: radius 15, minimum range 50 of 300) is shaped.
const HOWITZER: Weapon = shelling(weapon(100, [0, 30, 0, 100], 65, 40), 6, 40, 20);
// Placeholder: GLA Toxin Artillery's "tri-artillery cannon" has no numbers on its card, and
// its toxin damage over time is not modelled. A lighter, faster-firing shell than the howitzer.
const TOXIN_SHELL: Weapon = shelling(weapon(45, [15, 0, 0, 0], 55, 25), 8, 40, 15);
// Placeholder: the EU has no artillery card in the archive (only the general Salvo's artillery
// strike). A mid-weight shell between the other two.
const EU_SHELL: Weapon = shelling(weapon(70, [0, 20, 0, 40], 60, 30), 6, 45, 18);
// Placeholder: the rocket squads' cards say "anti-armor" but give no weapon numbers. A heavy
// rocket every 2 s with a little more than a rifle's reach: per credit it does about twice a
// rifle squad's damage to tanks and two thirds of it to infantry.
const ROCKETS: Weapon = weapon(45, [0, 10, 75, 10], 30, 20);

const HARVEST: CollectorSpec = CollectorSpec { capacity: 100, load_ticks: secs(3), unload_ticks: secs(1) };
const DERRICK: BuildingRole = BuildingRole::OilDerrick { credits_per_second: 5 };

static DEFS: [Def; 47] = [
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
        deploy: None,
        researches: NO_RESEARCH,
        experience: 0,
        veterancy: None,
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
        deploy: None,
        researches: NO_RESEARCH,
        experience: 0,
        veterancy: None,
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
            &[EU_SUPPLY, EU_POWER, EU_DERRICK, EU_BARRACKS, EU_FACTORY, EU_TECH_LAB, EU_ADVANCED_LAB, EU_AIRFIELD],
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
        &[APA_SUPPLY, APA_POWER, APA_DERRICK, APA_BARRACKS, APA_FACTORY, APA_AIRFIELD],
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
        &[GLA_STASH, GLA_DERRICK, GLA_BARRACKS, GLA_FACTORY, GLA_SALVAGE_BAY],
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
    // Researches: where the alpha's production data offers each upgrade.
    researching(
        building("EU Barracks", Faction::Eu, FACTORY, -10, 800, credits(500), 15, 7, &[EU_RECON, EU_ROCKETS]),
        &[HOLLOW_POINT],
    ),
    building("EU War Factory", Faction::Eu, FACTORY, -20, 1000, credits(900), 25, 9, &[EU_AEGIS, EU_ARTILLERY, EU_APC]),
    // Ranks (`ranked`): each unit's rank-1 health and rank-3 fire interval come from the alpha
    // unit it stands for (EU Howitzer: the rocket vehicle).
    // src: Recon squad hp 200, basic infantry. Cost, time, speed and weapon: Red Guard's.
    ranked(
        soldier("EU Recon Squad", Faction::Eu, 200, Armour::Infantry, credits(150), centis(627), 9, 2, RIFLES),
        21,
        93,
    ),
    // src: Aegis tank hp 660, $990, 24.44 s, main battle tank. Armour, speed, gun: Warlord's.
    ranked(
        soldier("EU Aegis Tank", Faction::Eu, 660, Armour::Heavy, credits(990), centis(2444), 9, 3, TANK_GUN),
        90,
        92,
    ),
    researching(
        building(
            "APA Barracks",
            Faction::Apa,
            FACTORY,
            -10,
            800,
            credits(500),
            15,
            7,
            &[APA_RED_GUARD, APA_TANK_HUNTER],
        ),
        &[POPULATION_CONTROL],
    ),
    // The alpha researches Firestorm Shells at the advanced war factory, which isn't modelled.
    researching(
        building(
            "APA War Factory",
            Faction::Apa,
            FACTORY,
            -20,
            1000,
            credits(900),
            25,
            9,
            &[APA_WARLORD, APA_THUNDERFIST, APA_GATLING],
        ),
        &[FIRESTORM_SHELLS],
    ),
    // src: Red Guard hp 180, Infantry, $150, 6.27 s, land speed 9.
    ranked(
        soldier("APA Red Guard", Faction::Apa, 180, Armour::Infantry, credits(150), centis(627), 9, 2, RIFLES),
        12,
        94,
    ),
    // src: Warlord tank hp 610, Heavy Mechanical, $990, land speed 9. Build time: Aegis's.
    ranked(
        soldier("APA Warlord Tank", Faction::Apa, 610, Armour::Heavy, credits(990), centis(2444), 9, 3, TANK_GUN),
        68,
        93,
    ),
    researching(
        building("GLA Barracks", Faction::Gla, FACTORY, 0, 800, credits(500), 15, 7, &[GLA_RABBLE, GLA_RPG]),
        &[SHORTENED_FUSES],
    ),
    // src: GLA war factory hp 1000, $900, 25 s.
    building(
        "GLA War Factory",
        Faction::Gla,
        FACTORY,
        0,
        1000,
        credits(900),
        25,
        9,
        &[GLA_TANK, GLA_TOXIN_ARTILLERY, GLA_TECHNICAL, GLA_TOXIN_CHOPPER],
    ),
    // src: Rabble squad: anti-infantry / light-armour infantry. Numbers are placeholders.
    ranked(
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
        30,
        94,
    ),
    // src: Scorpion tanks "are lightly armored but find strength in numbers". Numbers are
    // placeholders: cheaper, faster and weaker than the Warlord.
    ranked(
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
        72,
        92,
    ),
    // src: Thunderfist hp 600, Heavy Mechanical, $980, 21.51 s, heavy war factory; moves with a
    // cannon and deploys for a howitzer. Speed and set-up time (3 s) are placeholders.
    ranked(
        deploying(
            soldier(
                "APA Thunderfist",
                Faction::Apa,
                600,
                Armour::Heavy,
                credits(980),
                centis(2151),
                7,
                3,
                THUNDERFIST_CANNON,
            ),
            HOWITZER,
            30,
        ),
        45,
        80,
    ),
    // src: Toxin Artillery hp 950, $1000, 30.11 s, GLA war factory, anti-ground artillery.
    // Weapon, armour (Light, like the Scorpion) and speed are placeholders; it fires on the move.
    ranked(
        soldier(
            "GLA Toxin Artillery",
            Faction::Gla,
            950,
            Armour::Light,
            credits(1000),
            centis(3011),
            7,
            3,
            TOXIN_SHELL,
        ),
        105,
        95,
    ),
    // Placeholder throughout: no EU artillery card is archived. Fires on the move.
    ranked(soldier("EU Howitzer", Faction::Eu, 550, Armour::Heavy, credits(950), secs(24), 7, 3, EU_SHELL), 39, 93),
    // ---- Anti-armour infantry and fast vehicles ----------------------------------------------
    // src: Rocket Squad hp 180, $300, 16.29 s, EU barracks, anti-armour. Weapon and speed:
    // `ROCKETS`, a placeholder. The alpha's squad also fires up at aircraft, reaching 40 there
    // (its "Anti-air Attack" spec) against 30 on the ground.
    ranked(
        soldier(
            "EU Rocket Squad",
            Faction::Eu,
            180,
            Armour::Infantry,
            credits(300),
            centis(1629),
            8,
            2,
            anti_air(ROCKETS, 40),
        ),
        18,
        93,
    ),
    // src: Tank Hunter, APA barracks, anti-armour rocket infantry; the card has no numbers.
    // Placeholder: the EU Rocket Squad's.
    ranked(
        soldier("APA Tank Hunter", Faction::Apa, 180, Armour::Infantry, credits(300), centis(1629), 8, 2, ROCKETS),
        18,
        91,
    ),
    // Placeholder throughout: no GLA anti-armour infantry is archived. A Rabble-priced squad
    // with a shorter-ranged rocket, so the GLA has an answer to armour from its barracks. The
    // alpha's RPG targets ground and air alike (`TargetType` 0), at the same reach.
    ranked(
        soldier(
            "GLA RPG Squad",
            Faction::Gla,
            150,
            Armour::Infantry,
            credits(250),
            secs(14),
            9,
            2,
            anti_air(weapon(40, [0, 0, 60, 0], 26, 20), 26),
        ),
        14,
        93,
    ),
    // src: Armored Personnel Carrier hp 390, Light, $435, 14.66 s, EU war factory; machine gun
    // 46, cooldown 2.25, range 45; land speed 19. Transport and repair turret are not modelled.
    ranked(
        soldier(
            "EU APC",
            Faction::Eu,
            390,
            Armour::Light,
            credits(435),
            centis(1466),
            19,
            3,
            weapon(46, [0, 0, 0, 0], 45, 23),
        ),
        54,
        93,
    ),
    // src: Gatling Tank hp 490, $660, 23.3 s, APA war factory, light armour, anti-infantry
    // (anti-air too; there is no air). Gun and speed are placeholders: rapid, strong against
    // infantry.
    ranked(
        soldier(
            "APA Gatling Tank",
            Faction::Apa,
            490,
            Armour::Light,
            credits(660),
            centis(2330),
            12,
            3,
            // The alpha's Gatling tank turns a second gun skywards, reaching 50 there.
            anti_air(weapon(10, [12, 0, 0, 0], 32, 4), 50),
        ),
        60,
        90,
    ),
    // src: Technical, GLA light utility vehicle with a 12.7 mm machine gun; the card has no
    // numbers. Placeholder: the cheapest, fastest vehicle in the game, fragile.
    ranked(
        soldier(
            "GLA Technical",
            Faction::Gla,
            280,
            Armour::Light,
            credits(400),
            secs(10),
            17,
            3,
            weapon(14, [6, 0, 0, 0], 30, 6),
        ),
        23,
        93,
    ),
    // ---- Tech buildings ----------------------------------------------------------------------
    // src: the alpha's blueprints: EU tech lab (es_techstructurea) hp 750, $750, 20 s, 30 power;
    // advanced tech lab (es_techstructureb) hp 750, $1000, 25 s, 30 power; GLA salvage bay
    // (gs_techstructureb) hp 750, $800, 20 s.
    researching(
        building("EU Tech Lab", Faction::Eu, BuildingRole::Lab, -30, 750, credits(750), 20, 6, NONE),
        &[HIGHTECH_ALLOYS, IMPROVED_BALLISTICS, SENIOR_WARFIGHTER],
    ),
    researching(
        building("EU Advanced Tech Lab", Faction::Eu, BuildingRole::Lab, -30, 750, credits(1000), 25, 6, NONE),
        &[HIGH_EXPLOSIVES],
    ),
    researching(
        building("GLA Salvage Bay", Faction::Gla, BuildingRole::Lab, 0, 750, credits(800), 20, 6, NONE),
        &[SCRAP_REPAIR],
    ),
    // ---- Aircraft ----------------------------------------------------------------------------
    // src: the alpha's airfields (es_airfield, as_airfield): hp 750, $1200, 34 s, 30 power. The
    // GLA has none; its helicopter comes from its war factory (card: Toxin Chopper "produced:
    // GLA war factory").
    building("EU Airfield", Faction::Eu, FACTORY, -30, 750, credits(1200), 34, 9, &[EU_VALKYRIE]),
    building("APA Airfield", Faction::Apa, FACTORY, -30, 750, credits(1200), 34, 9, &[APA_MANTIS]),
    // src: the alpha's helicopters, `RtsMovementComponentData` and weapon specs. EU heavy rocket
    // helo (the Valkyrie; card hp 621, $1250, 32.58 s): speed 12, rockets 120 every 6 s at 60,
    // ground only. APA light gun helo (the Mantis; card hp 322): speed 16, machine gun 42 (+28
    // anti-armour) every 1.125 s at 35, ground only, $950, 24 s. GLA light chemical helo (the
    // Toxin Chopper): speed 19, toxin missiles 26 (+12 vs infantry) every 1.125 s at 30, ground
    // and air, hp 220, $750, 24 s. A card's number where it gives one, else the alpha's. No
    // source gives an armour class: Light. Ranks: rank-1 health +90 / +39 / +27 and a rank-3
    // interval of 93% (the alpha's -0.4 / -0.075 / -0.075 s).
    ranked(
        flying(soldier(
            "EU Valkyrie",
            Faction::Eu,
            621,
            Armour::Light,
            credits(1250),
            centis(3258),
            12,
            3,
            weapon(120, [0, 0, 0, 0], 60, 60),
        )),
        90,
        93,
    ),
    ranked(
        flying(soldier(
            "APA Mantis",
            Faction::Apa,
            322,
            Armour::Light,
            credits(950),
            secs(24),
            16,
            3,
            weapon(42, [0, 0, 28, 0], 35, 11),
        )),
        39,
        93,
    ),
    ranked(
        flying(soldier(
            "GLA Toxin Chopper",
            Faction::Gla,
            220,
            Armour::Light,
            credits(750),
            secs(24),
            19,
            3,
            anti_air(weapon(26, [12, 0, 0, 0], 30, 11), 30),
        )),
        27,
        93,
    ),
];

/// Workers carry less per trip; the GLA compensates with a higher collector cap.
const HARVEST_WORKER: CollectorSpec = CollectorSpec { capacity: 40, load_ticks: secs(2), unload_ticks: secs(1) };

pub static RULES: Rules = Rules { defs: &DEFS, generals: &GENERALS, upgrades: &UPGRADES };

/// Zero Hour's player weapon upgrades are all +25% damage (96 of 97 `PLAYER_UPGRADE` bonuses in
/// its `Weapon.ini`, e.g. AP Bullets, Uranium Shells).
const ZH_DAMAGE: i32 = 125;

const fn upgrade(name: &'static str, cost: i64, research_secs: u32, effects: &'static [(DefId, Effect)]) -> Upgrade {
    Upgrade { name, cost: credits(cost), research_ticks: secs(research_secs), effects }
}

/// Every upgrade's research site, cost and time come from the alpha's production data
/// (`RtsProductionComponentData.ResearchableUpgrades`) and which units it touches from the
/// units' `RtsUpgradeableData`. What it does comes from the archived cards; how much, where
/// neither says, from Zero Hour 1.04's INI data for the matching upgrade.
static UPGRADES: [Upgrade; 9] = [
    // src: alpha eu_structarmor, tech lab, $750, 25 s; card: "improves the armor of EU
    // buildings". Zero Hour's Fortified Structure: structures take 75% damage
    // (`GLAUpgradedStructureArmor`, DEFAULT 75% vs 100%).
    upgrade(
        "Hightech Alloys",
        750,
        25,
        &[
            (EU_CC, Effect::Toughness { percent: 75 }),
            (EU_SUPPLY, Effect::Toughness { percent: 75 }),
            (EU_POWER, Effect::Toughness { percent: 75 }),
            (EU_DERRICK, Effect::Toughness { percent: 75 }),
            (EU_BARRACKS, Effect::Toughness { percent: 75 }),
            (EU_FACTORY, Effect::Toughness { percent: 75 }),
            (EU_TECH_LAB, Effect::Toughness { percent: 75 }),
            (EU_ADVANCED_LAB, Effect::Toughness { percent: 75 }),
        ],
    ),
    // src: alpha eu_infattack, tech lab, $1000, 25 s, on rifle and rocket squads; card: "increases
    // the attack damage". Zero Hour: +25%.
    upgrade(
        "Improved Ballistics",
        1000,
        25,
        &[
            (EU_RECON, Effect::Damage { percent: ZH_DAMAGE, against: None }),
            (EU_ROCKETS, Effect::Damage { percent: ZH_DAMAGE, against: None }),
        ],
    ),
    // src: alpha eu_infarmor, tech lab, $1000, 25 s, on rifle and rocket squads; card: "improves
    // the armor". Zero Hour's general armour upgrade (Fortified Structure, as Hightech Alloys):
    // 75% damage taken. Its infantry armour upgrades only resist poison, which isn't modelled.
    upgrade(
        "Senior Warfighter",
        1000,
        25,
        &[(EU_RECON, Effect::Toughness { percent: 75 }), (EU_ROCKETS, Effect::Toughness { percent: 75 })],
    ),
    // src: alpha eu_vehdmg ("Advanced Ordinance"), advanced tech lab, $1250, 25 s, on the
    // tank, rocket vehicle and APC; card "High Explosives": "increases damage of all vehicles".
    // Zero Hour: +25%. The EU Howitzer stands in for the rocket vehicle.
    upgrade(
        "High Explosives",
        1250,
        25,
        &[
            (EU_AEGIS, Effect::Damage { percent: ZH_DAMAGE, against: None }),
            (EU_ARTILLERY, Effect::Damage { percent: ZH_DAMAGE, against: None }),
            (EU_APC, Effect::Damage { percent: ZH_DAMAGE, against: None }),
        ],
    ),
    // src: alpha eu_rifleammo, EU barracks, $750, 22 s, on the rifle squad; card: "increases the
    // attack damage dealt to enemy infantry". Zero Hour: +25%, against infantry only.
    upgrade(
        "Hollow Point Ammo",
        750,
        22,
        &[(EU_RECON, Effect::Damage { percent: ZH_DAMAGE, against: Some(Armour::Infantry) })],
    ),
    // src: alpha apa_populationcontrolupgrade, APA barracks, $750, 15 s, on rifle and rocket
    // squads; card: "adds an additional infantryman to all squads". Red Guard squads are 3,
    // Tank Hunters 2 (cards): one squad entity gains a man's share of hit points and fire,
    // 4/3 and 3/2.
    upgrade(
        "Population Control",
        750,
        15,
        &[
            (APA_RED_GUARD, Effect::Health { add: 60 }),
            (APA_RED_GUARD, Effect::Damage { percent: 133, against: None }),
            (APA_TANK_HUNTER, Effect::Health { add: 90 }),
            (APA_TANK_HUNTER, Effect::Damage { percent: 150, against: None }),
        ],
    ),
    // src: alpha gla_shortenedfusesupgrade, GLA barracks, $750, 20 s, on the rocket squad; card
    // (Insurgent): "make ... fire faster". Zero Hour's fire-rate bonuses are +25% (Nationalism,
    // `RATE_OF_FIRE 125%`): 80% of the time between shots.
    upgrade("Shortened Fuses", 750, 20, &[(GLA_RPG, Effect::Cooldown { percent: 80 })]),
    // src: alpha gla_scraprepair, salvage bay, $1000, 22 s, on the Scorpion, Toxin Artillery and
    // Technical; card: "allows GLA vehicles to self-repair when not in combat". Zero Hour's Junk
    // Repair: 2 hp a second (`AutoHealBehavior`), here after 3 s without a hit (`BaseRegenDelay`).
    upgrade(
        "Scrap Repair",
        1000,
        22,
        &[
            (GLA_TANK, Effect::Repair { hp: 2, calm_ticks: secs(3) }),
            (GLA_TOXIN_ARTILLERY, Effect::Repair { hp: 2, calm_ticks: secs(3) }),
            (GLA_TECHNICAL, Effect::Repair { hp: 2, calm_ticks: secs(3) }),
        ],
    ),
    // src: alpha apa_firestormshells, $750, 45 s, on the Thunderfist; card: "equips the Thunderfist
    // with firestorm artillery shells". The fire left behind isn't modelled; Zero Hour's Black
    // Napalm (the Inferno cannon's upgrade) raises its fire field's damage by half (5 to 7.5),
    // so the howitzer's shells do 150%.
    upgrade("Firestorm Shells", 750, 45, &[(APA_THUNDERFIST, Effect::ShellDamage { percent: 150 })]),
];

/// One general per faction for now, to exercise generals-as-data. The names are documented G2
/// generals; what each changed is not, so these economy modifiers are
/// placeholders in the spirit of pre-release sub-general designs (e.g. a supply doctrine:
/// cheaper supply trucks, faster builds). Each has one active power, modelled on support
/// powers the archive documents for other generals of the same side (numbers placeholders).
static GENERALS: [General; 3] = [
    General {
        name: "Bulldog",
        faction: Faction::Eu,
        modifiers: &[
            Modifier::Cost { def: EU_HARVESTER, percent: 80 },
            Modifier::BuildTime { def: EU_HARVESTER, percent: 85 },
            Modifier::Cost { def: EU_POWER, percent: 120 },
        ],
        // After the EU general Salvo's "Suppressing fire" (an artillery strike called onto the
        // field): seven shells on and around the point, half a second apart after a 3 s delay.
        // Placeholder numbers. No bonus against buildings: with one it took half a command
        // center per use, a base-killer rather than a strike on an army.
        power: Some(Power {
            name: "Artillery barrage",
            kind: PowerKind::Barrage {
                shells: 7,
                delay_ticks: secs(3),
                every_ticks: g2_sim::TICKS_PER_SECOND / 2,
                damage: 90,
                bonus: [0, 20, 0, 0],
                radius: Fx::from_int(7),
            },
            first_ticks: secs(240),
            cooldown_ticks: secs(240),
        }),
    },
    General {
        name: "Taskmaster",
        faction: Faction::Apa,
        modifiers: &[
            Modifier::BuildTime { def: APA_SUPPLY, percent: 75 },
            Modifier::BuildTime { def: APA_DERRICK, percent: 75 },
            Modifier::Cost { def: APA_HARVESTER, percent: 115 },
        ],
        // A field repair: the support powers' "Supply drop" heals nearby units. Every own unit
        // and building within 30 regains up to 400 hp.
        power: Some(Power {
            name: "Field repair",
            kind: PowerKind::Repair { radius: Fx::from_int(30), hp: 400 },
            first_ticks: secs(180),
            cooldown_ticks: secs(180),
        }),
    },
    General {
        name: "Junkyard",
        faction: Faction::Gla,
        modifiers: &[
            Modifier::Cost { def: GLA_WORKER, percent: 75 },
            Modifier::BuildTime { def: GLA_STASH, percent: 125 },
        ],
        // After the GLA general Muerta's "Ambush" (rebel soldiers from hidden tunnels): four
        // RPG squads appear at the point.
        power: Some(Power {
            name: "Ambush",
            kind: PowerKind::Ambush { def: GLA_RPG, count: 4 },
            first_ticks: secs(240),
            cooldown_ticks: secs(240),
        }),
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
        decks: Vec::new(),
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
            (APA_THUNDERFIST, "APA Thunderfist"),
            (GLA_TOXIN_ARTILLERY, "GLA Toxin Artillery"),
            (EU_ARTILLERY, "EU Howitzer"),
            (EU_ROCKETS, "EU Rocket Squad"),
            (APA_TANK_HUNTER, "APA Tank Hunter"),
            (GLA_RPG, "GLA RPG Squad"),
            (EU_APC, "EU APC"),
            (APA_GATLING, "APA Gatling Tank"),
            (GLA_TECHNICAL, "GLA Technical"),
            (EU_TECH_LAB, "EU Tech Lab"),
            (EU_ADVANCED_LAB, "EU Advanced Tech Lab"),
            (GLA_SALVAGE_BAY, "GLA Salvage Bay"),
            (EU_AIRFIELD, "EU Airfield"),
            (APA_AIRFIELD, "APA Airfield"),
            (EU_VALKYRIE, "EU Valkyrie"),
            (APA_MANTIS, "APA Mantis"),
            (GLA_TOXIN_CHOPPER, "GLA Toxin Chopper"),
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

    /// Every faction's barracks has a rifle squad and an anti-armour squad, and the rockets
    /// beat the rifles against tanks per credit spent (damage per second against `Heavy`); the
    /// war factory has a fast vehicle that outruns every tank and artillery piece of its side
    /// (aircraft aside).
    #[test]
    fn each_faction_has_anti_armour_infantry_and_a_fast_vehicle() {
        let r = rules();
        let dps_per_credit = |d: DefId, armour| {
            let w = r.def(d).weapon().unwrap();
            w.damage_vs(armour) as f64 * g2_sim::TICKS_PER_SECOND as f64
                / w.cooldown_ticks as f64
                / r.def(d).cost.credits as f64
        };
        for (barracks, rifles, rockets, factory, fast) in [
            (EU_BARRACKS, EU_RECON, EU_ROCKETS, EU_FACTORY, EU_APC),
            (APA_BARRACKS, APA_RED_GUARD, APA_TANK_HUNTER, APA_FACTORY, APA_GATLING),
            (GLA_BARRACKS, GLA_RABBLE, GLA_RPG, GLA_FACTORY, GLA_TECHNICAL),
        ] {
            assert_eq!(r.def(barracks).produces, &[rifles, rockets][..]);
            assert!(r.def(factory).produces.contains(&fast));
            assert!(
                dps_per_credit(rockets, Armour::Heavy) > dps_per_credit(rifles, Armour::Heavy),
                "{} vs tanks",
                r.def(rockets).name
            );
            assert!(
                dps_per_credit(rifles, Armour::Infantry) > dps_per_credit(rockets, Armour::Infantry),
                "{} vs infantry",
                r.def(rifles).name
            );
            for &other in r.def(factory).produces.iter().filter(|&&d| d != fast && !r.def(d).flies()) {
                assert!(r.def(fast).speed() > r.def(other).speed(), "{} vs {}", r.def(fast).name, r.def(other).name);
            }
        }
    }

    /// Every faction can build an aircraft and has a trainable unit that fires at aircraft, so
    /// no helicopter is out of every enemy's reach; shells never hit aircraft.
    #[test]
    fn every_faction_has_aircraft_and_anti_air() {
        let r = rules();
        let trainable = |f: Faction| {
            r.defs
                .iter()
                .filter(move |d| d.faction == Some(f) && d.is_building())
                .flat_map(|b| b.produces.iter().map(|&u| r.def(u)))
        };
        for f in Faction::ALL {
            assert!(trainable(f).any(|d| d.flies()), "{f:?} has no aircraft");
            let aa: Vec<_> = trainable(f)
                .filter(|d| d.weapon().is_some_and(|w| w.range_vs(true).is_some()))
                .map(|d| d.name)
                .collect();
            assert!(!aa.is_empty(), "{f:?} has nothing that fires at aircraft");
            for d in trainable(f).filter_map(|d| d.weapon().or(d.deploy.map(|x| x.weapon))) {
                assert!(d.shell.is_none() || d.range_vs(true).is_none(), "a shell weapon targets the air");
            }
        }
    }
}
