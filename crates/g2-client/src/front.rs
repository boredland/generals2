//! What the menus draw beyond plain UI: the imported alpha's front-end art (fonts, general
//! portraits, faction logos, loading-screen and victory/defeat art) when there is an import,
//! and a preview image of each skirmish map.
//!
//! Without an import every lookup here is `None` and the menus fall back to plain fills and
//! Bevy's own font.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use g2_sim::{Faction, GeneralId};

/// The alpha's front-end fonts, loaded once when there is an import.
#[derive(Resource, Clone)]
pub struct Fonts {
    /// Eurostile bold: every menu label.
    pub body: Handle<Font>,
    /// Eurostile Extended Black: titles, PLAY and START BATTLE.
    pub wide: Handle<Font>,
}

pub fn load_fonts(mut commands: Commands, cache: Option<Res<crate::alpha::Cache>>, server: Option<Res<AssetServer>>) {
    let (Some(_), Some(server)) = (cache, server) else { return };
    let font = |f: &str| server.load(bevy::asset::AssetPath::from(format!("ui/fonts/{f}")).with_source("alpha"));
    commands.insert_resource(Fonts { body: font("EurostileLTStdBold.otf"), wide: font("EurostileExt-Bla.otf") });
}

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

/// The general's loading-screen art, when the alpha has one for it; else its faction's basic
/// general's.
pub fn loading_art(faction: Faction, general: Option<GeneralId>) -> String {
    let key = general_key(faction, general);
    let known = [
        "AGEspionage01",
        "AGRocket01",
        "AGUrbanCombat01",
        "EGAirForce01",
        "EGBasic01",
        "EGRapidAssault01",
        "EGRecon01",
        "GGBasic01",
        "GGEngineer01",
        "GGMarauder01",
        "GGSalvage01",
        "GGToxin01",
    ];
    let compact = key.replace('_', "").replace("Airforce", "AirForce");
    let name = if known.contains(&compact.as_str()) { compact } else { basic(faction).replace('_', "") };
    // The faction basics without loading art of their own (the APA's) use the plain backdrop.
    if known.contains(&name.as_str()) {
        format!("LoadingScreen/LoadingScreen_{name}")
    } else {
        "LoadingScreen/LoadingScreen_Background".to_owned()
    }
}

/// The faction's small logo from the alpha's top bar.
pub fn logo(faction: Faction) -> &'static str {
    match faction {
        Faction::Eu => "HudtopGenerals_Factionlogos/EU_sm",
        Faction::Apa => "HudtopGenerals_Factionlogos/APA_sm",
        Faction::Gla => "HudtopGenerals_Factionlogos/GLA_sm",
    }
}

/// The end screen's art: the faction's victory or defeat emblem.
pub fn outcome_art(faction: Faction, won: bool) -> String {
    let f = match faction {
        Faction::Eu => "EU",
        Faction::Apa => "APA",
        Faction::Gla => "GLA",
    };
    format!("Victory_Defeat/{f}_{}", if won { "Victory" } else { "Defeat" })
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

    /// Every general maps to a portrait, icon and loading art that the alpha ships (checked
    /// against the archived install when it is present; skipped otherwise).
    #[test]
    fn every_general_has_alpha_art() {
        let Some(dir) = std::env::var_os("G2_ALPHA_DIR") else { return };
        let images = std::path::Path::new(&dir).join("UTFWinAssets/images");
        let rules = g2_rules::rules();
        for (i, g) in rules.generals.iter().enumerate() {
            let id = Some(GeneralId(i as u16));
            for rel in [portrait(g.faction, id), loading_art(g.faction, id)] {
                assert!(images.join(format!("{rel}.png")).is_file(), "{}: no {rel}.png", g.name);
            }
        }
        for f in Faction::ALL {
            for rel in [portrait(f, None), loading_art(f, None), logo(f).to_owned(), outcome_art(f, true)] {
                assert!(images.join(format!("{rel}.png")).is_file(), "{f:?}: no {rel}.png");
            }
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
