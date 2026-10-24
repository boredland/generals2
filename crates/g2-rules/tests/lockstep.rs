//! Two machines playing one match in lockstep over a simulated link, driven only through the
//! public `net` API: messages go through their byte encoding, arrive late and in bursts.

use g2_rules::{crater_ridge, rules};
use g2_sim::net::{DELAY_TURNS, Lockstep, Message, NetError, TURN_TICKS, Turn};
use g2_sim::{CommandKind, Faction, MatchSetup, PlayerId, PlayerSetup, Vec2, World, verify_replay};

const HOST: PlayerId = PlayerId(0);
const JOINER: PlayerId = PlayerId(1);

fn setup() -> MatchSetup {
    let players = [Faction::Eu, Faction::Gla]
        .map(|faction| PlayerSetup { faction, starting_credits: 5_000, ai: None, general: None })
        .to_vec();
    MatchSetup { map: crater_ridge(), seed: 7, players }
}

/// One machine: its world, its lockstep side, and messages in flight to it with the frame
/// they arrive on.
struct Machine {
    world: World,
    net: Lockstep,
    inbox: Vec<(u32, Vec<u8>)>,
    stalls: u32,
}

impl Machine {
    fn new(me: PlayerId, peer: PlayerId) -> Machine {
        Machine { world: World::new(&setup(), rules()), net: Lockstep::new(me, peer), inbox: Vec::new(), stalls: 0 }
    }

    /// One frame: take in what has arrived, then step once if allowed. Returns the turns sent.
    fn frame(&mut self, now: u32) -> Result<Vec<Turn>, NetError> {
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.inbox).into_iter().partition(|m| m.0 <= now);
        self.inbox = later;
        for (_, bytes) in due {
            let Message::Turn(t) = Message::decode(&bytes).unwrap() else { panic!("not a turn") };
            self.net.receive(&mut self.world, t)?;
        }
        let mut out = Vec::new();
        if self.net.before_step(&mut self.world, &mut out)? {
            self.world.step();
        } else {
            self.stalls += 1;
        }
        Ok(out)
    }
}

/// Runs both machines for `frames` frames with a link `delay` frames each way (plus `jitter`
/// on every other message), each player giving `orders(machine, tick)`.
fn play(
    frames: u32,
    delay: u32,
    jitter: u32,
    mut orders: impl FnMut(&World, PlayerId) -> Vec<CommandKind>,
) -> Result<[Machine; 2], NetError> {
    let mut m = [Machine::new(HOST, JOINER), Machine::new(JOINER, HOST)];
    let mut sent = 0u32;
    for now in 0..frames {
        for i in 0..2 {
            let me = m[i].net.me();
            for kind in orders(&m[i].world, me) {
                m[i].net.order(kind);
            }
            for t in m[i].frame(now)? {
                sent += 1;
                // Late in bursts, never reordered: TCP keeps order, so each message arrives no
                // earlier than the one before it.
                let lag = delay + if sent.is_multiple_of(2) { jitter } else { 0 };
                let at = m[1 - i].inbox.last().map_or(0, |l| l.0).max(now + lag);
                m[1 - i].inbox.push((at, Message::Turn(t).encode()));
            }
        }
    }
    Ok(m)
}

/// Each player sends its first two units toward the map centre every 20 seconds or so, at
/// different times on the two sides, plus a supply-gather order once.
fn some_orders(world: &World, me: PlayerId) -> Vec<CommandKind> {
    let t = world.tick();
    let mine = || world.entities().filter(move |e| e.owner == Some(me) && world.rules.def(e.def).is_unit());
    let units: Vec<_> = mine().map(|e| e.id).take(2).collect();
    let mut out = Vec::new();
    if t % 600 == 30 + me.0 as u32 * 17 && !units.is_empty() {
        let c = world.map.width / g2_sim::Fx::from_int(2);
        out.push(CommandKind::Move { units, target: Vec2::new(c, c) });
    }
    if t == 90 + me.0 as u32 {
        out.push(CommandKind::Stop { units: mine().map(|e| e.id).collect() });
    }
    out
}

/// The two machines step the same match: same state at every turn, same final state, and the
/// match replays from either one's log. They wait for each other when the link is slow.
#[test]
fn two_machines_stay_in_step_over_a_slow_link() {
    for (delay, jitter) in [(0, 0), (1, 0), (4, 3), (9, 0)] {
        let [a, b] = play(30 * 60, delay, jitter, some_orders).unwrap();
        let (ta, tb) = (a.world.tick(), b.world.tick());
        assert!(ta.abs_diff(tb) <= TURN_TICKS * (DELAY_TURNS + 1), "{delay}/{jitter}: ticks {ta} vs {tb}");
        // Bring the one ahead back to the other's tick by replaying, then compare.
        let n = ta.min(tb);
        let replay_a = a.world.replay(&setup());
        let mut short = replay_a.clone();
        short.length_ticks = n;
        short.hashes.retain(|&(t, _)| t <= n);
        let rb = b.world.replay(&setup());
        let mut short_b = rb.clone();
        short_b.length_ticks = n;
        short_b.hashes.retain(|&(t, _)| t <= n);
        assert_eq!(
            verify_replay(&short, crater_ridge(), rules()),
            verify_replay(&short_b, crater_ridge(), rules()),
            "{delay}/{jitter}"
        );
        // The applied commands agree, both players' orders ran, and each replay re-simulates.
        let ran = |r: &g2_sim::Replay| r.commands.iter().filter(|c| c.tick < n).cloned().collect::<Vec<_>>();
        assert_eq!(ran(&replay_a), ran(&rb), "{delay}/{jitter}");
        assert!(ran(&replay_a).iter().any(|c| c.player == HOST) && ran(&rb).iter().any(|c| c.player == JOINER));
        assert!(verify_replay(&replay_a, crater_ridge(), rules()).is_ok());
        assert!(verify_replay(&rb, crater_ridge(), rules()).is_ok());
        // A slow link makes the machines wait; a fast one never does.
        if delay == 0 {
            assert_eq!(a.stalls + b.stalls, 0);
        }
        if delay >= 9 {
            assert!(a.stalls > 0 && b.stalls > 0, "a slow link is waited for");
        }
    }
}

/// A machine whose state differs is caught at the next hash check.
#[test]
fn a_desync_is_detected() {
    let mut m = [Machine::new(HOST, JOINER), Machine::new(JOINER, HOST)];
    let dozer =
        m[1].world.entities().find(|e| e.owner == Some(JOINER) && e.def == g2_rules::ids::GLA_WORKER).unwrap().id;
    let mut found = None;
    for now in 0..30 * 10 {
        if now == 40 {
            // Bypasses the lockstep: only the joiner's world gets this order.
            m[1].world.submit(JOINER, CommandKind::Move { units: vec![dozer], target: Vec2::from_int(10, 10) });
        }
        for i in 0..2 {
            match m[i].frame(now) {
                Ok(out) => {
                    for t in out {
                        m[1 - i].inbox.push((now + 1, Message::Turn(t).encode()));
                    }
                }
                Err(e) => {
                    found.get_or_insert(e);
                }
            }
        }
    }
    let Some(NetError::Desync { tick, mine, theirs }) = found else { panic!("no desync reported: {found:?}") };
    assert_ne!(mine, theirs);
    assert!(tick > 40 && tick <= 40 + 2 * g2_sim::replay::HASH_INTERVAL, "caught at {tick}");
}

/// A machine never steps into a turn without the peer's orders for it, and a turn out of
/// order is a protocol error.
#[test]
fn a_machine_waits_for_the_peer_and_refuses_gaps() {
    let mut a = Machine::new(HOST, JOINER);
    let mut sent = Vec::new();
    // Nothing from the peer: the first `DELAY_TURNS` turns need no peer orders, then it waits.
    // It has sent its orders for each turn up to `DELAY_TURNS` ahead of where it stands.
    for now in 0..100 {
        sent.extend(a.frame(now).unwrap());
    }
    assert_eq!(a.world.tick(), DELAY_TURNS * TURN_TICKS);
    let turns: Vec<u32> = sent.iter().map(|t| t.turn).collect();
    assert_eq!(turns, (DELAY_TURNS..=2 * DELAY_TURNS).collect::<Vec<_>>());
    let skip = Turn { turn: DELAY_TURNS + 1, commands: vec![], check: None };
    assert!(matches!(a.net.receive(&mut a.world, skip), Err(NetError::Protocol(_))));
}

/// Every message survives its byte encoding, and garbage is refused, never a panic.
#[test]
fn messages_round_trip_and_garbage_is_refused() {
    let setup = World::new(&setup(), rules()).replay(&setup());
    let msgs = [
        Message::Hello { protocol: 1, setup },
        Message::Join { protocol: 1 },
        Message::Turn(Turn { turn: 9, commands: vec![], check: None }),
        Message::Turn(Turn {
            turn: 12,
            commands: vec![CommandKind::Surrender, CommandKind::Power { at: Vec2::from_int(3, 4) }],
            check: Some((30, 0xdead_beef)),
        }),
    ];
    for m in &msgs {
        let bytes = m.encode();
        assert_eq!(&Message::decode(&bytes).unwrap(), m);
        for cut in 0..bytes.len() {
            assert!(Message::decode(&bytes[..cut]).is_err(), "{m:?} cut at {cut}");
        }
    }
    let mut huge = vec![2u8, 0, 0, 0, 0];
    huge.extend_from_slice(&u32::MAX.to_le_bytes());
    assert!(Message::decode(&huge).is_err());
}
