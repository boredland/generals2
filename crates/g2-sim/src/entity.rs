use crate::defs::{DefId, UpgradeId};
use crate::fixed::{Fx, Vec2};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct EntityId {
    pub index: u32,
    pub generation: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct PlayerId(pub u8);

/// What an entity is currently doing. Every variant is plain data so the whole state
/// serialises into the replay hash.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Order {
    Idle,
    /// Walk to `target` at most `pace` world units per tick (a group's shared pace, so it
    /// arrives together; a lone unit's own speed).
    Move {
        target: Vec2,
        pace: Fx,
    },
    /// Collector loop: walk to `pile`, load, walk to a supply center, unload, repeat.
    Gather {
        pile: EntityId,
        phase: GatherPhase,
    },
    /// Constructor walking to (then working on) a placed foundation.
    Construct {
        site: EntityId,
    },
    /// Chase `target` and fire at it until it is destroyed.
    Attack {
        target: EntityId,
    },
    /// Walk to `dest` at `pace`, fighting the enemy `engaged` whenever one comes within
    /// acquisition range on the way (at full speed). An idle armed unit that spots an enemy
    /// switches to this with `dest` at its own position, so it walks back after the fight.
    AttackMove {
        dest: Vec2,
        pace: Fx,
        engaged: Option<EntityId>,
    },
    /// Infantry walking to `container` (a building or vehicle with room) to get in.
    Garrison {
        container: EntityId,
    },
    /// A support power's jet: flies straight to `aim`, does its power slot `slot`'s work there
    /// (an airstrike's bombs), then flies on to `exit` and leaves the map. Takes no orders.
    Sortie {
        aim: Vec2,
        exit: Vec2,
        slot: u8,
        dropped: bool,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GatherPhase {
    ToPile,
    Loading { ticks_left: u32 },
    ToDepot { depot: EntityId },
    Unloading { depot: EntityId, ticks_left: u32 },
}

/// What a production queue makes: a unit, or the research of an upgrade.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Product {
    Unit(DefId),
    Upgrade(UpgradeId),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Production {
    pub item: Product,
    /// Progress in "full-speed ticks"; unpowered buildings add half a tick per tick.
    pub progress_halves: u32,
}

impl Production {
    /// The unit being trained, if this is not a research.
    pub fn unit(&self) -> Option<DefId> {
        match self.item {
            Product::Unit(d) => Some(d),
            Product::Upgrade(_) => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entity {
    pub id: EntityId,
    pub def: DefId,
    pub owner: Option<PlayerId>,
    pub pos: Vec2,
    pub hp: i32,
    pub order: Order,
    /// Credits carried by a collector.
    pub cargo: i64,
    /// For buildings under construction: completed build ticks. `None` once finished.
    pub construction: Option<u32>,
    pub queue: Vec<Production>,
    /// Supply pile credits left / oil well reserve left.
    pub resource: i64,
    /// Oil derrick: the well it sits on.
    pub on_well: Option<EntityId>,
    /// Supply center the collector is bound to (counts against that center's cap).
    pub home: Option<EntityId>,
    pub powered: bool,
    /// Oil derrick: ticks into the current second of income.
    pub income_ticks: u32,
    /// Armed units: ticks until the weapon can fire again.
    pub cooldown: u32,
    /// Deployable units (`Def::deploy`): set up for the main gun, or between modes.
    pub deploy: DeployState,
    /// Tick this entity last took damage; repair upgrades wait for a quiet spell after it.
    pub last_hit: Option<u32>,
    /// Experience from kills; sets its rank (`World::rank`).
    pub experience: u32,
    /// The building or vehicle this unit is inside, if any. Inside it neither moves nor can be
    /// targeted; its position follows the container's.
    pub inside: Option<EntityId>,
    /// Stealthy units: the tick until which firing keeps them revealed. `None` when not set.
    pub revealed_until: Option<u32>,
    /// Hidden as a stealth unit by a cloaking power until this tick (or until it fires).
    pub cloaked_until: Option<u32>,
    /// The current plan for reaching the order's goal. Part of the state hash.
    pub route: Route,
}

/// A deployable unit's mode. Only `Packed` can move; only `Deployed` fires the main gun.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DeployState {
    #[default]
    Packed,
    /// Setting up; deployed when this reaches 0.
    Deploying(u32),
    Deployed,
    /// Packing up; packed (and free to move) when this reaches 0.
    Packing(u32),
}

/// A planned way around obstacles to the current `approach` target. Planned once per target
/// and grid, kept while both are unchanged. No plan (`target: None`) means the straight line
/// was clear when last checked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Route {
    /// Grid key the plan was made on.
    pub key: u64,
    pub target: Option<Vec2>,
    /// The target was unreachable: the waypoints lead to the closest reachable cell, where
    /// the unit waits until the grid or its target changes.
    pub partial: bool,
    /// Remaining waypoints, stored goal-first so the next one is `last()`.
    pub waypoints: Vec<Vec2>,
}

impl Entity {
    pub fn is_complete(&self) -> bool {
        self.construction.is_none()
    }
}

/// Most slots are occupied, so boxing `Entity` to shrink the rare free slot would only add an
/// allocation per spawn.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
enum Slot {
    Occupied(Entity),
    Free { generation: u32, next_free: Option<u32> },
}

/// Generational arena. Iteration is always in index order, which keeps every system
/// deterministic without sorting.
#[derive(Clone, Debug, Default)]
pub struct Entities {
    slots: Vec<Slot>,
    free_head: Option<u32>,
}

impl Entities {
    pub fn spawn(&mut self, make: impl FnOnce(EntityId) -> Entity) -> EntityId {
        let (index, generation) = match self.free_head {
            Some(index) => {
                let Slot::Free { generation, next_free } = self.slots[index as usize] else {
                    unreachable!("free list points at an occupied slot")
                };
                self.free_head = next_free;
                (index, generation)
            }
            None => {
                self.slots.push(Slot::Free { generation: 0, next_free: None });
                (self.slots.len() as u32 - 1, 0)
            }
        };
        let id = EntityId { index, generation };
        self.slots[index as usize] = Slot::Occupied(make(id));
        id
    }

    pub fn despawn(&mut self, id: EntityId) -> Option<Entity> {
        let slot = self.slots.get_mut(id.index as usize)?;
        match slot {
            Slot::Occupied(e) if e.id == id => {
                let next = Slot::Free { generation: id.generation.wrapping_add(1), next_free: self.free_head };
                let Slot::Occupied(entity) = core::mem::replace(slot, next) else { unreachable!() };
                self.free_head = Some(id.index);
                Some(entity)
            }
            _ => None,
        }
    }

    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        match self.slots.get(id.index as usize)? {
            Slot::Occupied(e) if e.id == id => Some(e),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, id: EntityId) -> Option<&mut Entity> {
        match self.slots.get_mut(id.index as usize)? {
            Slot::Occupied(e) if e.id == id => Some(e),
            _ => None,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Entity> {
        self.slots.iter().filter_map(|s| match s {
            Slot::Occupied(e) => Some(e),
            Slot::Free { .. } => None,
        })
    }

    /// Ids in index order; snapshot first when the loop body needs `&mut self`.
    pub fn ids(&self) -> Vec<EntityId> {
        self.iter().map(|e| e.id).collect()
    }

    pub fn len(&self) -> usize {
        self.iter().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn slot_count(&self) -> usize {
        self.slots.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank(id: EntityId) -> Entity {
        Entity {
            id,
            def: DefId(0),
            owner: None,
            pos: Vec2::ZERO,
            hp: 1,
            order: Order::Idle,
            cargo: 0,
            construction: None,
            queue: Vec::new(),
            resource: 0,
            on_well: None,
            home: None,
            powered: false,
            income_ticks: 0,
            cooldown: 0,
            deploy: DeployState::Packed,
            last_hit: None,
            experience: 0,
            inside: None,
            revealed_until: None,
            cloaked_until: None,
            route: Route::default(),
        }
    }

    #[test]
    fn stale_ids_do_not_resolve_after_slot_reuse() {
        let mut es = Entities::default();
        let a = es.spawn(blank);
        let b = es.spawn(blank);
        assert!(es.despawn(a).is_some());
        assert!(es.despawn(a).is_none(), "double despawn must be a no-op");
        let c = es.spawn(blank);
        assert_eq!(c.index, a.index);
        assert_ne!(c.generation, a.generation);
        assert!(es.get(a).is_none());
        assert!(es.get(c).is_some());
        assert_eq!(es.ids(), vec![c, b]);
    }
}
