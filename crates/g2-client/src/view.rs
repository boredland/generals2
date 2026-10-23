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

#[derive(Component)]
struct Rotor;

/// A short-lived combat effect: a tracer from shooter to target or the flash of a kill.
#[derive(Component)]
struct Effect {
    age: f32,
    life: f32,
    /// Grows from 1 to this many times its spawn scale over its life (kill flashes).
    grow: f32,
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
    mut bodies: Query<&mut Transform, (With<Body>, Without<SimRef>, Without<Scaffold>)>,
    mut scaffolds: Query<&mut Visibility, With<Scaffold>>,
    mut root_tf: Query<&mut Transform, (With<SimRef>, Without<Body>)>,
) {
    let world = &sim.world;
    mirror.0.retain(|id, &mut e| {
        let alive = world.entity(*id).is_some();
        if !alive {
            commands.entity(e).despawn();
        }
        alive
    });
    for e in world.entities() {
        let def = world.rules.def(e.def);
        let root = *mirror.0.entry(e.id).or_insert_with(|| spawn_entity(&mut commands, &assets, &sim, e));
        if !def.is_unit()
            && let Ok(mut tf) = root_tf.get_mut(root)
        {
            tf.translation = sim.to_world(e.pos);
        }
        let Ok(children) = roots.get(root) else { continue };
        // Foundations: the finished model rises inside the scaffold; supply piles shrink as they deplete.
        let rise = match (def.class, e.construction, e.owner) {
            (_, Some(done), Some(owner)) => (done as f32 / world.build_ticks_for(owner, e.def) as f32).clamp(0.04, 1.0),
            (Class::SupplyPile, _, _) => (e.resource as f32 / 6000.0).clamp(0.3, 1.4),
            _ => 1.0,
        };
        for &child in children {
            if let Ok(mut tf) = bodies.get_mut(child) {
                tf.scale = Vec3::new(1.0, rise, 1.0);
            }
            if let Ok(mut v) = scaffolds.get_mut(child) {
                *v = if e.construction.is_some() { Visibility::Inherited } else { Visibility::Hidden };
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

/// A tracer for every hit (a thin glowing bar from muzzle height at the shooter to the
/// target), plus an expanding fireball where a hit destroyed its target.
fn spawn_effects(mut commands: Commands, assets: Res<Assets3d>, sim: Res<Sim>, mut hits: ResMut<crate::sim::Hits>) {
    for (from, to, killed) in hits.0.drain(..) {
        let (a, b) = (sim.to_world(from) + Vec3::Y * 1.5, sim.to_world(to) + Vec3::Y * 1.5);
        let len = a.distance(b);
        if len > 0.1 {
            commands.spawn((
                Mesh3d(assets.cube.clone()),
                MeshMaterial3d(assets.tracer.clone()),
                Transform::from_translation((a + b) / 2.0)
                    .looking_at(b, Vec3::Y)
                    .with_scale(Vec3::new(0.25, 0.25, len)),
                Effect { age: 0.0, life: TRACER_SECS, grow: 1.0 },
            ));
        }
        if killed {
            commands.spawn((
                Mesh3d(assets.sphere.clone()),
                MeshMaterial3d(assets.blast.clone()),
                Transform::from_translation(b).with_scale(Vec3::splat(2.5)),
                Effect { age: 0.0, life: TRACER_SECS * 3.0, grow: 3.0 },
            ));
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
        let now = 1.0 + (fx.grow - 1.0) * (fx.age / fx.life);
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
