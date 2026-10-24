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
    let mut variations = 0;
    for name in a.ebx.keys().filter(|n| n.starts_with("sound/") && n.contains("/waves/")) {
        let p = a.asset(name).unwrap();
        if p.primary().and_then(Value::ty) != Some("SoundWaveAsset") {
            continue;
        }
        let wave = media::SoundWave::read(&p, &languages).unwrap_or_else(|e| panic!("{name}: {e}"));
        for v in &wave.variations {
            let pcm = media::decode_variation(&a, v).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(pcm.frames() > 0 && [44_100, 48_000].contains(&pcm.sample_rate), "{name}");
            variations += 1;
        }
    }
    assert_eq!(variations, 9_110);
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
    let maps = material::diffuse_maps(&a).unwrap();
    let bound = maps.values().flatten().flatten().filter(|t| a.res.contains_key(*t)).count();
    assert!(bound > 2_500, "{bound} materials with a diffuse map");
}
