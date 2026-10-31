//! Moves the procedural models' parts (`models::Motion`) by what their sim entity is doing, as
//! `rig.rs` plays the alpha's clips on its models: soldiers walk at their unit's pace, shift
//! their weight standing, raise their guns at what they attack and kick when they fire, and fall
//! when killed; treads run and wheels roll with the ground they cover; turrets turn to their
//! target, barrels lay to the elevation of their shot and kick back; a pumpjack nods while its
//! well pays; rotors, fans and radar feeds turn, cranes swing and flags fly in the wind.
//!
//! A pose is computed, not accumulated: from the sim tick (plus the frame's share of the next)
//! and a phase per sim entity, so a headless frame shows the same pose on every run. Only the
//! distance a vehicle has covered and its turret's bearing carry over from frame to frame, and
//! both follow the sim's positions and targets. Only the transforms of the parts already there
//! change (and a tread's mesh, swapped between a few pre-built phases of its run), so a moving
//! part costs a few multiplies a frame.

use std::f32::consts::TAU;

use bevy::prelude::*;
use g2_sim::entity::DeployState;
use g2_sim::{BuildingRole, EntityId, Order};

use crate::alpha::Action;
use crate::models::{Limb, Motion, SOLDIER_HIP};
use crate::sim::{Sim, to_world};
use crate::surface::{TREAD_PHASES, TREAD_PITCH};
use crate::view::SimRef;

/// A moving part: the root of the sim entity it belongs to, its place in the model at rest and
/// how it moves.
#[derive(Component)]
pub struct Moving {
    pub owner: Entity,
    pub rest: Transform,
    pub motion: Motion,
}

/// A track band's meshes, one per step of its run (`surface::tread_mesh`).
#[derive(Component)]
pub struct Tread(pub [Handle<Mesh>; TREAD_PHASES]);

/// A root's model state: what its parts show this frame (`pose`), its phase, and what carries
/// over between frames.
#[derive(Component)]
pub struct Drive {
    phase: f32,
    last: Option<Vec3>,
    seen: Option<f32>,
    pub pose: Pose,
}

impl Drive {
    /// A fresh model of sim entity `id`. Golden-ratio steps put neighbouring ids far apart in
    /// a cycle, so squads side by side don't step in time.
    pub fn new(id: EntityId) -> Drive {
        let pose = Pose { fired: f32::INFINITY, ..Pose::default() };
        Drive { phase: (id.index as f32 * 0.618_034).fract() * 30.0, last: None, seen: None, pose }
    }
}

/// What a model's moving parts show, in its own frame (+Z its front).
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Pose {
    /// Sim seconds plus the entity's phase.
    pub t: f32,
    /// Metres a second the unit walks; 0 standing.
    pub pace: f32,
    /// A soldier stands to fire, gun up, facing `aim`.
    pub aiming: bool,
    /// Bearing of what the unit attacks, from its front.
    pub aim: f32,
    /// The turret's bearing from the front; it turns toward `aim` at `TURRET_SPEED`.
    pub turret: f32,
    /// Radians the gun is laid above level; it moves toward its shot's at `ELEVATE_SPEED`.
    pub elevation: f32,
    /// Seconds since the gun last fired; large when it hasn't lately.
    pub fired: f32,
    /// Metres the vehicle has driven forward (backing up runs it back).
    pub travel: f32,
    /// A unit, whose rotors and fans always turn.
    pub unit: bool,
    /// A building at work: complete and powered; a derrick, complete on a well with oil left,
    /// since it pays with power or without (`World::update_derricks`).
    pub working: bool,
    /// Bearing the wind blows toward, from the model's +X toward its -Z (`fx::WIND`).
    pub wind: f32,
    /// How hard it blows, 0 to 1 (`fx::gust`).
    pub gust: f32,
}

/// Radians a second a turret turns at most, as the alpha's (`rig.rs`).
const TURRET_SPEED: f32 = 2.0;
/// Radians a second a gun lays up or down: our own, so a howitzer reaches its longest shot's
/// 50 degrees in about a second, before its first shell (which takes its 2-6.5 s cooldown).
const ELEVATE_SPEED: f32 = 0.9;
/// The steepest a gun lays: about 70 degrees, a howitzer's limit; also the cap on a long
/// shell's arc.
pub const MAX_ELEVATION: f32 = 1.2;
/// Metres a soldier covers per stride cycle (a step with each foot).
const STRIDE: f32 = 1.6;
/// Radians a leg swings either way from the hip at a walk.
const LEG_SWING: f32 = 0.5;
/// Metres a gun kicks back when it fires, and seconds it takes to run out again.
const KICK: f32 = 0.35;
const KICK_SECS: f32 = 0.3;
/// A fallen soldier tips this far, over `FALL_SECS`.
const FALL: f32 = 1.45;
const FALL_SECS: f32 = 0.6;

/// Sets each root's `Pose` from its sim entity: its action as `rig.rs` reads it (a unit that
/// moved over the last step walks), the target its order or last shot says, and how far it
/// went since the last frame. A root whose entity is gone, or out of sight, keeps its last pose,
/// as `view.rs` keeps a remembered building's look.
pub fn drive(
    sim: Res<Sim>,
    fixed: Res<Time<Fixed>>,
    prev: Option<Res<crate::sim::Previous>>,
    fire: Option<Res<crate::sim::Fire>>,
    mut roots: Query<(&SimRef, &Transform, &mut Drive)>,
) {
    let world = &sim.world;
    let now = crate::rig::sim_seconds(&sim, &fixed);
    let tick_secs = 1.0 / g2_sim::TICKS_PER_SECOND as f32;
    for (r, tf, mut d) in &mut roots {
        let Some(e) = world.entity(r.0).filter(|e| sim.sees(e)) else { continue };
        let def = world.rules.def(e.def);
        let shot = fire.as_ref().and_then(|f| f.0.get(&r.0)).copied();
        let fired = shot.map_or(1e3, |(tick, _)| now - tick as f32 * tick_secs);
        let aimed = aim_target(&sim, r.0);
        let target = aimed.map(|(at, _)| at).or_else(|| shot.map(|(_, at)| to_world(at)));
        let aim = target.map(|at| {
            let local = tf.rotation.inverse() * (at - tf.translation);
            local.x.atan2(local.z)
        });
        // A gun lays for the weapon it has now: packed, a Thunderfist's direct-fire cannon
        // holds it about level; set up, its howitzer raises it.
        let weapon = def.weapon_for(e.deploy == DeployState::Deployed);
        let lift = match (target, weapon) {
            (Some(at), Some(w)) => {
                let (here, there) = (tf.translation.with_y(0.0), at.with_y(0.0));
                let ground = |p: Vec3| sim.ground_y(p.x, p.z);
                let air = aimed.map_or(0.0, |(_, h)| h);
                elevation(here.distance(there), ground(there) + air - ground(here), w.shell.map(|s| s.speed.to_f32()))
            }
            _ => 0.0,
        };
        // Forward distance since the last frame; the first frame after spawning covers none.
        let at = tf.translation;
        let forward = tf.rotation * Vec3::Z;
        let travel = d.last.map_or(0.0, |before| (at - before).dot(forward));
        d.last = Some(at);
        // A turret seen for the first time is already on its target (a frame rendered mid-
        // fight), and its gun laid; after that they swing at their rates over the sim time
        // passed.
        let want = aim.unwrap_or(0.0);
        let (turret, elevation) = match d.seen {
            None => (want, lift),
            Some(then) => {
                let secs = (now - then).max(0.0);
                let step = TURRET_SPEED * secs;
                let delta = (want - d.pose.turret + std::f32::consts::PI).rem_euclid(TAU) - std::f32::consts::PI;
                let lay = ELEVATE_SPEED * secs;
                (
                    d.pose.turret + delta.clamp(-step, step),
                    d.pose.elevation + (lift - d.pose.elevation).clamp(-lay, lay),
                )
            }
        };
        d.seen = Some(now);
        let (pace, aiming) = if def.is_unit() {
            let moved = prev.as_ref().and_then(|p| p.0.get(&r.0)).map(|&b| (to_world(e.pos) - to_world(b)).length());
            match crate::rig::action(&sim, r.0, moved) {
                Action::Moving => {
                    let per_tick = match e.order {
                        Order::Move { pace, .. } | Order::AttackMove { pace, .. } => pace.to_f32(),
                        _ => def.speed().to_f32(),
                    };
                    (per_tick * g2_sim::TICKS_PER_SECOND as f32, false)
                }
                Action::Attacking => (0.0, aim.is_some()),
                _ => (0.0, false),
            }
        } else {
            (0.0, false)
        };
        let working = e.owner.is_some()
            && e.is_complete()
            && match def.role() {
                Some(BuildingRole::OilDerrick { .. }) => {
                    e.on_well.and_then(|w| world.entity(w)).is_some_and(|w| w.resource > 0)
                }
                _ => e.powered,
            };
        let wind = tf.rotation.inverse() * crate::fx::WIND;
        d.pose = Pose {
            t: now + d.phase,
            pace,
            aiming,
            aim: want,
            turret,
            elevation,
            fired,
            travel: d.pose.travel + travel,
            unit: def.is_unit(),
            working,
            wind: (-wind.z).atan2(wind.x),
            gust: crate::fx::gust(now),
        };
    }
}

/// What entity `id` attacks, by its order: where it stands and how high it flies.
fn aim_target(sim: &Sim, id: EntityId) -> Option<(Vec3, f32)> {
    let target = match sim.world.entity(id)?.order {
        Order::Attack { target } => Some(target),
        Order::AttackMove { engaged, .. } => engaged,
        _ => None,
    }?;
    let t = sim.world.entity(target)?;
    Some((to_world(t.pos), crate::view::flight_height(sim.world.rules.def(t.def))))
}

/// Radians above level a gun lays to hit a point `run` away across the ground and `rise` above
/// its own ground: for a shell flying `speed` a tick across the ground, the angle it leaves on
/// the arc `fx.rs` draws for its flight time (as the sim counts it, `World::fire`), so the
/// barrel points along the shell's first stretch; for direct fire, the line from muzzle to
/// target, as the tracer runs. Never below level nor past `MAX_ELEVATION`.
pub fn elevation(run: f32, rise: f32, speed: Option<f32>) -> f32 {
    let slope = match speed {
        Some(v) => {
            // The sim lands a shell after `floor(run / speed)` ticks, at least one; the view
            // draws it from the tick it was fired, one more.
            let ticks = (run / v.max(1e-3)).floor().max(1.0) + 1.0;
            let secs = ticks / g2_sim::TICKS_PER_SECOND as f32;
            crate::fx::launch_slope(run, secs, rise + crate::fx::SHELL_LAND - crate::fx::SHELL_MUZZLE)
        }
        None => (rise + crate::fx::TRACER_HIT - crate::fx::TRACER_MUZZLE) / run.max(1.0),
    };
    slope.atan().clamp(0.0, MAX_ELEVATION)
}

/// Puts every moving part where its root's `Pose` has it, and a tread on the mesh of its run.
/// Parts at rest are left untouched, so they cost nothing downstream.
#[allow(clippy::type_complexity)]
pub fn pose_parts(roots: Query<&Drive>, mut parts: Query<(&Moving, &mut Transform, Option<(&Tread, &mut Mesh3d)>)>) {
    for (m, mut tf, tread) in &mut parts {
        let Ok(d) = roots.get(m.owner) else { continue };
        tf.set_if_neq(place(m.motion, &m.rest, &d.pose));
        if let Some((tread, mut mesh)) = tread {
            let want = &tread.0[tread_phase(d.pose.travel)];
            if mesh.0 != *want {
                mesh.0 = want.clone();
            }
        }
    }
}

/// Which of a tread's `TREAD_PHASES` meshes shows it `travel` metres along: one step per
/// `TREAD_PITCH / TREAD_PHASES` driven, backwards when backing up.
pub fn tread_phase(travel: f32) -> usize {
    let step = TREAD_PITCH / TREAD_PHASES as f32;
    ((travel / step).floor() as i64).rem_euclid(TREAD_PHASES as i64) as usize
}

/// Where a part that moves by `motion` and sits at `rest` is in pose `p`.
pub fn place(motion: Motion, rest: &Transform, p: &Pose) -> Transform {
    match motion {
        Motion::Still | Motion::Tread => *rest,
        Motion::Spin { pivot, rate } if p.unit || p.working => {
            turn(rest, pivot, Quat::from_rotation_y((p.t * rate).rem_euclid(TAU)))
        }
        Motion::Spin { .. } => *rest,
        Motion::Slew { pivot, swing, rate } => turn(rest, pivot, Quat::from_rotation_y(swing * (p.t * rate).sin())),
        Motion::Nod { pivot, swing, rate } if p.working => {
            turn(rest, pivot, Quat::from_rotation_z(swing * (p.t * rate).sin()))
        }
        Motion::Ride { pivot, swing, rate } if p.working => {
            let lift = (rest.translation.x - pivot.x) * (swing * (p.t * rate).sin()).sin();
            Transform { translation: rest.translation + Vec3::Y * lift, ..*rest }
        }
        Motion::Nod { .. } | Motion::Ride { .. } => *rest,
        Motion::Turret(pivot) => turn(rest, pivot, Quat::from_rotation_y(p.turret)),
        Motion::Barrel { pivot, trunnion } => {
            // Back along the bore, then laid about the trunnion, then turned with the turret.
            let back = Transform { translation: rest.translation - Vec3::Z * KICK * kick(p.fired), ..*rest };
            let laid = turn(&back, trunnion, Quat::from_rotation_x(-p.elevation));
            turn(&laid, pivot, Quat::from_rotation_y(p.turret))
        }
        Motion::Flag { pole, k, len } => flag(rest, pole, k, len, p),
        Motion::Wheel(radius) => {
            Transform { rotation: Quat::from_rotation_x((p.travel / radius).rem_euclid(TAU)) * rest.rotation, ..*rest }
        }
        Motion::Soldier { k, feet, limb } => soldier(rest, k, feet, limb, p),
    }
}

/// `tf` turned by `q` about `pivot`. No turn leaves it exactly as it is, so a part at rest
/// keeps its rest transform to the bit and `pose_parts` doesn't touch it.
fn turn(tf: &Transform, pivot: Vec3, q: Quat) -> Transform {
    if q == Quat::IDENTITY {
        return *tf;
    }
    Transform { translation: pivot + q * (tf.translation - pivot), rotation: q * tf.rotation, scale: tf.scale }
}

/// Segment `k` of a flag on a pole through `pole`, each segment `len` long: a chain from the
/// pole streaming downwind (`Pose::wind`), every joint swinging a little further than the one
/// before it in a wave that runs out to the fly, faster and wider in a gust; in a lull the
/// cloth sags toward the pole.
fn flag(rest: &Transform, pole: Vec3, k: u8, len: f32, p: &Pose) -> Transform {
    let rate = 4.0 + 5.0 * p.gust;
    let swing = 0.12 + 0.22 * p.gust;
    let sag = Quat::from_rotation_z(-(1.0 - p.gust) * 0.7);
    let joint = |j: u8| {
        let yaw = p.wind + swing * (j as f32 + 1.0) * (p.t * rate - j as f32 * 0.9).sin();
        Quat::from_rotation_y(yaw) * sag
    };
    let start = (0..k).fold(pole, |at, j| at + joint(j) * Vec3::X * len);
    let q = joint(k);
    Transform { translation: start + q * Vec3::X * (len / 2.0), rotation: q * rest.rotation, scale: rest.scale }
}

/// How far back a gun is, as a share of `KICK`, `s` seconds after it fired: all the way at
/// once, then running out again.
fn kick(s: f32) -> f32 {
    if s < 0.0 { 0.0 } else { (1.0 - s / KICK_SECS).max(0.0).powi(2) }
}

/// Soldier `k`'s part `limb`: walking, legs swing from the hip and the body bobs and leans
/// into the stride; standing, the body shifts its weight a little; attacking, the rifle comes
/// up to the shoulder, kicks as it fires, and the whole soldier faces the target. His squad
/// mates step and sway out of time with him.
fn soldier(rest: &Transform, k: u8, feet: Vec3, limb: Limb, p: &Pose) -> Transform {
    let t = p.t + k as f32 * 0.37;
    let leg = matches!(limb, Limb::LeftLeg | Limb::RightLeg);
    let mut tf = *rest;
    let stride = TAU * t * p.pace / STRIDE;
    if p.pace > 0.0 {
        if leg {
            let side = if limb == Limb::LeftLeg { 1.0 } else { -1.0 };
            let hip = Vec3::new(rest.translation.x, SOLDIER_HIP, rest.translation.z);
            tf = turn(&tf, hip, Quat::from_rotation_x(side * LEG_SWING * stride.sin()));
        } else {
            // Up at each footfall's push, twice a stride, leaning into the walk.
            tf.translation.y += 0.07 * (0.5 - 0.5 * (2.0 * stride).cos());
            tf = turn(&tf, feet + Vec3::Y * SOLDIER_HIP, Quat::from_rotation_x(0.12));
        }
    } else if !leg {
        let sway = 0.035 * (t * 1.1).sin();
        tf = turn(&tf, feet, Quat::from_rotation_z(sway));
        tf.translation.y += 0.015 * (t * 2.3).sin();
    }
    if p.aiming {
        let gun = match limb {
            Limb::Rifle => Some(Vec3::new(-0.1, 0.42, 0.15)),
            Limb::Launcher => Some(Vec3::ZERO),
            _ => None,
        };
        if let Some(up) = gun {
            tf.translation += up - Vec3::Z * 0.4 * KICK * kick(p.fired);
        }
        tf = turn(&tf, feet, Quat::from_rotation_y(p.aim));
    }
    tf
}

/// A soldier's part where he fell: it tips over about his feet from `since` (sim seconds), lies
/// there, then sinks into the ground over the last `rig::SINK_SECS` of `rig::BODY_SECS` and goes,
/// as the alpha's bodies do.
#[derive(Component)]
pub struct Fallen {
    since: f32,
    rest: Transform,
    feet: Vec3,
    axis: Vec3,
}

/// Leaves the parts of each soldier who went out of the sim in plain sight (a soldier leaves
/// it by dying, or when his side is out) where he stood, taken off the root `view.rs` is about
/// to despawn, to fall over (`lay_bodies`). Bodies are decoration, like wrecks.
#[allow(clippy::type_complexity)]
pub fn leave_bodies(
    mut commands: Commands,
    sim: Res<Sim>,
    fixed: Res<Time<Fixed>>,
    parts: Query<(Entity, &Moving, &Transform, &GlobalTransform, &InheritedVisibility)>,
    roots: Query<(&SimRef, &Drive)>,
) {
    let now = crate::rig::sim_seconds(&sim, &fixed);
    for (e, m, local, global, visible) in &parts {
        let Motion::Soldier { k, feet, .. } = m.motion else { continue };
        let Ok((r, d)) = roots.get(m.owner) else { continue };
        if sim.world.entity(r.0).is_some() || sim.remembers(r.0) || !visible.get() {
            continue;
        }
        // The model's frame in the world: the part's global placement without its own.
        let model = global.affine() * local.compute_affine().inverse();
        // Each falls his own way, mostly backwards, by his place in the squad and his squad's
        // phase.
        let a = std::f32::consts::PI + (k as f32 - 1.0) * 0.8 + (d.phase.fract() - 0.5) * 0.6;
        let away = model.transform_vector3(Vec3::new(a.sin(), 0.0, a.cos())).with_y(0.0).normalize_or(Vec3::NEG_Z);
        commands.entity(e).remove::<(ChildOf, Moving)>().insert((
            crate::menu::MatchEntity,
            global.compute_transform(),
            Fallen {
                since: now,
                rest: global.compute_transform(),
                feet: model.transform_point3(feet),
                axis: Vec3::Y.cross(away),
            },
        ));
    }
}

/// Tips each fallen soldier's parts over, as a body falls: slowly, then all at once; then they
/// lie, sink and go.
pub fn lay_bodies(
    mut commands: Commands,
    sim: Res<Sim>,
    fixed: Res<Time<Fixed>>,
    mut bodies: Query<(Entity, &Fallen, &mut Transform)>,
) {
    let now = crate::rig::sim_seconds(&sim, &fixed);
    for (e, f, mut tf) in &mut bodies {
        let age = now - f.since;
        if !(0.0..crate::rig::BODY_SECS).contains(&age) {
            commands.entity(e).despawn();
            continue;
        }
        let fall = FALL * (age / FALL_SECS).clamp(0.0, 1.0).powi(2);
        let mut at = turn(&f.rest, f.feet, Quat::from_axis_angle(f.axis, fall));
        let sunk = ((age - (crate::rig::BODY_SECS - crate::rig::SINK_SECS)) / crate::rig::SINK_SECS).clamp(0.0, 1.0);
        at.translation.y -= sunk * 1.2;
        tf.set_if_neq(at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Part;

    /// Every part of every model, in every faction, at rest transform.
    fn all_parts() -> Vec<Part> {
        let mut out = Vec::new();
        for def in g2_rules::rules().defs.iter() {
            for f in [g2_sim::Faction::Eu, g2_sim::Faction::Apa, g2_sim::Faction::Gla] {
                out.extend(crate::models::parts_for(def, Some(f)));
            }
            out.extend(crate::models::scaffold(def.radius.to_f32() * 1.75, 7.0));
        }
        out
    }

    /// A unit that stands, has driven nowhere, has no target and hasn't fired holds its rest
    /// pose: legs, wheels, treads, turrets, barrels, all of it; only a soldier's weight shift
    /// moves him, never his legs. Its rotors still turn; an idle building's fans, dish and
    /// pumpjack don't. A walking soldier swings his legs from the hip, opposite ways.
    #[test]
    fn a_standing_unit_holds_its_rest_pose() {
        let still = Pose { t: 12.3, fired: 1e3, unit: true, ..Pose::default() };
        let idle = Pose { unit: false, ..still };
        for p in all_parts() {
            let rest = p.transform();
            let at = place(p.motion, &rest, &still);
            match p.motion {
                Motion::Spin { .. } => assert_eq!(place(p.motion, &rest, &idle), rest, "{p:?} turns idle"),
                Motion::Slew { .. } | Motion::Flag { .. } => {}
                Motion::Soldier { limb: Limb::Body | Limb::Rifle | Limb::Launcher, .. } => {
                    assert!((at.translation - rest.translation).length() < 0.06, "{p:?} shifts too far: {at:?}");
                }
                _ => assert_eq!(at, rest, "{p:?} moves standing"),
            }
        }
        let walk = Pose { pace: 2.0, ..still };
        let legs: Vec<f32> = all_parts()
            .into_iter()
            .filter_map(|p| match p.motion {
                Motion::Soldier { limb: Limb::LeftLeg | Limb::RightLeg, .. } => {
                    Some(place(p.motion, &p.transform(), &walk).rotation.to_euler(EulerRot::XYZ).0)
                }
                _ => None,
            })
            .collect();
        assert!(legs.chunks(2).all(|pair| pair[0] * pair[1] < 0.0), "legs swing opposite ways: {legs:?}");
    }

    /// The tread under a tank that hasn't moved is on its first phase, and it steps on with the
    /// distance driven: one phase per `TREAD_PITCH / TREAD_PHASES`, so twice the speed runs it
    /// twice as fast, backwards when backing up. A wheel turns by the distance over its radius,
    /// its top going forward.
    #[test]
    fn treads_and_wheels_run_with_the_distance_driven() {
        let step = TREAD_PITCH / TREAD_PHASES as f32;
        assert_eq!(tread_phase(0.0), 0);
        assert_eq!(tread_phase(step * 1.5), 1);
        assert_eq!(tread_phase(step * 3.5), 3, "three times the distance, three times the run");
        assert_eq!(tread_phase(-step * 0.5), TREAD_PHASES - 1, "backing up runs it back");
        assert_eq!(tread_phase(TREAD_PITCH + step * 0.5), 0, "one link on, the same look");
        let wheel = crate::models::parts_for(
            g2_rules::rules().defs.iter().find(|d| d.name == "GLA Technical").unwrap(),
            Some(g2_sim::Faction::Gla),
        )
        .into_iter()
        .find(|p| matches!(p.motion, Motion::Wheel(_)))
        .unwrap();
        let Motion::Wheel(radius) = wheel.motion else { unreachable!() };
        let rest = wheel.transform();
        let pose = |travel: f32| Pose { travel, ..Pose::default() };
        assert_eq!(place(wheel.motion, &rest, &pose(0.0)), rest);
        // A quarter turn forward: the wheel's top point is now at its front.
        let top = |tf: Transform| tf.rotation * (rest.rotation.inverse() * Vec3::Y);
        let quarter = top(place(wheel.motion, &rest, &pose(radius * std::f32::consts::FRAC_PI_2)));
        assert!(quarter.z > 0.99, "rolled forward: {quarter:?}");
        let back = top(place(wheel.motion, &rest, &pose(-radius * std::f32::consts::FRAC_PI_2)));
        assert!(back.z < -0.99, "rolled back: {back:?}");
    }

    /// A shell's gun lays higher the further its shot, from about level close in up to the
    /// steepest it can go, and never past it; a direct-fire gun lays along the line to its
    /// target, about level on flat ground. Every artillery def's longest shot is laid steeper
    /// than its shortest, and a howitzer's longest is the steep lob the footage shows.
    #[test]
    fn barrel_elevation_rises_with_range_within_limits() {
        let rules = g2_rules::rules();
        let mut guns = 0;
        for def in rules.defs.iter() {
            let Some(shell) = def.weapon_for(true).and_then(|w| w.shell.map(|s| (w, s))) else { continue };
            let (w, s) = shell;
            let v = Some(s.speed.to_f32());
            let mut last = 0.0;
            let near = s.min_range.to_f32().max(4.0);
            for k in 0..=10 {
                let run = near + (w.range.to_f32() - near) * k as f32 / 10.0;
                let e = elevation(run, 0.0, v);
                assert!((0.0..=MAX_ELEVATION).contains(&e), "{}: {e} at {run}", def.name);
                assert!(e >= last - 0.02, "{}: {e} at {run} below {last}", def.name);
                last = e;
            }
            assert!(elevation(w.range.to_f32(), 0.0, v) > elevation(near, 0.0, v) + 0.2, "{}", def.name);
            guns += 1;
        }
        assert!(guns >= 5, "the howitzers, Thunderfist, Sunstorm, toxin and rocket artillery: {guns}");
        let howitzer = rules.def(rules.find("EU Howitzer").unwrap()).weapon().unwrap();
        let lob = elevation(howitzer.range.to_f32(), 0.0, howitzer.shell.map(|s| s.speed.to_f32()));
        assert!((0.6..=MAX_ELEVATION).contains(&lob), "a howitzer lobs its longest shot: {lob}");
        assert!(elevation(1e4, 0.0, Some(0.5)) <= MAX_ELEVATION, "capped");
        assert!(elevation(40.0, 0.0, None) < 0.05, "a tank's gun about level");
        assert!(elevation(20.0, 16.5, None) > 0.6, "raised at a helicopter overhead");
        assert_eq!(elevation(30.0, -20.0, None), 0.0, "never below level");
    }

    /// A laid barrel pitches up about its breech, its muzzle rising by its length times the
    /// sine of the elevation, and still turns with the turret; firing kicks it back along its
    /// bore. The breech stays where it was.
    #[test]
    fn a_laid_barrel_pitches_about_its_breech() {
        let def = g2_rules::rules().defs.iter().find(|d| d.name == "APA Thunderfist").unwrap();
        let parts = crate::models::parts_for(def, Some(g2_sim::Faction::Apa));
        let barrel = parts.iter().find(|p| matches!(p.motion, Motion::Barrel { .. })).unwrap();
        let Motion::Barrel { trunnion, .. } = barrel.motion else { unreachable!() };
        let rest = barrel.transform();
        let len = barrel.size.z;
        let laid = Pose { elevation: 0.8, fired: 1e3, ..Pose::default() };
        let at = place(barrel.motion, &rest, &laid);
        let muzzle = |tf: Transform| tf.translation + tf.rotation * Vec3::Z * len / 2.0;
        let breech = |tf: Transform| tf.translation - tf.rotation * Vec3::Z * len / 2.0;
        assert!((muzzle(at).y - muzzle(rest).y - len * 0.8f32.sin()).abs() < 1e-3);
        assert!(breech(at).distance(trunnion) < 1e-3, "{} vs {trunnion}", breech(at));
        let fired = place(barrel.motion, &rest, &Pose { fired: 0.0, ..laid });
        let back = at.translation - fired.translation;
        assert!((back.length() - KICK).abs() < 1e-3 && back.y > 0.0, "kicked back along the bore: {back}");
    }

    /// A flag's segments join end to end from the pole and fly downwind, out toward where the
    /// wind blows; they move from one moment to the next, and the same moment gives the same
    /// cloth.
    #[test]
    fn flags_fly_downwind_in_one_piece() {
        let def = g2_rules::rules().defs.iter().find(|d| d.name == "EU Barracks").unwrap();
        let parts = crate::models::parts_for(def, Some(g2_sim::Faction::Eu));
        let cloth: Vec<&Part> = parts.iter().filter(|p| matches!(p.motion, Motion::Flag { .. })).collect();
        assert!(cloth.len() >= 4, "the yard flag and the masts' are cloth strips");
        for wind in [0.0, 1.0, -2.5] {
            let p = Pose { t: 7.0, wind, gust: 0.8, ..Pose::default() };
            let mut end: Option<Vec3> = None;
            let first = cloth[0].motion;
            let same_pole = |c: &&&Part| matches!((c.motion, first), (Motion::Flag { pole: a, .. }, Motion::Flag { pole: b, .. }) if a == b);
            for part in cloth.iter().filter(same_pole) {
                let Motion::Flag { pole, k, len } = part.motion else { unreachable!() };
                let tf = place(part.motion, &part.transform(), &p);
                let dir = tf.rotation * Vec3::X;
                let start = tf.translation - dir * len / 2.0;
                assert!(start.distance(end.unwrap_or(pole)) < 1e-3, "segment {k} joins the last");
                end = Some(tf.translation + dir * len / 2.0);
                let downwind = Vec3::new(wind.cos(), 0.0, -wind.sin());
                assert!(dir.with_y(0.0).normalize().dot(downwind) > 0.5, "segment {k} flies downwind: {dir}");
            }
            assert!(end.is_some());
        }
        let flag = cloth[cloth.len() - 1];
        let at = |t: f32| place(flag.motion, &flag.transform(), &Pose { t, gust: 0.5, ..Pose::default() });
        assert_ne!(at(1.0), at(1.3), "it waves");
        assert_eq!(at(1.0), at(1.0));
    }

    /// A derrick's pumpjack stands still unless its well is paying, and nodding, the rod rides
    /// with the horse head above it.
    #[test]
    fn a_derrick_nods_only_while_it_pumps() {
        let def = g2_rules::rules().defs.iter().find(|d| d.name == "EU Oil Derrick").unwrap();
        let parts = crate::models::parts_for(def, Some(g2_sim::Faction::Eu));
        let moving: Vec<&Part> =
            parts.iter().filter(|p| matches!(p.motion, Motion::Nod { .. } | Motion::Ride { .. })).collect();
        assert!(moving.len() >= 3, "beam, head and rod");
        for t in [0.0, 0.4, 1.3, 7.9] {
            let idle = Pose { t, ..Pose::default() };
            for p in &moving {
                assert_eq!(place(p.motion, &p.transform(), &idle), p.transform(), "still without oil or power");
            }
        }
        let working = Pose { t: 0.6, working: true, ..Pose::default() };
        let lift = |p: &Part| place(p.motion, &p.transform(), &working).translation.y - p.at.y;
        let head = moving.iter().find(|p| p.slot == crate::models::Slot::Hazard).unwrap();
        let rod = moving.iter().find(|p| matches!(p.motion, Motion::Ride { .. })).unwrap();
        assert!(lift(head).abs() > 0.2, "the head nods: {}", lift(head));
        assert!((lift(rod) - lift(head)).abs() < 0.05, "the rod rides with it: {} {}", lift(rod), lift(head));
    }

    /// The mirror of a match at one tick shows every moving part in the same place however
    /// many frames were drawn before it, and a tick later, with the sim stepped, the parts of
    /// running fans and walking squads have moved. Poses come from the tick, not from frames.
    #[test]
    fn poses_come_from_the_tick_not_from_frames_drawn() {
        use crate::sim::{Hits, Previous};
        use g2_sim::{Faction, PlayerSetup};
        let make = |frames: usize| {
            let players = Sim::players([Faction::Eu, Faction::Gla], false).map(|p| PlayerSetup { ai: None, ..p });
            let mut sim = Sim::new(g2_rules::crater_ridge(), None, 1, players);
            sim.fog = false;
            sim.speed = 0;
            for _ in 0..30 {
                sim.world.step();
            }
            let mut app = App::new();
            app.add_plugins((MinimalPlugins, AssetPlugin::default()))
                .init_asset::<Mesh>()
                .init_asset::<StandardMaterial>()
                .init_asset::<Image>()
                .insert_resource(sim)
                .init_resource::<Previous>()
                .init_resource::<Hits>()
                .add_plugins(crate::view::ViewPlugin);
            for _ in 0..frames {
                app.update();
            }
            app
        };
        let poses = |app: &mut App| {
            let mut out: Vec<(EntityId, [f32; 7])> = app
                .world_mut()
                .query::<(&Moving, &Transform)>()
                .iter(app.world())
                .map(|(m, tf)| (m.owner, tf))
                .collect::<Vec<_>>()
                .into_iter()
                .map(|(owner, tf)| {
                    let id = app.world().get::<SimRef>(owner).unwrap().0;
                    let (t, r) = (tf.translation, tf.rotation);
                    (id, [t.x, t.y, t.z, r.x, r.y, r.z, r.w])
                })
                .collect();
            out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.partial_cmp(&b.1).unwrap()));
            out
        };
        let (mut once, mut often) = (make(2), make(9));
        let first = poses(&mut once);
        assert!(!first.is_empty(), "the starting forces have moving parts");
        assert_eq!(first, poses(&mut often), "the same tick, the same poses");
        once.world_mut().resource_mut::<Sim>().world.step();
        once.update();
        assert_ne!(first, poses(&mut once), "a tick later, something has moved");
    }
}
