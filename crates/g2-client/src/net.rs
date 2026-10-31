//! Matches over the network: a TCP link carrying `g2_sim::net::Message`s, the handshake, and
//! this client's side of the lockstep. The host picks the match and plays player 0; the joiner
//! plays player 1, against the host (versus) or beside them against the Onslaught's waves
//! (co-op, the waves an AI player 2 that runs in both sims). The lobby (`lobby.rs`) sets the
//! match up; `--host`/`--join` skip it.
//!
//! Frames are a little-endian `u32` length, then the message. One thread reads and one writes,
//! so neither a slow peer nor a full socket buffer ever blocks a frame. The writer also sends a
//! `Ping` twice a second, which the other side's frame loop answers: the round trip it times
//! sets the delay orders run after (`Online::wish`), and a link silent for `DEAD_AFTER` (the
//! other machine gone without closing it: a pulled cable, a crash, a router dropping the
//! connection) ends the match with a message instead of a hang.
//!
//! Two machines on the internet need nothing between them but the host's port reachable: the
//! host's router forwards it (`portmap.rs` asks it to) or the host has a public IPv6 address.
//! No server of anyone's is involved.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use g2_sim::net::{DELAY_TURNS, Lockstep, MAX_DELAY_TURNS, Message, NetError, PROTOCOL, TURN_TICKS, build_id};
use g2_sim::{CommandKind, Faction, PlayerId, PlayerSetup, Replay, World};

use crate::portmap::{Addresses, PortMap, Router};

/// Larger than any message this protocol sends (a `Hello` is well under a kilobyte).
const MAX_FRAME: usize = 1 << 20;
/// How long the joiner waits for the host's `Hello`, and the host for the joiner's `Join`.
const HANDSHAKE: Duration = Duration::from_secs(30);
/// How long a connect may take: a round trip across the world is under half a second, so a
/// host that hasn't answered by then isn't there (or its port isn't forwarded).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Fixed steps without the peer's orders before the HUD says so: half a second.
const WAIT_SHOWN: u32 = 15;
/// How long one write may block on a peer that stopped reading before the link gives up.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// How often each side's writer pings.
const PING_EVERY: Duration = Duration::from_millis(500);
/// A link with nothing heard for this long is dead. Pings come from the writer thread, so a
/// machine busy loading the battlefield still sends them; twenty pings missed is no hiccup.
pub const DEAD_AFTER: Duration = Duration::from_secs(10);
/// The round trips the delay is chosen from: the slowest of these, so one quick ping after a
/// slow spell doesn't lower it.
const RTT_WINDOW: Duration = Duration::from_secs(10);

pub struct Link {
    out: Option<Sender<Vec<u8>>>,
    inbox: Mutex<Receiver<Vec<u8>>>,
    writer: Option<JoinHandle<()>>,
    /// Kept to shut the socket down on drop, so the reader thread's blocking read returns.
    socket: TcpStream,
    /// What a `Ping`'s stamp counts from: microseconds since this.
    started: Instant,
    /// When the reader thread last got a frame.
    heard: Arc<Mutex<Instant>>,
    /// Round trips timed, newest last, as long as `RTT_WINDOW` keeps them.
    rtts: Mutex<VecDeque<(Instant, Duration)>>,
    pub(crate) dead_after: Duration,
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
        let started = Instant::now();
        let heard = Arc::new(Mutex::new(started));
        let heard_by_reader = heard.clone();
        std::thread::spawn(move || {
            let mut len = [0u8; 4];
            while reader.read_exact(&mut len).is_ok() {
                let n = u32::from_le_bytes(len) as usize;
                let mut buf = vec![0; n.min(MAX_FRAME)];
                if n > MAX_FRAME || reader.read_exact(&mut buf).is_err() || in_tx.send(buf).is_err() {
                    break;
                }
                *heard_by_reader.lock().unwrap() = Instant::now();
            }
        });
        let writer = std::thread::spawn(move || {
            let mut pinged: Option<Instant> = None;
            loop {
                // Due pings go first, so a steady stream of turns can't hold them back.
                let due = pinged.map_or(Duration::ZERO, |p| PING_EVERY.saturating_sub(p.elapsed()));
                let buf = if due.is_zero() {
                    pinged = Some(Instant::now());
                    Message::Ping { stamp: started.elapsed().as_micros() as u64 }.encode()
                } else {
                    match out_rx.recv_timeout(due) {
                        Ok(buf) => buf,
                        Err(RecvTimeoutError::Timeout) => continue,
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                };
                let frame = [&(buf.len() as u32).to_le_bytes()[..], &buf].concat();
                if writer.write_all(&frame).is_err() {
                    break;
                }
            }
            let _ = writer.shutdown(Shutdown::Both);
        });
        Ok(Link {
            out: Some(out_tx),
            inbox: Mutex::new(in_rx),
            writer: Some(writer),
            socket,
            started,
            heard,
            rtts: Mutex::new(VecDeque::new()),
            dead_after: DEAD_AFTER,
        })
    }

    pub fn send(&self, m: &Message) {
        if let Some(out) = &self.out {
            // A closed link shows up on the receiving side.
            let _ = out.send(m.encode());
        }
    }

    /// The next message, if one has arrived. Pings are answered and pongs timed here, on the
    /// frame loop, so a round trip includes the frames each side takes to look at a message,
    /// as every `Turn` does.
    pub fn try_recv(&self) -> Result<Option<Message>, String> {
        loop {
            let next = self.inbox.lock().unwrap().try_recv();
            match next {
                Ok(b) => match self.take(&b)? {
                    Some(m) => return Ok(Some(m)),
                    None => continue,
                },
                Err(TryRecvError::Empty) => {
                    let silent = self.heard.lock().unwrap().elapsed();
                    return if silent > self.dead_after {
                        Err(format!("no word from the other player for {} seconds", silent.as_secs()))
                    } else {
                        Ok(None)
                    };
                }
                Err(TryRecvError::Disconnected) => return Err("the other player left".into()),
            }
        }
    }

    /// A frame as a message for the caller, or `None` for a ping or pong, dealt with here.
    fn take(&self, b: &[u8]) -> Result<Option<Message>, String> {
        match Message::decode(b).map_err(|e| format!("bad message from the other player: {e:?}"))? {
            Message::Ping { stamp } => self.send(&Message::Pong { stamp }),
            Message::Pong { stamp } => {
                let now = Instant::now();
                let rtt = now.duration_since(self.started + Duration::from_micros(stamp));
                let mut rtts = self.rtts.lock().unwrap();
                rtts.push_back((now, rtt));
                while rtts.front().is_some_and(|&(at, _)| now - at > RTT_WINDOW) {
                    rtts.pop_front();
                }
            }
            m => return Ok(Some(m)),
        }
        Ok(None)
    }

    /// The slowest round trip of the last `RTT_WINDOW`, once one has been timed.
    pub fn rtt(&self) -> Option<Duration> {
        self.rtts.lock().unwrap().iter().map(|&(_, d)| d).max()
    }

    fn recv_timeout(&self, d: Duration) -> Result<Message, String> {
        let until = Instant::now() + d;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            let next = self.inbox.lock().unwrap().recv_timeout(left);
            match next {
                Ok(b) => {
                    if let Some(m) = self.take(&b)? {
                        return Ok(m);
                    }
                }
                Err(RecvTimeoutError::Timeout) => return Err("no answer from the other player".into()),
                Err(RecvTimeoutError::Disconnected) => return Err("the other player left".into()),
            }
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

/// Where a typed address points: `host`, `host:port`, `[v6]`, `[v6]:port` or a bare IPv6
/// address, the port Generals' 8088 when left out. `host` may be a name, looked up in DNS.
pub fn with_port(addr: &str) -> String {
    let addr = addr.trim();
    let bare_v6 = addr.matches(':').count() > 1 && !addr.starts_with('[');
    match addr.rsplit_once(':') {
        _ if bare_v6 => format!("[{addr}]:{}", g2_sim::net::GAME_PORT),
        Some((_, port)) if !port.contains(']') => addr.to_owned(),
        _ => format!("{addr}:{}", g2_sim::net::GAME_PORT),
    }
}

/// Connects to `addr` (see `with_port`), trying each address its name has in turn, within
/// `CONNECT_TIMEOUT` each; the error says what went wrong in words a player can act on.
pub fn connect(addr: &str) -> Result<TcpStream, String> {
    let target = with_port(addr);
    let mut found: Vec<SocketAddr> =
        target.to_socket_addrs().map_err(|e| format!("can't find {target}: {e}"))?.collect();
    // IPv4 first: a home host behind a router is reachable on its forwarded IPv4 port, while
    // its name's IPv6 address may sit behind a firewall that drops the connect.
    found.sort_by_key(|a| a.is_ipv6());
    let mut last = format!("{target} has no address");
    for a in found {
        match TcpStream::connect_timeout(&a, CONNECT_TIMEOUT) {
            Ok(s) => return Ok(s),
            Err(e) => {
                last = match e.kind() {
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => format!(
                        "no answer from {a} within {} seconds: no game is hosted there, or the host's \
                         router doesn't forward the port",
                        CONNECT_TIMEOUT.as_secs()
                    ),
                    std::io::ErrorKind::ConnectionRefused => format!("nothing is hosted at {a}"),
                    _ => format!("can't reach {a}: {e}"),
                }
            }
        }
    }
    Err(last)
}

/// Listens on `port` for both IPv4 and IPv6: one socket on systems whose IPv6 socket takes
/// IPv4 too (Linux, macOS), two on those whose doesn't (Windows). Port 0: any free one, the
/// same for both.
pub fn listen(port: u16) -> Result<(Vec<TcpListener>, u16), String> {
    let v6 = TcpListener::bind(("::", port)).ok();
    let port = match &v6 {
        Some(l) => l.local_addr().map_err(|e| e.to_string())?.port(),
        None => port,
    };
    let v4 = TcpListener::bind(("0.0.0.0", port));
    let listeners: Vec<TcpListener> = match (v6, v4) {
        (Some(a), Ok(b)) => vec![a, b],
        (None, Ok(b)) => vec![b],
        // Taken by the IPv6 socket itself, which takes IPv4 as well.
        (Some(a), Err(e)) if e.kind() == std::io::ErrorKind::AddrInUse => vec![a],
        (_, Err(e)) => return Err(format!("can't listen on port {port}: {e}")),
    };
    let port = listeners[0].local_addr().map_err(|e| e.to_string())?.port();
    Ok((listeners, port))
}

/// An address as a player reads it: an IPv4 peer on a dual-stack socket as plain IPv4.
pub fn shown(addr: SocketAddr) -> String {
    match addr {
        SocketAddr::V6(v6) => match v6.ip().to_ipv4_mapped() {
            Some(v4) => SocketAddr::new(v4.into(), v6.port()).to_string(),
            None => addr.to_string(),
        },
        SocketAddr::V4(_) => addr.to_string(),
    }
}

/// Host: waits for one player on `port`, then offers it `setup` (a replay with no commands)
/// for player 1. Anyone who can reach the port can join: for a LAN or trusted players.
/// With a `router`, asks it to forward the port first; the forward comes back with the link,
/// to be kept for the match (`Online::with_forward`).
pub fn host(port: u16, setup: Replay, router: Option<Router>) -> Result<(Link, Option<PortMap>), String> {
    let (listeners, port) = listen(port)?;
    let forward = router.map(|r| {
        let mut f = PortMap::request(port, r);
        f.wait(Duration::from_secs(3));
        f
    });
    eprintln!("waiting for a player to join on port {port}");
    for (line, _) in
        crate::lobby::address_lines(port, forward.as_ref().map(PortMap::state), &Addresses::of_this_machine())
    {
        eprintln!("{line}");
    }
    for l in &listeners {
        l.set_nonblocking(true).map_err(|e| e.to_string())?;
    }
    let (stream, peer) = loop {
        if let Some(got) = listeners.iter().find_map(|l| l.accept().ok()) {
            break got;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    stream.set_nonblocking(false).map_err(|e| e.to_string())?;
    let peer = shown(peer);
    let link = Link::new(stream).map_err(|e| e.to_string())?;
    let build = build_id(g2_rules::rules());
    link.send(&Message::Hello { protocol: PROTOCOL, build, setup });
    match link.recv_timeout(HANDSHAKE) {
        Ok(Message::Join { protocol, build: theirs }) if protocol == PROTOCOL && theirs == build => {
            eprintln!("{peer} joined");
            Ok((link, forward))
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
    let stream = connect(addr)?;
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
/// Missions are played alone.
pub fn check_shape(setup: &Replay) -> Result<(), String> {
    if setup.mission.is_some() {
        return Err("the host offered a mission, and missions are played alone".into());
    }
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

/// How long a turn lasts, against which round trips are measured.
const TURN: Duration = Duration::from_millis(1000 * TURN_TICKS as u64 / g2_sim::TICKS_PER_SECOND as u64);
/// Added to half the round trip when choosing a delay. A round trip as timed already holds the
/// frame each side takes to look at a message and the jitter of the last ten seconds; this is
/// one more fixed step (a thirtieth of a second) and a little for jitter not seen yet.
const MARGIN: Duration = Duration::from_millis(40);
/// First steps looked back over for stalls: two seconds.
const STALL_WINDOW: usize = 60;
/// Stalls in that window that ask for another turn of delay: a tenth.
const STALLS_TOO_MANY: usize = STALL_WINDOW / 10;
/// How long a delay raised for stalls is asked for after the last raise.
const STALL_HOLD: Duration = Duration::from_secs(15);
/// The most turns stalls may add to what the round trip asks for: stalls from a slow machine
/// rather than a slow link aren't cured by any delay.
const STALL_EXTRA: u32 = 2;

/// This client's side of a network match.
pub struct Online {
    link: Link,
    lockstep: Lockstep,
    /// Why the match can't go on, once it can't.
    failed: Option<String>,
    /// Fixed steps in a row spent waiting for the peer.
    stalls: u32,
    /// Whether each of the last `STALL_WINDOW` first steps waited.
    recent: VecDeque<bool>,
    /// The delay asked for after stalls, and when.
    raised: Option<(Instant, u32)>,
    /// The router's forward of the host's port, removed with the match.
    forward: Option<PortMap>,
    /// Off in tests that measure the fixed delay the protocol had before.
    pub(crate) adaptive: bool,
}

impl Online {
    pub fn new(link: Link, me: PlayerId, peer: PlayerId) -> Online {
        Online {
            link,
            lockstep: Lockstep::new(me, peer),
            failed: None,
            stalls: 0,
            recent: VecDeque::with_capacity(STALL_WINDOW),
            raised: None,
            forward: None,
            adaptive: true,
        }
    }

    /// Keeps the router's forward open for as long as the match lasts.
    pub fn with_forward(mut self, forward: Option<PortMap>) -> Online {
        self.forward = forward;
        self
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
                    if self.recent.len() == STALL_WINDOW {
                        self.recent.pop_front();
                    }
                    self.recent.push_back(!go);
                }
                go
            }
            Err(e) => {
                self.failed = Some(e);
                self.end_forward();
                false
            }
        }
    }

    /// The match is over (a winner, or the link lost): the router's forward goes, on a thread,
    /// so the frame doesn't wait for the router.
    pub fn end_forward(&mut self) {
        if let Some(f) = self.forward.take() {
            f.release();
        }
    }

    /// The app is quitting: the forward goes now, waiting for the router (a second or two at
    /// most), since a thread left behind would die with the process.
    pub fn drop_forward(&mut self) {
        self.forward = None;
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
        if first && self.adaptive {
            let turn = world.tick() / TURN_TICKS;
            let wish = self.wish(self.lockstep.delay_at(turn));
            self.lockstep.set_wish(wish);
        }
        let mut out = Vec::new();
        let go = self.lockstep.before_step(world, &mut out).map_err(describe)?;
        for t in out {
            self.link.send(&Message::Turn(t));
        }
        Ok(go)
    }

    /// The delay this side asks for: enough turns for a turn to cross half the slowest recent
    /// round trip, and a turn more (up to `STALL_EXTRA`) for a while after frequent stalls,
    /// which jitter causes without the round trip showing it.
    fn wish(&mut self, current: u32) -> u32 {
        let from_rtt = self.link.rtt().map_or(DELAY_TURNS, |rtt| {
            let need = rtt / 2 + MARGIN;
            need.as_micros().div_ceil(TURN.as_micros()) as u32
        });
        let now = Instant::now();
        let stalled = self.recent.iter().filter(|&&s| s).count() >= STALLS_TOO_MANY;
        if stalled && self.raised.is_none_or(|(at, _)| now - at > STALL_WINDOW as u32 * TURN / TURN_TICKS) {
            self.raised = Some((now, (current + 1).min(from_rtt + STALL_EXTRA)));
            self.recent.clear();
        }
        let raised = self.raised.filter(|&(at, _)| now - at < STALL_HOLD).map_or(0, |(_, d)| d);
        from_rtt.max(raised).clamp(DELAY_TURNS, MAX_DELAY_TURNS)
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
        let host_link = std::thread::spawn(move || host(port, setup, None));
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
        host_sim.online = Some(Online::new(host_link.join().unwrap().unwrap().0, PlayerId(0), PlayerId(1)));

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
        let host_link = std::thread::spawn(move || host(port, setup, None));
        let (link, hello) = loop {
            match join(&format!("127.0.0.1:{port}")) {
                Ok(j) => break j,
                Err(_) => std::thread::sleep(Duration::from_millis(10)),
            }
        };
        let mut joiner = match_sim(&hello, PlayerId(1)).expect("a co-op match the joiner can play");
        accept(&link);
        joiner.online = Some(Online::new(link, PlayerId(1), PlayerId(0)));
        host_sim.online = Some(Online::new(host_link.join().unwrap().unwrap().0, PlayerId(0), PlayerId(1)));

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
        let hosting = std::thread::spawn(move || host(port, setup, None));
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
        host_sim.online = Some(Online::new(hosting.join().unwrap().unwrap().0, PlayerId(0), PlayerId(1)));
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
        let hosting = std::thread::spawn(move || host(port, setup, None));
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
        host_sim.online = Some(Online::new(hosting.join().unwrap().unwrap().0, PlayerId(0), PlayerId(1)));
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

    /// A link with nothing on it, though still open (the other machine frozen, or its network
    /// gone without a word), ends the match once `dead_after` passes, with a message saying so.
    #[test]
    fn a_silent_peer_ends_the_match() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let started = std::time::Instant::now();
        let near = std::thread::spawn(move || Link::new(listener.accept().unwrap().0).unwrap());
        // A peer that connects and never sends a byte, nor reads one.
        let _silent = TcpStream::connect(addr).unwrap();
        let mut near = near.join().unwrap();
        near.dead_after = Duration::from_millis(700);
        let players = Sim::players([Faction::Eu, Faction::Gla], false).map(|p| PlayerSetup { ai: None, ..p });
        let mut sim = Sim::new(g2_rules::crater_ridge(), None, 1, players);
        sim.online = Some(Online::new(near, PlayerId(0), PlayerId(1)));
        let watching = std::time::Instant::now();
        while sim.online.as_ref().unwrap().failed.is_none() {
            assert!(watching.elapsed() < Duration::from_secs(5), "never noticed");
            if sim.may_step(true) {
                sim.world.step();
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(started.elapsed() >= Duration::from_millis(700));
        let status = sim.online.as_ref().unwrap().status().unwrap();
        assert!(status.starts_with("no word from the other player"), "{status}");
    }

    /// A live peer keeps the link alive by its pings alone, however long neither side has
    /// anything else to say, and the pings time the round trip. Pongs are answered on each
    /// side's poll (here every 10 ms), so a pong held back until the next ping would time at
    /// least `PING_EVERY`; a shared CI runner oversleeping its polls times far less (162 ms on
    /// macOS), so the bound is the ping interval, not a loopback figure.
    #[test]
    fn pings_keep_a_quiet_link_alive_and_time_it() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let near = std::thread::spawn(move || Link::new(listener.accept().unwrap().0).unwrap());
        let far = Link::new(TcpStream::connect(addr).unwrap()).unwrap();
        let mut near = near.join().unwrap();
        near.dead_after = Duration::from_millis(1200);
        for _ in 0..250 {
            assert_eq!(near.try_recv(), Ok(None));
            assert_eq!(far.try_recv(), Ok(None));
            std::thread::sleep(Duration::from_millis(10));
        }
        let rtt = near.rtt().expect("a round trip timed");
        assert!(rtt < PING_EVERY, "{rtt:?} on loopback");
    }

    /// Addresses as players type them: a name, IPv4 or IPv6, with or without a port.
    #[test]
    fn typed_addresses_get_a_port() {
        for (typed, full) in [
            ("192.168.1.20", "192.168.1.20:8088"),
            ("192.168.1.20:47000", "192.168.1.20:47000"),
            ("example.org", "example.org:8088"),
            ("example.org:9", "example.org:9"),
            ("[::1]:47000", "[::1]:47000"),
            ("[2001:db8::7]", "[2001:db8::7]:8088"),
            ("2001:db8::7", "[2001:db8::7]:8088"),
            (" ::1 ", "[::1]:8088"),
        ] {
            assert_eq!(with_port(typed), full, "{typed}");
        }
        // Nothing listens on a port just freed: a refusal, said as such, at once.
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let e = connect(&format!("127.0.0.1:{port}")).unwrap_err();
        assert!(e.starts_with("nothing is hosted at"), "{e}");
        // A name looked up in DNS (the hosts file's, so no network is asked).
        let named = TcpListener::bind("127.0.0.1:0").unwrap();
        let s = connect(&format!("localhost:{}", named.local_addr().unwrap().port()));
        assert!(s.is_ok(), "{s:?}");
    }

    /// A joiner reaches a host by its IPv6 address, the host listening on both families; and an
    /// IPv4 joiner reaches the same host.
    #[test]
    fn joining_over_ipv6_loopback() {
        if TcpListener::bind("[::1]:0").is_err() {
            eprintln!("no IPv6 on this machine: skipped");
            return;
        }
        for addr in ["[::1]", "127.0.0.1"] {
            let players = Sim::players([Faction::Eu, Faction::Gla], false).map(|p| PlayerSetup { ai: None, ..p });
            let sim = Sim::new(g2_rules::crater_ridge(), None, 3, players);
            let setup = sim.world.replay(&sim.setup);
            let port = TcpListener::bind("[::]:0").unwrap().local_addr().unwrap().port();
            let hosting = std::thread::spawn(move || host(port, setup, None));
            let started = std::time::Instant::now();
            let (link, hello) = loop {
                match join(&format!("{addr}:{port}")) {
                    Ok(j) => break j,
                    Err(e) => assert!(started.elapsed() < Duration::from_secs(10), "{addr}: {e}"),
                }
                std::thread::sleep(Duration::from_millis(10));
            };
            accept(&link);
            assert!(hosting.join().unwrap().is_ok(), "{addr}");
            assert_eq!(hello.seed, 3);
        }
    }

    /// A link that delays every byte by `one_way` plus up to `jitter`, in order, as a long
    /// internet path does; bytes go through in chunks as they come.
    fn latency_proxy(target: SocketAddr, one_way: Duration, jitter: Duration, seed: u64) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (a, _) = listener.accept().unwrap();
            let b = TcpStream::connect(target).unwrap();
            for (from, to, seed) in [(a.try_clone().unwrap(), b.try_clone().unwrap(), seed), (b, a, seed ^ 0x9e37)] {
                let (tx, rx) = channel::<(Instant, Vec<u8>)>();
                let mut from = from;
                std::thread::spawn(move || {
                    let (mut last, mut rng) = (Instant::now(), seed | 1);
                    let mut buf = [0u8; 4096];
                    while let Ok(n @ 1..) = from.read(&mut buf) {
                        rng ^= rng << 13;
                        rng ^= rng >> 7;
                        rng ^= rng << 17;
                        let extra = jitter.mul_f64((rng % 1000) as f64 / 1000.0);
                        // Never before the bytes ahead of it: TCP keeps order.
                        last = last.max(Instant::now() + one_way + extra);
                        if tx.send((last, buf[..n].to_vec())).is_err() {
                            break;
                        }
                    }
                });
                let mut to = to;
                std::thread::spawn(move || {
                    for (at, bytes) in rx {
                        std::thread::sleep(at.saturating_duration_since(Instant::now()));
                        if to.write_all(&bytes).is_err() {
                            break;
                        }
                    }
                    let _ = to.shutdown(Shutdown::Both);
                });
            }
        });
        addr
    }

    /// What a match over a long link came to: the first steps that had to wait, of all
    /// taken after the first `settle` seconds, and the delay changes each side agreed.
    struct Played {
        stalled: f64,
        schedules: [Vec<(u32, u32)>; 2],
        delays: Vec<u32>,
    }

    /// Two machines over a latency proxy of `rtt` with `jitter` each way, each on its own
    /// thread and clock at 30 fixed steps a second as `SimPlugin` steps them (a second step
    /// only to catch up), for `secs` seconds of wall time, each moving a unit now and then.
    /// Then the one behind catches up to the other, and both must hold the same state.
    fn play_over(rtt: Duration, jitter: Duration, adaptive: bool, secs: u32, settle: u32) -> Played {
        let players: Vec<PlayerSetup> = [Faction::Apa, Faction::Gla]
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
        let host_sim = Sim::new(g2_rules::crater_ridge(), None, 11, players);
        let setup = host_sim.world.replay(&host_sim.setup);
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let hosting = std::thread::spawn(move || host(port, setup, None));
        std::thread::sleep(Duration::from_millis(50));
        let target = SocketAddr::from(([127, 0, 0, 1], port));
        let proxy = latency_proxy(target, rtt / 2, jitter, rtt.as_millis() as u64 + adaptive as u64);
        let (link, hello) = join(&proxy.to_string()).unwrap();
        accept(&link);
        let mut joiner = match_sim(&hello, PlayerId(1)).unwrap();
        joiner.online = Some(Online::new(link, PlayerId(1), PlayerId(0)));
        let mut host_sim = host_sim;
        host_sim.online = Some(Online::new(hosting.join().unwrap().unwrap().0, PlayerId(0), PlayerId(1)));
        let frame = Duration::from_secs(1) / g2_sim::TICKS_PER_SECOND;
        let run = move |mut sim: Sim, phase: Duration| {
            sim.online.as_mut().unwrap().adaptive = adaptive;
            std::thread::sleep(phase);
            let start = Instant::now();
            let (mut waits, mut steps) = (0u32, 0u32);
            let mut delays = Vec::new();
            for n in 0..secs * g2_sim::TICKS_PER_SECOND {
                std::thread::sleep((start + frame * n).saturating_duration_since(Instant::now()));
                if sim.world.tick() % 150 == 60 + 30 * sim.local.0 as u32 {
                    let me = sim.local;
                    let u = sim.world.entities().find(|e| e.owner == Some(me) && sim.world.rules.def(e.def).is_unit());
                    let target = g2_sim::Vec2::from_int(100 + (n % 7) as i64 * 10, 120);
                    sim.order(CommandKind::Move { units: vec![u.unwrap().id], target });
                }
                for first in [true, false] {
                    let go = sim.may_step(first);
                    if first && n >= settle * g2_sim::TICKS_PER_SECOND {
                        steps += 1;
                        waits += !go as u32;
                    }
                    if !go {
                        break;
                    }
                    sim.world.step();
                }
                let online = sim.online.as_ref().unwrap();
                assert_eq!(online.failed, None, "{rtt:?}");
                delays.push(online.lockstep.delay_at(sim.world.tick() / TURN_TICKS));
            }
            (sim, waits, steps, delays)
        };
        let a = std::thread::spawn(move || run(host_sim, Duration::ZERO));
        let b = std::thread::spawn(move || run(joiner, Duration::from_millis(13)));
        let ((mut a, wa, sa, delays), (mut b, wb, sb, _)) = (a.join().unwrap(), b.join().unwrap());
        // The one behind has every turn it needs from the one ahead, or will once the proxy
        // delivers them.
        let started = Instant::now();
        while a.world.tick() != b.world.tick() {
            assert!(started.elapsed() < Duration::from_secs(10), "ticks {} / {}", a.world.tick(), b.world.tick());
            let behind = if a.world.tick() < b.world.tick() { &mut a } else { &mut b };
            if behind.may_step(true) {
                behind.world.step();
            } else {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        assert_eq!(a.world.state_hash(), b.world.state_hash(), "{rtt:?}: the same state on both");
        let (la, lb) = (a.world.replay(&a.setup).commands, b.world.replay(&b.setup).commands);
        assert_eq!(la, lb, "{rtt:?}");
        assert!(la.iter().any(|c| c.player == PlayerId(0)) && la.iter().any(|c| c.player == PlayerId(1)));
        let schedule = |s: &Sim| s.online.as_ref().unwrap().lockstep.schedule().to_vec();
        Played { stalled: (wa + wb) as f64 / (sa + sb) as f64, schedules: [schedule(&a), schedule(&b)], delays }
    }

    /// Over a 150 ms and a 300 ms round trip with jitter, through a proxy delaying the real
    /// sockets, two machines stay in step with the delay adapting (the same state at the end,
    /// every hash check passed, every delay change on the same turn on both), and stall no more
    /// than with the fixed two turns orders waited before. A turn waits for half the round trip,
    /// not all of it, so the fixed delay held up to about 400 ms; at 600 ms it stalls often and
    /// the adapted one hardly at all.
    #[test]
    fn a_long_link_stays_in_step_and_the_delay_adapts() {
        let cases = [(150, 30), (300, 60), (600, 60)];
        let runs: Vec<_> = cases
            .iter()
            .flat_map(|&(rtt, jitter)| [false, true].map(move |adaptive| (rtt, jitter, adaptive)))
            .map(|(rtt, jitter, adaptive)| {
                let (rtt, jitter) = (Duration::from_millis(rtt), Duration::from_millis(jitter));
                std::thread::spawn(move || play_over(rtt, jitter, adaptive, 14, 4))
            })
            .collect();
        let played: Vec<Played> = runs.into_iter().map(|r| r.join().unwrap()).collect();
        for (k, &(rtt, jitter)) in cases.iter().enumerate() {
            let (fixed, adapted) = (&played[2 * k], &played[2 * k + 1]);
            let [sa, sb] = &adapted.schedules;
            let common = sa.len().min(sb.len());
            assert_eq!(sa[..common], sb[..common], "{rtt} ms: delay changes on the same turns");
            assert_eq!(fixed.schedules[0], vec![(0, DELAY_TURNS)], "not adaptive: no change");
            let settled = adapted.delays.last().copied().unwrap();
            println!(
                "{rtt} ms round trip, jitter up to {jitter} ms each way: stalled {:.1}% of steps at 2 turns, \
                 {:.1}% adapting (delay settled at {settled} turns; changes {sa:?})",
                100.0 * fixed.stalled,
                100.0 * adapted.stalled,
            );
            assert!(adapted.stalled < 0.05, "{rtt} ms: stalled {:.3} adapting", adapted.stalled);
            assert!(adapted.stalled <= fixed.stalled, "{rtt} ms");
            assert!(settled <= MAX_DELAY_TURNS);
        }
        assert!(played[4].stalled > 0.2, "600 ms with the fixed delay stalls often: {:.3}", played[4].stalled);
    }
}
