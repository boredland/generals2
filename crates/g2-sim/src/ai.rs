//! Built-in opponent. It runs inside the simulation and only issues ordinary commands, so a
//! replay of an AI match reproduces exactly without recording AI decisions separately.

use crate::command::CommandKind;
use crate::defs::{BuildingRole, Class};
use crate::entity::{EntityId, Order, PlayerId};
use crate::fixed::{Fx, Vec2};
use crate::world::{TICKS_PER_SECOND, World};

/// Decisions every second, not every tick: cheaper and closer to a human's pace.
const THINK_INTERVAL: u32 = TICKS_PER_SECOND;

pub struct Ai {
    player: PlayerId,
}

impl Ai {
    pub fn new(player: PlayerId) -> Ai {
        Ai { player }
    }

    pub(crate) fn think(&mut self, world: &World, out: &mut Vec<CommandKind>) {
        if world.tick() % THINK_INTERVAL != self.player.0 as u32 % THINK_INTERVAL {
            return;
        }
        let me = self.player;
        let rules = world.rules;
        let mine: Vec<_> = world.entities().filter(|e| e.owner == Some(me)).collect();
        let has = |role: fn(BuildingRole) -> bool| mine.iter().any(|e| rules.def(e.def).role().is_some_and(role));
        let idle_constructor =
            mine.iter().find(|e| rules.def(e.def).is_constructor() && e.order == Order::Idle).map(|e| e.id);
        let cc = mine.iter().find(|e| rules.def(e.def).role() == Some(BuildingRole::CommandCenter));

        // 1. Finish any abandoned foundation first.
        if let (Some(builder), Some(site)) = (idle_constructor, mine.iter().find(|e| e.construction.is_some())) {
            out.push(CommandKind::Construct { units: vec![builder], site: site.id });
            return;
        }

        // 2. Base layout: supply center, then power (if the faction uses it), then an oil derrick.
        if let (Some(builder), Some(cc)) = (idle_constructor, cc) {
            let builder_def = rules.def(world.entity(builder).unwrap().def);
            let want = if !has(|r| matches!(r, BuildingRole::SupplyCenter { .. })) {
                builder_def
                    .builds
                    .iter()
                    .copied()
                    .find(|&b| matches!(rules.def(b).role(), Some(BuildingRole::SupplyCenter { .. })))
            } else if world.player(me).faction.uses_power() && !has(|r| r == BuildingRole::PowerPlant) {
                builder_def.builds.iter().copied().find(|&b| rules.def(b).role() == Some(BuildingRole::PowerPlant))
            } else {
                None
            };
            if let Some(def) = want
                && world.player(me).credits >= rules.def(def).cost.credits
                && let Some(pos) = self.find_spot(world, def, cc.pos)
            {
                out.push(CommandKind::Build { builder, def, pos });
                return;
            }
            let derrick = builder_def
                .builds
                .iter()
                .copied()
                .find(|&b| matches!(rules.def(b).role(), Some(BuildingRole::OilDerrick { .. })));
            if let Some(def) = derrick
                && world.player(me).credits >= rules.def(def).cost.credits
                && let Some(well) = self.free_well(world, def, cc.pos)
            {
                out.push(CommandKind::Build { builder, def, pos: well });
                return;
            }
        }

        // 3. Fill every supply center up to its collector cap.
        for depot in mine.iter().filter(|e| e.is_complete()) {
            let d = rules.def(depot.def);
            let Some(BuildingRole::SupplyCenter { collector_cap }) = d.role() else { continue };
            let Some(&collector) = d.produces.iter().find(|&&u| rules.def(u).collector().is_some()) else { continue };
            if world.depot_load(depot.id) < collector_cap
                && world.player(me).credits >= rules.def(collector).cost.credits
            {
                out.push(CommandKind::Train { producer: depot.id, def: collector });
            }
        }

        // 4. Put idle collectors back to work.
        let idle: Vec<EntityId> = mine
            .iter()
            .filter(|e| rules.def(e.def).collector().is_some() && e.order == Order::Idle)
            .map(|e| e.id)
            .collect();
        if let (Some(&first), false) = (idle.first(), idle.is_empty())
            && let Some(pile) = world.nearest_pile(world.entity(first).unwrap().pos)
        {
            out.push(CommandKind::Gather { units: idle, pile });
        }
    }

    /// Spiral search outward from `around` for the first valid foundation spot.
    fn find_spot(&self, world: &World, def: crate::defs::DefId, around: Vec2) -> Option<Vec2> {
        let step = world.rules.def(def).radius * Fx::from_int(2) + Fx::from_int(4);
        for ring in 1..12i64 {
            for k in 0..8 * ring {
                let (dx, dy) = ring_offset(ring, k);
                let pos = around + Vec2::new(step * Fx::from_int(dx), step * Fx::from_int(dy));
                if world.check_placement(def, pos).is_ok() {
                    return Some(pos);
                }
            }
        }
        None
    }

    fn free_well(&self, world: &World, derrick: crate::defs::DefId, around: Vec2) -> Option<Vec2> {
        let mut best: Option<(Fx, Vec2)> = None;
        for w in world.entities().filter(|e| matches!(world.rules.def(e.def).class, Class::OilWell)) {
            if world.check_placement(derrick, w.pos).is_err() {
                continue;
            }
            let d = w.pos.distance(around);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, w.pos));
            }
        }
        best.map(|(_, p)| p)
    }
}

/// The `k`-th cell (0-based, `0..8*ring`) on the square ring at Chebyshev distance `ring`.
fn ring_offset(ring: i64, k: i64) -> (i64, i64) {
    let side = 2 * ring;
    let (edge, t) = (k / side, k % side);
    match edge {
        0 => (-ring + t, -ring),
        1 => (ring, -ring + t),
        2 => (ring - t, ring),
        _ => (-ring, ring - t),
    }
}

#[cfg(test)]
mod tests {
    use super::ring_offset;

    #[test]
    fn ring_covers_each_cell_once() {
        for ring in 1..5 {
            let mut cells: Vec<_> = (0..8 * ring).map(|k| ring_offset(ring, k)).collect();
            assert!(cells.iter().all(|&(x, y)| x.abs().max(y.abs()) == ring));
            cells.sort();
            cells.dedup();
            assert_eq!(cells.len() as i64, 8 * ring);
        }
    }
}
