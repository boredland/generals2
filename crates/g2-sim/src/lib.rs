//! Deterministic simulation for the Generals 2 recreation.
//!
//! Rules for everything in this crate:
//! - No floating point in state or logic (`Fx` fixed point only; `to_f32` is for rendering).
//! - No `HashMap`/`HashSet` iteration and no system time, threads or IO.
//! - Iterate entities in index order; any new state field goes into `World::state_hash`.
//!
//! The same setup plus the same command list must give the same state hash on every
//! platform and build profile. `tests/determinism.rs` and the golden replay enforce that.

pub mod ai;
pub mod command;
pub mod defs;
pub mod entity;
pub mod fixed;
mod hash;
pub mod map;
pub mod replay;
pub mod rng;
pub mod world;

pub use command::{Command, CommandKind, Reject};
pub use defs::{BuildingRole, Class, Cost, Def, DefId, Faction, Rules};
pub use entity::{Entity, EntityId, GatherPhase, Order, PlayerId};
pub use fixed::{Fx, Vec2};
pub use map::{MapDef, MatchSetup, PlayerSetup};
pub use replay::Replay;
pub use world::{Event, Player, TICKS_PER_SECOND, World};

use replay::HASH_INTERVAL;

/// Runs a replay from its setup and reports the first checkpoint whose hash differs.
pub fn verify_replay(replay: &Replay, map: MapDef, rules: &'static Rules) -> Result<u64, Divergence> {
    let setup = MatchSetup { map, seed: replay.seed, players: replay.players.clone() };
    let mut world = World::new(&setup, rules);
    for c in &replay.commands {
        world.schedule(c.clone());
    }
    let mut checks = replay.hashes.iter().peekable();
    while world.tick() < replay.length_ticks {
        if let Some(&&(tick, expected)) = checks.peek()
            && tick == world.tick()
        {
            let actual = world.state_hash();
            if actual != expected {
                return Err(Divergence { tick, expected, actual });
            }
            checks.next();
        }
        world.step();
    }
    Ok(world.state_hash())
}

#[derive(Debug, PartialEq, Eq)]
pub struct Divergence {
    pub tick: u32,
    pub expected: u64,
    pub actual: u64,
}

/// Records a match: steps `world` to `length_ticks`, collecting hash checkpoints, and packs the
/// result with the human command log. `drive` runs before every step to inject live input.
pub fn record(
    mut world: World,
    map_name: &str,
    setup: &MatchSetup,
    length_ticks: u32,
    mut drive: impl FnMut(&mut World),
) -> (World, Replay) {
    let mut hashes = Vec::new();
    while world.tick() < length_ticks {
        if world.tick().is_multiple_of(HASH_INTERVAL) {
            hashes.push((world.tick(), world.state_hash()));
        }
        drive(&mut world);
        world.step();
    }
    let replay = Replay {
        map: map_name.to_owned(),
        seed: setup.seed,
        players: setup.players.clone(),
        length_ticks,
        commands: world.command_log().to_vec(),
        hashes,
    };
    (world, replay)
}
