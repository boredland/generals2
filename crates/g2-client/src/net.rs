//! Matches over the network: a TCP link carrying `g2_sim::net::Message`s, the handshake, and
//! this client's side of the lockstep. The host picks the match and plays player 0; the joiner
//! plays player 1, against the host (versus) or beside them against the Onslaught's waves
//! (co-op, the waves an AI player 2 that runs in both sims). The lobby (`lobby.rs`) sets the
//! match up; `--host`/`--join` skip it.
//!
//! Frames are a little-endian `u32` length, then the message. One thread reads and one writes,
//! so neither a slow peer nor a full socket buffer ever blocks a frame.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError, channel};
use std::thread::JoinHandle;
use std::time::Duration;

use g2_sim::net::{Lockstep, Message, NetError, PROTOCOL, build_id};
use g2_sim::{CommandKind, Faction, PlayerId, PlayerSetup, Replay, World};

/// Larger than any message this protocol sends (a `Hello` is well under a kilobyte).
const MAX_FRAME: usize = 1 << 20;
/// How long the joiner waits for the host's `Hello`, and the host for the joiner's `Join`.
const HANDSHAKE: Duration = Duration::from_secs(30);
/// Fixed steps without the peer's orders before the HUD says so: half a second.
const WAIT_SHOWN: u32 = 15;
/// How long one write may block on a peer that stopped reading before the link gives up.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Link {
    out: Option<Sender<Vec<u8>>>,
    inbox: Mutex<Receiver<Vec<u8>>>,
    writer: Option<JoinHandle<()>>,
    /// Kept to shut the socket down on drop, so the reader thread's blocking read returns.
    socket: TcpStream,
}

impl Link {
    pub(crate) fn new(stream: TcpStream) -> std::io::Result<Link> {
        stream.set_nodelay(true)?;
        // A peer that stops reading fills the socket buffer; a write then gives up instead of
        // blocking forever, so closing the link (and quitting the match) can't hang.
        stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
        let socket = stream.try_clone()?;
        let (mut reader, mut writer) = (stream.try_clone()?, stream);
        let (in_tx, in_rx) = channel::<Vec<u8>>();
        let (out_tx, out_rx) = channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut len = [0u8; 4];
            while reader.read_exact(&mut len).is_ok() {
                let n = u32::from_le_bytes(len) as usize;
                let mut buf = vec![0; n.min(MAX_FRAME)];
                if n > MAX_FRAME || reader.read_exact(&mut buf).is_err() || in_tx.send(buf).is_err() {
                    break;
                }
            }
        });
        let writer = std::thread::spawn(move || {
            for buf in out_rx {
                let frame = [&(buf.len() as u32).to_le_bytes()[..], &buf].concat();
                if writer.write_all(&frame).is_err() {
                    break;
                }
            }
            let _ = writer.shutdown(Shutdown::Both);
        });
        Ok(Link { out: Some(out_tx), inbox: Mutex::new(in_rx), writer: Some(writer), socket })
    }

    pub fn send(&self, m: &Message) {
        if let Some(out) = &self.out {
            // A closed link shows up on the receiving side.
            let _ = out.send(m.encode());
        }
    }

    /// The next message, if one has arrived.
    pub fn try_recv(&self) -> Result<Option<Message>, String> {
        match self.inbox.lock().unwrap().try_recv() {
            Ok(b) => Message::decode(&b).map(Some).map_err(|e| format!("bad message from the other player: {e:?}")),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err("the other player left".into()),
        }
    }

    fn recv_timeout(&self, d: Duration) -> Result<Message, String> {
        match self.inbox.lock().unwrap().recv_timeout(d) {
            Ok(b) => Message::decode(&b).map_err(|e| format!("bad message from the other player: {e:?}")),
            Err(RecvTimeoutError::Timeout) => Err("no answer from the other player".into()),
            Err(RecvTimeoutError::Disconnected) => Err("the other player left".into()),
        }
    }
}

impl Drop for Link {
    /// Sends what is queued before closing (bounded by the write timeout), so the peer gets
    /// every turn this side played, then closes the socket.
    fn drop(&mut self) {
        self.out = None;
        if let Some(w) = self.writer.take() {
            let _ = w.join();
        }
        let _ = self.socket.shutdown(Shutdown::Both);
    }
}

/// Host: waits for one player on `port`, then offers it `setup` (a replay with no commands)
/// for player 1. Anyone who can reach the port can join: for a LAN or trusted players.
pub fn host(port: u16, setup: Replay) -> Result<Link, String> {
    let listener = TcpListener::bind(("0.0.0.0", port)).map_err(|e| format!("can't listen on port {port}: {e}"))?;
    eprintln!("waiting for a player to join on port {port}");
    let (stream, peer) = listener.accept().map_err(|e| format!("accept failed: {e}"))?;
    let link = Link::new(stream).map_err(|e| e.to_string())?;
    let build = build_id(g2_rules::rules());
    link.send(&Message::Hello { protocol: PROTOCOL, build, setup });
    match link.recv_timeout(HANDSHAKE) {
        Ok(Message::Join { protocol, build: theirs }) if protocol == PROTOCOL && theirs == build => {
            eprintln!("{peer} joined");
            Ok(link)
        }
        Ok(Message::Join { protocol, .. }) if protocol != PROTOCOL => {
            Err(format!("{peer} speaks protocol {protocol}, this build {PROTOCOL}: a different game version"))
        }
        Ok(Message::Join { .. }) => Err(format!("{peer} runs a different game version")),
        Ok(m) => Err(format!("{peer} sent {m:?} instead of joining")),
        // The joiner closes the link when it can't play the match (version, map or rules).
        Err(e) => Err(format!("{peer} did not join: {e}")),
    }
}

/// Joiner: connects to `addr` and receives the match, refusing a host that runs a different
/// game version. The caller checks the match against its own maps, then calls `accept`.
pub fn join(addr: &str) -> Result<(Link, Replay), String> {
    let stream = TcpStream::connect(addr).map_err(|e| format!("can't reach {addr}: {e}"))?;
    let link = Link::new(stream).map_err(|e| e.to_string())?;
    match link.recv_timeout(HANDSHAKE)? {
        Message::Hello { protocol, build, setup } if protocol == PROTOCOL && build == build_id(g2_rules::rules()) => {
            Ok((link, setup))
        }
        Message::Hello { protocol, .. } if protocol != PROTOCOL => {
            Err(format!("the host speaks protocol {protocol}, this build {PROTOCOL}: a different game version"))
        }
        Message::Hello { .. } => Err("the host runs a different game version".into()),
        m => Err(format!("the host sent {m:?} instead of a match")),
    }
}

pub fn accept(link: &Link) {
    link.send(&Message::Join { protocol: PROTOCOL, build: build_id(g2_rules::rules()) });
}

/// The team host and joiner share in co-op; the waves play alone (team 0).
pub const COOP_TEAM: u8 = 1;

/// The waves' faction in co-op: the first, in `Faction::ALL` order, that neither defender
/// plays, so the waves look like neither of them; EU, APA, GLA all taken can't happen with two.
pub fn wave_faction(defenders: [Faction; 2]) -> Faction {
    Faction::ALL.into_iter().find(|f| !defenders.contains(f)).unwrap_or(Faction::Gla)
}

/// Co-op's third player: the Onslaught AI of `faction`, as a skirmish's Onslaught seat plays
/// (`ai_perks`, `ai_rank`, no general), on no team. Player 2 on a two-start map, so it has no
/// start: its waves come from `World::onslaught_origin` and split between both bases.
pub fn waves(faction: Faction) -> PlayerSetup {
    let level = g2_sim::AiLevel::Onslaught;
    PlayerSetup {
        faction,
        starting_credits: 5_000,
        ai: Some(level),
        general: None,
        perks: g2_sim::ai::ai_perks(g2_rules::rules(), None, level),
        rank: g2_sim::ai::ai_rank(level),
        team: 0,
    }
}

/// The original map files or the built-in map by name, as a match on it needs: the sim's map
/// and the parsed source for drawing (`None` for the built-in map).
pub fn resolve_map(name: &str) -> Result<(g2_sim::MapDef, Option<g2_maps::MapFile>), String> {
    let source =
        if name.contains(':') { Some(g2_maps::load_file_named(&g2_maps::vendored_root(), name)?) } else { None };
    let map = match &source {
        Some(file) => file.to_map_def(name),
        None => g2_maps::resolve(name)?,
    };
    Ok((map, source))
}

/// Whether `setup` is a match two machines can play: two humans, host and joiner, against each
/// other (no teams), or on one team with the Onslaught's waves as a third player of no team.
pub fn check_shape(setup: &Replay) -> Result<(), String> {
    let p = &setup.players;
    let humans = p.len() >= 2 && p[0].ai.is_none() && p[1].ai.is_none();
    let versus = humans && p.len() == 2 && p[0].team == 0 && p[1].team == 0;
    let coop = humans
        && p.len() == 3
        && p[0].team != 0
        && p[0].team == p[1].team
        && p[2].ai == Some(g2_sim::AiLevel::Onslaught)
        && p[2].team != p[0].team;
    if versus || coop {
        Ok(())
    } else {
        Err("the host offered a match that is neither two players against each other nor two against the waves".into())
    }
}

/// The match `setup` offers, played as `me`: on a map this build has, unchanged, with generals
/// this build knows, of a shape two machines can play.
pub fn match_sim(setup: &Replay, me: PlayerId) -> Result<crate::sim::Sim, String> {
    let (map, source) = resolve_map(&setup.map)?;
    setup.check(&map, g2_rules::rules())?;
    check_shape(setup)?;
    let mut sim = crate::sim::Sim::new(map, source, setup.seed, setup.players.clone());
    sim.local = me;
    Ok(sim)
}

/// This client's side of a network match.
pub struct Online {
    link: Link,
    lockstep: Lockstep,
    /// Why the match can't go on, once it can't.
    failed: Option<String>,
    /// Fixed steps in a row spent waiting for the peer.
    stalls: u32,
}

impl Online {
    pub fn new(link: Link, me: PlayerId, peer: PlayerId) -> Online {
        Online { link, lockstep: Lockstep::new(me, peer), failed: None, stalls: 0 }
    }

    pub fn order(&mut self, kind: CommandKind) {
        self.lockstep.order(kind);
    }

    /// Whether `world` may step now; exchanges turns with the peer. `first`: the first step of
    /// this frame. Later ones go ahead only to catch up with a peer that is ahead.
    pub fn ready(&mut self, world: &mut World, first: bool) -> bool {
        if self.failed.is_some() {
            return false;
        }
        match self.exchange(world, first) {
            Ok(go) => {
                if first {
                    self.stalls = if go { 0 } else { self.stalls + 1 };
                }
                go
            }
            Err(e) => {
                self.failed = Some(e);
                false
            }
        }
    }

    fn exchange(&mut self, world: &mut World, first: bool) -> Result<bool, String> {
        while let Some(m) = self.link.try_recv()? {
            match m {
                Message::Turn(t) => self.lockstep.receive(world, t).map_err(describe)?,
                m => return Err(format!("unexpected message from the other player: {m:?}")),
            }
        }
        if !first && !self.lockstep.behind(world) {
            return Ok(false);
        }
        let mut out = Vec::new();
        let go = self.lockstep.before_step(world, &mut out).map_err(describe)?;
        for t in out {
            self.link.send(&Message::Turn(t));
        }
        Ok(go)
    }

    /// What the HUD should say about the link, if anything.
    pub fn status(&self) -> Option<String> {
        match &self.failed {
            Some(e) => Some(e.clone()),
            None if self.stalls > WAIT_SHOWN => Some("Waiting for the other player".into()),
            None => None,
        }
    }
}

fn describe(e: NetError) -> String {
    match e {
        NetError::Desync { tick, .. } => {
            format!("Out of sync with the other player at {}: the match can't go on", crate::hud::clock(tick))
        }
        NetError::Protocol(s) => format!("Network error: {s}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Sim;
    use g2_sim::{Faction, PlayerSetup};

    /// Host and joiner over a real loopback socket, each with its own `Sim`, the joiner set up
    /// from the host's `Hello`: both players' orders run on both machines, which end in the
    /// same state.
    #[test]
    fn host_and_joiner_play_the_same_match_over_tcp() {
        let players: Vec<PlayerSetup> = [Faction::Apa, Faction::Eu]
            .map(|faction| PlayerSetup {
                faction,
                starting_credits: 5_000,
                ai: None,
                general: None,
                perks: Vec::new(),
                rank: 1,
                team: 0,
            })
            .to_vec();
        let mut host_sim = Sim::new(g2_rules::crater_ridge(), None, 5, players);
        let setup = host_sim.world.replay(&host_sim.setup);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let host_link = std::thread::spawn(move || host(port, setup));
        let (link, hello) = loop {
            match join(&format!("127.0.0.1:{port}")) {
                Ok(j) => break j,
                Err(_) => std::thread::sleep(Duration::from_millis(10)),
            }
        };
        accept(&link);
        let mut joiner = Sim::new(g2_rules::crater_ridge(), None, hello.seed, hello.players.clone());
        joiner.local = PlayerId(1);
        joiner.online = Some(Online::new(link, PlayerId(1), PlayerId(0)));
        host_sim.online = Some(Online::new(host_link.join().unwrap().unwrap(), PlayerId(0), PlayerId(1)));

        let first_unit = |s: &Sim| {
            s.world.entities().find(|e| e.owner == Some(s.local) && s.world.rules.def(e.def).is_unit()).unwrap().id
        };
        let mut sims = [host_sim, joiner];
        let target = g2_sim::Vec2::from_int(150, 150);
        let started = std::time::Instant::now();
        while sims.iter().any(|s| s.world.tick() < 600) {
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "stuck at ticks {} / {}",
                sims[0].world.tick(),
                sims[1].world.tick()
            );
            for s in &mut sims {
                if s.world.tick() == 60 && s.online.as_ref().unwrap().lockstep.me() == s.local {
                    let u = first_unit(s);
                    s.order(CommandKind::Move { units: vec![u], target });
                }
                if s.world.tick() < 600 && s.may_step(true) {
                    s.world.step();
                }
                assert_eq!(s.online.as_ref().unwrap().failed, None);
            }
            // Spinning without a pause starved the socket threads on the macOS CI runner.
            std::thread::sleep(Duration::from_micros(200));
        }
        assert_eq!(sims[0].world.state_hash(), sims[1].world.state_hash());
        let log = sims[0].world.replay(&sims[0].setup).commands;
        assert_eq!(log, sims[1].world.replay(&sims[1].setup).commands);
        assert!(log.iter().any(|c| c.player == PlayerId(0)) && log.iter().any(|c| c.player == PlayerId(1)));
    }

    /// Co-op Onslaught over a real loopback socket, to the end: host and joiner on one team, the
    /// waves a third, AI player that runs in both sims. Each side only sends a dozer out, so the
    /// waves overrun both bases: the match ends with the same winner on both machines on the
    /// same tick, and both agree on every hash check and on the final state.
    #[test]
    fn coop_onslaught_runs_to_the_end_in_sync_over_tcp() {
        let mut players: Vec<PlayerSetup> = [Faction::Eu, Faction::Gla]
            .map(|faction| PlayerSetup {
                faction,
                starting_credits: 5_000,
                ai: None,
                general: None,
                perks: Vec::new(),
                rank: 1,
                team: COOP_TEAM,
            })
            .to_vec();
        players.push(waves(wave_faction([Faction::Eu, Faction::Gla])));
        let mut host_sim = Sim::new(g2_rules::crater_ridge(), None, 7, players);
        let setup = host_sim.world.replay(&host_sim.setup);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let host_link = std::thread::spawn(move || host(port, setup));
        let (link, hello) = loop {
            match join(&format!("127.0.0.1:{port}")) {
                Ok(j) => break j,
                Err(_) => std::thread::sleep(Duration::from_millis(10)),
            }
        };
        let mut joiner = match_sim(&hello, PlayerId(1)).expect("a co-op match the joiner can play");
        accept(&link);
        joiner.online = Some(Online::new(link, PlayerId(1), PlayerId(0)));
        host_sim.online = Some(Online::new(host_link.join().unwrap().unwrap(), PlayerId(0), PlayerId(1)));

        let mut sims = [host_sim, joiner];
        let end = g2_sim::onslaught::wave_tick(g2_sim::onslaught::WAVES) + 30 * 300;
        let started = std::time::Instant::now();
        let over = |s: &Sim| s.world.winner().is_some() || s.world.tick() >= end;
        while !sims.iter().all(over) {
            assert!(
                started.elapsed() < Duration::from_secs(240),
                "stuck at ticks {} / {}",
                sims[0].world.tick(),
                sims[1].world.tick()
            );
            for s in &mut sims {
                // A scout each, so both players' orders go over the wire.
                if s.world.tick() == 90 {
                    let me = s.local;
                    let rules = s.world.rules;
                    let dozer = s
                        .world
                        .entities()
                        .find(|e| e.owner == Some(me) && rules.def(e.def).is_constructor())
                        .unwrap()
                        .id;
                    s.order(CommandKind::Move { units: vec![dozer], target: g2_sim::Vec2::from_int(200, 200) });
                }
                // Several steps per poll: the match runs ten minutes of game time.
                for first in [true, false, false, false] {
                    if over(s) || !s.may_step(first) {
                        break;
                    }
                    s.world.step();
                }
                assert_eq!(s.online.as_ref().unwrap().failed, None);
            }
            // Spinning without a pause starved the socket threads on the macOS CI runner.
            std::thread::sleep(Duration::from_micros(200));
        }
        let [a, b] = &sims;
        assert_eq!(a.world.tick(), b.world.tick(), "the match ended on the same tick on both");
        assert_eq!(a.world.state_hash(), b.world.state_hash());
        assert_eq!(a.world.winner(), Some(PlayerId(2)), "two idle defenders are overrun");
        assert!(a.world.player(PlayerId(0)).defeated && a.world.player(PlayerId(1)).defeated);
        let log = a.world.replay(&a.setup).commands;
        assert_eq!(log, b.world.replay(&b.setup).commands);
        assert!(log.iter().any(|c| c.player == PlayerId(0)) && log.iter().any(|c| c.player == PlayerId(1)));
        let replay = a.world.replay(&a.setup);
        assert_eq!(
            g2_sim::verify_replay(&replay, g2_rules::crater_ridge(), g2_rules::rules()),
            Ok(a.world.state_hash())
        );
    }

    /// The same over the client's real schedule (`SimPlugin`'s fixed update), for 10 seconds of
    /// game time at 30 ticks a second: both apps keep pace, neither stalls for long, and they
    /// agree on every hash check (a desync would stop them with a message).
    #[test]
    fn two_client_apps_keep_pace_over_tcp() {
        use bevy::prelude::*;
        use bevy::time::TimeUpdateStrategy;
        let players: Vec<PlayerSetup> = [Faction::Gla, Faction::Apa]
            .map(|faction| PlayerSetup {
                faction,
                starting_credits: 5_000,
                ai: None,
                general: None,
                perks: Vec::new(),
                rank: 1,
                team: 0,
            })
            .to_vec();
        let mut host_sim = Sim::new(g2_rules::crater_ridge(), None, 2, players.clone());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let setup = host_sim.world.replay(&host_sim.setup);
        let hosting = std::thread::spawn(move || host(port, setup));
        let (link, hello) = loop {
            if let Ok(j) = join(&format!("127.0.0.1:{port}")) {
                break j;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        accept(&link);
        let mut joiner = Sim::new(g2_rules::crater_ridge(), None, hello.seed, hello.players);
        joiner.local = PlayerId(1);
        joiner.online = Some(Online::new(link, PlayerId(1), PlayerId(0)));
        host_sim.online = Some(Online::new(hosting.join().unwrap().unwrap(), PlayerId(0), PlayerId(1)));
        let app = |sim: Sim| {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins)
                .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(16)))
                .insert_resource(sim)
                .add_plugins(crate::sim::SimPlugin);
            app
        };
        let mut apps = [app(host_sim), app(joiner)];
        // 16 ms frames, both apps in turn; the reader threads deliver between frames.
        for frame in 0..625 {
            for (i, a) in apps.iter_mut().enumerate() {
                if frame == 100 {
                    let mut sim = a.world_mut().resource_mut::<Sim>();
                    let me = sim.local;
                    let u = sim.world.entities().find(|e| e.owner == Some(me) && sim.world.rules.def(e.def).is_unit());
                    let u = u.unwrap().id;
                    sim.order(CommandKind::Move {
                        units: vec![u],
                        target: g2_sim::Vec2::from_int(100 + 20 * i as i64, 100),
                    });
                }
                a.update();
            }
            std::thread::sleep(Duration::from_micros(200));
        }
        let ticks: Vec<u32> = apps.iter().map(|a| a.world().resource::<Sim>().world.tick()).collect();
        for a in &apps {
            assert_eq!(a.world().resource::<Sim>().online.as_ref().unwrap().failed, None);
        }
        // 10 s of frames: about 300 ticks each, within a turn or two of each other.
        assert!(ticks.iter().all(|&t| (280..=301).contains(&t)), "ticks {ticks:?}");
        assert!(ticks[0].abs_diff(ticks[1]) <= 2 * g2_sim::net::TURN_TICKS, "ticks {ticks:?}");
        let log = |a: &App| {
            let s = a.world().resource::<Sim>();
            s.world.replay(&s.setup).commands
        };
        let (l0, l1) = (log(&apps[0]), log(&apps[1]));
        let n = ticks[0].min(ticks[1]);
        let upto = |l: &[g2_sim::Command]| l.iter().filter(|c| c.tick < n).cloned().collect::<Vec<_>>();
        assert_eq!(upto(&l0), upto(&l1));
        assert_eq!(upto(&l0).iter().map(|c| c.player.0).collect::<Vec<_>>(), vec![0, 1]);
    }

    /// Quitting an online match through the menu saves each side's replay, whether both quit
    /// on the same frame or the joiner a second after the host; both replays verify, and they
    /// agree on every command up to the shorter one's end, with orders from both players in it.
    #[test]
    fn quitting_an_online_match_saves_replays_that_agree() {
        for stagger in [0, 60] {
            quit_online_match(stagger);
        }
    }

    fn quit_online_match(stagger: usize) {
        use crate::menu::{Act, MenuPlugin, Pending, Ui};
        use bevy::prelude::*;
        use bevy::time::TimeUpdateStrategy;
        let players: Vec<PlayerSetup> = [Faction::Eu, Faction::Gla]
            .map(|faction| PlayerSetup {
                faction,
                starting_credits: 5_000,
                ai: None,
                general: None,
                perks: Vec::new(),
                rank: 1,
                team: 0,
            })
            .to_vec();
        let mut host_sim = Sim::new(g2_rules::crater_ridge(), None, 3, players);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let setup = host_sim.world.replay(&host_sim.setup);
        let hosting = std::thread::spawn(move || host(port, setup));
        let (link, hello) = loop {
            if let Ok(j) = join(&format!("127.0.0.1:{port}")) {
                break j;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        accept(&link);
        let mut joiner = Sim::new(g2_rules::crater_ridge(), None, hello.seed, hello.players);
        joiner.local = PlayerId(1);
        joiner.online = Some(Online::new(link, PlayerId(1), PlayerId(0)));
        host_sim.online = Some(Online::new(hosting.join().unwrap().unwrap(), PlayerId(0), PlayerId(1)));
        let dir = std::env::temp_dir().join(format!("g2-quit-{}-{port}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = [dir.join("host.g2rp"), dir.join("join.g2rp")];
        let app = |sim: Sim, path: &std::path::Path| {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins)
                .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(16)))
                .insert_resource(sim)
                .insert_resource(Ui { screen: crate::menu::Screen::Match, ..default() })
                .init_resource::<crate::input::Selection>()
                .init_resource::<crate::input::Placement>()
                .init_resource::<crate::input::PowerAim>()
                .init_resource::<crate::sim::Previous>()
                .init_resource::<crate::sim::Hits>()
                .init_resource::<crate::sim::Deliveries>()
                .init_resource::<crate::view::Mirror>()
                .init_resource::<ButtonInput<MouseButton>>()
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<crate::settings::Settings>()
                .add_plugins((crate::sim::SimPlugin, MenuPlugin { blocking: true }))
                .insert_resource(crate::sim::RecordTo(path.to_string_lossy().into_owned()))
                .add_systems(Last, crate::sim::save_replay_on_exit);
            app
        };
        let mut apps = [app(host_sim, &paths[0]), app(joiner, &paths[1])];
        let mut quit = [None, None];
        for frame in 0..500 {
            for (i, a) in apps.iter_mut().enumerate() {
                if quit[i].is_some() {
                    continue;
                }
                if frame == 60 {
                    let mut sim = a.world_mut().resource_mut::<Sim>();
                    let me = sim.local;
                    let u = sim.world.entities().find(|e| e.owner == Some(me) && sim.world.rules.def(e.def).is_unit());
                    let u = u.unwrap().id;
                    sim.order(CommandKind::Move {
                        units: vec![u],
                        target: g2_sim::Vec2::from_int(150, 150 + 10 * i as i64),
                    });
                }
                if frame == 400 + stagger * i {
                    let mut pending = a.world_mut().resource_mut::<Pending>();
                    pending.0.push(Act::QuitToMenu);
                    pending.0.push(Act::QuitGame);
                }
                a.update();
                if let Some(exit) = a.should_exit() {
                    quit[i] = Some(exit);
                }
            }
            std::thread::sleep(Duration::from_micros(200));
        }
        assert!(quit.iter().all(Option::is_some), "stagger {stagger}: both apps quit: {quit:?}");
        let replays: Vec<g2_sim::Replay> =
            paths.iter().map(|p| g2_sim::Replay::decode(&std::fs::read(p).expect("replay saved")).unwrap()).collect();
        for r in &replays {
            assert!(g2_sim::verify_replay(r, g2_rules::crater_ridge(), g2_rules::rules()).is_ok(), "stagger {stagger}");
        }
        let n = replays[0].length_ticks.min(replays[1].length_ticks);
        // The host quits after 400 frames of 16 ms: about 190 ticks of play.
        assert!(n > 150, "stagger {stagger}: the match ran {n} ticks");
        let upto = |r: &g2_sim::Replay| r.commands.iter().filter(|c| c.tick < n).cloned().collect::<Vec<_>>();
        assert_eq!(upto(&replays[0]), upto(&replays[1]), "stagger {stagger}");
        let players: std::collections::BTreeSet<u8> = upto(&replays[0]).iter().map(|c| c.player.0).collect();
        assert_eq!(players.into_iter().collect::<Vec<_>>(), vec![0, 1], "stagger {stagger}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// When the other side goes away, the match stops with a message instead of hanging.
    #[test]
    fn a_vanished_peer_stops_the_match_with_a_message() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let near = std::thread::spawn(move || Link::new(listener.accept().unwrap().0).unwrap());
        let far = Link::new(TcpStream::connect(addr).unwrap()).unwrap();
        let near = near.join().unwrap();
        let players = Sim::players([Faction::Eu, Faction::Gla], false).map(|p| PlayerSetup { ai: None, ..p });
        let mut sim = Sim::new(g2_rules::crater_ridge(), None, 1, players);
        sim.online = Some(Online::new(near, PlayerId(0), PlayerId(1)));
        drop(far);
        let mut stopped = false;
        for _ in 0..5_000 {
            if !sim.may_step(true) && sim.online.as_ref().unwrap().failed.is_some() {
                stopped = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(stopped);
        assert_eq!(sim.online.as_ref().unwrap().status().as_deref(), Some("the other player left"));
    }
}
