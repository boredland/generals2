//! Moves the joints of skinned alpha models: a vehicle's `Turret` joint yaws toward what it
//! is attacking (back to straight ahead without a target), and its `wheel*` joints roll
//! with the distance it covers. Joint names are the alpha's skeletons' own; a model without
//! such joints (or the procedural models) is left alone.
//!
//! Models with the alpha's clips (`alpha::Cache::clips`) play them by what their sim entity
//! is doing (`alpha::clip_for`): soldiers stand, run, fire and die, helicopters hover,
//! derricks pump and radars turn. A clip's place is computed, not accumulated: from the sim
//! tick (plus the frame's share of the next one) and a phase per sim entity and per soldier
//! of a squad, so a headless frame shows the same pose on every run and machine. A run cycle
//! plays at the unit's pace over the stride its root motion covers, so the feet keep pace
//! with the ground. A soldier killed in sight leaves a body that plays its death clip.

use std::collections::BTreeMap;

use bevy::animation::{AnimatedBy, AnimationTargetId};
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;
use g2_sim::{EntityId, GatherPhase, Order};

use crate::alpha::{Action, ClipInfo};
use crate::sim::{Sim, to_world};
use crate::view::SimRef;

pub struct RigPlugin;

impl Plugin for RigPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Clips>().add_observer(tag_joints).add_systems(
            Update,
            (
                // Before the mirror despawns a dead soldier's root, so its models can stay.
                leave_bodies.after(crate::menu::MenuSet).before(crate::view::ViewSet),
                (load_clips, update_aims, aim_turrets, roll_wheels, spin_rotors, play_clips)
                    .chain()
                    .after(crate::menu::MenuSet)
                    .after(crate::view::ViewSet),
            ),
        );
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

/// A helicopter's rotor joint (`*Propellor*`): turns with sim time about its own up axis,
/// or its side axis for a tail rotor (its joint sits behind the cabin), from its bind pose.
/// The alpha's hover clips leave the propellers still; the engine spun them in code.
#[derive(Component)]
pub struct RotorJoint {
    rest: Quat,
    axis: Vec3,
}

/// Radians per second a rotor turns, as the procedural models' rotors (`view.rs`).
const ROTOR_SPEED: f32 = 18.0;

/// Radians per second a turret turns at most, so it swings rather than snaps.
const TURRET_SPEED: f32 = 2.0;

/// The clips each def's model plays, read once the import is known.
#[derive(Resource, Default)]
pub struct Clips {
    loaded: bool,
    by_def: BTreeMap<&'static str, DefClips>,
}

/// One def's clips: the graph holding them (one node per clip under the root), per action
/// the node to play with its length and stride, and the clip assets to wait for.
#[derive(Clone)]
struct DefClips {
    graph: Handle<AnimationGraph>,
    actions: Vec<(Action, AnimationNodeIndex, ClipInfo)>,
    /// Every joint at its bind pose, for actions without a clip.
    rest: AnimationNodeIndex,
    assets: Vec<Handle<AnimationClip>>,
}

impl DefClips {
    fn get(&self, a: Action) -> Option<(AnimationNodeIndex, &ClipInfo)> {
        self.actions.iter().find(|(x, ..)| *x == a).map(|(_, n, c)| (*n, c))
    }
}

/// A model playing its def's clips: the sim entity's root it stands for (`None` for a
/// body), its def, and its phase in seconds.
#[derive(Component)]
pub struct Animated {
    owner: Option<Entity>,
    def: &'static str,
    phase: f32,
}

/// A soldier's body where it was killed: it plays its death clip from `since` (sim seconds)
/// and holds the last pose, then sinks into the ground over the last `SINK_SECS` of
/// `BODY_SECS` and goes.
#[derive(Component)]
struct Body {
    since: f32,
    ground: f32,
}

/// How long a body lies, in sim seconds (the procedural kills' wrecks stay 25 s).
const BODY_SECS: f32 = 15.0;
const SINK_SECS: f32 = 3.0;

/// Sim seconds, with the frame's share of the next tick.
fn sim_seconds(sim: &Sim, fixed: &Time<Fixed>) -> f32 {
    let part = if sim.speed == 0 { 0.0 } else { fixed.overstep_fraction() };
    (sim.world.tick() as f32 + part) / g2_sim::TICKS_PER_SECOND as f32
}

/// Reads every def's clip list and starts loading its clips, once.
fn load_clips(
    mut clips: ResMut<Clips>,
    cache: Option<Res<crate::alpha::Cache>>,
    server: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    if clips.loaded {
        return;
    }
    clips.loaded = true;
    let Some(cache) = cache else { return };
    for def in g2_rules::rules().defs {
        let Some((file, list)) = cache.clips(def) else { continue };
        let mut graph = AnimationGraph::new();
        let mut assets = Vec::new();
        let mut add = |c: &ClipInfo| {
            let path = bevy::asset::AssetPath::from(file.clone()).with_source("alpha");
            let handle: Handle<AnimationClip> = server.load(GltfAssetLabel::Animation(c.index).from_asset(path));
            assets.push(handle.clone());
            graph.add_clip(handle, 1.0, graph.root)
        };
        let Some(rest) = list.iter().find(|c| c.name == "BindPose").map(&mut add) else { continue };
        let mut actions = Vec::new();
        for a in Action::ALL {
            let Some(name) = crate::alpha::clip_for(def.name, a) else { continue };
            if let Some(c) = list.iter().find(|c| c.name == name) {
                actions.push((a, add(c), c.clone()));
            }
        }
        if !actions.is_empty() {
            clips.by_def.insert(def.name, DefClips { graph: graphs.add(graph), actions, rest, assets });
        }
    }
}

/// Whether every clip the models play has loaded (true without any), for the headless
/// screenshot to wait on, as it waits on the models.
pub fn clips_loaded(world: &World) -> bool {
    let (Some(clips), Some(server)) = (world.get_resource::<Clips>(), world.get_resource::<AssetServer>()) else {
        return true;
    };
    clips.loaded && clips.by_def.values().flat_map(|d| &d.assets).all(|h| server.is_loaded_with_dependencies(h))
}

/// What sim entity `id` is doing, for its model. `moved` is how far it went over the last
/// step, when known: a unit that moved runs. Without it (a frame rendered straight after
/// the sim ran), its order says whether it is on its way.
fn action(sim: &Sim, id: EntityId, moved: Option<f32>) -> Action {
    let Some(e) = sim.world.entity(id) else { return Action::Idle };
    let walking = matches!(
        e.order,
        Order::Move { .. }
            | Order::Garrison { .. }
            | Order::AttackMove { engaged: None, .. }
            | Order::Gather { phase: GatherPhase::ToPile | GatherPhase::ToDepot { .. }, .. }
    );
    if moved.map_or(walking, |d| d > 1e-3) {
        return Action::Moving;
    }
    match e.order {
        Order::Attack { .. } | Order::AttackMove { engaged: Some(_), .. } | Order::Special { .. } => Action::Attacking,
        Order::Construct { site } if sim.world.entity(site).is_some_and(|s| s.construction.is_some()) => {
            Action::Attacking
        }
        Order::Gather { phase: GatherPhase::Loading { .. } | GatherPhase::Unloading { .. }, .. } => Action::Attacking,
        _ => Action::Idle,
    }
}

/// Once an alpha model's joints exist, tags the ones this module moves.
fn tag_joints(ready: On<WorldInstanceReady>, mut commands: Commands) {
    let model = ready.entity;
    commands.queue(move |world: &mut World| tag_model(world, model));
}

/// Tags the turret and wheel joints under `model` with the sim entity's root, found by
/// walking up from the model, and gives a model whose def has clips a player for them (a
/// body has no sim entity, only its clips).
fn tag_model(world: &mut World, model: Entity) {
    let mut owner = model;
    while world.get::<SimRef>(owner).is_none() {
        let Some(p) = world.get::<ChildOf>(owner) else {
            if world.get::<Animated>(model).is_some() {
                attach_player(world, model, None);
            }
            return;
        };
        owner = p.parent();
    }
    attach_player(world, model, Some(owner));
    let scale = world.get::<Transform>(model).map_or(1.0, |t| t.scale.y);
    let mut stack: Vec<Entity> = world.get::<Children>(model).map(|c| c.to_vec()).unwrap_or_default();
    let mut tags = Vec::new();
    while let Some(e) = stack.pop() {
        stack.extend(world.get::<Children>(e).map(|c| c.to_vec()).unwrap_or_default());
        let (Some(name), Some(tf)) = (world.get::<Name>(e), world.get::<Transform>(e)) else { continue };
        let n = name.as_str();
        if n == "Turret" {
            tags.push((e, Joint::Turret, tf.rotation));
        } else if n.starts_with("wheel") {
            // In model space a wheel's height above the model's base is its radius (the
            // alpha's origin is at ground level). Global transforms aren't settled the
            // frame the scene spawns, so the local chain gives it.
            let radius = model_space(world, e, model).y.max(0.05) * scale;
            tags.push((e, Joint::Wheel(radius), tf.rotation));
        } else if is_rotor(n) {
            tags.push((e, Joint::Rotor(rotor_axis(n, model_space(world, e, model))), tf.rotation));
        }
    }
    for (e, joint, rest) in tags {
        match joint {
            Joint::Turret => world.entity_mut(e).insert(TurretJoint { owner, rest }),
            Joint::Wheel(radius) => world.entity_mut(e).insert(WheelJoint { owner, rest, radius, rolled: 0.0 }),
            Joint::Rotor(axis) => world.entity_mut(e).insert(RotorJoint { rest, axis }),
        };
    }
}

enum Joint {
    Turret,
    Wheel(f32),
    Rotor(Vec3),
}

/// The joints this module turns itself, which the clips leave alone.
fn is_rotor(name: &str) -> bool {
    name.to_ascii_lowercase().contains("propellor") && !name.ends_with("_CUSH") && !name.ends_with("_cushion")
}

/// The axis a rotor joint spins about, in its own frame: the thinnest extent of the vertices
/// it carries, measured over every helicopter skeleton's propellers. Main rotors lie flat
/// (thin in Y); tail rotors stand on the tail boom (thin in X) and sit more than 2 units
/// behind the model's centre. The EU harvester's `Backpropellor` sits behind too, but lies
/// flat as a second lift rotor, 3.9 high against the tail rotors' 1.7-2.1.
fn rotor_axis(name: &str, at: Vec3) -> Vec3 {
    if name.starts_with("Back") && at.z < -2.0 && at.y < 3.0 { Vec3::X } else { Vec3::Y }
}

/// Gives `model` (a spawned alpha scene) an `AnimationPlayer` for its def's clips, when it
/// has any. The clip files target joints by their path of names from the skeleton's root
/// joint (`Reference`), which is how the model's joints are tagged here; turret and wheel
/// joints are left out, as this module turns those. The phase comes from the sim entity's
/// index and the model's place in its squad, so soldiers of a squad, and squads side by
/// side, don't step in time.
fn attach_player(world: &mut World, model: Entity, owner: Option<Entity>) {
    let def = match owner.and_then(|o| world.get::<SimRef>(o)).map(|r| r.0) {
        Some(id) => world.get_resource::<Sim>().and_then(|s| s.world.entity(id).map(|e| s.world.rules.def(e.def).name)),
        None => world.get::<Animated>(model).map(|a| a.def),
    };
    let Some(def) = def else { return };
    let Some(clips) = world.get_resource::<Clips>().and_then(|c| c.by_def.get(def)).cloned() else { return };
    let Some(root) = joint_root(world, model) else { return };
    let mut stack = vec![(root, Vec::<Name>::new())];
    while let Some((e, mut path)) = stack.pop() {
        let Some(name) = world.get::<Name>(e).cloned() else { continue };
        let ours = name.as_str() == "Turret" || name.as_str().starts_with("wheel") || is_rotor(name.as_str());
        path.push(name);
        if !ours {
            world.entity_mut(e).insert((AnimationTargetId::from_names(path.iter()), AnimatedBy(model)));
        }
        for c in world.get::<Children>(e).map(|c| c.to_vec()).unwrap_or_default() {
            stack.push((c, path.clone()));
        }
    }
    let mut player = AnimationPlayer::default();
    player.play(clips.rest).set_weight(0.0).pause();
    for (_, node, _) in &clips.actions {
        player.play(*node).set_weight(0.0).pause();
    }
    if world.get::<Animated>(model).is_none() {
        let id = owner.and_then(|o| world.get::<SimRef>(o)).map_or(0, |r| r.0.index);
        let slot = world.get::<ChildOf>(model).and_then(|p| world.get::<Children>(p.parent())).map_or(0, |c| {
            c.iter().filter(|&e| world.get::<WorldAssetRoot>(e).is_some()).position(|e| e == model).unwrap_or(0)
        });
        // Golden-ratio steps: neighbouring ids and squad slots land far apart in a cycle.
        let phase = (id as f32 * 0.618_034 + slot as f32 * 0.381_966).fract() * 30.0;
        world.entity_mut(model).insert(Animated { owner, def, phase });
    }
    world.entity_mut(model).insert((player, AnimationGraphHandle(clips.graph.clone())));
}

/// The skeleton's root joint under a model scene: the joint named `Reference`.
fn joint_root(world: &World, model: Entity) -> Option<Entity> {
    let mut stack: Vec<Entity> = world.get::<Children>(model).map(|c| c.to_vec()).unwrap_or_default();
    while let Some(e) = stack.pop() {
        if world.get::<Name>(e).is_some_and(|n| n.as_str() == "Reference") {
            return Some(e);
        }
        stack.extend(world.get::<Children>(e).map(|c| c.to_vec()).unwrap_or_default());
    }
    None
}

/// Puts every animated model on the clip for its entity's action (its idle clip where the
/// action has none, its bind pose without either), at a time from the sim clock and its
/// phase. One clip plays at full weight, the rest at none; all stay paused, so only this
/// sets their time.
fn play_clips(
    sim: Res<Sim>,
    clips: Res<Clips>,
    prev: Option<Res<crate::sim::Previous>>,
    fixed: Res<Time<Fixed>>,
    roots: Query<&SimRef>,
    mut players: Query<(&Animated, &mut AnimationPlayer, Option<&Body>)>,
) {
    let now = sim_seconds(&sim, &fixed);
    for (anim, mut player, body) in &mut players {
        let Some(def) = clips.by_def.get(anim.def) else { continue };
        let chosen = match (body, anim.owner.and_then(|o| roots.get(o).ok())) {
            // A death plays once from the kill and holds its last pose.
            (Some(b), _) => def.get(Action::Dying).map(|(n, c)| (n, (now - b.since).clamp(0.0, c.seconds))),
            (None, Some(r)) => {
                let e = sim.world.entity(r.0);
                let moved = e
                    .zip(prev.as_ref().and_then(|p| p.0.get(&r.0)))
                    .map(|(e, &before)| (to_world(e.pos) - to_world(before)).length());
                let act = action(&sim, r.0, moved);
                // Units per second: a group's shared pace, else the unit's own speed.
                let pace = e.map_or(0.0, |e| match e.order {
                    Order::Move { pace, .. } | Order::AttackMove { pace, .. } => pace.to_f32(),
                    _ => sim.world.rules.def(e.def).speed().to_f32(),
                }) * g2_sim::TICKS_PER_SECOND as f32;
                def.get(act).map(|x| (act, x)).or_else(|| def.get(Action::Idle).map(|x| (Action::Idle, x))).map(
                    |(act, (n, c))| {
                        // A run cycle over its root motion's stride at the unit's pace, so the
                        // feet keep up with the ground; every other clip at its own rate.
                        let rate = if act == Action::Moving && c.travel > 0.1 && pace > 0.0 {
                            pace * c.seconds / c.travel
                        } else {
                            1.0
                        };
                        (n, ((now + anim.phase) * rate).rem_euclid(c.seconds.max(1e-3)))
                    },
                )
            }
            (None, None) => None,
        };
        let (node, at) = chosen.unwrap_or((def.rest, 0.0));
        for (&n, a) in player.playing_animations_mut() {
            a.set_weight(if n == node { 1.0 } else { 0.0 });
            if n == node {
                a.set_seek_time(at);
            }
        }
    }
}

/// Leaves a body where a soldier with a death clip went out of the sim in plain sight (a
/// soldier only leaves the sim by dying, or when its side is out): its squad's models stay,
/// taken off the root `view.rs` is about to despawn, and play the clip where they stood, so
/// no second copy of the scene has to load. Bodies are decoration, like the wrecks of
/// `view.rs`, and go after `BODY_SECS`.
fn leave_bodies(
    mut commands: Commands,
    sim: Res<Sim>,
    clips: Res<Clips>,
    fixed: Res<Time<Fixed>>,
    mut models: Query<(Entity, &mut Animated, &GlobalTransform, &InheritedVisibility)>,
    roots: Query<&SimRef>,
    mut bodies: Query<(Entity, &Body, &mut Transform)>,
) {
    let now = sim_seconds(&sim, &fixed);
    for (e, mut anim, global, visible) in &mut models {
        let Some(Ok(r)) = anim.owner.map(|o| roots.get(o)) else { continue };
        // `view.rs` keeps a remembered entity as a ghost, so only the forgotten go.
        if sim.world.entity(r.0).is_some() || sim.remembers(r.0) || !visible.get() {
            continue;
        }
        if clips.by_def.get(anim.def).and_then(|d| d.get(Action::Dying)).is_none() {
            continue;
        }
        anim.owner = None;
        let tf = global.compute_transform();
        commands.entity(e).remove::<ChildOf>().insert((
            crate::menu::MatchEntity,
            tf,
            Body { since: now, ground: tf.translation.y },
        ));
    }
    for (e, body, mut tf) in &mut bodies {
        let age = now - body.since;
        if !(0.0..BODY_SECS).contains(&age) {
            commands.entity(e).despawn();
        } else {
            tf.translation.y = body.ground - ((age - (BODY_SECS - SINK_SECS)) / SINK_SECS).clamp(0.0, 1.0) * 1.2;
        }
    }
}

/// Where joint `e` sits in the model's own space: its local transforms composed up to `model`.
fn model_space(world: &World, e: Entity, model: Entity) -> Vec3 {
    let mut m = Mat4::IDENTITY;
    let mut at = e;
    while at != model {
        let Some(tf) = world.get::<Transform>(at) else { break };
        m = tf.to_matrix() * m;
        let Some(p) = world.get::<ChildOf>(at) else { break };
        at = p.parent();
    }
    m.w_axis.truncate()
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

/// Turns each rotor by sim time from its bind pose, so a frame shows it where it is at that
/// tick on every run.
fn spin_rotors(sim: Res<Sim>, fixed: Res<Time<Fixed>>, mut rotors: Query<(&RotorJoint, &mut Transform)>) {
    let angle = (sim_seconds(&sim, &fixed) * ROTOR_SPEED).rem_euclid(std::f32::consts::TAU);
    for (r, mut tf) in &mut rotors {
        tf.rotation = r.rest * Quat::from_axis_angle(r.axis, angle);
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

    /// Three soldier-like models given clips through the real systems: a one-joint clip per
    /// action (the joint's height is the clip's time), on the def of the unit the EU starts
    /// with (its bulldozer; a squad draws three models the same way). Idle, the models play
    /// the idle clip, each at its own phase; ordered to move, the run, paced by the stride;
    /// two apps at the same tick pose them the same, whatever ran before; and once the unit
    /// is gone, the models stay as bodies when the view despawns the root, playing the death
    /// clip from that moment.
    #[test]
    fn soldiers_play_their_clips_by_order_and_tick() {
        use bevy::animation::{AnimationPlugin, animated_field};
        use bevy::time::TimeUpdateStrategy;
        use g2_sim::{CommandKind, Faction};
        let make = || {
            let mut app = App::new();
            app.add_plugins((
                MinimalPlugins,
                AssetPlugin::default(),
                bevy::transform::TransformPlugin,
                AnimationPlugin,
            ))
            .insert_resource(TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(50)))
            .insert_resource(Sim::new(
                g2_rules::crater_ridge(),
                None,
                1,
                Sim::players([Faction::Eu, Faction::Gla], false),
            ))
            .init_resource::<Clips>()
            .add_systems(Update, (leave_bodies, play_clips).chain());
            // Paused, as the headless render's warm-up is: the frame shows the tick itself.
            app.world_mut().resource_mut::<Sim>().speed = 0;
            app.world_mut().resource_mut::<Clips>().loaded = true;
            // Clips whose joint `Reference/Hips` rises with time: 1 per second for the idle,
            // 10 for the run, 100 for the death, so its height says which plays and where.
            let clip = |rate: f32, seconds: f32| {
                let mut c = AnimationClip::default();
                let curve = AnimatableCurve::new(
                    animated_field!(Transform::translation),
                    AnimatableKeyframeCurve::new([0.0, seconds].map(|t| (t, Vec3::Y * t * rate))).unwrap(),
                );
                c.add_curve_to_target(
                    AnimationTargetId::from_names(["Reference", "Hips"].map(Name::new).iter()),
                    curve,
                );
                c
            };
            let world = app.world_mut();
            let mut graph = AnimationGraph::new();
            let mut add = |c: AnimationClip| {
                let h = world.resource_mut::<Assets<AnimationClip>>().add(c);
                graph.add_clip(h, 1.0, graph.root)
            };
            let rest = add(AnimationClip::default());
            let info =
                |name: &str, seconds: f32, travel: f32| ClipInfo { index: 0, seconds, travel, name: name.into() };
            let actions = vec![
                (Action::Idle, add(clip(1.0, 2.0)), info("idle", 2.0, 0.0)),
                (Action::Moving, add(clip(10.0, 0.5)), info("run", 0.5, 2.0)),
                (Action::Dying, add(clip(100.0, 1.0)), info("die", 1.0, 0.0)),
            ];
            let graph = world.resource_mut::<Assets<AnimationGraph>>().add(graph);
            world
                .resource_mut::<Clips>()
                .by_def
                .insert("EU Bulldozer", DefClips { graph, actions, rest, assets: vec![] });
            app
        };
        let squad = |app: &App| {
            let sim = app.world().resource::<Sim>();
            sim.world.entities().find(|e| sim.world.rules.def(e.def).name == "EU Bulldozer").map(|e| e.id)
        };
        // Three soldier models under the squad's root, each with its `Reference/Hips` joint.
        let spawn = |app: &mut App| -> Vec<Entity> {
            let id = squad(app).expect("the EU starts with a bulldozer");
            let root = app.world_mut().spawn((SimRef(id), Transform::default(), Visibility::default())).id();
            (0..3)
                .map(|_| {
                    let w = app.world_mut();
                    // In sight: there is no renderer here to work visibility out.
                    let model = w.spawn((WorldAssetRoot::default(), ChildOf(root), InheritedVisibility::VISIBLE)).id();
                    let r = w.spawn((Name::new("Reference"), Transform::default(), ChildOf(model))).id();
                    w.spawn((Name::new("Hips"), Transform::default(), ChildOf(r)));
                    tag_model(w, model);
                    model
                })
                .collect()
        };
        let hips = |app: &mut App, model: Entity| -> f32 {
            let w = app.world_mut();
            let r = w.get::<Children>(model).unwrap()[0];
            let h = w.get::<Children>(r).unwrap()[0];
            w.get::<Transform>(h).unwrap().translation.y
        };
        let mut app = make();
        let soldiers = spawn(&mut app);
        app.update();
        app.update();
        let idle: Vec<f32> = soldiers.iter().map(|&s| hips(&mut app, s)).collect();
        let idle_phases: Vec<f32> = soldiers.iter().map(|&s| app.world().get::<Animated>(s).unwrap().phase).collect();
        assert!(idle.iter().all(|y| (0.0..=2.0).contains(y)), "idle clip: {idle:?}");
        assert!(idle[0] != idle[1] && idle[1] != idle[2], "each soldier at its own phase: {idle:?}");
        // The same tick in a fresh app, after other frames ran: the same poses.
        let mut other = make();
        let theirs = spawn(&mut other);
        for _ in 0..7 {
            other.update();
        }
        let again: Vec<f32> = theirs.iter().map(|&s| hips(&mut other, s)).collect();
        assert_eq!(idle, again, "poses come from the tick, not from frames played");
        // Walk: the run clip, its time paced by the unit's speed over its 2-unit stride.
        let id = squad(&app).unwrap();
        let target = app.world().resource::<Sim>().world.entity(id).unwrap().pos + g2_sim::Vec2::from_int(30, 0);
        let eu = app.world().resource::<Sim>().world.entity(id).unwrap().owner.unwrap();
        app.world_mut().resource_mut::<Sim>().world.submit(eu, CommandKind::Move { units: vec![id], target });
        // Commands land a few ticks later (the sim's input delay).
        let step_until = |app: &mut App, done: &dyn Fn(&Sim) -> bool| {
            for _ in 0..60 {
                if done(app.world().resource::<Sim>()) {
                    return;
                }
                app.world_mut().resource_mut::<Sim>().world.step();
            }
            panic!("timed out");
        };
        step_until(&mut app, &|s| matches!(s.world.entity(id).map(|e| e.order), Some(Order::Move { .. })));
        app.world_mut().resource_mut::<Sim>().world.step();
        app.update();
        let run: Vec<f32> = soldiers.iter().map(|&s| hips(&mut app, s)).collect();
        assert!(run.iter().all(|y| (0.0..=5.0).contains(y)) && run != idle, "run clip: {run:?}");
        let sim = app.world().resource::<Sim>();
        let now = sim.world.tick() as f32 / g2_sim::TICKS_PER_SECOND as f32;
        let order = sim.world.entity(id).unwrap().order;
        let Order::Move { pace, .. } = order else { panic!("walking: {order:?}") };
        let pace = pace.to_f32() * g2_sim::TICKS_PER_SECOND as f32;
        let phase = app.world().get::<Animated>(soldiers[0]).unwrap().phase;
        let want = ((now + phase) * pace * 0.5 / 2.0).rem_euclid(0.5) * 10.0;
        assert!((run[0] - want).abs() < 1e-3, "paced by speed over stride: {} against {want}", run[0]);
        // Gone (its side surrenders): the models come off the root before the view despawns
        // it, and play the death clip from that moment.
        app.world_mut().resource_mut::<Sim>().world.submit(eu, CommandKind::Surrender);
        step_until(&mut app, &|s| s.world.entity(id).is_none());
        assert!(squad(&app).is_none(), "the squad left the sim");
        let root = app.world().get::<ChildOf>(soldiers[0]).unwrap().parent();
        app.update();
        app.world_mut().entity_mut(root).despawn();
        let since = app.world().resource::<Sim>().world.tick() as f32 / g2_sim::TICKS_PER_SECOND as f32;
        for _ in 0..3 {
            app.world_mut().resource_mut::<Sim>().world.step();
        }
        app.update();
        let now = app.world().resource::<Sim>().world.tick() as f32 / g2_sim::TICKS_PER_SECOND as f32;
        for &s in &soldiers {
            let body = app.world().get::<Body>(s).expect("the soldier's model is its body");
            assert_eq!(body.since, since, "from the kill");
            assert!(app.world().get::<ChildOf>(s).is_none(), "off the despawned root");
        }
        let died: Vec<f32> = soldiers.iter().map(|&s| hips(&mut app, s)).collect();
        assert!(died.iter().all(|y| (y - (now - since) * 100.0).abs() < 1e-3), "death clip from the kill: {died:?}");
        // Each keeps its soldier's phase.
        let phases: Vec<f32> = soldiers.iter().map(|&s| app.world().get::<Animated>(s).unwrap().phase).collect();
        assert_eq!(phases, idle_phases);
    }
}
