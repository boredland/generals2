//! The network lobby behind the menu's NETWORK pages: finding games on the local network,
//! hosting one, joining one, and the game options both players agree on before the match.
//!
//! Generals' LAN lobby (`LANAPI.cpp` in EA's GPL release) works the same way: the lobby page
//! lists games heard on UDP port 8086, a host waits for players with its game options open,
//! players change their own seat and accept, and any change the host makes clears every accept.
//! Here a lobby `Query` is broadcast and every host answers it, instead of each host
//! announcing itself every 10 seconds, so the list fills as soon as the page opens.
//!
//! Nothing blocks the frame: sockets are non-blocking and polled once a frame, the connect runs
//! on its own thread, and the TCP link is `net::Link`'s, whose threads read and write.

use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use g2_sim::net::{BEACON_PORT, Beacon, GAME_PORT, Message, PROTOCOL, Seat, build_id};
use g2_sim::{Faction, GeneralId, MapDef, PlayerId, PlayerSetup, Replay};

use crate::menu::{Act, Screen, Ui};
use crate::net::{COOP_TEAM, Link, Online};
use crate::profile::Profile;
use crate::sim::Sim;

/// A text field of the lobby pages.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entry {
    Name,
    Port,
    Address,
}

impl Entry {
    /// Whether `c` may be typed here.
    fn takes(self, c: char) -> bool {
        match self {
            Entry::Name => !c.is_control(),
            Entry::Port => c.is_ascii_digit(),
            Entry::Address => c.is_ascii_alphanumeric() || ".:-[]".contains(c),
        }
    }

    /// The longest the text may get.
    fn max(self) -> usize {
        match self {
            Entry::Name => NAME_LENGTH,
            Entry::Port => 5,
            Entry::Address => 64,
        }
    }
}

/// What the lobby pages edit: the fields, the picked game, the side this player plays, and
/// the game a host opens with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Form {
    pub name: String,
    pub port: String,
    pub address: String,
    pub typing: Option<Entry>,
    /// The game picked in the list, by its place in `Discovery::games`.
    pub picked: Option<usize>,
    pub faction: Faction,
    pub general: Option<GeneralId>,
    pub map: String,
    pub coop: bool,
    /// The last thing that went wrong (a refused host, a lost link), shown until the next act.
    pub note: Option<String>,
}

impl Default for Form {
    fn default() -> Self {
        Form {
            name: default_name(),
            port: GAME_PORT.to_string(),
            address: String::new(),
            typing: None,
            picked: None,
            faction: Faction::Eu,
            general: None,
            map: "Crater Ridge".into(),
            coop: false,
            note: None,
        }
    }
}

impl Form {
    pub fn text(&self, e: Entry) -> &str {
        match e {
            Entry::Name => &self.name,
            Entry::Port => &self.port,
            Entry::Address => &self.address,
        }
    }

    fn text_mut(&mut self, e: Entry) -> &mut String {
        match e {
            Entry::Name => &mut self.name,
            Entry::Port => &mut self.port,
            Entry::Address => &mut self.address,
        }
    }

    fn name_or_default(&self) -> String {
        if self.name.trim().is_empty() { default_name() } else { self.name.clone() }
    }
}

/// The next side in a lobby's faction list, which holds each faction and then its generals,
/// as Zero Hour's lists every general as a faction of its own.
pub fn next_side(faction: Faction, general: Option<GeneralId>) -> (Faction, Option<GeneralId>) {
    match crate::menu::next_general(faction, general) {
        Some(g) => (faction, Some(g)),
        None => (Faction::ALL[(faction as usize + 1) % Faction::ALL.len()], None),
    }
}

/// The side as the profile plays it: its perks slotted on the general, at its rank.
fn pick(world: &World, faction: Faction, general: Option<GeneralId>) -> Pick {
    let profile = world.resource::<Profile>();
    Pick { faction, general, perks: profile.slotted(general), rank: general.map_or(1, |g| profile.progress(g).rank) }
}

/// Carries out the lobby pages' acts. `maps`: every map a game may be hosted on.
pub fn apply(world: &mut World, act: Act, maps: &[String]) {
    let mut ui = world.resource_mut::<Ui>();
    ui.lobby.note = None;
    if !matches!(act, Act::Type(_)) {
        ui.lobby.typing = None;
    }
    let form = ui.lobby.clone();
    let mut net = world.remove_resource::<Net>().unwrap_or_default();
    let mut note = None;
    match act {
        Act::OpenLobby | Act::LeaveGame => {
            net = Net::Browsing(Discovery::lan());
            let mut ui = world.resource_mut::<Ui>();
            if act == Act::OpenLobby {
                // The side the skirmish page last had, as the player left it.
                let own = ui.config.slots[0].clone();
                (ui.lobby.faction, ui.lobby.general) = (own.faction, own.general);
                ui.lobby.picked = None;
            }
            ui.screen = Screen::Lobby;
        }
        Act::LeaveLobby => {
            net = Net::Idle;
            world.resource_mut::<Ui>().screen = Screen::Main;
        }
        Act::Type(e) => world.resource_mut::<Ui>().lobby.typing = e,
        Act::PickGame(i) => world.resource_mut::<Ui>().lobby.picked = Some(i),
        Act::HostGame => {
            let port = if form.port.is_empty() { Ok(GAME_PORT) } else { form.port.parse::<u16>() };
            let seed = world.resource::<Ui>().config.seed;
            let offer =
                Offer { map: form.map.clone(), coop: form.coop, seed, own: pick(world, form.faction, form.general) };
            let opened = port
                .map_err(|_| format!("{:?} is no port", form.port))
                .and_then(|p| Host::open(p, Some(BEACON_PORT), form.name_or_default(), offer));
            match opened {
                Ok(h) => {
                    net = Net::Hosting(Box::new(h));
                    world.resource_mut::<Ui>().screen = Screen::NetGame;
                }
                Err(e) => note = Some(e),
            }
        }
        Act::JoinGame | Act::DirectConnect => {
            let addr = match (&net, act) {
                (_, Act::DirectConnect) if form.address.is_empty() => Err("type the host's address first".to_owned()),
                (_, Act::DirectConnect) => Ok(form.address.clone()),
                (Net::Browsing(d), _) => match form.picked.and_then(|i| d.games.get(i)) {
                    Some(g) if !g.compatible => Err("that game runs a different version".to_owned()),
                    Some(g) if !g.open => Err("that game is full".to_owned()),
                    Some(g) => Ok(g.addr.to_string()),
                    None => Err("pick a game in the list first".to_owned()),
                },
                _ => Err("pick a game in the list first".to_owned()),
            };
            match addr {
                Ok(a) => {
                    let own = pick(world, form.faction, form.general);
                    net = Net::Joined(Box::new(Guest::connect(&a, form.name_or_default(), own)));
                    world.resource_mut::<Ui>().screen = Screen::NetGame;
                }
                Err(e) => note = Some(e),
            }
        }
        Act::NetMode | Act::NetMap(_) | Act::NetSide | Act::NetAccept | Act::NetStart => {
            let (faction, general) = next_side(form.faction, form.general);
            let side = pick(world, faction, general);
            let done = match (&mut net, act) {
                (Net::Hosting(h), Act::NetMode) => h.change(|o| o.coop = !o.coop),
                (Net::Hosting(h), Act::NetMap(d)) => {
                    let at = maps.iter().position(|m| *m == h.offer().map).unwrap_or(0) as i32;
                    match maps.get((at + d).rem_euclid(maps.len().max(1) as i32) as usize) {
                        Some(m) => h.change(|o| o.map = m.clone()),
                        None => Ok(()),
                    }
                }
                (Net::Hosting(h), Act::NetSide) => h.change(|o| o.own = side),
                (Net::Hosting(h), Act::NetStart) => h.start(),
                (Net::Joined(g), Act::NetSide) => {
                    g.pick(side);
                    Ok(())
                }
                (Net::Joined(g), Act::NetAccept) => {
                    g.toggle_ready();
                    Ok(())
                }
                _ => Ok(()),
            };
            let mut ui = world.resource_mut::<Ui>();
            if act == Act::NetSide {
                (ui.lobby.faction, ui.lobby.general) = (faction, general);
            }
            if let Net::Hosting(h) = &net {
                (ui.lobby.map, ui.lobby.coop) = (h.offer().map.clone(), h.offer().coop);
            }
            note = done.err();
        }
        _ => unreachable!("not a lobby act: {act:?}"),
    }
    world.resource_mut::<Ui>().lobby.note = note;
    world.insert_resource(net);
}

/// Polls the lobby once a frame: new games, the other player's seat, the match starting (it
/// replaces the world, as the skirmish page's START does) or the session ending.
pub fn poll(world: &mut World) {
    let Some(mut net) = world.get_resource_mut::<Net>() else { return };
    match net.poll() {
        Polled::Quiet => {}
        Polled::Changed => world.resource_mut::<Ui>().set_changed(),
        Polled::Started(sim) => {
            let (map, seed) = (sim.setup.map.name.clone(), sim.setup.seed);
            world.insert_resource(Net::Idle);
            crate::menu::swap_world(world, *sim);
            let mut ui = world.resource_mut::<Ui>();
            ui.config.map = map;
            ui.config.seed = seed;
            ui.earned = None;
            ui.screen = Screen::Match;
            ui.paused = false;
        }
        // Back to the game list, saying why, as Generals' lobby does when a host leaves.
        Polled::Failed(why) => {
            world.insert_resource(Net::Browsing(Discovery::lan()));
            let mut ui = world.resource_mut::<Ui>();
            ui.lobby.note = Some(why);
            ui.screen = Screen::Lobby;
        }
    }
}

/// Typing into the lobby field being edited: characters it takes, Backspace, and Enter or Esc
/// to finish (a click elsewhere finishes too, in `apply`).
pub fn type_text(mut keys: MessageReader<KeyboardInput>, mut ui: ResMut<Ui>) {
    let Some(entry) = ui.lobby.typing else {
        keys.clear();
        return;
    };
    for k in keys.read().filter(|k| k.state == ButtonState::Pressed) {
        let text = ui.lobby.text_mut(entry);
        match &k.logical_key {
            Key::Backspace => {
                text.pop();
            }
            Key::Enter | Key::Escape => ui.lobby.typing = None,
            Key::Character(s) => {
                for c in s.chars().filter(|&c| entry.takes(c)) {
                    if text.chars().count() < entry.max() {
                        text.push(c);
                    }
                }
            }
            Key::Space if entry == Entry::Name && text.chars().count() < entry.max() => text.push(' '),
            _ => {}
        }
    }
}

/// One seat of the game options as drawn: who, which side, whether accepted.
pub struct Row {
    pub name: String,
    pub faction: Faction,
    pub general: Option<GeneralId>,
    /// The host's seat counts as accepted, as in Generals.
    pub ready: bool,
    /// The local player's own seat, which they can change.
    pub own: bool,
    /// The waves, in co-op.
    pub waves: bool,
}

impl Net {
    /// The map of the game hosted or joined, once known.
    pub fn offered_map(&self) -> Option<String> {
        match self {
            Net::Hosting(h) => Some(h.offer.map.clone()),
            Net::Joined(g) => g.offered.as_ref().map(|o| o.setup.map.clone()),
            _ => None,
        }
    }

    /// The game as it stands, for the game options page: its setup (`None` until a joiner has
    /// heard from the host), its seats and whether this machine hosts it.
    pub fn game(&self) -> Option<(Replay, Vec<Row>, bool)> {
        let (setup, names, ready, host) = match self {
            Net::Hosting(h) => {
                let guest = h.guest().map(|s| s.name.clone());
                let joiner = match (&guest, h.has_guest()) {
                    (Some(n), _) => n.clone(),
                    (None, true) => "Joining...".into(),
                    (None, false) => "Open".into(),
                };
                (h.setup(), [h.name.clone(), joiner], h.guest_ready(), true)
            }
            Net::Joined(g) => {
                let o = g.offered.as_ref()?;
                (o.setup.clone(), [o.host.clone(), g.seat.name.clone()], g.ready(), false)
            }
            _ => return None,
        };
        let rows = setup
            .players
            .iter()
            .enumerate()
            .map(|(i, p)| Row {
                name: names.get(i).cloned().unwrap_or_else(|| "Onslaught".into()),
                faction: p.faction,
                general: p.general,
                ready: i == 0 || (i == 1 && ready),
                own: i == usize::from(!host),
                waves: i == 2,
            })
            .collect();
        Some((setup, rows, host))
    }

    /// The page's system lines: who came and went, while hosting; the connection, joining.
    pub fn log(&self) -> Vec<String> {
        match self {
            Net::Hosting(h) => h.log.clone(),
            Net::Joined(g) => match &g.offered {
                None if g.link.is_none() => vec![format!("Connecting to {}...", g.addr)],
                None => vec![format!("Connected to {}, waiting for the game...", g.addr)],
                Some(o) => vec![format!("Joined {}'s game at {}.", o.host, g.addr)],
            },
            _ => Vec::new(),
        }
    }
}

/// Generals' `g_lanPlayerNameLength`: 12 characters.
pub const NAME_LENGTH: usize = 12;
/// A lobby asks for games this often...
const ASK_EVERY: Duration = Duration::from_secs(2);
/// ...and forgets one unheard for three questions. Generals forgets a game after twice its
/// 10-second announcement interval; asking is quicker, so the list follows sooner.
const FORGET_AFTER: Duration = Duration::from_secs(6);
/// How long a join may take to reach the host.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// The player name to start with: the account's, as Generals takes the login name.
pub fn default_name() -> String {
    let name = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_else(|_| "Player".into());
    name.chars().take(NAME_LENGTH).collect()
}

/// What the lobby is doing: nothing (another page), looking for games, hosting one, or seated
/// in someone else's.
#[derive(Resource, Default)]
pub enum Net {
    #[default]
    Idle,
    Browsing(Discovery),
    Hosting(Box<Host>),
    Joined(Box<Guest>),
}

/// What a poll brought: nothing to redraw, a change to show, the match starting, or the
/// session ended with a reason.
pub enum Polled {
    Quiet,
    Changed,
    Started(Box<Sim>),
    Failed(String),
}

impl Net {
    pub fn poll(&mut self) -> Polled {
        match self {
            Net::Idle => Polled::Quiet,
            Net::Browsing(d) => {
                if d.poll() {
                    Polled::Changed
                } else {
                    Polled::Quiet
                }
            }
            Net::Hosting(h) => h.poll(),
            Net::Joined(g) => g.poll(),
        }
    }
}

/// A game heard on the local network.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    /// Where to connect: the answer's sender, at the game's port.
    pub addr: SocketAddr,
    pub name: String,
    pub map: String,
    pub coop: bool,
    /// Its joiner seat is free.
    pub open: bool,
    /// Same protocol and build as this one: only these can be joined.
    pub compatible: bool,
    heard: Instant,
}

/// The lobby page's game list: asks every `ASK_EVERY` and keeps who answered.
pub struct Discovery {
    socket: Option<UdpSocket>,
    /// Where questions go: the local network's broadcast address and this machine, on the
    /// beacon port (tests ask one host's own port).
    targets: Vec<SocketAddr>,
    asked: Option<Instant>,
    pub games: Vec<Found>,
}

impl Discovery {
    /// Asks the local network on `BEACON_PORT`, and this machine (a broadcast doesn't come back
    /// to the sender's own host on every system).
    pub fn lan() -> Discovery {
        Discovery::asking(vec![
            SocketAddr::from(([255, 255, 255, 255], BEACON_PORT)),
            SocketAddr::from(([127, 0, 0, 1], BEACON_PORT)),
        ])
    }

    pub fn asking(targets: Vec<SocketAddr>) -> Discovery {
        let socket = UdpSocket::bind(("0.0.0.0", 0)).ok().filter(|s| {
            // Without broadcast (a sandbox) only this machine's games show; joining by
            // address still works.
            let _ = s.set_broadcast(true);
            s.set_nonblocking(true).is_ok()
        });
        Discovery { socket, targets, asked: None, games: Vec::new() }
    }

    /// Asks again when due, takes in answers and forgets the silent; whether the list changed.
    pub fn poll(&mut self) -> bool {
        let Some(socket) = &self.socket else { return false };
        let now = Instant::now();
        if self.asked.is_none_or(|t| now - t >= ASK_EVERY) {
            let query = Beacon::Query { protocol: PROTOCOL }.encode();
            for t in &self.targets {
                let _ = socket.send_to(&query, t);
            }
            self.asked = Some(now);
        }
        let mut changed = false;
        let mut buf = [0u8; 1500];
        while let Ok((n, from)) = socket.recv_from(&mut buf) {
            let Some(Beacon::Game { protocol, build, port, name, map, coop, open }) = Beacon::decode(&buf[..n]) else {
                continue;
            };
            let addr = SocketAddr::new(from.ip(), port);
            let compatible = protocol == PROTOCOL && build == build_id(g2_rules::rules());
            let found = Found { addr, name, map, coop, open, compatible, heard: now };
            match self.games.iter_mut().find(|g| g.addr == addr) {
                Some(g) => {
                    changed |= *g != Found { heard: g.heard, ..found.clone() };
                    *g = found;
                }
                None => {
                    self.games.push(found);
                    changed = true;
                }
            }
        }
        let before = self.games.len();
        self.games.retain(|g| now - g.heard < FORGET_AFTER);
        changed || self.games.len() != before
    }
}

/// What a player picks for their own seat: a faction and, optionally, one of its generals, as
/// Zero Hour's faction list holds each general; perks and rank come from their profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pick {
    pub faction: Faction,
    pub general: Option<GeneralId>,
    pub perks: Vec<g2_sim::defs::PerkId>,
    pub rank: u8,
}

impl Pick {
    pub fn plain(faction: Faction) -> Pick {
        Pick { faction, general: None, perks: Vec::new(), rank: 1 }
    }

    fn player(&self, team: u8) -> PlayerSetup {
        PlayerSetup {
            faction: self.faction,
            starting_credits: 5_000,
            ai: None,
            general: self.general,
            perks: self.perks.clone(),
            rank: self.rank,
            team,
        }
    }
}

/// The game a host offers: map, mode and the host's own seat. Its changes clear the joiner's
/// accept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub map: String,
    pub coop: bool,
    pub seed: u64,
    pub own: Pick,
}

/// The seat a joiner sits in until they pick: the first faction the host doesn't play.
fn joiner_default(host: Faction) -> Pick {
    Pick::plain(Faction::ALL.into_iter().find(|&f| f != host).unwrap_or(Faction::Gla))
}

/// The other player, seated in a hosted game.
struct Seated {
    link: Link,
    seat: Option<Seat>,
}

/// A hosted game, open for one joiner.
pub struct Host {
    listener: TcpListener,
    pub port: u16,
    /// Answers lobby questions; `None` when the beacon port is taken (another host on this
    /// machine): the game is then joined by address.
    beacon: Option<UdpSocket>,
    pub name: String,
    offer: Offer,
    map: MapDef,
    /// Bumped on every change to `offer`.
    revision: u32,
    guest: Option<Seated>,
    /// `Hello` sent; waiting for the joiner's `Join`.
    starting: bool,
    /// System lines for the page, newest last, as Generals' chat window shows who came and went.
    pub log: Vec<String>,
}

impl Host {
    /// Listens on `port` (0: any free one) and answers lobby questions on `beacon`.
    pub fn open(port: u16, beacon: Option<u16>, name: String, offer: Offer) -> Result<Host, String> {
        let listener = TcpListener::bind(("0.0.0.0", port)).map_err(|e| format!("can't host on port {port}: {e}"))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let listed = beacon.and_then(|b| UdpSocket::bind(("0.0.0.0", b)).ok());
        let unlisted = beacon.is_some() && listed.is_none();
        let beacon = listed;
        if let Some(b) = &beacon {
            b.set_nonblocking(true).map_err(|e| e.to_string())?;
        }
        let (map, _) = crate::net::resolve_map(&offer.map)?;
        let mut log = vec![format!("Hosting on port {port}.")];
        if unlisted {
            log.push("Not listed on the local network (its port is taken): join by address.".into());
        }
        Ok(Host { listener, port, beacon, name, offer, map, revision: 0, guest: None, starting: false, log })
    }

    /// The beacon's port, if listed (tests listen on any free one).
    #[cfg(test)]
    pub fn beacon_port(&self) -> Option<u16> {
        self.beacon.as_ref().and_then(|b| b.local_addr().ok()).map(|a| a.port())
    }

    pub fn offer(&self) -> &Offer {
        &self.offer
    }

    /// Changes the game: the joiner's accept no longer holds, and they see the new offer.
    pub fn change(&mut self, change: impl FnOnce(&mut Offer)) -> Result<(), String> {
        let mut next = self.offer.clone();
        change(&mut next);
        if next == self.offer || self.starting {
            return Ok(());
        }
        if next.map != self.offer.map {
            let (map, _) = crate::net::resolve_map(&next.map)?;
            if map.spawns.len() < 2 {
                return Err(format!("{} has {} starts", next.map, map.spawns.len()));
            }
            self.map = map;
        }
        self.offer = next;
        self.revision += 1;
        self.send_offer();
        Ok(())
    }

    /// The joiner's name and seat, once they sent one.
    pub fn guest(&self) -> Option<&Seat> {
        self.guest.as_ref().and_then(|g| g.seat.as_ref())
    }

    pub fn has_guest(&self) -> bool {
        self.guest.is_some()
    }

    /// Whether the joiner accepted the game as it stands now.
    pub fn guest_ready(&self) -> bool {
        self.guest().is_some_and(|s| s.ready == Some(self.revision))
    }

    /// The match as offered: host, joiner (as seated, else a default seat) and in co-op the
    /// waves; a replay with no commands, as `Hello` carries it.
    pub fn setup(&self) -> Replay {
        let team = if self.offer.coop { COOP_TEAM } else { 0 };
        let joiner = match self.guest() {
            Some(s) => Pick { faction: s.faction, general: s.general, perks: s.perks.clone(), rank: s.rank },
            None => joiner_default(self.offer.own.faction),
        };
        let mut players = vec![self.offer.own.player(team), joiner.player(team)];
        if self.offer.coop {
            players.push(crate::net::waves(crate::net::wave_faction([players[0].faction, players[1].faction])));
        }
        Replay {
            map: self.offer.map.clone(),
            map_fingerprint: Some(self.map.fingerprint()),
            seed: self.offer.seed,
            players,
            length_ticks: 0,
            commands: Vec::new(),
            hashes: Vec::new(),
        }
    }

    /// Starts the match, once the joiner accepted it: they get `Hello`, and the match begins
    /// here when their `Join` arrives (`poll`).
    pub fn start(&mut self) -> Result<(), String> {
        if !self.guest_ready() {
            return Err("the other player hasn't accepted the game yet".into());
        }
        let setup = self.setup();
        crate::net::check_shape(&setup)?;
        let guest = self.guest.as_ref().expect("a ready guest");
        guest.link.send(&Message::Hello { protocol: PROTOCOL, build: build_id(g2_rules::rules()), setup });
        self.starting = true;
        Ok(())
    }

    fn send_offer(&self) {
        if let Some(g) = &self.guest {
            g.link.send(&Message::Lobby {
                protocol: PROTOCOL,
                build: build_id(g2_rules::rules()),
                setup: self.setup(),
                revision: self.revision,
                name: self.name.clone(),
            });
        }
    }

    fn poll(&mut self) -> Polled {
        let mut changed = false;
        if let Some(b) = &self.beacon {
            let mut buf = [0u8; 1500];
            while let Ok((n, from)) = b.recv_from(&mut buf) {
                if let Some(Beacon::Query { .. }) = Beacon::decode(&buf[..n]) {
                    let game = Beacon::Game {
                        protocol: PROTOCOL,
                        build: build_id(g2_rules::rules()),
                        port: self.port,
                        name: self.name.clone(),
                        map: self.offer.map.clone(),
                        coop: self.offer.coop,
                        open: self.guest.is_none(),
                    };
                    let _ = b.send_to(&game.encode(), from);
                }
            }
        }
        while let Ok((stream, peer)) = self.listener.accept() {
            // One joiner: a second connection is closed at once, which its lobby reports.
            if self.guest.is_some() || stream.set_nonblocking(false).is_err() {
                continue;
            }
            match Link::new(stream) {
                Ok(link) => {
                    self.guest = Some(Seated { link, seat: None });
                    self.log.push(format!("{peer} connected."));
                    self.send_offer();
                    changed = true;
                }
                Err(e) => self.log.push(format!("{peer} could not connect: {e}")),
            }
        }
        while let Some(g) = &mut self.guest {
            match g.link.try_recv() {
                Ok(None) => break,
                Ok(Some(Message::Seat(mut s))) => {
                    s.name = s.name.chars().take(NAME_LENGTH).collect();
                    let before = g.seat.clone();
                    g.seat = Some(s.clone());
                    let (setup, map) = (self.setup(), &self.map);
                    if let Err(e) = setup.check(map, g2_rules::rules()) {
                        // A seat this build can't play (a mod's general): the old one stays.
                        let g = self.guest.as_mut().unwrap();
                        g.seat = before;
                        self.log.push(format!("{} picked a seat this game can't use: {e}", s.name));
                    } else if before.as_ref().is_none_or(|b| b.name != s.name) {
                        self.log.push(format!("{} joined.", s.name));
                    }
                    changed = true;
                }
                Ok(Some(Message::Join { protocol, build })) if self.starting => {
                    if protocol != PROTOCOL || build != build_id(g2_rules::rules()) {
                        return self.lose("the other player runs a different game version".into());
                    }
                    let setup = self.setup();
                    let g = self.guest.take().unwrap();
                    return match crate::net::match_sim(&setup, PlayerId(0)) {
                        Ok(mut sim) => {
                            sim.online = Some(Online::new(g.link, PlayerId(0), PlayerId(1)));
                            Polled::Started(Box::new(sim))
                        }
                        Err(e) => Polled::Failed(e),
                    };
                }
                Ok(Some(m)) => return self.lose(format!("the other player sent {m:?} in the lobby")),
                Err(e) => return self.lose(e),
            }
        }
        if changed { Polled::Changed } else { Polled::Quiet }
    }

    /// The joiner is gone; the game stays open for the next one.
    fn lose(&mut self, why: String) -> Polled {
        self.guest = None;
        self.starting = false;
        self.log.push(format!("{why}."));
        Polled::Changed
    }
}

/// The offer as a joiner last received it.
#[derive(Clone, Debug)]
pub struct Offered {
    pub setup: Replay,
    pub revision: u32,
    pub host: String,
}

/// Seated in someone else's game.
pub struct Guest {
    pub addr: String,
    /// The connect thread's result; in a `Mutex` only so the lobby is a `Resource` (`Sync`).
    connecting: Option<std::sync::Mutex<Receiver<Result<TcpStream, String>>>>,
    link: Option<Link>,
    pub offered: Option<Offered>,
    pub seat: Seat,
}

impl Guest {
    /// Connects to `addr` (`host:port`; the port defaults to Generals' 8088) on a thread.
    pub fn connect(addr: &str, name: String, pick: Pick) -> Guest {
        let addr = if addr.contains(':') { addr.to_owned() } else { format!("{addr}:{}", g2_sim::net::GAME_PORT) };
        let (tx, rx) = channel();
        let target = addr.clone();
        std::thread::spawn(move || {
            let r = target
                .to_socket_addrs()
                .map_err(|e| e.to_string())
                .and_then(|mut a| a.next().ok_or_else(|| "no such address".to_owned()))
                .and_then(|a| TcpStream::connect_timeout(&a, CONNECT_TIMEOUT).map_err(|e| e.to_string()))
                .map_err(|e| format!("can't reach {target}: {e}"));
            let _ = tx.send(r);
        });
        let seat = Seat {
            name,
            faction: pick.faction,
            general: pick.general,
            perks: pick.perks,
            rank: pick.rank,
            ready: None,
        };
        Guest { addr, connecting: Some(std::sync::Mutex::new(rx)), link: None, offered: None, seat }
    }

    /// Whether the offer as it stands is accepted.
    pub fn ready(&self) -> bool {
        self.offered.as_ref().is_some_and(|o| self.seat.ready == Some(o.revision))
    }

    /// Accepts the offer as it stands, or takes the accept back.
    pub fn toggle_ready(&mut self) {
        self.seat.ready = match (&self.offered, self.ready()) {
            (Some(o), false) => Some(o.revision),
            _ => None,
        };
        self.send_seat();
    }

    /// A new seat: the accept goes with it, as in Generals.
    pub fn pick(&mut self, pick: Pick) {
        self.seat = Seat {
            faction: pick.faction,
            general: pick.general,
            perks: pick.perks,
            rank: pick.rank,
            ready: None,
            ..self.seat.clone()
        };
        self.send_seat();
    }

    fn send_seat(&self) {
        if let Some(l) = &self.link {
            l.send(&Message::Seat(self.seat.clone()));
        }
    }

    fn poll(&mut self) -> Polled {
        if let Some(rx) = &mut self.connecting {
            let polled = rx.get_mut().unwrap().try_recv();
            match polled {
                Err(TryRecvError::Empty) => return Polled::Quiet,
                Err(TryRecvError::Disconnected) => return Polled::Failed(format!("can't reach {}", self.addr)),
                Ok(Err(e)) => return Polled::Failed(e),
                Ok(Ok(stream)) => {
                    self.connecting = None;
                    match Link::new(stream) {
                        Ok(link) => self.link = Some(link),
                        Err(e) => return Polled::Failed(e.to_string()),
                    }
                    self.send_seat();
                }
            }
        }
        let Some(link) = &self.link else { return Polled::Quiet };
        let mut changed = false;
        loop {
            match link.try_recv() {
                Ok(None) => break,
                Ok(Some(Message::Lobby { protocol, build, setup, revision, name })) => {
                    if protocol != PROTOCOL || build != build_id(g2_rules::rules()) {
                        return Polled::Failed("the host runs a different game version".into());
                    }
                    if let Err(e) = crate::net::check_shape(&setup) {
                        return Polled::Failed(e);
                    }
                    self.offered = Some(Offered { setup, revision, host: name.chars().take(NAME_LENGTH).collect() });
                    changed = true;
                }
                Ok(Some(Message::Hello { protocol, build, setup })) => {
                    if protocol != PROTOCOL || build != build_id(g2_rules::rules()) {
                        return Polled::Failed("the host runs a different game version".into());
                    }
                    let link = self.link.take().unwrap();
                    return match crate::net::match_sim(&setup, PlayerId(1)) {
                        Ok(mut sim) => {
                            crate::net::accept(&link);
                            sim.online = Some(Online::new(link, PlayerId(1), PlayerId(0)));
                            Polled::Started(Box::new(sim))
                        }
                        // Closing the link tells the host this machine can't play it.
                        Err(e) => Polled::Failed(e),
                    };
                }
                Ok(Some(m)) => return Polled::Failed(format!("the host sent {m:?} in the lobby")),
                Err(e) => return Polled::Failed(e.replace("the other player", "the host")),
            }
        }
        if changed { Polled::Changed } else { Polled::Quiet }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(coop: bool) -> Offer {
        Offer { map: "Crater Ridge".into(), coop, seed: 4, own: Pick::plain(Faction::Eu) }
    }

    /// Polls both until `done` holds, as two menus would frame by frame.
    fn until(host: &mut Host, guest: &mut Guest, what: &str, mut done: impl FnMut(&Host, &Guest) -> bool) {
        let started = Instant::now();
        while !done(host, guest) {
            assert!(started.elapsed() < Duration::from_secs(10), "never {what}");
            for p in [host.poll(), guest.poll()] {
                match p {
                    Polled::Quiet | Polled::Changed => {}
                    Polled::Started(_) => panic!("started while waiting for {what}"),
                    Polled::Failed(e) => panic!("while waiting for {what}: {e}"),
                }
            }
            // Spinning without a pause starved the socket threads on the macOS CI runner.
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// A lobby's question finds a host on the beacon port it asks, with the game's map, mode
    /// and whether its seat is free.
    #[test]
    fn a_lobby_finds_a_hosted_game() {
        let host = Host::open(0, Some(0), "Hoster".into(), offer(true)).unwrap();
        let beacon = host.beacon_port().unwrap();
        let mut net = Net::Hosting(Box::new(host));
        let mut d = Discovery::asking(vec![SocketAddr::from(([127, 0, 0, 1], beacon))]);
        let started = Instant::now();
        while d.games.is_empty() {
            assert!(started.elapsed() < Duration::from_secs(10), "no game found");
            net.poll();
            d.poll();
            std::thread::sleep(Duration::from_millis(1));
        }
        let Net::Hosting(h) = &net else { unreachable!() };
        let g = &d.games[0];
        assert_eq!(
            (g.name.as_str(), g.map.as_str(), g.coop, g.open, g.compatible),
            ("Hoster", "Crater Ridge", true, true, true)
        );
        assert_eq!(g.addr.port(), h.port);
    }

    /// The host's change clears the joiner's accept; the joiner's pick reaches the host's
    /// match; only an accepted game starts, and then both sides hold the same co-op match.
    #[test]
    fn accepting_and_starting_a_coop_game() {
        let mut host = Host::open(0, None, "Hoster".into(), offer(false)).unwrap();
        let mut guest = Guest::connect(&format!("127.0.0.1:{}", host.port), "Joiner".into(), Pick::plain(Faction::Apa));
        until(&mut host, &mut guest, "seated", |h, g| h.guest().is_some() && g.offered.is_some());
        assert_eq!(host.setup().players[1].faction, Faction::Apa);
        assert!(host.start().is_err(), "not accepted yet");
        guest.toggle_ready();
        until(&mut host, &mut guest, "accepted", |h, _| h.guest_ready());
        host.change(|o| o.coop = true).unwrap();
        until(&mut host, &mut guest, "the new offer", |_, g| g.offered.as_ref().is_some_and(|o| o.revision == 1));
        assert!(!host.guest_ready() && !guest.ready(), "the change cleared the accept");
        guest.toggle_ready();
        until(&mut host, &mut guest, "accepted again", |h, _| h.guest_ready());
        host.start().unwrap();
        let (mut a, mut b) = (None, None);
        let started = Instant::now();
        while a.is_none() || b.is_none() {
            assert!(started.elapsed() < Duration::from_secs(10), "never started");
            if let Polled::Started(s) = guest.poll() {
                b = Some(s);
            }
            if let Polled::Started(s) = host.poll() {
                a = Some(s);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let (a, b) = (a.unwrap(), b.unwrap());
        assert_eq!(a.setup.players, b.setup.players);
        assert_eq!(a.setup.players.len(), 3);
        assert_eq!((a.local, b.local), (PlayerId(0), PlayerId(1)));
        assert!(a.world.allied(PlayerId(0), PlayerId(1)) && !a.world.allied(PlayerId(0), PlayerId(2)));
        assert_eq!(a.world.state_hash(), b.world.state_hash());
    }
}
