//! Canonical state hash. Two worlds with equal hashes at a tick are treated as identical; any
//! field that influences future ticks must be fed in here, in a fixed order.

use crate::entity::{GatherPhase, Order};
use crate::world::World;

struct Fnv(u64);

impl Fnv {
    fn u64(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    fn i64(&mut self, v: i64) {
        self.u64(v as u64);
    }
}

impl crate::map::MapDef {
    /// Fingerprint of everything the sim reads from a map. Replays store it, so a changed map
    /// file (a submodule bump, an importer change) is reported as such, not as a divergence.
    pub fn fingerprint(&self) -> u64 {
        let mut h = Fnv(0xcbf2_9ce4_8422_2325);
        h.i64(self.width.0);
        h.i64(self.height.0);
        for list in [&self.spawns.iter().map(|&p| (p, 0)).collect::<Vec<_>>(), &self.supply_piles, &self.oil_wells] {
            h.u64(list.len() as u64);
            for &(p, amount) in list {
                h.i64(p.x.0);
                h.i64(p.y.0);
                h.i64(amount);
            }
        }
        // Hashed only when present, so open-ground maps (Crater Ridge) keep their fingerprint.
        if self.terrain.iter().any(|&b| b) {
            h.u64(self.terrain.len() as u64);
            for (i, _) in self.terrain.iter().enumerate().filter(|&(_, &b)| b) {
                h.u64(i as u64);
            }
        }
        h.0
    }
}

impl World {
    pub fn state_hash(&self) -> u64 {
        let mut h = Fnv(0xcbf2_9ce4_8422_2325);
        h.u64(self.tick as u64);
        let (s, inc) = self.rng.raw_state();
        h.u64(s);
        h.u64(inc);
        for p in &self.players {
            h.u64(p.faction as u64);
            h.i64(p.credits);
            h.i64(p.oil);
            h.i64(p.power_supply as i64);
            h.i64(p.power_drain as i64);
            h.u64(p.defeated as u64);
            // Only when set, so hashes of matches without generals (the golden replay) are
            // unchanged.
            if let Some(g) = p.general {
                h.u64(0x6e6e_0000 | g.0 as u64);
            }
        }
        h.u64(self.entities.slot_count() as u64);
        for e in self.entities.iter() {
            h.u64(e.id.index as u64);
            h.u64(e.id.generation as u64);
            h.u64(e.def.0 as u64);
            h.u64(e.owner.map_or(u64::MAX, |o| o.0 as u64));
            h.i64(e.pos.x.0);
            h.i64(e.pos.y.0);
            h.i64(e.hp as i64);
            hash_order(&mut h, e.order);
            h.i64(e.cargo);
            h.u64(e.construction.map_or(u64::MAX, u64::from));
            h.u64(e.queue.len() as u64);
            for q in &e.queue {
                h.u64(q.def.0 as u64);
                h.u64(q.progress_halves as u64);
            }
            h.i64(e.resource);
            h.u64(e.on_well.map_or(u64::MAX, |w| ((w.index as u64) << 32) | w.generation as u64));
            h.u64(e.home.map_or(u64::MAX, |w| ((w.index as u64) << 32) | w.generation as u64));
            h.u64(e.powered as u64);
            h.u64(e.income_ticks as u64);
            let r = &e.route;
            h.u64(r.key);
            match r.target {
                Some(t) => {
                    h.i64(t.x.0);
                    h.i64(t.y.0);
                }
                None => h.u64(u64::MAX),
            }
            h.u64(r.partial as u64);
            h.u64(r.waypoints.len() as u64);
            for p in &r.waypoints {
                h.i64(p.x.0);
                h.i64(p.y.0);
            }
        }
        h.0
    }
}

fn id(h: &mut Fnv, e: crate::entity::EntityId) {
    h.u64(((e.index as u64) << 32) | e.generation as u64);
}

fn hash_order(h: &mut Fnv, o: Order) {
    match o {
        Order::Idle => h.u64(0),
        Order::Move { target } => {
            h.u64(1);
            h.i64(target.x.0);
            h.i64(target.y.0);
        }
        Order::Gather { pile, phase } => {
            h.u64(2);
            id(h, pile);
            match phase {
                GatherPhase::ToPile => h.u64(0),
                GatherPhase::Loading { ticks_left } => {
                    h.u64(1);
                    h.u64(ticks_left as u64);
                }
                GatherPhase::ToDepot { depot } => {
                    h.u64(2);
                    id(h, depot);
                }
                GatherPhase::Unloading { depot, ticks_left } => {
                    h.u64(3);
                    id(h, depot);
                    h.u64(ticks_left as u64);
                }
            }
        }
        Order::Construct { site } => {
            h.u64(3);
            id(h, site);
        }
    }
}
