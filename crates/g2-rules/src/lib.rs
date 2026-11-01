//! The rules of the three factions (the data files in `data/`, read by `data`), the campaign
//! and the built-in map.
use std::sync::OnceLock;

use g2_sim::defs::Rules;
use g2_sim::{Fx, MapDef, Vec2};

pub mod data;
pub mod missions;

/// Declares each id constant and `KEYS`, every constant's name with its id. The data files
/// name entries by these names in lower case (`EU_CC` is `eu_cc`); loading puts each at its id,
/// so code that names one finds it, and puts what a mod adds after them.
macro_rules! keyed_ids {
    ($ty:ident { $($(#[$doc:meta])* $name:ident = $n:literal,)* }) => {
        $($(#[$doc])* pub const $name: $ty = $ty($n);)*
        /// Every constant above by name, in id order.
        pub const KEYS: &[(&str, $ty)] = &[$((stringify!($name), $name)),*];
    };
}

/// Stable ids into `rules().defs`; `tests::id_constants_match_table_order` keeps them in sync.
pub mod ids {
    use g2_sim::DefId;
    keyed_ids!(DefId {
        SUPPLY_PILE = 0,
        OIL_WELL = 1,
        EU_CC = 2,
        EU_DOZER = 3,
        EU_SUPPLY = 4,
        EU_HARVESTER = 5,
        EU_POWER = 6,
        EU_DERRICK = 7,
        APA_CC = 8,
        APA_CRANE = 9,
        APA_SUPPLY = 10,
        APA_HARVESTER = 11,
        APA_POWER = 12,
        APA_DERRICK = 13,
        GLA_CC = 14,
        GLA_WORKER = 15,
        GLA_STASH = 16,
        GLA_DERRICK = 17,
        EU_BARRACKS = 18,
        EU_FACTORY = 19,
        EU_RECON = 20,
        EU_AEGIS = 21,
        APA_BARRACKS = 22,
        APA_FACTORY = 23,
        APA_RED_GUARD = 24,
        APA_WARLORD = 25,
        GLA_BARRACKS = 26,
        GLA_FACTORY = 27,
        GLA_RABBLE = 28,
        GLA_TANK = 29,
        APA_THUNDERFIST = 30,
        GLA_TOXIN_ARTILLERY = 31,
        EU_ARTILLERY = 32,
        EU_ROCKETS = 33,
        APA_TANK_HUNTER = 34,
        GLA_RPG = 35,
        EU_APC = 36,
        APA_GATLING = 37,
        GLA_TECHNICAL = 38,
        EU_TECH_LAB = 39,
        EU_ADVANCED_LAB = 40,
        GLA_SALVAGE_BAY = 41,
        EU_AIRFIELD = 42,
        APA_AIRFIELD = 43,
        EU_VALKYRIE = 44,
        APA_MANTIS = 45,
        GLA_TOXIN_CHOPPER = 46,
        SHANTY = 47,
        TOWN_HOUSE = 48,
        URBAN_BLOCK = 49,
        EU_GUN_TURRET = 50,
        APA_FLAME_TURRET = 51,
        GLA_ROCKET_SITE = 52,
        EU_BOMBER = 53,
        APA_BOMBER = 54,
        APA_FIGHTER = 55,
        GLA_SCUD_STORM = 56,
        EU_FOX = 57,
        EU_LONGBOW = 58,
        EU_SNIPERS = 59,
        EU_ASSAULT_APC = 60,
        EU_SHOCK_TROOPERS = 61,
        EU_LASER_TANK = 62,
        EU_RAILGUN_TANK = 63,
        EU_RAILGUN_TURRET = 64,
        APA_INFERNO = 65,
        APA_NAPALM_MORTAR = 66,
        APA_DRAGON = 67,
        APA_ROCKET_TANK = 68,
        APA_SABOTEURS = 69,
        APA_SUNSTORM = 70,
        APA_SHADOW_TANK = 71,
        APA_URBAN_ASSAULT = 72,
        GLA_BATTLE_BUS = 73,
        GLA_BLACK_MARKET_ARTILLERY = 74,
        GLA_SHRAPNEL_TANK = 75,
        GLA_TOXIN_TANK = 76,
        GLA_MOBILE_CHEMIST = 77,
        GLA_BOMB_TRUCK = 78,
        GLA_BATTLE_BUNKER = 79,
        EU_SPY_DRONE = 80,
        APA_SORTIE_FIGHTER = 81,
        GLA_TUNNEL = 82,
        EU_COMMANDO = 83,
        APA_COMMANDO = 84,
        GLA_AGENT = 85,
    });
}

/// Stable ids into `rules().upgrades`.
pub mod upgrades {
    use g2_sim::defs::UpgradeId;
    keyed_ids!(UpgradeId {
        HIGHTECH_ALLOYS = 0,
        IMPROVED_BALLISTICS = 1,
        SENIOR_WARFIGHTER = 2,
        HIGH_EXPLOSIVES = 3,
        HOLLOW_POINT = 4,
        POPULATION_CONTROL = 5,
        SHORTENED_FUSES = 6,
        SCRAP_REPAIR = 7,
        FIRESTORM_SHELLS = 8,
    });
}

/// Stable ids into `rules().generals`.
pub mod generals {
    use g2_sim::defs::GeneralId;
    keyed_ids!(GeneralId {
        BULLDOG = 0,
        TASKMASTER = 1,
        JUNKYARD = 2,
        DAEDALUS = 3,
        MERCURY_RED = 4,
        CUTTING_EDGE = 5,
        CLEAVER = 6,
        INFERNO = 7,
        RED_ARROW = 8,
        THOUSAND_SUNS = 9,
        BLACKJACK = 10,
        THRAX = 11,
        WILD_DOG = 12,
        KITBASH = 13,
    });
}

/// Stable ids into `rules().perks`: eight per general, in each general's list order, then the
/// perks that add a power (appended, so a saved profile's ids keep their meaning).
pub mod perks {
    use g2_sim::defs::PerkId;
    keyed_ids!(PerkId {
        // Bulldog (EU).
        BD_DOUBLE_SHIFTS = 0,
        BD_HEAVY_PAYLOAD = 1,
        BD_PROTECTOR_I = 2,
        BD_PROTECTOR_II = 3,
        BD_FACTORY_OVERTIME = 4,
        BD_INSOURCED = 5,
        BD_AERODYNAMICS_I = 6,
        BD_AERODYNAMICS_II = 7,
        // Taskmaster (APA).
        TM_SPY_EFFICIENCY = 8,
        TM_LOGISTICS = 9,
        TM_PROTECTOR_I = 10,
        TM_PROTECTOR_II = 11,
        TM_FACTORY_OVERTIME = 12,
        TM_VULCAN_BARRELS = 13,
        TM_KANBAN = 14,
        TM_COMMAND_I = 15,
        // Junkyard (GLA).
        JY_DISCIPLINE = 16,
        JY_SCAVENGER = 17,
        JY_LEVERAGE = 18,
        JY_INSOURCED = 19,
        JY_PROTECTOR_I = 20,
        JY_PROTECTOR_II = 21,
        JY_FACTORY_OVERTIME = 22,
        JY_LEADERSHIP = 23,
        // Daedalus (EU).
        DD_TAILWIND = 24,
        DD_HEAVY_BOMBS = 25,
        DD_PROTECTOR_I = 26,
        DD_PROTECTOR_II = 27,
        DD_KANBAN = 28,
        DD_AERODYNAMICS_I = 29,
        DD_AERODYNAMICS_II = 30,
        DD_INSOURCED = 31,
        // Mercury Red (EU).
        MR_DOUBLE_TIME = 32,
        MR_FIELD_MEDICS = 33,
        MR_PROTECTOR_I = 34,
        MR_PROTECTOR_II = 35,
        MR_RAPID_DEPLOYMENT = 36,
        MR_FACTORY_OVERTIME = 37,
        MR_INSOURCED = 38,
        MR_LEADERSHIP = 39,
        // Cutting Edge (EU).
        CE_CAPACITORS = 40,
        CE_FOCUSING = 41,
        CE_PROTECTOR_I = 42,
        CE_PROTECTOR_II = 43,
        CE_FACTORY_OVERTIME = 44,
        CE_REINFORCED = 45,
        CE_INSOURCED = 46,
        CE_OVERCHARGE = 47,
        // Cleaver (EU).
        CL_COOLANT = 48,
        CL_PLASMA_CORE = 49,
        CL_PROTECTOR_I = 50,
        CL_PROTECTOR_II = 51,
        CL_FACTORY_OVERTIME = 52,
        CL_INSOURCED = 53,
        CL_HEAVY_PLATING = 54,
        CL_SLUGS = 55,
        // Inferno (APA).
        IN_DOUBLE_SHIFTS = 56,
        IN_INFERNAL_DIESEL = 57,
        IN_PROTECTOR_I = 58,
        IN_PROTECTOR_II = 59,
        IN_FACTORY_OVERTIME = 60,
        IN_WILDFIRE = 61,
        IN_INSOURCED = 62,
        IN_LEADERSHIP = 63,
        // Red Arrow (APA).
        RA_SALVO_TIMING = 64,
        RA_WARHEADS = 65,
        RA_PROTECTOR_I = 66,
        RA_PROTECTOR_II = 67,
        RA_TANK_HUNTERS = 68,
        RA_FACTORY_OVERTIME = 69,
        RA_INSOURCED = 70,
        RA_GUIDANCE = 71,
        // 1000 Suns (APA).
        TS_FISSION = 72,
        TS_ENRICHED = 73,
        TS_PROTECTOR_I = 74,
        TS_PROTECTOR_II = 75,
        TS_FACTORY_OVERTIME = 76,
        TS_URANIUM = 77,
        TS_INSOURCED = 78,
        TS_LEADERSHIP = 79,
        // Blackjack (APA).
        BJ_QUICK_HANDS = 80,
        BJ_DEEP_COVER = 81,
        BJ_PROTECTOR_I = 82,
        BJ_PROTECTOR_II = 83,
        BJ_POPULATION = 84,
        BJ_FACTORY_OVERTIME = 85,
        BJ_STICKY = 86,
        BJ_LEADERSHIP = 87,
        // Thrax (GLA).
        TX_CATALYST = 88,
        TX_CONCENTRATE = 89,
        TX_PROTECTOR_I = 90,
        TX_PROTECTOR_II = 91,
        TX_FACTORY_OVERTIME = 92,
        TX_ANTHRAX = 93,
        TX_INSOURCED = 94,
        TX_LEADERSHIP = 95,
        // Wild Dog (GLA).
        WD_QUICK_RELOAD = 96,
        WD_SHAPED_CHARGES = 97,
        WD_PROTECTOR_I = 98,
        WD_PROTECTOR_II = 99,
        WD_FACTORY_OVERTIME = 100,
        WD_JUNK_ARMOR = 101,
        WD_INSOURCED = 102,
        WD_LEADERSHIP = 103,
        // Kitbash (GLA).
        KB_SCRAP_PARTS = 104,
        KB_LONG_FIELD = 105,
        KB_PROTECTOR_I = 106,
        KB_PROTECTOR_II = 107,
        KB_FACTORY_OVERTIME = 108,
        KB_JUNK_REPAIR = 109,
        KB_INSOURCED = 110,
        KB_LEADERSHIP = 111,
        // Perks that add a power.
        MR_STRIKE_TEAM = 112,
        CL_RAILGUN_DROP = 113,
        TM_OFFSHORE = 114,
        BJ_SORTIE = 115,
        JY_FAULTY_CONSTRUCTION = 116,
    });
}
static RULES: OnceLock<Result<Rules, data::LoadError>> = OnceLock::new();

/// The rules: the data files embedded in the binary, or `G2_RULES_DIR`'s copies of them
/// (`docs/RULES.md`), read once. A program checks them at startup with `try_rules`; after
/// that this can't fail.
pub fn rules() -> &'static Rules {
    try_rules().unwrap_or_else(|e| panic!("bad rules file: {e}"))
}

/// The rules, or what is wrong with a file `G2_RULES_DIR` gives.
pub fn try_rules() -> Result<&'static Rules, &'static data::LoadError> {
    RULES.get_or_init(|| data::load(std::env::var_os("G2_RULES_DIR").as_deref().map(std::path::Path::new))).as_ref()
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
        bridges: Vec::new(),
        heights: Default::default(),
    }
}

pub fn map_by_name(name: &str) -> Option<MapDef> {
    (name == "Crater Ridge").then(crater_ridge)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::*;
    use g2_sim::defs::GeneralId;
    use g2_sim::defs::{Armour, Class, DefId, Faction, Modifier, PerkId, PowerKind};

    #[test]
    fn perk_ids_match_each_generals_list() {
        let r = rules();
        let mut seen = Vec::new();
        // The eight first, in table order; a general's power perk, if any, after them.
        let power_perks = r.perks.iter().filter(|p| p.adds_power.is_some()).count();
        for g in r.generals {
            let (own, extra) = g.perks.split_at(8.min(g.perks.len()));
            assert_eq!(own.len(), 8, "{}: eight perks", g.name);
            assert!(extra.iter().all(|&k| r.perk(k).adds_power.is_some()), "{}: only a power perk after", g.name);
            for (slot, &k) in own.iter().enumerate() {
                assert_eq!(k.0 as usize, seen.len(), "{} perk {slot} out of table order", g.name);
                seen.push(k);
                let perk = r.perk(k);
                assert!(perk.adds_power.is_none(), "{}: a power perk among the eight", perk.name);
                assert!((1..=5).contains(&perk.rank), "{}: rank {}", perk.name, perk.rank);
                if let Some(lower) = perk.requires {
                    assert!(g.perks.contains(&lower), "{}: lower tier from another general", perk.name);
                    assert!(r.perk(lower).rank < perk.rank, "{}: lower tier ranks below it", perk.name);
                }
            }
        }
        let extra: Vec<PerkId> = r.generals.iter().flat_map(|g| g.perks.iter().skip(8).copied()).collect();
        assert_eq!(extra.len(), power_perks, "every power perk belongs to a general");
        for (i, k) in extra.iter().enumerate() {
            assert!(!extra[..i].contains(k), "{} belongs to two generals", r.perk(*k).name);
        }
        assert_eq!(seen.len() + extra.len(), r.perks.len(), "every perk belongs to one general");
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
            (GLA_BATTLE_BUNKER, "GLA Battle Bunker"),
            (EU_SPY_DRONE, "EU Spy Drone"),
            (APA_SORTIE_FIGHTER, "APA Sortie Fighter"),
            (GLA_TUNNEL, "GLA Tunnel Network"),
            (EU_COMMANDO, "EU Commando"),
            (APA_COMMANDO, "APA Commando"),
            (GLA_AGENT, "GLA Agent"),
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
    /// The faction-wide list of a wording is the longest of the faction's perks with it (some
    /// generals' perks of the same wording cover only their own units).
    #[test]
    fn faction_wide_perks_cover_generals_units() {
        let r = rules();
        let infantry = |d: DefId| r.def(d).armour == Armour::Infantry;
        let all = [Faction::Eu, Faction::Apa, Faction::Gla];
        type Covers<'a> = &'a dyn Fn(DefId) -> bool;
        let wordings: [(&str, &[Faction], Covers); 4] = [
            ("Infantry take 10% less damage.", &all, &infantry),
            ("Vehicles build 15% faster.", &all, &|u| !infantry(u) && !r.def(u).flies()),
            ("Units gain double experience from kills.", &[Faction::Gla], &|_| true),
            ("Units regain 2 hit points a second after 5 s without being hit.", &[Faction::Gla], &|_| true),
        ];
        let defs = |perk: &g2_sim::defs::Perk| -> Vec<DefId> {
            perk.effects
                .iter()
                .map(|&(d, _)| d)
                .chain(perk.modifiers.iter().map(|m| match *m {
                    Modifier::Cost { def, .. } | Modifier::BuildTime { def, .. } => def,
                }))
                .collect()
        };
        for (wording, factions, applies) in wordings {
            for &f in factions {
                let list = r
                    .generals
                    .iter()
                    .filter(|g| g.faction == f)
                    .flat_map(|g| g.perks.iter().map(|&k| r.perk(k)))
                    .filter(|p| p.description == wording)
                    .map(defs)
                    .max_by_key(Vec::len)
                    .unwrap_or_else(|| panic!("{f:?}: no perk {wording:?}"));
                for g in r.generals.iter().filter(|g| g.faction == f) {
                    for u in g.units.iter().map(|x| x.unit).filter(|&u| r.def(u).is_unit()) {
                        assert_eq!(list.contains(&u), applies(u), "{}: {} in {wording:?}", g.name, r.def(u).name);
                    }
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
            PowerKind::Ambush { count, .. } => count as i64,
            PowerKind::Airstrike { jets, bombs, damage, radius, .. } => {
                (jets * bombs) as i64 * damage as i64 * radius.0
            }
            PowerKind::Recon { .. } => 1,
            PowerKind::Missiles { missiles, damage, radius, cloud_radius, cloud_damage, cloud_ticks, .. } => {
                missiles as i64 * damage as i64 * radius.0 + cloud_damage as i64 * cloud_ticks as i64 * cloud_radius.0
            }
            PowerKind::Cloak { radius, ticks } | PowerKind::Decoys { radius, ticks } => ticks as i64 * radius.0,
            PowerKind::Sabotage { damage } => damage as i64,
            PowerKind::Field(f) => {
                let off: i64 = f.allies.iter().chain(&f.enemies).flatten().map(|&(_, p)| (p - 100).abs() as i64).sum();
                f.radius.0 * (f.damage as i64 + f.hp as i64 + f.ranks as i64 * 100 + f.ticks as i64 * (1 + off))
            }
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
