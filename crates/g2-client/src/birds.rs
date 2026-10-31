//! Birds: a few small flocks wheel high over open ground away from the bases, as kites and
//! vultures do over a savanna. A shot or blast landing near one scatters it: every bird flies
//! off away from the bang, climbing, and the flock stays away for `AWAY_SECS` before it drifts
//! back and settles into its circle again.
//!
//! Decoration only, like `fx.rs`: nothing feeds back into the sim. Where a flock circles comes
//! from the map's name, and where each bird is from the sim time and the shots the sim reported
//! (`sim::Hits`, and a headless frame's `fx::Aftermath`), so a replay or a screenshot draws the
//! same birds. Each flock is one mesh of a few triangles a bird, rewritten every frame: one draw
//! call a flock, whatever its size.

use bevy::asset::RenderAssetUsages;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::sim::{Shot, Sim};

/// Birds in a flock.
pub const FLOCK: usize = 7;
/// Flocks on a map at most, one per `GROUND_PER_FLOCK` square units of it.
pub const MAX_FLOCKS: usize = 6;
const GROUND_PER_FLOCK: f32 = 60_000.0;
/// How near a bang must land to a flock's circle to scatter it: its radius and this much.
/// Our own, about the reach of the guns around it, so a flock never wheels over a fight.
pub const SCARE_REACH: f32 = 45.0;
/// Seconds a scattered flock flies off before it is out of sight, and stays away after the
/// last bang near it, and takes to come back.
pub const FLEE_SECS: f32 = 9.0;
pub const AWAY_SECS: f32 = 50.0;
pub const RETURN_SECS: f32 = 10.0;
/// Metres a second a fleeing bird flies outward, and climbs.
const FLEE_SPEED: f32 = 16.0;
const CLIMB: f32 = 2.5;
/// Wing span, metres: a kite's is 1.5-1.8; drawn a little large so it reads at play zoom.
const SPAN: f32 = 2.2;
/// How far a flock's circle keeps from a start: past the base's build area (`terrain::
/// BASE_SITE`, 52) with room for its defences' fire.
const BASE_CLEAR: f32 = 90.0;

/// One flock: where it circles, and when the bangs near it came.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Flock {
    pub home: Vec3,
    pub radius: f32,
    seed: u32,
    /// The latest scattering: when it began, from where (the bang), and how far out the birds
    /// already were (a flock scattered again as it came back starts from where it was).
    scare: Option<(f32, Vec3, f32)>,
    /// Sim seconds after which the flock comes back: `AWAY_SECS` after the last bang near it.
    back: f32,
}

impl Flock {
    pub fn new(home: Vec3, radius: f32, seed: u32) -> Flock {
        Flock { home, radius, seed, scare: None, back: f32::NEG_INFINITY }
    }

    /// A bang at `at` at sim time `t`. Bangs must come in time order.
    pub fn bang(&mut self, at: Vec3, t: f32) {
        if at.with_y(0.0).distance(self.home.with_y(0.0)) > self.radius + SCARE_REACH {
            return;
        }
        let out = self.out(t);
        // Already leaving or away: they stay away longer. Home or coming back: they scatter.
        let leaving = self.scare.is_some_and(|(start, _, _)| t < self.back.max(start + FLEE_SECS));
        if !leaving {
            self.scare = Some((t, at, out));
        }
        self.back = self.back.max(t + AWAY_SECS);
    }

    /// Metres the flock is out from its circle at time `t`: 0 at home, growing while it flees,
    /// shrinking back to 0 as it returns.
    fn out(&self, t: f32) -> f32 {
        let Some((start, _, from)) = self.scare else { return 0.0 };
        if t < start {
            return 0.0;
        }
        let far = from + FLEE_SPEED * (t - start).min(FLEE_SECS);
        if t < self.back {
            return far;
        }
        let gone = from + FLEE_SPEED * FLEE_SECS;
        (gone.min(far) * (1.0 - (t - self.back) / RETURN_SECS)).max(0.0)
    }

    /// Where bird `k` is at sim time `t`, and its wings' lift (-1 down to 1 up), or `None` while
    /// the flock is away out of sight.
    pub fn bird(&self, k: usize, t: f32) -> Option<(Vec3, Vec3, f32)> {
        let r = |n: u32| noise(self.seed.wrapping_add(k as u32 * 31 + n));
        // Each bird on its own circle, at its own height and pace, so the flock wheels loosely.
        let radius = self.radius * (0.55 + 0.45 * r(1));
        let pace = 0.16 + 0.08 * r(2);
        let a = r(3) * std::f32::consts::TAU + t * pace;
        let height = 14.0 + 6.0 * r(4) + 1.5 * (t * 0.4 + r(5) * 6.0).sin();
        let circling = Vec3::new(a.cos(), 0.0, a.sin()) * radius;
        let mut at = self.home + circling + Vec3::Y * height;
        let mut heading = Vec3::new(-a.sin(), 0.0, a.cos());
        let out = self.out(t);
        let mut flap = 0.35;
        if let Some((_, bang, _)) = self.scare.filter(|_| out > 0.0) {
            if t >= self.scare.map_or(t, |(start, _, _)| start + FLEE_SECS) && t < self.back {
                return None;
            }
            // Away from the bang, each bird a little off the line, so the flock breaks up.
            let from = (self.home + circling - bang).with_y(0.0).normalize_or(Vec3::X);
            let spread = (r(6) - 0.5) * 1.2;
            let dir = Quat::from_rotation_y(spread) * from;
            at += dir * out + Vec3::Y * CLIMB * out / FLEE_SPEED;
            heading = if t < self.back { dir } else { -dir };
            flap = 1.0;
        }
        // Mostly gliding, a few beats now and then; flapping hard while fleeing.
        let beat = (t * 9.0 + r(7) * 40.0).sin();
        let soar = (t * 0.7 + r(8) * 9.0).sin().max(0.0);
        let lift = if flap >= 1.0 { beat } else { beat * flap * soar };
        Some((at, heading, lift))
    }
}

fn mix(mut h: u32) -> u32 {
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^ (h >> 15)
}

/// Deterministic noise in 0..1 from `n`.
fn noise(n: u32) -> f32 {
    (mix(n.wrapping_mul(0x9e37_79b9)) & 0xffff) as f32 / 65_535.0
}

/// Where the flocks circle on a map `width` × `height` with starts `bases` and resources
/// `spots`: over open ground away from both, picked from a grid by a hash of the map's `seed`,
/// so a map always has the same ones.
pub fn homes(width: f32, height: f32, bases: &[Vec3], spots: &[Vec3], seed: u32) -> Vec<(Vec3, f32)> {
    let want = ((width * height / GROUND_PER_FLOCK) as usize).clamp(1, MAX_FLOCKS);
    let mut out: Vec<(Vec3, f32)> = Vec::new();
    let step = 24.0;
    let (nx, nz) = ((width / step) as i32, (height / step) as i32);
    let mut cells: Vec<(u32, Vec3)> = (0..nz)
        .flat_map(|j| (0..nx).map(move |i| (i, j)))
        .map(|(i, j)| {
            let h = (i as u32).wrapping_mul(0x9e37_79b1) ^ (j as u32).wrapping_mul(0x85eb_ca6b) ^ seed;
            (mix(h), Vec3::new((i as f32 + 0.5) * step, 0.0, (j as f32 + 0.5) * step))
        })
        .collect();
    cells.sort_by_key(|&(h, _)| h);
    for (h, at) in cells {
        let radius = 14.0 + (h % 10) as f32;
        let margin = radius + 20.0;
        let inside = at.x > margin && at.z > margin && at.x < width - margin && at.z < height - margin;
        let clear = bases.iter().all(|b| b.with_y(0.0).distance(at) > BASE_CLEAR)
            && spots.iter().all(|s| s.with_y(0.0).distance(at) > 30.0)
            && out.iter().all(|(o, _)| o.distance(at) > 90.0);
        if inside && clear {
            out.push((at, radius));
            if out.len() == want {
                break;
            }
        }
    }
    out
}

/// Puts the map's flocks up, each with its own mesh. Runs with each new scene.
pub fn setup(
    mut commands: Commands,
    sim: Res<Sim>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let map = &sim.world.map;
    let p = |v: g2_sim::Vec2| crate::sim::to_world(v);
    let bases: Vec<Vec3> = map.spawns.iter().map(|&s| p(s)).collect();
    let spots: Vec<Vec3> = map.supply_piles.iter().chain(map.oil_wells.iter()).map(|&(s, _)| p(s)).collect();
    let seed = map.name.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193));
    // Dark against the pale ground, as birds high over it read; unlit, a silhouette.
    let look = materials.add(StandardMaterial {
        base_color: Color::srgb(0.13, 0.11, 0.1),
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    for (k, (at, radius)) in
        homes(map.width.to_f32(), map.height.to_f32(), &bases, &spots, seed).into_iter().enumerate()
    {
        let home = at.with_y(sim.ground_y(at.x, at.z));
        commands.spawn((
            crate::menu::MatchEntity,
            Flock::new(home, radius, seed ^ (k as u32 * 0x2545_f491)),
            Mesh3d(meshes.add(flock_mesh(&[]))),
            MeshMaterial3d(look.clone()),
            Transform::default(),
            Visibility::Hidden,
            NotShadowCaster,
            NotShadowReceiver,
            // The mesh is rewritten every frame; its bounds would lag the birds.
            bevy::camera::visibility::NoFrustumCulling,
        ));
    }
}

/// One bird's triangles: a slim body along `heading` and two wings hinged at its shoulders,
/// their tips raised or lowered by `lift`.
fn bird_triangles(at: Vec3, heading: Vec3, lift: f32, out: &mut Vec<[f32; 3]>) {
    let fwd = heading.normalize_or(Vec3::Z);
    let side = Vec3::Y.cross(fwd).normalize_or(Vec3::X);
    let half = SPAN / 2.0;
    let nose = at + fwd * 0.45;
    let tail = at - fwd * 0.5;
    let up = Vec3::Y * lift * half * 0.55;
    for s in [-1.0, 1.0] {
        let shoulder = at + side * s * 0.12;
        let tip = at + side * s * half + up - fwd * 0.2;
        let trailing = at + side * s * half * 0.45 + up * 0.45 - fwd * 0.3;
        out.extend([shoulder + fwd * 0.18, tip, trailing].map(|v| v.to_array()));
        out.extend([shoulder + fwd * 0.18, trailing, shoulder - fwd * 0.2].map(|v| v.to_array()));
    }
    out.extend([nose, at + side * 0.12, tail].map(|v| v.to_array()));
    out.extend([nose, tail, at - side * 0.12].map(|v| v.to_array()));
}

/// A mesh of the given triangle corners (three per triangle), facing up.
fn flock_mesh(corners: &[[f32; 3]]) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    write_mesh(&mut mesh, corners.to_vec());
    mesh
}

fn write_mesh(mesh: &mut Mesh, corners: Vec<[f32; 3]>) {
    let n = corners.len();
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; n]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, corners);
    mesh.insert_indices(Indices::U32((0..n as u32).collect()));
}

/// Scatters the flocks the latest bangs landed near, then draws every bird where it is now.
/// Runs before `view::spawn_effects` drains the shots and the aftermath: a headless frame's
/// `fx::Aftermath` (the last 45 s of blasts and kills) scatters them too, so a still of a
/// battle shows no birds over it.
pub fn fly(
    sim: Res<Sim>,
    fixed: Res<Time<Fixed>>,
    hits: Res<crate::sim::Hits>,
    aftermath: Option<Res<crate::fx::Aftermath>>,
    mut flocks: Query<(&mut Flock, &Mesh3d, &mut Visibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let now = crate::rig::sim_seconds(&sim, &fixed);
    let bang = |s: &Shot| match *s {
        Shot::Hit { at, .. } | Shot::Blast { at, .. } => Some(crate::sim::to_world(at)),
        _ => None,
    };
    let past = aftermath.iter().flat_map(|a| a.0.iter());
    let bangs: Vec<(Vec3, f32)> = past
        .filter_map(|(s, age)| bang(s).map(|at| (at, now - age)))
        .chain(hits.0.iter().filter_map(|s| bang(s).map(|at| (at, now))))
        .collect();
    for (mut flock, mesh, mut vis) in &mut flocks {
        for &(at, t) in &bangs {
            flock.bang(at, t);
        }
        let mut corners = Vec::with_capacity(FLOCK * 18);
        for k in 0..FLOCK {
            if let Some((at, heading, lift)) = flock.bird(k, now) {
                bird_triangles(at, heading, lift, &mut corners);
            }
        }
        // Birds over ground the player hasn't explored would give it away.
        let show = !corners.is_empty() && sim.explored_at(flock.home.x, flock.home.z);
        vis.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
        if show && let Some(mut m) = meshes.get_mut(&mesh.0) {
            write_mesh(&mut m, corners);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flock() -> Flock {
        Flock::new(Vec3::new(200.0, 0.0, 200.0), 20.0, 7)
    }

    fn spread(f: &Flock, t: f32) -> Option<f32> {
        let birds: Vec<Vec3> = (0..FLOCK).filter_map(|k| f.bird(k, t).map(|b| b.0)).collect();
        (!birds.is_empty()).then(|| birds.iter().map(|b| b.with_y(0.0).distance(f.home)).fold(0.0, f32::max))
    }

    /// An undisturbed flock wheels over its circle, high up, the same way at the same time.
    #[test]
    fn a_quiet_flock_circles_its_home() {
        let f = flock();
        for t in [0.0, 13.7, 600.0] {
            assert!(spread(&f, t).unwrap() <= f.radius + 0.01);
            for k in 0..FLOCK {
                let (at, _, lift) = f.bird(k, t).unwrap();
                assert!(at.y > 10.0 && (-1.0..=1.0).contains(&lift));
                assert_eq!(f.bird(k, t), flock().bird(k, t), "from the time alone");
            }
        }
    }

    /// A blast near the flock scatters it: every bird gets further from the blast as it flees,
    /// none is drawn while it is away, and it is back over its circle once the quiet has lasted
    /// `AWAY_SECS` and its flight back. A blast far away leaves it be.
    #[test]
    fn birds_keep_away_from_recent_blasts() {
        let mut f = flock();
        let blast = Vec3::new(215.0, 0.0, 190.0);
        f.bang(Vec3::new(600.0, 0.0, 600.0), 5.0);
        assert_eq!(f, flock(), "a blast far off");
        f.bang(blast, 10.0);
        let near = |t: f32| {
            (0..FLOCK)
                .filter_map(|k| f.bird(k, t))
                .map(|b| b.0.with_y(0.0).distance(blast))
                .fold(f32::INFINITY, f32::min)
        };
        let mut last = near(10.0);
        for k in 1..=8 {
            let d = near(10.0 + k as f32);
            assert!(d > last, "fleeing: {d} after {last}");
            last = d;
        }
        assert!(last > 100.0, "well away after 8 s: {last}");
        for t in [20.0, 40.0, 59.9] {
            assert_eq!(spread(&f, t), None, "away at {t}");
        }
        // A second blast while it is away keeps it away longer.
        f.bang(blast, 40.0);
        assert_eq!(spread(&f, 70.0), None, "still away after the second blast");
        let home = 40.0 + AWAY_SECS + RETURN_SECS;
        assert!(spread(&f, home + 0.1).unwrap() <= f.radius + 0.01, "back over its circle");
    }

    /// The flocks keep clear of the starts and resources, inside the map, apart from each
    /// other, and come out the same for the same map.
    #[test]
    fn flocks_circle_open_ground() {
        let bases = [Vec3::new(60.0, 0.0, 60.0), Vec3::new(440.0, 0.0, 440.0)];
        let spots = [Vec3::new(250.0, 0.0, 250.0)];
        let a = homes(500.0, 500.0, &bases, &spots, 99);
        assert_eq!(a, homes(500.0, 500.0, &bases, &spots, 99));
        assert!(!a.is_empty() && a.len() <= MAX_FLOCKS);
        for (i, &(at, r)) in a.iter().enumerate() {
            assert!(bases.iter().all(|b| b.distance(at) > BASE_CLEAR), "{at}");
            assert!(spots[0].distance(at) > 30.0);
            assert!(at.x > r && at.z > r && at.x < 500.0 - r && at.z < 500.0 - r);
            assert!(a[..i].iter().all(|(o, _)| o.distance(at) > 90.0));
        }
    }
}
