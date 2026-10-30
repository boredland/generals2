//! Checks against a real alpha install. Point `G2_ALPHA_DIR` at a copy you own (the game
//! folder or its `Data/`) to run these; they are skipped when it is unset, so CI and anyone
//! without the alpha never read EA data. The exact counts are those of the October 2013
//! closed-alpha client ("Generals2 F2P"); another build will fail them on purpose.

use std::path::PathBuf;

use g2_frostbite::ebx::Value;
use g2_frostbite::{Alpha, material, media, mesh, movie, res_type, texture};

/// A relative `G2_ALPHA_DIR` is taken from the repository root: cargo runs tests in the
/// crate's folder, where the same value would point nowhere.
fn install() -> Option<Alpha> {
    let dir = PathBuf::from(std::env::var_os("G2_ALPHA_DIR")?);
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(dir);
    Some(Alpha::open(&dir).expect("G2_ALPHA_DIR is not a readable alpha install"))
}

#[test]
fn index_finds_every_superbundle_bundle_and_entry() {
    let Some(a) = install() else { return };
    assert_eq!((a.superbundles, a.bundles), (22, 277));
    assert_eq!((a.ebx.len(), a.res.len(), a.chunk_count()), (14_768, 5_490, 10_872));
}

/// Every ebx and res payload decompresses to exactly the size its bundle records.
#[test]
fn every_entry_decompresses_to_its_recorded_size() {
    let Some(a) = install() else { return };
    for (name, e) in a.ebx.iter().chain(&a.res) {
        let bytes = a.entry_bytes(e).unwrap_or_else(|err| panic!("{name}: {err}"));
        assert_eq!(bytes.len() as u64, e.original_size, "{name}");
    }
}

/// Nothing marks a chunk as compressed or raw, so `Alpha::chunk` guesses from the bytes.
/// Every chunk the guess leaves raw must start with a known raw payload: EA SNR audio
/// (`48 00 00 0C`), the ambience stream layout (`01 10 00 00`) or EA VP6 video (`MVhd`).
#[test]
fn every_chunk_is_either_block_compressed_or_known_raw_media() {
    let Some(a) = install() else { return };
    let (mut unpacked, mut audio, mut ambience, mut video) = (0, 0, 0, 0);
    for id in a.chunk_ids() {
        let raw = a.chunk_raw(id).unwrap_or_else(|err| panic!("{}: {err}", g2_frostbite::hex(id)));
        if g2_frostbite::unblock(&raw).is_some() {
            unpacked += 1;
            continue;
        }
        match raw.get(..4) {
            Some([0x48, 0x00, 0x00, 0x0c]) => audio += 1,
            Some([0x01, 0x10, 0x00, 0x00]) => ambience += 1,
            Some(b"MVhd") => video += 1,
            other => panic!("{}: unrecognised raw chunk {other:02x?}", g2_frostbite::hex(id)),
        }
    }
    assert_eq!((unpacked, audio, ambience, video), (3_805, 7_049, 16, 2));
}

/// Every texture's pixel chunk resolves, and every flat 2D texture decodes: no format code
/// in the install is left unmapped. Cube and volume maps (sky and post-process lookups)
/// are rejected by design.
#[test]
fn every_flat_texture_decodes() {
    let Some(a) = install() else { return };
    let (mut flat, mut other) = (0, 0);
    for (name, e) in a.res_of(res_type::TEXTURE) {
        let h = texture::Header::parse(&a.entry_bytes(e).unwrap()).unwrap();
        assert!(a.has_chunk(&h.chunk), "{name}: pixel chunk missing");
        if h.kind != 0 || h.slices > 1 || h.depth > 1 {
            assert!(texture::decode(&a, e).is_err(), "{name}");
            other += 1;
            continue;
        }
        let img = texture::decode(&a, e).unwrap_or_else(|err| panic!("{name}: {err}"));
        assert_eq!(img.rgba.len(), (h.width * h.height * 4) as usize, "{name}");
        flat += 1;
    }
    assert_eq!((flat, other), (1_329, 10));
}

/// Every ebx partition parses (its descriptor tables end exactly at the string section),
/// and the asset kinds the importer reads are all there.
#[test]
fn every_ebx_parses() {
    let Some(a) = install() else { return };
    let mut kinds = std::collections::BTreeMap::<String, usize>::new();
    for name in a.ebx.keys() {
        let p = a.asset(name).unwrap_or_else(|e| panic!("{name}: {e}"));
        let ty = p.primary().and_then(Value::ty).unwrap_or("?").to_owned();
        *kinds.entry(ty).or_default() += 1;
    }
    assert_eq!(kinds["SoundWaveAsset"], 2_101);
    assert_eq!(kinds["MovieTextureAsset"], 2);
    assert_eq!(kinds["TextureAsset"], 1_339);
    assert_eq!(kinds["MeshVariationDatabase"], 176);
}

/// Every sound wave has English variations and each decodes; every segment is one SPS
/// stream. 9,110 variations is the count the import writes.
#[test]
fn every_sound_variation_decodes() {
    let Some(a) = install() else { return };
    let languages = media::languages(&a).unwrap();
    let (mut variations, mut looping) = (0, 0);
    for name in a.ebx.keys().filter(|n| n.starts_with("sound/") && n.contains("/waves/")) {
        let p = a.asset(name).unwrap();
        if p.primary().and_then(Value::ty) != Some("SoundWaveAsset") {
            continue;
        }
        let wave = media::SoundWave::read(&p, &languages).unwrap_or_else(|e| panic!("{name}: {e}"));
        for v in &wave.variations {
            let (pcm, looped) = media::decode_variation(&a, v).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(pcm.frames() > 0 && [44_100, 48_000].contains(&pcm.sample_rate), "{name}");
            if let Some(l) = &looped {
                assert!(l.start < l.end && l.end <= pcm.frames(), "{name}: loop {l:?} of {}", pcm.frames());
                looping += 1;
            }
            variations += 1;
        }
    }
    assert_eq!(variations, 9_110);
    // A part loops in music (intro, loop), moving units' sounds (start, loop, end) and
    // weapon-fire loops (loop, tail).
    assert_eq!(looping, 127);
}

/// Both movies split into a VP6 stream; the logo movie's EA-XA audio decodes to its
/// 10.67 s, and the portrait's alpha matte and EALayer3 audio are reported, not decoded.
#[test]
fn movies_split_into_video_matte_and_audio() {
    let Some(a) = install() else { return };
    let split = |name: &str| movie::split(&media::movie(&a, &a.asset(name).unwrap()).unwrap()).unwrap();
    let logos = split("_rts/art/cinematics/mov/cnc_frontend/cnclive_frontend_logos_avi");
    let audio = logos.audio.unwrap().unwrap();
    assert_eq!((audio.channels, audio.sample_rate, audio.frames()), (2, 48_000, 512_000));
    assert!(logos.alpha.is_none());
    let portrait = split("_rts/art/ui/test/gg_basic_01_intro_01_alpha");
    assert!(portrait.alpha.is_some_and(|m| m.starts_with(b"MVhd")));
    assert!(matches!(portrait.audio, Some(Err(g2_frostbite::Error::Unsupported(_)))));
}

/// Every mesh decodes: positions inside its bounding box, indices inside its vertices. Most
/// materials have a diffuse map bound through the mesh variation databases.
#[test]
fn every_mesh_decodes_inside_its_bounds() {
    let Some(a) = install() else { return };
    let (mut meshes, mut triangles) = (0, 0);
    for (name, e) in a.res_of(res_type::MESH_SET) {
        let m = mesh::read(&a, e).unwrap_or_else(|err| panic!("{name}: {err}"));
        let (lo, hi) = m.bounds;
        for s in &m.subsets {
            for p in &s.positions {
                assert!((0..3).all(|i| p[i] >= lo[i] - 0.05 && p[i] <= hi[i] + 0.05), "{name}: vertex outside bounds");
            }
            triangles += s.indices.len() / 3;
        }
        meshes += 1;
    }
    assert_eq!(meshes, 2_154);
    assert!(triangles > 4_000_000, "{triangles} triangles");
    let maps = material::diffuse_maps(&a, &mut |name, e| panic!("{name}: {e}"));
    let bound = maps.values().flatten().flatten().filter(|t| a.res.contains_key(*t)).count();
    assert!(bound > 2_500, "{bound} materials with a diffuse map");
}

/// A texture the models need but the install lacks (its res taken out of the index here)
/// leaves those models untextured, and each gets one manifest line naming the texture;
/// the rest of the import goes on.
#[test]
fn a_missing_texture_is_reported_for_each_model_that_uses_it() {
    use g2_frostbite::import::{Parts, import};
    let Some(mut a) = install() else { return };
    let maps = material::diffuse_maps(&a, &mut |name, e| panic!("{name}: {e}"));
    let mut users = std::collections::BTreeMap::<&str, Vec<&str>>::new();
    for (model, slots) in &maps {
        for t in slots.iter().flatten().collect::<std::collections::BTreeSet<_>>() {
            if a.res_of(res_type::MESH_SET).any(|(n, _)| n == model) {
                users.entry(t.as_str()).or_default().push(model.as_str());
            }
        }
    }
    let (texture, models) = users.iter().max_by_key(|(_, m)| m.len()).map(|(t, m)| (t.to_string(), m.clone())).unwrap();
    let models: Vec<String> = models.into_iter().map(str::to_owned).collect();
    assert!(models.len() > 1, "a shared texture to remove");
    a.res.remove(&texture);
    let out = std::env::temp_dir().join(format!("g2-missing-texture-{}", std::process::id()));
    let parts = Parts { sounds: false, movies: false, textures: false, models: true, ui: false };
    let report = import(&a, &out, parts, &mut |_| {}).expect("the import finishes");
    let mut lines: Vec<&str> = report.failed.iter().map(|(what, _)| what.as_str()).collect();
    lines.sort_unstable();
    let mut want: Vec<String> = models.iter().map(|m| format!("{m} texture {texture}")).collect();
    want.sort_unstable();
    assert_eq!(lines, want);
    assert_eq!(report.written.get("models"), Some(&2_154), "every model is still written");
    let _ = std::fs::remove_dir_all(&out);
}

/// The animation package: every one of the 1,700 clips (1,683 VBR, 17 raw) decodes onto
/// its controller's DOFs, every channel on a DOF of its own kind (`Package::clip` refuses a
/// mismatch), and every key time lies inside the clip. Animated quaternions decode to unit
/// length before normalising: 99% within 0.5% over every key of every clip (a wrong bit
/// order, dequantisation or block layout gives lengths all over the place), and every block
/// decodes inside its bytes. The run cycle of the infantry rig moves its feet: the left
/// ankle's rotation sweeps more than 30 degrees over the cycle and returns near its start.
#[test]
fn every_animation_clip_decodes_onto_its_joints() {
    use g2_frostbite::ant::{Package, Path};
    let Some(a) = install() else { return };
    let p = Package::read(&a).unwrap();
    let count = |ty: &str| p.objects.values().filter(|o| o.ty == ty).count();
    assert_eq!(
        (count("VbrAnimationAsset"), count("RawAnimationAsset"), count("ClipControllerAsset")),
        (1_683, 17, 1_700)
    );
    let mut errors = Vec::new();
    for (g, o) in p.objects.iter().filter(|(_, o)| o.ty == "ClipControllerAsset") {
        let c = p.clip(*g).unwrap_or_else(|e| panic!("{}: {e}", o.name));
        assert!(
            c.times.windows(2).all(|w| w[0] < w[1]) && c.times.last().is_some_and(|&t| t <= c.frames),
            "{}",
            c.name
        );
        for t in &c.tracks {
            assert!(t.keys.len() == 1 || t.keys.len() == c.times.len(), "{} {}: key count", c.name, t.joint);
        }
        errors.extend(c.unit_error);
    }
    errors.sort_by(f32::total_cmp);
    let at = |f: f64| errors[(errors.len() as f64 * f) as usize];
    assert!(errors.len() > 3_000_000, "{} animated quaternion keys", errors.len());
    assert!(
        at(0.5) < 0.001 && at(0.99) < 0.005 && at(1.0 - 1e-9) < 0.02,
        "|q| - 1: median {} p99 {}",
        at(0.5),
        at(0.99)
    );
    let rigs = p.rigs();
    let infantry = rigs.iter().find(|r| r.name == "MASTER_Infantry_SKL").unwrap();
    assert_eq!((infantry.joints.len(), infantry.clips.len()), (50, 240));
    let run = p.objects.iter().find(|(_, o)| o.name == "RifleShared_RunFwd").unwrap().0;
    let c = p.clip(*run).unwrap();
    assert_eq!((c.fps, c.frames, c.times.len()), (30.0, 17, 9));
    assert!((c.travel - 4.15).abs() < 0.05, "a stride of 4.15 units: {}", c.travel);
    let foot = c.tracks.iter().find(|t| t.joint == "LeftFoot" && t.path == Path::Rotation).unwrap();
    let angle = |a: [f32; 4], b: [f32; 4]| {
        2.0 * (a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>().abs().min(1.0)).acos().to_degrees()
    };
    let sweep = foot.keys.iter().map(|&k| angle(foot.keys[0], k)).fold(0.0, f32::max);
    assert!(sweep > 30.0, "the foot swings {sweep} degrees");
    assert!(angle(foot.keys[0], *foot.keys.last().unwrap()) < 10.0, "and the cycle closes");
}
