//! Definitions for the three factions and the built-in maps.
//!
//! Values marked `// src:` come from archived reference material, documented in notes kept
//! outside this repository.
//! Everything else is a placeholder chosen for playability and will be tuned.
use g2_sim::defs::{
    Armour, BuildingRole, Class, CollectorSpec, Cost, Def, DefId, Deploy, Effect, Faction, Garrison, General,
    GeneralUnit, Modifier, Perk, PerkId, Power, PowerKind, PowerTuning, Rules, Shell, Targets, Upgrade, UpgradeId,
    Veterancy, Weapon,
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
    pub const SHANTY: DefId = DefId(47);
    pub const TOWN_HOUSE: DefId = DefId(48);
    pub const URBAN_BLOCK: DefId = DefId(49);
    pub const EU_GUN_TURRET: DefId = DefId(50);
    pub const APA_FLAME_TURRET: DefId = DefId(51);
    pub const GLA_ROCKET_SITE: DefId = DefId(52);
    pub const EU_BOMBER: DefId = DefId(53);
    pub const APA_BOMBER: DefId = DefId(54);
    pub const APA_FIGHTER: DefId = DefId(55);
    pub const GLA_SCUD_STORM: DefId = DefId(56);
    pub const EU_FOX: DefId = DefId(57);
    pub const EU_LONGBOW: DefId = DefId(58);
    pub const EU_SNIPERS: DefId = DefId(59);
    pub const EU_ASSAULT_APC: DefId = DefId(60);
    pub const EU_SHOCK_TROOPERS: DefId = DefId(61);
    pub const EU_LASER_TANK: DefId = DefId(62);
    pub const EU_RAILGUN_TANK: DefId = DefId(63);
    pub const EU_RAILGUN_TURRET: DefId = DefId(64);
    pub const APA_INFERNO: DefId = DefId(65);
    pub const APA_NAPALM_MORTAR: DefId = DefId(66);
    pub const APA_DRAGON: DefId = DefId(67);
    pub const APA_ROCKET_TANK: DefId = DefId(68);
    pub const APA_SABOTEURS: DefId = DefId(69);
    pub const APA_SUNSTORM: DefId = DefId(70);
    pub const APA_SHADOW_TANK: DefId = DefId(71);
    pub const APA_URBAN_ASSAULT: DefId = DefId(72);
    pub const GLA_BATTLE_BUS: DefId = DefId(73);
    pub const GLA_BLACK_MARKET_ARTILLERY: DefId = DefId(74);
    pub const GLA_SHRAPNEL_TANK: DefId = DefId(75);
    pub const GLA_TOXIN_TANK: DefId = DefId(76);
    pub const GLA_MOBILE_CHEMIST: DefId = DefId(77);
    pub const GLA_BOMB_TRUCK: DefId = DefId(78);
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
    pub const DAEDALUS: GeneralId = GeneralId(3);
    pub const MERCURY_RED: GeneralId = GeneralId(4);
    pub const CUTTING_EDGE: GeneralId = GeneralId(5);
    pub const CLEAVER: GeneralId = GeneralId(6);
    pub const INFERNO: GeneralId = GeneralId(7);
    pub const RED_ARROW: GeneralId = GeneralId(8);
    pub const THOUSAND_SUNS: GeneralId = GeneralId(9);
    pub const BLACKJACK: GeneralId = GeneralId(10);
    pub const THRAX: GeneralId = GeneralId(11);
    pub const WILD_DOG: GeneralId = GeneralId(12);
    pub const KITBASH: GeneralId = GeneralId(13);
}

/// Stable ids into `RULES.perks`: eight per general, in each general's list order.
pub mod perks {
    use g2_sim::defs::PerkId;
    // Bulldog (EU).
    pub const BD_DOUBLE_SHIFTS: PerkId = PerkId(0);
    pub const BD_HEAVY_PAYLOAD: PerkId = PerkId(1);
    pub const BD_PROTECTOR_I: PerkId = PerkId(2);
    pub const BD_PROTECTOR_II: PerkId = PerkId(3);
    pub const BD_FACTORY_OVERTIME: PerkId = PerkId(4);
    pub const BD_INSOURCED: PerkId = PerkId(5);
    pub const BD_AERODYNAMICS_I: PerkId = PerkId(6);
    pub const BD_AERODYNAMICS_II: PerkId = PerkId(7);
    // Taskmaster (APA).
    pub const TM_SPY_EFFICIENCY: PerkId = PerkId(8);
    pub const TM_LOGISTICS: PerkId = PerkId(9);
    pub const TM_PROTECTOR_I: PerkId = PerkId(10);
    pub const TM_PROTECTOR_II: PerkId = PerkId(11);
    pub const TM_FACTORY_OVERTIME: PerkId = PerkId(12);
    pub const TM_VULCAN_BARRELS: PerkId = PerkId(13);
    pub const TM_KANBAN: PerkId = PerkId(14);
    pub const TM_COMMAND_I: PerkId = PerkId(15);
    // Junkyard (GLA).
    pub const JY_DISCIPLINE: PerkId = PerkId(16);
    pub const JY_SCAVENGER: PerkId = PerkId(17);
    pub const JY_LEVERAGE: PerkId = PerkId(18);
    pub const JY_INSOURCED: PerkId = PerkId(19);
    pub const JY_PROTECTOR_I: PerkId = PerkId(20);
    pub const JY_PROTECTOR_II: PerkId = PerkId(21);
    pub const JY_FACTORY_OVERTIME: PerkId = PerkId(22);
    pub const JY_LEADERSHIP: PerkId = PerkId(23);
    // Daedalus (EU).
    pub const DD_TAILWIND: PerkId = PerkId(24);
    pub const DD_HEAVY_BOMBS: PerkId = PerkId(25);
    pub const DD_PROTECTOR_I: PerkId = PerkId(26);
    pub const DD_PROTECTOR_II: PerkId = PerkId(27);
    pub const DD_KANBAN: PerkId = PerkId(28);
    pub const DD_AERODYNAMICS_I: PerkId = PerkId(29);
    pub const DD_AERODYNAMICS_II: PerkId = PerkId(30);
    pub const DD_INSOURCED: PerkId = PerkId(31);
    // Mercury Red (EU).
    pub const MR_DOUBLE_TIME: PerkId = PerkId(32);
    pub const MR_FIELD_MEDICS: PerkId = PerkId(33);
    pub const MR_PROTECTOR_I: PerkId = PerkId(34);
    pub const MR_PROTECTOR_II: PerkId = PerkId(35);
    pub const MR_RAPID_DEPLOYMENT: PerkId = PerkId(36);
    pub const MR_FACTORY_OVERTIME: PerkId = PerkId(37);
    pub const MR_INSOURCED: PerkId = PerkId(38);
    pub const MR_LEADERSHIP: PerkId = PerkId(39);
    // Cutting Edge (EU).
    pub const CE_CAPACITORS: PerkId = PerkId(40);
    pub const CE_FOCUSING: PerkId = PerkId(41);
    pub const CE_PROTECTOR_I: PerkId = PerkId(42);
    pub const CE_PROTECTOR_II: PerkId = PerkId(43);
    pub const CE_FACTORY_OVERTIME: PerkId = PerkId(44);
    pub const CE_REINFORCED: PerkId = PerkId(45);
    pub const CE_INSOURCED: PerkId = PerkId(46);
    pub const CE_OVERCHARGE: PerkId = PerkId(47);
    // Cleaver (EU).
    pub const CL_COOLANT: PerkId = PerkId(48);
    pub const CL_PLASMA_CORE: PerkId = PerkId(49);
    pub const CL_PROTECTOR_I: PerkId = PerkId(50);
    pub const CL_PROTECTOR_II: PerkId = PerkId(51);
    pub const CL_FACTORY_OVERTIME: PerkId = PerkId(52);
    pub const CL_INSOURCED: PerkId = PerkId(53);
    pub const CL_HEAVY_PLATING: PerkId = PerkId(54);
    pub const CL_SLUGS: PerkId = PerkId(55);
    // Inferno (APA).
    pub const IN_DOUBLE_SHIFTS: PerkId = PerkId(56);
    pub const IN_INFERNAL_DIESEL: PerkId = PerkId(57);
    pub const IN_PROTECTOR_I: PerkId = PerkId(58);
    pub const IN_PROTECTOR_II: PerkId = PerkId(59);
    pub const IN_FACTORY_OVERTIME: PerkId = PerkId(60);
    pub const IN_IGNITION: PerkId = PerkId(61);
    pub const IN_INSOURCED: PerkId = PerkId(62);
    pub const IN_LEADERSHIP: PerkId = PerkId(63);
    // Red Arrow (APA).
    pub const RA_SALVO_TIMING: PerkId = PerkId(64);
    pub const RA_WARHEADS: PerkId = PerkId(65);
    pub const RA_PROTECTOR_I: PerkId = PerkId(66);
    pub const RA_PROTECTOR_II: PerkId = PerkId(67);
    pub const RA_TANK_HUNTERS: PerkId = PerkId(68);
    pub const RA_FACTORY_OVERTIME: PerkId = PerkId(69);
    pub const RA_INSOURCED: PerkId = PerkId(70);
    pub const RA_GUIDANCE: PerkId = PerkId(71);
    // 1000 Suns (APA).
    pub const TS_FISSION: PerkId = PerkId(72);
    pub const TS_ENRICHED: PerkId = PerkId(73);
    pub const TS_PROTECTOR_I: PerkId = PerkId(74);
    pub const TS_PROTECTOR_II: PerkId = PerkId(75);
    pub const TS_FACTORY_OVERTIME: PerkId = PerkId(76);
    pub const TS_URANIUM: PerkId = PerkId(77);
    pub const TS_INSOURCED: PerkId = PerkId(78);
    pub const TS_LEADERSHIP: PerkId = PerkId(79);
    // Blackjack (APA).
    pub const BJ_QUICK_HANDS: PerkId = PerkId(80);
    pub const BJ_DEEP_COVER: PerkId = PerkId(81);
    pub const BJ_PROTECTOR_I: PerkId = PerkId(82);
    pub const BJ_PROTECTOR_II: PerkId = PerkId(83);
    pub const BJ_POPULATION: PerkId = PerkId(84);
    pub const BJ_FACTORY_OVERTIME: PerkId = PerkId(85);
    pub const BJ_STICKY: PerkId = PerkId(86);
    pub const BJ_LEADERSHIP: PerkId = PerkId(87);
    // Thrax (GLA).
    pub const TX_CATALYST: PerkId = PerkId(88);
    pub const TX_CONCENTRATE: PerkId = PerkId(89);
    pub const TX_PROTECTOR_I: PerkId = PerkId(90);
    pub const TX_PROTECTOR_II: PerkId = PerkId(91);
    pub const TX_FACTORY_OVERTIME: PerkId = PerkId(92);
    pub const TX_ANTHRAX: PerkId = PerkId(93);
    pub const TX_INSOURCED: PerkId = PerkId(94);
    pub const TX_LEADERSHIP: PerkId = PerkId(95);
    // Wild Dog (GLA).
    pub const WD_QUICK_RELOAD: PerkId = PerkId(96);
    pub const WD_SHAPED_CHARGES: PerkId = PerkId(97);
    pub const WD_PROTECTOR_I: PerkId = PerkId(98);
    pub const WD_PROTECTOR_II: PerkId = PerkId(99);
    pub const WD_FACTORY_OVERTIME: PerkId = PerkId(100);
    pub const WD_JUNK_ARMOR: PerkId = PerkId(101);
    pub const WD_INSOURCED: PerkId = PerkId(102);
    pub const WD_LEADERSHIP: PerkId = PerkId(103);
    // Kitbash (GLA).
    pub const KB_SCRAP_PARTS: PerkId = PerkId(104);
    pub const KB_LONG_FIELD: PerkId = PerkId(105);
    pub const KB_PROTECTOR_I: PerkId = PerkId(106);
    pub const KB_PROTECTOR_II: PerkId = PerkId(107);
    pub const KB_FACTORY_OVERTIME: PerkId = PerkId(108);
    pub const KB_JUNK_REPAIR: PerkId = PerkId(109);
    pub const KB_INSOURCED: PerkId = PerkId(110);
    pub const KB_LEADERSHIP: PerkId = PerkId(111);
}
use ids::*;
use perks::*;
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
        class: Class::Building {
            power,
            power_radius: Fx::from_int(if power > 0 { 60 } else { 0 }),
            role,
            weapon: None,
        },
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
        garrison: None,
        stealth: false,
        detection: Fx::ZERO,
    }
}

/// A base defence firing `weapon`, revealing enemy stealth within `detection` of its centre.
#[allow(clippy::too_many_arguments)]
const fn defense(
    name: &'static str,
    faction: Faction,
    power: i32,
    hp: i32,
    cost: Cost,
    build_secs: u32,
    radius: i64,
    weapon: Weapon,
    detection: i64,
) -> Def {
    let mut d = building(name, faction, BuildingRole::Defense, power, hp, cost, build_secs, radius, NONE);
    d.class = Class::Building { power, power_radius: Fx::ZERO, role: BuildingRole::Defense, weapon: Some(weapon) };
    d.detection = Fx::from_int(detection);
    d
}

/// `d` is hidden from enemies without a detector in range (the alpha's passive stealth).
const fn stealthy(mut d: Def) -> Def {
    d.stealth = true;
    d
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
        garrison: None,
        stealth: false,
        detection: Fx::ZERO,
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
        garrison: None,
        stealth: false,
        detection: Fx::ZERO,
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
        garrison_bonus: 0,
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

/// `w` deals `bonus` more to a building with units inside (the card's "vs. garrisoned
/// buildings").
const fn garrison_buster(mut w: Weapon, bonus: i32) -> Weapon {
    w.garrison_bonus = bonus;
    w
}

/// `d` carries up to `capacity` infantry that can't fire from inside (the alpha's transports).
const fn carrying(mut d: Def, capacity: u32) -> Def {
    d.garrison = Some(Garrison { capacity, fires: false, range_bonus: Fx::ZERO });
    d
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
// cooldown 4, range 65. Blast radius 6, shell speed 40/s and minimum range 20: placeholders,
// after the way Zero Hour's artillery (Inferno cannon: radius 15, minimum range 50 of 300) is
// shaped.
const HOWITZER: Weapon = garrison_buster(shelling(weapon(100, [0, 30, 0, 0], 65, 40), 6, 40, 20), 100);
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

static DEFS: [Def; 79] = [
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
        garrison: None,
        stealth: false,
        detection: Fx::ZERO,
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
        garrison: None,
        stealth: false,
        detection: Fx::ZERO,
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
            &[
                EU_SUPPLY,
                EU_POWER,
                EU_DERRICK,
                EU_BARRACKS,
                EU_FACTORY,
                EU_TECH_LAB,
                EU_ADVANCED_LAB,
                EU_AIRFIELD,
                EU_GUN_TURRET,
            ],
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
        &[APA_SUPPLY, APA_POWER, APA_DERRICK, APA_BARRACKS, APA_FACTORY, APA_AIRFIELD, APA_FLAME_TURRET],
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
        &[GLA_STASH, GLA_DERRICK, GLA_BARRACKS, GLA_FACTORY, GLA_SALVAGE_BAY, GLA_ROCKET_SITE, GLA_SCUD_STORM],
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
    // src: stealthed, the alpha's passive `GLA_RocketSoldier_Stealth`, unlocked from the start.
    stealthy(ranked(
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
    )),
    // src: Armored Personnel Carrier hp 390, Light, $435, 14.66 s, EU war factory; machine gun
    // 46, cooldown 2.25, range 45; land speed 19. Carries two infantry squads who can't fire
    // from inside (the alpha's `RtsGarrisonComponentData`: capacity 2, no attack from garrison).
    // The repair turret is not modelled.
    carrying(
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
        2,
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
    // ---- Civilian buildings -----------------------------------------------------------------
    // src: the alpha's civilian buildings (afri_cb_*), by their display name: shanty (hp 300,
    // room for 1; 600, 2; 800, 3), town house (600, 2; 800, 3; 1200, 4) and urban block (1200
    // and 1700, 4), all firing out with an 8-unit range bonus. One row per size class, each the
    // class's middle size. Footprints: the map's buildings (from the original Generals / Zero
    // Hour maps) take the largest class that fits inside their own footprint; smaller ones
    // stay scenery.
    civilian("Shanty", 600, 2, 2),
    civilian("Town House", 800, 3, 3),
    civilian("Urban Block", 1200, 4, 5),
    // ---- Base defences ------------------------------------------------------------------
    // src: the alpha's base defences, each the first ground turret its builder lists: EU
    // es_basedefensegun (autocannon 26, +8 vs. infantry, every 0.8 s, range 57, ground only;
    // hp 450, $450, 18 s, 20 power; detects stealth within 50), APA as_basedefenseflame
    // (flamethrower 44, +20 vs. infantry and +60 vs. garrisoned buildings, every 2 s, range 45;
    // hp 500, $500, 18 s, 20 power; detects within 40), GLA gs_basedefenserocket, the wiki's
    // Rocket Site (RPG 40, +28 vs. heavy, every 2.3 s, range 47 against the ground and 57
    // against aircraft, where the alpha fires a heavier second rocket; hp 400, $300, 18 s;
    // detects within 45). Ranges count from the edge, as a garrison's do. The EU and APA names
    // and every footprint are placeholders.
    defense("EU Gun Turret", Faction::Eu, -20, 450, credits(450), 18, 3, weapon(26, [8, 0, 0, 0], 57, 8), 50),
    defense(
        "APA Flame Turret",
        Faction::Apa,
        -20,
        500,
        credits(500),
        18,
        3,
        garrison_buster(weapon(44, [20, 0, 0, 0], 45, 20), 60),
        40,
    ),
    defense(
        "GLA Rocket Site",
        Faction::Gla,
        0,
        400,
        credits(300),
        18,
        4,
        anti_air(weapon(40, [0, 0, 28, 0], 47, 23), 57),
        45,
    ),
    // ---- Support jets --------------------------------------------------------------------
    // Delivery aircraft for the factions' support powers: never built or ordered, they fly one
    // pass and leave (the wiki: "It cannot be built nor directly controlled in-game"). src: the
    // alpha's eu_airbomber (MaxHealth 300, ArmorRating 4) for the Air Raid and the Fuel Air
    // Bomb, au_airbomber (400, armour 0) for Carpet Bombing, au_airfighter_spysortie2 (90,
    // armour 0) for Spy Sorties. Armour 4 and 0 map to our Heavy and Light. Speed is a
    // placeholder: fast enough to cross a map in a few seconds, slow enough for anti-air to
    // get a few shots in.
    support_jet("EU Bomber", Faction::Eu, 300, Armour::Heavy),
    support_jet("APA Bomber", Faction::Apa, 400, Armour::Light),
    support_jet("APA Fighter", Faction::Apa, 90, Armour::Light),
    // ---- Superweapons --------------------------------------------------------------------
    // src: the alpha's gs_scudstorm: MaxHealth 1500, $4000, BuildTime 60 s, built by the
    // Worker (the wiki's Dirty Scud Storm); it launches GLA_PlayerAbility_DirtyScudStorm. No
    // power draw (GLA has none). Footprint is a placeholder.
    building("GLA Scud Storm", Faction::Gla, BuildingRole::Superweapon, 0, 1500, credits(4000), 60, 7, NONE),
    // ---- Generals' units ------------------------------------------------------------------
    // Units a general's factories add or swap in (the alpha's `StaticData/Generals/*` `Units`
    // lists). src: each unit's own alpha blueprint: MaxHealth, Cost, BuildTime, the main
    // weapon's DamageRating, DamageTypes bonuses (type 0 vs. infantry, 1 light, 2 heavy, 3
    // structure, 4 garrisoned), Range and RateOfFire, and the veterancy's rank-1 health and
    // rank-3 fire-rate cut. ArmorRating 0-2 is our Infantry for squads and Light for vehicles,
    // 3-5 Light, 6+ Heavy. Speeds are placeholders by class (infantry 9, tanks 10, light
    // vehicles 16, helicopters 12). Wiki names where the archive has them.
    // Fox LRV (eu_lightreconvehicle): hp 300, $450, 12 s; machine gun 16 (+12 inf), 30, 0.98 s;
    // carries one squad (`Capacity` 1, can't fire out).
    carrying(
        ranked(
            soldier(
                "EU Fox LRV",
                Faction::Eu,
                300,
                Armour::Light,
                credits(450),
                secs(12),
                16,
                2,
                weapon(16, [12, 0, 0, 0], 30, 10),
            ),
            54,
            85,
        ),
        1,
    ),
    // Longbow (eu_lightrockethelo): hp 240 (the wiki's card 410), $850, 24 s; rockets 36 (+16
    // light), 30, 1.2 s, ground and air (`TargetType` 0).
    ranked(
        flying(soldier(
            "EU Longbow",
            Faction::Eu,
            240,
            Armour::Light,
            credits(850),
            secs(24),
            12,
            3,
            anti_air(weapon(36, [0, 16, 0, 0], 30, 12), 30),
        )),
        36,
        88,
    ),
    // Sniper Team (eu_elitesnipersoldiersquad): hp 240, $750, 13 s; sniper rifle 25 (+200 vs.
    // infantry), 50, 4 s; stealthed (the wiki's passive Camouflage).
    stealthy(ranked(
        soldier(
            "EU Sniper Team",
            Faction::Eu,
            240,
            Armour::Infantry,
            credits(750),
            secs(13),
            9,
            2,
            weapon(25, [200, 0, 0, 0], 50, 40),
        ),
        14,
        83,
    )),
    // Assault APC (eu_apcassault): hp 440, $600, 13 s; machine gun 28, 32, 1.5 s; carries two.
    carrying(
        ranked(
            soldier(
                "EU Assault APC",
                Faction::Eu,
                440,
                Armour::Light,
                credits(600),
                secs(13),
                17,
                3,
                weapon(28, [0, 0, 0, 0], 32, 15),
            ),
            54,
            85,
        ),
        2,
    ),
    // Shock Troopers (eu_shocktroopersquad): hp 240, $375, 10 s; 23, 25, 1.2 s. Replaces the
    // Recon Squad for its generals.
    ranked(
        soldier(
            "EU Shock Troopers",
            Faction::Eu,
            240,
            Armour::Infantry,
            credits(375),
            secs(10),
            10,
            2,
            weapon(23, [0, 0, 0, 0], 25, 12),
        ),
        21,
        94,
    ),
    // Heavy Assault Laser (eu_heavylasertank): hp 600 (the wiki's 1000), $1200, 22 s; laser
    // 110, 35, 3 s. A direct-fire gun stands in for the beam.
    ranked(
        soldier(
            "EU Heavy Assault Laser",
            Faction::Eu,
            600,
            Armour::Light,
            credits(1200),
            secs(22),
            9,
            3,
            weapon(110, [0, 0, 0, 0], 35, 30),
        ),
        72,
        80,
    ),
    // Railgun Tank (eu_heavyrailguntank_exp): hp 900, $1250, 22 s; railgun 280 (+80 heavy,
    // +120 garrisoned), 50, 4 s.
    ranked(
        soldier(
            "EU Railgun Tank",
            Faction::Eu,
            900,
            Armour::Heavy,
            credits(1250),
            secs(22),
            8,
            4,
            garrison_buster(weapon(280, [0, 0, 80, 0], 50, 40), 120),
        ),
        72,
        80,
    ),
    // Railgun turret (es_basedefenserailgun): hp 500, $575, 18 s, 20 power; railgun 100 (+100
    // garrisoned), 50, 2.5 s, ground only; no stealth detection (the alpha's range 0). Replaces
    // the Gun Turret for its generals.
    defense(
        "EU Railgun Turret",
        Faction::Eu,
        -20,
        500,
        credits(575),
        18,
        3,
        garrison_buster(weapon(100, [0, 0, 0, 0], 50, 25), 100),
        0,
    ),
    // Inferno Squad (au_eliteflamesoldiersquad): hp 330, $500, 13 s; flamethrower 50 (+65
    // inf, +30 structure, +80 garrisoned), 25, 3.15 s.
    ranked(
        soldier(
            "APA Inferno Squad",
            Faction::Apa,
            330,
            Armour::Infantry,
            credits(500),
            secs(13),
            9,
            2,
            garrison_buster(weapon(50, [65, 0, 0, 30], 25, 32), 80),
        ),
        42,
        79,
    ),
    // Napalm Mortar Squad (au_elitenapalmsoldiersquad): hp 160 (the wiki's 205), $700, 14 s;
    // napalm 40 (+55 inf, +30 structure, +80 garrisoned), 50, 4.15 s, lobbed as a shell.
    ranked(
        soldier(
            "APA Napalm Mortar Squad",
            Faction::Apa,
            160,
            Armour::Infantry,
            credits(700),
            secs(14),
            8,
            2,
            garrison_buster(shelling(weapon(40, [55, 0, 0, 30], 50, 42), 5, 40, 12), 80),
        ),
        42,
        79,
    ),
    // Dragon Tank (au_mediumflametank): hp 575, $800, 16 s; flamethrower 65 (+95 garrisoned),
    // 30, 3.15 s.
    ranked(
        soldier(
            "APA Dragon Tank",
            Faction::Apa,
            575,
            Armour::Light,
            credits(800),
            secs(16),
            10,
            3,
            garrison_buster(weapon(65, [0, 0, 0, 0], 30, 32), 95),
        ),
        72,
        80,
    ),
    // Rocket Tank (au_mediumrockettank): hp 580 (the wiki's 480), $950, 20 s; rockets 45 (+33
    // light), 35, 2 s, ground and air.
    ranked(
        soldier(
            "APA Rocket Tank",
            Faction::Apa,
            580,
            Armour::Light,
            credits(950),
            secs(20),
            10,
            3,
            anti_air(weapon(45, [0, 33, 0, 0], 35, 20), 35),
        ),
        54,
        85,
    ),
    // Saboteurs (au_saboteursquad): hp 325, $700, 12 s; sticky grenades 100 (+88 heavy), 18,
    // 3 s; stealthed (the wiki's Camouflage).
    stealthy(ranked(
        soldier(
            "APA Saboteurs",
            Faction::Apa,
            325,
            Armour::Infantry,
            credits(700),
            secs(12),
            10,
            2,
            weapon(100, [0, 0, 88, 0], 18, 30),
        ),
        54,
        85,
    )),
    // Sunstorm (au_heavynukeartillery): hp 600, $1550, 24 s; on the move 60 (+20 inf), 30,
    // 2 s; deployed, the nuclear howitzer 215, 65, 5 s. Shell speed and set-up: Thunderfist's.
    ranked(
        deploying(
            soldier(
                "APA Sunstorm",
                Faction::Apa,
                600,
                Armour::Light,
                credits(1550),
                secs(24),
                7,
                3,
                weapon(60, [20, 0, 0, 0], 30, 20),
            ),
            shelling(weapon(215, [0, 0, 0, 0], 65, 50), 8, 40, 20),
            30,
        ),
        45,
        67,
    ),
    // Shadow Tank (au_mediumstealthtank): hp 545, $1075, 17 s; cannon 85 (+30 light), 35,
    // 2.5 s; stealthed (the wiki's passive stealth).
    stealthy(ranked(
        soldier(
            "APA Shadow Tank",
            Faction::Apa,
            545,
            Armour::Light,
            credits(1075),
            secs(17),
            10,
            3,
            weapon(85, [0, 30, 0, 0], 35, 25),
        ),
        72,
        80,
    )),
    // Urban Assault Squad (au_urbanassaulttroopersquad): hp 300, $375, 10 s; shotguns 44, 20,
    // 2 s. Replaces the Red Guard for its generals.
    ranked(
        soldier(
            "APA Urban Assault Squad",
            Faction::Apa,
            300,
            Armour::Infantry,
            credits(375),
            secs(10),
            9,
            2,
            weapon(44, [0, 0, 0, 0], 20, 20),
        ),
        12,
        95,
    ),
    // Battle Bus (gu_battlebus): hp 525, $750, 14 s, unarmed; carries four squads who can't
    // fire out (the wiki's bunker deploy isn't modelled).
    carrying(
        ranked(
            with_armour(
                unit("GLA Battle Bus", Faction::Gla, 525, credits(750), 14, 15, false, None, NONE),
                Armour::Light,
            ),
            54,
            100,
        ),
        4,
    ),
    // Black Market Artillery (gu_heavyrocketartillery): hp 750 (the wiki's 850), $1250, 24 s;
    // rockets 70, 60, 1.75 s, lobbed as shells. The alpha's also fire at aircraft (`TargetType`
    // 0; the wiki: "anti-aircraft"); a unit here has one weapon and shells can't hit the air.
    ranked(
        soldier(
            "GLA Black Market Artillery",
            Faction::Gla,
            750,
            Armour::Light,
            credits(1250),
            secs(24),
            8,
            3,
            shelling(weapon(70, [0, 0, 0, 0], 60, 18), 6, 50, 15),
        ),
        105,
        89,
    ),
    // Shrapnel Tank (gu_heavyshrapneltank): hp 650, $1000, 16 s; cannon 60 (+35 heavy), 35,
    // 2.1 s. Replaces the Scorpion for its generals.
    ranked(
        soldier(
            "GLA Shrapnel Tank",
            Faction::Gla,
            650,
            Armour::Light,
            credits(1000),
            secs(16),
            10,
            3,
            weapon(60, [0, 0, 35, 0], 35, 21),
        ),
        72,
        80,
    ),
    // Toxin Tank (gu_mediumtoxintank): hp 625, $875, 17 s; toxin cannon 60 (+50 inf, +50
    // garrisoned), 35, 3 s. Replaces the Scorpion for its general.
    ranked(
        soldier(
            "GLA Toxin Tank",
            Faction::Gla,
            625,
            Armour::Light,
            credits(875),
            secs(17),
            10,
            3,
            garrison_buster(weapon(60, [50, 0, 0, 0], 35, 30), 50),
        ),
        54,
        85,
    ),
    // Mobile Chemist (gu_lightchemicalvehicle): hp 200, $550, 12 s; submachine gun 11 (+11
    // inf), 32, 1.2 s. Its healing isn't modelled. Replaces the Technical.
    ranked(
        soldier(
            "GLA Mobile Chemist",
            Faction::Gla,
            200,
            Armour::Light,
            credits(550),
            secs(12),
            16,
            2,
            weapon(11, [11, 0, 0, 0], 32, 12),
        ),
        23,
        94,
    ),
    // Bomb Truck (gu_lightbombvehicle): hp 160, $400, 13 s; high explosives 185 (+85 heavy),
    // 7, 1 s: it drives up and blows its charge, but as a gun, not by dying.
    soldier(
        "GLA Bomb Truck",
        Faction::Gla,
        160,
        Armour::Light,
        credits(400),
        secs(13),
        16,
        2,
        weapon(185, [0, 0, 85, 0], 7, 10),
    ),
];

/// A support power's aircraft: flies, unarmed, never built (no cost, no build time), worth the
/// usual 1 experience to whoever shoots it down.
const fn support_jet(name: &'static str, faction: Faction, hp: i32, armour: Armour) -> Def {
    flying(with_armour(unit(name, faction, hp, credits(0), 0, 45, false, None, NONE), armour))
}

/// Every faction's support powers, in slot order (slot 1 first). src: the alpha's
/// `RtsPlayer*Ability` assets:
/// - EU_PlayerAbility_AirRaid: 3 bombers, 320 base damage in a 25 radius (the AOE's 20, full
///   damage to 75% of it), recharge 360 s, 4 s before activation.
/// - EU_PlayerAbility_FuelAirBomb: 600 damage in a 35 radius, recharge 420 s, 3.5 s.
/// - APA_PlayerAbility_CarpetBombing: radius 35, recharge 420 s, 3.5 s; no damage figure.
/// - APA_PlayerAbility_SpySorties: radius 30, recharge 90 s, no delay, `ShroudBlocksActivation`
///   false (aimed into the fog).
///
/// The first use waits one recharge, as the generals' powers do. How the alpha splits a strike
/// into bombs isn't archived: the bomb counts and spacing are ours, chosen so each strike's
/// total over its area matches the alpha's figure. Carpet Bombing's damage is a placeholder.
static SUPPORT: [(Faction, Power); 6] = [
    (
        Faction::Eu,
        Power {
            name: "Air Raid",
            kind: PowerKind::Airstrike {
                def: EU_BOMBER,
                jets: 3,
                spread: Fx::from_int(10),
                bombs: 2,
                spacing: Fx::from_int(12),
                damage: 160,
                bonus: [0, 0, 0, 0],
                radius: Fx::from_int(10),
            },
            first_ticks: secs(360),
            cooldown_ticks: secs(360),
            blind: false,
            requires: None,
        },
    ),
    (
        Faction::Eu,
        Power {
            name: "Fuel Air Bomb",
            kind: PowerKind::Airstrike {
                def: EU_BOMBER,
                jets: 1,
                spread: Fx::ZERO,
                bombs: 1,
                spacing: Fx::ZERO,
                damage: 600,
                bonus: [0, 0, 0, 0],
                radius: Fx::from_int(35),
            },
            first_ticks: secs(420),
            cooldown_ticks: secs(420),
            blind: false,
            requires: None,
        },
    ),
    (
        Faction::Apa,
        Power {
            name: "Carpet Bombing",
            kind: PowerKind::Airstrike {
                def: APA_BOMBER,
                jets: 1,
                spread: Fx::ZERO,
                bombs: 7,
                spacing: Fx::from_int(10),
                damage: 150,
                bonus: [0, 0, 0, 0],
                radius: Fx::from_int(8),
            },
            first_ticks: secs(420),
            cooldown_ticks: secs(420),
            blind: false,
            requires: None,
        },
    ),
    (
        Faction::Apa,
        Power {
            name: "Spy Sorties",
            kind: PowerKind::Recon { def: APA_FIGHTER },
            first_ticks: secs(90),
            cooldown_ticks: secs(90),
            blind: true,
            requires: None,
        },
    ),
    // src: GLA_PlayerAbility_DirtyScudStorm (RtsPlayerSuperweaponAbility): MainDamage 2000 over
    // radius 65, eight missiles 0.1 s apart (its weapon container), launched 6 s after the
    // order, a toxic cloud of 20 damage a second for 9 s, recharge 420 s, aimed into the fog
    // (`ShroudBlocksActivation` false). The 2000 is split over the eight missiles (250 each,
    // radius 22, spread around the point to cover the 65); missile speed 60 is the projectile's
    // `InitialSpeed`.
    (
        Faction::Gla,
        Power {
            name: "Scud Storm",
            kind: PowerKind::Missiles {
                missiles: 8,
                delay_ticks: secs(6),
                every_ticks: g2_sim::TICKS_PER_SECOND / 10,
                speed: Fx::from_ratio(60, g2_sim::TICKS_PER_SECOND as i64),
                spread: Fx::from_int(30),
                damage: 250,
                radius: Fx::from_int(22),
                cloud_radius: Fx::from_int(65),
                cloud_damage: 20,
                cloud_ticks: secs(9),
            },
            first_ticks: secs(420),
            cooldown_ticks: secs(420),
            blind: true,
            requires: Some(GLA_SCUD_STORM),
        },
    ),
    // src: GLA_PlayerAbility_GPSScrambler: radius 50, recharge 90 s, 0.5 s delay, aimed into
    // the fog; its sphere effect hides the player's units in the area (an awareness change).
    // The alpha's effect lasts 4 s after leaving the sphere; ours lasts a flat 20 s, a
    // placeholder long enough to cross open ground.
    (
        Faction::Gla,
        Power {
            name: "GPS Scrambler",
            kind: PowerKind::Cloak { radius: Fx::from_int(50), ticks: secs(20) },
            first_ticks: secs(90),
            cooldown_ticks: secs(90),
            blind: true,
            requires: None,
        },
    ),
];

/// A neutral civilian building infantry can garrison: `capacity` places, firing out with the
/// alpha's 8-unit range bonus.
const fn civilian(name: &'static str, hp: i32, capacity: u32, radius: i64) -> Def {
    Def {
        name,
        faction: None,
        class: Class::Building { power: 0, power_radius: Fx::ZERO, role: BuildingRole::Civilian, weapon: None },
        hp,
        armour: Armour::Structure,
        cost: credits(0),
        build_ticks: 0,
        radius: Fx::from_int(radius),
        produces: NONE,
        builds: NONE,
        deploy: None,
        researches: NO_RESEARCH,
        experience: 0,
        veterancy: None,
        garrison: Some(Garrison { capacity, fires: true, range_bonus: Fx::from_int(8) }),
        stealth: false,
        detection: Fx::ZERO,
    }
}

/// Workers carry less per trip; the GLA compensates with a higher collector cap.
const HARVEST_WORKER: CollectorSpec = CollectorSpec { capacity: 40, load_ticks: secs(2), unload_ticks: secs(1) };

pub static RULES: Rules =
    Rules { defs: &DEFS, generals: &GENERALS, upgrades: &UPGRADES, perks: &PERKS, support: &SUPPORT };

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
/// The generals' perks (the alpha's Perks 3.0, October 2013: ranks 1-5, one slot per rank,
/// "about eight or nine perks ... unique per General", additive on the normal game, bought and
/// promoted with experience earned on that general). The names and what they act on are the
/// alpha's Perks 2.0 tooltips from the developer walkthrough (archive/videos/official,
/// 22:06-23:40), matched to what our units and powers can do: "Double Shifts" ('Carpet Bomb'
/// comes online faster after use) tunes our general's power the same way. Every percentage,
/// rank and price is a placeholder, of the size of a Zero Hour upgrade (10-25%).
static PERKS: [Perk; 112] = [
    // ---- Bulldog (EU) ----
    perk(
        "Double Shifts",
        "The artillery barrage comes online 20% faster after use.",
        1,
        200,
        None,
        &[],
        &[],
        Some(PowerTuning::Cooldown { percent: 80 }),
    ),
    perk(
        "Heavy Payload",
        "Barrage shells deal 20% more damage.",
        2,
        500,
        None,
        &[],
        &[],
        Some(PowerTuning::Strength { percent: 120 }),
    ),
    perk("Protector I", "Infantry take 10% less damage.", 1, 300, None, EU_INFANTRY_10, &[], None),
    perk(
        "Protector II",
        "Infantry take a further 10% less damage.",
        3,
        800,
        Some(BD_PROTECTOR_I),
        EU_INFANTRY_10,
        &[],
        None,
    ),
    perk("Factory Overtime", "Vehicles build 15% faster.", 2, 500, None, &[], EU_VEHICLES_FAST, None),
    perk(
        "Insourced",
        "Tanks cost 10% less.",
        3,
        800,
        None,
        &[],
        &[Modifier::Cost { def: EU_AEGIS, percent: 90 }],
        None,
    ),
    perk(
        "Aerodynamics I",
        "Aircraft gain 15% more hit points.",
        4,
        1000,
        None,
        &[(EU_VALKYRIE, Effect::Health { add: 93 })],
        &[],
        None,
    ),
    perk(
        "Aerodynamics II",
        "Aircraft gain a further 15% more hit points.",
        5,
        1500,
        Some(BD_AERODYNAMICS_I),
        &[(EU_VALKYRIE, Effect::Health { add: 93 })],
        &[],
        None,
    ),
    // ---- Taskmaster (APA) ----
    perk(
        "Spy Efficiency",
        "The field repair comes online 20% faster after use.",
        1,
        200,
        None,
        &[],
        &[],
        Some(PowerTuning::Cooldown { percent: 80 }),
    ),
    perk(
        "Logistics",
        "The field repair restores 25% more hit points.",
        2,
        500,
        None,
        &[],
        &[],
        Some(PowerTuning::Strength { percent: 125 }),
    ),
    perk("Protector I", "Infantry take 10% less damage.", 1, 300, None, APA_INFANTRY_10, &[], None),
    perk(
        "Protector II",
        "Infantry take a further 10% less damage.",
        3,
        800,
        Some(TM_PROTECTOR_I),
        APA_INFANTRY_10,
        &[],
        None,
    ),
    perk("Factory Overtime", "Vehicles build 15% faster.", 2, 500, None, &[], APA_VEHICLES_FAST, None),
    perk(
        "Vulcan Barrels",
        "The Gatling Tank deals 20% more damage.",
        3,
        800,
        None,
        &[(APA_GATLING, Effect::Damage { percent: 120, against: None })],
        &[],
        None,
    ),
    perk(
        "Kanban",
        "Aircraft build 20% faster.",
        4,
        1000,
        None,
        &[],
        &[Modifier::BuildTime { def: APA_MANTIS, percent: 80 }],
        None,
    ),
    perk(
        "Command I",
        "The command center takes 25% less damage.",
        5,
        1500,
        None,
        &[(APA_CC, Effect::Toughness { percent: 75 })],
        &[],
        None,
    ),
    // ---- Junkyard (GLA) ----
    perk(
        "Discipline",
        "The ambush comes online 20% faster after use.",
        1,
        200,
        None,
        &[],
        &[],
        Some(PowerTuning::Cooldown { percent: 80 }),
    ),
    perk(
        "Scavenger",
        "Units regain 2 hit points a second after 5 s without being hit.",
        3,
        800,
        None,
        GLA_SCAVENGE,
        &[],
        None,
    ),
    perk(
        "Leverage",
        "The ambush brings 50% more squads.",
        4,
        1000,
        None,
        &[],
        &[],
        Some(PowerTuning::Strength { percent: 150 }),
    ),
    perk(
        "Insourced",
        "Tanks cost 10% less.",
        2,
        500,
        None,
        &[],
        &[Modifier::Cost { def: GLA_TANK, percent: 90 }],
        None,
    ),
    perk("Protector I", "Infantry take 10% less damage.", 1, 300, None, GLA_INFANTRY_10, &[], None),
    perk(
        "Protector II",
        "Infantry take a further 10% less damage.",
        3,
        800,
        Some(JY_PROTECTOR_I),
        GLA_INFANTRY_10,
        &[],
        None,
    ),
    perk("Factory Overtime", "Vehicles build 15% faster.", 2, 500, None, &[], GLA_VEHICLES_FAST, None),
    perk("Leadership", "Units gain double experience from kills.", 5, 1500, None, GLA_LEADERSHIP, &[], None),
    perk(
        "Tailwind",
        "Air Raid comes online 20% faster after use.",
        1,
        200,
        None,
        &[],
        &[],
        Some(PowerTuning::Cooldown { percent: 80 }),
    ),
    perk(
        "Heavy Bombs",
        "Air Raid bombs deal 20% more damage.",
        2,
        500,
        None,
        &[],
        &[],
        Some(PowerTuning::Strength { percent: 120 }),
    ),
    perk("Protector I", "Infantry take 10% less damage.", 1, 300, None, EU_INFANTRY_10, &[], None),
    perk(
        "Protector II",
        "Infantry take a further 10% less damage.",
        3,
        800,
        Some(DD_PROTECTOR_I),
        EU_INFANTRY_10,
        &[],
        None,
    ),
    perk(
        "Kanban",
        "Longbows build 20% faster.",
        2,
        500,
        None,
        &[],
        &[Modifier::BuildTime { def: EU_LONGBOW, percent: 80 }],
        None,
    ),
    perk(
        "Aerodynamics I",
        "Aircraft gain 15% more hit points.",
        3,
        800,
        None,
        &[(EU_VALKYRIE, Effect::Health { add: 93 }), (EU_LONGBOW, Effect::Health { add: 36 })],
        &[],
        None,
    ),
    perk(
        "Aerodynamics II",
        "Aircraft gain a further 15% more hit points.",
        4,
        1000,
        Some(DD_AERODYNAMICS_I),
        &[(EU_VALKYRIE, Effect::Health { add: 93 }), (EU_LONGBOW, Effect::Health { add: 36 })],
        &[],
        None,
    ),
    perk(
        "Insourced",
        "Fox LRVs cost 10% less.",
        5,
        1500,
        None,
        &[],
        &[Modifier::Cost { def: EU_FOX, percent: 90 }],
        None,
    ),
    perk(
        "Double Time",
        "Bulwark comes online 20% faster after use.",
        1,
        200,
        None,
        &[],
        &[],
        Some(PowerTuning::Cooldown { percent: 80 }),
    ),
    perk(
        "Field Medics",
        "Bulwark restores 25% more hit points.",
        2,
        500,
        None,
        &[],
        &[],
        Some(PowerTuning::Strength { percent: 125 }),
    ),
    perk(
        "Protector I",
        "Shock Troopers take 10% less damage.",
        1,
        300,
        None,
        &[(EU_SHOCK_TROOPERS, PROTECTOR), (EU_ROCKETS, PROTECTOR)],
        &[],
        None,
    ),
    perk(
        "Protector II",
        "Shock Troopers take a further 10% less damage.",
        3,
        800,
        Some(MR_PROTECTOR_I),
        &[(EU_SHOCK_TROOPERS, PROTECTOR), (EU_ROCKETS, PROTECTOR)],
        &[],
        None,
    ),
    perk(
        "Rapid Deployment",
        "Shock Troopers build 20% faster.",
        2,
        500,
        None,
        &[],
        &[Modifier::BuildTime { def: EU_SHOCK_TROOPERS, percent: 80 }],
        None,
    ),
    perk(
        "Factory Overtime",
        "Assault APCs build 15% faster.",
        3,
        800,
        None,
        &[],
        &[Modifier::BuildTime { def: EU_ASSAULT_APC, percent: 85 }],
        None,
    ),
    perk(
        "Insourced",
        "Tanks cost 10% less.",
        4,
        1000,
        None,
        &[],
        &[Modifier::Cost { def: EU_AEGIS, percent: 90 }],
        None,
    ),
    perk(
        "Leadership",
        "Infantry gain double experience from kills.",
        5,
        1500,
        None,
        &[(EU_SHOCK_TROOPERS, LEAD), (EU_ROCKETS, LEAD)],
        &[],
        None,
    ),
    perk(
        "Capacitors",
        "Skylance comes online 20% faster after use.",
        1,
        200,
        None,
        &[],
        &[],
        Some(PowerTuning::Cooldown { percent: 80 }),
    ),
    perk(
        "Focusing Optics",
        "Skylance deals 20% more damage.",
        2,
        500,
        None,
        &[],
        &[],
        Some(PowerTuning::Strength { percent: 120 }),
    ),
    perk(
        "Protector I",
        "Infantry take 10% less damage.",
        1,
        300,
        None,
        &[(EU_SHOCK_TROOPERS, PROTECTOR), (EU_ROCKETS, PROTECTOR)],
        &[],
        None,
    ),
    perk(
        "Protector II",
        "Infantry take a further 10% less damage.",
        3,
        800,
        Some(CE_PROTECTOR_I),
        &[(EU_SHOCK_TROOPERS, PROTECTOR), (EU_ROCKETS, PROTECTOR)],
        &[],
        None,
    ),
    perk(
        "Factory Overtime",
        "Heavy Assault Lasers build 15% faster.",
        2,
        500,
        None,
        &[],
        &[Modifier::BuildTime { def: EU_LASER_TANK, percent: 85 }],
        None,
    ),
    perk(
        "Reinforced Turrets",
        "Railgun Turrets take 25% less damage.",
        3,
        800,
        None,
        &[(EU_RAILGUN_TURRET, Effect::Toughness { percent: 75 })],
        &[],
        None,
    ),
    perk(
        "Insourced",
        "Heavy Assault Lasers cost 10% less.",
        4,
        1000,
        None,
        &[],
        &[Modifier::Cost { def: EU_LASER_TANK, percent: 90 }],
        None,
    ),
    perk(
        "Overcharge",
        "Heavy Assault Lasers deal 20% more damage.",
        5,
        1500,
        None,
        &[(EU_LASER_TANK, Effect::Damage { percent: 120, against: None })],
        &[],
        None,
    ),
    perk(
        "Coolant",
        "Plasma Bomb comes online 20% faster after use.",
        1,
        200,
        None,
        &[],
        &[],
        Some(PowerTuning::Cooldown { percent: 80 }),
    ),
    perk(
        "Plasma Core",
        "Plasma Bomb deals 20% more damage.",
        2,
        500,
        None,
        &[],
        &[],
        Some(PowerTuning::Strength { percent: 120 }),
    ),
    perk("Protector I", "Infantry take 10% less damage.", 1, 300, None, EU_INFANTRY_10, &[], None),
    perk(
        "Protector II",
        "Infantry take a further 10% less damage.",
        3,
        800,
        Some(CL_PROTECTOR_I),
        EU_INFANTRY_10,
        &[],
        None,
    ),
    perk(
        "Factory Overtime",
        "Railgun Tanks build 15% faster.",
        2,
        500,
        None,
        &[],
        &[Modifier::BuildTime { def: EU_RAILGUN_TANK, percent: 85 }],
        None,
    ),
    perk(
        "Insourced",
        "Railgun Tanks cost 10% less.",
        3,
        800,
        None,
        &[],
        &[Modifier::Cost { def: EU_RAILGUN_TANK, percent: 90 }],
        None,
    ),
    perk(
        "Heavy Plating",
        "Railgun Tanks take 10% less damage.",
        4,
        1000,
        None,
        &[(EU_RAILGUN_TANK, PROTECTOR)],
        &[],
        None,
    ),
    perk(
        "Depleted Slugs",
        "Railguns deal 20% more damage.",
        5,
        1500,
        None,
        &[
            (EU_RAILGUN_TANK, Effect::Damage { percent: 120, against: None }),
            (EU_RAILGUN_TURRET, Effect::Damage { percent: 120, against: None }),
        ],
        &[],
        None,
    ),
    perk(
        "Double Shifts",
        "Inferno Artillery comes online 20% faster after use.",
        1,
        200,
        None,
        &[],
        &[],
        Some(PowerTuning::Cooldown { percent: 80 }),
    ),
    perk(
        "Infernal Diesel",
        "Inferno Artillery deals 20% more damage.",
        2,
        500,
        None,
        &[],
        &[],
        Some(PowerTuning::Strength { percent: 120 }),
    ),
    perk(
        "Protector I",
        "Infantry take 10% less damage.",
        1,
        300,
        None,
        &[(APA_RED_GUARD, PROTECTOR), (APA_TANK_HUNTER, PROTECTOR), (APA_NAPALM_MORTAR, PROTECTOR)],
        &[],
        None,
    ),
    perk(
        "Protector II",
        "Infantry take a further 10% less damage.",
        3,
        800,
        Some(IN_PROTECTOR_I),
        &[(APA_RED_GUARD, PROTECTOR), (APA_TANK_HUNTER, PROTECTOR), (APA_NAPALM_MORTAR, PROTECTOR)],
        &[],
        None,
    ),
    perk(
        "Factory Overtime",
        "Dragon Tanks build 15% faster.",
        2,
        500,
        None,
        &[],
        &[Modifier::BuildTime { def: APA_DRAGON, percent: 85 }],
        None,
    ),
    perk(
        "Ignition",
        "Dragon Tanks deal 20% more damage.",
        3,
        800,
        None,
        &[(APA_DRAGON, Effect::Damage { percent: 120, against: None })],
        &[],
        None,
    ),
    perk(
        "Insourced",
        "Rocket Tanks cost 10% less.",
        4,
        1000,
        None,
        &[],
        &[Modifier::Cost { def: APA_ROCKET_TANK, percent: 90 }],
        None,
    ),
    perk(
        "Leadership",
        "Units gain double experience from kills.",
        5,
        1500,
        None,
        &[(APA_DRAGON, LEAD), (APA_ROCKET_TANK, LEAD), (APA_NAPALM_MORTAR, LEAD)],
        &[],
        None,
    ),
    perk(
        "Salvo Timing",
        "Thunderclap comes online 20% faster after use.",
        1,
        200,
        None,
        &[],
        &[],
        Some(PowerTuning::Cooldown { percent: 80 }),
    ),
    perk(
        "Heavy Warheads",
        "Thunderclap rockets deal 20% more damage.",
        2,
        500,
        None,
        &[],
        &[],
        Some(PowerTuning::Strength { percent: 120 }),
    ),
    perk("Protector I", "Infantry take 10% less damage.", 1, 300, None, APA_INFANTRY_10, &[], None),
    perk(
        "Protector II",
        "Infantry take a further 10% less damage.",
        3,
        800,
        Some(RA_PROTECTOR_I),
        APA_INFANTRY_10,
        &[],
        None,
    ),
    perk(
        "Tank Hunters",
        "Tank Hunters build 20% faster.",
        2,
        500,
        None,
        &[],
        &[Modifier::BuildTime { def: APA_TANK_HUNTER, percent: 80 }],
        None,
    ),
    perk(
        "Factory Overtime",
        "Rocket Tanks build 15% faster.",
        3,
        800,
        None,
        &[],
        &[Modifier::BuildTime { def: APA_ROCKET_TANK, percent: 85 }],
        None,
    ),
    perk(
        "Insourced",
        "Rocket Tanks cost 10% less.",
        4,
        1000,
        None,
        &[],
        &[Modifier::Cost { def: APA_ROCKET_TANK, percent: 90 }],
        None,
    ),
    perk(
        "Guidance",
        "Rocket units deal 20% more damage.",
        5,
        1500,
        None,
        &[
            (APA_TANK_HUNTER, Effect::Damage { percent: 120, against: None }),
            (APA_ROCKET_TANK, Effect::Damage { percent: 120, against: None }),
        ],
        &[],
        None,
    ),
    perk(
        "Fission",
        "Neutron Bomb comes online 20% faster after use.",
        1,
        200,
        None,
        &[],
        &[],
        Some(PowerTuning::Cooldown { percent: 80 }),
    ),
    perk(
        "Enriched Core",
        "Neutron Bomb deals 20% more damage.",
        2,
        500,
        None,
        &[],
        &[],
        Some(PowerTuning::Strength { percent: 120 }),
    ),
    perk("Protector I", "Infantry take 10% less damage.", 1, 300, None, APA_INFANTRY_10, &[], None),
    perk(
        "Protector II",
        "Infantry take a further 10% less damage.",
        3,
        800,
        Some(TS_PROTECTOR_I),
        APA_INFANTRY_10,
        &[],
        None,
    ),
    perk(
        "Factory Overtime",
        "Shadow Tanks build 15% faster.",
        2,
        500,
        None,
        &[],
        &[Modifier::BuildTime { def: APA_SHADOW_TANK, percent: 85 }],
        None,
    ),
    perk(
        "Uranium Shells",
        "Sunstorms deal 20% more damage.",
        3,
        800,
        None,
        &[(APA_SUNSTORM, Effect::ShellDamage { percent: 120 })],
        &[],
        None,
    ),
    perk(
        "Insourced",
        "Sunstorms cost 10% less.",
        4,
        1000,
        None,
        &[],
        &[Modifier::Cost { def: APA_SUNSTORM, percent: 90 }],
        None,
    ),
    perk(
        "Leadership",
        "Tanks gain double experience from kills.",
        5,
        1500,
        None,
        &[(APA_SHADOW_TANK, LEAD), (APA_ROCKET_TANK, LEAD), (APA_SUNSTORM, LEAD)],
        &[],
        None,
    ),
    perk(
        "Quick Hands",
        "Misdirection comes online 20% faster after use.",
        1,
        200,
        None,
        &[],
        &[],
        Some(PowerTuning::Cooldown { percent: 80 }),
    ),
    perk(
        "Deep Cover",
        "Misdirection lasts 25% longer.",
        2,
        500,
        None,
        &[],
        &[],
        Some(PowerTuning::Strength { percent: 125 }),
    ),
    perk(
        "Protector I",
        "Infantry take 10% less damage.",
        1,
        300,
        None,
        &[(APA_URBAN_ASSAULT, PROTECTOR), (APA_TANK_HUNTER, PROTECTOR), (APA_SABOTEURS, PROTECTOR)],
        &[],
        None,
    ),
    perk(
        "Protector II",
        "Infantry take a further 10% less damage.",
        3,
        800,
        Some(BJ_PROTECTOR_I),
        &[(APA_URBAN_ASSAULT, PROTECTOR), (APA_TANK_HUNTER, PROTECTOR), (APA_SABOTEURS, PROTECTOR)],
        &[],
        None,
    ),
    perk(
        "Population Control",
        "Saboteurs cost 10% less.",
        2,
        500,
        None,
        &[],
        &[Modifier::Cost { def: APA_SABOTEURS, percent: 90 }],
        None,
    ),
    perk(
        "Factory Overtime",
        "Shadow Tanks build 15% faster.",
        3,
        800,
        None,
        &[],
        &[Modifier::BuildTime { def: APA_SHADOW_TANK, percent: 85 }],
        None,
    ),
    perk(
        "Sticky Charges",
        "Saboteurs deal 20% more damage.",
        4,
        1000,
        None,
        &[(APA_SABOTEURS, Effect::Damage { percent: 120, against: None })],
        &[],
        None,
    ),
    perk(
        "Leadership",
        "Units gain double experience from kills.",
        5,
        1500,
        None,
        &[(APA_URBAN_ASSAULT, LEAD), (APA_SHADOW_TANK, LEAD), (APA_SABOTEURS, LEAD)],
        &[],
        None,
    ),
    perk(
        "Catalyst",
        "Toxin Bomb comes online 20% faster after use.",
        1,
        200,
        None,
        &[],
        &[],
        Some(PowerTuning::Cooldown { percent: 80 }),
    ),
    perk(
        "Concentrate",
        "Toxin Bomb deals 20% more damage.",
        2,
        500,
        None,
        &[],
        &[],
        Some(PowerTuning::Strength { percent: 120 }),
    ),
    perk("Protector I", "Infantry take 10% less damage.", 1, 300, None, GLA_INFANTRY_10, &[], None),
    perk(
        "Protector II",
        "Infantry take a further 10% less damage.",
        3,
        800,
        Some(TX_PROTECTOR_I),
        GLA_INFANTRY_10,
        &[],
        None,
    ),
    perk(
        "Factory Overtime",
        "Toxin Tanks build 15% faster.",
        2,
        500,
        None,
        &[],
        &[Modifier::BuildTime { def: GLA_TOXIN_TANK, percent: 85 }],
        None,
    ),
    perk(
        "Anthrax Beta",
        "Toxin Tanks deal 20% more damage.",
        3,
        800,
        None,
        &[(GLA_TOXIN_TANK, Effect::Damage { percent: 120, against: None })],
        &[],
        None,
    ),
    perk(
        "Insourced",
        "Toxin Tanks cost 10% less.",
        4,
        1000,
        None,
        &[],
        &[Modifier::Cost { def: GLA_TOXIN_TANK, percent: 90 }],
        None,
    ),
    perk(
        "Leadership",
        "Units gain double experience from kills.",
        5,
        1500,
        None,
        &[(GLA_TOXIN_TANK, LEAD), (GLA_MOBILE_CHEMIST, LEAD), (GLA_RPG, LEAD)],
        &[],
        None,
    ),
    perk(
        "Quick Reload",
        "Mass RPG Strike comes online 20% faster after use.",
        1,
        200,
        None,
        &[],
        &[],
        Some(PowerTuning::Cooldown { percent: 80 }),
    ),
    perk(
        "Shaped Charges",
        "Mass RPG Strike deals 20% more damage.",
        2,
        500,
        None,
        &[],
        &[],
        Some(PowerTuning::Strength { percent: 120 }),
    ),
    perk("Protector I", "Infantry take 10% less damage.", 1, 300, None, GLA_INFANTRY_10, &[], None),
    perk(
        "Protector II",
        "Infantry take a further 10% less damage.",
        3,
        800,
        Some(WD_PROTECTOR_I),
        GLA_INFANTRY_10,
        &[],
        None,
    ),
    perk(
        "Factory Overtime",
        "Shrapnel Tanks build 15% faster.",
        2,
        500,
        None,
        &[],
        &[Modifier::BuildTime { def: GLA_SHRAPNEL_TANK, percent: 85 }],
        None,
    ),
    perk(
        "Junk Armor",
        "Shrapnel Tanks take 10% less damage.",
        3,
        800,
        None,
        &[(GLA_SHRAPNEL_TANK, PROTECTOR)],
        &[],
        None,
    ),
    perk(
        "Insourced",
        "Shrapnel Tanks cost 10% less.",
        4,
        1000,
        None,
        &[],
        &[Modifier::Cost { def: GLA_SHRAPNEL_TANK, percent: 90 }],
        None,
    ),
    perk(
        "Leadership",
        "Units gain double experience from kills.",
        5,
        1500,
        None,
        &[(GLA_SHRAPNEL_TANK, LEAD), (GLA_RABBLE, LEAD), (GLA_RPG, LEAD)],
        &[],
        None,
    ),
    perk(
        "Scrap Parts",
        "Stealth Field comes online 20% faster after use.",
        1,
        200,
        None,
        &[],
        &[],
        Some(PowerTuning::Cooldown { percent: 80 }),
    ),
    perk(
        "Long Field",
        "Stealth Field lasts 25% longer.",
        2,
        500,
        None,
        &[],
        &[],
        Some(PowerTuning::Strength { percent: 125 }),
    ),
    perk("Protector I", "Infantry take 10% less damage.", 1, 300, None, GLA_INFANTRY_10, &[], None),
    perk(
        "Protector II",
        "Infantry take a further 10% less damage.",
        3,
        800,
        Some(KB_PROTECTOR_I),
        GLA_INFANTRY_10,
        &[],
        None,
    ),
    perk(
        "Factory Overtime",
        "Black Market Artillery builds 15% faster.",
        2,
        500,
        None,
        &[],
        &[Modifier::BuildTime { def: GLA_BLACK_MARKET_ARTILLERY, percent: 85 }],
        None,
    ),
    perk(
        "Junk Repair",
        "Vehicles regain 2 hit points a second after 5 s without being hit.",
        3,
        800,
        None,
        &[(GLA_TANK, SCAVENGE), (GLA_TECHNICAL, SCAVENGE), (GLA_BLACK_MARKET_ARTILLERY, SCAVENGE)],
        &[],
        None,
    ),
    perk(
        "Insourced",
        "Black Market Artillery costs 10% less.",
        4,
        1000,
        None,
        &[],
        &[Modifier::Cost { def: GLA_BLACK_MARKET_ARTILLERY, percent: 90 }],
        None,
    ),
    perk("Leadership", "Units gain double experience from kills.", 5, 1500, None, GLA_LEADERSHIP, &[], None),
];

#[allow(clippy::too_many_arguments)]
const fn perk(
    name: &'static str,
    description: &'static str,
    rank: u8,
    price: u32,
    requires: Option<PerkId>,
    effects: &'static [(DefId, Effect)],
    modifiers: &'static [Modifier],
    power: Option<PowerTuning>,
) -> Perk {
    Perk { name, description, rank, price, requires, effects, modifiers, power }
}

// A faction's perk tables list every unit of its kind, the generals' own ones too: only a
// player with that general can train them, so for anyone else those entries do nothing.
const PROTECTOR: Effect = Effect::Toughness { percent: 90 };
const EU_INFANTRY_10: &[(DefId, Effect)] =
    &[(EU_RECON, PROTECTOR), (EU_ROCKETS, PROTECTOR), (EU_SNIPERS, PROTECTOR), (EU_SHOCK_TROOPERS, PROTECTOR)];
const APA_INFANTRY_10: &[(DefId, Effect)] = &[
    (APA_RED_GUARD, PROTECTOR),
    (APA_TANK_HUNTER, PROTECTOR),
    (APA_INFERNO, PROTECTOR),
    (APA_NAPALM_MORTAR, PROTECTOR),
    (APA_SABOTEURS, PROTECTOR),
    (APA_URBAN_ASSAULT, PROTECTOR),
];
const GLA_INFANTRY_10: &[(DefId, Effect)] = &[(GLA_RABBLE, PROTECTOR), (GLA_RPG, PROTECTOR)];
const OVERTIME: u32 = 85;
const EU_VEHICLES_FAST: &[Modifier] = &[
    Modifier::BuildTime { def: EU_AEGIS, percent: OVERTIME },
    Modifier::BuildTime { def: EU_ARTILLERY, percent: OVERTIME },
    Modifier::BuildTime { def: EU_APC, percent: OVERTIME },
    Modifier::BuildTime { def: EU_FOX, percent: OVERTIME },
    Modifier::BuildTime { def: EU_ASSAULT_APC, percent: OVERTIME },
    Modifier::BuildTime { def: EU_LASER_TANK, percent: OVERTIME },
    Modifier::BuildTime { def: EU_RAILGUN_TANK, percent: OVERTIME },
];
const APA_VEHICLES_FAST: &[Modifier] = &[
    Modifier::BuildTime { def: APA_WARLORD, percent: OVERTIME },
    Modifier::BuildTime { def: APA_THUNDERFIST, percent: OVERTIME },
    Modifier::BuildTime { def: APA_GATLING, percent: OVERTIME },
    Modifier::BuildTime { def: APA_DRAGON, percent: OVERTIME },
    Modifier::BuildTime { def: APA_ROCKET_TANK, percent: OVERTIME },
    Modifier::BuildTime { def: APA_SUNSTORM, percent: OVERTIME },
    Modifier::BuildTime { def: APA_SHADOW_TANK, percent: OVERTIME },
];
const GLA_VEHICLES_FAST: &[Modifier] = &[
    Modifier::BuildTime { def: GLA_TANK, percent: OVERTIME },
    Modifier::BuildTime { def: GLA_TOXIN_ARTILLERY, percent: OVERTIME },
    Modifier::BuildTime { def: GLA_TECHNICAL, percent: OVERTIME },
    Modifier::BuildTime { def: GLA_BATTLE_BUS, percent: OVERTIME },
    Modifier::BuildTime { def: GLA_BLACK_MARKET_ARTILLERY, percent: OVERTIME },
    Modifier::BuildTime { def: GLA_SHRAPNEL_TANK, percent: OVERTIME },
    Modifier::BuildTime { def: GLA_TOXIN_TANK, percent: OVERTIME },
    Modifier::BuildTime { def: GLA_MOBILE_CHEMIST, percent: OVERTIME },
    Modifier::BuildTime { def: GLA_BOMB_TRUCK, percent: OVERTIME },
];
// A kill is 1 experience, so only whole multiples change anything.
const LEAD: Effect = Effect::Experience { percent: 200 };
const GLA_LEADERSHIP: &[(DefId, Effect)] = &[
    (GLA_RABBLE, LEAD),
    (GLA_RPG, LEAD),
    (GLA_TANK, LEAD),
    (GLA_TOXIN_ARTILLERY, LEAD),
    (GLA_TECHNICAL, LEAD),
    (GLA_TOXIN_CHOPPER, LEAD),
    (GLA_BATTLE_BUS, LEAD),
    (GLA_BLACK_MARKET_ARTILLERY, LEAD),
    (GLA_SHRAPNEL_TANK, LEAD),
    (GLA_TOXIN_TANK, LEAD),
    (GLA_MOBILE_CHEMIST, LEAD),
    (GLA_BOMB_TRUCK, LEAD),
];
const SCAVENGE: Effect = Effect::Repair { hp: 2, calm_ticks: secs(5) };
const GLA_SCAVENGE: &[(DefId, Effect)] = &[
    (GLA_RABBLE, SCAVENGE),
    (GLA_RPG, SCAVENGE),
    (GLA_TANK, SCAVENGE),
    (GLA_TOXIN_ARTILLERY, SCAVENGE),
    (GLA_TECHNICAL, SCAVENGE),
    (GLA_TOXIN_CHOPPER, SCAVENGE),
    (GLA_BATTLE_BUS, SCAVENGE),
    (GLA_BLACK_MARKET_ARTILLERY, SCAVENGE),
    (GLA_SHRAPNEL_TANK, SCAVENGE),
    (GLA_TOXIN_TANK, SCAVENGE),
    (GLA_MOBILE_CHEMIST, SCAVENGE),
    (GLA_BOMB_TRUCK, SCAVENGE),
];

static GENERALS: [General; 14] = [
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
            blind: false,
            requires: None,
        }),
        // levels: more shells (placeholders; the general is ours).
        power_levels: &[
            Power {
                name: "Artillery barrage",
                kind: PowerKind::Barrage {
                    shells: 7,
                    delay_ticks: secs(3),
                    every_ticks: 5 * g2_sim::TICKS_PER_SECOND / 10,
                    damage: 90,
                    bonus: [0, 20, 0, 0],
                    radius: Fx::from_int(7),
                },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
            Power {
                name: "Artillery barrage",
                kind: PowerKind::Barrage {
                    shells: 9,
                    delay_ticks: secs(3),
                    every_ticks: 5 * g2_sim::TICKS_PER_SECOND / 10,
                    damage: 90,
                    bonus: [0, 20, 0, 0],
                    radius: Fx::from_int(7),
                },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
        ],
        units: &[],
        perks: &[
            BD_DOUBLE_SHIFTS,
            BD_HEAVY_PAYLOAD,
            BD_PROTECTOR_I,
            BD_PROTECTOR_II,
            BD_FACTORY_OVERTIME,
            BD_INSOURCED,
            BD_AERODYNAMICS_I,
            BD_AERODYNAMICS_II,
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
        // A field repair: the support powers' "Supply drop" heals nearby units. Every own unit
        // and building within 30 regains up to 400 hp.
        power: Some(Power {
            name: "Field repair",
            kind: PowerKind::Repair { radius: Fx::from_int(30), hp: 400 },
            first_ticks: secs(180),
            cooldown_ticks: secs(180),
            blind: false,
            requires: None,
        }),
        // the alpha's global Supply Drop: radius 30, then 40 at lvl02 and lvl03, recharge 180 s; units: the wiki's Inferno Squad and Saboteurs for Taskmaster.
        power_levels: &[
            Power {
                name: "Field repair",
                kind: PowerKind::Repair { radius: Fx::from_int(40), hp: 400 },
                first_ticks: secs(180),
                cooldown_ticks: secs(180),
                blind: false,
                requires: None,
            },
            Power {
                name: "Field repair",
                kind: PowerKind::Repair { radius: Fx::from_int(40), hp: 500 },
                first_ticks: secs(180),
                cooldown_ticks: secs(180),
                blind: false,
                requires: None,
            },
        ],
        units: &[
            GeneralUnit { unit: APA_INFERNO, replaces: None, at: APA_BARRACKS },
            GeneralUnit { unit: APA_SABOTEURS, replaces: None, at: APA_BARRACKS },
        ],
        perks: &[
            TM_SPY_EFFICIENCY,
            TM_LOGISTICS,
            TM_PROTECTOR_I,
            TM_PROTECTOR_II,
            TM_FACTORY_OVERTIME,
            TM_VULCAN_BARRELS,
            TM_KANBAN,
            TM_COMMAND_I,
        ],
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
            blind: false,
            requires: None,
        }),
        // units: the alpha's GLA_JunkyardGeneral (Battle Bus, Black Market Artillery, the Shrapnel Tank in place of the Scorpion); levels: more squads (placeholders).
        power_levels: &[
            Power {
                name: "Ambush",
                kind: PowerKind::Ambush { def: GLA_RPG, count: 5 },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
            Power {
                name: "Ambush",
                kind: PowerKind::Ambush { def: GLA_RPG, count: 6 },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
        ],
        units: &[
            GeneralUnit { unit: GLA_BATTLE_BUS, replaces: None, at: GLA_FACTORY },
            GeneralUnit { unit: GLA_BLACK_MARKET_ARTILLERY, replaces: None, at: GLA_FACTORY },
            GeneralUnit { unit: GLA_SHRAPNEL_TANK, replaces: Some(GLA_TANK), at: GLA_FACTORY },
        ],
        perks: &[
            JY_DISCIPLINE,
            JY_SCAVENGER,
            JY_LEVERAGE,
            JY_INSOURCED,
            JY_PROTECTOR_I,
            JY_PROTECTOR_II,
            JY_FACTORY_OVERTIME,
            JY_LEADERSHIP,
        ],
    },
    // src: the EU air force general (the wiki's Daedalus, the alpha's EU_AirforceGeneral, icon EG_Airforce_01): Fox LRV in place of the APC, Longbow helicopters, the Sniper Team; power Air Raid (lvl01-03: 3 bombers, recharge 240 s, 1 s before activation).
    // Levels 2 and 3 use the alpha's lvl02/03 numbers where they differ, else placeholder
    // steps; the damage of every power is a placeholder (the alpha's are effect assets).
    General {
        name: "Daedalus",
        faction: Faction::Eu,
        modifiers: &[],
        power: Some(Power {
            name: "Air Raid",
            kind: PowerKind::Airstrike {
                def: EU_BOMBER,
                jets: 3,
                spread: Fx::from_int(10),
                bombs: 2,
                spacing: Fx::from_int(12),
                damage: 140,
                bonus: [0, 0, 0, 0],
                radius: Fx::from_int(10),
            },
            first_ticks: secs(240),
            cooldown_ticks: secs(240),
            blind: false,
            requires: None,
        }),
        power_levels: &[
            Power {
                name: "Air Raid",
                kind: PowerKind::Airstrike {
                    def: EU_BOMBER,
                    jets: 3,
                    spread: Fx::from_int(10),
                    bombs: 2,
                    spacing: Fx::from_int(12),
                    damage: 175,
                    bonus: [0, 0, 0, 0],
                    radius: Fx::from_int(10),
                },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
            Power {
                name: "Air Raid",
                kind: PowerKind::Airstrike {
                    def: EU_BOMBER,
                    jets: 4,
                    spread: Fx::from_int(10),
                    bombs: 2,
                    spacing: Fx::from_int(12),
                    damage: 175,
                    bonus: [0, 0, 0, 0],
                    radius: Fx::from_int(10),
                },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
        ],
        units: &[
            GeneralUnit { unit: EU_FOX, replaces: Some(EU_APC), at: EU_FACTORY },
            GeneralUnit { unit: EU_LONGBOW, replaces: None, at: EU_AIRFIELD },
            GeneralUnit { unit: EU_SNIPERS, replaces: None, at: EU_BARRACKS },
        ],
        perks: &[
            DD_TAILWIND,
            DD_HEAVY_BOMBS,
            DD_PROTECTOR_I,
            DD_PROTECTOR_II,
            DD_KANBAN,
            DD_AERODYNAMICS_I,
            DD_AERODYNAMICS_II,
            DD_INSOURCED,
        ],
    },
    // src: the EU rapid assault general (the wiki's Mercury Red: Shock Troopers; the alpha's EU_RapidAssaultGeneral): Shock Troopers in place of the Recon Squad, the Assault APC in place of the APC, Longbows; power Bulwark (radius 25, recharge 180 s; the alpha's effect is a defensive buff, here a field repair).
    // Levels 2 and 3 use the alpha's lvl02/03 numbers where they differ, else placeholder
    // steps; the damage of every power is a placeholder (the alpha's are effect assets).
    General {
        name: "Mercury Red",
        faction: Faction::Eu,
        modifiers: &[],
        power: Some(Power {
            name: "Bulwark",
            kind: PowerKind::Repair { radius: Fx::from_int(25), hp: 300 },
            first_ticks: secs(180),
            cooldown_ticks: secs(180),
            blind: false,
            requires: None,
        }),
        power_levels: &[
            Power {
                name: "Bulwark",
                kind: PowerKind::Repair { radius: Fx::from_int(25), hp: 375 },
                first_ticks: secs(180),
                cooldown_ticks: secs(180),
                blind: false,
                requires: None,
            },
            Power {
                name: "Bulwark",
                kind: PowerKind::Repair { radius: Fx::from_int(25), hp: 450 },
                first_ticks: secs(180),
                cooldown_ticks: secs(180),
                blind: false,
                requires: None,
            },
        ],
        units: &[
            GeneralUnit { unit: EU_SHOCK_TROOPERS, replaces: Some(EU_RECON), at: EU_BARRACKS },
            GeneralUnit { unit: EU_ASSAULT_APC, replaces: Some(EU_APC), at: EU_FACTORY },
            GeneralUnit { unit: EU_LONGBOW, replaces: None, at: EU_AIRFIELD },
        ],
        perks: &[
            MR_DOUBLE_TIME,
            MR_FIELD_MEDICS,
            MR_PROTECTOR_I,
            MR_PROTECTOR_II,
            MR_RAPID_DEPLOYMENT,
            MR_FACTORY_OVERTIME,
            MR_INSOURCED,
            MR_LEADERSHIP,
        ],
    },
    // src: the EU experimental weapons general (the wiki's Cutting Edge: Heavy Assault Laser; the alpha's EU_LaserGeneral): the Assault APC in place of the APC, the Heavy Assault Laser, the Railgun Turret in place of the Gun Turret, Longbows, Shock Troopers in place of the Recon Squad; power Skylance (an orbital strike, radius 18, recharge 240 s, 300 base damage from lvl02).
    // Levels 2 and 3 use the alpha's lvl02/03 numbers where they differ, else placeholder
    // steps; the damage of every power is a placeholder (the alpha's are effect assets).
    General {
        name: "Cutting Edge",
        faction: Faction::Eu,
        modifiers: &[],
        power: Some(Power {
            name: "Skylance",
            kind: PowerKind::Barrage {
                shells: 1,
                delay_ticks: secs(3),
                every_ticks: g2_sim::TICKS_PER_SECOND / 10,
                damage: 240,
                bonus: [0, 0, 0, 0],
                radius: Fx::from_int(18),
            },
            first_ticks: secs(240),
            cooldown_ticks: secs(240),
            blind: false,
            requires: None,
        }),
        power_levels: &[
            Power {
                name: "Skylance",
                kind: PowerKind::Barrage {
                    shells: 1,
                    delay_ticks: secs(3),
                    every_ticks: g2_sim::TICKS_PER_SECOND / 10,
                    damage: 300,
                    bonus: [0, 0, 0, 0],
                    radius: Fx::from_int(18),
                },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
            Power {
                name: "Skylance",
                kind: PowerKind::Barrage {
                    shells: 2,
                    delay_ticks: secs(3),
                    every_ticks: 5 * g2_sim::TICKS_PER_SECOND / 10,
                    damage: 300,
                    bonus: [0, 0, 0, 0],
                    radius: Fx::from_int(18),
                },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
        ],
        units: &[
            GeneralUnit { unit: EU_ASSAULT_APC, replaces: Some(EU_APC), at: EU_FACTORY },
            GeneralUnit { unit: EU_LASER_TANK, replaces: None, at: EU_FACTORY },
            GeneralUnit { unit: EU_RAILGUN_TURRET, replaces: Some(EU_GUN_TURRET), at: EU_DOZER },
            GeneralUnit { unit: EU_LONGBOW, replaces: None, at: EU_AIRFIELD },
            GeneralUnit { unit: EU_SHOCK_TROOPERS, replaces: Some(EU_RECON), at: EU_BARRACKS },
        ],
        perks: &[
            CE_CAPACITORS,
            CE_FOCUSING,
            CE_PROTECTOR_I,
            CE_PROTECTOR_II,
            CE_FACTORY_OVERTIME,
            CE_REINFORCED,
            CE_INSOURCED,
            CE_OVERCHARGE,
        ],
    },
    // src: the EU railgun general (the wiki's Cleaver: Railgun Squad; the alpha's EU_ScienceGeneral, icon EG_Railgun_01): Fox LRV, the Assault APC in place of the APC, Longbows, the Railgun Turret in place of the Gun Turret, the Railgun Tank; power Plasma Bomb (radius 25/30/35, recharge 240 s, 2.5 s).
    // Levels 2 and 3 use the alpha's lvl02/03 numbers where they differ, else placeholder
    // steps; the damage of every power is a placeholder (the alpha's are effect assets).
    General {
        name: "Cleaver",
        faction: Faction::Eu,
        modifiers: &[],
        power: Some(Power {
            name: "Plasma Bomb",
            kind: PowerKind::Barrage {
                shells: 1,
                delay_ticks: secs(3),
                every_ticks: g2_sim::TICKS_PER_SECOND / 10,
                damage: 200,
                bonus: [0, 0, 0, 0],
                radius: Fx::from_int(25),
            },
            first_ticks: secs(240),
            cooldown_ticks: secs(240),
            blind: false,
            requires: None,
        }),
        power_levels: &[
            Power {
                name: "Plasma Bomb",
                kind: PowerKind::Barrage {
                    shells: 1,
                    delay_ticks: secs(3),
                    every_ticks: g2_sim::TICKS_PER_SECOND / 10,
                    damage: 200,
                    bonus: [0, 0, 0, 0],
                    radius: Fx::from_int(30),
                },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
            Power {
                name: "Plasma Bomb",
                kind: PowerKind::Barrage {
                    shells: 1,
                    delay_ticks: secs(3),
                    every_ticks: g2_sim::TICKS_PER_SECOND / 10,
                    damage: 200,
                    bonus: [0, 0, 0, 0],
                    radius: Fx::from_int(35),
                },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
        ],
        units: &[
            GeneralUnit { unit: EU_FOX, replaces: None, at: EU_FACTORY },
            GeneralUnit { unit: EU_ASSAULT_APC, replaces: Some(EU_APC), at: EU_FACTORY },
            GeneralUnit { unit: EU_LONGBOW, replaces: None, at: EU_AIRFIELD },
            GeneralUnit { unit: EU_RAILGUN_TURRET, replaces: Some(EU_GUN_TURRET), at: EU_DOZER },
            GeneralUnit { unit: EU_RAILGUN_TANK, replaces: None, at: EU_FACTORY },
        ],
        perks: &[
            CL_COOLANT,
            CL_PLASMA_CORE,
            CL_PROTECTOR_I,
            CL_PROTECTOR_II,
            CL_FACTORY_OVERTIME,
            CL_INSOURCED,
            CL_HEAVY_PLATING,
            CL_SLUGS,
        ],
    },
    // src: the APA flame general (the wiki's Inferno: Dragon Tank, Inferno Squad, Napalm Mortar Squad, Medium Rocket Tank; the alpha's APA_FlameGeneral): the Napalm Mortar Squad, the Dragon Tank in place of the Warlord, the Rocket Tank in place of the Gatling Tank; power Inferno Artillery (radius 20/20/25, recharge 240 s, 3 s).
    // Levels 2 and 3 use the alpha's lvl02/03 numbers where they differ, else placeholder
    // steps; the damage of every power is a placeholder (the alpha's are effect assets).
    General {
        name: "Inferno",
        faction: Faction::Apa,
        modifiers: &[],
        power: Some(Power {
            name: "Inferno Artillery",
            kind: PowerKind::Barrage {
                shells: 6,
                delay_ticks: secs(3),
                every_ticks: 5 * g2_sim::TICKS_PER_SECOND / 10,
                damage: 70,
                bonus: [30, 0, 0, 0],
                radius: Fx::from_int(7),
            },
            first_ticks: secs(240),
            cooldown_ticks: secs(240),
            blind: false,
            requires: None,
        }),
        power_levels: &[
            Power {
                name: "Inferno Artillery",
                kind: PowerKind::Barrage {
                    shells: 6,
                    delay_ticks: secs(3),
                    every_ticks: 5 * g2_sim::TICKS_PER_SECOND / 10,
                    damage: 85,
                    bonus: [30, 0, 0, 0],
                    radius: Fx::from_int(7),
                },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
            Power {
                name: "Inferno Artillery",
                kind: PowerKind::Barrage {
                    shells: 8,
                    delay_ticks: secs(3),
                    every_ticks: 5 * g2_sim::TICKS_PER_SECOND / 10,
                    damage: 85,
                    bonus: [30, 0, 0, 0],
                    radius: Fx::from_int(9),
                },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
        ],
        units: &[
            GeneralUnit { unit: APA_NAPALM_MORTAR, replaces: None, at: APA_BARRACKS },
            GeneralUnit { unit: APA_DRAGON, replaces: Some(APA_WARLORD), at: APA_FACTORY },
            GeneralUnit { unit: APA_ROCKET_TANK, replaces: Some(APA_GATLING), at: APA_FACTORY },
        ],
        perks: &[
            IN_DOUBLE_SHIFTS,
            IN_INFERNAL_DIESEL,
            IN_PROTECTOR_I,
            IN_PROTECTOR_II,
            IN_FACTORY_OVERTIME,
            IN_IGNITION,
            IN_INSOURCED,
            IN_LEADERSHIP,
        ],
    },
    // src: the APA rocket general (the wiki's Red Arrow: Rocket Soldier, Tank Hunter; the alpha's APA_RocketGeneral): the Rocket Tank in place of the Gatling Tank, the Napalm Mortar Squad; power Thunderclap (100 damage, radius 20, recharge 240 s, 5 s before activation).
    // Levels 2 and 3 use the alpha's lvl02/03 numbers where they differ, else placeholder
    // steps; the damage of every power is a placeholder (the alpha's are effect assets).
    General {
        name: "Red Arrow",
        faction: Faction::Apa,
        modifiers: &[],
        power: Some(Power {
            name: "Thunderclap",
            kind: PowerKind::Barrage {
                shells: 8,
                delay_ticks: secs(5),
                every_ticks: 2 * g2_sim::TICKS_PER_SECOND / 10,
                damage: 100,
                bonus: [0, 0, 0, 0],
                radius: Fx::from_int(6),
            },
            first_ticks: secs(240),
            cooldown_ticks: secs(240),
            blind: false,
            requires: None,
        }),
        power_levels: &[
            Power {
                name: "Thunderclap",
                kind: PowerKind::Barrage {
                    shells: 10,
                    delay_ticks: secs(5),
                    every_ticks: 2 * g2_sim::TICKS_PER_SECOND / 10,
                    damage: 100,
                    bonus: [0, 0, 0, 0],
                    radius: Fx::from_int(6),
                },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
            Power {
                name: "Thunderclap",
                kind: PowerKind::Barrage {
                    shells: 12,
                    delay_ticks: secs(5),
                    every_ticks: 2 * g2_sim::TICKS_PER_SECOND / 10,
                    damage: 100,
                    bonus: [0, 0, 0, 0],
                    radius: Fx::from_int(6),
                },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
        ],
        units: &[
            GeneralUnit { unit: APA_ROCKET_TANK, replaces: Some(APA_GATLING), at: APA_FACTORY },
            GeneralUnit { unit: APA_NAPALM_MORTAR, replaces: None, at: APA_BARRACKS },
        ],
        perks: &[
            RA_SALVO_TIMING,
            RA_WARHEADS,
            RA_PROTECTOR_I,
            RA_PROTECTOR_II,
            RA_TANK_HUNTERS,
            RA_FACTORY_OVERTIME,
            RA_INSOURCED,
            RA_GUIDANCE,
        ],
    },
    // src: the APA nuclear general (the wiki's 1000 Suns: Sunstorm, Shadow Tank; the alpha's APA_AtomicGeneral): the Rocket Tank, the Sunstorm in place of the Thunderfist, the Shadow Tank; power Neutron Bomb (radius 20, recharge 240 s, 2 s before activation).
    // Levels 2 and 3 use the alpha's lvl02/03 numbers where they differ, else placeholder
    // steps; the damage of every power is a placeholder (the alpha's are effect assets).
    General {
        name: "1000 Suns",
        faction: Faction::Apa,
        modifiers: &[],
        power: Some(Power {
            name: "Neutron Bomb",
            kind: PowerKind::Barrage {
                shells: 1,
                delay_ticks: secs(2),
                every_ticks: g2_sim::TICKS_PER_SECOND / 10,
                damage: 260,
                bonus: [100, 0, 0, 0],
                radius: Fx::from_int(20),
            },
            first_ticks: secs(240),
            cooldown_ticks: secs(240),
            blind: false,
            requires: None,
        }),
        power_levels: &[
            Power {
                name: "Neutron Bomb",
                kind: PowerKind::Barrage {
                    shells: 1,
                    delay_ticks: secs(2),
                    every_ticks: g2_sim::TICKS_PER_SECOND / 10,
                    damage: 325,
                    bonus: [100, 0, 0, 0],
                    radius: Fx::from_int(20),
                },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
            Power {
                name: "Neutron Bomb",
                kind: PowerKind::Barrage {
                    shells: 1,
                    delay_ticks: secs(2),
                    every_ticks: g2_sim::TICKS_PER_SECOND / 10,
                    damage: 400,
                    bonus: [100, 0, 0, 0],
                    radius: Fx::from_int(20),
                },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
        ],
        units: &[
            GeneralUnit { unit: APA_ROCKET_TANK, replaces: None, at: APA_FACTORY },
            GeneralUnit { unit: APA_SUNSTORM, replaces: Some(APA_THUNDERFIST), at: APA_FACTORY },
            GeneralUnit { unit: APA_SHADOW_TANK, replaces: None, at: APA_FACTORY },
        ],
        perks: &[
            TS_FISSION,
            TS_ENRICHED,
            TS_PROTECTOR_I,
            TS_PROTECTOR_II,
            TS_FACTORY_OVERTIME,
            TS_URANIUM,
            TS_INSOURCED,
            TS_LEADERSHIP,
        ],
    },
    // src: the APA espionage general (the wiki's Blackjack: Saboteurs, Shadow Tank, Urban Assault Squad; the alpha's APA_EspionageGeneral): the Urban Assault Squad in place of the Red Guard, the Dragon Tank, the Shadow Tank in place of the Warlord, the Saboteurs; power Misdirection (radius 15, recharge 240 s; here a cloak over the point).
    // Levels 2 and 3 use the alpha's lvl02/03 numbers where they differ, else placeholder
    // steps; the damage of every power is a placeholder (the alpha's are effect assets).
    General {
        name: "Blackjack",
        faction: Faction::Apa,
        modifiers: &[],
        power: Some(Power {
            name: "Misdirection",
            kind: PowerKind::Cloak { radius: Fx::from_int(15), ticks: secs(20) },
            first_ticks: secs(240),
            cooldown_ticks: secs(240),
            blind: false,
            requires: None,
        }),
        power_levels: &[
            Power {
                name: "Misdirection",
                kind: PowerKind::Cloak { radius: Fx::from_int(15), ticks: secs(25) },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
            Power {
                name: "Misdirection",
                kind: PowerKind::Cloak { radius: Fx::from_int(20), ticks: secs(30) },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
        ],
        units: &[
            GeneralUnit { unit: APA_URBAN_ASSAULT, replaces: Some(APA_RED_GUARD), at: APA_BARRACKS },
            GeneralUnit { unit: APA_DRAGON, replaces: None, at: APA_FACTORY },
            GeneralUnit { unit: APA_SHADOW_TANK, replaces: Some(APA_WARLORD), at: APA_FACTORY },
            GeneralUnit { unit: APA_SABOTEURS, replaces: None, at: APA_BARRACKS },
        ],
        perks: &[
            BJ_QUICK_HANDS,
            BJ_DEEP_COVER,
            BJ_PROTECTOR_I,
            BJ_PROTECTOR_II,
            BJ_POPULATION,
            BJ_FACTORY_OVERTIME,
            BJ_STICKY,
            BJ_LEADERSHIP,
        ],
    },
    // src: the GLA toxin general (the wiki's Dr. Thrax: Mobile Chemist, Toxin Tank; the alpha's GLA_ToxinGeneral): the Toxin Tank in place of the Scorpion, the Mobile Chemist in place of the Technical, the Bomb Truck; power Toxin Bomb (radius 20/25/30, recharge 240 s): a toxin cloud.
    // Levels 2 and 3 use the alpha's lvl02/03 numbers where they differ, else placeholder
    // steps; the damage of every power is a placeholder (the alpha's are effect assets).
    General {
        name: "Thrax",
        faction: Faction::Gla,
        modifiers: &[],
        power: Some(Power {
            name: "Toxin Bomb",
            kind: PowerKind::Missiles {
                missiles: 1,
                delay_ticks: secs(1),
                every_ticks: 1,
                speed: Fx::from_int(4),
                spread: Fx::ZERO,
                damage: 60,
                radius: Fx::from_int(20),
                cloud_radius: Fx::from_int(20),
                cloud_damage: 15,
                cloud_ticks: secs(9),
            },
            first_ticks: secs(240),
            cooldown_ticks: secs(240),
            blind: false,
            requires: None,
        }),
        power_levels: &[
            Power {
                name: "Toxin Bomb",
                kind: PowerKind::Missiles {
                    missiles: 1,
                    delay_ticks: secs(1),
                    every_ticks: 1,
                    speed: Fx::from_int(4),
                    spread: Fx::ZERO,
                    damage: 60,
                    radius: Fx::from_int(25),
                    cloud_radius: Fx::from_int(25),
                    cloud_damage: 15,
                    cloud_ticks: secs(9),
                },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
            Power {
                name: "Toxin Bomb",
                kind: PowerKind::Missiles {
                    missiles: 1,
                    delay_ticks: secs(1),
                    every_ticks: 1,
                    speed: Fx::from_int(4),
                    spread: Fx::ZERO,
                    damage: 60,
                    radius: Fx::from_int(30),
                    cloud_radius: Fx::from_int(30),
                    cloud_damage: 15,
                    cloud_ticks: secs(9),
                },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
        ],
        units: &[
            GeneralUnit { unit: GLA_TOXIN_TANK, replaces: Some(GLA_TANK), at: GLA_FACTORY },
            GeneralUnit { unit: GLA_MOBILE_CHEMIST, replaces: Some(GLA_TECHNICAL), at: GLA_FACTORY },
            GeneralUnit { unit: GLA_BOMB_TRUCK, replaces: None, at: GLA_FACTORY },
        ],
        perks: &[
            TX_CATALYST,
            TX_CONCENTRATE,
            TX_PROTECTOR_I,
            TX_PROTECTOR_II,
            TX_FACTORY_OVERTIME,
            TX_ANTHRAX,
            TX_INSOURCED,
            TX_LEADERSHIP,
        ],
    },
    // src: the GLA marauder general (the wiki's Wild Dog: Marauder Mk. II; the alpha's GLA_MarauderGeneral): the Shrapnel Tank in place of the Scorpion; power Mass RPG Strike (radius 35, recharge 240 s, 1 s).
    // Levels 2 and 3 use the alpha's lvl02/03 numbers where they differ, else placeholder
    // steps; the damage of every power is a placeholder (the alpha's are effect assets).
    General {
        name: "Wild Dog",
        faction: Faction::Gla,
        modifiers: &[],
        power: Some(Power {
            name: "Mass RPG Strike",
            kind: PowerKind::Barrage {
                shells: 10,
                delay_ticks: secs(1),
                every_ticks: 2 * g2_sim::TICKS_PER_SECOND / 10,
                damage: 60,
                bonus: [0, 0, 40, 0],
                radius: Fx::from_int(5),
            },
            first_ticks: secs(240),
            cooldown_ticks: secs(240),
            blind: false,
            requires: None,
        }),
        power_levels: &[
            Power {
                name: "Mass RPG Strike",
                kind: PowerKind::Barrage {
                    shells: 13,
                    delay_ticks: secs(1),
                    every_ticks: 2 * g2_sim::TICKS_PER_SECOND / 10,
                    damage: 60,
                    bonus: [0, 0, 40, 0],
                    radius: Fx::from_int(5),
                },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
            Power {
                name: "Mass RPG Strike",
                kind: PowerKind::Barrage {
                    shells: 16,
                    delay_ticks: secs(1),
                    every_ticks: 2 * g2_sim::TICKS_PER_SECOND / 10,
                    damage: 60,
                    bonus: [0, 0, 40, 0],
                    radius: Fx::from_int(5),
                },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
        ],
        units: &[GeneralUnit { unit: GLA_SHRAPNEL_TANK, replaces: Some(GLA_TANK), at: GLA_FACTORY }],
        perks: &[
            WD_QUICK_RELOAD,
            WD_SHAPED_CHARGES,
            WD_PROTECTOR_I,
            WD_PROTECTOR_II,
            WD_FACTORY_OVERTIME,
            WD_JUNK_ARMOR,
            WD_INSOURCED,
            WD_LEADERSHIP,
        ],
    },
    // src: the GLA engineer general (the alpha's GLA_EngineerGeneral; the wiki lists Kitbash among the GLA generals, the name a guess from the role): the Black Market Artillery; power Stealth Field (radius 25, recharge 240 s): a cloak over the point.
    // Levels 2 and 3 use the alpha's lvl02/03 numbers where they differ, else placeholder
    // steps; the damage of every power is a placeholder (the alpha's are effect assets).
    General {
        name: "Kitbash",
        faction: Faction::Gla,
        modifiers: &[],
        power: Some(Power {
            name: "Stealth Field",
            kind: PowerKind::Cloak { radius: Fx::from_int(25), ticks: secs(20) },
            first_ticks: secs(240),
            cooldown_ticks: secs(240),
            blind: false,
            requires: None,
        }),
        power_levels: &[
            Power {
                name: "Stealth Field",
                kind: PowerKind::Cloak { radius: Fx::from_int(25), ticks: secs(25) },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
            Power {
                name: "Stealth Field",
                kind: PowerKind::Cloak { radius: Fx::from_int(30), ticks: secs(30) },
                first_ticks: secs(240),
                cooldown_ticks: secs(240),
                blind: false,
                requires: None,
            },
        ],
        units: &[GeneralUnit { unit: GLA_BLACK_MARKET_ARTILLERY, replaces: None, at: GLA_FACTORY }],
        perks: &[
            KB_SCRAP_PARTS,
            KB_LONG_FIELD,
            KB_PROTECTOR_I,
            KB_PROTECTOR_II,
            KB_FACTORY_OVERTIME,
            KB_JUNK_REPAIR,
            KB_INSOURCED,
            KB_LEADERSHIP,
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
        decks: Vec::new(),
        civilians: Vec::new(),
    }
}

pub fn map_by_name(name: &str) -> Option<MapDef> {
    (name == "Crater Ridge").then(crater_ridge)
}

#[cfg(test)]
mod tests {
    use super::*;
    use g2_sim::defs::GeneralId;

    #[test]
    fn perk_ids_match_each_generals_list() {
        let r = rules();
        let mut seen = Vec::new();
        for g in r.generals {
            assert_eq!(g.perks.len(), 8, "{}: eight perks", g.name);
            for (slot, &k) in g.perks.iter().enumerate() {
                assert_eq!(k.0 as usize, seen.len(), "{} perk {slot} out of table order", g.name);
                seen.push(k);
                let perk = r.perk(k);
                assert!((1..=5).contains(&perk.rank), "{}: rank {}", perk.name, perk.rank);
                if let Some(lower) = perk.requires {
                    assert!(g.perks.contains(&lower), "{}: lower tier from another general", perk.name);
                    assert!(r.perk(lower).rank < perk.rank, "{}: lower tier ranks below it", perk.name);
                }
            }
        }
        assert_eq!(seen.len(), r.perks.len(), "every perk belongs to one general");
    }

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
            (SHANTY, "Shanty"),
            (TOWN_HOUSE, "Town House"),
            (URBAN_BLOCK, "Urban Block"),
            (EU_GUN_TURRET, "EU Gun Turret"),
            (APA_FLAME_TURRET, "APA Flame Turret"),
            (GLA_ROCKET_SITE, "GLA Rocket Site"),
            (EU_BOMBER, "EU Bomber"),
            (APA_BOMBER, "APA Bomber"),
            (APA_FIGHTER, "APA Fighter"),
            (GLA_SCUD_STORM, "GLA Scud Storm"),
            (EU_FOX, "EU Fox LRV"),
            (EU_LONGBOW, "EU Longbow"),
            (EU_SNIPERS, "EU Sniper Team"),
            (EU_ASSAULT_APC, "EU Assault APC"),
            (EU_SHOCK_TROOPERS, "EU Shock Troopers"),
            (EU_LASER_TANK, "EU Heavy Assault Laser"),
            (EU_RAILGUN_TANK, "EU Railgun Tank"),
            (EU_RAILGUN_TURRET, "EU Railgun Turret"),
            (APA_INFERNO, "APA Inferno Squad"),
            (APA_NAPALM_MORTAR, "APA Napalm Mortar Squad"),
            (APA_DRAGON, "APA Dragon Tank"),
            (APA_ROCKET_TANK, "APA Rocket Tank"),
            (APA_SABOTEURS, "APA Saboteurs"),
            (APA_SUNSTORM, "APA Sunstorm"),
            (APA_SHADOW_TANK, "APA Shadow Tank"),
            (APA_URBAN_ASSAULT, "APA Urban Assault Squad"),
            (GLA_BATTLE_BUS, "GLA Battle Bus"),
            (GLA_BLACK_MARKET_ARTILLERY, "GLA Black Market Artillery"),
            (GLA_SHRAPNEL_TANK, "GLA Shrapnel Tank"),
            (GLA_TOXIN_TANK, "GLA Toxin Tank"),
            (GLA_MOBILE_CHEMIST, "GLA Mobile Chemist"),
            (GLA_BOMB_TRUCK, "GLA Bomb Truck"),
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

    /// A perk worded for all of a faction's infantry, vehicles or units also covers the units
    /// its generals add: a general's Leadership or Protector reaches the general's own units.
    #[test]
    fn faction_wide_perks_cover_generals_units() {
        let r = rules();
        let infantry = |d: DefId| r.def(d).armour == Armour::Infantry;
        let modifier_def = |m: &Modifier| match *m {
            Modifier::Cost { def, .. } | Modifier::BuildTime { def, .. } => def,
        };
        for g in r.generals {
            for u in g.units.iter().map(|x| x.unit).filter(|&u| r.def(u).is_unit()) {
                let tables: Vec<(&str, Vec<DefId>)> = match g.faction {
                    Faction::Eu => vec![
                        ("Protector", EU_INFANTRY_10.iter().map(|&(d, _)| d).collect()),
                        ("Overtime", EU_VEHICLES_FAST.iter().map(modifier_def).collect()),
                    ],
                    Faction::Apa => vec![
                        ("Protector", APA_INFANTRY_10.iter().map(|&(d, _)| d).collect()),
                        ("Overtime", APA_VEHICLES_FAST.iter().map(modifier_def).collect()),
                    ],
                    Faction::Gla => vec![
                        ("Protector", GLA_INFANTRY_10.iter().map(|&(d, _)| d).collect()),
                        ("Overtime", GLA_VEHICLES_FAST.iter().map(modifier_def).collect()),
                        ("Leadership", GLA_LEADERSHIP.iter().map(|&(d, _)| d).collect()),
                        ("Scavenger", GLA_SCAVENGE.iter().map(|&(d, _)| d).collect()),
                    ],
                };
                for (name, defs) in tables {
                    let applies = match name {
                        "Protector" => infantry(u),
                        "Overtime" => !infantry(u) && !r.def(u).flies(),
                        _ => true,
                    };
                    assert_eq!(defs.contains(&u), applies, "{}: {} in {name}", g.name, r.def(u).name);
                }
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

    /// Every general's units are its faction's, offered by one of its faction's buildings or
    /// constructors that has the replaced unit in its own list; no two entries offer the same
    /// unit at the same place, and no general replaces a constructor or collector.
    #[test]
    fn generals_units_fit_their_factions() {
        let r = rules();
        for (gi, g) in r.generals.iter().enumerate() {
            for (k, x) in g.units.iter().enumerate() {
                let (u, at) = (r.def(x.unit), r.def(x.at));
                assert_eq!(u.faction, Some(g.faction), "{}: {} isn't theirs", g.name, u.name);
                assert_eq!(at.faction, Some(g.faction), "{}: {} isn't theirs", g.name, at.name);
                let own = if at.is_constructor() { at.builds } else { at.produces };
                assert!(!own.is_empty(), "{}: {} makes nothing", g.name, at.name);
                if let Some(old) = x.replaces {
                    assert!(own.contains(&old), "{}: {} doesn't make {}", g.name, at.name, r.def(old).name);
                    assert!(!r.def(old).is_constructor() && r.def(old).collector().is_none());
                }
                assert!(
                    !g.units[..k].iter().any(|y| y.unit == x.unit && y.at == x.at),
                    "{}: {} listed twice",
                    g.name,
                    u.name
                );
                let offered = r.offers_for(Some(GeneralId(gi as u16)), x.at);
                assert!(offered.contains(&x.unit), "{}: {} not offered", g.name, u.name);
                assert!(x.replaces.is_none_or(|old| !offered.contains(&old)), "{}: {} still offered", g.name, u.name);
            }
        }
    }

    /// Each general's power has two upgraded levels, the same kind as level 1, each at least as
    /// strong and no slower to recharge.
    #[test]
    fn power_levels_grow() {
        let strength = |k: &PowerKind| match *k {
            PowerKind::Barrage { shells, damage, radius, .. } => (shells as i64) * damage as i64 * radius.0,
            PowerKind::Repair { radius, hp } => hp as i64 * radius.0,
            PowerKind::Ambush { count, .. } => count as i64,
            PowerKind::Airstrike { jets, bombs, damage, radius, .. } => {
                (jets * bombs) as i64 * damage as i64 * radius.0
            }
            PowerKind::Recon { .. } => 1,
            PowerKind::Missiles { missiles, damage, radius, cloud_radius, .. } => {
                missiles as i64 * damage as i64 * radius.0 + cloud_radius.0
            }
            PowerKind::Cloak { radius, ticks } => ticks as i64 * radius.0,
        };
        for g in rules().generals {
            let first = g.power.unwrap_or_else(|| panic!("{}: no power", g.name));
            assert_eq!(g.power_levels.len(), 2, "{}: three levels", g.name);
            let mut prev = first;
            for &lv in g.power_levels {
                assert_eq!(std::mem::discriminant(&lv.kind), std::mem::discriminant(&first.kind), "{}", g.name);
                assert!(strength(&lv.kind) >= strength(&prev.kind), "{}: a level is weaker", g.name);
                assert!(lv.cooldown_ticks <= prev.cooldown_ticks, "{}: a level recharges slower", g.name);
                prev = lv;
            }
        }
    }
}
