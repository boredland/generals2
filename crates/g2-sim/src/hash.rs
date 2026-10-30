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
        for cells in [&self.terrain, &self.decks] {
            if cells.iter().any(|&b| b) {
                h.u64(cells.len() as u64);
                for (i, _) in cells.iter().enumerate().filter(|&(_, &b)| b) {
                    h.u64(i as u64);
                }
            }
        }
        // Hashed only when present, so maps without them keep their fingerprint.
        if !self.civilians.is_empty() {
            h.u64(self.civilians.len() as u64);
            for &(p, r) in &self.civilians {
                h.i64(p.x.0);
                h.i64(p.y.0);
                h.i64(r.0);
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
            h.i64(p.power_supply as i64);
            h.i64(p.power_drain as i64);
            h.u64(p.defeated as u64);
            // Only when set, so hashes of matches without generals (the golden replay) are
            // unchanged.
            if let Some(g) = p.general {
                h.u64(0x6e6e_0000 | g.0 as u64);
            }
            // Every filled power slot, tagged with its index (a general's is slot 0).
            for (slot, t) in p.power_ready.iter().enumerate() {
                if let Some(t) = t {
                    h.u64(0x7077_0000_0000 | (slot as u64) << 32 | *t as u64);
                }
            }
            // Only once something is researched, so matches without upgrades hash as before.
            for u in &p.upgrades {
                h.u64(0x7570_0000 | u.0 as u64);
            }
            // Only once set, so matches without stealth hash as before.
            if p.hit_by_stealth {
                h.u64(0x6873_7468);
            }
        }
        for v in &self.visions {
            v.hash_into(|x| h.u64(x));
        }
        // Only Normal AIs carry state; Easy matches (the golden replay) hash as before.
        for ai in self.ai.iter().flatten() {
            ai.hash_into(|x| h.u64(x));
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
                match q.item {
                    crate::entity::Product::Unit(d) => h.u64(d.0 as u64),
                    crate::entity::Product::Upgrade(u) => h.u64(0x7570_0000 | u.0 as u64),
                }
                h.u64(q.progress_halves as u64);
            }
            h.i64(e.resource);
            h.u64(e.on_well.map_or(u64::MAX, |w| ((w.index as u64) << 32) | w.generation as u64));
            h.u64(e.home.map_or(u64::MAX, |w| ((w.index as u64) << 32) | w.generation as u64));
            h.u64(e.powered as u64);
            h.u64(e.income_ticks as u64);
            h.u64(e.cooldown as u64);
            h.u64(e.experience as u64);
            // Only once a stealth unit has fired, so matches without stealth hash as before.
            if let Some(t) = e.revealed_until {
                h.u64(0x7276_0000_0000 | t as u64);
            }
            // Only while cloaked by a power, so matches without one hash as before.
            if let Some(t) = e.cloaked_until {
                h.u64(0x636c_0000_0000 | t as u64);
            }
            // Only once a special shot has been fired, so matches without one hash as before.
            if e.special_at != 0 {
                h.u64(0x7370_0000_0000 | e.special_at as u64);
            }
            // Only for units inside something, so matches without garrisons hash as before.
            if let Some(c) = e.inside {
                h.u64(0x6761_7272);
                id(&mut h, c);
            }
            // Only repair upgrades read it, so only owners with upgrades hash it; matches
            // without upgrades hash as before.
            if e.owner.is_some_and(|o| !self.players[o.0 as usize].upgrades.is_empty()) {
                h.u64(e.last_hit.map_or(u64::MAX, u64::from));
            }
            h.u64(match e.deploy {
                crate::entity::DeployState::Packed => 0,
                crate::entity::DeployState::Deploying(t) => 1 << 32 | t as u64,
                crate::entity::DeployState::Deployed => 2 << 32,
                crate::entity::DeployState::Packing(t) => 3 << 32 | t as u64,
            });
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
        h.u64(self.shells.len() as u64);
        for s in &self.shells {
            id(&mut h, s.shooter);
            h.u64(s.owner.0 as u64);
            h.i64(s.from.x.0);
            h.i64(s.from.y.0);
            h.i64(s.at.x.0);
            h.i64(s.at.y.0);
            h.u64(s.fired as u64);
            h.u64(s.lands as u64);
            h.i64(s.damage as i64);
            for b in s.bonus {
                h.i64(b as i64);
            }
            h.i64(s.radius.0);
            // Only when set, so shells without one (the golden replay's) hash as before.
            if s.garrison_bonus != 0 {
                h.i64(0x6762_0000 + s.garrison_bonus as i64);
            }
        }
        // Only once there are clouds, so matches without them hash as before.
        for c in &self.clouds {
            h.u64(0x636c_6f75 | (c.owner.0 as u64) << 32);
            h.i64(c.at.x.0);
            h.i64(c.at.y.0);
            h.i64(c.radius.0);
            h.i64(c.damage as i64);
            h.u64(((c.from as u64) << 32) | c.until as u64);
        }
        // Only in a mission, so other matches hash as before. What was said is output only.
        if let Some(m) = &self.mission {
            h.u64(0x6d69_7373_0000 | m.id.0 as u64);
            for s in &m.objectives {
                h.u64(*s as u64);
            }
            for &f in &m.fired {
                h.u64(f as u64);
            }
            for &(tag, e) in &m.tags {
                h.u64(tag as u64);
                id(&mut h, e);
            }
            h.u64(m.result.map_or(2, u64::from));
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
        Order::Move { target, pace } => {
            h.u64(1);
            h.i64(target.x.0);
            h.i64(target.y.0);
            h.i64(pace.0);
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
        Order::Attack { target } => {
            h.u64(4);
            id(h, target);
        }
        Order::AttackMove { dest, pace, engaged } => {
            h.u64(5);
            h.i64(dest.x.0);
            h.i64(dest.y.0);
            h.i64(pace.0);
            h.u64(engaged.map_or(u64::MAX, |e| ((e.index as u64) << 32) | e.generation as u64));
        }
        Order::Garrison { container } => {
            h.u64(6);
            id(h, container);
        }
        Order::Special { target } => {
            h.u64(8);
            id(h, target);
        }
        Order::Sortie { aim, exit, slot, dropped } => {
            h.u64(7);
            h.i64(aim.x.0);
            h.i64(aim.y.0);
            h.i64(exit.x.0);
            h.i64(exit.y.0);
            h.u64(((slot as u64) << 1) | dropped as u64);
        }
    }
}
