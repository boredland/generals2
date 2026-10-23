use crate::defs::DefId;
use crate::entity::{EntityId, PlayerId};
use crate::fixed::Vec2;

/// A player intent scheduled for a specific tick. Commands are the only input to the
/// simulation, so a replay is just the match setup plus this list.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Command {
    pub tick: u32,
    pub player: PlayerId,
    pub kind: CommandKind,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CommandKind {
    Move {
        units: Vec<EntityId>,
        target: Vec2,
    },
    Gather {
        units: Vec<EntityId>,
        pile: EntityId,
    },
    /// Place a foundation and send `builder` to construct it.
    Build {
        builder: EntityId,
        def: DefId,
        pos: Vec2,
    },
    /// Send constructors to help finish an existing foundation.
    Construct {
        units: Vec<EntityId>,
        site: EntityId,
    },
    Train {
        producer: EntityId,
        def: DefId,
    },
    CancelTrain {
        producer: EntityId,
    },
    Stop {
        units: Vec<EntityId>,
    },
    /// Concede: the player is defeated and all their assets are removed.
    Surrender,
}

/// Why a command was ignored. Rejections are deterministic and do not change state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reject {
    NotOwner,
    NoSuchEntity,
    CannotBuild,
    CannotProduce,
    InsufficientFunds,
    InvalidPlacement,
    NotACollector,
    NotAConstructor,
    /// Every reachable supply center is at its faction's collector cap.
    DepotFull,
    QueueFull,
}
