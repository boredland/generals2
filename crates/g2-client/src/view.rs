//! Mirrors sim entities as procedural models (`models.rs`). All art is original: no game assets.
//!
//! Each sim entity gets one root (`SimRef` + `Transform` at its ground position) with the model's
//! parts as children. Foundations show the scaffold with the building rising inside it.
//! Overlays (collector counts, health bars) are drawn by `hud.rs`; `overlay_anchor` and
//! `depot_label` supply their position and data.

use std::collections::BTreeMap;

use bevy::prelude::*;
use g2_sim::{BuildingRole, Class, EntityId, Faction};

use crate::models::{self, Part, Prim, Slot};
use crate::sim::{LOCAL, Previous, Sim, to_world};

/// Links a render entity to its sim entity.
#[derive(Component)]
pub struct SimRef(pub EntityId);

/// The finished model under a root; scaled vertically while under construction.
#[derive(Component)]
struct Body;

/// Scaffold shown while the building is a foundation.
#[derive(Component)]
struct Scaffold;

/// A deployable unit's stabiliser legs: they slide out while it sets up (scaled across by the
/// deploy progress) and are hidden while it is packed.
#[derive(Component)]
struct Outriggers;

#[derive(Component)]
struct Rotor;

/// A short-lived combat effect: a tracer from shooter to target, the flash of a kill or a
/// blast, or a shell in the air.
#[derive(Component)]
struct Effect {
    age: f32,
    life: f32,
    /// Grows from 1 to this many times its spawn scale over its life (kill flashes).
    grow: f32,
    /// A shell: it flies from `.0` to `.1` on an arc this high at its peak, over `life`.
    arc: Option<(Vec3, Vec3, f32)>,
}

/// Seconds a tracer stays visible; kill flashes last three times as long.
const TRACER_SECS: f32 = 0.12;

/// Present in screenshot mode: effects keep their spawn state so the captured frame shows them.
#[derive(Resource)]
pub struct FreezeEffects;

#[derive(Resource, Default)]
pub struct Mirror(pub BTreeMap<EntityId, Entity>);

#[derive(Resource)]
struct Assets3d {
    cube: Handle<Mesh>,
    cylinder: Handle<Mesh>,
    sphere: Handle<Mesh>,
    materials: BTreeMap<(Option<Faction>, SlotKey), Handle<StandardMaterial>>,
    /// Unlit, additive-looking muzzle/tracer and explosion colours.
    tracer: Handle<StandardMaterial>,
    blast: Handle<StandardMaterial>,
}

/// `Slot` with crate colours folded to an index, so it can key a map.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SlotKey(u8);

impl From<Slot> for SlotKey {
    fn from(s: Slot) -> SlotKey {
        SlotKey(match s {
            Slot::Hull => 0,
            Slot::Panel => 1,
            Slot::Dark => 2,
            Slot::Team => 3,
            Slot::Hazard => 4,
            Slot::Concrete => 5,
            Slot::Glow => 6,
            Slot::Oil => 7,
            Slot::Wood => 8,
            Slot::Crate(i) => 16 + i % 5,
        })
    }
}

pub struct ViewPlugin;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Mirror>()
            .add_systems(Startup, setup_scene)
            .add_systems(Update, (sync_entities, interpolate, spin_rotors, spawn_effects, age_effects).chain());
    }
}

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sim: Res<Sim>,
) {
    let mut table = BTreeMap::new();
    let all_slots = [
        Slot::Hull,
        Slot::Panel,
        Slot::Dark,
        Slot::Team,
        Slot::Hazard,
        Slot::Concrete,
        Slot::Glow,
        Slot::Oil,
        Slot::Wood,
    ]
    .into_iter()
    .chain((0..5).map(Slot::Crate));
    let factions = [None, Some(Faction::Eu), Some(Faction::Apa), Some(Faction::Gla)];
    for slot in all_slots {
        for f in factions {
            let base_color = models::color(f, slot);
            let (roughness, metallic, emissive) = match slot {
                Slot::Glow => (0.3, 0.0, base_color.to_linear() * 1.5),
                Slot::Oil => (0.08, 0.0, LinearRgba::BLACK),
                Slot::Hull | Slot::Panel | Slot::Dark => (0.55, 0.35, LinearRgba::BLACK),
                _ => (0.8, 0.0, LinearRgba::BLACK),
            };
            let m = materials.add(StandardMaterial {
                base_color,
                perceptual_roughness: roughness,
                metallic,
                emissive,
                ..default()
            });
            table.insert((f, SlotKey::from(slot)), m);
        }
    }
    let glow = |c: Color, k: f32| StandardMaterial {
        base_color: c,
        emissive: c.to_linear() * k,
        unlit: true,
        alpha_mode: AlphaMode::Add,
        ..default()
    };
    commands.insert_resource(Assets3d {
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        cylinder: meshes.add(Cylinder::new(0.5, 1.0)),
        sphere: meshes.add(Sphere::new(0.5).mesh().ico(2).unwrap()),
        materials: table,
        tracer: materials.add(glow(Color::srgb(1.0, 0.85, 0.45), 4.0)),
        blast: materials.add(glow(Color::srgb(1.0, 0.5, 0.15), 3.0)),
    });
    let size = sim.world.map.width.to_f32().max(sim.world.map.height.to_f32());
    crate::look::spawn_lights(&mut commands, size);
}

fn spawn_parts(parent: &mut ChildSpawnerCommands, assets: &Assets3d, faction: Option<Faction>, parts: &[Part]) {
    for p in parts {
        let mesh = match p.prim {
            Prim::Box => assets.cube.clone(),
            Prim::Cylinder => assets.cylinder.clone(),
            Prim::Sphere => assets.sphere.clone(),
        };
        let material = assets.materials[&(faction, SlotKey::from(p.slot))].clone();
        let mut e = parent.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(p.at).with_rotation(Quat::from_rotation_y(p.yaw)).with_scale(p.size),
        ));
        if p.spin {
            e.insert(Rotor);
        }
    }
}

fn spawn_entity(commands: &mut Commands, assets: &Assets3d, sim: &Sim, e: &g2_sim::Entity) -> Entity {
    let def = sim.world.rules.def(e.def);
    let faction = e.owner.map(|o| sim.world.player(o).faction);
    let parts = models::parts_for(def, faction);
    let mut root =
        commands.spawn((SimRef(e.id), Transform::from_translation(sim.to_world(e.pos)), Visibility::default()));
    root.with_children(|c| {
        c.spawn((Body, Transform::default(), Visibility::default()))
            .with_children(|b| spawn_parts(b, assets, faction, &parts));
        if def.is_building() {
            let w = def.radius.to_f32() * 1.75;
            let scaffold: Vec<Part> = models::scaffold(w, 7.0);
            c.spawn((Scaffold, Transform::default(), Visibility::Hidden))
                .with_children(|s| spawn_parts(s, assets, faction, &scaffold));
        }
        if def.deploy.is_some() {
            let legs = models::outriggers(def.radius.to_f32());
            c.spawn((Outriggers, Transform::default(), Visibility::Hidden))
                .with_children(|s| spawn_parts(s, assets, faction, &legs));
        }
    });
    root.id()
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn sync_entities(
    mut commands: Commands,
    mut mirror: ResMut<Mirror>,
    assets: Res<Assets3d>,
    sim: Res<Sim>,
    roots: Query<&Children, With<SimRef>>,
    mut bodies: Query<&mut Transform, (With<Body>, Without<SimRef>, Without<Scaffold>, Without<Outriggers>)>,
    mut scaffolds: Query<&mut Visibility, (With<Scaffold>, Without<SimRef>, Without<Outriggers>)>,
    mut legs: Query<(&mut Transform, &mut Visibility), (With<Outriggers>, Without<SimRef>, Without<Body>)>,
    mut root_tf: Query<
        (&mut Transform, &mut Visibility),
        (With<SimRef>, Without<Body>, Without<Scaffold>, Without<Outriggers>),
    >,
) {
    let world = &sim.world;
    // A model goes when its entity does, except a remembered building destroyed out of sight:
    // it stays as a ghost until the player sees its spot again (and the sim forgets it).
    mirror.0.retain(|id, &mut e| {
        let keep = world.entity(*id).is_some() || sim.remembers(*id);
        if !keep {
            commands.entity(e).despawn();
        }
        keep
    });
    for e in world.entities() {
        let def = world.rules.def(e.def);
        let root = *mirror.0.entry(e.id).or_insert_with(|| spawn_entity(&mut commands, &assets, &sim, e));
        if let Ok((mut tf, mut vis)) = root_tf.get_mut(root) {
            if !def.is_unit() {
                tf.translation = sim.to_world(e.pos);
            }
            vis.set_if_neq(if sim.shows(e) { Visibility::Inherited } else { Visibility::Hidden });
        }
        // A remembered building out of sight keeps the look it had when last seen.
        if !sim.sees(e) {
            continue;
        }
        let Ok(children) = roots.get(root) else { continue };
        // Foundations: the finished model rises inside the scaffold; supply piles shrink as they deplete.
        let rise = match (def.class, e.construction, e.owner) {
            (_, Some(done), Some(owner)) => (done as f32 / world.build_ticks_for(owner, e.def) as f32).clamp(0.04, 1.0),
            (Class::SupplyPile, _, _) => (e.resource as f32 / 6000.0).clamp(0.3, 1.4),
            _ => 1.0,
        };
        // Outriggers: 0 packed, 1 deployed, in between while setting up or packing up.
        let out = match (e.deploy, def.deploy) {
            (g2_sim::entity::DeployState::Deployed, _) => 1.0,
            (g2_sim::entity::DeployState::Deploying(t), Some(d)) => 1.0 - t as f32 / d.ticks.max(1) as f32,
            (g2_sim::entity::DeployState::Packing(t), Some(d)) => t as f32 / d.ticks.max(1) as f32,
            _ => 0.0,
        };
        for &child in children {
            if let Ok(mut tf) = bodies.get_mut(child) {
                tf.scale = Vec3::new(1.0, rise, 1.0);
            }
            if let Ok(mut v) = scaffolds.get_mut(child) {
                *v = if e.construction.is_some() { Visibility::Inherited } else { Visibility::Hidden };
            }
            if let Ok((mut tf, mut v)) = legs.get_mut(child) {
                tf.scale = Vec3::new(0.4 + 0.6 * out, 1.0, 1.0);
                v.set_if_neq(if out > 0.0 { Visibility::Inherited } else { Visibility::Hidden });
            }
        }
    }
}

/// Units are drawn between their previous and current sim positions, so motion is smooth at any
/// display rate even though the sim runs at 30 Hz.
fn interpolate(sim: Res<Sim>, prev: Res<Previous>, fixed: Res<Time<Fixed>>, mut q: Query<(&SimRef, &mut Transform)>) {
    let alpha = if sim.speed == 0 { 1.0 } else { fixed.overstep_fraction() };
    for (r, mut tf) in &mut q {
        let Some(e) = sim.world.entity(r.0) else { continue };
        if !sim.world.rules.def(e.def).is_unit() {
            continue;
        }
        // Lerp on the flat sim plane, then sample the ground once, so a unit follows the
        // terrain between ticks instead of cutting through it.
        let now = to_world(e.pos);
        let before = prev.0.get(&r.0).map_or(now, |&p| to_world(p));
        let mut at = before.lerp(now, alpha);
        at.y = sim.ground_y(at.x, at.z);
        tf.translation = at;
        let heading = now - before;
        if heading.length_squared() > 1e-6 {
            tf.rotation = Quat::from_rotation_y(heading.x.atan2(heading.z));
        }
    }
}

fn spin_rotors(time: Res<Time>, mut q: Query<&mut Transform, With<Rotor>>) {
    for mut tf in &mut q {
        tf.rotate_y(time.delta_secs() * 18.0);
    }
}

/// A tracer for every direct hit (a thin glowing bar from muzzle height at the shooter to the
/// target), an expanding fireball where a hit destroyed its target, a shell flying on an arc
/// for its flight time, and a blast the size of its radius where it lands. Under fog, only
/// what has an end the local player sees.
fn spawn_effects(mut commands: Commands, assets: Res<Assets3d>, sim: Res<Sim>, mut hits: ResMut<crate::sim::Hits>) {
    use crate::sim::Shot;
    let vision = sim.world.vision(crate::sim::LOCAL);
    let seen = |p: g2_sim::Vec2| !sim.fog || vision.is_visible(p);
    let tick_secs = 1.0 / g2_sim::TICKS_PER_SECOND as f32;
    for shot in hits.0.drain(..) {
        match shot {
            Shot::Hit { from, at, killed, tracer } => {
                if !seen(from) && !seen(at) {
                    continue;
                }
                let (a, b) = (sim.to_world(from) + Vec3::Y * 1.5, sim.to_world(at) + Vec3::Y * 1.5);
                let len = a.distance(b);
                if tracer && len > 0.1 {
                    commands.spawn((
                        Mesh3d(assets.cube.clone()),
                        MeshMaterial3d(assets.tracer.clone()),
                        Transform::from_translation((a + b) / 2.0)
                            .looking_at(b, Vec3::Y)
                            .with_scale(Vec3::new(0.25, 0.25, len)),
                        Effect { age: 0.0, life: TRACER_SECS, grow: 1.0, arc: None },
                    ));
                }
                if killed {
                    commands.spawn((
                        Mesh3d(assets.sphere.clone()),
                        MeshMaterial3d(assets.blast.clone()),
                        Transform::from_translation(b).with_scale(Vec3::splat(2.5)),
                        Effect { age: 0.0, life: TRACER_SECS * 3.0, grow: 3.0, arc: None },
                    ));
                }
            }
            Shot::Shell { from, at, flight } => {
                if !seen(from) && !seen(at) {
                    continue;
                }
                let (a, b) = (sim.to_world(from) + Vec3::Y * 2.5, sim.to_world(at) + Vec3::Y * 0.5);
                let peak = (a.distance(b) * 0.35).max(4.0);
                commands.spawn((
                    Mesh3d(assets.sphere.clone()),
                    MeshMaterial3d(assets.tracer.clone()),
                    Transform::from_translation(a).with_scale(Vec3::splat(0.9)),
                    Effect { age: 0.0, life: flight as f32 * tick_secs, grow: 1.0, arc: Some((a, b, peak)) },
                ));
            }
            Shot::Blast { at, radius } => {
                if !seen(at) {
                    continue;
                }
                let r = radius.to_f32();
                commands.spawn((
                    Mesh3d(assets.sphere.clone()),
                    MeshMaterial3d(assets.blast.clone()),
                    Transform::from_translation(sim.to_world(at) + Vec3::Y * 0.5).with_scale(Vec3::new(r, r * 0.5, r)),
                    Effect { age: 0.0, life: TRACER_SECS * 4.0, grow: 1.4, arc: None },
                ));
            }
        }
    }
}

fn age_effects(
    mut commands: Commands,
    time: Res<Time>,
    frozen: Option<Res<FreezeEffects>>,
    mut q: Query<(Entity, &mut Effect, &mut Transform)>,
) {
    if frozen.is_some() {
        return;
    }
    for (id, mut fx, mut tf) in &mut q {
        let before = 1.0 + (fx.grow - 1.0) * (fx.age / fx.life);
        fx.age += time.delta_secs();
        if fx.age >= fx.life {
            commands.entity(id).despawn();
            continue;
        }
        let t = fx.age / fx.life;
        if let Some((a, b, peak)) = fx.arc {
            tf.translation = a.lerp(b, t) + Vec3::Y * peak * 4.0 * t * (1.0 - t);
        }
        let now = 1.0 + (fx.grow - 1.0) * t;
        tf.scale *= now / before;
    }
}

/// World-space anchor for overlays above an entity.
pub fn overlay_anchor(sim: &Sim, e: &g2_sim::Entity) -> Vec3 {
    let def = sim.world.rules.def(e.def);
    let height = match def.class {
        Class::Building { role: BuildingRole::CommandCenter, .. } => 12.0,
        Class::Building { .. } => 8.0,
        Class::Unit { collector: Some(_), .. }
            if e.owner.is_some_and(|o| sim.world.player(o).faction == Faction::Eu) =>
        {
            10.0
        }
        _ => 4.0,
    };
    sim.to_world(e.pos) + Vec3::Y * height
}

/// Whether the local player should see a collector-count label on this entity.
pub fn depot_label(sim: &Sim, e: &g2_sim::Entity) -> Option<(u32, u32)> {
    match sim.world.rules.def(e.def).role() {
        Some(BuildingRole::SupplyCenter { collector_cap }) if e.owner == Some(LOCAL) && e.is_complete() => {
            Some((sim.world.depot_load(e.id), collector_cap))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use g2_sim::{CommandKind, PlayerId, PlayerSetup, Vec2};

    /// A remembered enemy building destroyed out of sight keeps its model (a ghost), so the
    /// player doesn't learn of the kill; once its spot is seen again the ghost goes.
    #[test]
    fn a_building_destroyed_out_of_sight_stays_as_a_ghost_until_seen() {
        let players = Sim::players([Faction::Eu, Faction::Gla], false).map(|p| PlayerSetup { ai: false, ..p });
        let sim = Sim::new(g2_rules::crater_ridge(), None, 1, players);
        let enemy = PlayerId(1);
        let rules = sim.world.rules;
        let first = |s: &Sim, owner, pred: fn(&g2_sim::Def) -> bool| {
            s.world.entities().find(|e| e.owner == Some(owner) && pred(rules.def(e.def))).unwrap().id
        };
        let dozer = first(&sim, LOCAL, |d| d.is_constructor());
        let cc = first(&sim, enemy, |d| d.is_building());
        let (home, cc_pos) = (sim.world.entity(dozer).unwrap().pos, sim.world.entity(cc).unwrap().pos);
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .insert_resource(sim)
            .init_resource::<Previous>()
            .init_resource::<crate::sim::Hits>()
            .add_plugins(ViewPlugin);
        let step_until = |app: &mut App, done: &dyn Fn(&Sim) -> bool, what: &str| {
            for _ in 0..30 * 120 {
                if done(app.world().resource::<Sim>()) {
                    return;
                }
                app.world_mut().resource_mut::<Sim>().world.step();
            }
            panic!("timed out: {what}");
        };
        let order = |app: &mut App, target: Vec2| {
            app.world_mut().resource_mut::<Sim>().order(CommandKind::Move { units: vec![dozer], target });
        };
        // Scout the enemy command center, then drive home until it is out of sight.
        order(&mut app, cc_pos - Vec2::from_int(14, 14));
        step_until(&mut app, &|s| s.remembers(cc), "scouted");
        order(&mut app, home);
        step_until(&mut app, &|s| !s.sees(s.world.entity(cc).unwrap()), "out of sight");
        app.update();
        assert!(app.world().resource::<Mirror>().0.contains_key(&cc), "drawn while remembered");
        // The GLA surrenders: its buildings go at the next cleanup, where we can't see them.
        app.world_mut().resource_mut::<Sim>().world.submit(enemy, CommandKind::Surrender);
        step_until(&mut app, &|s| s.world.entity(cc).is_none(), "destroyed");
        app.update();
        assert!(app.world().resource::<Sim>().remembers(cc));
        assert!(app.world().resource::<Mirror>().0.contains_key(&cc), "a ghost stays");
        // Back in sight of the spot: the sim forgets it and the ghost goes.
        order(&mut app, cc_pos - Vec2::from_int(14, 14));
        step_until(&mut app, &|s| !s.remembers(cc), "seen again");
        app.update();
        assert!(!app.world().resource::<Mirror>().0.contains_key(&cc), "the ghost is gone");
    }
}
