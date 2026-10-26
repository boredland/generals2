//! Deterministic lockstep for a two-player match over any ordered, reliable byte link (TCP).
//! No IO here: the driver moves `Message` bytes; this module decides when the sim may step.
//!
//! Time is cut into turns of `TURN_TICKS` ticks. The orders a player gives during turn `k` run
//! at the start of turn `k + DELAY_TURNS`, on both machines. Each player sends one `Turn` per
//! turn, empty or not, so the other knows it has everything for that turn, and a machine steps
//! into a turn only once it holds the other player's `Turn` for it: both apply the same commands
//! on the same tick. Every `HASH_INTERVAL` ticks a `Turn` also carries the sender's state hash,
//! and a mismatch is a desync.
//!
//! Commands from the peer are scheduled as the peer's player, so the sim's own ownership checks
//! refuse anything aimed at the other player's units.

use std::collections::BTreeMap;

use crate::command::{Command, CommandKind};
use crate::entity::PlayerId;
use crate::replay::{DecodeError, HASH_INTERVAL, Reader, Replay, Writer};
use crate::world::World;

/// Bumped on any change to `Message` or to how turns are scheduled.
pub const PROTOCOL: u16 = 2;
/// Bumped on any change to simulation logic that the rules tables don't show (AI, combat,
/// movement): two machines must run the same sim, or they drift apart within a second.
pub const SIM_VERSION: u32 = 9;

/// What both machines must agree on before a match: `SIM_VERSION` and the rules tables
/// (every unit, building, weapon, general, upgrade and perk). Two different releases almost always
/// differ here, so they are refused at the handshake instead of failing with "out of sync".
pub fn build_id(rules: &crate::defs::Rules) -> u64 {
    let text = format!("{SIM_VERSION} {:?} {:?} {:?} {:?}", rules.defs, rules.generals, rules.upgrades, rules.perks);
    // FNV-1a, as `state_hash`.
    text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}
/// Ticks per turn: a tenth of a second.
pub const TURN_TICKS: u32 = 3;
/// Turns between giving an order and its running: 0.2-0.3 s at `TURN_TICKS`, room for one
/// round trip on most links before a machine has to wait.
pub const DELAY_TURNS: u32 = 2;

// Hash checks sit on turn boundaries, where both machines are at the same point.
const _: () = assert!(HASH_INTERVAL.is_multiple_of(TURN_TICKS));

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    /// Host to joiner: the match to play, as a replay with no commands yet, and the host's
    /// `build_id`.
    Hello {
        protocol: u16,
        build: u64,
        setup: Replay,
    },
    /// Joiner to host: the match was accepted, by a machine with this `build_id`.
    Join {
        protocol: u16,
        build: u64,
    },
    Turn(Turn),
}

/// One player's orders for one turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turn {
    pub turn: u32,
    pub commands: Vec<CommandKind>,
    /// `(tick, state hash before stepping it)`, on every `HASH_INTERVAL`th tick.
    pub check: Option<(u32, u64)>,
}

impl Message {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer(Vec::new());
        match self {
            Message::Hello { protocol, build, setup } => {
                w.u8(0);
                w.u16(*protocol);
                w.u64(*build);
                w.bytes(&setup.encode());
            }
            Message::Join { protocol, build } => {
                w.u8(1);
                w.u16(*protocol);
                w.u64(*build);
            }
            Message::Turn(t) => {
                w.u8(2);
                w.u32(t.turn);
                w.u32(t.commands.len() as u32);
                for c in &t.commands {
                    w.command(c);
                }
                match t.check {
                    Some((tick, hash)) => {
                        w.u8(1);
                        w.u32(tick);
                        w.u64(hash);
                    }
                    None => w.u8(0),
                }
            }
        }
        w.0
    }

    pub fn decode(bytes: &[u8]) -> Result<Message, DecodeError> {
        let mut r = Reader { b: bytes, at: 0 };
        let m = match r.u8()? {
            // The protocol comes first, so any version of this decoder can report a mismatch.
            0 => Message::Hello { protocol: r.u16()?, build: r.u64()?, setup: Replay::decode(r.bytes()?)? },
            1 => Message::Join { protocol: r.u16()?, build: r.u64()? },
            2 => {
                let turn = r.u32()?;
                let n = r.u32()? as usize;
                // Every command takes at least its tag byte.
                if n > bytes.len() - r.at {
                    return Err(DecodeError::Truncated);
                }
                let commands = (0..n).map(|_| r.command()).collect::<Result<_, _>>()?;
                let check = match r.u8()? {
                    0 => None,
                    1 => Some((r.u32()?, r.u64()?)),
                    t => return Err(DecodeError::BadTag(t)),
                };
                Message::Turn(Turn { turn, commands, check })
            }
            t => return Err(DecodeError::BadTag(t)),
        };
        if r.at != bytes.len() {
            return Err(DecodeError::Truncated);
        }
        Ok(m)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetError {
    /// The two machines' states differ at `tick`.
    Desync { tick: u32, mine: u64, theirs: u64 },
    /// The peer broke the protocol.
    Protocol(String),
}

/// One machine's side of a lockstep match.
#[derive(Clone, Debug)]
pub struct Lockstep {
    me: PlayerId,
    peer: PlayerId,
    /// The local player's orders since the last `Turn` was sent.
    queued: Vec<CommandKind>,
    /// The turn the next `Turn` sent carries orders for.
    next_send: u32,
    /// The next turn expected from the peer; every earlier one has arrived.
    next_recv: u32,
    /// Hash checks taken here and received, by tick, until both are in.
    mine: BTreeMap<u32, u64>,
    theirs: BTreeMap<u32, u64>,
}

impl Lockstep {
    pub fn new(me: PlayerId, peer: PlayerId) -> Lockstep {
        Lockstep {
            me,
            peer,
            queued: Vec::new(),
            next_send: DELAY_TURNS,
            next_recv: DELAY_TURNS,
            mine: BTreeMap::new(),
            theirs: BTreeMap::new(),
        }
    }

    pub fn me(&self) -> PlayerId {
        self.me
    }

    /// Queues an order of the local player; it goes out with the next `Turn`.
    pub fn order(&mut self, kind: CommandKind) {
        self.queued.push(kind);
    }

    /// Call before every `world.step()`. On a turn boundary this sends (into `out`) the local
    /// orders for the turn `DELAY_TURNS` ahead, scheduling them in `world` as well, and takes
    /// the hash check. Returns whether `world` may step now: not while the peer's orders for
    /// the current turn are still missing.
    pub fn before_step(&mut self, world: &mut World, out: &mut Vec<Turn>) -> Result<bool, NetError> {
        let tick = world.tick();
        let turn = tick / TURN_TICKS;
        if tick.is_multiple_of(TURN_TICKS) && self.next_send == turn + DELAY_TURNS {
            let check = tick.is_multiple_of(HASH_INTERVAL).then(|| (tick, world.state_hash()));
            if let Some((t, h)) = check {
                self.mine.insert(t, h);
            }
            let commands = std::mem::take(&mut self.queued);
            let at = self.next_send * TURN_TICKS;
            for kind in &commands {
                world.schedule(Command { tick: at, player: self.me, kind: kind.clone() });
            }
            out.push(Turn { turn: self.next_send, commands, check });
            self.next_send += 1;
            self.compare()?;
        }
        Ok(turn < self.next_recv)
    }

    /// Takes in the peer's `Turn`: its orders are scheduled for that turn's first tick, which
    /// this machine has not stepped yet (it can't have, without them).
    pub fn receive(&mut self, world: &mut World, t: Turn) -> Result<(), NetError> {
        if t.turn != self.next_recv {
            return Err(NetError::Protocol(format!("turn {} arrived, expected {}", t.turn, self.next_recv)));
        }
        // The check a turn must carry: the sender's hash at the tick it sent the turn on, when
        // that tick is a hash tick, and none otherwise. Anything else would leave a check that
        // never pairs up and silently stops every later comparison.
        let sent_at = (t.turn - DELAY_TURNS) * TURN_TICKS;
        if t.check.map(|c| c.0) != sent_at.is_multiple_of(HASH_INTERVAL).then_some(sent_at) {
            return Err(NetError::Protocol(format!("turn {} carries check {:?}", t.turn, t.check)));
        }
        let at = t.turn * TURN_TICKS;
        for kind in t.commands {
            world.schedule(Command { tick: at, player: self.peer, kind });
        }
        if let Some((tick, hash)) = t.check {
            self.theirs.insert(tick, hash);
        }
        self.next_recv += 1;
        self.compare()
    }

    /// Whether the peer is more than a turn ahead of `world`: the driver may step twice in one
    /// frame to catch up, so a machine that once stalled doesn't keep the other waiting.
    pub fn behind(&self, world: &World) -> bool {
        self.next_recv > world.tick() / TURN_TICKS + DELAY_TURNS + 1
    }

    fn compare(&mut self) -> Result<(), NetError> {
        while let Some((&tick, &theirs)) = self.theirs.first_key_value() {
            let Some(&mine) = self.mine.get(&tick) else { break };
            if mine != theirs {
                return Err(NetError::Desync { tick, mine, theirs });
            }
            self.theirs.remove(&tick);
            self.mine.retain(|&t, _| t > tick);
        }
        Ok(())
    }
}
