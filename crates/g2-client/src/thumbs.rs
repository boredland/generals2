//! Pictures of each def's own model for the framed HUD's portrait and build tray (docs/STYLE.md,
//! HUD): the procedural model as the battlefield draws it, in its owner's team colour, stood in
//! a studio of its own (a fixed key light, a three-quarter view, a transparent background) and
//! rendered once into a small image, cached by def, faction and team colour. The studios sit on
//! a render layer of their own, so the battlefield's cameras never see them, and go once their
//! picture is taken. The HUD shows a class silhouette until then.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, Exposure, RenderTarget, ScalingMode};
use bevy::prelude::*;
use bevy::render::render_resource::{PipelineCache, TextureFormat};
use bevy::render::view::Tonemapping;
use bevy::render::{Render, RenderApp, RenderSystems};
use g2_sim::{DefId, Faction, PlayerId};

use crate::sim::Sim;

/// The first studio's render layer; the battlefield's cameras draw layer 0 only.
const FIRST_LAYER: usize = 1;
/// Pixels a side: the portrait shows a picture 68 px wide at 720p (102 at 1080p), a build
/// button 42 (63).
const SIDE: u32 = 128;
/// Studios open at once. Each is a camera with its own passes and per-view buffers, drawn
/// every frame until its picture is taken: about 10 MB of peak memory a studio. Four take a
/// faction's 14 to 17 pictures in five frames, as twelve did.
const STUDIOS: usize = 4;

/// Studio `slot`'s layer. Every studio stands at the same spot, each on a layer of its own,
/// so a picture comes out the same whichever studio takes it.
fn studio(slot: usize) -> RenderLayers {
    RenderLayers::layer(FIRST_LAYER + slot)
}

/// Every studio's layer: what the key light shines on, and what no battlefield camera draws.
fn studios() -> RenderLayers {
    (0..STUDIOS).fold(RenderLayers::none(), |all, s| all.union(&studio(s)))
}

/// How far back a studio's camera stands from its model; its view reaches twice as far.
const REACH: f32 = 200.0;
/// The view from the front right (models face +Z) and above, so a picture shows a unit's nose
/// and flank and a building's front and side, lit from the upper left as the battlefield is.
const YAW: f32 = 0.6;
const PITCH: f32 = 0.5;
/// Room around the model, as a share of its size in the picture.
const MARGIN: f32 = 1.08;

/// A picture: `def` as drawn for `faction` in team palette entry `team`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct Key {
    pub def: DefId,
    pub faction: Option<Faction>,
    pub team: usize,
}

impl Key {
    /// `def` as `owner`'s (no owner: neutral, in the first palette entry, as `view.rs` draws it).
    pub(crate) fn of(sim: &Sim, def: DefId, owner: Option<PlayerId>) -> Key {
        Key {
            def,
            faction: owner.map(|o| sim.world.player(o).faction),
            team: owner.map_or(0, |o| sim.teams[o.0 as usize]),
        }
    }
}

enum State {
    Queued,
    /// In studio `slot` since the render world had drawn `since` frames.
    Shooting {
        camera: Entity,
        set: Entity,
        slot: usize,
        since: u32,
    },
    Ready,
}

struct Thumb {
    image: Option<Handle<Image>>,
    state: State,
}

#[derive(Resource, Default)]
pub(crate) struct Thumbs {
    by_key: BTreeMap<Key, Thumb>,
    /// The studios' key light, there while any studio is.
    light: Option<Entity>,
}

impl Thumbs {
    /// `key`'s picture once it is taken; the first call asks for it.
    pub(crate) fn get(&mut self, key: Key) -> Option<Handle<Image>> {
        let t = self.by_key.entry(key).or_insert(Thumb { image: None, state: State::Queued });
        if matches!(t.state, State::Ready) { t.image.clone() } else { None }
    }
}

/// Whether every picture asked for is taken (true without any), for the headless screenshot
/// to wait on, as it waits on the models.
pub fn ready(world: &World) -> bool {
    world.get_resource::<Thumbs>().is_none_or(|t| t.by_key.values().all(|t| matches!(t.state, State::Ready)))
}

/// What the render world tells the studios: how many frames it has drawn, and how many of the
/// latest in a row it drew with every pipeline compiled. A frame drawn while one was still
/// compiling left out whatever needed it.
#[derive(Default)]
struct Counts {
    drawn: AtomicU32,
    complete: AtomicU32,
}

#[derive(Resource, Clone, Default)]
struct Frames(Arc<Counts>);

/// After the frame's draws: the pipeline cache queued what wasn't ready (`process_queue`, just
/// before the draws) and left the rest out of them.
fn count_frames(cache: Res<PipelineCache>, frames: Res<Frames>) {
    if cache.waiting_pipelines().next().is_none() {
        frames.0.complete.fetch_add(1, Ordering::SeqCst);
    } else {
        frames.0.complete.store(0, Ordering::SeqCst);
    }
    frames.0.drawn.fetch_add(1, Ordering::SeqCst);
}

pub struct ThumbsPlugin;

impl Plugin for ThumbsPlugin {
    fn build(&self, app: &mut App) {
        let frames = Frames::default();
        app.init_resource::<Thumbs>()
            .insert_resource(frames.clone())
            .add_systems(Update, (reset.run_if(crate::menu::scene_start), shoot).chain().after(crate::view::ViewSet));
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.insert_resource(frames).add_systems(Render, count_frames.in_set(RenderSystems::Cleanup));
        }
    }
}

/// Every def the local player can build or train: what each of their faction's builders and
/// factories offers them, as the build tray does.
pub(crate) fn buildable(sim: &Sim) -> BTreeSet<DefId> {
    let rules = sim.world.rules;
    let faction = sim.world.player(sim.local).faction;
    let makers = (0..rules.defs.len() as u16).map(DefId).filter(|&m| rules.def(m).faction == Some(faction));
    makers.flat_map(|m| sim.world.offers_of(sim.local, m)).collect()
}

/// A new match brings new materials and maybe another side, so the pictures are taken afresh.
/// The local player's whole build list is asked for at once, so the tray doesn't wait on one.
fn reset(
    mut commands: Commands,
    sim: Res<Sim>,
    mut thumbs: ResMut<Thumbs>,
    alpha: Option<Res<crate::alpha_hud::Layout>>,
) {
    for t in thumbs.by_key.values() {
        if let State::Shooting { camera, set, .. } = t.state {
            commands.entity(camera).try_despawn();
            commands.entity(set).try_despawn();
        }
    }
    if let Some(light) = thumbs.light.take() {
        commands.entity(light).try_despawn();
    }
    thumbs.by_key.clear();
    // The alpha's HUD shows the alpha's own icons and portraits.
    if alpha.is_some() {
        return;
    }
    for def in buildable(&sim) {
        thumbs.get(Key::of(&sim, def, Some(sim.local)));
    }
}

/// Takes the pictures asked for: opens a studio for each while there are free ones, and closes
/// a studio once its picture is complete.
fn shoot(
    mut commands: Commands,
    sim: Res<Sim>,
    assets: Option<Res<crate::view::Assets3d>>,
    images: Option<ResMut<Assets<Image>>>,
    frames: Res<Frames>,
    mut thumbs: ResMut<Thumbs>,
) {
    let (Some(assets), Some(mut images)) = (assets, images) else { return };
    let drawn = frames.0.drawn.load(Ordering::SeqCst);
    let complete = frames.0.complete.load(Ordering::SeqCst) >= 2;
    let thumbs = &mut *thumbs;
    let mut busy = [false; STUDIOS];
    for t in thumbs.by_key.values_mut() {
        let State::Shooting { camera, set, slot, since } = t.state else { continue };
        // The studio was in the world before frame `since + 1` was extracted, so a third frame
        // on, the newest two drawn complete, the last of them drew it whole.
        if complete && drawn >= since + 3 {
            commands.entity(camera).despawn();
            commands.entity(set).despawn();
            t.state = State::Ready;
        } else {
            busy[slot] = true;
        }
    }
    let view = Quat::from_euler(EulerRot::YXZ, YAW, -PITCH, 0.0);
    for (key, t) in thumbs.by_key.iter_mut().filter(|(_, t)| matches!(t.state, State::Queued)) {
        if !busy.contains(&false) {
            break;
        }
        let parts = crate::view::still_model(&assets, sim.world.rules.def(key.def), key.faction, key.team);
        // A faction building held by nobody has no model of its own: it keeps its silhouette.
        if parts.is_empty() {
            t.state = State::Ready;
            continue;
        }
        let Some(slot) = busy.iter().position(|b| !b) else { break };
        busy[slot] = true;
        let (centre, side) = frame(&parts, view);
        // Below the ground, out of the way of anything on the battlefield's layer.
        let origin = Vec3::new(0.0, -REACH, 0.0);
        let image = t.image.get_or_insert_with(|| images.add(target())).clone();
        let set = commands
            .spawn((crate::menu::MatchEntity, Transform::from_translation(origin), Visibility::Visible))
            .with_children(|s| {
                for part in parts {
                    s.spawn((part, studio(slot)));
                }
            })
            .id();
        let camera = commands
            .spawn((
                crate::menu::MatchEntity,
                Camera3d::default(),
                // Before the battlefield's camera, so the HUD drawn over it this frame can
                // already show the picture.
                Camera { order: -1, clear_color: ClearColorConfig::Custom(Color::NONE), ..default() },
                RenderTarget::from(image),
                Projection::Orthographic(OrthographicProjection {
                    scaling_mode: ScalingMode::Fixed { width: side, height: side },
                    near: 0.0,
                    far: 2.0 * REACH,
                    ..OrthographicProjection::default_3d()
                }),
                Transform::from_translation(origin + view * centre.extend(REACH)).with_rotation(view),
                Exposure::SUNLIGHT,
                Tonemapping::AgX,
                // The battlefield's grade, so a picture's colours are the model's on the ground.
                crate::look::grading(),
                // A strong fill, so the dark side of a dark hull still reads at 42 px.
                AmbientLight { color: Color::srgb(0.95, 0.90, 0.82), brightness: 14_000.0, ..default() },
                studio(slot),
            ))
            .id();
        t.state = State::Shooting { camera, set, slot, since: drawn };
    }
    match (busy.contains(&true), thumbs.light) {
        (true, None) => {
            // From the upper left of the picture, over the viewer's shoulder.
            let toward = view * Vec3::new(0.9, -1.1, -0.8).normalize();
            let light = commands.spawn((
                crate::menu::MatchEntity,
                DirectionalLight { color: Color::srgb(1.0, 0.95, 0.86), illuminance: 100_000.0, ..default() },
                Transform::default().looking_to(toward, Vec3::Y),
                studios(),
            ));
            thumbs.light = Some(light.id());
        }
        (false, Some(light)) => {
            commands.entity(light).despawn();
            thumbs.light = None;
        }
        _ => {}
    }
}

/// A picture's render target: the GPU keeps the only copy, as nothing reads it back.
fn target() -> Image {
    let mut image = Image::new_target_texture(SIDE, SIDE, TextureFormat::Rgba8UnormSrgb, None);
    image.asset_usage = RenderAssetUsages::RENDER_WORLD;
    image
}

/// The square of the view that holds every part with a margin: its centre in view space and
/// its side. Box corners bound cylinders and spheres too.
fn frame(parts: &[(Mesh3d, MeshMaterial3d<StandardMaterial>, Transform)], view: Quat) -> (Vec2, f32) {
    let to_view = view.inverse();
    let (mut lo, mut hi) = (Vec2::MAX, Vec2::MIN);
    for (.., tf) in parts {
        for c in 0..8 {
            let corner = Vec3::new(
                if c & 1 == 0 { -0.5 } else { 0.5 },
                if c & 2 == 0 { -0.5 } else { 0.5 },
                if c & 4 == 0 { -0.5 } else { 0.5 },
            );
            let p = (to_view * tf.transform_point(corner)).truncate();
            (lo, hi) = (lo.min(p), hi.max(p));
        }
    }
    if lo.x > hi.x {
        return (Vec2::ZERO, 1.0);
    }
    ((lo + hi) / 2.0, (hi - lo).max_element() * MARGIN)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let players = Sim::players([Faction::Apa, Faction::Gla], false);
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Image>()
            .insert_resource(Sim::new(g2_rules::crater_ridge(), None, 1, players))
            .init_resource::<crate::sim::Previous>()
            .init_resource::<crate::sim::Hits>()
            .add_plugins((crate::view::ViewPlugin, ThumbsPlugin));
        app
    }

    /// Every def the local player can build gets a picture in its team colour as the match
    /// starts, each in a studio only its own camera sees; the battlefield's camera sees none.
    #[test]
    fn every_buildable_def_is_pictured_on_a_layer_the_battlefield_does_not_draw() {
        let mut app = app();
        crate::camera::spawn(&mut app.world_mut().commands(), Vec3::ZERO, crate::camera::DEFAULT_DISTANCE);
        for _ in 0..3 {
            app.update();
        }
        let world = app.world_mut();
        let sim = world.resource::<Sim>();
        let want: Vec<Key> = buildable(sim).into_iter().map(|d| Key::of(sim, d, Some(sim.local))).collect();
        for name in ["APA War Factory", "APA Warlord Tank", "APA Red Guard", "APA Crane"] {
            let def = sim.world.rules.find(name).unwrap();
            assert!(want.iter().any(|k| k.def == def), "{name} is not in the build list");
        }
        assert!(want.iter().all(|k| k.faction == Some(Faction::Apa) && k.team == sim.teams[0]));
        let thumbs = world.resource::<Thumbs>();
        for key in &want {
            assert!(thumbs.by_key.contains_key(key), "no picture asked for {}", sim.world.rules.def(key.def).name);
        }
        let all = studios();
        assert!(!all.intersects(&RenderLayers::default()), "a studio is on the battlefield's layer");
        let shooting = thumbs.by_key.values().filter(|t| matches!(t.state, State::Shooting { .. })).count();
        assert_eq!(shooting, STUDIOS, "as many studios as there are, the rest queued");
        let mut cameras = world.query::<(&Camera, Option<&RenderLayers>, Has<crate::camera::RtsCamera>)>();
        let (mut battlefield, mut seen) = (0, RenderLayers::none());
        for (_, layers, rts) in cameras.iter(world) {
            let layers = layers.cloned().unwrap_or_default();
            if rts {
                battlefield += 1;
                assert!(!layers.intersects(&all), "the battlefield's camera draws a studio");
            } else {
                assert!((0..STUDIOS).any(|s| layers == studio(s)), "a studio camera draws more than its studio");
                assert!(!layers.intersects(&seen), "two studio cameras share a layer");
                seen = seen.union(&layers);
            }
        }
        assert_eq!((battlefield, &seen), (1, &all));
        let mut meshes = world.query::<(&Mesh3d, Option<&RenderLayers>)>();
        let pictured = meshes.iter(world).filter(|(_, l)| l.is_some_and(|l| l.intersects(&all))).count();
        assert!(pictured > STUDIOS * 5, "the studios hold the models: {pictured} parts");
        assert!(
            meshes.iter(world).all(|(_, l)| l.is_none_or(|l| (0..STUDIOS).any(|s| *l == studio(s)))),
            "a part is in two studios, or in a studio and on the battlefield"
        );
    }

    /// Asking again for a def in the same team colour gives the picture already taken (or being
    /// taken), never a second one; another team's colour is another picture.
    #[test]
    fn the_same_def_and_team_share_one_image() {
        let mut app = app();
        app.update();
        app.update();
        let world = app.world_mut();
        let images = world.resource::<Assets<Image>>().len();
        let sim = world.resource::<Sim>();
        let def = buildable(sim).into_iter().next().unwrap();
        let mine = Key::of(sim, def, Some(sim.local));
        let theirs = Key::of(sim, def, Some(PlayerId(1)));
        assert_ne!(mine, theirs);
        let mut thumbs = world.resource_mut::<Thumbs>();
        let count = thumbs.by_key.len();
        let first = thumbs.by_key[&mine].image.clone().expect("in a studio already");
        assert_eq!(thumbs.get(mine), None, "not taken without a renderer");
        assert_eq!(thumbs.by_key.len(), count, "asking again adds nothing");
        thumbs.get(theirs);
        assert_eq!(thumbs.by_key.len(), count + 1);
        app.update();
        let world = app.world_mut();
        let thumbs = world.resource::<Thumbs>();
        assert_eq!(thumbs.by_key[&mine].image.as_ref(), Some(&first), "the same image");
        assert_eq!(world.resource::<Assets<Image>>().len(), images, "every studio busy: no new image");
    }
}
