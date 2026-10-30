//! Mirrors sim entities as procedural models (`models.rs`), or, when the player imported their
//! copy of the 2013 alpha (`alpha.rs`), as the alpha's own models.
//!
//! Each sim entity gets one root (`SimRef` + `Transform` at its ground position) with the model's
//! parts as children. Foundations show the scaffold with the building rising inside it.
//! Overlays (collector counts, health bars) are drawn by `hud.rs`; `overlay_anchor` and
//! `depot_label` supply their position and data.

use std::collections::BTreeMap;

use bevy::prelude::*;
use g2_sim::{BuildingRole, Class, EntityId, Faction};

use crate::models::{self, Part, Prim, Slot};
use crate::sim::{Previous, Sim, to_world};

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

/// A dark ring under an own stealth unit, shown while it is hidden from the enemy.
#[derive(Component)]
struct StealthHalo;

#[derive(Component)]
struct Rotor;

/// What a kill leaves behind: it sits on the ground for `life` seconds, then sinks out of sight
/// over the last `SINK_SECS` and goes.
#[derive(Component)]
struct Wreck {
    age: f32,
    life: f32,
    depth: f32,
    ground: f32,
}

impl Wreck {
    /// Lowers `tf` as far as the wreck has sunk by its age (not at all before its last
    /// `SINK_SECS`).
    fn sink(&self, tf: &mut Transform) {
        let sunk = ((self.age - (self.life - SINK_SECS)) / SINK_SECS).clamp(0.0, 1.0);
        if sunk > 0.0 {
            tf.translation.y =
                self.ground - self.depth * sunk + (tf.translation.y - self.ground).max(0.0) * (1.0 - sunk);
        }
    }
}

const SINK_SECS: f32 = 6.0;
/// Unit and building wrecks stay this long.
const UNIT_WRECK_SECS: f32 = 25.0;
const BUILDING_WRECK_SECS: f32 = 70.0;

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
    /// Every slot but `Team` per faction (`None` for neutral resources).
    materials: BTreeMap<(Option<Faction>, SlotKey), Handle<StandardMaterial>>,
    /// `Slot::Team` per `models::TEAM_PALETTE` entry.
    team: Vec<Handle<StandardMaterial>>,
    /// Rubble left where something died (its scorch mark is `fx::Palette::scorch`).
    rubble: Handle<StandardMaterial>,
    /// A toxin cloud's low green haze.
    toxin: Handle<StandardMaterial>,
    /// Imported alpha models by def name: scene and the offset that puts its footprint's
    /// centre on the entity.
    alpha: BTreeMap<&'static str, (Handle<WorldAsset>, Vec3)>,
    /// A flat unit-radius ring, the team marker under alpha models.
    ring: Handle<Mesh>,
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
        app.init_resource::<Mirror>().add_systems(
            Update,
            (
                setup_scene.run_if(crate::menu::scene_start),
                sync_entities,
                interpolate,
                spin_rotors,
                crate::fx::setup.run_if(crate::menu::scene_start),
                spawn_effects,
                crate::fx::age,
                crate::fx::face_camera,
                age_wrecks,
                sync_clouds,
            )
                .chain()
                .after(crate::menu::MenuSet),
        );
    }
}

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    cache: Option<Res<crate::alpha::Cache>>,
    server: Res<AssetServer>,
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
    let material = |base_color: Color, slot: Slot| {
        let (roughness, metallic, emissive) = match slot {
            Slot::Glow => (0.3, 0.0, base_color.to_linear() * 1.5),
            Slot::Oil => (0.08, 0.0, LinearRgba::BLACK),
            Slot::Hull | Slot::Panel | Slot::Dark => (0.55, 0.35, LinearRgba::BLACK),
            _ => (0.8, 0.0, LinearRgba::BLACK),
        };
        StandardMaterial { base_color, perceptual_roughness: roughness, metallic, emissive, ..default() }
    };
    for slot in all_slots.filter(|&s| s != Slot::Team) {
        for f in factions {
            table.insert((f, SlotKey::from(slot)), materials.add(material(models::color(f, slot), slot)));
        }
    }
    let team = models::TEAM_PALETTE.iter().map(|&c| materials.add(material(c, Slot::Team))).collect();
    commands.insert_resource(Assets3d {
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        cylinder: meshes.add(Cylinder::new(0.5, 1.0)),
        sphere: meshes.add(Sphere::new(0.5).mesh().ico(2).unwrap()),
        materials: table,
        team,
        rubble: materials.add(StandardMaterial {
            base_color: Color::srgb(0.16, 0.14, 0.12),
            perceptual_roughness: 0.95,
            ..default()
        }),
        toxin: materials.add(StandardMaterial {
            base_color: Color::srgba(0.4, 0.75, 0.15, 0.22),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
        alpha: cache.map_or_else(BTreeMap::new, |c| alpha_models(&c, &server)),
        ring: meshes.add(Torus::new(0.94, 1.0)),
    });
    crate::look::spawn_lights(&mut commands);
}

/// Spawns `parts` in `faction`'s materials, with team-coloured parts in palette entry `team`.
fn spawn_parts(
    parent: &mut ChildSpawnerCommands,
    assets: &Assets3d,
    faction: Option<Faction>,
    team: usize,
    parts: &[Part],
) {
    for p in parts {
        let mesh = match p.prim {
            Prim::Box => assets.cube.clone(),
            Prim::Cylinder => assets.cylinder.clone(),
            Prim::Sphere => assets.sphere.clone(),
        };
        let material = match p.slot {
            Slot::Team => assets.team[team].clone(),
            slot => assets.materials[&(faction, SlotKey::from(slot))].clone(),
        };
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
    let team = e.owner.map_or(0, |o| sim.teams[o.0 as usize]);
    let alpha = assets.alpha.get(def.name);
    let parts = if alpha.is_some() { Vec::new() } else { models::parts_for(def, faction) };
    let mut root = commands.spawn((
        crate::menu::MatchEntity,
        SimRef(e.id),
        Transform::from_translation(sim.to_world(e.pos) + Vec3::Y * flight_height(def)),
        Visibility::default(),
    ));
    root.with_children(|c| {
        c.spawn((Body, Transform::default(), Visibility::default())).with_children(|b| match alpha {
            Some((scene, offset)) => {
                // The alpha's models carry no team colour: a thin ring on the ground in the
                // team's colour keeps the sides apart, as the alpha's selection rings did.
                // Civilian buildings change hands, so they have none; a garrison's pips show
                // who holds one.
                let r = def.radius.to_f32();
                if def.role() != Some(g2_sim::BuildingRole::Civilian) {
                    b.spawn((
                        Mesh3d(assets.ring.clone()),
                        MeshMaterial3d(assets.team[team].clone()),
                        Transform::from_xyz(0.0, 0.08, 0.0).with_scale(Vec3::new(r, 1.0, r)),
                    ));
                }
                // A squad is three soldiers in the procedural model's wedge.
                let spots: &[Vec3] = if def.armour == g2_sim::Armour::Infantry {
                    &[Vec3::new(0.0, 0.0, 0.7), Vec3::new(-0.8, 0.0, -0.4), Vec3::new(0.8, 0.0, -0.4)]
                } else {
                    &[Vec3::ZERO]
                };
                for &at in spots {
                    b.spawn((WorldAssetRoot(scene.clone()), Transform::from_translation(at + *offset)));
                }
            }
            None => spawn_parts(b, assets, faction, team, &parts),
        });
        if def.is_building() {
            let w = def.radius.to_f32() * 1.75;
            let scaffold: Vec<Part> = models::scaffold(w, 7.0);
            c.spawn((Scaffold, Transform::default(), Visibility::Hidden))
                .with_children(|s| spawn_parts(s, assets, faction, team, &scaffold));
        }
        if def.deploy.is_some() && alpha.is_none() {
            let legs = models::outriggers(def.radius.to_f32());
            c.spawn((Outriggers, Transform::default(), Visibility::Hidden))
                .with_children(|s| spawn_parts(s, assets, faction, team, &legs));
        }
        // Stealth units, and any unit a cloaking power may cover: a dark ring on the ground
        // while hidden, so their owner sees which ones the enemy can't.
        if def.is_unit() && !def.is_support_jet() {
            let r = def.radius.to_f32() * 1.6;
            c.spawn((
                StealthHalo,
                Mesh3d(assets.ring.clone()),
                MeshMaterial3d(assets.materials[&(Some(Faction::Gla), SlotKey::from(Slot::Dark))].clone()),
                Transform::from_xyz(0.0, 0.12, 0.0).with_scale(Vec3::new(r, 1.0, r)),
                Visibility::Hidden,
            ));
        }
    });
    root.id()
}

/// The alpha's models for our defs, at the alpha's own size: our footprints are its blueprints'
/// (`g2-rules`: `EntityRadius`, build-grid half extents), in the same units as its meshes, so
/// every model keeps the size it has next to every other, a tank against a war factory as in
/// the alpha. Each is moved so the centre of its footprint sits on the entity: the alpha's
/// origins are often off-centre. Height is left alone: the alpha's origin is at ground level,
/// and parts below it (a derrick's well, 3.2 m down) belong under the ground.
fn alpha_models(
    cache: &crate::alpha::Cache,
    server: &AssetServer,
) -> BTreeMap<&'static str, (Handle<WorldAsset>, Vec3)> {
    let bounds = cache.bounds();
    let mut out = BTreeMap::new();
    for def in g2_rules::rules().defs {
        let Some((path, (lo, hi))) = cache.model(def, &bounds) else { continue };
        let centre = (lo + hi) / 2.0;
        let offset = Vec3::new(-centre.x, 0.0, -centre.z);
        let handle =
            server.load(GltfAssetLabel::Scene(0).from_asset(bevy::asset::AssetPath::from(path).with_source("alpha")));
        out.insert(def.name, (handle, offset));
    }
    out
}

/// Whether every imported alpha model is loaded (true without an import). A model that
/// failed to load counts as not loaded, so a broken cache fails the headless check.
pub fn alpha_models_loaded(world: &World) -> bool {
    let (Some(assets), Some(server)) = (world.get_resource::<Assets3d>(), world.get_resource::<AssetServer>()) else {
        return true;
    };
    assets.alpha.values().all(|(scene, ..)| server.is_loaded_with_dependencies(scene))
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
    mut halos: Query<&mut Visibility, (With<StealthHalo>, Without<SimRef>, Without<Scaffold>, Without<Outriggers>)>,
    mut root_tf: Query<
        (&mut Transform, &mut Visibility),
        (With<SimRef>, Without<Body>, Without<Scaffold>, Without<Outriggers>, Without<StealthHalo>),
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
        // Hidden from the enemy right now: the owner's ring shows it.
        let hidden = e.owner == Some(sim.local) && world.hidden(e);
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
            if let Ok(mut v) = halos.get_mut(child) {
                v.set_if_neq(if hidden { Visibility::Inherited } else { Visibility::Hidden });
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
        // terrain between ticks instead of cutting through it. Aircraft ride above it.
        let now = to_world(e.pos);
        let before = prev.0.get(&r.0).map_or(now, |&p| to_world(p));
        let mut at = before.lerp(now, alpha);
        at.y = sim.ground_y(at.x, at.z) + flight_height(sim.world.rules.def(e.def));
        tf.translation = at;
        let heading = now - before;
        if heading.length_squared() > 1e-6 {
            tf.rotation = Quat::from_rotation_y(heading.x.atan2(heading.z));
        }
    }
}

/// How high an aircraft flies above the ground: the alpha helicopters' `AirCeiling` (16.5).
pub const FLIGHT_HEIGHT: f32 = 16.5;

/// How high `def` flies above the ground: `FLIGHT_HEIGHT` for aircraft, 0 for ground units.
pub fn flight_height(def: &g2_sim::Def) -> f32 {
    if def.flies() { FLIGHT_HEIGHT } else { 0.0 }
}

fn spin_rotors(time: Res<Time>, mut q: Query<&mut Transform, With<Rotor>>) {
    for mut tf in &mut q {
        tf.rotate_y(time.delta_secs() * 18.0);
    }
}

/// Combat effects (`fx.rs`) for every shot since the last frame, and the wreck of every kill;
/// then the battlefield's own life (`fx::live`): damage smoke, wreck smoke, dust, tread marks
/// and steaming stacks.
#[allow(clippy::too_many_arguments)]
fn spawn_effects(
    mut commands: Commands,
    assets: Res<Assets3d>,
    palette: Option<Res<crate::fx::Palette>>,
    living: Option<ResMut<crate::fx::Living>>,
    aftermath: Option<ResMut<crate::fx::Aftermath>>,
    sim: Res<Sim>,
    time: Res<Time>,
    frozen: Option<Res<FreezeEffects>>,
    mirror: Res<Mirror>,
    mut hits: ResMut<crate::sim::Hits>,
    roots: Query<&Transform, With<SimRef>>,
    ambient: Query<(), With<crate::fx::Ambient>>,
) {
    let (Some(palette), Some(mut living)) = (palette, living) else { return };
    if let Some(mut aftermath) = aftermath {
        let past = std::mem::take(&mut aftermath.0);
        for (at, def, age) in crate::fx::replay_aftermath(&mut commands, &palette, &sim, &mut living, past) {
            spawn_wreck(&mut commands, &assets, &palette, &sim, &mut living, at, def, age);
        }
    }
    for (at, def) in crate::fx::spawn_shots(&mut commands, &palette, &sim, &mut living, hits.0.drain(..)) {
        spawn_wreck(&mut commands, &assets, &palette, &sim, &mut living, at, def, 0.0);
    }
    let dt = if sim.speed == 0 { 0.0 } else { time.delta_secs() };
    let root =
        |id: EntityId| mirror.0.get(&id).and_then(|&e| roots.get(e).ok()).map(|tf| (tf.translation, tf.rotation));
    crate::fx::live(
        &mut commands,
        &palette,
        &sim,
        &mut living,
        dt,
        frozen.is_some(),
        &|name| assets.alpha.contains_key(name),
        &root,
        ambient.iter().count(),
    );
}

/// Leaves a scorch mark and a few tumbled slabs where a unit or building died, `age` seconds
/// ago, and sets it smouldering (`fx::Living::wreck`). Wrecks are decoration only: they are
/// not sim entities, never block anything and are not hashed. Their shapes come from the spot,
/// so a replay or a second viewer draws the same ones.
#[allow(clippy::too_many_arguments)]
fn spawn_wreck(
    commands: &mut Commands,
    assets: &Assets3d,
    palette: &crate::fx::Palette,
    sim: &Sim,
    living: &mut crate::fx::Living,
    at: g2_sim::Vec2,
    def: g2_sim::DefId,
    age: f32,
) {
    let d = sim.world.rules.def(def);
    let building = d.is_building();
    let r = d.radius.to_f32().max(1.5);
    let ground = sim.ground_y(at.x.to_f32(), at.y.to_f32());
    let origin = sim.to_world(at).with_y(ground);
    let life = if building { BUILDING_WRECK_SECS } else { UNIT_WRECK_SECS };
    if age >= life {
        return;
    }
    living.wreck(origin, r, building, age, life);
    let seed = (at.x.0 as u32).wrapping_mul(0x9e37_79b1) ^ (at.y.0 as u32).wrapping_mul(0x85eb_ca6b);
    let rnd = |k: u32| {
        let mut h = seed ^ k.wrapping_mul(0xc2b2_ae35);
        h ^= h >> 15;
        h = h.wrapping_mul(0x2c1b_3c6d);
        h ^= h >> 12;
        (h & 0xffff) as f32 / 65_535.0
    };
    let wreck = |depth: f32| Wreck { age, life, depth, ground };
    // The scorch: a soft charred patch on the ground, wider than what died, at its own angle.
    let mut tf = Transform::from_translation(origin + Vec3::Y * 0.08)
        .with_rotation(Quat::from_rotation_y(rnd(97) * std::f32::consts::TAU))
        .with_scale(Vec3::new(r * 3.4, 1.0, r * 3.4));
    wreck(0.3).sink(&mut tf);
    commands.spawn((
        crate::menu::MatchEntity,
        Mesh3d(palette.decal.clone()),
        MeshMaterial3d(palette.scorch.clone()),
        tf,
        bevy::light::NotShadowCaster,
        wreck(0.3),
    ));
    let slabs = if building { 5 } else { 2 };
    for k in 0..slabs {
        let (a, dist) = (rnd(k * 3) * std::f32::consts::TAU, rnd(k * 3 + 1) * r * 0.8);
        let size =
            Vec3::new(0.5 + rnd(k * 3 + 2) * r * 0.5, 0.25 + rnd(k * 5 + 7) * 0.5, 0.5 + rnd(k * 7 + 3) * r * 0.4);
        let mut tf = Transform::from_translation(origin + Vec3::new(a.cos() * dist, size.y * 0.35, a.sin() * dist))
            .with_rotation(Quat::from_euler(
                EulerRot::YXZ,
                rnd(k * 11) * std::f32::consts::TAU,
                (rnd(k * 13) - 0.5) * 0.4,
                (rnd(k * 17) - 0.5) * 0.4,
            ))
            .with_scale(size);
        wreck(size.y + 0.4).sink(&mut tf);
        commands.spawn((
            crate::menu::MatchEntity,
            Mesh3d(assets.cube.clone()),
            MeshMaterial3d(assets.rubble.clone()),
            tf,
            wreck(size.y + 0.4),
        ));
    }
}

/// A drawn toxin cloud, by its centre.
#[derive(Component)]
struct CloudHaze(g2_sim::Vec2);

/// One flat green haze per active toxin cloud the local player can see, rebuilt whenever the
/// set of active clouds changes.
fn sync_clouds(
    mut commands: Commands,
    sim: Res<Sim>,
    assets: Option<Res<Assets3d>>,
    drawn: Query<(Entity, &CloudHaze)>,
) {
    let Some(assets) = assets else { return };
    let world = &sim.world;
    let vision = world.vision(sim.local);
    let active: Vec<_> =
        world.clouds().iter().filter(|c| c.from <= world.tick() && (!sim.fog || vision.is_visible(c.at))).collect();
    if active.len() == drawn.iter().count() && drawn.iter().all(|(_, h)| active.iter().any(|c| c.at == h.0)) {
        return;
    }
    for (e, _) in &drawn {
        commands.entity(e).despawn();
    }
    for c in active {
        let r = c.radius.to_f32();
        commands.spawn((
            crate::menu::MatchEntity,
            CloudHaze(c.at),
            Mesh3d(assets.cylinder.clone()),
            MeshMaterial3d(assets.toxin.clone()),
            Transform::from_translation(sim.to_world(c.at) + Vec3::Y * 0.6).with_scale(Vec3::new(
                r * 2.0,
                1.2,
                r * 2.0,
            )),
        ));
    }
}

/// Wrecks sit still, then sink into the ground over their last seconds and go. Frozen with the
/// effects in screenshot mode.
fn age_wrecks(
    mut commands: Commands,
    time: Res<Time>,
    sim: Res<Sim>,
    frozen: Option<Res<FreezeEffects>>,
    mut q: Query<(Entity, &mut Wreck, &mut Transform)>,
) {
    if frozen.is_some() || sim.speed == 0 {
        return;
    }
    for (id, mut w, mut tf) in &mut q {
        w.age += time.delta_secs() * sim.speed as f32;
        if w.age >= w.life {
            commands.entity(id).despawn();
            continue;
        }
        w.sink(&mut tf);
    }
}

/// World-space anchor for overlays above an entity.
pub fn overlay_anchor(sim: &Sim, e: &g2_sim::Entity) -> Vec3 {
    let def = sim.world.rules.def(e.def);
    let height = match def.class {
        Class::Building { role: BuildingRole::CommandCenter, .. } => 12.0,
        // Civilian buildings are low: a shanty is 2.5 tall, a town house 6, a block 9.
        Class::Building { role: BuildingRole::Civilian, .. } => 2.0 + def.radius.to_f32() * 1.5,
        Class::Building { .. } => 8.0,
        Class::Unit { collector: Some(_), .. }
            if e.owner.is_some_and(|o| sim.world.player(o).faction == Faction::Eu) =>
        {
            10.0
        }
        _ => 4.0,
    };
    sim.to_world(e.pos) + Vec3::Y * (height + flight_height(def))
}

/// Whether the local player should see a collector-count label on this entity.
pub fn depot_label(sim: &Sim, e: &g2_sim::Entity) -> Option<(u32, u32)> {
    match sim.world.rules.def(e.def).role() {
        Some(BuildingRole::SupplyCenter { collector_cap }) if e.owner == Some(sim.local) && e.is_complete() => {
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
        let players = Sim::players([Faction::Eu, Faction::Gla], false).map(|p| PlayerSetup { ai: None, ..p });
        let sim = Sim::new(g2_rules::crater_ridge(), None, 1, players);
        let enemy = PlayerId(1);
        let rules = sim.world.rules;
        let first = |s: &Sim, owner, pred: fn(&g2_sim::Def) -> bool| {
            s.world.entities().find(|e| e.owner == Some(owner) && pred(rules.def(e.def))).unwrap().id
        };
        let dozer = first(&sim, sim.local, |d| d.is_constructor());
        let cc = first(&sim, enemy, |d| d.is_building());
        let (home, cc_pos) = (sim.world.entity(dozer).unwrap().pos, sim.world.entity(cc).unwrap().pos);
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Image>()
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

    /// A kill leaves a scorch mark and rubble where it happened, a plain hit leaves nothing; the
    /// wreckage sits on the ground, then sinks out of sight and goes, and never touches the sim.
    #[test]
    fn a_kill_leaves_wreckage_that_sinks_and_goes() {
        let players = Sim::players([Faction::Eu, Faction::Gla], false).map(|p| PlayerSetup { ai: None, ..p });
        let mut sim = Sim::new(g2_rules::crater_ridge(), None, 1, players);
        sim.fog = false;
        let target = sim
            .world
            .entities()
            .find(|e| e.owner == Some(PlayerId(1)) && sim.world.rules.def(e.def).is_unit())
            .unwrap();
        let (at, def) = (target.pos, target.def);
        let hash = sim.world.state_hash();
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Image>()
            .insert_resource(sim)
            .init_resource::<Previous>()
            .init_resource::<crate::sim::Hits>()
            .add_plugins(ViewPlugin);
        app.update();
        let wrecks = |app: &mut App| app.world_mut().query::<&Wreck>().iter(app.world()).count();
        let hit = |killed| crate::sim::Shot::Hit {
            from: at,
            at,
            killed,
            tracer: false,
            target: killed.then_some(def),
            look: None,
            air: (false, false),
        };
        app.world_mut().resource_mut::<crate::sim::Hits>().0.push(hit(true));
        app.update();
        let n = wrecks(&mut app);
        assert!(n >= 3, "a scorch and rubble: {n}");
        // Not a kill: nothing is left behind.
        app.world_mut().resource_mut::<crate::sim::Hits>().0.push(hit(false));
        app.update();
        assert_eq!(wrecks(&mut app), n, "a plain hit leaves no wreck");
        // Time passes: the wreck sinks before it goes.
        let top = |app: &mut App| {
            app.world_mut()
                .query::<(&Wreck, &Transform)>()
                .iter(app.world())
                .map(|(_, t)| t.translation.y)
                .fold(f32::MIN, f32::max)
        };
        let high = top(&mut app);
        for mut w in app.world_mut().query::<&mut Wreck>().iter_mut(app.world_mut()) {
            w.age = w.life - SINK_SECS / 2.0;
        }
        app.update();
        assert!(top(&mut app) < high, "sinking");
        for mut w in app.world_mut().query::<&mut Wreck>().iter_mut(app.world_mut()) {
            w.age = w.life;
        }
        app.update();
        assert_eq!(wrecks(&mut app), 0, "gone");
        assert_eq!(app.world().resource::<Sim>().world.state_hash(), hash, "decoration never touches the sim");
    }

    /// A shell handed over partway through its flight (a screenshot of one fired earlier)
    /// starts at that point of its arc, not at the muzzle, and still lands on time.
    #[test]
    fn a_shell_partway_through_its_flight_starts_where_it_is() {
        let players = Sim::players([Faction::Eu, Faction::Gla], false).map(|p| PlayerSetup { ai: None, ..p });
        let sim = Sim::new(g2_rules::crater_ridge(), None, 1, players);
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Image>()
            .insert_resource(sim)
            .init_resource::<Previous>()
            .init_resource::<crate::sim::Hits>()
            .insert_resource(FreezeEffects)
            .add_plugins(ViewPlugin);
        let (from, at) = (Vec2::from_int(100, 100), Vec2::from_int(160, 100));
        let look = crate::fx::Look::Shell { toxic: false, napalm: false };
        for done in [0.0, 0.5] {
            app.world_mut().resource_mut::<crate::sim::Hits>().0.push(crate::sim::Shot::Shell {
                from,
                at,
                flight: 20,
                done,
                look,
            });
        }
        app.update();
        let mut shells: Vec<(f32, f32, f32)> = app
            .world_mut()
            .query::<(&crate::fx::Flight, &Transform)>()
            .iter(app.world())
            .map(|(f, tf)| (tf.translation.x, f.age, f.life))
            .collect();
        shells.sort_by(|a, b| a.0.total_cmp(&b.0));
        let [(x0, age0, life0), (x1, age1, life1)] = shells[..] else { panic!("two shells: {shells:?}") };
        assert!((x0 - 100.0).abs() < 0.01, "a fresh shell starts at the muzzle: {x0}");
        assert!((x1 - 130.0).abs() < 0.01, "a half-flown shell starts halfway: {x1}");
        let tick = 1.0 / g2_sim::TICKS_PER_SECOND as f32;
        assert!(age0 == 0.0 && (life0 - 20.0 * tick).abs() < 1e-4);
        assert!((life1 - age1 - 20.0 * tick).abs() < 1e-4, "lands after the 20 ticks still left");
    }
}
