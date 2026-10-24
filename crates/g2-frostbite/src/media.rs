//! Sounds and movies, located through their ebx assets.

use crate::audio::{self, Pcm};
use crate::ebx::{Link, Partition, Value};
use crate::{Alpha, Error};

/// A `SoundWaveAsset`'s variations in one language: each is a list of segments, decoded
/// in order to give the variation's audio (music is an intro segment then a loop).
pub struct SoundWave {
    pub name: String,
    pub variations: Vec<Variation>,
}

pub struct Variation {
    pub chunk: [u8; 16],
    /// Each segment's `SamplesOffset` into the chunk: where its SPS stream starts.
    pub segments: Vec<usize>,
}

/// The language whose voice-over the alpha ships (`Loc/en.sb`). Other languages are listed
/// in the assets but their chunks are not in the install.
pub const LANGUAGE: &str = "English";

impl SoundWave {
    /// Reads a sound wave asset; `language_names` maps the audio system's `AudioLanguage`
    /// instance GUIDs to names (see [`languages`]).
    pub fn read(p: &Partition, language_names: &[([u8; 16], String)]) -> Result<SoundWave, Error> {
        let prim = p.primary().ok_or(Error::Bad("sound wave without a primary instance"))?;
        if prim.ty() != Some("SoundWaveAsset") {
            return Err(Error::Bad("not a SoundWaveAsset"));
        }
        let name = prim.get("Name").and_then(Value::str).unwrap_or_default().to_owned();
        let chunks: Vec<[u8; 16]> = prim
            .get("Chunks")
            .map(Value::items)
            .unwrap_or_default()
            .iter()
            .map(|c| c.get("ChunkId").and_then(Value::guid).ok_or(Error::Bad("sound chunk without id")))
            .collect::<Result<_, _>>()?;
        let mut all = Vec::new();
        for l in prim.get("Variations").map(Value::items).unwrap_or_default() {
            let v = l.link().and_then(|l| p.resolve(l)).ok_or(Error::Bad("unresolved sound variation"))?;
            let index = v.get("ChunkIndex").and_then(Value::int).ok_or(Error::Bad("variation without chunk"))?;
            let chunk = *chunks.get(index as usize).ok_or(Error::Bad("variation chunk out of range"))?;
            let segments = v
                .get("Segments")
                .map(Value::items)
                .unwrap_or_default()
                .iter()
                .map(|s| {
                    let offset = s.get("SamplesOffset").and_then(Value::int).ok_or(Error::Bad("segment offset"))?;
                    usize::try_from(offset).map_err(|_| Error::Bad("segment offset"))
                })
                .collect::<Result<Vec<_>, Error>>()?;
            all.push(Variation { chunk, segments });
        }
        let localization = prim.get("Localization").map(Value::items).unwrap_or_default();
        if localization.is_empty() {
            return Ok(SoundWave { name, variations: all });
        }
        for info in localization {
            let lang = match info.get("Language").and_then(Value::link) {
                Some(Link::External(i)) => p.imports.get(i).map(|(_, instance)| *instance),
                _ => None,
            };
            let is_shipped =
                lang.and_then(|g| language_names.iter().find(|(id, _)| *id == g)).is_some_and(|(_, n)| n == LANGUAGE);
            if is_shipped {
                let first = info.get("FirstVariationIndex").and_then(Value::int).unwrap_or(0) as usize;
                let count = info.get("VariationCount").and_then(Value::int).unwrap_or(0) as usize;
                let keep = first..first + count;
                let variations =
                    all.into_iter().enumerate().filter(|(i, _)| keep.contains(i)).map(|(_, v)| v).collect();
                return Ok(SoundWave { name, variations });
            }
        }
        Err(Error::Missing(format!("{name}: no {LANGUAGE} variations")))
    }
}

/// `AudioLanguage` instances of the audio system: (instance GUID, name).
pub fn languages(alpha: &Alpha) -> Result<Vec<([u8; 16], String)>, Error> {
    let p = alpha.asset("sound/systems/audiosystem")?;
    Ok(p.instances
        .iter()
        .filter(|i| i.value.ty() == Some("AudioLanguage"))
        .filter_map(|i| Some((i.guid, i.value.get("Name")?.str()?.to_owned())))
        .collect())
}

/// Decodes one variation: its segments back to back.
pub fn decode_variation(alpha: &Alpha, v: &Variation) -> Result<Pcm, Error> {
    let chunk = alpha.chunk(&v.chunk)?;
    let mut out: Option<Pcm> = None;
    for &offset in &v.segments {
        let pcm = audio::decode_sps(chunk.get(offset..).ok_or(Error::Truncated)?)?;
        match &mut out {
            None => out = Some(pcm),
            Some(o) if (o.sample_rate, o.channels) == (pcm.sample_rate, pcm.channels) => o.samples.extend(pcm.samples),
            Some(_) => return Err(Error::Bad("segments differ in format")),
        }
    }
    out.ok_or(Error::Bad("variation without segments"))
}

/// A `MovieTextureAsset`'s video: an EA VP6 stream (`MVhd`), which ffmpeg reads directly.
pub fn movie(alpha: &Alpha, p: &Partition) -> Result<Vec<u8>, Error> {
    let prim = p.primary().ok_or(Error::Bad("movie without a primary instance"))?;
    let id = prim.get("ChunkGuid").and_then(Value::guid).ok_or(Error::Bad("movie without a chunk"))?;
    let bytes = alpha.chunk(&id)?;
    if !bytes.starts_with(b"MVhd") {
        return Err(Error::Unsupported("movie chunk is not EA VP6".into()));
    }
    Ok(bytes)
}
