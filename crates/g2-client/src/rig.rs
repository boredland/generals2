//! Moves the joints of skinned alpha models: a vehicle's `Turret` joint yaws toward what it
//! is attacking (back to straight ahead without a target), and its `wheel*` joints roll
//! with the distance it covers. Joint names are the alpha's skeletons' own; a model without
//! such joints (or the procedural models) is left alone.

use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;
use g2_sim::{EntityId, Order};

use crate::sim::{Sim, to_world};
use crate::view::SimRef;

pub struct RigPlugin;

impl Plugin for RigPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(tag_joints)
            .add_systems(Update, (update_aims, aim_turrets, roll_wheels).chain().after(crate::menu::MenuSet));
    }
}

/// A turret joint of the model drawn for sim entity `.0`, with its bind-pose rotation.
#[derive(Component)]
pub struct TurretJoint {
    owner: Entity,
    rest: Quat,
}

/// A wheel joint: its bind-pose rotation, its radius in world units (height of its centre
/// above the model's ground times the model's scale) and the angle it has rolled.
#[derive(Component)]
pub struct WheelJoint {
    owner: Entity,
    rest: Quat,
    radius: f32,
    rolled: f32,
}

/// Where a sim entity's root is aiming in world space; `None` faces the hull's front.
#[derive(Component, Default)]
pub struct Aim(pub Option<Vec3>);

/// Radians per second a turret turns at most, so it swings rather than snaps.
const TURRET_SPEED: f32 = 2.0;

/// Once an alpha model's joints exist, tags the ones this module moves.
fn tag_joints(ready: On<WorldInstanceReady>, mut commands: Commands) {
    let model = ready.entity;
    commands.queue(move |world: &mut World| tag_model(world, model));
}

/// Tags the turret and wheel joints under `model` with the sim entity's root, found by
/// walking up from the model.
fn tag_model(world: &mut World, model: Entity) {
    let mut owner = model;
    while world.get::<SimRef>(owner).is_none() {
        let Some(p) = world.get::<ChildOf>(owner) else { return };
        owner = p.parent();
    }
    let scale = world.get::<Transform>(model).map_or(1.0, |t| t.scale.y);
    let mut stack: Vec<Entity> = world.get::<Children>(model).map(|c| c.to_vec()).unwrap_or_default();
    let mut tags = Vec::new();
    while let Some(e) = stack.pop() {
        stack.extend(world.get::<Children>(e).map(|c| c.to_vec()).unwrap_or_default());
        let (Some(name), Some(tf)) = (world.get::<Name>(e), world.get::<Transform>(e)) else { continue };
        let n = name.as_str();
        if n == "Turret" {
            tags.push((e, None, tf.rotation));
        } else if n.starts_with("wheel") {
            // In model space a wheel's height above the model's base is its radius (the
            // alpha's origin is at ground level). Global transforms aren't settled the
            // frame the scene spawns, so the local chain gives it.
            let radius = model_space_height(world, e, model).max(0.05) * scale;
            tags.push((e, Some(radius), tf.rotation));
        }
    }
    for (e, radius, rest) in tags {
        match radius {
            None => world.entity_mut(e).insert(TurretJoint { owner, rest }),
            Some(radius) => world.entity_mut(e).insert(WheelJoint { owner, rest, radius, rolled: 0.0 }),
        };
    }
}

/// Height of joint `e` in the model's own space: its local translations composed up to `model`.
fn model_space_height(world: &World, e: Entity, model: Entity) -> f32 {
    let mut m = Mat4::IDENTITY;
    let mut at = e;
    while at != model {
        let Some(tf) = world.get::<Transform>(at) else { break };
        m = tf.to_matrix() * m;
        let Some(p) = world.get::<ChildOf>(at) else { break };
        at = p.parent();
    }
    m.w_axis.y
}

/// Each turreted root's `Aim` from its sim entity's order.
fn update_aims(
    mut commands: Commands,
    sim: Res<Sim>,
    turrets: Query<&TurretJoint>,
    mut roots: Query<(&SimRef, Option<&mut Aim>)>,
) {
    for joint in &turrets {
        let Ok((sim_ref, aim)) = roots.get_mut(joint.owner) else { continue };
        let at = aim_point(&sim, sim_ref.0);
        match aim {
            Some(mut a) => a.0 = at,
            None => {
                commands.entity(joint.owner).insert(Aim(at));
            }
        }
    }
}

/// Where entity `id` is aiming: the entity it attacks, if it is still there.
fn aim_point(sim: &Sim, id: EntityId) -> Option<Vec3> {
    let e = sim.world.entity(id)?;
    let target = match e.order {
        Order::Attack { target } => Some(target),
        Order::AttackMove { engaged, .. } => engaged,
        _ => None,
    }?;
    sim.world.entity(target).map(|t| to_world(t.pos))
}

/// Turns each turret toward its unit's target at `TURRET_SPEED`; without a target, back
/// to its rest. The target is taken into the turret's rest frame (its parent's global
/// transform, then the joint's own rest placement), so rotated parent bones and the
/// export's X mirror turn it the right way.
fn aim_turrets(
    time: Res<Time>,
    roots: Query<&Aim>,
    globals: Query<&GlobalTransform>,
    mut turrets: Query<(&TurretJoint, &ChildOf, &mut Transform)>,
) {
    let step = TURRET_SPEED * time.delta_secs();
    for (joint, parent, mut tf) in &mut turrets {
        let (Ok(aim), Ok(parent_global)) = (roots.get(joint.owner), globals.get(parent.parent())) else { continue };
        let want = aim.0.map_or(0.0, |at| {
            let in_parent = parent_global.affine().inverse().transform_point3(at);
            let local = joint.rest.inverse() * (in_parent - tf.translation);
            local.x.atan2(local.z)
        });
        let now = (joint.rest.inverse() * tf.rotation).to_euler(EulerRot::YXZ).0;
        let delta = (want - now + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        let yaw = now + delta.clamp(-step, step);
        tf.rotation = joint.rest * Quat::from_rotation_y(yaw);
    }
}

/// Rolls each wheel by the distance its unit moved since the last frame over its radius.
fn roll_wheels(
    roots: Query<&GlobalTransform, With<SimRef>>,
    mut last: Local<std::collections::HashMap<Entity, Vec3>>,
    mut wheels: Query<(&mut WheelJoint, &mut Transform), Without<SimRef>>,
) {
    let mut moved = std::collections::HashMap::new();
    for (mut w, mut tf) in &mut wheels {
        let Ok(root) = roots.get(w.owner) else { continue };
        let distance = *moved.entry(w.owner).or_insert_with(|| {
            let now = root.translation();
            let before = last.insert(w.owner, now).unwrap_or(now);
            // Forward distance: backing up rolls the wheels backwards.
            (now - before).dot(root.affine().transform_vector3(Vec3::Z).normalize_or_zero())
        });
        w.rolled = (w.rolled + distance / w.radius).rem_euclid(std::f32::consts::TAU);
        tf.rotation = w.rest * Quat::from_rotation_x(w.rolled);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A model mirrored in X and scaled 2x, as imported ones are, holding a `Turret` joint
    /// and a wheel 0.5 above its base, under a unit's root: the turret's world-space front
    /// swings toward the aim point no faster than `TURRET_SPEED`, settles on it, and swings
    /// back to the hull's front once there is none; the wheel rolls by the distance moved
    /// over its world radius, backwards when the unit backs up.
    #[test]
    fn turrets_track_their_aim_and_wheels_roll_with_distance() {
        use bevy::time::TimeUpdateStrategy;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::transform::TransformPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(100)))
            .add_systems(Update, (aim_turrets, roll_wheels).chain());
        let root =
            app.world_mut().spawn((SimRef(EntityId { index: 0, generation: 0 }), Transform::default(), Aim(None))).id();
        let model = app.world_mut().spawn((Transform::from_scale(Vec3::new(-2.0, 2.0, 2.0)), ChildOf(root))).id();
        let turret =
            app.world_mut().spawn((Name::new("Turret"), Transform::from_xyz(0.0, 1.0, 0.0), ChildOf(model))).id();
        let wheel =
            app.world_mut().spawn((Name::new("wheel_L01"), Transform::from_xyz(1.0, 0.5, 0.0), ChildOf(model))).id();
        tag_model(app.world_mut(), model);
        app.update();
        // The turret's front in world space (its local +Z through the mirrored chain).
        let front = |app: &App| -> Vec3 {
            app.world().get::<GlobalTransform>(turret).unwrap().affine().transform_vector3(Vec3::Z).normalize()
        };
        let bearing = |app: &App| front(app).x.atan2(front(app).z);
        assert!(front(&app).z > 0.999, "at rest the turret faces the hull's front");
        // Aim 90 degrees to the right of the hull (+X when facing +Z).
        app.world_mut().get_mut::<Aim>(root).unwrap().0 = Some(Vec3::new(10.0, 2.0, 0.0));
        // Global transforms trail the rotation by one update: two updates, two steps.
        app.update();
        app.update();
        let first = bearing(&app);
        assert!(first > 0.0 && first <= 2.0 * TURRET_SPEED * 0.1 + 1e-3, "toward it, at most the turn rate: {first}");
        for _ in 0..20 {
            app.update();
        }
        assert!(front(&app).x > 0.999, "settled facing the aim at +X: {:?}", front(&app));
        app.world_mut().get_mut::<Aim>(root).unwrap().0 = None;
        for _ in 0..20 {
            app.update();
        }
        assert!(front(&app).z > 0.999, "back to the front: {:?}", front(&app));
        // Roll: the wheel's world radius is 0.5 x 2 = 1, so 1.5 forward is 1.5 rad.
        let rolled = |app: &App| app.world().get::<WheelJoint>(wheel).unwrap().rolled;
        let before = rolled(&app);
        app.world_mut().get_mut::<Transform>(root).unwrap().translation.z += 1.5;
        app.update();
        app.update();
        assert!((rolled(&app) - before - 1.5).abs() < 1e-3, "rolled {} for 1.5 forward", rolled(&app) - before);
        app.world_mut().get_mut::<Transform>(root).unwrap().translation.z -= 0.5;
        app.update();
        app.update();
        assert!((rolled(&app) - before - 1.0).abs() < 1e-3, "rolled back 0.5: {}", rolled(&app) - before);
    }
}
