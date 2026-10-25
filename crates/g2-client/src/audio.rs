//! Sound, synthesized here at startup unless the player imported the 2013 alpha
//! (`alpha.rs`): then its effects replace the synthesized cues one for one, units answer in
//! the alpha's voices, and the menu plays its music. Each synthesized cue is a fraction of a
//! second of filtered noise, sine sweeps and envelopes, rendered once into a sample buffer
//! and replayed through `bevy_audio`.
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
use crate::sim::Sim;

/// Output sample rate of every sound, synthesized or imported.
pub const RATE: u32 = 44_100;
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
            .add_systems(Update, (toggle_mute, play_cues, speak, play_music).chain().before(crate::menu::MenuSet));
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
    pub const ALL: [Cue; 11] = [
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

/// A sound at its own rate, mono or stereo (`samples` interleaved). It plays in stereo at
/// `pan` (-1 left .. 1 right): a mono sound on both sides, a stereo one each side from its
/// own channel. With `loop_from` (a frame), it plays to the end and then repeats from that
/// frame on, without end (music after its intro). The mixer resamples to the device's rate.
#[derive(Asset, TypePath, Clone)]
pub struct Sound {
    samples: Arc<[f32]>,
    channels: u16,
    rate: u32,
    pan: f32,
    loop_from: Option<usize>,
}

impl Sound {
    /// `channels` is 1 or 2.
    pub fn new(samples: Arc<[f32]>, channels: u16, rate: u32) -> Sound {
        debug_assert!(matches!(channels, 1 | 2) && samples.len().is_multiple_of(usize::from(channels)));
        Sound { samples, channels, rate, pan: 0.0, loop_from: None }
    }

    /// A synthesized cue: mono at `RATE`.
    fn synth(samples: Vec<f32>) -> Sound {
        Sound::new(samples.into(), 1, RATE)
    }

    fn panned(mut self, pan: f32) -> Sound {
        self.pan = pan;
        self
    }

    pub fn looping(mut self, from: usize) -> Sound {
        self.loop_from = Some(from);
        self
    }

    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels)
    }

    #[cfg(test)]
    pub fn rate(&self) -> u32 {
        self.rate
    }

    #[cfg(test)]
    pub fn channels(&self) -> u16 {
        self.channels
    }

    #[cfg(test)]
    pub fn loop_from(&self) -> Option<usize> {
        self.loop_from
    }
}

impl Decodable for Sound {
    type Decoder = Playback;

    fn decoder(&self) -> Playback {
        // Constant-power panning: the sum of both channels' power stays the same at any pan.
        let angle = (self.pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
        let frames = self.frames();
        Playback {
            samples: self.samples.clone(),
            channels: usize::from(self.channels),
            frames,
            rate: self.rate,
            gains: [angle.cos(), angle.sin()],
            at: 0,
            loop_from: self.loop_from.filter(|&f| f < frames),
            fade: self.rate as usize / 200,
            wrapped: false,
        }
    }
}

/// Plays a `Sound`'s frames once, interleaved left/right, or on from its loop start forever.
pub struct Playback {
    samples: Arc<[f32]>,
    channels: usize,
    frames: usize,
    rate: u32,
    gains: [f32; 2],
    /// Index into the interleaved stereo output: frame `at / 2`, channel `at % 2`.
    at: usize,
    loop_from: Option<usize>,
    /// Frames a repeat fades in over (5 ms). The alpha's music loop ends in silence and starts
    /// loud mid-phrase, which follows the intro seamlessly but would click after its own end.
    fade: usize,
    /// The loop has repeated at least once: its start now follows its end, not the intro.
    wrapped: bool,
}

impl Iterator for Playback {
    type Item = bevy::audio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        if self.at / 2 == self.frames
            && let Some(from) = self.loop_from
        {
            self.at = from * 2;
            self.wrapped = true;
        }
        let (frame, side) = (self.at / 2, self.at % 2);
        if frame >= self.frames {
            return None;
        }
        // A mono frame feeds both sides; a stereo one each side from its own channel.
        let source = frame * self.channels + side.min(self.channels - 1);
        let mut out = self.samples[source] * self.gains[side];
        if self.wrapped
            && let Some(into) = self.loop_from.map(|from| frame - from).filter(|&k| k < self.fade)
        {
            out *= into as f32 / self.fade as f32;
        }
        self.at += 1;
        Some(out)
    }
}

impl bevy::audio::Source for Playback {
    fn current_span_len(&self) -> Option<usize> {
        // A loop never ends; the format holds throughout, so there is no span to report.
        if self.loop_from.is_some() { None } else { Some(self.frames * 2 - self.at) }
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::new(2).unwrap()
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(self.rate).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        if self.loop_from.is_some() {
            return None;
        }
        Some(Duration::from_secs_f64(self.frames as f64 / f64::from(self.rate)))
    }
}

/// Pan positions a placed cue snaps to, from hard left to hard right; each is its own handle
/// on the same samples.
const PANS: usize = 9;
/// Placed cues never pan further than this, so nothing plays in one ear only.
const MAX_PAN: f32 = 0.8;

/// Each cue's sounds, one handle per (variation, pan slot); a cue with several variations
/// (the alpha's) picks one in turn.
#[derive(Resource)]
struct Bank(BTreeMap<Cue, Vec<[Handle<Sound>; PANS]>>);

/// Every unit's voice lines, by (def name, response); centred, never panned.
#[derive(Resource, Default)]
pub struct Voices(pub BTreeMap<(&'static str, crate::alpha::Response), Vec<Handle<Sound>>>);

fn load_sounds(mut commands: Commands, mut sounds: ResMut<Assets<Sound>>, cache: Option<Res<crate::alpha::Cache>>) {
    let alpha = cache.map(|c| crate::alpha::Sounds::load(&c)).unwrap_or_default();
    let mut bank = BTreeMap::new();
    for cue in Cue::ALL {
        let variations = alpha.effects.get(&cue).cloned().unwrap_or_else(|| vec![Sound::synth(synth(cue))]);
        let handles = variations
            .into_iter()
            .map(|sound| std::array::from_fn(|k| sounds.add(sound.clone().panned(pan_of_slot(k)))))
            .collect();
        bank.insert(cue, handles);
    }
    commands.insert_resource(Bank(bank));
    let voices = alpha
        .voices
        .into_iter()
        .map(|(key, v)| (key, v.into_iter().map(|sound| sounds.add(sound)).collect()))
        .collect();
    commands.insert_resource(Voices(voices));
    if let Some(music) = alpha.music {
        commands.insert_resource(Music(sounds.add(music)));
    }
}

/// The menu's music, when the alpha supplies one.
#[derive(Resource)]
pub struct Music(pub Handle<Sound>);

fn pan_of_slot(k: usize) -> f32 {
    (k as f32 / (PANS - 1) as f32 * 2.0 - 1.0) * MAX_PAN
}

/// The pan slot for a sound `offset` world units right of the camera's focus, along the
/// camera's own right direction: full pan a screen-half (`NEAR`) to the side.
fn pan_slot(offset: f32) -> usize {
    let pan = (offset / NEAR).clamp(-1.0, 1.0);
    ((pan + 1.0) / 2.0 * (PANS - 1) as f32).round() as usize
}

fn toggle_mute(keys: Res<ButtonInput<KeyCode>>, mut muted: ResMut<Muted>) {
    if keys.just_pressed(KeyCode::KeyM) {
        muted.0 = !muted.0;
    }
}

/// Plays what the sim gathered since the last frame (`sim::Heard`), a click for each order the
/// local player gave since (`Sim::unheard_orders`, drained here, so a new match starts at
/// zero) and each menu choice about to apply.
#[allow(clippy::too_many_arguments)]
fn play_cues(
    mut commands: Commands,
    mut sim: ResMut<Sim>,
    mut heard: ResMut<crate::sim::Heard>,
    pending: Option<Res<crate::menu::Pending>>,
    bank: Option<Res<Bank>>,
    muted: Res<Muted>,
    cam: Query<&RtsCamera>,
    mut turn: Local<usize>,
) {
    let mut cues = core::mem::take(&mut heard.0);
    let orders = core::mem::take(&mut sim.bypass_change_detection().unheard_orders) as usize;
    cues.extend(std::iter::repeat_n((Cue::Click, None), orders + pending.map_or(0, |p| p.0.len())));
    let Some(bank) = bank else { return };
    if muted.0 {
        return;
    }
    // The camera's focus and its right-hand direction on the ground: placed cues pan by how
    // far right of the focus they are, as seen on screen whatever the camera's yaw.
    let view = cam.iter().next().map(|c| {
        let right = c.transform().right();
        (Vec2::new(c.pivot.x, c.pivot.z), Vec2::new(right.x, right.z).normalize_or_zero())
    });
    let mut started = BTreeMap::<Cue, usize>::new();
    for (cue, at) in cues {
        let (gain, slot) = match (at, view) {
            (Some(p), Some((focus, right))) => {
                let p = Vec2::new(p.x.to_f32(), p.y.to_f32());
                (attenuation(p.distance(focus)), pan_slot((p - focus).dot(right)))
            }
            _ => (1.0, PANS / 2),
        };
        let n = started.entry(cue).or_default();
        if gain <= 0.0 || *n >= VOICES_PER_FRAME {
            continue;
        }
        *n += 1;
        // Variations in turn: a local counter, never the sim's RNG, so sound can't touch
        // the match.
        let variations = &bank.0[&cue];
        *turn = turn.wrapping_add(1);
        commands.spawn((
            AudioPlayer(variations[*turn % variations.len()][slot].clone()),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(gain * cue.level())),
        ));
    }
}

/// Seconds a unit stays quiet after speaking, so a burst of orders isn't a burst of lines.
const SPEECH_GAP: f32 = 1.5;

/// The alpha's voice lines, when imported: a unit acknowledges an order to move or attack,
/// and a new unit announces itself. One line at a time: within `SPEECH_GAP` of the last
/// line the next request passes silently.
fn speak(
    mut commands: Commands,
    mut sim: ResMut<Sim>,
    voices: Option<Res<Voices>>,
    muted: Res<Muted>,
    time: Res<Time>,
    mut quiet_until: Local<f32>,
    mut turn: Local<usize>,
) {
    use crate::alpha::Response;
    let sim = sim.bypass_change_detection();
    let (answering, rolled_out, selected) = (sim.answering.take(), sim.rolled_out.take(), sim.selected.take());
    let Some(voices) = voices.filter(|v| !v.0.is_empty()) else { return };
    let now = time.elapsed_secs();
    if muted.0 || now < *quiet_until {
        return;
    }
    let rules = sim.world.rules;
    let unit_says = |unit, r| sim.world.entity(unit).map(|e| (rules.def(e.def).name, r));
    // An order's answer beats a selection's, and both beat a new unit's announcement: the
    // player is waiting on the answer.
    let line = match (answering, selected, rolled_out) {
        (Some((unit, attack)), ..) => unit_says(unit, if attack { Response::Attack } else { Response::Move }),
        (None, Some(unit), _) => unit_says(unit, Response::Select),
        (None, None, Some(def)) => Some((rules.def(def).name, Response::Create)),
        _ => None,
    };
    let Some(lines) = line.and_then(|key| voices.0.get(&key)) else { return };
    *turn = turn.wrapping_add(1);
    let handle = lines[*turn % lines.len()].clone();
    commands.spawn((AudioPlayer(handle), PlaybackSettings::DESPAWN.with_volume(Volume::Linear(0.8))));
    *quiet_until = now + SPEECH_GAP;
}

#[derive(Component)]
struct MenuMusic;

/// The alpha's menu music while a menu covers the battlefield (main menu, setup, end
/// screen), stopped in a match: its intro once, then its loop (the sound repeats itself).
/// Silent without an import.
fn play_music(
    mut commands: Commands,
    music: Option<Res<Music>>,
    ui: Option<Res<crate::menu::Ui>>,
    muted: Res<Muted>,
    playing: Query<Entity, With<MenuMusic>>,
    movie: Option<Res<crate::movie::Playing>>,
) {
    let Some(music) = music else { return };
    // The intro movie has its own soundtrack; the music starts once it is over.
    let want = !muted.0 && movie.is_none() && ui.is_some_and(|u| u.screen != crate::menu::Screen::Match);
    match (want, playing.iter().next()) {
        (true, None) => {
            commands.spawn((
                AudioPlayer(music.0.clone()),
                PlaybackSettings::ONCE.with_volume(Volume::Linear(0.5)),
                MenuMusic,
            ));
        }
        (false, Some(e)) => commands.entity(e).despawn(),
        _ => {}
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
    let me = sim.local;
    let vision = world.vision(me);
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
            Event::Produced { player, .. } if player == me => out.push((Cue::Ready, None)),
            Event::Completed { player, .. } if player == me => out.push((Cue::Built, None)),
            // Ours always, the enemy's where it lands in sight: a power is a big moment.
            Event::PowerUsed { player, at } if player == me || seen(at) => out.push((Cue::Launch, Some(at))),
            Event::Delivered { player, depot, .. } if player == me => {
                out.push((Cue::Cash, world.entity(depot).map(|d| d.pos)));
            }
            Event::Rejected { player, .. } if player == me => out.push((Cue::Refused, None)),
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

    /// A sound with a loop plays its intro once, then repeats from the loop's start without
    /// end; one without a loop plays once and stops. Each repeat fades in over 5 ms, but the
    /// first pass from the intro into the loop is left alone (the track leads into it).
    #[test]
    fn looped_sounds_repeat_from_their_loop_start() {
        let left = |sound: Sound, n: usize| sound.decoder().step_by(2).take(n).collect::<Vec<f32>>();
        let gain = std::f32::consts::FRAC_PI_4.cos();
        // A constant 1.0 at 48 kHz (a 240-frame fade): two fades' worth of intro, then a
        // loop of three fades' worth.
        let fade = 240;
        let (intro, body) = (2 * fade, 3 * fade);
        let samples: Arc<[f32]> = vec![1.0; intro + body].into();
        let looped = Sound::new(samples.clone(), 1, 48_000).looping(intro);
        let out = left(looped, intro + 3 * body);
        assert_eq!(out.len(), intro + 3 * body, "a loop never ends");
        assert!(out[..intro + body].iter().all(|&s| s == gain), "first pass: intro into loop untouched");
        for pass in 1..3 {
            let start = intro + pass * body;
            let ramp = &out[start..start + fade];
            assert_eq!(ramp[0], 0.0, "repeat {pass} starts from silence");
            assert!(ramp.windows(2).all(|w| w[1] > w[0]), "repeat {pass} fades in");
            assert!(ramp[fade - 1] < gain && out[start + fade] == gain, "full level after the fade");
            assert!(out[start + fade..start + body].iter().all(|&s| s == gain), "rest of repeat {pass} untouched");
        }
        let once = left(Sound::new(samples, 1, 48_000), intro + 3 * body);
        assert_eq!(once.len(), intro + body, "without a loop: played once");
    }

    /// A stereo sound keeps its channels apart (left from left, right from right) at its own
    /// rate, at the same centred level as a mono one; a mono one feeds both sides.
    #[test]
    fn stereo_sounds_keep_their_channels_and_rate() {
        let gain = std::f32::consts::FRAC_PI_4.cos();
        let stereo = Sound::new([0.5, -0.25, 1.0, 0.0].into(), 2, 48_000);
        let d = stereo.decoder();
        assert_eq!((bevy::audio::Source::sample_rate(&d).get(), bevy::audio::Source::channels(&d).get()), (48_000, 2));
        assert_eq!(d.collect::<Vec<f32>>(), [0.5 * gain, -0.25 * gain, gain, 0.0]);
        let mono = Sound::new([0.5, 1.0].into(), 1, 44_100);
        assert_eq!(mono.decoder().collect::<Vec<f32>>(), [0.5 * gain, 0.5 * gain, gain, gain]);
        assert_eq!(bevy::audio::Source::total_duration(&mono.decoder()), Some(Duration::from_secs_f64(2.0 / 44_100.0)));
    }

    /// A sound right of the camera's focus is louder in the right channel and one on the left
    /// in the left, mirrored; one at the focus is balanced; and panning keeps the total power
    /// (so a sound doesn't get louder or quieter as it crosses the screen).
    #[test]
    fn sounds_pan_with_their_side_of_the_screen() {
        let channels = |offset: f32| {
            let sound = Sound::synth(synth(Cue::Click)).panned(pan_of_slot(pan_slot(offset)));
            let out: Vec<f32> = sound.decoder().collect();
            let power = |ch: usize| out.iter().skip(ch).step_by(2).map(|v| v * v).sum::<f32>();
            (power(0), power(1))
        };
        let (l0, r0) = channels(0.0);
        assert!((l0 - r0).abs() < 1e-3 * l0, "centred: {l0} / {r0}");
        let (lr, rr) = channels(NEAR);
        let (ll, rl) = channels(-NEAR);
        assert!(rr > 5.0 * lr, "right: {lr} / {rr}");
        assert!((lr - rl).abs() < 1e-3 * l0 && (rr - ll).abs() < 1e-3 * l0, "mirrored");
        assert!(lr > 0.0, "never in one ear only");
        for offset in [-200.0, -NEAR / 2.0, 7.0, NEAR / 3.0, 500.0] {
            let (l, r) = channels(offset);
            assert!(((l + r) - (l0 + r0)).abs() < 1e-3 * (l0 + r0), "power at {offset}");
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
                    Event::Produced { player, .. } | Event::Completed { player, .. } if player == sim.local => {
                        ours += 1
                    }
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
            let vision = sim.world.vision(sim.local);
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

    /// Left and right are the screen's: a point the in-game camera draws right of the screen's
    /// centre pans right, at every yaw the camera can turn to.
    #[test]
    fn the_right_of_the_screen_pans_right() {
        for yaw in [0.0, 0.7, std::f32::consts::PI, -2.0] {
            let cam =
                RtsCamera { pivot: Vec3::new(100.0, 0.0, 100.0), distance: 105.0, yaw, pitch: crate::look::PITCH };
            let tf = cam.transform();
            let right = tf.right();
            let ground_right = Vec2::new(right.x, right.z).normalize();
            let p = Vec3::new(100.0, 0.0, 100.0) + Vec3::new(ground_right.x, 0.0, ground_right.y) * 30.0;
            // Camera space: +X is right on screen.
            let in_view = tf.compute_affine().inverse().transform_point3(p);
            assert!(in_view.x > 0.0, "yaw {yaw}: the point is drawn left of centre");
            let offset = (Vec2::new(p.x, p.z) - Vec2::new(100.0, 100.0)).dot(ground_right);
            assert!(pan_of_slot(pan_slot(offset)) > 0.0, "yaw {yaw}");
        }
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
            sim.world.entities().find(|e| e.owner == Some(sim.local) && rules.def(e.def).is_constructor()).unwrap().id
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

    /// With voice lines loaded: an order is answered by the unit given it, as a move or an
    /// attack line of its own kind; a second order within `SPEECH_GAP` passes silently; and
    /// selecting answers with a select line. Lines are tagged by their sample value.
    #[test]
    fn ordered_units_answer_once_per_gap_in_their_own_voice() {
        use crate::alpha::Response;
        use bevy::time::TimeUpdateStrategy;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), bevy::audio::AudioPlugin::default()))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(100)))
            .insert_resource(Sim::new(crater_ridge(), None, 3, Sim::players([Faction::Eu, Faction::Gla], false)))
            .init_resource::<ButtonInput<KeyCode>>()
            .add_plugins((crate::sim::SimPlugin, SoundPlugin));
        app.world_mut().resource_mut::<Sim>().speed = 0;
        app.update();
        // Replace the (empty) bank: each (def, response) line is one sample of a distinct value.
        let rules = g2_rules::rules();
        let mut voices = Voices::default();
        for (i, def) in rules.defs.iter().enumerate() {
            for (k, r) in Response::ALL.into_iter().enumerate() {
                let tag = (i * 4 + k) as f32;
                let h = app.world_mut().resource_mut::<Assets<Sound>>().add(Sound::new(vec![tag].into(), 1, RATE));
                voices.0.insert((def.name, r), vec![h]);
            }
        }
        let tag_of = |name: &str, r: Response| {
            let i = rules.defs.iter().position(|d| d.name == name).unwrap();
            (i * 4 + Response::ALL.iter().position(|&x| x == r).unwrap()) as f32
        };
        app.insert_resource(voices);
        let spoken = |app: &mut App| -> Vec<f32> {
            let mut q = app.world_mut().query::<(Entity, &AudioPlayer<Sound>)>();
            let found: Vec<(Entity, Handle<Sound>)> = q.iter(app.world()).map(|(e, p)| (e, p.0.clone())).collect();
            let sounds = app.world().resource::<Assets<Sound>>();
            // Voices are single-sample; clicks and cues are longer and are left out.
            let tags = found
                .iter()
                .filter_map(|(_, h)| sounds.get(h))
                .filter(|s| s.samples.len() == 1)
                .map(|s| s.samples[0])
                .collect();
            for (e, _) in found {
                app.world_mut().despawn(e);
            }
            tags
        };
        let (dozer, dozer_name) = {
            let sim = app.world().resource::<Sim>();
            let e =
                sim.world.entities().find(|e| e.owner == Some(sim.local) && rules.def(e.def).is_constructor()).unwrap();
            (e.id, rules.def(e.def).name)
        };
        let order = |app: &mut App, x| {
            let target = g2_sim::Vec2::from_int(x, 150);
            app.world_mut().resource_mut::<Sim>().order(g2_sim::CommandKind::Move { units: vec![dozer], target });
        };
        order(&mut app, 150);
        app.update();
        assert_eq!(spoken(&mut app), [tag_of(dozer_name, Response::Move)]);
        // Within the gap: the next order is acknowledged by a click only.
        order(&mut app, 151);
        app.update();
        assert!(spoken(&mut app).is_empty());
        // After the gap (frames of 100 ms), selecting the unit answers with a select line.
        for _ in 0..20 {
            app.update();
        }
        app.world_mut().resource_mut::<Sim>().selected = Some(dozer);
        app.update();
        assert_eq!(spoken(&mut app), [tag_of(dozer_name, Response::Select)]);
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
                sim.world
                    .entities()
                    .find(|e| e.owner == Some(sim.local) && rules.def(e.def).is_constructor())
                    .unwrap()
                    .id
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

    /// Through the menu: each choice clicks once (Surrender too, though it is also an order),
    /// and a match's cues still queued when the next match starts are dropped, not heard over it.
    #[test]
    fn menu_choices_click_once_and_a_new_match_starts_quiet() {
        use crate::menu::{Act, MenuPlugin, Pending};
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), bevy::audio::AudioPlugin::default()))
            .insert_resource(Sim::new(crater_ridge(), None, 3, Sim::players([Faction::Eu, Faction::Gla], false)))
            .init_resource::<crate::input::Selection>()
            .init_resource::<crate::input::Placement>()
            .init_resource::<crate::view::Mirror>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_plugins((crate::sim::SimPlugin, MenuPlugin { blocking: true }, SoundPlugin));
        // Two frames per choice: the sound runs before the menu applies it, so an order the
        // choice gives is heard on the next frame.
        let act = |app: &mut App, a: Act| {
            app.world_mut().resource_mut::<Pending>().0.push(a);
            let mut n = 0;
            for _ in 0..2 {
                app.update();
                let mut q = app.world_mut().query_filtered::<Entity, With<AudioPlayer<Sound>>>();
                let ids: Vec<Entity> = q.iter(app.world()).collect();
                ids.iter().for_each(|&e| {
                    app.world_mut().despawn(e);
                });
                n += ids.len();
            }
            n
        };
        act(&mut app, Act::Start);
        assert_eq!(act(&mut app, Act::Pause), 1, "pause");
        assert_eq!(act(&mut app, Act::Surrender), 1, "surrender: one click, not two");
        // A blast of the old match still queued (gathered by a fixed step after this frame's
        // sound ran) when the next match is swapped in: the new match must not hear it.
        app.world_mut().resource_mut::<crate::sim::Heard>().0.push((Cue::Blast, None));
        let fresh = Sim::new(crater_ridge(), None, 4, Sim::players([Faction::Eu, Faction::Gla], false));
        crate::menu::swap_world(app.world_mut(), fresh);
        assert!(app.world().resource::<crate::sim::Heard>().0.is_empty(), "the swap dropped the old match's cues");
    }
}
