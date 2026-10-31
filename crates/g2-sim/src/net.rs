//! Deterministic lockstep for a two-player match over any ordered, reliable byte link (TCP).
//! No IO here: the driver moves `Message` bytes; this module decides when the sim may step.
//!
//! Time is cut into turns of `TURN_TICKS` ticks. The orders a player gives during turn `k` run
//! at the start of turn `k + delay`, on both machines. Each player sends one `Turn` per
//! turn, empty or not, so the other knows it has everything for that turn, and a machine steps
//! into a turn only once it holds the other player's `Turn` for it: both apply the same commands
//! on the same tick. Every `HASH_INTERVAL` ticks the next `Turn` sent also carries the sender's
//! state hash, and a mismatch is a desync.
//!
//! The delay starts at `DELAY_TURNS`, room for a round trip on a LAN. Over the internet a round
//! trip can take longer than that, and every turn would wait for the other player's orders, so
//! the delay follows the link: each machine says how many turns it would like (`Lockstep::want`),
//! the joiner tells the host its wish in a `Turn`, and the host announces the change in one of
//! its own `Turn`s, for a turn neither machine has reached. Both switch at that turn's start.
//! Which turn a machine sends when only decides how early the other gets it: the turn number in
//! every `Turn` says when its orders run, so the delay can't desync the sims, only stall them.
//!
//! Commands from the peer are scheduled as the peer's player, so the sim's own ownership checks
//! refuse anything aimed at the other player's units. AI players (co-op Onslaught's waves) need
//! no peer: the AI runs inside the sim, on both machines alike.
//!
//! Before the match, the lobby: the host offers the match as it stands (`Lobby`), the joiner
//! answers with the seat it picked (`Seat`), and once the joiner is ready the host starts it
//! with `Hello`, which the joiner accepts with `Join`. On a local network hosts are found by a
//! `Beacon` query, as Generals' LAN lobby finds games. Throughout, `Ping` and `Pong` measure the
//! round trip and show the link is alive.

use std::collections::BTreeMap;

use crate::command::{Command, CommandKind};
use crate::defs::{Faction, GeneralId, PerkId};
use crate::entity::PlayerId;
use crate::replay::{DecodeError, HASH_INTERVAL, Reader, Replay, Writer};
use crate::world::World;

/// Bumped on any change to `Message`, `Beacon` or to how turns are scheduled.
pub const PROTOCOL: u16 = 4;
/// Bumped on any change to simulation logic that the rules tables don't show (AI, combat,
/// movement): two machines must run the same sim, or they drift apart within a second.
pub const SIM_VERSION: u32 = 17;

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
/// The delay a match starts with, and the least it goes down to: 0.2-0.3 s at `TURN_TICKS`,
/// room for one round trip on a LAN before a machine has to wait.
pub const DELAY_TURNS: u32 = 2;
/// The most turns orders wait: a second, enough for a round trip of about 1.8 s. A link slower
/// than that is not one a match can be played over.
pub const MAX_DELAY_TURNS: u32 = 10;

// Hash checks sit on turn boundaries, where both machines are at the same point.
const _: () = assert!(HASH_INTERVAL.is_multiple_of(TURN_TICKS));
// A delay going down by one leaves one turn boundary with nothing to send, and the next change
// waits until this one is in force, so two never come in a row: a hash check taken on a silent
// boundary goes out with the next `Turn`, well before the next check is taken.
const _: () = assert!(2 * TURN_TICKS <= HASH_INTERVAL);

/// The UDP port of LAN discovery, Generals' own `lobbyPort` (`LANAPI.cpp`); the game itself
/// listens on Generals' `NETWORK_BASE_PORT_NUMBER`, 8088, unless the host picks another.
pub const BEACON_PORT: u16 = 8086;
pub const GAME_PORT: u16 = 8088;

/// LAN discovery, one datagram each: a lobby broadcasts `Query`, and every host with a game
/// open answers `Game` to the asker. Generals' LAN lobby announces games every 10 seconds and
/// drops one unheard for 20 (`s_resendDelta`); asking, a list fills at once instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Beacon {
    Query {
        protocol: u16,
    },
    /// A hosted game: its TCP port, name (the host's), map, mode, and whether its joiner seat
    /// is free. `build` lets the list mark games this build can't join.
    Game {
        protocol: u16,
        build: u64,
        port: u16,
        name: String,
        map: String,
        coop: bool,
        open: bool,
    },
}

const BEACON_MAGIC: &[u8; 4] = b"G2LN";

impl Beacon {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer(BEACON_MAGIC.to_vec());
        match self {
            Beacon::Query { protocol } => {
                w.u8(0);
                w.u16(*protocol);
            }
            Beacon::Game { protocol, build, port, name, map, coop, open } => {
                w.u8(1);
                w.u16(*protocol);
                w.u64(*build);
                w.u16(*port);
                w.bytes(name.as_bytes());
                w.bytes(map.as_bytes());
                w.u8(*coop as u8 | (*open as u8) << 1);
            }
        }
        w.0
    }

    /// Any datagram may arrive on the port, so anything else is `None`, not an error.
    pub fn decode(bytes: &[u8]) -> Option<Beacon> {
        let rest = bytes.strip_prefix(BEACON_MAGIC)?;
        let mut r = Reader { b: rest, at: 0 };
        let text = |r: &mut Reader| r.bytes().ok().and_then(|b| String::from_utf8(b.to_vec()).ok());
        let b = match r.u8().ok()? {
            0 => Beacon::Query { protocol: r.u16().ok()? },
            1 => {
                let (protocol, build, port) = (r.u16().ok()?, r.u64().ok()?, r.u16().ok()?);
                let (name, map) = (text(&mut r)?, text(&mut r)?);
                let flags = r.u8().ok()?;
                Beacon::Game { protocol, build, port, name, map, coop: flags & 1 != 0, open: flags & 2 != 0 }
            }
            _ => return None,
        };
        (r.at == rest.len()).then_some(b)
    }
}

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
    /// Host to joiner, in the lobby: the match as it stands, the joiner's seat as player 1.
    /// `revision` counts the host's own changes (map, mode, its seat): a joiner's ready counts
    /// only for the revision it saw, so any change the host makes clears it, as Generals' game
    /// options clear every accept.
    Lobby {
        protocol: u16,
        build: u64,
        setup: Replay,
        revision: u32,
        /// The host's player name, for the joiner's player list.
        name: String,
    },
    /// Joiner to host, in the lobby: the seat the joiner picked, and whether they are ready.
    Seat(Seat),
    /// Either way, at any time: answered at once with a `Pong` of the same `stamp` (the asker's
    /// clock, which times the round trip). A link that stays silent for long is a dead one, so
    /// both keep asking even when there is nothing else to say.
    Ping {
        stamp: u64,
    },
    Pong {
        stamp: u64,
    },
}

/// The joiner's choices in the lobby. The host checks them against the rules before taking
/// them into the match (`Replay::check`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seat {
    /// The joiner's player name, shown in the host's player list.
    pub name: String,
    pub faction: Faction,
    pub general: Option<GeneralId>,
    pub perks: Vec<PerkId>,
    pub rank: u8,
    /// Ready for the host's offer of this `revision`; `None` while not ready.
    pub ready: Option<u32>,
}

/// One player's orders for one turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turn {
    pub turn: u32,
    pub commands: Vec<CommandKind>,
    /// `(tick, state hash before stepping it)` of the sender's latest `HASH_INTERVAL`th tick, on
    /// the first turn it sent from that tick on.
    pub check: Option<(u32, u64)>,
    /// The delay the sender would like, in turns (`Lockstep::set_wish`).
    pub wish: u8,
    /// From the host only: the delay from the start of turn `turn + 1` on.
    pub delay: Option<u8>,
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
                w.u8(t.wish);
                w.u8(t.delay.unwrap_or(0));
            }
            Message::Lobby { protocol, build, setup, revision, name } => {
                w.u8(3);
                w.u16(*protocol);
                w.u64(*build);
                w.bytes(&setup.encode());
                w.u32(*revision);
                w.bytes(name.as_bytes());
            }
            Message::Seat(s) => {
                w.u8(4);
                w.bytes(s.name.as_bytes());
                w.u8(s.faction as u8);
                w.u16(s.general.map_or(u16::MAX, |g| g.0));
                w.u8(s.perks.len() as u8);
                for k in &s.perks {
                    w.u16(k.0);
                }
                w.u8(s.rank);
                match s.ready {
                    Some(rev) => {
                        w.u8(1);
                        w.u32(rev);
                    }
                    None => w.u8(0),
                }
            }
            Message::Ping { stamp } => {
                w.u8(5);
                w.u64(*stamp);
            }
            Message::Pong { stamp } => {
                w.u8(6);
                w.u64(*stamp);
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
                let wish = r.u8()?;
                let delay = Some(r.u8()?).filter(|&d| d != 0);
                Message::Turn(Turn { turn, commands, check, wish, delay })
            }
            3 => Message::Lobby {
                protocol: r.u16()?,
                build: r.u64()?,
                setup: Replay::decode(r.bytes()?)?,
                revision: r.u32()?,
                name: r.text()?,
            },
            4 => {
                let name = r.text()?;
                let faction = r.faction()?;
                let general = Some(r.u16()?).filter(|&g| g != u16::MAX).map(GeneralId);
                let n = r.u8()? as usize;
                if n > crate::defs::PERK_SLOTS {
                    return Err(DecodeError::BadTag(n as u8));
                }
                let perks = (0..n).map(|_| r.u16().map(PerkId)).collect::<Result<_, _>>()?;
                let rank = r.u8()?;
                let ready = match r.u8()? {
                    0 => None,
                    1 => Some(r.u32()?),
                    t => return Err(DecodeError::BadTag(t)),
                };
                Message::Seat(Seat { name, faction, general, perks, rank, ready })
            }
            5 => Message::Ping { stamp: r.u64()? },
            6 => Message::Pong { stamp: r.u64()? },
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
    /// The next turn boundary `before_step` has to deal with; it is called again and again on
    /// one tick while the peer's orders are missing.
    next_boundary: u32,
    /// The next turn expected from the peer; every earlier one has arrived.
    next_recv: u32,
    /// The hash check taken on a turn boundary that sent nothing (after the delay went down),
    /// for the next `Turn` sent.
    unsent: Option<(u32, u64)>,
    /// Hash checks taken here and received, by tick, until both are in.
    mine: BTreeMap<u32, u64>,
    theirs: BTreeMap<u32, u64>,
    /// The delay in turns from each turn boundary on, the first at turn 0, as both machines
    /// agreed it. Kept whole: the turn a peer's `Turn` was sent at, which says which hash
    /// check it must carry, follows from it.
    schedule: Vec<(u32, u32)>,
    /// The delay this machine would like, and the peer's latest wish (the host's concern).
    wish: u32,
    peer_wish: u32,
}

impl Lockstep {
    /// `me` and `peer`, the two human players; the host is the lower-numbered one (player 0),
    /// which decides the delay.
    pub fn new(me: PlayerId, peer: PlayerId) -> Lockstep {
        Lockstep {
            me,
            peer,
            queued: Vec::new(),
            next_send: DELAY_TURNS,
            next_boundary: 0,
            next_recv: DELAY_TURNS,
            unsent: None,
            mine: BTreeMap::new(),
            theirs: BTreeMap::new(),
            schedule: vec![(0, DELAY_TURNS)],
            wish: DELAY_TURNS,
            peer_wish: DELAY_TURNS,
        }
    }

    pub fn me(&self) -> PlayerId {
        self.me
    }

    fn hosts(&self) -> bool {
        self.me.0 < self.peer.0
    }

    /// Queues an order of the local player; it goes out with the next `Turn`.
    pub fn order(&mut self, kind: CommandKind) {
        self.queued.push(kind);
    }

    /// The delay this machine would like, from what it measures of the link; kept within
    /// `DELAY_TURNS..=MAX_DELAY_TURNS`. It goes to the peer with every `Turn`, and the host
    /// takes the larger of both wishes: up at once, down a turn at a time.
    pub fn set_wish(&mut self, turns: u32) {
        self.wish = turns.clamp(DELAY_TURNS, MAX_DELAY_TURNS);
    }

    /// The delay in force on `turn`'s boundary, as far as it is agreed yet.
    pub fn delay_at(&self, turn: u32) -> u32 {
        self.schedule.iter().rev().find(|&&(from, _)| from <= turn).map_or(DELAY_TURNS, |&(_, d)| d)
    }

    /// Every agreed delay with the turn it starts on, the first `(0, DELAY_TURNS)`. Both machines
    /// hold the same list, up to the changes one of them hasn't heard of yet.
    pub fn schedule(&self) -> &[(u32, u32)] {
        &self.schedule
    }

    /// The boundary the sender of turn `t` sent it on: the first whose delay reaches `t`.
    fn sent_at(&self, t: u32) -> u32 {
        for (i, &(from, d)) in self.schedule.iter().enumerate() {
            let until = self.schedule.get(i + 1).map_or(u32::MAX, |s| s.0);
            let at = from.max(t.saturating_sub(d));
            if at < until {
                return at;
            }
        }
        unreachable!("the last delay reaches every turn")
    }

    /// Call before every `world.step()`. On a turn boundary this takes the hash check and sends
    /// (into `out`) the local orders for the turn the delay reaches, scheduling them in `world`
    /// as well: none after the delay went down, several after it went up. Returns whether
    /// `world` may step now: not while the peer's orders for the current turn are missing.
    pub fn before_step(&mut self, world: &mut World, out: &mut Vec<Turn>) -> Result<bool, NetError> {
        let tick = world.tick();
        let turn = tick / TURN_TICKS;
        if tick.is_multiple_of(TURN_TICKS) && self.next_boundary == turn {
            self.next_boundary += 1;
            if tick.is_multiple_of(HASH_INTERVAL) {
                let hash = world.state_hash();
                self.mine.insert(tick, hash);
                self.unsent = Some((tick, hash));
            }
            let reach = turn + self.delay_at(turn);
            let mut first = true;
            while self.next_send <= reach {
                let n = self.next_send;
                let (commands, check, delay) = if first {
                    (std::mem::take(&mut self.queued), self.unsent.take(), self.announce(turn, n))
                } else {
                    (Vec::new(), None, None)
                };
                first = false;
                for kind in &commands {
                    world.schedule(Command { tick: n * TURN_TICKS, player: self.me, kind: kind.clone() });
                }
                out.push(Turn { turn: n, commands, check, wish: self.wish as u8, delay: delay.map(|d| d as u8) });
                self.next_send += 1;
            }
            self.compare()?;
        }
        Ok(turn < self.next_recv)
    }

    /// The host's change of delay to go out with turn `n`, sent on boundary `now`, if both
    /// wishes call for one and the last change is in force already. It starts on turn `n + 1`,
    /// a boundary neither machine has reached: the joiner can't step into turn `n` without it.
    fn announce(&mut self, now: u32, n: u32) -> Option<u32> {
        if !self.hosts() || self.schedule.last().is_some_and(|&(from, _)| from > now) {
            return None;
        }
        let (current, target) = (self.delay_at(now), self.wish.max(self.peer_wish));
        let next = match target.cmp(&current) {
            std::cmp::Ordering::Greater => target,
            std::cmp::Ordering::Less => current - 1,
            std::cmp::Ordering::Equal => return None,
        };
        self.schedule.push((n + 1, next));
        Some(next)
    }

    /// Takes in the peer's `Turn`: its orders are scheduled for that turn's first tick, which
    /// this machine has not stepped yet (it can't have, without them).
    pub fn receive(&mut self, world: &mut World, t: Turn) -> Result<(), NetError> {
        if t.turn != self.next_recv {
            return Err(NetError::Protocol(format!("turn {} arrived, expected {}", t.turn, self.next_recv)));
        }
        // The check a turn must carry: the sender's hash at the latest hash tick since the turn
        // it sent before, when there was one, and none otherwise. Anything else would leave a
        // check that never pairs up and silently stops every later comparison.
        let sent = self.sent_at(t.turn) * TURN_TICKS;
        let after = (t.turn > DELAY_TURNS).then(|| self.sent_at(t.turn - 1) * TURN_TICKS);
        let due = after.map_or(0, |a| (a / HASH_INTERVAL + 1) * HASH_INTERVAL);
        if t.check.map(|c| c.0) != (due <= sent).then_some(due) {
            return Err(NetError::Protocol(format!("turn {} carries check {:?}", t.turn, t.check)));
        }
        if let Some(d) = t.delay.map(u32::from) {
            if self.hosts() || !(DELAY_TURNS..=MAX_DELAY_TURNS).contains(&d) {
                return Err(NetError::Protocol(format!("turn {} sets a delay of {d}", t.turn)));
            }
            self.schedule.push((t.turn + 1, d));
        }
        self.peer_wish = u32::from(t.wish).clamp(DELAY_TURNS, MAX_DELAY_TURNS);
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
        let turn = world.tick() / TURN_TICKS;
        self.next_recv > turn + self.delay_at(turn) + 1
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
