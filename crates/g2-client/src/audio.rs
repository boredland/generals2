//! Sound, all of it synthesized here at startup: no recordings or sound assets. Each cue is a
//! fraction of a second of filtered noise, sine sweeps and envelopes, rendered once into a
//! sample buffer and replayed through `bevy_audio`.
//!
//! The sim's events drive the cues. Like the effects in `view.rs`, combat is heard only where
//! the local player can see one end of it, and the further from the camera's focus the
//! quieter. Each kind of cue starts at most `VOICES_PER_FRAME` times per frame, so a large
//! fight thickens the sound instead of clipping it. `M` mutes.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use bevy::audio::{AddAudioSource, AudioPlayer, ChannelCount, Decodable, PlaybackSettings, SampleRate, Volume};
use bevy::prelude::*;
use g2_sim::world::Event;

use crate::camera::RtsCamera;
use crate::sim::{LOCAL, Sim};

/// Output sample rate of every synthesized sound.
const RATE: u32 = 44_100;
/// Most voices per cue kind started in one frame; the rest of a volley is dropped.
const VOICES_PER_FRAME: usize = 3;
/// Placed cues are at full volume within this distance (world units) of the camera's focus,
/// about a screen's width, and fade out to silence at `HEARING`.
const NEAR: f32 = 60.0;
const HEARING: f32 = 180.0;

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.add_audio_source::<Sound>()
            .init_resource::<Muted>()
            .add_systems(Startup, load_sounds)
            // Before the menu applies its queued actions, so each one is heard as a click.
            .add_systems(Update, (toggle_mute, play_cues).chain().before(crate::menu::MenuSet));
    }
}

/// Something worth hearing, by kind.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum Cue {
    /// A direct-fire shot: a rifle crack, or a cannon's thump when `heavy`.
    Shot { heavy: bool },
    /// An artillery shell leaving the barrel.
    Launch,
    /// A shell landing.
    Blast,
    /// A unit or building destroyed.
    Kill { building: bool },
    /// One of our units rolled out.
    Ready,
    /// One of our buildings finished.
    Built,
    /// A collector unloaded at one of our depots.
    Cash,
    /// An order or a menu choice acknowledged.
    Click,
    /// One of our orders refused (no money, bad placement).
    Refused,
}

impl Cue {
    const ALL: [Cue; 11] = [
        Cue::Shot { heavy: false },
        Cue::Shot { heavy: true },
        Cue::Launch,
        Cue::Blast,
        Cue::Kill { building: false },
        Cue::Kill { building: true },
        Cue::Ready,
        Cue::Built,
        Cue::Cash,
        Cue::Click,
        Cue::Refused,
    ];

    /// Mix level: gunfire under the blasts, notifications above the battle.
    fn level(self) -> f32 {
        match self {
            Cue::Shot { heavy: false } => 0.22,
            Cue::Shot { heavy: true } => 0.35,
            Cue::Launch => 0.4,
            Cue::Blast => 0.55,
            Cue::Kill { building: false } => 0.45,
            Cue::Kill { building: true } => 0.7,
            Cue::Ready | Cue::Built => 0.45,
            Cue::Cash => 0.15,
            Cue::Click => 0.3,
            // A clipped buzz: dense, so it sits lower than the click.
            Cue::Refused => 0.18,
        }
    }
}

#[derive(Resource, Default)]
pub struct Muted(pub bool);

/// A synthesized mono sound.
#[derive(Asset, TypePath, Clone)]
pub struct Sound(Arc<[f32]>);

impl Decodable for Sound {
    type Decoder = Playback;

    fn decoder(&self) -> Playback {
        Playback { samples: self.0.clone(), at: 0 }
    }
}

/// Plays a `Sound`'s samples once.
pub struct Playback {
    samples: Arc<[f32]>,
    at: usize,
}

impl Iterator for Playback {
    type Item = bevy::audio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        let s = *self.samples.get(self.at)?;
        self.at += 1;
        Some(s as bevy::audio::Sample)
    }
}

impl bevy::audio::Source for Playback {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.samples.len() - self.at)
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::new(1).unwrap()
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(RATE).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f64(self.samples.len() as f64 / RATE as f64))
    }
}

#[derive(Resource)]
struct Bank(BTreeMap<Cue, Handle<Sound>>);

fn load_sounds(mut commands: Commands, mut sounds: ResMut<Assets<Sound>>) {
    let bank = Cue::ALL.into_iter().map(|c| (c, sounds.add(Sound(synth(c).into())))).collect();
    commands.insert_resource(Bank(bank));
}

fn toggle_mute(keys: Res<ButtonInput<KeyCode>>, mut muted: ResMut<Muted>) {
    if keys.just_pressed(KeyCode::KeyM) {
        muted.0 = !muted.0;
    }
}

/// Plays what the sim gathered since the last frame (`sim::Heard`), a click for each order the
/// local player gave since (`Sim::unheard_orders`, drained here, so a new match starts at
/// zero) and each menu choice about to apply.
fn play_cues(
    mut commands: Commands,
    mut sim: ResMut<Sim>,
    mut heard: ResMut<crate::sim::Heard>,
    pending: Option<Res<crate::menu::Pending>>,
    bank: Option<Res<Bank>>,
    muted: Res<Muted>,
    cam: Query<&RtsCamera>,
) {
    let mut cues = core::mem::take(&mut heard.0);
    let orders = core::mem::take(&mut sim.bypass_change_detection().unheard_orders) as usize;
    cues.extend(std::iter::repeat_n((Cue::Click, None), orders + pending.map_or(0, |p| p.0.len())));
    let Some(bank) = bank else { return };
    if muted.0 {
        return;
    }
    let focus = cam.iter().next().map(|c| Vec2::new(c.pivot.x, c.pivot.z));
    let mut started = BTreeMap::<Cue, usize>::new();
    for (cue, at) in cues {
        let gain = match (at, focus) {
            (Some(p), Some(f)) => attenuation(Vec2::new(p.x.to_f32(), p.y.to_f32()).distance(f)),
            _ => 1.0,
        };
        let n = started.entry(cue).or_default();
        if gain <= 0.0 || *n >= VOICES_PER_FRAME {
            continue;
        }
        *n += 1;
        commands.spawn((
            AudioPlayer(bank.0[&cue].clone()),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(gain * cue.level())),
        ));
    }
}

/// Gain for a cue `d` world units from the camera's focus.
fn attenuation(d: f32) -> f32 {
    if d <= NEAR { 1.0 } else { (1.0 - (d - NEAR) / (HEARING - NEAR)).max(0.0).powi(2) }
}

/// The cues in the sim's latest step the local player may hear. Combat is heard where one end
/// of it is visible (fog off: everywhere); production, construction, deliveries and refusals
/// only for the local player, so the enemy's base gives nothing away.
pub fn sim_cues(sim: &Sim) -> Vec<(Cue, Option<g2_sim::Vec2>)> {
    let world = &sim.world;
    let vision = world.vision(LOCAL);
    let seen = |p: g2_sim::Vec2| !sim.fog || vision.is_visible(p);
    let mut out = Vec::new();
    for e in world.events() {
        match *e {
            Event::Hit { shooter, at, killed, splash, target_def, .. } => {
                let shooter = world.entity(shooter);
                let from = shooter.map_or(at, |s| s.pos);
                if !seen(at) && !seen(from) {
                    continue;
                }
                if !splash {
                    let heavy = shooter.and_then(|s| world.rules.def(s.def).weapon()).is_some_and(|w| w.damage >= 30);
                    out.push((Cue::Shot { heavy }, Some(from)));
                }
                if killed {
                    out.push((Cue::Kill { building: world.rules.def(target_def).is_building() }, Some(at)));
                }
            }
            Event::Fired { from, .. } if seen(from) => out.push((Cue::Launch, Some(from))),
            Event::Landed { at, .. } if seen(at) => out.push((Cue::Blast, Some(at))),
            Event::Produced { player: LOCAL, .. } => out.push((Cue::Ready, None)),
            Event::Completed { player: LOCAL, .. } => out.push((Cue::Built, None)),
            Event::Delivered { player: LOCAL, depot, .. } => out.push((Cue::Cash, world.entity(depot).map(|d| d.pos))),
            Event::Rejected { player: LOCAL, .. } => out.push((Cue::Refused, None)),
            _ => {}
        }
    }
    out
}

// ---- synthesis ------------------------------------------------------------------------------

/// Renders `cue`, normalised to a peak of 0.9 so no sound clips on its own.
fn synth(cue: Cue) -> Vec<f32> {
    let mut noise = Noise(0x9e37_79b9 ^ cue_seed(cue));
    let mut s = match cue {
        // A crack: bright noise with a 15 ms decay, over a short 900 Hz snap.
        Cue::Shot { heavy: false } => render(0.12, |t, _| {
            let n = noise.next();
            (n * 0.8 + sine(900.0, t) * 0.3) * env(t, 0.001, 0.015)
        }),
        // A cannon: a sine dropping 90 to 45 Hz under low-passed noise.
        Cue::Shot { heavy: true } => {
            let mut lp = Lowpass::new(1_200.0);
            render(0.4, |t, _| {
                let thump = sine_sweep(90.0, 45.0, 0.25, t) * env(t, 0.002, 0.12);
                thump + lp.apply(noise.next()) * env(t, 0.001, 0.05) * 1.2
            })
        }
        // A howitzer: deeper and longer than the cannon, with a ringing tail.
        Cue::Launch => {
            let mut lp = Lowpass::new(700.0);
            render(0.7, |t, _| {
                sine_sweep(70.0, 35.0, 0.4, t) * env(t, 0.003, 0.2) + lp.apply(noise.next()) * env(t, 0.002, 0.12) * 1.4
            })
        }
        // A shell landing: rumbling noise that closes down from bright to dark.
        Cue::Blast => {
            let mut lp = Lowpass::new(2_500.0);
            render(1.1, |t, _| {
                lp.set(2_500.0 * (-t * 4.0).exp() + 150.0);
                lp.apply(noise.next()) * env(t, 0.004, 0.35) * 2.0 + sine_sweep(55.0, 30.0, 0.6, t) * env(t, 0.005, 0.3)
            })
        }
        // A vehicle or squad lost: a crunch, then debris.
        Cue::Kill { building: false } => {
            let (mut lp, mut debris_lp) = (Lowpass::new(1_200.0), Lowpass::new(2_500.0));
            render(0.7, |t, _| {
                let crunch = lp.apply(noise.next()) * env(t, 0.002, 0.18) * 1.6;
                let rattle = if (t * 37.0).fract() < 0.08 { noise.next() } else { 0.0 };
                let debris = debris_lp.apply(rattle) * 0.8 * env(t, 0.1, 0.25);
                crunch + debris + sine_sweep(120.0, 50.0, 0.3, t) * env(t, 0.002, 0.15) * 0.6
            })
        }
        // A building collapsing: a long, low roar.
        Cue::Kill { building: true } => {
            let mut lp = Lowpass::new(900.0);
            render(1.6, |t, _| {
                lp.set(900.0 * (-t * 1.5).exp() + 80.0);
                lp.apply(noise.next()) * env(t, 0.01, 0.6) * 2.4 + sine_sweep(45.0, 25.0, 1.2, t) * env(t, 0.01, 0.5)
            })
        }
        // Two rising tones.
        Cue::Ready => chime(&[660.0, 880.0], 0.09),
        // Three rising tones, a major triad.
        Cue::Built => chime(&[523.25, 659.25, 783.99], 0.11),
        // A coin tick.
        Cue::Cash => render(0.06, |t, _| sine(1_760.0, t) * env(t, 0.001, 0.02)),
        // A short UI blip.
        Cue::Click => render(0.05, |t, _| (sine(1_200.0, t) * 0.7 + sine(2_400.0, t) * 0.3) * env(t, 0.001, 0.012)),
        // Two low, buzzy pulses.
        Cue::Refused => render(0.32, |t, _| {
            let gate = if t < 0.12 || (0.18..0.30).contains(&t) { 1.0 } else { 0.0 };
            let buzz = (sine(170.0, t) + sine(340.0, t) * 0.5 + sine(510.0, t) * 0.33).clamp(-0.8, 0.8);
            buzz * gate * 0.8
        }),
    };
    fade_out(&mut s, 0.01);
    let peak = s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if peak > 0.0 {
        s.iter_mut().for_each(|v| *v *= 0.9 / peak);
    }
    s
}

fn cue_seed(cue: Cue) -> u32 {
    Cue::ALL.iter().position(|&c| c == cue).unwrap_or(0) as u32 * 0x85eb_ca6b
}

/// `secs` of samples from `f(t, i)`.
fn render(secs: f32, mut f: impl FnMut(f32, usize) -> f32) -> Vec<f32> {
    let n = (secs * RATE as f32) as usize;
    (0..n).map(|i| f(i as f32 / RATE as f32, i)).collect()
}

/// Notes one after another, each a sine with its octave, `step` seconds apart.
fn chime(notes: &[f32], step: f32) -> Vec<f32> {
    let len = step * notes.len() as f32 + 0.25;
    render(len, |t, _| {
        notes
            .iter()
            .enumerate()
            .filter(|&(k, _)| t >= k as f32 * step)
            .map(|(k, &f)| {
                let u = t - k as f32 * step;
                (sine(f, u) + sine(2.0 * f, u) * 0.25) * env(u, 0.004, 0.12)
            })
            .sum()
    })
}

fn sine(freq: f32, t: f32) -> f32 {
    (std::f32::consts::TAU * freq * t).sin()
}

/// A sine gliding exponentially from `from` to `to` Hz over `secs` (phase-continuous).
fn sine_sweep(from: f32, to: f32, secs: f32, t: f32) -> f32 {
    let k = (to / from).ln() / secs;
    let u = t.min(secs);
    // Phase is the integral of the frequency; past `secs` the frequency stays at `to`.
    let phase = from * ((k * u).exp() - 1.0) / k + to * (t - u);
    (std::f32::consts::TAU * phase).sin()
}

/// Linear attack over `attack` seconds, then exponential decay with time constant `decay`.
fn env(t: f32, attack: f32, decay: f32) -> f32 {
    if t < attack { t / attack } else { (-(t - attack) / decay).exp() }
}

/// Ramps the last `secs` to zero, so no sound ends on a click.
fn fade_out(s: &mut [f32], secs: f32) {
    let n = ((secs * RATE as f32) as usize).min(s.len());
    let len = s.len();
    for (k, v) in s[len - n..].iter_mut().enumerate() {
        *v *= 1.0 - (k + 1) as f32 / n as f32;
    }
}

/// White noise in -1..1 (xorshift), seeded per cue so every build sounds the same.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

/// Two cascaded one-pole low-pass stages (12 dB per octave): one stage alone leaves white
/// noise hissing well above the cutoff, and explosions should rumble.
struct Lowpass {
    a: f32,
    y: [f32; 2],
}

impl Lowpass {
    fn new(cutoff: f32) -> Lowpass {
        let mut f = Lowpass { a: 0.0, y: [0.0; 2] };
        f.set(cutoff);
        f
    }

    fn set(&mut self, cutoff: f32) {
        self.a = 1.0 - (-std::f32::consts::TAU * cutoff / RATE as f32).exp();
    }

    fn apply(&mut self, x: f32) -> f32 {
        self.y[0] += self.a * (x - self.y[0]);
        self.y[1] += self.a * (self.y[0] - self.y[1]);
        self.y[1]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use g2_rules::crater_ridge;
    use g2_sim::Faction;

    /// Every cue renders to a short sound that never clips, starts and ends at silence (no
    /// click), and carries real signal.
    #[test]
    fn every_cue_is_a_short_clean_sound() {
        for cue in Cue::ALL {
            let s = synth(cue);
            let secs = s.len() as f32 / RATE as f32;
            assert!((0.04..2.0).contains(&secs), "{cue:?}: {secs} s");
            assert!(s.iter().all(|v| v.is_finite() && v.abs() <= 0.9 + 1e-6), "{cue:?} clips");
            assert!(s[0].abs() < 0.05 && s[s.len() - 1].abs() < 1e-3, "{cue:?} clicks at an edge");
            let rms = (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt();
            assert!(rms > 0.05, "{cue:?} is nearly silent (rms {rms})");
        }
    }

    /// Notifications are the local player's own: in an AI match the local side's every
    /// finished unit and building is announced, the enemy's never are.
    #[test]
    fn only_our_production_is_announced() {
        let mut sim = Sim::new(crater_ridge(), None, 3, Sim::players([Faction::Eu, Faction::Gla], true));
        let (mut ours, mut theirs, mut ready, mut built) = (0, 0, 0, 0);
        for _ in 0..30 * 240 {
            sim.world.step();
            for e in sim.world.events() {
                match *e {
                    Event::Produced { player, .. } | Event::Completed { player, .. } if player == LOCAL => ours += 1,
                    Event::Produced { .. } | Event::Completed { .. } => theirs += 1,
                    _ => {}
                }
            }
            for (cue, _) in sim_cues(&sim) {
                ready += usize::from(cue == Cue::Ready);
                built += usize::from(cue == Cue::Built);
            }
        }
        assert!(ours > 0 && theirs > 0, "both sides built something: {ours} / {theirs}");
        assert_eq!(ready + built, ours);
    }

    /// Combat is heard only where the local player sees one end of it: with the fog on, no shot
    /// in a fight between two hidden units is heard; with the fog off, every one is.
    #[test]
    fn fights_in_the_fog_are_silent() {
        let mut sim = Sim::new(crater_ridge(), None, 3, Sim::players([Faction::Eu, Faction::Gla], true));
        let (mut hidden, mut heard_hidden, mut heard_with_fog_off) = (0, 0, 0);
        for _ in 0..30 * 600 {
            sim.world.step();
            let vision = sim.world.vision(LOCAL);
            let unseen_hits = sim
                .world
                .events()
                .iter()
                .filter(|e| match **e {
                    Event::Hit { shooter, at, splash: false, .. } => {
                        let from = sim.world.entity(shooter).map_or(at, |s| s.pos);
                        !vision.is_visible(at) && !vision.is_visible(from)
                    }
                    _ => false,
                })
                .count();
            if unseen_hits == 0 {
                continue;
            }
            hidden += unseen_hits;
            let shots = |sim: &Sim| sim_cues(sim).iter().filter(|(c, _)| matches!(c, Cue::Shot { .. })).count();
            let all_hits = sim.world.events().iter().filter(|e| matches!(e, Event::Hit { splash: false, .. })).count();
            heard_hidden += all_hits - shots(&sim);
            sim.fog = false;
            heard_with_fog_off += usize::from(shots(&sim) == all_hits);
            sim.fog = true;
        }
        assert!(hidden > 0, "no fight happened out of the local player's sight");
        // Every unseen hit is exactly the part not heard.
        assert_eq!(heard_hidden, hidden);
        assert!(heard_with_fog_off > 0);
    }

    /// Through the real schedule: an order clicks once, a played minute turns into sounds with
    /// no frame starting more than `VOICES_PER_FRAME` of one kind, and muting stops new sounds
    /// while the match goes on. Counts the `AudioPlayer`s spawned; where no audio device is
    /// open, bevy leaves them unplayed.
    #[test]
    fn a_match_is_heard_until_muted() {
        use bevy::time::TimeUpdateStrategy;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), bevy::audio::AudioPlugin::default()))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(33)))
            .insert_resource(Sim::new(crater_ridge(), None, 3, Sim::players([Faction::Eu, Faction::Gla], false)))
            .init_resource::<ButtonInput<KeyCode>>()
            .add_plugins((crate::sim::SimPlugin, SoundPlugin));
        let started = |app: &mut App| {
            let mut q = app.world_mut().query_filtered::<Entity, With<AudioPlayer<Sound>>>();
            let ids: Vec<Entity> = q.iter(app.world()).collect();
            for &e in &ids {
                app.world_mut().despawn(e);
            }
            ids.len()
        };
        // An order clicks once (the match is paused, so nothing else is heard).
        app.world_mut().resource_mut::<Sim>().speed = 0;
        app.update();
        started(&mut app);
        let dozer = {
            let sim = app.world().resource::<Sim>();
            let rules = sim.world.rules;
            sim.world.entities().find(|e| e.owner == Some(LOCAL) && rules.def(e.def).is_constructor()).unwrap().id
        };
        app.world_mut()
            .resource_mut::<Sim>()
            .order(g2_sim::CommandKind::Move { units: vec![dozer], target: g2_sim::Vec2::from_int(150, 150) });
        app.update();
        assert_eq!(started(&mut app), 1, "the order's click");
        // A minute of play at ×8.
        app.world_mut().resource_mut::<Sim>().speed = 8;
        let (mut per_frame_max, mut total) = (0, 0);
        for _ in 0..30 * 60 {
            app.update();
            let n = started(&mut app);
            per_frame_max = per_frame_max.max(n);
            total += n;
        }
        assert!(total > 0, "a minute of play made no sound");
        assert!(per_frame_max <= VOICES_PER_FRAME * Cue::ALL.len(), "{per_frame_max} sounds in one frame");
        // Muted: the player keeps giving orders and the match runs on, in silence. (The
        // human side gives no orders of its own, so the clicks are what would be heard.)
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyM);
        let mut while_muted = 0;
        for k in 0..30 * 30 {
            if k % 30 == 0 {
                let target = g2_sim::Vec2::from_int(150 + k / 30, 150);
                app.world_mut().resource_mut::<Sim>().order(g2_sim::CommandKind::Move { units: vec![dozer], target });
            }
            app.update();
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
            while_muted += started(&mut app);
        }
        assert!(app.world().resource::<Muted>().0);
        assert_eq!(while_muted, 0);
    }

    /// After a match with many orders, a fresh match (the menu swaps in a new `Sim`) clicks for
    /// its very first order: acknowledgements don't wait for the old match's count.
    #[test]
    fn a_new_match_clicks_from_its_first_order() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), bevy::audio::AudioPlugin::default()))
            .init_resource::<ButtonInput<KeyCode>>()
            .add_plugins((crate::sim::SimPlugin, SoundPlugin));
        let fresh = || {
            let mut sim = Sim::new(crater_ridge(), None, 3, Sim::players([Faction::Eu, Faction::Gla], false));
            sim.speed = 0;
            sim
        };
        let clicks_for = |app: &mut App, orders: usize| {
            let dozer = {
                let sim = app.world().resource::<Sim>();
                let rules = sim.world.rules;
                sim.world.entities().find(|e| e.owner == Some(LOCAL) && rules.def(e.def).is_constructor()).unwrap().id
            };
            for k in 0..orders {
                let target = g2_sim::Vec2::from_int(100 + k as i64, 100);
                app.world_mut().resource_mut::<Sim>().order(g2_sim::CommandKind::Move { units: vec![dozer], target });
            }
            app.update();
            let mut q = app.world_mut().query_filtered::<Entity, With<AudioPlayer<Sound>>>();
            let ids: Vec<Entity> = q.iter(app.world()).collect();
            ids.iter().for_each(|&e| {
                app.world_mut().despawn(e);
            });
            ids.len()
        };
        app.insert_resource(fresh());
        app.update();
        for _ in 0..10 {
            assert_eq!(clicks_for(&mut app, 2), 2);
        }
        app.insert_resource(fresh());
        assert_eq!(clicks_for(&mut app, 1), 1, "the new match's first order");
    }
}
