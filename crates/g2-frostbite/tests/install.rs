//! Checks against a real alpha install. Point `G2_ALPHA_DIR` at a copy you own (the game
//! folder or its `Data/`) to run these; they are skipped when it is unset, so CI and anyone
//! without the alpha never read EA data.

use std::path::PathBuf;

use g2_frostbite::{Alpha, res_type, texture};

fn install() -> Option<Alpha> {
    let dir = PathBuf::from(std::env::var_os("G2_ALPHA_DIR")?);
    Some(Alpha::open(&dir).expect("G2_ALPHA_DIR is not a readable alpha install"))
}

/// Every resource and every chunk a texture names resolves, and every flat 2D texture
/// decodes: no format code in the install is left unmapped.
#[test]
fn every_flat_texture_decodes() {
    let Some(a) = install() else { return };
    let (mut flat, mut other) = (0, 0);
    for (name, e) in a.res_of(res_type::TEXTURE) {
        let h = texture::Header::parse(&a.entry_bytes(e).unwrap()).unwrap();
        assert!(a.has_chunk(&h.chunk), "{name}: pixel chunk missing");
        if h.kind != 0 || h.slices > 1 || h.depth > 1 {
            other += 1;
            continue;
        }
        let img = texture::decode(&a, e).unwrap_or_else(|err| panic!("{name}: {err}"));
        assert_eq!(img.rgba.len(), (h.width * h.height * 4) as usize, "{name}");
        flat += 1;
    }
    assert!(flat > 1000, "only {flat} flat textures");
    println!("{flat} flat textures decoded, {other} cube/volume maps skipped");
}

/// Ebx and res payloads decompress to exactly the size their bundle records.
#[test]
fn entries_decompress_to_their_recorded_size() {
    let Some(a) = install() else { return };
    for (name, e) in a.ebx.iter().step_by(97).chain(a.res.iter().step_by(13)) {
        let bytes = a.entry_bytes(e).unwrap_or_else(|err| panic!("{name}: {err}"));
        assert_eq!(bytes.len() as u64, e.original_size, "{name}");
    }
}
