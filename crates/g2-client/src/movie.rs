//! The alpha's logo movie, played once over the main menu when the player's import has its
//! frames (`g2-tools alpha DIR import` decodes them with ffmpeg). The frames are PNGs shown
//! in turn on a full-screen image node at the movie's own rate, with its decoded audio; any
//! key or click skips it. The menu music waits until it is over.

use std::path::PathBuf;
use std::time::Duration;

use bevy::asset::AssetPath;
use bevy::prelude::*;

/// The movie that opens the game: EA's and the studio's logos, 10.7 s.
pub const INTRO: &str = "movies/_rts/art/cinematics/mov/cnc_frontend/cnclive_frontend_logos_avi";

/// Frames to load ahead of the one on screen, so playback doesn't wait on the disk.
const AHEAD: usize = 8;

pub struct MoviePlugin;

impl Plugin for MoviePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, start).add_systems(Update, (play, skip).chain().before(crate::menu::MenuSet));
    }
}

/// A movie on screen, and what is needed to step through it.
#[derive(Resource)]
pub struct Playing {
    /// Asset path of each frame (`alpha://.../NNNN.png`), in order.
    frames: Vec<AssetPath<'static>>,
    /// Loaded frames, by index; released once shown.
    loaded: Vec<Option<Handle<Image>>>,
    fps: f64,
    elapsed: Duration,
    shown: Option<usize>,
}

impl Playing {
    /// The frame for `elapsed`, or `None` once the movie is over.
    fn frame_at(&self, elapsed: Duration) -> Option<usize> {
        let i = (elapsed.as_secs_f64() * self.fps) as usize;
        (i < self.frames.len()).then_some(i)
    }
}

/// The node the frames are drawn on, above every menu.
#[derive(Component)]
struct Screen;

/// The movie's own audio.
#[derive(Component)]
struct Soundtrack;

/// Reads the intro's frame list and rate from the import, if it has them.
pub fn intro(cache: &crate::alpha::Cache) -> Option<(Vec<AssetPath<'static>>, f64, Option<PathBuf>)> {
    let dir = cache.0.join(format!("{INTRO}.frames"));
    let fps: f64 = std::fs::read_to_string(dir.join("fps.txt")).ok()?.trim().parse().ok()?;
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".png"))
        .collect();
    names.sort_unstable();
    let frames: Vec<AssetPath<'static>> = names
        .iter()
        .map(|n| AssetPath::from(format!("{INTRO}.frames/{n}")).with_source("alpha").into_owned())
        .collect();
    let audio = cache.0.join(format!("{INTRO}.wav"));
    (!frames.is_empty() && fps > 0.0).then_some((frames, fps, audio.is_file().then_some(audio)))
}

fn start(
    mut commands: Commands,
    cache: Option<Res<crate::alpha::Cache>>,
    ui: Option<Res<crate::menu::Ui>>,
    mut sounds: ResMut<Assets<crate::audio::Sound>>,
    muted: Res<crate::audio::Muted>,
) {
    // Only in front of the main menu: a match started from the command line skips it.
    if ui.is_none_or(|u| u.screen != crate::menu::Screen::Main) {
        return;
    }
    let Some((frames, fps, audio)) = cache.and_then(|c| intro(&c)) else { return };
    let n = frames.len();
    commands.insert_resource(Playing { frames, loaded: vec![None; n], fps, elapsed: Duration::ZERO, shown: None });
    commands.spawn((
        Screen,
        GlobalZIndex(100),
        BackgroundColor(Color::BLACK),
        ImageNode::default(),
        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
    ));
    if !muted.0
        && let Some(sound) = audio.and_then(|p| crate::alpha::read_wav(&p).ok()).map(crate::alpha::Wav::into_sound)
    {
        commands.spawn((AudioPlayer(sounds.add(sound)), PlaybackSettings::DESPAWN, Soundtrack));
    }
}

/// Advances the movie with real time, keeping a few frames loaded ahead; ends it after the
/// last frame.
fn play(
    mut commands: Commands,
    playing: Option<ResMut<Playing>>,
    time: Res<Time>,
    server: Res<AssetServer>,
    images: Res<Assets<Image>>,
    mut screen: Query<&mut ImageNode, With<Screen>>,
) {
    let Some(mut p) = playing else { return };
    let elapsed = p.elapsed + time.delta();
    let Some(now) = p.frame_at(elapsed) else {
        end(&mut commands);
        return;
    };
    for i in now..(now + AHEAD).min(p.frames.len()) {
        if p.loaded[i].is_none() {
            let path = p.frames[i].clone();
            p.loaded[i] = Some(server.load(path));
        }
    }
    // Hold the clock on a frame that isn't decoded yet, so a slow disk delays the movie
    // rather than skipping frames.
    let handle = p.loaded[now].clone().unwrap();
    if !images.contains(&handle) {
        return;
    }
    p.elapsed = elapsed;
    if p.shown != Some(now) {
        if let Ok(mut node) = screen.single_mut() {
            node.image = handle;
        }
        if let Some(prev) = p.shown {
            for i in prev..now {
                p.loaded[i] = None;
            }
        }
        p.shown = Some(now);
    }
}

/// Any key or mouse button ends the movie, and the press is used up: the menu under the
/// movie must not also take it (Esc on the main menu quits the game).
fn skip(
    mut commands: Commands,
    playing: Option<Res<Playing>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
) {
    if playing.is_some() && (keys.get_just_pressed().next().is_some() || mouse.get_just_pressed().next().is_some()) {
        keys.reset_all();
        mouse.reset_all();
        end(&mut commands);
    }
}

/// Headless: the intro's frame `i` on the movie screen, held still. False when the import
/// has no such frame.
pub fn hold(app: &mut App, cache: &crate::alpha::Cache, i: usize) -> bool {
    let Some(path) = intro(cache).and_then(|(frames, ..)| frames.get(i).cloned()) else { return false };
    let image = app.world().resource::<AssetServer>().load(path);
    app.world_mut().spawn((
        Screen,
        HeldFrame,
        GlobalZIndex(100),
        BackgroundColor(Color::BLACK),
        ImageNode::new(image),
        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
    ));
    true
}

/// A frame shown by `hold`, so the capture waits for it.
#[derive(Component)]
struct HeldFrame;

/// True unless a held frame is still loading (or failed to).
pub fn held_frame_loaded(world: &World) -> bool {
    let Some(server) = world.get_resource::<AssetServer>() else { return true };
    let mut q = world.try_query_filtered::<&ImageNode, With<HeldFrame>>().unwrap();
    q.iter(world).all(|n| server.is_loaded_with_dependencies(&n.image))
}

fn end(commands: &mut Commands) {
    commands.remove_resource::<Playing>();
    commands.queue(|world: &mut World| {
        let gone: Vec<Entity> =
            world.query_filtered::<Entity, Or<(With<Screen>, With<Soundtrack>)>>().iter(world).collect();
        for e in gone {
            world.despawn(e);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The movie shows each frame for 1/fps of a second and ends after the last one.
    #[test]
    fn frames_follow_the_clock_and_end_after_the_last() {
        let frames = (1..=3).map(|i| AssetPath::from(format!("{i:04}.png")).into_owned()).collect();
        let p = Playing { frames, loaded: vec![None; 3], fps: 30.0, elapsed: Duration::ZERO, shown: None };
        let at = |ms: u64| p.frame_at(Duration::from_millis(ms));
        assert_eq!(
            [at(0), at(33), at(34), at(66), at(67), at(99), at(100)],
            [Some(0), Some(0), Some(1), Some(1), Some(2), Some(2), None]
        );
    }

    /// Against a real import (`G2_ALPHA_CACHE`; skipped when unset): in front of the main
    /// menu the intro's frames are shown in order on the screen node, the menu music stays
    /// off while it plays, and the movie ends on its own after its 10.7 s. Esc skips it and
    /// is used up doing so: the main menu under the movie doesn't also take it and quit.
    /// The PNG loader comes with rendering, which this app lacks, so each frame `play`
    /// asks for is stood in by a 1x1 image under its handle; decoding is left to the
    /// headless screenshot check.
    #[test]
    fn the_intro_plays_through_then_the_music_starts() {
        use bevy::time::TimeUpdateStrategy;
        let Some(dir) = std::env::var_os("G2_ALPHA_CACHE") else { return };
        let cache = crate::alpha::Cache(PathBuf::from(dir));
        let run = |skip_at: Option<u32>| -> (u32, Vec<usize>, bool, bool, bool) {
            let mut app = App::new();
            app.register_asset_source(
                "alpha",
                bevy::asset::io::AssetSourceBuilder::platform_default(&cache.0.to_string_lossy(), None),
            )
            .add_plugins((MinimalPlugins, AssetPlugin::default(), bevy::audio::AudioPlugin::default()))
            .init_asset::<Image>()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(33)))
            .insert_resource(cache.clone())
            .insert_resource(crate::menu::Ui::default())
            .insert_resource(crate::sim::Sim::new(
                g2_rules::crater_ridge(),
                None,
                1,
                crate::sim::Sim::players([g2_sim::Faction::Eu, g2_sim::Faction::Gla], false),
            ))
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_plugins((
                crate::sim::SimPlugin,
                crate::menu::MenuPlugin { blocking: true },
                crate::audio::SoundPlugin,
                MoviePlugin,
            ));
            let (mut shown, mut music_while_playing) = (Vec::new(), false);
            let started = std::time::Instant::now();
            for frame in 0..600 {
                assert!(started.elapsed() < Duration::from_secs(60), "the test itself is stuck");
                if Some(frame) == skip_at {
                    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
                }
                app.update();
                // Read before the next update clears it: did the menu take the press and quit?
                let quit = !app.world().resource::<Messages<AppExit>>().is_empty()
                    || app.world().resource::<crate::menu::Ui>().screen != crate::menu::Screen::Main;
                app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release_all();
                app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
                let Some(p) = app.world().get_resource::<Playing>() else {
                    // The music system ran before the movie ended this update: look one later.
                    app.update();
                    let music = music_on(&mut app);
                    return (frame, shown, music_while_playing, music, quit);
                };
                if let Some(i) = p.shown
                    && shown.last() != Some(&i)
                {
                    shown.push(i);
                }
                let wanted: Vec<Handle<Image>> = p.loaded.iter().flatten().cloned().collect();
                let mut images = app.world_mut().resource_mut::<Assets<Image>>();
                for h in wanted {
                    if !images.contains(&h) {
                        let _ = images.insert(&h, Image::default());
                    }
                }
                music_while_playing |= music_on(&mut app);
            }
            panic!("the intro never ended in 600 updates");
        };
        fn music_on(app: &mut App) -> bool {
            let Some(music) = app.world().get_resource::<crate::audio::Music>().map(|m| m.0.clone()) else {
                panic!("the import has no menu music")
            };
            app.world_mut().query::<&AudioPlayer<crate::audio::Sound>>().iter(app.world()).any(|p| p.0 == music)
        }
        let (ended, shown, music_while_playing, music_after, _) = run(None);
        assert!(!music_while_playing, "no menu music under the movie's own soundtrack");
        assert!(shown.windows(2).all(|w| w[1] > w[0]), "frames only move forward");
        assert!(
            shown.len() > 250 && shown.last() == Some(&319),
            "{} of 320 frames shown, last {:?}",
            shown.len(),
            shown.last()
        );
        let secs = f64::from(ended) * 0.033;
        assert!((10.6..12.0).contains(&secs), "ended after {secs} s");
        assert!(music_after, "the menu music starts once the movie is over");
        let (ended, _, _, _, quit) = run(Some(30));
        assert_eq!(ended, 30, "Esc ends the movie the frame it is pressed");
        assert!(!quit, "skipping the movie doesn't also quit the game from the main menu");
    }
}
