//! Built-in opponent. It runs inside the simulation and only issues ordinary commands, so a
//! replay of an AI match reproduces exactly without recording AI decisions separately.
//!
//! Both levels run the same economy and base layout. `Easy` (the original AI) sends its army
//! in waves of five and defends with the units at home. `Normal` judges the battle first: it
//! keeps an estimate of the enemy army it has seen, attacks only with an army that outweighs
//! it, pulls units out of a fight they are losing, calls the field army home when the base is
//! attacked by more than its guards can hold, moves constructors away from enemies, builds
//! more factories when credits pile up and opens a second supply center when the base's
//! piles run low.

use crate::command::CommandKind;
use crate::defs::{BuildingRole, Class};
use crate::entity::{DeployState, Entity, EntityId, Order, PlayerId};
use crate::fixed::{Fx, Vec2};
use crate::map::AiLevel;
use crate::world::{TICKS_PER_SECOND, World};

/// Decisions every second, not every tick: cheaper and closer to a human's pace.
const THINK_INTERVAL: u32 = TICKS_PER_SECOND;
/// Idle combat units gather at the factory until there are this many, then attack together.
const WAVE: usize = 5;
/// Enemy units this close to one of our buildings trigger the whole army's defence.
const DEFEND_RADIUS: Fx = Fx::from_int(50);
/// A wave's own fight: enemies this close to its centre count against it.
const BATTLE_RADIUS: Fx = Fx::from_int(60);
/// Constructors move home when an armed enemy comes this close.
const DANGER_RADIUS: Fx = Fx::from_int(45);

pub struct Ai {
    player: PlayerId,
    level: AiLevel,
    /// Combat strength of the largest enemy army seen lately (see `strength`), fading by a
    /// sixteenth every second, so an army out of sight still counts for a while.
    enemy_seen: i64,
    /// No new wave before this tick: set by a retreat, so the army regroups first.
    regroup_until: u32,
}

/// What a unit adds to a fight: hit points times damage per second, so a fresh tank counts
/// for more than a damaged one and a fast gun for more than a slow one. Unarmed units: 0.
pub fn strength(world: &World, e: &Entity) -> i64 {
    let d = world.rules.def(e.def);
    let Some(w) = d.weapon_for(e.deploy == DeployState::Deployed).or(d.deploy.map(|dep| dep.weapon)) else {
        return 0;
    };
    e.hp.max(0) as i64 * w.damage as i64 * TICKS_PER_SECOND as i64 / w.cooldown_ticks.max(1) as i64
}

impl Ai {
    pub fn new(player: PlayerId, level: AiLevel) -> Ai {
        Ai { player, level, enemy_seen: 0, regroup_until: 0 }
    }

    /// Hash input: the state a `Normal` AI decides on. `Easy` keeps none, so it adds nothing.
    pub(crate) fn hash_into(&self, mut put: impl FnMut(u64)) {
        if self.level != AiLevel::Easy {
            put(self.enemy_seen as u64);
            put(self.regroup_until as u64);
        }
    }

    pub(crate) fn think(&mut self, world: &World, out: &mut Vec<CommandKind>) {
        if world.tick() % THINK_INTERVAL != self.player.0 as u32 % THINK_INTERVAL {
            return;
        }
        let me = self.player;
        let rules = world.rules;
        let mine: Vec<_> = world.entities().filter(|e| e.owner == Some(me)).collect();
        let idle_constructor =
            mine.iter().find(|e| rules.def(e.def).is_constructor() && e.order == Order::Idle).map(|e| e.id);
        let cc = mine.iter().find(|e| rules.def(e.def).role() == Some(BuildingRole::CommandCenter));
        let count =
            |role: fn(BuildingRole) -> bool| mine.iter().filter(|e| rules.def(e.def).role().is_some_and(role)).count();

        // 0. Never be left without a constructor: one lost (or never built) is retrained, and
        //    until it is queued the army waits, so its credits can't keep the rebuild away.
        let constructors = mine.iter().filter(|e| rules.def(e.def).is_constructor()).count()
            + mine.iter().flat_map(|e| &e.queue).filter(|p| rules.def(p.def).is_constructor()).count();
        let rebuild = mine.iter().filter(|e| e.is_complete()).find_map(|e| {
            rules.def(e.def).produces.iter().find(|&&u| rules.def(u).is_constructor()).map(|&u| (e.id, u))
        });
        let saving = constructors == 0 && rebuild.is_some();
        if constructors == 0
            && let Some((producer, def)) = rebuild
            && world.player(me).credits >= rules.def(def).cost.credits
        {
            out.push(CommandKind::Train { producer, def });
        }

        // 1. Finish any abandoned foundation first.
        if let (Some(builder), Some(site)) = (idle_constructor, mine.iter().find(|e| e.construction.is_some())) {
            out.push(CommandKind::Construct { units: vec![builder], site: site.id });
            return;
        }

        // 2. Base layout: supply center, power (if the faction uses it), one oil derrick, a
        //    barracks and a war factory; then derricks on any other free well.
        if let (Some(builder), Some(cc)) = (idle_constructor, cc) {
            let builder_def = rules.def(world.entity(builder).unwrap().def);
            let find =
                |pred: fn(&crate::defs::Def) -> bool| builder_def.builds.iter().copied().find(|&b| pred(rules.def(b)));
            let derrick = find(|d| matches!(d.role(), Some(BuildingRole::OilDerrick { .. })));
            let derricks = count(|r| matches!(r, BuildingRole::OilDerrick { .. }));
            // Barracks first: the faction's factory producing the cheaper unit.
            let factories: Vec<_> = {
                let mut f: Vec<_> = builder_def
                    .builds
                    .iter()
                    .copied()
                    .filter(|&b| rules.def(b).role() == Some(BuildingRole::Factory))
                    .collect();
                f.sort_by_key(|&b| rules.def(b).produces.first().map_or(0, |&u| rules.def(u).cost.credits));
                f
            };
            let missing_factory = factories.iter().copied().find(|&f| !mine.iter().any(|e| e.def == f));
            let depots: Vec<&&Entity> = mine
                .iter()
                .filter(|e| matches!(rules.def(e.def).role(), Some(BuildingRole::SupplyCenter { .. })))
                .collect();
            let supply = find(|d| matches!(d.role(), Some(BuildingRole::SupplyCenter { .. })));
            let smart = self.level != AiLevel::Easy;
            // Normal: expand to the nearest pile with no supply center of ours close by,
            // once the base's own pile is nearly spent; add a factory when credits pile up.
            let expansion = || {
                let near_ours = |p: Vec2| depots.iter().any(|d| d.pos.distance(p) <= Fx::from_int(60));
                let spent = world
                    .nearest_pile(cc.pos)
                    .and_then(|p| world.entity(p))
                    .is_none_or(|p| p.resource < 2_000 || p.pos.distance(cc.pos) > Fx::from_int(90));
                spent
                    .then(|| {
                        world
                            .entities()
                            .filter(|e| matches!(rules.def(e.def).class, Class::SupplyPile) && e.resource >= 2_000)
                            .filter(|e| !near_ours(e.pos))
                            .min_by_key(|e| (e.pos.distance(cc.pos), e.id))
                            .map(|e| e.pos)
                    })
                    .flatten()
            };
            let expand_to = if smart && depots.len() < 3 { expansion() } else { None };
            let rich = world.player(me).credits >= 3_000
                && mine.iter().filter(|e| rules.def(e.def).role() == Some(BuildingRole::Factory)).count() < 4;
            let want = if depots.is_empty() {
                supply
            } else if world.player(me).faction.uses_power() && count(|r| r == BuildingRole::PowerPlant) == 0 {
                find(|d| d.role() == Some(BuildingRole::PowerPlant))
            } else if derricks == 0 && derrick.is_some_and(|d| self.free_well(world, d, cc.pos).is_some()) {
                derrick
            } else if let Some(f) = missing_factory {
                Some(f)
            } else if expand_to.is_some() {
                supply
            } else if smart && rich {
                factories.last().copied()
            } else {
                derrick
            };
            if let Some(def) = want
                && world.player(me).credits >= rules.def(def).cost.credits
            {
                let pos = if Some(def) == derrick {
                    self.free_well(world, def, cc.pos)
                } else if matches!(rules.def(def).role(), Some(BuildingRole::SupplyCenter { .. })) {
                    match expand_to.filter(|_| !depots.is_empty()) {
                        // Beside the new pile, toward home.
                        Some(pile) => self.find_spot_toward(world, def, pile, cc.pos),
                        // Toward the nearest pile: shorter trips, and the same layout from every
                        // start (the spiral's fixed ring order favours one corner of the map).
                        None => {
                            let toward =
                                world.nearest_pile(cc.pos).and_then(|p| world.entity(p)).map_or(cc.pos, |p| p.pos);
                            self.find_spot_toward(world, def, cc.pos, toward)
                        }
                    }
                } else {
                    self.find_spot(world, def, cc.pos)
                };
                if let Some(pos) = pos {
                    out.push(CommandKind::Build { builder, def, pos });
                    return;
                }
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

        // 3b. Keep every factory training (one at a time, so collectors come first). A factory
        // with several units trains the one this AI owns fewest of, weighting the first (the
        // line unit) double, so artillery makes up about a third of a war factory's output.
        let owned = |def| {
            mine.iter().filter(|e| e.def == def).count()
                + mine.iter().flat_map(|e| &e.queue).filter(|p| p.def == def).count()
        };
        for f in mine.iter().filter(|e| !saving && e.is_complete() && e.queue.is_empty()) {
            if rules.def(f.def).role() != Some(BuildingRole::Factory) {
                continue;
            }
            let produces = rules.def(f.def).produces;
            let pick = produces
                .iter()
                .enumerate()
                .min_by_key(|&(i, &u)| (owned(u) * if i == 0 { 1 } else { 2 }, i))
                .map(|(_, &u)| u);
            if let Some(unit) = pick
                && world.player(me).credits >= rules.def(unit).cost.credits
            {
                out.push(CommandKind::Train { producer: f.id, def: unit });
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

        // 5. Army.
        let army: Vec<_> = mine.iter().copied().filter(|e| rules.def(e.def).weapon().is_some()).collect();
        let home = cc.map(|c| c.pos).or_else(|| mine.first().map(|e| e.pos));
        if self.level == AiLevel::Easy {
            self.waves(world, &mine, &army, home, out);
        } else {
            self.command_army(world, &mine, &army, home, out);
        }
    }

    /// `Easy`: defend with the units at home, attack in waves of five. Only visible, armed
    /// intruders call the defence: a passing harvester or dozer is no threat, and idle guards
    /// shoot it anyway if it comes within range.
    fn waves(&self, world: &World, mine: &[&Entity], army: &[&Entity], home: Option<Vec2>, out: &mut Vec<CommandKind>) {
        let (me, rules) = (self.player, world.rules);
        let vision = world.vision(me);
        let intruder = world.entities().find(|e| {
            e.owner.is_some_and(|o| o != me)
                && rules.def(e.def).weapon().is_some()
                && vision.is_visible(e.pos)
                && mine.iter().any(|b| rules.def(b.def).is_building() && b.pos.distance(e.pos) <= DEFEND_RADIUS)
        });
        if let Some(intruder) = intruder {
            // Only units at home defend: a wave already under way keeps going, and a unit
            // already fighting keeps its target.
            let home: Vec<EntityId> = army
                .iter()
                .filter(|e| {
                    !matches!(e.order, Order::Attack { .. } | Order::AttackMove { engaged: Some(_), .. })
                        && mine.iter().any(|b| rules.def(b.def).is_building() && b.pos.distance(e.pos) <= DEFEND_RADIUS)
                })
                .map(|e| e.id)
                .collect();
            if !home.is_empty() {
                out.push(CommandKind::Attack { units: home, target: intruder.id });
                // Those units are busy this second; the wave check waits for the next.
                return;
            }
        }
        let idle: Vec<EntityId> = army.iter().filter(|e| e.order == Order::Idle).map(|e| e.id).collect();
        if idle.len() >= WAVE
            && let Some(home) = home
            && let Some(target) = self.enemy_base(world, home)
        {
            out.push(CommandKind::AttackMove { units: idle, target });
        }
    }

    /// `Normal`: weigh the fight before every move. Strength is `strength` summed; the army
    /// attacks once it is half again as strong as the enemy army seen lately (and at least a
    /// wave), a unit whose own fight goes worse than one to two falls back with the units
    /// near it to regroup, and a base attacked by more than its guards can hold calls the
    /// field army home.
    fn command_army(
        &mut self,
        world: &World,
        mine: &[&Entity],
        army: &[&Entity],
        home: Option<Vec2>,
        out: &mut Vec<CommandKind>,
    ) {
        let (me, rules) = (self.player, world.rules);
        let Some(home) = home else { return };
        let vision = world.vision(me);
        let enemies: Vec<&Entity> = world
            .entities()
            .filter(|e| e.owner.is_some_and(|o| o != me) && e.hp > 0 && vision.is_visible(e.pos))
            .collect();
        let sum = |units: &mut dyn Iterator<Item = &&Entity>| units.map(|e| strength(world, e)).sum::<i64>();
        let seen_now = sum(&mut enemies.iter());
        // Slow fade (a 64th per second): an army that walked out of sight still exists.
        self.enemy_seen = (self.enemy_seen - self.enemy_seen / 64).max(seen_now);
        let buildings: Vec<&&Entity> = mine.iter().filter(|e| rules.def(e.def).is_building()).collect();
        let near_base = |p: Vec2| buildings.iter().any(|b| b.pos.distance(p) <= DEFEND_RADIUS);
        let at_home = |e: &&Entity| near_base(e.pos);

        // Constructors: away from armed enemies, back to the command center.
        for c in mine.iter().filter(|e| rules.def(e.def).is_constructor()) {
            let threatened = enemies.iter().any(|e| strength(world, e) > 0 && e.pos.distance(c.pos) <= DANGER_RADIUS);
            if threatened && c.pos.distance(home) > Fx::from_int(20) && !matches!(c.order, Order::Move { .. }) {
                out.push(CommandKind::Move { units: vec![c.id], target: home });
            }
        }

        // Defence: armed intruders near the base. Guards at home fight them; if the intruders
        // outweigh the guards, every wave comes home too.
        let intruders: Vec<&&Entity> = enemies.iter().filter(|e| strength(world, e) > 0 && near_base(e.pos)).collect();
        if let Some(first) = intruders.iter().min_by_key(|e| (e.pos.distance(home), e.id)) {
            let threat = sum(&mut intruders.iter().copied());
            let guards: Vec<&&Entity> = army.iter().filter(|e| at_home(e)).collect();
            let free: Vec<EntityId> = guards
                .iter()
                .filter(|e| !matches!(e.order, Order::Attack { .. } | Order::AttackMove { engaged: Some(_), .. }))
                .map(|e| e.id)
                .collect();
            if !free.is_empty() {
                out.push(CommandKind::AttackMove { units: free, target: first.pos });
            }
            if sum(&mut guards.iter().copied()) < threat {
                let away: Vec<EntityId> = army.iter().filter(|e| !at_home(e)).map(|e| e.id).collect();
                if !away.is_empty() {
                    out.push(CommandKind::AttackMove { units: away, target: first.pos });
                }
            }
            return;
        }

        // Units in the field: each one in a fight judges the fight around itself, so two waves
        // in different places don't average out. A losing group falls back to regroup; units
        // already on their way home are left alone, so a retreat is issued once.
        let out_there: Vec<&&Entity> = army.iter().filter(|e| !at_home(e)).collect();
        let retreating = |e: &&Entity| matches!(e.order, Order::Move { target, .. } if target == home);
        let mut fleeing: Vec<EntityId> = Vec::new();
        for e in out_there.iter().filter(|e| !retreating(e)) {
            let near = |p: Vec2| p.distance(e.pos) <= BATTLE_RADIUS;
            let theirs = sum(&mut enemies.iter().filter(|x| near(x.pos)));
            if theirs == 0 {
                continue;
            }
            let ours = sum(&mut out_there.iter().copied().filter(|x| near(x.pos)));
            if ours * 2 < theirs {
                fleeing.push(e.id);
            }
        }
        if !fleeing.is_empty() {
            out.push(CommandKind::Move { units: fleeing, target: home });
            self.regroup_until = world.tick() + 20 * TICKS_PER_SECOND;
        }

        // New wave: every idle armed unit, at home or left in the field by an earlier wave,
        // once it outweighs the enemy army seen lately by half and the army isn't regrouping.
        if world.tick() < self.regroup_until {
            return;
        }
        let idle: Vec<&&Entity> = army.iter().filter(|e| e.order == Order::Idle).collect();
        let ready = sum(&mut idle.iter().copied());
        if idle.len() >= WAVE
            && ready * 2 >= self.enemy_seen * 3
            && let Some(target) = self.enemy_base(world, home)
        {
            out.push(CommandKind::AttackMove { units: idle.iter().map(|e| e.id).collect(), target });
        }
    }

    /// Spiral search outward from `around` for the first valid foundation spot.
    fn find_spot(&self, world: &World, def: crate::defs::DefId, around: Vec2) -> Option<Vec2> {
        let step = world.rules.def(def).radius * Fx::from_int(2) + Fx::from_int(4);
        for ring in 1..12i64 {
            for k in 0..8 * ring {
                let pos = spiral_cell(world, around, step, ring, k);
                if world.check_placement(def, pos).is_ok() {
                    return Some(pos);
                }
            }
        }
        None
    }

    /// The valid foundation spot nearest `toward` among the spiral's first rings around
    /// `around`; ties go to the earlier ring cell.
    fn find_spot_toward(&self, world: &World, def: crate::defs::DefId, around: Vec2, toward: Vec2) -> Option<Vec2> {
        let step = world.rules.def(def).radius * Fx::from_int(2) + Fx::from_int(4);
        let mut best: Option<(Fx, Vec2)> = None;
        for ring in 1..4i64 {
            for k in 0..8 * ring {
                let pos = spiral_cell(world, around, step, ring, k);
                if world.check_placement(def, pos).is_ok() && best.is_none_or(|(d, _)| pos.distance(toward) < d) {
                    best = Some((pos.distance(toward), pos));
                }
            }
        }
        best.map(|(_, p)| p).or_else(|| self.find_spot(world, def, around))
    }

    /// Where an attack wave heads: just outside the nearest enemy building this AI remembers,
    /// on the side facing home (not its centre, a blocked cell no route can end on). With none
    /// remembered, it scouts the nearest place it can't see: start positions, oil wells and
    /// supply piles first (the map is public; enemies build there), then any cell it has
    /// never explored, so a hidden last building is always found. Units engage whatever they
    /// meet once they are close anyway.
    fn enemy_base(&self, world: &World, home: Vec2) -> Option<Vec2> {
        let vision = world.vision(self.player);
        let seen = vision
            .memory
            .iter()
            .min_by_key(|(id, s)| (s.pos.distance(home), **id))
            .map(|(_, s)| (s.pos, world.rules.def(s.def).radius));
        let scout = || {
            let resources = world
                .entities()
                .filter(|e| matches!(world.rules.def(e.def).class, Class::OilWell | Class::SupplyPile))
                .map(|e| e.pos);
            let nearest = |points: &mut dyn Iterator<Item = Vec2>| points.min_by_key(|&p| (p.distance(home), p.x, p.y));
            let likely: Vec<Vec2> = world.map.spawns.iter().copied().chain(resources).collect();
            nearest(&mut likely.iter().copied().filter(|&p| !vision.is_explored(p)))
                .or_else(|| nearest(&mut vision.unexplored_centres()))
                .or_else(|| nearest(&mut likely.iter().copied().filter(|&p| !vision.is_visible(p))))
                .map(|p| (p, Fx::from_int(5)))
        };
        let (pos, radius) = seen.or_else(scout)?;
        let delta = home - pos;
        let dist = delta.length();
        if dist == Fx::ZERO {
            return Some(pos);
        }
        let stand_off = radius + Fx::from_int(8);
        Some(pos + delta.scale(stand_off.min(dist) / dist))
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

/// The `k`-th spiral cell around `around`, `step` apart, with both axes flipped to point at the
/// map centre. Starts that mirror each other through the centre (as on every 2-player map)
/// then scan mirrored cells in the same order and build mirrored bases.
fn spiral_cell(world: &World, around: Vec2, step: Fx, ring: i64, k: i64) -> Vec2 {
    let toward = |v: Fx, size: Fx| if v * Fx::from_int(2) > size { -1 } else { 1 };
    let (fx, fy) = (toward(around.x, world.map.width), toward(around.y, world.map.height));
    let (dx, dy) = ring_offset(ring, k);
    around + Vec2::new(step * Fx::from_int(dx * fx), step * Fx::from_int(dy * fy))
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
