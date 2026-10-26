//! What the menus draw beyond plain UI: the imported alpha's general portraits when there is
//! an import, and a preview image of each skirmish map.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use g2_sim::{Faction, GeneralId};

/// The alpha general each of ours stands for, by the archetype the rules table names for it
/// (`g2-rules`: "the alpha's EU_AirforceGeneral" and so on). The alpha has no flame general's
/// art: Inferno shows the APA tank general's.
fn alpha_general(name: &str) -> Option<&'static str> {
    Some(match name {
        "Bulldog" => "EG_Basic_01",
        "Taskmaster" => "AG_Basic_01",
        "Junkyard" => "GG_Salvage_01",
        "Daedalus" => "EG_Airforce_01",
        "Mercury Red" => "EG_RapidAssault_01",
        "Cutting Edge" => "EG_ExperimentalWeapons_01",
        "Cleaver" => "EG_Railgun_01",
        "Inferno" => "AG_Tank_01",
        "Red Arrow" => "AG_Rocket_01",
        "1000 Suns" => "AG_Atomic_01",
        "Blackjack" => "AG_Espionage_01",
        "Thrax" => "GG_Toxin_01",
        "Wild Dog" => "GG_Marauder_01",
        "Kitbash" => "GG_Engineer_01",
        _ => return None,
    })
}

/// The faction's "basic" general, shown when a slot has no general.
fn basic(faction: Faction) -> &'static str {
    match faction {
        Faction::Eu => "EG_Basic_01",
        Faction::Apa => "AG_Basic_01",
        Faction::Gla => "GG_Basic_01",
    }
}

fn general_key(faction: Faction, general: Option<GeneralId>) -> &'static str {
    general.and_then(|g| alpha_general(g2_rules::rules().general(g).name)).unwrap_or_else(|| basic(faction))
}

/// The general's half-length portrait (`UserPortraits`), for the large tiles.
pub fn portrait(faction: Faction, general: Option<GeneralId>) -> String {
    let key = match general_key(faction, general) {
        // The alpha's file names have these typos.
        "EG_RapidAssault_01" => "EG_RapidAssult_01",
        k => k,
    };
    format!("UserPortraits/{key}_UP")
}

/// A preview of skirmish map `name`: the map's own preview (`<name>.tga` beside an official
/// map, the picture the original game showed) or, for a map without one, a top-down sketch of
/// its terrain, resources and starts. `None` when the map can't be read.
pub fn map_preview(name: &str) -> Option<Image> {
    if let Some((_, folder)) = name.split_once(':')
        && let Some(entry) = g2_maps::catalogue(&g2_maps::vendored_root()).into_iter().find(|e| e.name == name)
    {
        let tga = entry.path.with_file_name(format!("{folder}.tga"));
        if let Some(image) = std::fs::read(&tga).ok().and_then(|b| decode_tga(&b)) {
            return Some(image);
        }
    }
    let map = g2_maps::resolve(name).ok()?;
    Some(sketch(&map))
}

/// Uncompressed or run-length 24/32-bit true-colour TGA (what the official map previews are),
/// as an sRGB image, top row first. Anything else is `None`.
fn decode_tga(b: &[u8]) -> Option<Image> {
    let (id_len, cmap, kind) = (*b.first()? as usize, *b.get(1)?, *b.get(2)?);
    let (w, h) =
        (u16::from_le_bytes([*b.get(12)?, *b.get(13)?]) as u32, u16::from_le_bytes([*b.get(14)?, *b.get(15)?]) as u32);
    let (bpp, desc) = (*b.get(16)? as usize, *b.get(17)?);
    if cmap != 0 || !matches!(kind, 2 | 10) || !matches!(bpp, 24 | 32) || w == 0 || h == 0 {
        return None;
    }
    let px = bpp / 8;
    let mut src = b.get(18 + id_len..)?;
    let n = (w * h) as usize;
    let mut pixels: Vec<[u8; 4]> = Vec::with_capacity(n);
    let read = |s: &[u8]| [s[2], s[1], s[0], if px == 4 { s[3] } else { 255 }];
    while pixels.len() < n {
        if kind == 2 {
            pixels.push(read(src.get(..px)?));
            src = &src[px..];
        } else {
            let head = *src.first()?;
            let count = (head & 0x7f) as usize + 1;
            src = &src[1..];
            if head & 0x80 != 0 {
                let p = read(src.get(..px)?);
                src = &src[px..];
                pixels.extend(std::iter::repeat_n(p, count));
            } else {
                for _ in 0..count {
                    pixels.push(read(src.get(..px)?));
                    src = &src[px..];
                }
            }
        }
    }
    pixels.truncate(n);
    // Bottom-up unless the descriptor's top-left bit says otherwise.
    let top_first = desc & 0x20 != 0;
    let mut data = Vec::with_capacity(n * 4);
    for y in 0..h {
        let row = if top_first { y } else { h - 1 - y };
        for x in 0..w {
            data.extend_from_slice(&pixels[(row * w + x) as usize]);
        }
    }
    Some(finish(w, h, data))
}

/// A top-down sketch of `map` for maps without a preview of their own: dry ground with
/// impassable cells dark, supply piles light, oil wells black, and each start a ring.
fn sketch(map: &g2_sim::MapDef) -> Image {
    const N: u32 = 128;
    let (mw, mh) = (map.width.to_f32(), map.height.to_f32());
    let (gw, gh) = g2_sim::nav::Grid::dims(map.width, map.height);
    let mut data = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let (wx, wy) = ((x as f32 + 0.5) / N as f32 * mw, (y as f32 + 0.5) / N as f32 * mh);
            let n = crate::terrain::fbm(wx * 0.02, wy * 0.02, 3, 7);
            let mut c = [0.53 + n * 0.1, 0.42 + n * 0.08, 0.26 + n * 0.05];
            let cell = (wy as usize / g2_sim::nav::CELL as usize).min(gh - 1) * gw
                + (wx as usize / g2_sim::nav::CELL as usize).min(gw - 1);
            if map.terrain.get(cell).copied().unwrap_or(false) {
                c = [0.22, 0.19, 0.15];
            }
            let near = |p: g2_sim::Vec2, r: f32| (p.x.to_f32() - wx).hypot(p.y.to_f32() - wy) < r;
            if map.supply_piles.iter().any(|&(p, _)| near(p, 6.0)) {
                c = [0.85, 0.82, 0.70];
            }
            if map.oil_wells.iter().any(|&(p, _)| near(p, 5.0)) {
                c = [0.05, 0.05, 0.05];
            }
            if map.spawns.iter().any(|&p| near(p, 12.0) && !near(p, 9.0)) {
                c = [0.95, 0.95, 0.95];
            }
            data.extend_from_slice(&[(c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8, 255]);
        }
    }
    finish(N, N, data)
}

fn finish(w: u32, h: u32, data: Vec<u8>) -> Image {
    let mut image = Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::linear();
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every general maps to a portrait the alpha ships (checked against the archived install
    /// when it is present; skipped otherwise).
    #[test]
    fn every_general_has_an_alpha_portrait() {
        let Some(dir) = std::env::var_os("G2_ALPHA_DIR") else { return };
        let images = std::path::Path::new(&dir).join("UTFWinAssets/images");
        let rules = g2_rules::rules();
        let generals = (0..rules.generals.len()).map(|i| GeneralId(i as u16));
        for (f, g) in generals.map(|g| (rules.general(g).faction, Some(g))).chain(Faction::ALL.map(|f| (f, None))) {
            let rel = portrait(f, g);
            assert!(images.join(format!("{rel}.png")).is_file(), "{f:?} {g:?}: no {rel}.png");
        }
    }

    /// An official map's preview is its own TGA, decoded the right way up; Crater Ridge, which
    /// has none, gets a sketch.
    #[test]
    fn map_previews_load() {
        let tga = map_preview("zh:Tournament Desert").expect("an official preview");
        assert_eq!(tga.texture_descriptor.size.width, 128);
        let sketch = map_preview("Crater Ridge").expect("a sketch");
        assert_eq!(sketch.texture_descriptor.size.width, 128);
    }
}
