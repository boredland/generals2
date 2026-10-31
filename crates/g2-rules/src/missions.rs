//! The campaign: eight missions in the world of the 2013 Generals 2 (the EU, the APA and the
//! GLA). The first four teach the factions and end in a finale; the next four, one for each
//! faction and a two-front finale, use the scripted enemies (`g2_sim::mission::Script`):
//! patrols, guards, waves along routes, timed reinforcements while a building stands, bases
//! that rebuild from a short list, and a seat that wakes partway through. The briefings and
//! messages are written for this project; nothing is taken from EA's games.
//!
//! Positions are world units on each map (Crater Ridge is built in; the others are the
//! original game's maps as `g2_maps` imports them). Wave sizes and timings are placeholders,
//! tuned until the `Normal` AI in the player's seat wins every mission on Easy and Normal
//! (`g2-maps/tests/campaign.rs`); the difficulty scales them from `Difficulty`.

use g2_sim::TICKS_PER_SECOND as S;
use g2_sim::defs::Faction::{Apa, Eu, Gla};
use g2_sim::map::AiLevel::{Easy, Normal};
use g2_sim::mission::{Action, Area, Force, Goal, Mission, Objective, Side, Trigger, What, When};

use crate::ids::*;

const fn f(owner: u8, def: g2_sim::DefId, x: i64, y: i64) -> Force {
    Force::new(owner, def, x, y)
}

const fn area(x: i64, y: i64, radius: i64) -> Area {
    Area { at: (x, y), radius }
}

pub static MISSIONS: [Mission; 8] = [
    // 1. EU: a small GLA cell, an Easy opponent, help on the way.
    Mission {
        key: "first-light",
        name: "First Light",
        map: "Crater Ridge",
        briefing: "Crater Ridge, 0600. A GLA cell has dug in on the far rim of the crater and is taxing \
                   every convoy that crosses it. Your forward command center came down on the near rim \
                   overnight with a dozer and a recon company. Get your supply line running, keep the \
                   command center standing, and burn out their headquarters. Brussels promises armour \
                   once you have shown you can hold the rim.",
        sides: &[
            Side { faction: Eu, credits: 5_000, ai: Some(Easy), general: None, builds: None },
            Side { faction: Gla, credits: 2_000, ai: Some(Easy), general: None, builds: None },
        ],
        forces: &[
            f(0, EU_CC, 80, 80).tagged(1),
            f(0, EU_DOZER, 80, 100),
            f(0, EU_RECON, 112, 108).times(4),
            f(0, EU_AEGIS, 100, 124).times(2),
            f(1, GLA_CC, 320, 320).tagged(2),
            f(1, GLA_WORKER, 320, 298),
            f(1, GLA_BARRACKS, 300, 352),
            f(1, GLA_ROCKET_SITE, 292, 290),
            f(1, GLA_RABBLE, 300, 300).times(3),
        ],
        objectives: &[
            Objective { text: "Destroy the GLA headquarters on the far rim.", goal: Goal::Destroy { tag: 2 } },
            Objective { text: "Keep your command center standing.", goal: Goal::Protect { tag: 1 } },
        ],
        triggers: &[
            Trigger {
                when: When::At(5 * S),
                then: &[Action::Say("Command center is down and secure. Get the harvesters running.")],
            },
            Trigger {
                when: When::At(90 * S),
                then: &[
                    Action::Say("Rabble on the crater floor, heading for your base!"),
                    Action::Spawn { forces: &[f(1, GLA_RABBLE, 250, 250).times(3)], route: &[(100, 110)] },
                ],
            },
            Trigger {
                when: When::At(210 * S),
                then: &[
                    Action::Say("Two Aegis tanks have rolled in from the coast road. They are yours."),
                    Action::Spawn { forces: &[f(0, EU_AEGIS, 40, 120).times(2)], route: &[] },
                ],
            },
            Trigger {
                when: When::At(300 * S),
                then: &[
                    Action::Say("A supply drop has landed: 2,000 credits."),
                    Action::Credits { player: 0, amount: 2_000 },
                ],
            },
            Trigger {
                when: When::Destroyed(2),
                then: &[Action::Say("Their headquarters is burning. The rim is ours.")],
            },
        ],
    },
    // 2. APA: hold a road against four GLA waves, with a depot to protect.
    Mission {
        key: "dust-line",
        name: "Dust Line",
        map: "zh:Tournament Desert",
        briefing: "The desert road is the only way APA supplies reach the coast, and the GLA means to cut \
                   it. Our depot in the south must not fall. Raiders will come down from the northern \
                   oasis in waves, each larger than the last. Hold for nine minutes, until the main \
                   column is through. Warlord tanks are on their way from the depot at Qaryat; they will \
                   reach you in five.",
        sides: &[
            Side { faction: Apa, credits: 5_000, ai: Some(Easy), general: None, builds: None },
            Side { faction: Gla, credits: 0, ai: Some(Easy), general: None, builds: None },
        ],
        forces: &[
            f(0, APA_CC, 168, 99),
            f(0, APA_CRANE, 168, 122),
            f(0, APA_SUPPLY, 212, 48).tagged(1),
            f(0, APA_HARVESTER, 200, 70),
            f(0, APA_RED_GUARD, 168, 145).times(4),
            f(0, APA_GATLING, 140, 140).times(2),
        ],
        objectives: &[
            Objective { text: "Hold the road for nine minutes.", goal: Goal::Survive { ticks: 540 * S } },
            Objective { text: "The supply depot must not fall.", goal: Goal::Protect { tag: 1 } },
        ],
        triggers: &[
            Trigger {
                when: When::At(5 * S),
                then: &[Action::Say("Lookouts on the ridge. First raiders expected within two minutes.")],
            },
            Trigger {
                when: When::At(120 * S),
                then: &[
                    Action::Say("First wave: rabble and a technical, coming down the road."),
                    Action::Spawn {
                        forces: &[f(1, GLA_RABBLE, 167, 385).times(4), f(1, GLA_TECHNICAL, 190, 385)],
                        route: &[(168, 130)],
                    },
                ],
            },
            Trigger {
                when: When::At(240 * S),
                then: &[
                    Action::Say("Second wave! RPG teams with them this time."),
                    Action::Spawn {
                        forces: &[
                            f(1, GLA_RABBLE, 150, 385).times(4),
                            f(1, GLA_RPG, 185, 385).times(3),
                            f(1, GLA_TECHNICAL, 210, 370).times(2),
                        ],
                        route: &[(168, 130)],
                    },
                ],
            },
            Trigger {
                when: When::At(300 * S),
                then: &[
                    Action::Say("The Warlords from Qaryat have arrived."),
                    Action::Spawn { forces: &[f(0, APA_WARLORD, 110, 70).times(2)], route: &[] },
                ],
            },
            Trigger {
                when: When::At(390 * S),
                then: &[
                    Action::Say("Third wave: Scorpion tanks on the road!"),
                    Action::Spawn {
                        forces: &[f(1, GLA_TANK, 167, 385).times(3), f(1, GLA_RPG, 140, 375).times(3)],
                        route: &[(168, 130)],
                    },
                ],
            },
            Trigger {
                when: When::At(480 * S),
                then: &[
                    Action::Say("They are throwing everything at us. One minute to go!"),
                    Action::Spawn {
                        forces: &[
                            f(1, GLA_TANK, 167, 385).times(3),
                            f(1, GLA_RABBLE, 140, 385).times(5),
                            f(1, GLA_TECHNICAL, 200, 370).times(3),
                        ],
                        route: &[(190, 60)],
                    },
                ],
            },
            Trigger { when: When::Done(0), then: &[Action::Say("The column is through. The road held.")] },
        ],
    },
    // 3. GLA: take the pass, then raze an EU firebase that plays as `Normal`.
    Mission {
        key: "mountain-pass",
        name: "Mountain Pass",
        map: "gen:Alpine Assault",
        briefing: "The EU has built a firebase above the old mining town, and its guns cover the only \
                   pass through the mountains. Our cell in the valley is small, but the people of the \
                   town remember who feeds them. Take the pass, where the miners have hidden weapons and \
                   money for us, then tear the firebase down to the last wall. Expect their patrols to \
                   come looking for you.",
        sides: &[
            Side { faction: Gla, credits: 4_000, ai: Some(Easy), general: None, builds: None },
            Side { faction: Eu, credits: 2_000, ai: Some(Normal), general: None, builds: None },
        ],
        forces: &[
            f(0, GLA_CC, 225, 122),
            f(0, GLA_WORKER, 205, 140).times(2),
            f(0, GLA_STASH, 250, 98),
            f(0, GLA_BARRACKS, 192, 104),
            f(0, GLA_RABBLE, 225, 160).times(4),
            f(0, GLA_TECHNICAL, 250, 150).times(2),
            f(1, EU_CC, 200, 375).tagged(2),
            f(1, EU_DOZER, 200, 352),
            f(1, EU_SUPPLY, 165, 382),
            f(1, EU_POWER, 232, 392),
            f(1, EU_BARRACKS, 238, 362),
            f(1, EU_GUN_TURRET, 205, 330),
            f(1, EU_RECON, 200, 338).times(3),
        ],
        objectives: &[
            Objective {
                text: "Take the pass below the old mine.",
                goal: Goal::Reach { area: Area { at: (200, 230), radius: 30 }, tag: None },
            },
            Objective {
                text: "Destroy every building of the EU firebase.",
                goal: Goal::DestroyAll { owner: 1, what: What::Buildings },
            },
        ],
        triggers: &[
            // Measured with the `Normal` AI in the player's seat: since the EU harvester carries
            // 190 a trip, the firebase out-earns the cell, and with three Scorpions and nothing
            // more here the AI lost (at 9:26), as it did at most credit levels tried for either
            // side. With five and 2,000 credits it wins with 4,000 to 5,000 credits of its own
            // against every enemy level tried from 500 to 4,000; with 3,000 or 3,500 it wins up
            // to 3,000 enemy credits.
            Trigger {
                when: When::Done(0),
                then: &[
                    Action::Say("The miners kept their word: Scorpions, technicals and 2,000 credits at the pass."),
                    Action::Spawn {
                        forces: &[
                            f(0, GLA_TANK, 198, 227).times(3),
                            f(0, GLA_TANK, 220, 215).times(2),
                            f(0, GLA_TECHNICAL, 225, 224).times(2),
                        ],
                        route: &[],
                    },
                    Action::Credits { player: 0, amount: 2_000 },
                ],
            },
            Trigger {
                when: When::At(180 * S),
                then: &[
                    Action::Say("An EU patrol is coming down from the firebase."),
                    Action::Spawn {
                        forces: &[f(1, EU_RECON, 199, 319).times(4), f(1, EU_AEGIS, 220, 318)],
                        route: &[(220, 150)],
                    },
                ],
            },
            Trigger {
                when: When::At(420 * S),
                then: &[
                    Action::Say("EU armour is moving on the valley!"),
                    Action::Spawn {
                        forces: &[f(1, EU_AEGIS, 200, 320).times(2), f(1, EU_ROCKETS, 180, 318).times(3)],
                        route: &[(220, 150)],
                    },
                ],
            },
            Trigger {
                when: When::Destroyed(2),
                then: &[Action::Say("Their command center is gone. Finish the rest.")],
            },
        ],
    },
    // 4. EU: an APA base that plays as `Normal`, attack waves on top, and a command center to keep.
    Mission {
        key: "iron-dragon",
        name: "Iron Dragon",
        map: "zh:Tournament Desert",
        briefing: "The APA has moved a full armoured brigade into the desert under the Iron Dragon banner, \
                   and the road the GLA could not cut is now theirs. We hold the northern oasis with a \
                   base that is ready to fight. Their brigade will not wait for us: expect assaults from \
                   the south every few minutes. Break them, then destroy their headquarters and every \
                   building around it. If our command center falls, the operation is over.",
        sides: &[
            Side { faction: Eu, credits: 5_000, ai: Some(Easy), general: None, builds: None },
            // Measured with the `Normal` AI in the player's seat (the AI of main 3c6cd29, which
            // places its guns and routes constructors) and the waves walking their routes in
            // formation: 3,750 credits and the five Gatlings below is the hardest of the settings
            // tried that it still wins with 4,500, 5,000 and 5,500 credits of its own (at 13:26
            // with 5,000). Results don't grow with the enemy's strength: four Gatlings with 3,500
            // or 3,750 credits lose with 5,000, three with 3,500 lose with 5,000 and 5,500, and
            // 4,000 credits lose with any of three to six Gatlings tried.
            Side { faction: Apa, credits: 3_750, ai: Some(Normal), general: None, builds: None },
        ],
        forces: &[
            f(0, EU_CC, 167, 352).tagged(1),
            f(0, EU_DOZER, 167, 330).times(2),
            f(0, EU_SUPPLY, 125, 402),
            f(0, EU_POWER, 205, 385),
            f(0, EU_BARRACKS, 130, 345),
            f(0, EU_FACTORY, 205, 345),
            f(0, EU_AEGIS, 170, 310).times(3),
            f(0, EU_RECON, 145, 312).times(4),
            f(0, EU_ROCKETS, 195, 312).times(2),
            f(1, APA_CC, 168, 99).tagged(2),
            f(1, APA_CRANE, 168, 122),
            f(1, APA_SUPPLY, 212, 48),
            f(1, APA_POWER, 128, 72),
            f(1, APA_BARRACKS, 125, 110),
            f(1, APA_FACTORY, 212, 100),
            f(1, APA_FLAME_TURRET, 150, 140),
            f(1, APA_FLAME_TURRET, 190, 140),
            f(1, APA_RED_GUARD, 150, 160).times(4),
            f(1, APA_TANK_HUNTER, 130, 150).times(2),
            f(1, APA_WARLORD, 190, 165).times(2),
            f(1, APA_GATLING, 215, 150).times(5),
        ],
        objectives: &[
            Objective { text: "Destroy the APA headquarters.", goal: Goal::Destroy { tag: 2 } },
            Objective {
                text: "Destroy every building of the APA base.",
                goal: Goal::DestroyAll { owner: 1, what: What::Buildings },
            },
            Objective { text: "Keep your command center standing.", goal: Goal::Protect { tag: 1 } },
        ],
        triggers: &[
            Trigger {
                when: When::At(150 * S),
                then: &[
                    Action::Say("APA infantry and tank hunters, coming up the road!"),
                    Action::Spawn {
                        forces: &[f(1, APA_RED_GUARD, 167, 200).times(4), f(1, APA_TANK_HUNTER, 190, 200).times(2)],
                        route: &[(167, 330)],
                    },
                ],
            },
            Trigger {
                when: When::At(270 * S),
                then: &[
                    Action::Say("Reinforcements from Brussels: two Aegis tanks and an APC."),
                    Action::Spawn { forces: &[f(0, EU_AEGIS, 250, 405).times(2), f(0, EU_APC, 230, 410)], route: &[] },
                ],
            },
            Trigger {
                when: When::At(330 * S),
                then: &[
                    Action::Say("Warlord tanks and Gatlings, heading for the oasis!"),
                    Action::Spawn {
                        forces: &[f(1, APA_WARLORD, 167, 200).times(2), f(1, APA_GATLING, 190, 200).times(2)],
                        route: &[(167, 330)],
                    },
                ],
            },
            Trigger {
                when: When::At(540 * S),
                then: &[
                    Action::Say("The Iron Dragon itself: their heaviest assault yet!"),
                    Action::Spawn {
                        forces: &[
                            f(1, APA_DRAGON, 167, 200).times(2),
                            f(1, APA_WARLORD, 190, 200).times(2),
                            f(1, APA_INFERNO, 145, 200).times(3),
                        ],
                        route: &[(167, 330)],
                    },
                ],
            },
            Trigger {
                when: When::Destroyed(2),
                then: &[Action::Say("Their headquarters has fallen. Clear the base.")],
            },
        ],
    },
    // 5. APA: a GLA base that rebuilds from a short list, technicals patrolling the snowfield,
    //    rocket teams guarding the eastern pile, and raids while their headquarters stands.
    Mission {
        key: "frozen-watch",
        name: "Frozen Watch",
        map: "gen:Winter Wolf",
        briefing: "The pass at Winter Wolf freezes shut in a month, and a GLA cell has dug in on its \
                   southern slope to tax the last trucks through. They are poor but stubborn: their \
                   barracks will rebuild whatever you knock down, and their technicals drive a loop \
                   across the snowfield looking for stragglers. Rocket teams sit on the supply pile \
                   to the east. Every few minutes they send raiders north. Take out the headquarters \
                   and the raids stop; burn every building and the pass is ours for the winter.",
        sides: &[
            Side { faction: Apa, credits: 5_000, ai: Some(Easy), general: None, builds: None },
            Side {
                faction: Gla,
                credits: 3_000,
                ai: Some(Easy),
                general: None,
                builds: Some(&[GLA_WORKER, GLA_STASH, GLA_BARRACKS, GLA_RABBLE, GLA_RPG, GLA_ROCKET_SITE]),
            },
        ],
        forces: &[
            f(0, APA_CC, 60, 80),
            f(0, APA_CRANE, 60, 102),
            f(0, APA_SUPPLY, 92, 44),
            f(0, APA_HARVESTER, 96, 64),
            f(0, APA_POWER, 32, 100),
            f(0, APA_BARRACKS, 88, 100),
            f(0, APA_RED_GUARD, 100, 124).times(4),
            f(0, APA_WARLORD, 115, 82).times(2),
            f(0, APA_TANK_HUNTER, 70, 130).times(2),
            f(1, GLA_CC, 320, 300).tagged(2),
            f(1, GLA_WORKER, 300, 304),
            f(1, GLA_STASH, 272, 330),
            f(1, GLA_BARRACKS, 336, 265),
            f(1, GLA_ROCKET_SITE, 292, 272),
            f(1, GLA_ROCKET_SITE, 352, 300),
            f(1, GLA_ROCKET_SITE, 300, 320),
            f(1, GLA_RABBLE, 300, 284).times(4),
            f(1, GLA_RPG, 340, 320).times(3),
            f(1, GLA_TANK, 310, 250).times(3).guard(area(310, 255, 30)),
            f(1, GLA_TECHNICAL, 250, 200).times(3).tagged(3).patrol(&[(250, 200), (150, 180), (200, 110), (265, 150)]),
            f(1, GLA_RPG, 220, 105).times(4).guard(area(225, 110, 25)),
        ],
        objectives: &[
            Objective { text: "Destroy the GLA headquarters.", goal: Goal::Destroy { tag: 2 } },
            Objective {
                text: "Destroy every building of the GLA cell.",
                goal: Goal::DestroyAll { owner: 1, what: What::Buildings },
            },
        ],
        triggers: &[
            Trigger {
                when: When::At(5 * S),
                then: &[Action::Say("Technicals are circling the snowfield. Rocket teams hold the east pile.")],
            },
            Trigger {
                when: When::Every { from: 120 * S, every: 90 * S, times: 6, while_alive: Some(2) },
                then: &[
                    Action::Say("Raiders coming up from the southern slope!"),
                    Action::Spawn {
                        forces: &[
                            f(1, GLA_RABBLE, 330, 240).times(4),
                            f(1, GLA_RPG, 315, 235).times(2),
                            f(1, GLA_TECHNICAL, 345, 250).times(2),
                        ],
                        route: &[(250, 200), (150, 160), (80, 110)],
                    },
                ],
            },
            Trigger {
                when: When::Destroyed(3),
                then: &[Action::Say("Their patrol leader is down. The snowfield is quiet.")],
            },
            Trigger {
                when: When::Destroyed(2),
                then: &[Action::Say("Their headquarters is gone: no more raids. Clear the slope.")],
            },
        ],
    },
    // 6. EU: a GLA seat that stands idle, its patrol and its guards on the oil field, until the
    //    player comes close or time runs out; then it plays, and its waves take the long road.
    Mission {
        key: "ash-road",
        name: "Ash Road",
        map: "zh:Leipzig Lowlands",
        briefing: "The lowlands east of the river are quiet, and quiet is what worries Brussels. A GLA \
                   warlord has a base in the north-east and has not moved in weeks; his technicals \
                   drive the same loop every morning and rocket teams squat on the oil wells in the \
                   middle of the plain. The moment we touch the wells, or within seven minutes \
                   whatever we do, he will wake up and come at us along the ash road. Set up your base \
                   in the south-west, keep the command center standing, and leave nothing of his.",
        sides: &[
            Side { faction: Eu, credits: 5_000, ai: Some(Easy), general: None, builds: None },
            Side {
                faction: Gla,
                credits: 4_000,
                ai: None,
                general: None,
                builds: Some(&[
                    GLA_WORKER,
                    GLA_STASH,
                    GLA_BARRACKS,
                    GLA_FACTORY,
                    GLA_RABBLE,
                    GLA_RPG,
                    GLA_TECHNICAL,
                    GLA_TANK,
                    GLA_ROCKET_SITE,
                ]),
            },
        ],
        forces: &[
            f(0, EU_CC, 80, 340).tagged(1),
            f(0, EU_DOZER, 80, 315),
            f(0, EU_SUPPLY, 45, 385),
            f(0, EU_HARVESTER, 52, 362),
            f(0, EU_POWER, 110, 372),
            f(0, EU_BARRACKS, 116, 330),
            f(0, EU_AEGIS, 110, 300).times(2),
            f(0, EU_RECON, 135, 345).times(4),
            f(1, GLA_CC, 500, 80).tagged(2),
            f(1, GLA_WORKER, 480, 98),
            f(1, GLA_STASH, 535, 45),
            f(1, GLA_BARRACKS, 468, 58),
            f(1, GLA_FACTORY, 530, 112),
            f(1, GLA_ROCKET_SITE, 460, 112),
            f(1, GLA_ROCKET_SITE, 482, 132),
            f(1, GLA_ROCKET_SITE, 440, 80),
            f(1, GLA_RABBLE, 470, 100).times(4),
            f(1, GLA_TANK, 430, 140).times(5).guard(area(440, 130, 30)),
            f(1, GLA_TECHNICAL, 300, 310).times(3).tagged(3).patrol(&[(300, 310), (440, 310), (440, 170), (340, 170)]),
            f(1, GLA_RPG, 292, 190).times(4).guard(area(292, 190, 25)),
        ],
        objectives: &[
            Objective {
                text: "Destroy every building of the GLA base.",
                goal: Goal::DestroyAll { owner: 1, what: What::Buildings },
            },
            Objective { text: "Keep your command center standing.", goal: Goal::Protect { tag: 1 } },
        ],
        triggers: &[
            Trigger {
                when: When::At(5 * S),
                then: &[Action::Say("All quiet in the north-east. Their technicals are on their morning loop.")],
            },
            Trigger {
                when: When::Entered { player: 0, area: area(290, 215, 40) },
                then: &[
                    Action::Say("We're at the wells, and the warlord has noticed. His base is stirring."),
                    Action::Wake { player: 1, level: Easy },
                ],
            },
            Trigger {
                when: When::At(420 * S),
                then: &[
                    Action::Say("Movement at the GLA base: they are coming."),
                    Action::Wake { player: 1, level: Easy },
                ],
            },
            Trigger {
                when: When::At(240 * S),
                then: &[
                    Action::Say("Two Aegis tanks and a howitzer have crossed the river. They are yours."),
                    Action::Spawn {
                        forces: &[f(0, EU_AEGIS, 40, 300).times(2), f(0, EU_ARTILLERY, 30, 320)],
                        route: &[],
                    },
                ],
            },
            Trigger {
                when: When::Every { from: 150 * S, every: 75 * S, times: 8, while_alive: Some(2) },
                then: &[
                    Action::Say("A GLA column on the ash road!"),
                    Action::Spawn {
                        forces: &[
                            f(1, GLA_RABBLE, 450, 140).times(4),
                            f(1, GLA_TECHNICAL, 470, 150).times(2),
                            f(1, GLA_TANK, 490, 140).times(3),
                        ],
                        route: &[(440, 200), (240, 310), (110, 305)],
                    },
                ],
            },
            Trigger {
                when: When::Destroyed(2),
                then: &[Action::Say("The warlord's headquarters is burning. Finish the base.")],
            },
        ],
    },
    // 7. GLA: raid an EU firebase whose patrols and guards are fed from a war factory; while
    //    it stands, a fresh patrol rolls out every few minutes.
    Mission {
        key: "burnt-orchards",
        name: "Burnt Orchards",
        map: "gen:Scorched Earth",
        briefing: "The EU has turned the orchards east of the village into a firebase, and every \
                   morning a patrol rolls out of its war factory to remind the farmers who is in \
                   charge. Its tanks stand guard south of the factory. Our cell is small, but the \
                   village is with us and the burnt groves give cover. The war factory is the heart \
                   of it: while it stands, new patrols keep coming. Bring it down first, then tear \
                   down the rest so the orchards can grow back.",
        sides: &[
            Side { faction: Gla, credits: 4_000, ai: Some(Easy), general: None, builds: None },
            Side {
                faction: Eu,
                credits: 2_000,
                ai: Some(Easy),
                general: None,
                builds: Some(&[
                    EU_DOZER,
                    EU_SUPPLY,
                    EU_HARVESTER,
                    EU_POWER,
                    EU_BARRACKS,
                    EU_FACTORY,
                    EU_RECON,
                    EU_ROCKETS,
                    EU_AEGIS,
                    EU_GUN_TURRET,
                ]),
            },
        ],
        forces: &[
            f(0, GLA_CC, 50, 125),
            f(0, GLA_WORKER, 50, 152).times(2),
            f(0, GLA_STASH, 40, 65),
            f(0, GLA_BARRACKS, 80, 102),
            f(0, GLA_FACTORY, 86, 152),
            f(0, GLA_RABBLE, 72, 128).times(4),
            f(0, GLA_TECHNICAL, 56, 178).times(2),
            f(0, GLA_RPG, 30, 100).times(2),
            f(1, EU_CC, 265, 150).tagged(2),
            f(1, EU_DOZER, 265, 175),
            f(1, EU_SUPPLY, 260, 72),
            f(1, EU_POWER, 292, 120),
            f(1, EU_BARRACKS, 242, 118),
            f(1, EU_FACTORY, 285, 192).tagged(3),
            f(1, EU_GUN_TURRET, 230, 160),
            f(1, EU_GUN_TURRET, 232, 104),
            f(1, EU_GUN_TURRET, 225, 200),
            f(1, EU_ROCKETS, 215, 132).times(3),
            f(1, EU_RECON, 190, 60).times(4).tagged(4).patrol(&[(190, 60), (185, 190), (140, 250)]),
            f(1, EU_AEGIS, 250, 205).times(3).guard(area(250, 205, 30)),
        ],
        objectives: &[
            Objective { text: "Destroy the EU war factory.", goal: Goal::Destroy { tag: 3 } },
            Objective {
                text: "Destroy every building of the firebase.",
                goal: Goal::DestroyAll { owner: 1, what: What::Buildings },
            },
        ],
        triggers: &[
            Trigger {
                when: When::At(5 * S),
                then: &[Action::Say("An EU patrol is walking the groves. Their tanks sit south of the factory.")],
            },
            Trigger {
                when: When::Every { from: 180 * S, every: 110 * S, times: 5, while_alive: Some(3) },
                then: &[
                    Action::Say("A fresh EU patrol is rolling out of the war factory."),
                    Action::Spawn {
                        forces: &[f(1, EU_AEGIS, 290, 216), f(1, EU_RECON, 270, 220).times(4)],
                        route: &[(200, 170), (90, 135)],
                    },
                ],
            },
            Trigger {
                when: When::At(300 * S),
                then: &[
                    Action::Say("The villagers have dug up a cache: 1,500 credits and a pair of Scorpions."),
                    Action::Credits { player: 0, amount: 1_500 },
                    Action::Spawn { forces: &[f(0, GLA_TANK, 30, 230).times(2)], route: &[] },
                ],
            },
            Trigger {
                when: When::Done(0),
                then: &[Action::Say("The war factory is down: no more patrols. Burn the rest.")],
            },
        ],
    },
    // 8. EU: two enemies, an APA brigade that plays from the start and a GLA cell that joins,
    //    and raids, when the brigade's headquarters falls (or at seven minutes); a command
    //    center to keep.
    Mission {
        key: "cold-front",
        name: "Cold Front",
        map: "gen:Tournament Tundra",
        briefing: "Two armies, one valley. The APA has a brigade camped in the south-east of the tundra \
                   and sends a column at us every few minutes; the GLA holds the north-east, its \
                   fighters guarding the frozen fields and its technicals on patrol, and so far it has \
                   kept out of the APA's way. That will not last: the day the APA falls, or the day \
                   the GLA tires of waiting, they will come for us too. Hold the north-west, keep the \
                   command center standing, and leave neither of them a building.",
        sides: &[
            Side { faction: Eu, credits: 5_000, ai: Some(Easy), general: None, builds: None },
            Side {
                faction: Apa,
                credits: 2_000,
                ai: Some(Easy),
                general: None,
                builds: Some(&[
                    APA_CRANE,
                    APA_SUPPLY,
                    APA_HARVESTER,
                    APA_POWER,
                    APA_BARRACKS,
                    APA_FACTORY,
                    APA_RED_GUARD,
                    APA_TANK_HUNTER,
                    APA_GATLING,
                    APA_FLAME_TURRET,
                ]),
            },
            Side {
                faction: Gla,
                credits: 1_500,
                ai: None,
                general: None,
                builds: Some(&[GLA_WORKER, GLA_STASH, GLA_BARRACKS, GLA_RABBLE, GLA_RPG, GLA_ROCKET_SITE]),
            },
        ],
        forces: &[
            f(0, EU_CC, 70, 75).tagged(1),
            f(0, EU_DOZER, 70, 100),
            f(0, EU_SUPPLY, 115, 40),
            f(0, EU_HARVESTER, 120, 62),
            f(0, EU_POWER, 40, 100),
            f(0, EU_BARRACKS, 110, 122),
            f(0, EU_FACTORY, 45, 50),
            f(0, EU_AEGIS, 112, 80).times(3),
            f(0, EU_RECON, 135, 105).times(4),
            f(0, EU_ROCKETS, 70, 128).times(2),
            f(1, APA_CC, 345, 345).tagged(2),
            f(1, APA_CRANE, 330, 325),
            f(1, APA_SUPPLY, 300, 375),
            f(1, APA_POWER, 375, 375),
            f(1, APA_BARRACKS, 372, 318),
            f(1, APA_FACTORY, 308, 332),
            f(1, APA_FLAME_TURRET, 320, 300),
            f(1, APA_FLAME_TURRET, 285, 355),
            f(1, APA_RED_GUARD, 345, 300).times(4),
            f(1, APA_TANK_HUNTER, 360, 290).times(3),
            f(1, APA_GATLING, 290, 300).times(3).guard(area(290, 300, 30)),
            f(2, GLA_CC, 345, 75).tagged(3),
            f(2, GLA_WORKER, 345, 100),
            f(2, GLA_STASH, 372, 48),
            f(2, GLA_BARRACKS, 320, 45),
            f(2, GLA_ROCKET_SITE, 300, 80),
            f(2, GLA_RABBLE, 300, 110).times(5).guard(area(300, 110, 30)),
            f(2, GLA_RPG, 320, 120).times(3).guard(area(300, 110, 30)),
            f(2, GLA_TECHNICAL, 250, 105).times(3).tagged(4).patrol(&[(250, 105), (250, 200), (320, 200)]),
        ],
        objectives: &[
            Objective {
                text: "Destroy every building of the APA brigade.",
                goal: Goal::DestroyAll { owner: 1, what: What::Buildings },
            },
            Objective {
                text: "Destroy every building of the GLA cell.",
                goal: Goal::DestroyAll { owner: 2, what: What::Buildings },
            },
            Objective { text: "Keep your command center standing.", goal: Goal::Protect { tag: 1 } },
        ],
        triggers: &[
            Trigger {
                when: When::At(5 * S),
                then: &[Action::Say("APA to the south-east, GLA to the north-east. The GLA is watching, for now.")],
            },
            Trigger {
                when: When::Every { from: 180 * S, every: 120 * S, times: 6, while_alive: Some(2) },
                then: &[
                    Action::Say("An APA column is crossing the tundra!"),
                    Action::Spawn {
                        forces: &[
                            f(1, APA_RED_GUARD, 300, 270).times(4),
                            f(1, APA_TANK_HUNTER, 280, 270).times(2),
                            f(1, APA_GATLING, 320, 275).times(2),
                        ],
                        route: &[(210, 310), (100, 150), (80, 110)],
                    },
                ],
            },
            Trigger {
                when: When::Destroyed(2),
                then: &[
                    Action::Say("The APA headquarters is down, and the GLA has decided we are next."),
                    Action::Wake { player: 2, level: Easy },
                ],
            },
            Trigger {
                when: When::At(420 * S),
                then: &[Action::Say("The GLA is on the move!"), Action::Wake { player: 2, level: Easy }],
            },
            Trigger {
                when: When::Every { from: 450 * S, every: 120 * S, times: 4, while_alive: Some(3) },
                then: &[
                    Action::Say("GLA raiders out of the north-east!"),
                    Action::Spawn {
                        forces: &[f(2, GLA_RABBLE, 300, 140).times(4), f(2, GLA_TECHNICAL, 280, 140).times(2)],
                        route: &[(200, 110), (110, 90)],
                    },
                ],
            },
            Trigger {
                when: When::At(360 * S),
                then: &[
                    Action::Say("Brussels sends two Aegis tanks and an APC."),
                    Action::Spawn { forces: &[f(0, EU_AEGIS, 30, 175).times(2), f(0, EU_APC, 30, 195)], route: &[] },
                ],
            },
        ],
    },
];
