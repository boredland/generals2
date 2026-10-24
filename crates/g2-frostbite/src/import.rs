//! Decodes an alpha install into a local cache the client reads:
//!
//! ```text
//! <out>/<asset path>[ <variation>].wav          every sound wave (asset paths start sound/), English
//! <out>/movies/<asset path>.vp6, .alpha.vp6, .wav   video, alpha matte, decoded audio
//! <out>/models/<asset path>.glb                 every mesh, diffuse map embedded
//! <out>/models/bounds.txt                       each model's bounding box (name, min xyz, max xyz)
//! <out>/textures/<asset path>.png               every flat texture
//! <out>/manifest.txt                            counts and every failure, one per line
//! ```
//!
//! Asset paths are the alpha's own (lowercase, `/`-separated). Nothing written here may be
//! redistributed: it is EA's content, decoded from the player's own copy.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::ebx::Value;
use crate::{Alpha, Error, material, media, mesh, movie, res_type, texture};

/// Per-kind counts and failures.
#[derive(Default)]
pub struct Report {
    pub written: BTreeMap<&'static str, usize>,
    pub failed: Vec<(String, String)>,
}

impl Report {
    fn ok(&mut self, kind: &'static str) {
        *self.written.entry(kind).or_default() += 1;
    }

    fn fail(&mut self, what: &str, e: impl std::fmt::Display) {
        self.failed.push((what.to_owned(), e.to_string()));
    }

    pub fn summary(&self) -> String {
        let mut s = String::new();
        for (k, n) in &self.written {
            let _ = writeln!(s, "{k}: {n}");
        }
        let _ = writeln!(s, "failed: {}", self.failed.len());
        for (what, why) in &self.failed {
            let _ = writeln!(s, "  {what}: {why}");
        }
        s
    }
}

/// Which parts to import; all by default.
#[derive(Clone, Copy)]
pub struct Parts {
    pub sounds: bool,
    pub movies: bool,
    pub textures: bool,
    pub models: bool,
}

impl Default for Parts {
    fn default() -> Self {
        Parts { sounds: true, movies: true, textures: true, models: true }
    }
}

pub fn import(alpha: &Alpha, out: &Path, parts: Parts, progress: &mut dyn FnMut(&str)) -> Result<Report, Error> {
    let mut report = Report::default();
    if parts.sounds {
        progress("sounds");
        sounds(alpha, out, &mut report)?;
    }
    if parts.movies {
        progress("movies");
        movies(alpha, out, &mut report)?;
    }
    if parts.textures {
        progress("textures");
        for (name, e) in alpha.res_of(res_type::TEXTURE) {
            match texture::decode(alpha, e)
                .and_then(|img| write(&out.join("textures").join(format!("{name}.png")), &png(&img)))
            {
                Ok(()) => report.ok("textures"),
                Err(err) => report.fail(name, err),
            }
        }
    }
    if parts.models {
        progress("models");
        models(alpha, out, &mut report)?;
    }
    write(&out.join("manifest.txt"), report.summary().as_bytes())?;
    Ok(report)
}

fn sounds(alpha: &Alpha, out: &Path, report: &mut Report) -> Result<(), Error> {
    let languages = media::languages(alpha)?;
    for name in alpha.ebx.keys().filter(|n| n.starts_with("sound/") && n.contains("/waves/")) {
        let p = alpha.asset(name)?;
        if p.primary().and_then(Value::ty) != Some("SoundWaveAsset") {
            continue;
        }
        let wave = match media::SoundWave::read(&p, &languages) {
            Ok(w) => w,
            Err(e) => {
                report.fail(name, e);
                continue;
            }
        };
        let many = wave.variations.len() > 1;
        for (i, v) in wave.variations.iter().enumerate() {
            let file = if many { format!("{name} {i}.wav") } else { format!("{name}.wav") };
            match media::decode_variation(alpha, v).and_then(|pcm| write(&out.join(file), &pcm.to_wav())) {
                Ok(()) => report.ok("sounds"),
                Err(e) => report.fail(&format!("{name} {i}"), e),
            }
        }
    }
    Ok(())
}

fn movies(alpha: &Alpha, out: &Path, report: &mut Report) -> Result<(), Error> {
    for name in alpha.ebx.keys() {
        let p = alpha.asset(name)?;
        if p.primary().and_then(Value::ty) != Some("MovieTextureAsset") {
            continue;
        }
        let base = out.join("movies").join(name);
        let result = media::movie(alpha, &p).and_then(|bytes| movie::split(&bytes)).and_then(|m| {
            write(&base.with_extension("vp6"), &m.video)?;
            if let Some(a) = &m.alpha {
                write(&base.with_extension("alpha.vp6"), a)?;
            }
            match m.audio {
                Some(Ok(pcm)) => write(&base.with_extension("wav"), &pcm.to_wav())?,
                Some(Err(e)) => report.fail(&format!("{name} audio"), e),
                None => {}
            }
            Ok(())
        });
        match result {
            Ok(()) => report.ok("movies"),
            Err(e) => report.fail(name, e),
        }
    }
    Ok(())
}

fn models(alpha: &Alpha, out: &Path, report: &mut Report) -> Result<(), Error> {
    let maps = material::diffuse_maps(alpha)?;
    // Mesh assets name their MeshSet res `<asset path>`; a shared texture is decoded once.
    let mut pngs: BTreeMap<String, Option<Vec<u8>>> = BTreeMap::new();
    // `name min.x min.y min.z max.x max.y max.z` per model, so the client sizes a model to
    // its footprint without parsing it.
    let mut bounds = String::new();
    for (name, e) in alpha.res_of(res_type::MESH_SET) {
        let result = mesh::read(alpha, e).map(|m| {
            let textures: Vec<Option<Vec<u8>>> = maps
                .get(name)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .map(|t| {
                    let t = t.as_ref()?;
                    pngs.entry(t.clone())
                        .or_insert_with(|| {
                            let e = alpha.res.get(t)?;
                            texture::decode(alpha, e).ok().map(|img| png(&img))
                        })
                        .clone()
                })
                .collect();
            // In the glTF's space: X negated, as `to_glb` writes it.
            let (lo, hi) = m.bounds;
            let _ = writeln!(bounds, "{name} {} {} {} {} {} {}", -hi[0], lo[1], lo[2], -lo[0], hi[1], hi[2]);
            mesh::to_glb(&m, &textures)
        });
        match result.and_then(|glb| write(&out.join("models").join(format!("{name}.glb")), &glb)) {
            Ok(()) => report.ok("models"),
            Err(err) => report.fail(name, err),
        }
    }
    write(&out.join("models/bounds.txt"), bounds.as_bytes())
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let io = |e| Error::Io(path.to_path_buf(), e);
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).map_err(io)?;
    std::fs::write(path, bytes).map_err(io)
}

/// An uncompressed-deflate RGBA PNG: stored zlib blocks keep this dependency-free and
/// fast; the cache trades disk for import time.
fn png(img: &texture::Image) -> Vec<u8> {
    let mut raw = Vec::with_capacity((img.width as usize * 4 + 1) * img.height as usize);
    for row in img.rgba.chunks_exact(img.width as usize * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let idat = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 1);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = img.width.to_be_bytes().to_vec();
    ihdr.extend(img.height.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    for (kind, data) in [(b"IHDR", ihdr.as_slice()), (b"IDAT", &idat), (b"IEND", &[])] {
        out.extend((data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend(&body);
        out.extend(crc32(&body).to_be_bytes());
    }
    out
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// Where the client looks for the cache: `$G2_ALPHA_CACHE`, else
/// `$XDG_DATA_HOME/generals2/alpha`, else `~/.local/share/generals2/alpha`.
pub fn default_cache() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("G2_ALPHA_CACHE") {
        return Some(PathBuf::from(p));
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(data.join("generals2/alpha"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_the_png_reference_value() {
        // CRC-32 of "IEND" as every PNG's last chunk carries it.
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
    }

    #[test]
    fn png_has_signature_header_and_round_trips_through_zlib() {
        let img = texture::Image { width: 2, height: 1, rgba: vec![1, 2, 3, 4, 5, 6, 7, 8] };
        let p = png(&img);
        assert!(p.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(&p[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes(p[16..20].try_into().unwrap()), 2);
        let idat_len = u32::from_be_bytes(p[33..37].try_into().unwrap()) as usize;
        assert_eq!(&p[37..41], b"IDAT");
        let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&p[41..41 + idat_len]).unwrap();
        assert_eq!(raw, [0, 1, 2, 3, 4, 5, 6, 7, 8]);
    }
}
