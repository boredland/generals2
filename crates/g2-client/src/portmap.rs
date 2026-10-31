//! Asking the player's own router to forward the game port, so a game hosted behind it can be
//! joined from the internet without the player opening the port by hand.
//!
//! Three protocols, tried in order: UPnP IGD (most home routers), then PCP and its predecessor
//! NAT-PMP (Apple, OpenWrt and others). Each talks only to the router on the local network:
//! UPnP finds it by an SSDP multicast on the LAN, PCP and NAT-PMP send to the default gateway.
//! The router also tells its own public address, so the game never asks a website for it.
//!
//! Everything runs on a thread and gives up after about two seconds: a router that doesn't
//! answer, or says no, leaves the game hosted as before, reachable on the LAN or through a
//! forward the player sets up. The forward is removed when the `PortMap` is dropped (the game
//! closed, the match over, the app quit) and lapses on its own after `LEASE` otherwise.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError, channel};
use std::time::{Duration, Instant};

/// How long one step (a discovery, an HTTP request, a datagram's answers) may take. Routers
/// answer on the LAN in milliseconds; a step that takes longer finds no router.
const STEP: Duration = Duration::from_millis(1000);
/// How long the router keeps the forward if this app never removes it (a crash).
const LEASE: u32 = 3600;
/// What the router's forwarding table shows the forward as.
const DESCRIPTION: &str = "Open RTS";
const SSDP: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(239, 255, 255, 250)), 1900);
const PMP_PORT: u16 = 5351;
/// The service types whose `AddPortMapping` forwards a port, IGD v1 and v2 alike.
const WAN_SERVICES: [&str; 3] = [
    "urn:schemas-upnp-org:service:WANIPConnection:2",
    "urn:schemas-upnp-org:service:WANIPConnection:1",
    "urn:schemas-upnp-org:service:WANPPPConnection:1",
];

/// Where to look for the router: the real LAN, or a test's fake one.
#[derive(Clone, Debug)]
pub struct Router {
    /// Where the UPnP search goes: SSDP's multicast group.
    pub ssdp: SocketAddr,
    /// Where PCP and NAT-PMP requests go; `None`: the default gateway.
    pub pmp: Option<SocketAddr>,
}

impl Router {
    /// The real LAN's router; none in tests, which must never ask the router of the machine
    /// they run on (they use fakes on loopback).
    pub fn lan() -> Option<Router> {
        (!cfg!(test)).then_some(Router { ssdp: SSDP, pmp: None })
    }
}

/// What came of asking the router.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Forward {
    Asking,
    /// The router forwards TCP `port` on its public address (when it told it) to this machine.
    Open {
        via: &'static str,
        external: Option<IpAddr>,
        port: u16,
    },
    /// No forward: why not, in a few words.
    Closed(String),
}

/// A forward held open on the router; removing it needs to know how it was made.
#[derive(Clone, Debug)]
enum Lease {
    Upnp { control: Url, service: &'static str, port: u16 },
    Pcp { gateway: SocketAddr, client: Ipv4Addr, nonce: [u8; 12], internal: u16 },
    Pmp { gateway: SocketAddr, internal: u16 },
}

/// What the asking thread finds: the forward, and how to remove it when there is one.
type Answer = (Forward, Option<Lease>);

/// A forward of one TCP port, asked for on a thread; removed on drop.
pub struct PortMap {
    pub port: u16,
    /// The asking thread's answer; in a `Mutex` only so a holder can be a `Resource` (`Sync`).
    answer: Option<Mutex<Receiver<Answer>>>,
    state: Forward,
    lease: Option<Lease>,
}

impl PortMap {
    /// Asks `router` to forward TCP `port` to this machine.
    pub fn request(port: u16, router: Router) -> PortMap {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let (forward, lease) = match open(port, &router) {
                Ok((forward, lease)) => (forward, Some(lease)),
                Err(why) => (Forward::Closed(why), None),
            };
            match &forward {
                Forward::Open { via, external, port } => {
                    let at = external.map_or("its public address".to_owned(), |ip| ip.to_string());
                    eprintln!("the router forwards TCP port {port} on {at} to this machine ({via})");
                }
                Forward::Closed(why) => eprintln!("no port forward from the router: {why}"),
                Forward::Asking => {}
            }
            let _ = tx.send((forward, lease));
        });
        PortMap { port, answer: Some(Mutex::new(rx)), state: Forward::Asking, lease: None }
    }

    pub fn state(&self) -> &Forward {
        &self.state
    }

    /// Takes in the thread's answer once it is there; whether the state changed.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &mut self.answer else { return false };
        let got = rx.get_mut().unwrap().try_recv();
        match got {
            Ok(answer) => self.take(answer),
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => self.take((Forward::Closed("the request failed".into()), None)),
        }
        true
    }

    /// Waits up to `d` for the answer (a headless render, which has no frames to poll in).
    pub fn wait(&mut self, d: Duration) {
        let Some(rx) = &mut self.answer else { return };
        let got = rx.get_mut().unwrap().recv_timeout(d);
        match got {
            Ok(answer) => self.take(answer),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => self.take((Forward::Closed("the request failed".into()), None)),
        }
    }

    fn take(&mut self, (state, lease): Answer) {
        (self.state, self.lease, self.answer) = (state, lease, None);
    }

    /// Removes the forward on a thread, for callers that can't wait for the router (a frame).
    pub fn release(self) {
        std::thread::spawn(move || drop(self));
    }
}

impl Drop for PortMap {
    /// Removes the forward. One still being asked for is waited for first, or it would stay
    /// open on the router until its lease ran out.
    fn drop(&mut self) {
        self.wait(3 * STEP);
        if let Some(lease) = self.lease.take() {
            match close(&lease) {
                Ok(()) => eprintln!("the router's forward of port {} is removed", self.port),
                Err(e) => eprintln!("could not remove the router's forward of port {}: {e}", self.port),
            }
        }
    }
}

fn open(port: u16, router: &Router) -> Result<(Forward, Lease), String> {
    let upnp = match upnp_open(port, router.ssdp) {
        Ok(found) => return Ok(found),
        Err(e) => e,
    };
    let Some(gateway) = router.pmp.or_else(|| default_gateway().map(|g| SocketAddr::new(g.into(), PMP_PORT))) else {
        return Err(format!("{upnp}, and no default gateway for NAT-PMP"));
    };
    match pcp_open(port, gateway) {
        Ok(found) => Ok(found),
        // A NAT-PMP router answers PCP's version 2 with "unsupported version"; one that answers
        // neither has none of the three.
        Err(Pcp::OldVersion) | Err(Pcp::Silent) => {
            pmp_open(port, gateway).map_err(|pmp| format!("{upnp}; NAT-PMP: {pmp}"))
        }
        Err(Pcp::Refused(why)) => Err(format!("{upnp}; PCP: {why}")),
    }
}

fn close(lease: &Lease) -> Result<(), String> {
    match lease {
        Lease::Upnp { control, service, port } => {
            let args = [
                ("NewRemoteHost", String::new()),
                ("NewExternalPort", port.to_string()),
                ("NewProtocol", "TCP".into()),
            ];
            soap(control, service, "DeletePortMapping", &args).map(|_| ()).map_err(|e| e.to_string())
        }
        Lease::Pcp { gateway, client, nonce, internal } => {
            pcp_exchange(*gateway, &pcp_map(*client, *nonce, *internal, 0, 0)).map(|_| ()).map_err(|e| format!("{e:?}"))
        }
        Lease::Pmp { gateway, internal } => pmp_exchange(*gateway, &pmp_map(*internal, 0, 0), 16).map(|_| ()),
    }
}

// ---- UPnP IGD ------------------------------------------------------------------------------

/// An `http://host:port/path` URL, as routers give them: always an IP address, never TLS.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Url {
    host: SocketAddr,
    path: String,
}

impl Url {
    fn parse(s: &str) -> Option<Url> {
        let rest = s.trim().strip_prefix("http://")?;
        let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
        let host = if authority.rsplit_once(':').is_some_and(|(h, p)| !h.ends_with(':') && p.parse::<u16>().is_ok()) {
            authority.to_socket_addrs().ok()?.next()?
        } else {
            (authority.trim_matches(['[', ']']), 80).to_socket_addrs().ok()?.next()?
        };
        Some(Url { host, path: if path.is_empty() { "/".into() } else { path.into() } })
    }

    /// `link` read relative to this URL, as a device description's `controlURL` is.
    fn join(&self, link: &str) -> Option<Url> {
        if link.starts_with("http://") {
            return Url::parse(link);
        }
        let path = if link.starts_with('/') { link.to_owned() } else { format!("/{link}") };
        Some(Url { host: self.host, path })
    }
}

#[derive(Debug)]
enum SoapError {
    Http(String),
    /// The router's UPnP error code and text (725: only permanent leases; 718: the port is
    /// forwarded to another machine already).
    Upnp(u32, String),
}

impl std::fmt::Display for SoapError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            SoapError::Http(e) => f.write_str(e),
            SoapError::Upnp(718, _) => f.write_str("the router forwards that port to another machine already"),
            SoapError::Upnp(code, text) => write!(f, "the router refused ({code} {text})"),
        }
    }
}

fn upnp_open(port: u16, ssdp: SocketAddr) -> Result<(Forward, Lease), String> {
    let location = ssdp_search(ssdp).ok_or("no UPnP router answered")?;
    let (status, description) = http(&location, format!("GET {} HTTP/1.1\r\n", location.path), "")?;
    if status != 200 {
        return Err(format!("the router's UPnP description: HTTP {status}"));
    }
    let (control, service) = wan_service(&location, &description).ok_or("the UPnP router has no WAN connection")?;
    // The address the router reaches this machine at: the one this machine sends to it from.
    let client = local_ip_towards(control.host.ip()).ok_or("no local address towards the router")?;
    let add = |lease: u32| {
        let args = [
            ("NewRemoteHost", String::new()),
            ("NewExternalPort", port.to_string()),
            ("NewProtocol", "TCP".into()),
            ("NewInternalPort", port.to_string()),
            ("NewInternalClient", client.to_string()),
            ("NewEnabled", "1".into()),
            ("NewPortMappingDescription", DESCRIPTION.into()),
            ("NewLeaseDuration", lease.to_string()),
        ];
        soap(&control, service, "AddPortMapping", &args)
    };
    match add(LEASE) {
        Ok(_) => {}
        // Older routers take only forwards without an end.
        Err(SoapError::Upnp(725, _)) => {
            add(0).map_err(|e| format!("UPnP: {e}"))?;
        }
        Err(e) => return Err(format!("UPnP: {e}")),
    }
    let external = soap(&control, service, "GetExternalIPAddress", &[])
        .ok()
        .and_then(|body| xml_field(&body, "NewExternalIPAddress"))
        .and_then(|ip| ip.parse().ok());
    Ok((Forward::Open { via: "UPnP", external, port }, Lease::Upnp { control, service, port }))
}

/// Asks for internet gateway devices; the first answer's description URL.
fn ssdp_search(to: SocketAddr) -> Option<Url> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    let _ = socket.set_multicast_ttl_v4(2);
    let queries: Vec<String> = ["urn:schemas-upnp-org:device:InternetGatewayDevice:1", WAN_SERVICES[1]]
        .iter()
        .map(|st| format!("M-SEARCH * HTTP/1.1\r\nHOST: {SSDP}\r\nMAN: \"ssdp:discover\"\r\nMX: 1\r\nST: {st}\r\n\r\n"))
        .collect();
    let started = Instant::now();
    let mut resent = false;
    for q in &queries {
        let _ = socket.send_to(q.as_bytes(), to);
    }
    let mut buf = [0u8; 2048];
    while let Some(left) = STEP.checked_sub(started.elapsed()) {
        // Multicast is datagrams: one lost question is asked again.
        if !resent && started.elapsed() > STEP / 3 {
            for q in &queries {
                let _ = socket.send_to(q.as_bytes(), to);
            }
            resent = true;
        }
        socket.set_read_timeout(Some(left.min(STEP / 3).max(Duration::from_millis(1)))).ok()?;
        let Ok((n, _)) = socket.recv_from(&mut buf) else { continue };
        let text = String::from_utf8_lossy(&buf[..n]);
        if let Some(url) = header(&text, "location").and_then(Url::parse) {
            return Some(url);
        }
    }
    None
}

/// A header's value in an HTTP-style message, by case-insensitive name.
fn header<'a>(message: &'a str, name: &str) -> Option<&'a str> {
    message
        .lines()
        .find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case(name)).map(|(_, v)| v.trim()))
}

/// The WAN connection service of a device description, and its control URL.
fn wan_service(location: &Url, xml: &str) -> Option<(Url, &'static str)> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let base = doc
        .descendants()
        .find(|n| n.has_tag_name("URLBase"))
        .and_then(|n| n.text())
        .and_then(Url::parse)
        .unwrap_or_else(|| location.clone());
    fn child<'a>(n: roxmltree::Node<'a, '_>, name: &str) -> Option<&'a str> {
        n.children().find(|c| c.has_tag_name(name)).and_then(|c| c.text()).map(str::trim)
    }
    let services: Vec<(&str, &str)> = doc
        .descendants()
        .filter(|n| n.has_tag_name("service"))
        .filter_map(|n| Some((child(n, "serviceType")?, child(n, "controlURL")?)))
        .collect();
    // An IP connection before a PPP one: a router lists both when it can dial either.
    WAN_SERVICES.iter().find_map(|&want| {
        let (_, control) = services.iter().find(|(kind, _)| *kind == want)?;
        Some((base.join(control)?, want))
    })
}

/// The text of the first element named `name`, in a SOAP answer.
fn xml_field(xml: &str, name: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    doc.descendants().find(|n| n.has_tag_name(name)).and_then(|n| n.text()).map(|t| t.trim().to_owned())
}

fn soap(control: &Url, service: &str, action: &str, args: &[(&str, String)]) -> Result<String, SoapError> {
    let fields: String = args.iter().map(|(k, v)| format!("<{k}>{v}</{k}>")).collect();
    let body = format!(
        "<?xml version=\"1.0\"?>\r\n<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body><u:{action} xmlns:u=\"{service}\">\
         {fields}</u:{action}></s:Body></s:Envelope>\r\n"
    );
    let head = format!(
        "POST {} HTTP/1.1\r\nContent-Type: text/xml; charset=\"utf-8\"\r\nSOAPAction: \"{service}#{action}\"\r\n",
        control.path
    );
    match http(control, head, &body).map_err(SoapError::Http)? {
        (200, reply) => Ok(reply),
        (status, reply) => {
            let code = xml_field(&reply, "errorCode").and_then(|c| c.parse().ok()).unwrap_or(0);
            let text = xml_field(&reply, "errorDescription").unwrap_or_else(|| format!("HTTP {status}"));
            Err(SoapError::Upnp(code, text))
        }
    }
}

/// One HTTP exchange: `head` is the request line and any headers; the status and body.
fn http(url: &Url, head: String, body: &str) -> Result<(u16, String), String> {
    let mut s = TcpStream::connect_timeout(&url.host, STEP).map_err(|e| format!("can't reach the router: {e}"))?;
    s.set_read_timeout(Some(STEP)).and_then(|_| s.set_write_timeout(Some(STEP))).map_err(|e| e.to_string())?;
    let request =
        format!("{head}Host: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", url.host, body.len());
    s.write_all(request.as_bytes()).map_err(|e| format!("the router: {e}"))?;
    let mut reply = Vec::new();
    // Bounded: a description is a few kilobytes.
    s.take(1 << 20).read_to_end(&mut reply).map_err(|e| format!("the router: {e}"))?;
    let reply = String::from_utf8_lossy(&reply);
    let (head, body) = reply.split_once("\r\n\r\n").ok_or("the router sent no HTTP answer")?;
    let status = head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).ok_or("the router sent no HTTP status")?;
    let chunked = header(head, "transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked"));
    Ok((status, if chunked { dechunk(body) } else { body.to_owned() }))
}

/// A chunked HTTP body, joined.
fn dechunk(mut body: &str) -> String {
    let mut out = String::new();
    while let Some((size, rest)) = body.split_once("\r\n") {
        let Ok(n) = usize::from_str_radix(size.split(';').next().unwrap_or("").trim(), 16) else { break };
        if n == 0 || n > rest.len() {
            break;
        }
        out.push_str(&rest[..n]);
        body = rest[n..].trim_start_matches("\r\n");
    }
    out
}

// ---- PCP and NAT-PMP -----------------------------------------------------------------------

#[derive(Debug)]
enum Pcp {
    /// A NAT-PMP router: try that.
    OldVersion,
    Silent,
    Refused(String),
}

/// A PCP MAP request (RFC 6887): forward TCP `internal` to `client` for `lifetime` seconds,
/// asking for the same port outside; lifetime 0 removes it.
fn pcp_map(client: Ipv4Addr, nonce: [u8; 12], internal: u16, external: u16, lifetime: u32) -> Vec<u8> {
    let mut b = vec![2, 1, 0, 0];
    b.extend_from_slice(&lifetime.to_be_bytes());
    b.extend_from_slice(&client.to_ipv6_mapped().octets());
    b.extend_from_slice(&nonce);
    b.extend_from_slice(&[6, 0, 0, 0]);
    b.extend_from_slice(&internal.to_be_bytes());
    b.extend_from_slice(&external.to_be_bytes());
    b.extend_from_slice(&Ipv4Addr::UNSPECIFIED.to_ipv6_mapped().octets());
    b
}

/// A PCP MAP answer: the forwarded port and public address.
fn pcp_answer(b: &[u8], nonce: [u8; 12]) -> Result<(u16, IpAddr), Pcp> {
    match b {
        [0, ..] => Err(Pcp::OldVersion),
        _ if b.len() < 60 || b[0] != 2 || b[1] != 0x81 || b[24..36] != nonce => {
            Err(Pcp::Refused("a garbled answer".into()))
        }
        _ if b[3] != 0 => Err(Pcp::Refused(format!("result code {}", b[3]))),
        _ => {
            let port = u16::from_be_bytes([b[42], b[43]]);
            let ip = Ipv6Addr::from(<[u8; 16]>::try_from(&b[44..60]).unwrap());
            Ok((port, ip.to_ipv4_mapped().map_or(IpAddr::V6(ip), IpAddr::V4)))
        }
    }
}

fn pcp_open(port: u16, gateway: SocketAddr) -> Result<(Forward, Lease), Pcp> {
    let client = match local_ip_towards(gateway.ip()) {
        Some(IpAddr::V4(ip)) => ip,
        _ => return Err(Pcp::Silent),
    };
    // Matches an answer to its request; any value that differs between requests does.
    let nonce: [u8; 12] = {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        let mut n = [0u8; 12];
        n.copy_from_slice(&(t ^ u128::from(std::process::id()) << 64).to_le_bytes()[..12]);
        n
    };
    let reply = pcp_exchange(gateway, &pcp_map(client, nonce, port, port, LEASE))?;
    let (external, ip) = pcp_answer(&reply, nonce)?;
    let lease = Lease::Pcp { gateway, client, nonce, internal: port };
    Ok((Forward::Open { via: "PCP", external: Some(ip), port: external }, lease))
}

fn pcp_exchange(gateway: SocketAddr, request: &[u8]) -> Result<Vec<u8>, Pcp> {
    datagram(gateway, request, 4).ok_or(Pcp::Silent)
}

/// A NAT-PMP mapping request (RFC 6886) for TCP; lifetime 0 removes it.
fn pmp_map(internal: u16, external: u16, lifetime: u32) -> Vec<u8> {
    let mut b = vec![0, 2, 0, 0];
    b.extend_from_slice(&internal.to_be_bytes());
    b.extend_from_slice(&external.to_be_bytes());
    b.extend_from_slice(&lifetime.to_be_bytes());
    b
}

/// A NAT-PMP answer of `len` bytes to opcode `op`, checked for success.
fn pmp_reply(b: &[u8], op: u8, len: usize) -> Result<&[u8], String> {
    if b.len() < len || b[0] != 0 || b[1] != 128 + op {
        return Err("a garbled answer".into());
    }
    match u16::from_be_bytes([b[2], b[3]]) {
        0 => Ok(b),
        2 => Err("the router has port forwarding switched off".into()),
        code => Err(format!("the router refused (result code {code})")),
    }
}

fn pmp_exchange(gateway: SocketAddr, request: &[u8], len: usize) -> Result<Vec<u8>, String> {
    let reply = datagram(gateway, request, len).ok_or("no answer")?;
    pmp_reply(&reply, request[1], len).map(<[u8]>::to_vec)
}

fn pmp_open(port: u16, gateway: SocketAddr) -> Result<(Forward, Lease), String> {
    let mapped = pmp_exchange(gateway, &pmp_map(port, port, LEASE), 16)?;
    let external = u16::from_be_bytes([mapped[10], mapped[11]]);
    let ip = pmp_exchange(gateway, &[0, 0], 12).ok().map(|b| IpAddr::V4(Ipv4Addr::new(b[8], b[9], b[10], b[11])));
    Ok((Forward::Open { via: "NAT-PMP", external: ip, port: external }, Lease::Pmp { gateway, internal: port }))
}

/// Sends `request` to `to` and waits for an answer of at least `min` bytes from it, asking
/// again once, as RFC 6886 retransmits (but not for as long).
fn datagram(to: SocketAddr, request: &[u8], min: usize) -> Option<Vec<u8>> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect(to).ok()?;
    let mut buf = [0u8; 1100];
    for wait in [200, 400] {
        socket.send(request).ok()?;
        socket.set_read_timeout(Some(Duration::from_millis(wait))).ok()?;
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(wait) {
            match socket.recv(&mut buf) {
                Ok(n) if n >= min || buf[0] == 0 => return Some(buf[..n].to_vec()),
                Ok(_) => {}
                // Nothing listens at the gateway's port: its "port unreachable" ends the wait.
                Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => return None,
                Err(_) => break,
            }
        }
    }
    None
}

// ---- this machine's addresses --------------------------------------------------------------

/// The address this machine sends to `to` from. Connecting a UDP socket only picks the route;
/// no datagram leaves the machine.
pub fn local_ip_towards(to: IpAddr) -> Option<IpAddr> {
    let any: IpAddr = if to.is_ipv4() { Ipv4Addr::UNSPECIFIED.into() } else { Ipv6Addr::UNSPECIFIED.into() };
    let s = UdpSocket::bind((any, 0)).ok()?;
    s.connect((to, 9)).ok()?;
    s.local_addr().ok().map(|a| a.ip()).filter(|ip| !ip.is_unspecified())
}

/// Where this machine can be reached: its LAN address, and its public IPv6 address when it has
/// one (IPv6 needs no forward, only a firewall that lets the port in).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Addresses {
    pub lan: Option<Ipv4Addr>,
    pub ipv6: Option<Ipv6Addr>,
}

impl Addresses {
    /// The source addresses towards the documentation ranges (RFC 5737, RFC 3849), which pick
    /// the default routes' interfaces without sending anything anywhere.
    pub fn of_this_machine() -> Addresses {
        let lan = match local_ip_towards(Ipv4Addr::new(192, 0, 2, 1).into()) {
            Some(IpAddr::V4(ip)) => Some(ip),
            _ => None,
        };
        let ipv6 = match local_ip_towards(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1).into()) {
            // Global unicast only: a ULA or link-local address isn't reachable from outside.
            Some(IpAddr::V6(ip)) if ip.segments()[0] & 0xe000 == 0x2000 => Some(ip),
            _ => None,
        };
        Addresses { lan, ipv6 }
    }
}

/// Whether a router's "public" address is one no one outside can reach: the provider puts the
/// router behind its own NAT (carrier-grade NAT, 100.64/10) or another router.
pub fn behind_another_nat(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || (v4.octets()[0] == 100 && v4.octets()[1] & 0xc0 == 64),
        IpAddr::V6(_) => false,
    }
}

/// The default gateway: from the kernel's routing table on Linux, elsewhere the LAN address
/// with its last byte 1, where nearly every home router sits.
fn default_gateway() -> Option<Ipv4Addr> {
    let table = std::fs::read_to_string("/proc/net/route").unwrap_or_default();
    let from_table = table.lines().skip(1).find_map(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        (f.get(1) == Some(&"00000000"))
            .then(|| u32::from_str_radix(f.get(2)?, 16).ok())
            .flatten()
            .map(|g| Ipv4Addr::from(g.to_le_bytes()))
    });
    from_table.or_else(|| {
        let lan = Addresses::of_this_machine().lan?.octets();
        Some(Ipv4Addr::new(lan[0], lan[1], lan[2], 1))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::Arc;

    /// A fake UPnP router on loopback: answers the SSDP search with its description's URL,
    /// serves the description, and records the SOAP actions it gets.
    fn fake_igd(refuse_leases: bool) -> (Router, Arc<Mutex<Vec<String>>>) {
        let http = TcpListener::bind("127.0.0.1:0").unwrap();
        let http_port = http.local_addr().unwrap().port();
        let ssdp = UdpSocket::bind("127.0.0.1:0").unwrap();
        let router = Router { ssdp: ssdp.local_addr().unwrap(), pmp: Some("127.0.0.1:9".parse().unwrap()) };
        std::thread::spawn(move || {
            let mut buf = [0u8; 2048];
            while let Ok((n, from)) = ssdp.recv_from(&mut buf) {
                if String::from_utf8_lossy(&buf[..n]).starts_with("M-SEARCH") {
                    let answer = format!(
                        "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=120\r\nST: x\r\nLocation: http://127.0.0.1:{http_port}/rootDesc.xml\r\n\r\n"
                    );
                    let _ = ssdp.send_to(answer.as_bytes(), from);
                }
            }
        });
        let actions = Arc::new(Mutex::new(Vec::new()));
        let seen = actions.clone();
        std::thread::spawn(move || {
            for stream in http.incoming() {
                let mut s = stream.unwrap();
                let mut req = Vec::new();
                let mut buf = [0u8; 4096];
                // Read the head, then as much body as it announces.
                loop {
                    let n = s.read(&mut buf).unwrap();
                    req.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&req);
                    if let Some((head, body)) = text.split_once("\r\n\r\n") {
                        let len = header(head, "content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
                        if body.len() >= len {
                            break;
                        }
                    }
                }
                let text = String::from_utf8_lossy(&req).into_owned();
                let (status, body) = if text.starts_with("GET /rootDesc.xml") {
                    // Chunked, as some routers send it, and with a PPP service listed first.
                    let desc = "<?xml version=\"1.0\"?><root xmlns=\"urn:schemas-upnp-org:device-1-0\"><device>\
                        <serviceList><service><serviceType>urn:schemas-upnp-org:service:WANPPPConnection:1</serviceType>\
                        <controlURL>/ppp</controlURL></service><service>\
                        <serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>\
                        <controlURL>ctl/IPConn</controlURL></service></serviceList></device></root>";
                    let (a, b) = desc.split_at(40);
                    let chunked = format!("{:x}\r\n{a}\r\n{:x}\r\n{b}\r\n0\r\n\r\n", a.len(), b.len());
                    let _ = write!(s, "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{chunked}");
                    continue;
                } else {
                    let action = header(&text, "soapaction").unwrap_or("").trim_matches('"').to_owned();
                    let path = text.split_whitespace().nth(1).unwrap_or("").to_owned();
                    let lease = text.split("<NewLeaseDuration>").nth(1).and_then(|r| r.split('<').next()).unwrap_or("");
                    seen.lock().unwrap().push(format!("{path} {action} {lease}"));
                    // As a real router answers: the envelope's namespaces declared.
                    let envelope = |body: &str| {
                        format!(
                            "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\">\
                             <s:Body>{body}</s:Body></s:Envelope>"
                        )
                    };
                    if action.ends_with("#AddPortMapping") && refuse_leases && lease != "0" {
                        (
                            500,
                            envelope(
                                "<s:Fault><detail><UPnPError xmlns=\"urn:schemas-upnp-org:control-1-0\"><errorCode>725\
                             </errorCode><errorDescription>OnlyPermanentLeasesSupported</errorDescription></UPnPError>\
                             </detail></s:Fault>",
                            ),
                        )
                    } else if action.ends_with("#GetExternalIPAddress") {
                        (
                            200,
                            envelope(&format!(
                                "<u:GetExternalIPAddressResponse xmlns:u=\"{}\"><NewExternalIPAddress>203.0.113.7\
                             </NewExternalIPAddress></u:GetExternalIPAddressResponse>",
                                WAN_SERVICES[1]
                            )),
                        )
                    } else {
                        (200, envelope(""))
                    }
                };
                let _ = write!(s, "HTTP/1.1 {status} X\r\nContent-Length: {}\r\n\r\n{body}", body.len());
            }
        });
        (router, actions)
    }

    /// Through a fake UPnP router: the forward is made on the IP connection's control URL for
    /// this machine, the public address read from the router, and the forward removed on drop;
    /// a router that takes only permanent forwards gets one.
    #[test]
    fn upnp_forwards_the_port_and_removes_it() {
        for refuse in [false, true] {
            let (router, actions) = fake_igd(refuse);
            let mut map = PortMap::request(47123, router);
            map.wait(Duration::from_secs(5));
            assert_eq!(
                map.state(),
                &Forward::Open { via: "UPnP", external: Some("203.0.113.7".parse().unwrap()), port: 47123 }
            );
            drop(map);
            let service = WAN_SERVICES[1];
            let mut want = vec![format!("/ctl/IPConn {service}#AddPortMapping {LEASE}")];
            if refuse {
                want.push(format!("/ctl/IPConn {service}#AddPortMapping 0"));
            }
            want.push(format!("/ctl/IPConn {service}#GetExternalIPAddress "));
            want.push(format!("/ctl/IPConn {service}#DeletePortMapping "));
            assert_eq!(*actions.lock().unwrap(), want);
        }
    }

    /// A fake NAT-PMP router: without UPnP or PCP, NAT-PMP forwards the port, and the drop
    /// sends the removal (lifetime 0).
    #[test]
    fn nat_pmp_forwards_the_port_when_upnp_is_missing() {
        let pmp = UdpSocket::bind("127.0.0.1:0").unwrap();
        let silent = UdpSocket::bind("127.0.0.1:0").unwrap();
        let router = Router { ssdp: silent.local_addr().unwrap(), pmp: Some(pmp.local_addr().unwrap()) };
        let got = Arc::new(Mutex::new(Vec::new()));
        let seen = got.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 1100];
            while let Ok((n, from)) = pmp.recv_from(&mut buf) {
                let req = buf[..n].to_vec();
                seen.lock().unwrap().push(req.clone());
                let reply = match req[..2] {
                    // PCP: "unsupported version", as a NAT-PMP router answers it.
                    [2, _] => vec![0, 0x81, 0, 1, 0, 0, 0, 0],
                    [0, 0] => vec![0, 128, 0, 0, 0, 0, 0, 9, 198, 51, 100, 4],
                    [0, 2] => [&[0, 130, 0, 0, 0, 0, 0, 9], &req[4..8], &req[8..12]].concat(),
                    _ => continue,
                };
                let _ = pmp.send_to(&reply, from);
            }
        });
        let mut map = PortMap::request(47124, router);
        map.wait(Duration::from_secs(5));
        assert_eq!(
            map.state(),
            &Forward::Open { via: "NAT-PMP", external: Some("198.51.100.4".parse().unwrap()), port: 47124 }
        );
        drop(map);
        let got = got.lock().unwrap();
        let kinds: Vec<(u8, u8)> = got.iter().map(|r| (r[0], r[1])).collect();
        assert_eq!(kinds, vec![(2, 1), (0, 2), (0, 0), (0, 2)]);
        assert_eq!(got[1], pmp_map(47124, 47124, LEASE));
        assert_eq!(got[3], pmp_map(47124, 0, 0), "removed: lifetime 0");
    }

    /// PCP's MAP request and answer: the fields at their RFC 6887 offsets, the answer matched
    /// to its nonce.
    #[test]
    fn pcp_messages_build_and_parse() {
        let nonce = [7u8; 12];
        let req = pcp_map(Ipv4Addr::new(192, 168, 1, 20), nonce, 8088, 8088, 3600);
        assert_eq!(req.len(), 60);
        assert_eq!(&req[..8], &[2, 1, 0, 0, 0, 0, 0x0e, 0x10]);
        assert_eq!(&req[8..24], &Ipv4Addr::new(192, 168, 1, 20).to_ipv6_mapped().octets());
        assert_eq!((&req[24..36], req[36], &req[40..44]), (&nonce[..], 6, &[0x1f, 0x98, 0x1f, 0x98][..]));
        let mut answer = req.clone();
        answer[1] = 0x81;
        answer[42..44].copy_from_slice(&9000u16.to_be_bytes());
        answer[44..60].copy_from_slice(&Ipv4Addr::new(203, 0, 113, 9).to_ipv6_mapped().octets());
        assert_eq!(pcp_answer(&answer, nonce).unwrap(), (9000, IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9))));
        assert!(matches!(pcp_answer(&answer, [0; 12]), Err(Pcp::Refused(_))), "another request's answer");
        answer[3] = 2;
        assert!(matches!(pcp_answer(&answer, nonce), Err(Pcp::Refused(_))));
        assert!(matches!(pcp_answer(&[0, 0x81, 0, 1], nonce), Err(Pcp::OldVersion)));
    }

    /// No router at all: the request gives up within its time and says so.
    #[test]
    fn no_router_closes_quickly() {
        let silent = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = silent.local_addr().unwrap();
        let started = Instant::now();
        let mut map = PortMap::request(47125, Router { ssdp: addr, pmp: Some(addr) });
        map.wait(Duration::from_secs(10));
        assert!(matches!(map.state(), Forward::Closed(_)), "{:?}", map.state());
        assert!(started.elapsed() < Duration::from_millis(2600), "took {:?}", started.elapsed());
    }

    #[test]
    fn urls_and_addresses() {
        let base = Url::parse("http://192.168.1.1:5000/rootDesc.xml").unwrap();
        assert_eq!(base.host, "192.168.1.1:5000".parse().unwrap());
        assert_eq!(base.join("ctl/IPConn").unwrap().path, "/ctl/IPConn");
        assert_eq!(base.join("http://192.168.1.1:80/x").unwrap().host.port(), 80);
        assert_eq!(Url::parse("http://10.0.0.1").unwrap().host.port(), 80);
        assert_eq!(Url::parse("http://[fe80::1]:49000/d.xml").unwrap().host.port(), 49000);
        assert_eq!(dechunk("4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n"), "Wikipedia");
        for (ip, behind) in
            [("100.72.1.2", true), ("192.168.0.5", true), ("203.0.113.1", false), ("100.128.0.1", false)]
        {
            assert_eq!(behind_another_nat(ip.parse().unwrap()), behind, "{ip}");
        }
    }
}
