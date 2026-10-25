//! Decodes an alpha install into a local cache the client reads:
//!
//! ```text
//! <out>/<asset path>[ <variation>].wav          every sound wave (asset paths start sound/), English
//! <out>/movies/<asset path>.vp6, .alpha.vp6, .wav   video, alpha matte, decoded audio
//! <out>/movies/<asset path>.frames/NNNN.png, fps.txt  the video as frames (needs ffmpeg)
//! <out>/models/<asset path>.glb                 every mesh, pointing at its textures below
//! <out>/models/bounds.txt                       each model's bounding box (name, min xyz, max xyz)
//! <out>/textures/<asset path>.png               every flat texture
//! <out>/ui/                                     the HUD's images, layouts, fonts, as installed
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

    /// Records a failure once: two parts can read the same broken asset (the material scan
    /// reads every ebx), and a model can use one texture in several slots.
    fn fail(&mut self, what: &str, e: impl std::fmt::Display) {
        let entry = (what.to_owned(), e.to_string());
        if !self.failed.contains(&entry) {
            self.failed.push(entry);
        }
    }

    /// An ebx asset, or `None` with the failure recorded: one unreadable asset is a line in
    /// the manifest, not the end of the import.
    fn asset(&mut self, alpha: &Alpha, name: &str) -> Option<crate::ebx::Partition> {
        alpha.asset(name).map_err(|e| self.fail(name, e)).ok()
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
    pub ui: bool,
}

impl Default for Parts {
    fn default() -> Self {
        Parts { sounds: true, movies: true, textures: true, models: true, ui: true }
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
    if parts.ui {
        progress("ui");
        ui(alpha, out, &mut report)?;
    }
    write(&out.join("manifest.txt"), report.summary().as_bytes())?;
    Ok(report)
}

fn sounds(alpha: &Alpha, out: &Path, report: &mut Report) -> Result<(), Error> {
    let languages = media::languages(alpha)?;
    for name in alpha.ebx.keys().filter(|n| n.starts_with("sound/") && n.contains("/waves/")) {
        let Some(p) = report.asset(alpha, name) else { continue };
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
            match media::decode_variation(alpha, v)
                .and_then(|(pcm, looped)| write(&out.join(file), &pcm.to_wav(looped)))
            {
                Ok(()) => report.ok("sounds"),
                Err(e) => report.fail(&format!("{name} {i}"), e),
            }
        }
    }
    Ok(())
}

/// The HUD's art and layouts: `UTFWinAssets/` (images, layouts, fonts, text styles) is
/// loose next to `Data/`, and is copied as it is to `<out>/ui/`.
fn ui(alpha: &Alpha, out: &Path, report: &mut Report) -> Result<(), Error> {
    let src = alpha.root().join("UTFWinAssets");
    if !src.is_dir() {
        report.fail("UTFWinAssets", "not found next to Data/");
        return Ok(());
    }
    let dst = out.join("ui");
    let mut stack = vec![src.clone()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).map_err(|e| Error::Io(dir.clone(), e))? {
            let path = e.map_err(|e| Error::Io(dir.clone(), e))?.path();
            let ext = path.extension().and_then(|x| x.to_str()).map(str::to_ascii_lowercase);
            if path.is_dir() {
                stack.push(path);
            } else if matches!(ext.as_deref(), Some("png" | "layout" | "ttf" | "otf" | "css")) {
                let rel = path.strip_prefix(&src).unwrap();
                match std::fs::read(&path)
                    .map_err(|e| Error::Io(path.clone(), e))
                    .and_then(|b| write(&dst.join(rel), &b))
                {
                    Ok(()) => report.ok("ui files"),
                    Err(err) => report.fail(&rel.to_string_lossy(), err),
                }
            }
        }
    }
    Ok(())
}

fn movies(alpha: &Alpha, out: &Path, report: &mut Report) -> Result<(), Error> {
    for name in alpha.ebx.keys() {
        let Some(p) = report.asset(alpha, name) else { continue };
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
                Some(Ok(pcm)) => write(&base.with_extension("wav"), &pcm.to_wav(None))?,
                Some(Err(e)) => report.fail(&format!("{name} audio"), e),
                None => {}
            }
            Ok(())
        });
        match result {
            Ok(()) => {
                report.ok("movies");
                match movie_frames(&base.with_extension("vp6"), &base.with_extension("frames")) {
                    Ok(()) => report.ok("movie frame sets"),
                    Err(e) => report.fail(&format!("{name} frames"), e),
                }
            }
            Err(e) => report.fail(name, e),
        }
    }
    Ok(())
}

/// Decodes a VP6 stream to `dir/NNNN.png` (from 0001) and `dir/fps.txt` with ffmpeg, which
/// reads EA VP6; the game shows the frames in turn. Without ffmpeg on the `PATH` this fails
/// and the movie stays undecoded.
fn movie_frames(vp6: &Path, dir: &Path) -> Result<(), Error> {
    let io = |e| Error::Io(dir.to_path_buf(), e);
    if dir.is_dir() {
        std::fs::remove_dir_all(dir).map_err(io)?;
    }
    std::fs::create_dir_all(dir).map_err(io)?;
    let run = |args: &[&std::ffi::OsStr], program: &str| -> Result<String, Error> {
        let out = std::process::Command::new(program)
            .args(args)
            .output()
            .map_err(|e| Error::Unsupported(format!("{program} not found on PATH ({e}); movie frames skipped")))?;
        if !out.status.success() {
            return Err(Error::Unsupported(format!(
                "{program} failed: {}",
                String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("")
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let rate = run(
        &["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=r_frame_rate", "-of", "csv=p=0"]
            .map(std::ffi::OsStr::new)
            .iter()
            .copied()
            .chain([vp6.as_os_str()])
            .collect::<Vec<_>>(),
        "ffprobe",
    )?;
    let fps = rate
        .trim()
        .split_once('/')
        .and_then(|(n, d)| Some(n.parse::<f64>().ok()? / d.parse::<f64>().ok()?))
        .filter(|f| f.is_finite() && *f > 0.0)
        .ok_or(Error::Bad("movie frame rate unreadable"))?;
    let pattern = dir.join("%04d.png");
    run(
        &[std::ffi::OsStr::new("-v"), "error".as_ref(), "-i".as_ref(), vp6.as_os_str(), pattern.as_os_str()],
        "ffmpeg",
    )?;
    write(&dir.join("fps.txt"), format!("{fps}\n").as_bytes())
}

fn models(alpha: &Alpha, out: &Path, report: &mut Report) -> Result<(), Error> {
    let maps = material::diffuse_maps(alpha, &mut |name, e| report.fail(name, e));
    // Mesh assets name their MeshSet res `<asset path>`. A texture is written once, to
    // `textures/<name>.png` (the flat texture export writes the same file), and every model
    // that uses it points there. One that can't be written leaves its materials untextured
    // and is a manifest line naming the model, so nothing goes missing silently.
    let mut written: BTreeMap<String, Result<(), String>> = BTreeMap::new();
    // `name min.x min.y min.z max.x max.y max.z` per model, so the client sizes a model to
    // its footprint without parsing it.
    let mut bounds = String::new();
    for (name, e) in alpha.res_of(res_type::MESH_SET) {
        let result = mesh::read(alpha, e).map(|m| {
            let textures: Vec<Option<String>> = maps
                .get(name)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .map(|t| {
                    let t = t.as_ref()?;
                    let state = written.entry(t.clone()).or_insert_with(|| {
                        let path = out.join("textures").join(format!("{t}.png"));
                        if path.is_file() {
                            return Ok(());
                        }
                        let e = alpha.res.get(t).ok_or("no such texture res")?;
                        texture::decode(alpha, e).and_then(|img| write(&path, &png(&img))).map_err(|e| e.to_string())
                    });
                    match state {
                        Ok(()) => Some(texture_uri(name, t)),
                        Err(why) => {
                            report.fail(&format!("{name} texture {t}"), &*why);
                            None
                        }
                    }
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

/// The URI from `models/<model>.glb` to `textures/<texture>.png`: up out of the model's
/// folders, then down; each path segment percent-encoded, as glTF URIs are.
fn texture_uri(model: &str, texture: &str) -> String {
    let up = "../".repeat(model.matches('/').count() + 1);
    let encode = |segment: &str| {
        segment
            .bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
                _ => format!("%{b:02X}"),
            })
            .collect::<String>()
    };
    let down: Vec<String> = texture.split('/').map(encode).collect();
    format!("{up}textures/{}.png", down.join("/"))
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

    /// A model's texture URI climbs out of every folder of the model's path to the cache
    /// root and descends into `textures/`, so it resolves from the `.glb`'s own folder;
    /// characters a URI can't hold are percent-encoded per segment, `/` kept.
    #[test]
    fn texture_uris_resolve_from_the_model_folder() {
        assert_eq!(
            texture_uri("_rts/art/eu/tank", "_rts/art/eu/tank_di"),
            "../../../../textures/_rts/art/eu/tank_di.png"
        );
        assert_eq!(texture_uri("tank", "a b/c%d"), "../textures/a%20b/c%25d.png");
        let model = Path::new("models/_rts/art/eu/tank.glb");
        let resolved = model.parent().unwrap().join(texture_uri("_rts/art/eu/tank", "x/y"));
        let mut parts = Vec::new();
        for c in resolved.components() {
            match c {
                std::path::Component::ParentDir => {
                    parts.pop();
                }
                c => parts.push(c),
            }
        }
        assert_eq!(parts.iter().collect::<PathBuf>(), Path::new("textures/x/y.png"));
    }
}
