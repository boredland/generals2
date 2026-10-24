//! Checks against a real alpha install. Point `G2_ALPHA_DIR` at a copy you own (the game
//! folder or its `Data/`) to run these; they are skipped when it is unset, so CI and anyone
//! without the alpha never read EA data. The exact counts are those of the October 2013
//! closed-alpha client ("Generals2 F2P"); another build will fail them on purpose.

use std::path::PathBuf;

use g2_frostbite::{Alpha, res_type, texture};

fn install() -> Option<Alpha> {
    let dir = PathBuf::from(std::env::var_os("G2_ALPHA_DIR")?);
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
