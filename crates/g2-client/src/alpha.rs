//! Optional content from the 2013 closed alpha, used when the player has imported their own
//! copy (`g2-tools alpha DIR import`): its unit and building models, sound effects, unit
//! voice responses and menu music. Without the cache everything stays procedural, and the
//! game plays the same either way: this changes looks and sounds only.
//!
//! The cache is EA's content decoded on the player's machine. Nothing from it is part of
//! this project, and the mapping below names asset paths only.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use g2_sim::Def;

use crate::audio::{Cue, Sound};

/// Before `DefaultPlugins`: when an import exists, its folder becomes the `alpha://` asset
/// source and the `Cache` resource. Without one, nothing changes.
pub fn register(app: &mut App) {
    let Some(cache) = Cache::find() else { return };
    let root = cache.0.to_string_lossy().into_owned();
    app.register_asset_source("alpha", bevy::asset::io::AssetSourceBuilder::platform_default(&root, None))
        .insert_resource(cache);
}

/// The imported cache, when there is one with models and sounds in it.
#[derive(Resource, Clone)]
pub struct Cache(pub PathBuf);

impl Cache {
    /// `$G2_ALPHA_CACHE`, else the importer's default folder; `None` when it holds no
    /// import (the client then runs on its own content).
    pub fn find() -> Option<Cache> {
        let dir = g2_frostbite::import::default_cache()?;
        dir.join("manifest.txt").is_file().then_some(Cache(dir))
    }

    /// The model for one of our defs: its path in the `alpha` asset source and its bounding
    /// box (min, max) from the importer's `models/bounds.txt`.
    pub fn model(&self, def: &Def, bounds: &BTreeMap<String, (Vec3, Vec3)>) -> Option<(String, (Vec3, Vec3))> {
        let rel = model_for(def.name)?;
        let asset = format!("_rts/art/{rel}");
        let b = *bounds.get(&asset)?;
        self.0.join(format!("models/{asset}.glb")).is_file().then(|| (format!("models/{asset}.glb"), b))
    }

    /// `models/bounds.txt`: asset path → (min, max).
    pub fn bounds(&self) -> BTreeMap<String, (Vec3, Vec3)> {
        let text = std::fs::read_to_string(self.0.join("models/bounds.txt")).unwrap_or_default();
        text.lines()
            .filter_map(|l| {
                let mut it = l.split(' ');
                let name = it.next()?.to_owned();
                let v: Vec<f32> = it.map(str::parse).collect::<Result<_, _>>().ok()?;
                (v.len() == 6).then(|| (name, (Vec3::new(v[0], v[1], v[2]), Vec3::new(v[3], v[4], v[5]))))
            })
            .collect()
    }

    /// Every variation of one sound asset (`<path>.wav` or `<path> N.wav`), in order.
    fn variations(&self, asset: &str) -> Vec<PathBuf> {
        let one = self.0.join(format!("{asset}.wav"));
        if one.is_file() {
            return vec![one];
        }
        (0..).map(|i| self.0.join(format!("{asset} {i}.wav"))).take_while(|p| p.is_file()).collect()
    }
}

/// Our defs' look-alikes in the alpha, by role: each faction's own building or the unit of
/// the same job (an Aegis Tank is the EU medium cannon tank). Infantry use one soldier;
/// the client places it three times as a squad.
fn model_for(def: &str) -> Option<&'static str> {
    Some(match def {
        "EU Command Center" => "_eur/es_commandcenter/es_commandcenter_mesh",
        "EU Bulldozer" => "_eur/eu_constructionunit/eu_constructionunit_skn_mesh",
        "EU Supply Center" => "_eur/es_supplycenter/es_supplycenter_mesh",
        "EU Harvester" => "_eur/eu_harvester/eu_harvester_skn_mesh",
        "EU Power Plant" => "_eur/es_powerplant/es_powerplant_mesh",
        "EU Oil Derrick" => "_eur/es_oilderrick/es_oilderrick_mesh",
        "EU Barracks" => "_eur/es_barracks/es_barracks_mesh",
        "EU War Factory" => "_eur/es_warfactory/es_warfactory_mesh",
        "EU Recon Squad" => "_eur/eu_riflesoldier/eu_riflesoldier_skn_mesh",
        "EU Rocket Squad" => "_eur/eu_rocketsoldier/eu_rocketsoldier_skn_mesh",
        "EU Aegis Tank" => "_eur/eu_mediumcannontank/eu_mediumcannontank_skn_mesh",
        "EU Howitzer" => "_eur/eu_mediumrocketvehicle/eu_mediumrocketvehicle_skn_mesh",
        "EU APC" => "_eur/eu_apc/eu_apc_skn_mesh",
        "APA Command Center" => "_asia/as_commandcenter/as_commandcenter_mesh",
        "APA Crane" => "_asia/au_constructionunit/au_constructionunit_skn_mesh",
        "APA Supply Center" => "_asia/as_supplycenter/as_supplycenter_mesh",
        "APA Harvester" => "_asia/au_harvester/au_harvester_skn_mesh",
        "APA Power Plant" => "_asia/as_powerplant/as_powerplant_mesh",
        "APA Oil Derrick" => "_asia/as_oilderrick/as_oilderrick_mesh",
        "APA Barracks" => "_asia/as_barracks/as_barracks_mesh",
        "APA War Factory" => "_asia/as_warfactory/as_warfactory_mesh",
        "APA Red Guard" => "_asia/au_riflesoldier/au_riflesoldier01_skn_mesh",
        "APA Tank Hunter" => "_asia/au_rocketsoldier/au_rocketsoldier_skn_mesh",
        "APA Warlord Tank" => "_asia/au_mediumcannontank/au_mediumcannontank_skn_mesh",
        "APA Thunderfist" => "_asia/au_heavycannonartillery/au_heavycannonartillery_skn_mesh",
        "APA Gatling Tank" => "_asia/au_mediumguntank/au_mediumguntank_skn_mesh",
        "GLA Command Center" => "_gla/gs_commandcenter/gs_commandcenter_mesh",
        "GLA Worker" => "_gla/gu_constructionunit/gu_constructionunit01_skn_mesh",
        "GLA Supply Stash" => "_gla/gs_supplycenter/gs_supplycenter_mesh",
        "GLA Oil Derrick" => "_gla/gs_oilderrick/gs_oilderrick_mesh",
        "GLA Barracks" => "_gla/gs_barracks/gs_barracks_mesh",
        "GLA War Factory" => "_gla/gs_warfactory/gs_warfactory_mesh",
        "GLA Rabble Squad" => "_gla/gu_rebelsoldier/gu_rebelsoldier01_skn_mesh",
        "GLA RPG Squad" => "_gla/gu_rocketsoldier/gu_rocketsoldier01_skn_mesh",
        "GLA Scorpion Tank" => "_gla/gu_mediumcannontank/gu_mediumcannontank_skn_mesh",
        "GLA Toxin Artillery" => "_gla/gu_heavytoxinartillery/gu_heavytoxinartillery_skn_mesh",
        "GLA Technical" => "_gla/gu_lightreconvehicle/gu_lightreconvehicle_skn_mesh",
        "EU Tech Lab" => "_eur/es_techstructurea/es_techstructurea_mesh",
        "EU Advanced Tech Lab" => "_eur/es_techstructureb/es_techstructureb_mesh",
        "GLA Salvage Bay" => "_gla/gs_techstructureb/gs_techstructureb_mesh",
        "EU Airfield" => "_eur/es_airfield/es_airfield_mesh",
        "APA Airfield" => "_asia/as_airfield/as_airfield_mesh",
        "EU Valkyrie" => "_eur/eu_heavyrockethelo/eu_heavyrockethelo_skn_mesh",
        "APA Mantis" => "_asia/au_lightgunhelo/au_lightgunhelo_skn_mesh",
        "GLA Toxin Chopper" => "_gla/gu_lightchemicalhelo/gu_lightchemicalhelo_skn_mesh",
        "EU Bomber" => "_eur/eu_airbomber/eu_airbomber_airraid_mesh",
        "APA Bomber" => "_asia/au_airbomber/au_airbomber_mesh",
        "APA Fighter" => "_asia/au_airfighter/au_airfighter_spysortie2_mesh",
        "Shanty" => "civilian/africa/afri_cb_shanty/afri_cb_shanty_05_mesh",
        "Town House" => "civilian/africa/afri_cb_town/afri_cb_town_09_mesh",
        "Urban Block" => "civilian/africa/afri_cb_urban/afri_cb_urban_03_mesh",
        "EU Gun Turret" => "_eur/es_basedefensegun/es_basedefensegun_mesh",
        "APA Flame Turret" => "_asia/as_basedefenseflame/as_basedefenseflame_mesh",
        "GLA Rocket Site" => "_gla/gs_basedefenserocket/gs_basedefenserocket_mesh",
        _ => return None,
    })
}

/// The voice lines for our units: (folder under `sound/vo/eng/waves/unit_responses/`,
/// line prefix). The prefix is the folder's unit name except where the alpha's files differ.
fn voice_for(def: &str) -> Option<(&'static str, &'static str)> {
    Some(match def {
        "EU Bulldozer" => ("eu/eu_constructionunit", "eu_constructionunit"),
        "EU Harvester" => ("eu/eu_harvester", "eu_harvester"),
        "EU Recon Squad" => ("eu/eu_riflesoldiersquad", "eu_riflesoldiersquad"),
        "EU Rocket Squad" => ("eu/eu_rocketsoldier", "eu_rocketsoldiersquad"),
        "EU Aegis Tank" => ("eu/eu_mediumcannontank", "eu_mediumcannontank"),
        "EU Howitzer" => ("eu/eu_mediumrocketvehicle", "eu_mediumrocketvehicle"),
        "EU APC" => ("eu/eu_apc", "eu_apc"),
        "APA Crane" => ("apa/au_constructionunit", "au_constructionunit"),
        "APA Harvester" => ("apa/au_harvester", "au_harvester"),
        "APA Red Guard" => ("apa/au_riflesoldiersquad", "au_riflesoldiersquad"),
        "APA Tank Hunter" => ("apa/au_rocketsoldiersquad", "au_rocketsoldiersquad"),
        "APA Warlord Tank" => ("apa/au_mediumcannontank", "au_mediumcannontank"),
        "APA Thunderfist" => ("apa/au_heavycannonartillery", "au_heavycannonartillery"),
        "APA Gatling Tank" => ("apa/au_mediumguntank", "au_mediumguntank"),
        "GLA Worker" => ("gla/gu_constructionunit", "gu_constructionunit"),
        "GLA Rabble Squad" => ("gla/gu_rebelsoldiersquad", "gu_rebelsoldiersquad"),
        "GLA RPG Squad" => ("gla/gu_rocketsoldiersquad", "gu_rocketsoldiersquad"),
        "GLA Scorpion Tank" => ("gla/gu_mediumcannontank", "gu_mediumcannontank"),
        "GLA Toxin Artillery" => ("gla/gu_heavytoxinartillery", "gu_heavytoxinartillery"),
        "GLA Technical" => ("gla/gu_lightreconvehicle", "gu_lightreconvehicle"),
        "EU Valkyrie" => ("eu/eu_heavyrockethelo", "eu_heavyrockethelo"),
        "APA Mantis" => ("apa/au_lightgunhelo", "au_lightgunhelo"),
        "GLA Toxin Chopper" => ("gla/gu_lightchemicalhelo", "gu_lightchemicalhelo"),
        _ => return None,
    })
}

/// What a unit says.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Response {
    Create,
    Select,
    Move,
    Attack,
}

impl Response {
    pub const ALL: [Response; 4] = [Response::Create, Response::Select, Response::Move, Response::Attack];

    fn suffix(self) -> &'static str {
        match self {
            Response::Create => "create",
            Response::Select => "select",
            Response::Move => "move",
            Response::Attack => "attack",
        }
    }
}

/// The alpha's sound for each of our cues, by asset path.
fn effect_for(cue: Cue) -> &'static str {
    match cue {
        Cue::Shot { heavy: false } => "sound/sfx/eu/eu_riflesoldier/waves/eu_riflesoldier_wpnfire",
        Cue::Shot { heavy: true } => "sound/sfx/eu/eu_mediumcannontank/waves/eu_mediumcannontank_wpnfire",
        Cue::Launch => "sound/sfx/apa/au_heavycannonartillery/waves/au_heavycannonartillery_wpnfire",
        Cue::Blast => "sound/sfx/neutral/weapon_impacts/waves/shell_dirt_impact",
        Cue::Kill { building: false } => "sound/sfx/eu/eu_mediumcannontank/waves/eu_mediumcannontank_death",
        Cue::Kill { building: true } => "sound/sfx/neutral/structures/waves/structure_destruction_metal",
        Cue::Ready => "sound/sfx/neutral/ui/waves/ui_production_complete",
        Cue::Built => "sound/sfx/neutral/structures/waves/eu_structure_deploy",
        Cue::Cash => "sound/sfx/neutral/structures/waves/np_supplycrate_pickup",
        Cue::Click => "sound/sfx/neutral/ui/waves/ui_default",
        Cue::Refused => "sound/sfx/neutral/ui/waves/ui_error",
        Cue::Promoted => "sound/sfx/neutral/ui/waves/ui_upgrade_unlocked",
    }
}

/// The menu's music: the alpha's main-menu loop.
pub const MENU_MUSIC: &str = "sound/music/waves/xlevel_menumusic_loop_v1";

/// Decoded alpha sounds, each at its own channels and rate: the cue effects and every
/// unit's voice lines, as variations, and the menu music.
#[derive(Resource, Default)]
pub struct Sounds {
    pub effects: BTreeMap<Cue, Vec<Sound>>,
    pub voices: BTreeMap<(&'static str, Response), Vec<Sound>>,
    /// Plays its intro once, then repeats its loop.
    pub music: Option<Sound>,
}

impl Sounds {
    pub fn load(cache: &Cache) -> Sounds {
        let read = |paths: Vec<PathBuf>| -> Vec<Sound> {
            paths.iter().filter_map(|p| read_wav(p).ok()).map(Wav::into_sound).collect()
        };
        let mut s = Sounds::default();
        for cue in Cue::ALL {
            let v = read(cache.variations(effect_for(cue)));
            if !v.is_empty() {
                s.effects.insert(cue, v);
            }
        }
        for def in g2_rules::rules().defs {
            let Some((folder, unit)) = voice_for(def.name) else { continue };
            for r in Response::ALL {
                let asset = format!("sound/vo/eng/waves/unit_responses/{folder}/{unit}_{}", r.suffix());
                let v = read(cache.variations(&asset));
                if !v.is_empty() {
                    s.voices.insert((def.name, r), v);
                }
            }
        }
        // The track is an intro then a loop: it repeats from the loop's start, or from the top
        // when the file names no loop. Whatever follows the loop (nothing, in the alpha's
        // menu music) is never reached, so it is cut.
        s.music = cache.variations(MENU_MUSIC).first().and_then(|p| read_wav(p).ok()).map(|mut w| {
            let (from, to) = w.looped.clone().map_or((0, w.frames()), |l| (l.start, l.end));
            w.samples.truncate(to * usize::from(w.channels));
            w.into_sound().looping(from)
        });
        s
    }
}

/// A WAV read by [`read_wav`]: its samples (interleaved, at most two channels), its rate,
/// and the frames its loop covers, if any.
pub struct Wav {
    pub samples: Vec<f32>,
    pub channels: u16,
    pub rate: u32,
    pub looped: Option<std::ops::Range<usize>>,
}

impl Wav {
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels)
    }

    pub fn into_sound(self) -> Sound {
        Sound::new(self.samples.into(), self.channels, self.rate)
    }
}

/// A 16-bit PCM WAV, kept at its own rate. Mono and stereo stay as they are; more channels
/// (two of the alpha's ambience loops are quad) fold to stereo, front and rear summed per
/// side. A `smpl` chunk's first loop (frames, last one inclusive) is kept.
pub fn read_wav(path: &Path) -> Result<Wav, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let bad = || format!("{}: not a 16-bit PCM WAV", path.display());
    if bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err(bad());
    }
    let (mut channels, mut src_rate, mut data, mut looped) = (0usize, 0u32, None, None);
    let mut p = 12;
    while p + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[p + 4..p + 8].try_into().unwrap()) as usize;
        let body = bytes.get(p + 8..p + 8 + size).ok_or_else(bad)?;
        match &bytes[p..p + 4] {
            b"fmt " if body.len() >= 16 => {
                if u16::from_le_bytes([body[0], body[1]]) != 1 || u16::from_le_bytes([body[14], body[15]]) != 16 {
                    return Err(bad());
                }
                channels = usize::from(u16::from_le_bytes([body[2], body[3]]));
                src_rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
            }
            b"data" => data = Some(body),
            // Loop count at 28, the first loop's start and end frames at 44 and 48.
            b"smpl" if body.len() >= 60 && body[28..32] != [0; 4] => {
                let word = |at: usize| u32::from_le_bytes(body[at..at + 4].try_into().unwrap()) as usize;
                looped = Some(word(44)..word(48) + 1);
            }
            _ => {}
        }
        p += 8 + size + size % 2;
    }
    let data = data.ok_or_else(bad)?;
    if channels == 0 || src_rate == 0 {
        return Err(bad());
    }
    let sample = |s: &[u8; 2]| f32::from(i16::from_le_bytes(*s)) / 32768.0;
    let frames = data.as_chunks::<2>().0.chunks_exact(channels);
    let (samples, out_channels): (Vec<f32>, u16) = match channels {
        1 | 2 => (frames.flatten().map(sample).collect(), channels as u16),
        // Even channels to the left, odd to the right (L, R, rear L, rear R), averaged.
        _ => (
            frames
                .flat_map(|f| {
                    let side = |odd: usize| {
                        let (sum, n) = f.iter().skip(odd).step_by(2).fold((0.0, 0), |(s, n), x| (s + sample(x), n + 1));
                        sum / n as f32
                    };
                    [side(0), side(1)]
                })
                .collect(),
            2,
        ),
    };
    let frames = samples.len() / usize::from(out_channels);
    let looped = looped.filter(|l: &std::ops::Range<usize>| l.start < l.end && l.end <= frames);
    Ok(Wav { samples, channels: out_channels, rate: src_rate, looped })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_def_has_a_model_and_every_unit_a_voice() {
        for def in g2_rules::rules().defs {
            let civilian = def.role() == Some(g2_sim::BuildingRole::Civilian);
            if def.faction.is_none() && !civilian {
                continue;
            }
            assert!(model_for(def.name).is_some(), "{} has no alpha model", def.name);
            // Support jets are never selected or ordered, so they have nothing to answer.
            if !def.is_building() && !def.is_support_jet() {
                assert!(voice_for(def.name).is_some(), "{} has no alpha voice", def.name);
            }
        }
    }

    /// Against a real import (`G2_ALPHA_CACHE`; skipped when unset): every def's model, every
    /// cue's effect, every unit's responses (attack only for armed units: constructors and
    /// collectors have none in the alpha either) and the menu music resolve to decodable files.
    #[test]
    fn every_mapped_asset_exists_in_an_import() {
        let Some(dir) = std::env::var_os("G2_ALPHA_CACHE") else { return };
        let cache = Cache(PathBuf::from(dir));
        let bounds = cache.bounds();
        let civilian = |d: &&Def| d.role() == Some(g2_sim::BuildingRole::Civilian);
        for def in g2_rules::rules().defs.iter().filter(|d| d.faction.is_some() || civilian(d)) {
            assert!(cache.model(def, &bounds).is_some(), "{}: model missing from the import", def.name);
        }
        let s = Sounds::load(&cache);
        for cue in Cue::ALL {
            assert!(s.effects.contains_key(&cue), "{cue:?}: effect missing");
        }
        for def in g2_rules::rules().defs.iter().filter(|d| voice_for(d.name).is_some()) {
            for r in Response::ALL {
                if r == Response::Attack && def.weapon().is_none() {
                    continue;
                }
                assert!(s.voices.contains_key(&(def.name, r)), "{} {r:?}: voice missing", def.name);
            }
        }
        // The menu music, stereo at 48 kHz as the alpha has it: its intro segment (175.213 s)
        // once, then its loop segment (150.752 s) repeats.
        let music = s.music.expect("menu music");
        assert_eq!((music.channels(), music.rate()), (2, 48_000));
        let from = music.loop_from().expect("music loops");
        let secs = |n: usize| n as f32 / music.rate() as f32;
        assert!((secs(from) - 175.213).abs() < 0.01, "loop starts after the intro, at {} s", secs(from));
        assert!((secs(music.frames() - from) - 150.75).abs() < 0.01, "loop lasts {} s", secs(music.frames() - from));
    }

    fn wav(channels: u16, rate: u32, samples: &[i16], looped: Option<(u32, u32)>) -> Vec<u8> {
        let mut w = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        w.extend(16u32.to_le_bytes());
        w.extend(1u16.to_le_bytes());
        w.extend(channels.to_le_bytes());
        w.extend(rate.to_le_bytes());
        w.extend((rate * u32::from(channels) * 2).to_le_bytes());
        w.extend((channels * 2).to_le_bytes());
        w.extend(16u16.to_le_bytes());
        w.extend(b"LIST");
        w.extend(3u32.to_le_bytes());
        w.extend([1, 2, 3, 0]); // odd-sized chunk plus its pad byte
        w.extend(b"data");
        w.extend(((samples.len() * 2) as u32).to_le_bytes());
        for s in samples {
            w.extend(s.to_le_bytes());
        }
        if let Some((start, last)) = looped {
            w.extend(b"smpl");
            w.extend(60u32.to_le_bytes());
            for v in [0, 0, 0, 60, 0, 0, 0, 1, 0, 0, 0, start, last, 0, 0] {
                w.extend(u32::to_le_bytes(v));
            }
        }
        w
    }

    /// Mono and stereo WAVs read as they are, at their own rate; four channels fold to
    /// stereo by side; a `smpl` loop is kept in frames.
    #[test]
    fn wavs_keep_their_channels_rate_and_loop() {
        let dir = std::env::temp_dir().join(format!("g2-alpha-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.wav");
        let read = |bytes: Vec<u8>| {
            std::fs::write(&path, bytes).unwrap();
            read_wav(&path)
        };
        // Stereo at 48 kHz: frames (L, R) = (16384, 0), (0, -16384), (16384, 16384).
        let w = read(wav(2, 48_000, &[16384, 0, 0, -16384, 16384, 16384], None)).unwrap();
        assert_eq!((w.channels, w.rate, w.frames(), w.looped.clone()), (2, 48_000, 3, None));
        assert_eq!(w.samples, [0.5, 0.0, 0.0, -0.5, 0.5, 0.5]);
        let w = read(wav(1, 44_100, &[16384, -8192], None)).unwrap();
        assert_eq!((w.channels, w.rate, w.samples.as_slice()), (1, 44_100, [0.5, -0.25].as_slice()));
        // Quad (L, R, rear L, rear R): each side averages its front and rear.
        let w = read(wav(4, 48_000, &[16384, 0, 0, -16384], None)).unwrap();
        assert_eq!((w.channels, w.samples.as_slice()), (2, [0.25, -0.25].as_slice()));
        // A loop over frames 1..=2 (the last two).
        let w = read(wav(2, 48_000, &[16384, 0, 0, -16384, 16384, 16384], Some((1, 2)))).unwrap();
        assert_eq!(w.looped, Some(1..3));
        assert!(read(b"RIFF....WAVEjunk".to_vec()).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
