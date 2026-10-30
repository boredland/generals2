//! The campaign: four missions in the world of the 2013 Generals 2 (the EU, the APA and the
//! GLA), one for each faction and a finale, each harder than the last. The briefings and
//! messages are written for this project; nothing is taken from EA's games.
//!
//! Positions are world units on each map (Crater Ridge is built in; the others are the
//! original game's maps as `g2_maps` imports them). Wave sizes and timings are placeholders,
//! tuned until the `Normal` AI in the player's seat wins every mission
//! (`g2-maps/tests/campaign.rs`).

use g2_sim::TICKS_PER_SECOND as S;
use g2_sim::defs::Faction::{Apa, Eu, Gla};
use g2_sim::map::AiLevel::{Easy, Normal};
use g2_sim::mission::{Action, Area, Force, Goal, Mission, Objective, Side, Trigger, What, When};

use crate::ids::*;

const fn f(owner: u8, def: g2_sim::DefId, x: i64, y: i64) -> Force {
    Force::new(owner, def, x, y)
}

pub static MISSIONS: [Mission; 4] = [
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
            Side { faction: Eu, credits: 5_000, ai: Easy, general: None },
            Side { faction: Gla, credits: 2_000, ai: Easy, general: None },
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
                    Action::Spawn { forces: &[f(1, GLA_RABBLE, 250, 250).times(3)], attack: Some((100, 110)) },
                ],
            },
            Trigger {
                when: When::At(210 * S),
                then: &[
                    Action::Say("Two Aegis tanks have rolled in from the coast road. They are yours."),
                    Action::Spawn { forces: &[f(0, EU_AEGIS, 40, 120).times(2)], attack: None },
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
            Side { faction: Apa, credits: 5_000, ai: Easy, general: None },
            Side { faction: Gla, credits: 0, ai: Easy, general: None },
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
                        attack: Some((168, 130)),
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
                        attack: Some((168, 130)),
                    },
                ],
            },
            Trigger {
                when: When::At(300 * S),
                then: &[
                    Action::Say("The Warlords from Qaryat have arrived."),
                    Action::Spawn { forces: &[f(0, APA_WARLORD, 110, 70).times(2)], attack: None },
                ],
            },
            Trigger {
                when: When::At(390 * S),
                then: &[
                    Action::Say("Third wave: Scorpion tanks on the road!"),
                    Action::Spawn {
                        forces: &[f(1, GLA_TANK, 167, 385).times(3), f(1, GLA_RPG, 140, 375).times(3)],
                        attack: Some((168, 130)),
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
                        attack: Some((190, 60)),
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
            Side { faction: Gla, credits: 4_000, ai: Easy, general: None },
            Side { faction: Eu, credits: 2_000, ai: Normal, general: None },
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
                        attack: None,
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
                        attack: Some((220, 150)),
                    },
                ],
            },
            Trigger {
                when: When::At(420 * S),
                then: &[
                    Action::Say("EU armour is moving on the valley!"),
                    Action::Spawn {
                        forces: &[f(1, EU_AEGIS, 200, 320).times(2), f(1, EU_ROCKETS, 180, 318).times(3)],
                        attack: Some((220, 150)),
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
            Side { faction: Eu, credits: 5_000, ai: Easy, general: None },
            // Measured with the `Normal` AI in the player's seat: 3,250 credits and the four
            // Gatlings below is the hardest of the settings tried that it still wins with 4,500,
            // 5,000 and 5,500 credits of its own (at 8:43 with 5,000). 3,500 credits with them
            // overrun its command center (with 5,000 and 5,500), and so do five Gatlings (with
            // 4,500 and 5,500) and six or more (with 5,000).
            Side { faction: Apa, credits: 3_250, ai: Normal, general: None },
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
            f(1, APA_GATLING, 215, 150).times(4),
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
                        attack: Some((167, 330)),
                    },
                ],
            },
            Trigger {
                when: When::At(270 * S),
                then: &[
                    Action::Say("Reinforcements from Brussels: two Aegis tanks and an APC."),
                    Action::Spawn {
                        forces: &[f(0, EU_AEGIS, 250, 405).times(2), f(0, EU_APC, 230, 410)],
                        attack: None,
                    },
                ],
            },
            Trigger {
                when: When::At(330 * S),
                then: &[
                    Action::Say("Warlord tanks and Gatlings, heading for the oasis!"),
                    Action::Spawn {
                        forces: &[f(1, APA_WARLORD, 167, 200).times(2), f(1, APA_GATLING, 190, 200).times(2)],
                        attack: Some((167, 330)),
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
                        attack: Some((167, 330)),
                    },
                ],
            },
            Trigger {
                when: When::Destroyed(2),
                then: &[Action::Say("Their headquarters has fallen. Clear the base.")],
            },
        ],
    },
];
