//! Replay = match setup + every applied command + periodic state hashes.
//! Binary, little-endian, versioned. The map is stored by name and resolved by the caller:
//! "Crater Ridge" is built in; `<gen|zh>:<name>` is imported from the original game's map.

use crate::command::{Command, CommandKind};
use crate::defs::{DefId, Faction};
use crate::entity::{EntityId, PlayerId};
use crate::fixed::{Fx, Vec2};
use crate::map::{AiLevel, PlayerSetup};

const MAGIC: &[u8; 4] = b"G2RP";
/// v2 added `map_fingerprint`, v3 each player's general, v4 the AI level (0 human, 1 + level;
/// before v4 the byte was a flag, and an AI player was the original, now `Easy`, AI), v5 the
/// `Power` command (tag 11), v6 the `Onslaught` level (AI byte 3), v7 the `Research` command
/// (tag 12). Older replays still decode.
pub const VERSION: u16 = 7;
/// Encodes "no general" in v3+.
const NO_GENERAL: u16 = u16::MAX;
/// Hash checkpoint spacing in ticks.
pub const HASH_INTERVAL: u32 = 30;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replay {
    pub map: String,
    /// `MapDef::fingerprint` at recording time; `None` for v1 replays.
    pub map_fingerprint: Option<u64>,
    pub seed: u64,
    pub players: Vec<PlayerSetup>,
    pub length_ticks: u32,
    pub commands: Vec<Command>,
    /// `(tick, hash)` taken before stepping `tick`, every `HASH_INTERVAL` ticks.
    pub hashes: Vec<(u32, u64)>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum DecodeError {
    BadMagic,
    UnsupportedVersion(u16),
    Truncated,
    BadTag(u8),
    BadUtf8,
}

impl Replay {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer(Vec::new());
        w.0.extend_from_slice(MAGIC);
        w.u16(VERSION);
        w.bytes(self.map.as_bytes());
        w.u64(self.map_fingerprint.unwrap_or(0));
        w.u64(self.seed);
        w.u32(self.players.len() as u32);
        for p in &self.players {
            w.u8(p.faction as u8);
            w.i64(p.starting_credits);
            w.u8(p.ai.map_or(0, |l| 1 + l as u8));
            w.u16(p.general.map_or(NO_GENERAL, |g| g.0));
        }
        w.u32(self.length_ticks);
        w.u32(self.commands.len() as u32);
        for c in &self.commands {
            w.u32(c.tick);
            w.u8(c.player.0);
            w.command(&c.kind);
        }
        w.u32(self.hashes.len() as u32);
        for &(t, h) in &self.hashes {
            w.u32(t);
            w.u64(h);
        }
        w.0
    }

    pub fn decode(bytes: &[u8]) -> Result<Replay, DecodeError> {
        let mut r = Reader { b: bytes, at: 0 };
        if r.take(4)? != MAGIC {
            return Err(DecodeError::BadMagic);
        }
        let version = r.u16()?;
        if !(1..=VERSION).contains(&version) {
            return Err(DecodeError::UnsupportedVersion(version));
        }
        let map = String::from_utf8(r.bytes()?.to_vec()).map_err(|_| DecodeError::BadUtf8)?;
        let map_fingerprint = if version >= 2 { Some(r.u64()?) } else { None };
        let seed = r.u64()?;
        let mut players = Vec::new();
        for _ in 0..r.u32()? {
            let faction = match r.u8()? {
                0 => Faction::Eu,
                1 => Faction::Apa,
                2 => Faction::Gla,
                t => return Err(DecodeError::BadTag(t)),
            };
            let starting_credits = r.i64()?;
            let ai = match (version, r.u8()?) {
                (_, 0) => None,
                (..=3, _) => Some(AiLevel::Easy),
                (_, 1) => Some(AiLevel::Easy),
                (_, 2) => Some(AiLevel::Normal),
                (6.., 3) => Some(AiLevel::Onslaught),
                (_, t) => return Err(DecodeError::BadTag(t)),
            };
            let general = match version {
                3.. => Some(r.u16()?).filter(|&g| g != NO_GENERAL).map(crate::defs::GeneralId),
                _ => None,
            };
            players.push(PlayerSetup { faction, starting_credits, ai, general });
        }
        let length_ticks = r.u32()?;
        let mut commands = Vec::new();
        for _ in 0..r.u32()? {
            let tick = r.u32()?;
            let player = PlayerId(r.u8()?);
            commands.push(Command { tick, player, kind: r.command()? });
        }
        let mut hashes = Vec::new();
        for _ in 0..r.u32()? {
            hashes.push((r.u32()?, r.u64()?));
        }
        if r.at != bytes.len() {
            return Err(DecodeError::Truncated);
        }
        Ok(Replay { map, map_fingerprint, seed, players, length_ticks, commands, hashes })
    }
}

impl Replay {
    /// Whether this replay can be simulated on `map` with `rules`: the map is the one it was
    /// recorded on (v1 replays carry no fingerprint and pass), the map has enough player starts,
    /// and every general exists and belongs to its player's faction. Replay files are untrusted
    /// input; call this before `verify_replay` or `World::new`, which assume a valid setup.
    pub fn check(&self, map: &crate::map::MapDef, rules: &crate::defs::Rules) -> Result<(), String> {
        if let Some(f) = self.map_fingerprint
            && f != map.fingerprint()
        {
            return Err(format!(
                "map {:?} changed since this replay was recorded (fingerprint {f:016x}, now {:016x})",
                self.map,
                map.fingerprint()
            ));
        }
        if self.players.len() > map.spawns.len() {
            return Err(format!(
                "{} players but map {:?} has {} starts",
                self.players.len(),
                self.map,
                map.spawns.len()
            ));
        }
        for (i, p) in self.players.iter().enumerate() {
            let Some(g) = p.general else { continue };
            let general =
                rules.generals.get(g.0 as usize).ok_or_else(|| format!("player {i}: unknown general id {}", g.0))?;
            if general.faction != p.faction {
                return Err(format!("player {i}: general {} does not belong to {:?}", general.name, p.faction));
            }
        }
        Ok(())
    }
}

/// Little-endian writer shared by replays and the lockstep wire format (`net`).
pub(crate) struct Writer(pub(crate) Vec<u8>);

impl Writer {
    pub(crate) fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    pub(crate) fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub(crate) fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub(crate) fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i64(&mut self, v: i64) {
        self.u64(v as u64);
    }
    pub(crate) fn bytes(&mut self, b: &[u8]) {
        self.u32(b.len() as u32);
        self.0.extend_from_slice(b);
    }
    fn id(&mut self, id: EntityId) {
        self.u32(id.index);
        self.u32(id.generation);
    }
    fn ids(&mut self, ids: &[EntityId]) {
        self.u32(ids.len() as u32);
        for &i in ids {
            self.id(i);
        }
    }
    fn pos(&mut self, p: Vec2) {
        self.i64(p.x.0);
        self.i64(p.y.0);
    }
    pub(crate) fn command(&mut self, k: &CommandKind) {
        match k {
            CommandKind::Move { units, target } => {
                self.u8(0);
                self.ids(units);
                self.pos(*target);
            }
            CommandKind::Gather { units, pile } => {
                self.u8(1);
                self.ids(units);
                self.id(*pile);
            }
            CommandKind::Build { builder, def, pos } => {
                self.u8(2);
                self.id(*builder);
                self.u16(def.0);
                self.pos(*pos);
            }
            CommandKind::Construct { units, site } => {
                self.u8(3);
                self.ids(units);
                self.id(*site);
            }
            CommandKind::Train { producer, def } => {
                self.u8(4);
                self.id(*producer);
                self.u16(def.0);
            }
            CommandKind::CancelTrain { producer } => {
                self.u8(5);
                self.id(*producer);
            }
            CommandKind::Stop { units } => {
                self.u8(6);
                self.ids(units);
            }
            CommandKind::Surrender => self.u8(7),
            CommandKind::Attack { units, target } => {
                self.u8(8);
                self.ids(units);
                self.id(*target);
            }
            CommandKind::AttackMove { units, target } => {
                self.u8(9);
                self.ids(units);
                self.pos(*target);
            }
            CommandKind::Deploy { units, deployed } => {
                self.u8(10);
                self.ids(units);
                self.u8(*deployed as u8);
            }
            CommandKind::Power { at } => {
                self.u8(11);
                self.pos(*at);
            }
            CommandKind::Research { producer, upgrade } => {
                self.u8(12);
                self.id(*producer);
                self.u16(upgrade.0);
            }
        }
    }
}

pub(crate) struct Reader<'a> {
    pub(crate) b: &'a [u8],
    pub(crate) at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], DecodeError> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.b.len()).ok_or(DecodeError::Truncated)?;
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }
    pub(crate) fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }
    pub(crate) fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub(crate) fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub(crate) fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn i64(&mut self) -> Result<i64, DecodeError> {
        Ok(self.u64()? as i64)
    }
    pub(crate) fn bytes(&mut self) -> Result<&[u8], DecodeError> {
        let n = self.u32()? as usize;
        self.take(n)
    }
    fn id(&mut self) -> Result<EntityId, DecodeError> {
        Ok(EntityId { index: self.u32()?, generation: self.u32()? })
    }
    fn ids(&mut self) -> Result<Vec<EntityId>, DecodeError> {
        let n = self.u32()? as usize;
        // Bound the allocation by what the input can actually hold (8 bytes per id).
        if n > (self.b.len() - self.at) / 8 {
            return Err(DecodeError::Truncated);
        }
        (0..n).map(|_| self.id()).collect()
    }
    fn pos(&mut self) -> Result<Vec2, DecodeError> {
        Ok(Vec2::new(Fx(self.i64()?), Fx(self.i64()?)))
    }
    pub(crate) fn command(&mut self) -> Result<CommandKind, DecodeError> {
        Ok(match self.u8()? {
            0 => CommandKind::Move { units: self.ids()?, target: self.pos()? },
            1 => CommandKind::Gather { units: self.ids()?, pile: self.id()? },
            2 => CommandKind::Build { builder: self.id()?, def: DefId(self.u16()?), pos: self.pos()? },
            3 => CommandKind::Construct { units: self.ids()?, site: self.id()? },
            4 => CommandKind::Train { producer: self.id()?, def: DefId(self.u16()?) },
            5 => CommandKind::CancelTrain { producer: self.id()? },
            6 => CommandKind::Stop { units: self.ids()? },
            7 => CommandKind::Surrender,
            8 => CommandKind::Attack { units: self.ids()?, target: self.id()? },
            9 => CommandKind::AttackMove { units: self.ids()?, target: self.pos()? },
            10 => CommandKind::Deploy { units: self.ids()?, deployed: self.u8()? != 0 },
            11 => CommandKind::Power { at: self.pos()? },
            12 => CommandKind::Research { producer: self.id()?, upgrade: crate::defs::UpgradeId(self.u16()?) },
            t => return Err(DecodeError::BadTag(t)),
        })
    }
}
