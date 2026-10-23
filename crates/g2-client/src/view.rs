//! Mirrors sim entities as placeholder 3D primitives. All art is original and procedural:
//! no game assets are loaded.

use std::collections::BTreeMap;

use bevy::prelude::*;
use g2_sim::{BuildingRole, Class, EntityId, Faction, PlayerId};

use crate::sim::{Previous, Sim, to_world};

/// Links a render entity to its sim entity.
#[derive(Component)]
pub struct SimRef(pub EntityId);

#[derive(Resource, Default)]
pub struct Mirror(pub BTreeMap<EntityId, Entity>);

#[derive(Resource)]
struct Palette {
    ground: Handle<StandardMaterial>,
    faction: [Handle<StandardMaterial>; 3],
    faction_ghost: [Handle<StandardMaterial>; 3],
    supply: Handle<StandardMaterial>,
    oil: Handle<StandardMaterial>,
}

pub struct ViewPlugin;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Mirror>()
            .add_systems(Startup, setup_scene)
            .add_systems(Update, (sync_entities, interpolate).chain());
    }
}

pub fn faction_color(f: Faction) -> Color {
    match f {
        Faction::Eu => Color::srgb(0.25, 0.45, 0.85),
        Faction::Apa => Color::srgb(0.85, 0.25, 0.2),
        Faction::Gla => Color::srgb(0.75, 0.65, 0.3),
    }
}

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sim: Res<Sim>,
) {
    let mat = |materials: &mut Assets<StandardMaterial>, c: Color| {
        materials.add(StandardMaterial { base_color: c, perceptual_roughness: 0.9, ..default() })
    };
    let ghost = |materials: &mut Assets<StandardMaterial>, f: Faction| {
        materials.add(StandardMaterial {
            base_color: faction_color(f).with_alpha(0.45),
            alpha_mode: AlphaMode::Blend,
            ..default()
        })
    };
    let palette = Palette {
        ground: mat(&mut materials, Color::srgb(0.36, 0.33, 0.26)),
        faction: Faction::ALL.map(|f| mat(&mut materials, faction_color(f))),
        faction_ghost: Faction::ALL.map(|f| ghost(&mut materials, f)),
        supply: mat(&mut materials, Color::srgb(0.55, 0.42, 0.22)),
        oil: mat(&mut materials, Color::srgb(0.06, 0.06, 0.07)),
    };
    let (w, h) = (sim.world.map.width.to_f32(), sim.world.map.height.to_f32());
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(w, h))),
        MeshMaterial3d(palette.ground.clone()),
        Transform::from_xyz(w / 2.0, 0.0, h / 2.0),
    ));
    commands.spawn((
        DirectionalLight { illuminance: 9_000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, 0.6, -0.9, 0.0)),
    ));
    commands.insert_resource(GlobalAmbientLight { brightness: 350.0, ..default() });
    commands.insert_resource(palette);
}

fn faction_of(sim: &Sim, owner: Option<PlayerId>) -> Option<Faction> {
    owner.map(|o| sim.world.player(o).faction)
}

/// Mesh and vertical offset for a def. Sizes derive from the sim radius so what you see is what
/// collides.
fn shape(def: &g2_sim::Def) -> (Mesh, f32) {
    let r = def.radius.to_f32();
    match def.class {
        Class::SupplyPile => (Cuboid::new(r * 1.6, r * 0.6, r * 1.6).into(), r * 0.3),
        Class::OilWell => (Cylinder::new(r, 0.3).into(), 0.15),
        Class::Unit { constructor: true, .. } => (Cuboid::new(r * 1.6, r, r * 2.0).into(), r * 0.5),
        Class::Unit { collector: Some(_), .. } => (Cylinder::new(r, r * 1.2).into(), r * 0.6),
        Class::Unit { .. } => (Capsule3d::new(r * 0.5, r).into(), r),
        Class::Building { role, .. } => {
            let height = match role {
                BuildingRole::CommandCenter => 9.0,
                BuildingRole::SupplyCenter { .. } => 5.0,
                BuildingRole::PowerPlant => 7.0,
                BuildingRole::OilDerrick { .. } => 10.0,
                BuildingRole::Factory => 6.0,
            };
            match role {
                BuildingRole::OilDerrick { .. } => (Cylinder::new(r * 0.6, height).into(), height / 2.0),
                _ => (Cuboid::new(r * 1.7, height, r * 1.7).into(), height / 2.0),
            }
        }
    }
}

fn sync_entities(
    mut commands: Commands,
    mut mirror: ResMut<Mirror>,
    mut meshes: ResMut<Assets<Mesh>>,
    palette: Res<Palette>,
    sim: Res<Sim>,
    mut q: Query<(&mut Transform, &mut MeshMaterial3d<StandardMaterial>), With<SimRef>>,
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
        let material = match (def.class, faction_of(&sim, e.owner)) {
            (Class::SupplyPile, _) => palette.supply.clone(),
            (Class::OilWell, _) => palette.oil.clone(),
            (_, Some(f)) if e.construction.is_some() => palette.faction_ghost[f as usize].clone(),
            (_, Some(f)) => palette.faction[f as usize].clone(),
            (_, None) => palette.supply.clone(),
        };
        let (_, lift) = shape(def);
        // Supply piles shrink as they deplete; foundations rise as construction progresses.
        let scale = match (def.class, e.construction) {
            (Class::SupplyPile, _) => Vec3::new(1.0, (e.resource as f32 / 6000.0).clamp(0.25, 1.6), 1.0),
            (_, Some(done)) => Vec3::new(1.0, (done as f32 / def.build_ticks.max(1) as f32).max(0.05), 1.0),
            _ => Vec3::ONE,
        };
        match mirror.0.get(&e.id) {
            Some(&r) => {
                if let Ok((mut tf, mut m)) = q.get_mut(r) {
                    tf.scale = scale;
                    if !def.is_unit() {
                        tf.translation = to_world(e.pos) + Vec3::Y * lift * scale.y;
                    }
                    if m.0 != material {
                        m.0 = material;
                    }
                }
            }
            None => {
                let (mesh, _) = shape(def);
                let r = commands
                    .spawn((
                        SimRef(e.id),
                        Mesh3d(meshes.add(mesh)),
                        MeshMaterial3d(material),
                        Transform::from_translation(to_world(e.pos) + Vec3::Y * lift * scale.y).with_scale(scale),
                    ))
                    .id();
                mirror.0.insert(e.id, r);
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
        let def = sim.world.rules.def(e.def);
        if !def.is_unit() {
            continue;
        }
        let now = to_world(e.pos);
        let before = prev.0.get(&r.0).map_or(now, |&p| to_world(p));
        let pos = before.lerp(now, alpha);
        let (_, lift) = shape(def);
        tf.translation = pos + Vec3::Y * lift;
        let heading = now - before;
        if heading.length_squared() > 1e-6 {
            tf.rotation = Quat::from_rotation_y(heading.x.atan2(heading.z));
        }
    }
}
