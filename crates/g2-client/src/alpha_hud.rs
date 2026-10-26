//! The 2013 alpha's HUD, drawn from its own layout (`ui/layouts/bottomBarUI.layout`) and art
//! when the player has imported their copy (`alpha.rs`). Only the shell comes from the
//! alpha: every number, the minimap and the build grid are this game's, fed by the same
//! `hud.rs` systems through the same `Field` markers. Slots the alpha had for systems this
//! game doesn't have (upgrades, stances, the generals' perk tree, superweapon timers) are
//! left empty.
//!
//! Each window in the layout becomes an absolutely placed UI node, sized by the alpha's own
//! scaling rule (`utfwin::place`) for the window's size; `hud.rs` rebuilds the HUD when the
//! window is resized.

use bevy::prelude::*;
use g2_sim::Faction;

use crate::alpha::Cache;
use crate::hud::Field;
use crate::utfwin::{self, Window};

/// The bottom-bar layout, read once per cache.
#[derive(Resource)]
pub struct Layout {
    pub bottom_bar: Window,
}

impl Layout {
    pub fn load(cache: &Cache) -> Option<Layout> {
        let ui = cache.0.join("ui");
        let images = utfwin::image_index(&ui.join("images"));
        let xml = std::fs::read_to_string(ui.join("layouts/bottomBarUI.layout")).ok()?;
        match utfwin::parse(&xml, &images) {
            Ok(bottom_bar) => Some(Layout { bottom_bar }),
            Err(e) => {
                warn!("alpha HUD layout unreadable, using our own HUD: {e}");
                None
            }
        }
    }
}

/// The alpha's build icon and portrait for each of our defs, under `ui/images/`.
pub fn icon_for(def: &str) -> Option<(&'static str, &'static str)> {
    Some(match def {
        "EU Command Center" => {
            ("BuildButtons/Active/EU/ES_CommandCenter", "UnitPortraits/EU/Structures/ES_CommandCenter_Portrait")
        }
        "EU Bulldozer" => {
            ("BuildButtons/Active/EU/EU_ConstructionUnit", "UnitPortraits/EU/Vehicles/EU_ConstructionUnit_Portrait")
        }
        "EU Supply Center" => {
            ("BuildButtons/Active/EU/ES_SupplyCenter", "UnitPortraits/EU/Structures/ES_SupplyCenter_Portrait")
        }
        "EU Harvester" => ("BuildButtons/Active/EU/EU_Harvester", "UnitPortraits/EU/Vehicles/EU_Harvester_Portrait"),
        "EU Power Plant" => {
            ("BuildButtons/Active/EU/ES_PowerPlant", "UnitPortraits/EU/Structures/ES_PowerPlant_Portrait")
        }
        "EU Oil Derrick" => {
            ("BuildButtons/Active/EU/ES_OilDerrick", "UnitPortraits/EU/Structures/ES_OilDerrick_Portrait")
        }
        "EU Barracks" => ("BuildButtons/Active/EU/ES_Barracks", "UnitPortraits/EU/Structures/ES_Barracks_Portrait"),
        "EU War Factory" => {
            ("BuildButtons/Active/EU/ES_WarFactory", "UnitPortraits/EU/Structures/ES_WarFactory_Portrait")
        }
        "EU Recon Squad" => {
            ("BuildButtons/Active/EU/EU_RifleSoldier_Squad", "UnitPortraits/EU/Units/EU_RifleSoldierSquad_Portrait")
        }
        "EU Rocket Squad" => {
            ("BuildButtons/Active/EU/EU_RocketSoldier_Squad", "UnitPortraits/EU/Units/EU_RocketSoldierSquad_Portrait")
        }
        "EU Aegis Tank" => {
            ("BuildButtons/Active/EU/EU_MediumCannonTank", "UnitPortraits/EU/Vehicles/EU_MediumCannonTank_Portrait")
        }
        "EU Howitzer" => (
            "BuildButtons/Active/EU/EU_MediumRocketVehicle",
            "UnitPortraits/EU/Vehicles/EU_MediumRocketVehicle_Portrait",
        ),
        "EU APC" => ("BuildButtons/Active/EU/EU_APC", "UnitPortraits/EU/Vehicles/EU_APC_Portrait"),
        "APA Command Center" => {
            ("BuildButtons/Active/APA/AS_CommandCenter", "UnitPortraits/APA/Structures/AS_CommandCenter_Portrait")
        }
        "APA Crane" => {
            ("BuildButtons/Active/APA/AU_ConstructionUnit", "UnitPortraits/APA/Vehicles/AU_ConstructionUnit_Portrait")
        }
        "APA Supply Center" => {
            ("BuildButtons/Active/APA/AS_SupplyCenter", "UnitPortraits/APA/Structures/AS_SupplyCenter_Portrait")
        }
        "APA Harvester" => ("BuildButtons/Active/APA/AU_Harvester", "UnitPortraits/APA/Vehicles/AU_Harvester_Portrait"),
        "APA Power Plant" => {
            ("BuildButtons/Active/APA/AS_PowerPlant", "UnitPortraits/APA/Structures/AS_PowerPlant_Portrait")
        }
        "APA Oil Derrick" => {
            ("BuildButtons/Active/APA/AS_OilDerrick", "UnitPortraits/APA/Structures/AS_OilDerrick_Portrait")
        }
        "APA Barracks" => ("BuildButtons/Active/APA/AS_Barracks", "UnitPortraits/APA/Structures/AS_Barracks_Portrait"),
        "APA War Factory" => {
            ("BuildButtons/Active/APA/AS_WarFactory", "UnitPortraits/APA/Structures/AS_Warfactory_Portrait")
        }
        "APA Red Guard" => {
            ("BuildButtons/Active/APA/AU_RifleSoldierSquad", "UnitPortraits/APA/Units/AU_RifleSoldierSquad_Portrait")
        }
        "APA Tank Hunter" => {
            ("BuildButtons/Active/APA/AU_RocketSoldierSquad", "UnitPortraits/APA/Units/AU_RocketSoldierSquad_Portrait")
        }
        "APA Warlord Tank" => {
            ("BuildButtons/Active/APA/AU_MedCannonTank", "UnitPortraits/APA/Vehicles/AU_MediumCannonTank_Portrait")
        }
        "APA Thunderfist" => (
            "BuildButtons/Active/APA/AU_HeavyCannonArtillery",
            "UnitPortraits/APA/Vehicles/AU_HeavyCannonArtillery_Portrait",
        ),
        "APA Gatling Tank" => {
            ("BuildButtons/Active/APA/AU_MedGunTank", "UnitPortraits/APA/Vehicles/AU_MedGunTank_Portrait")
        }
        "GLA Command Center" => {
            ("BuildButtons/Active/GLA/GS_CommandCenter", "UnitPortraits/GLA/Structures/GS_CommandCenter_Portrait")
        }
        "GLA Worker" => {
            ("BuildButtons/Active/GLA/GU_ConstructionUnit", "UnitPortraits/GLA/Units/GU_ConstructionUnit_Portrait")
        }
        "GLA Supply Stash" => {
            ("BuildButtons/Active/GLA/GS_SupplyCenter", "UnitPortraits/GLA/Structures/GS_SupplyCenter_Portrait")
        }
        "GLA Oil Derrick" => {
            ("BuildButtons/Active/GLA/GS_OilDerrick", "UnitPortraits/GLA/Structures/GS_OilDerrick_Portrait")
        }
        "GLA Barracks" => ("BuildButtons/Active/GLA/GS_Barracks", "UnitPortraits/GLA/Structures/GS_Barracks_Portrait"),
        "GLA War Factory" => {
            ("BuildButtons/Active/GLA/GS_WarFactory", "UnitPortraits/GLA/Structures/GS_WarFactory_Portrait")
        }
        "GLA Rabble Squad" => {
            ("BuildButtons/Active/GLA/GU_RebelSoldier_Squad", "UnitPortraits/GLA/Units/GU_RebelSoldierSquad_Portrait")
        }
        "GLA RPG Squad" => {
            ("BuildButtons/Active/GLA/GU_RocketSoldierSquad", "UnitPortraits/GLA/Units/GU_RocketSoldierSquad_Portrait")
        }
        "GLA Scorpion Tank" => {
            ("BuildButtons/Active/GLA/GU_MediumCannonTank", "UnitPortraits/GLA/Vehicles/GU_MediumCannonTank_Portrait")
        }
        "GLA Toxin Artillery" => (
            "BuildButtons/Active/GLA/GU_HeavyToxinArtillery",
            "UnitPortraits/GLA/Vehicles/GU_HeavyToxinArtillery_Portrait",
        ),
        "GLA Technical" => {
            ("BuildButtons/Active/GLA/GU_LightReconVehicle", "UnitPortraits/GLA/Vehicles/GU_LightReconVehicle_Portrait")
        }
        "EU Tech Lab" => {
            ("BuildButtons/Active/EU/ES_TechStructureA", "UnitPortraits/EU/Structures/ES_TechStructureA_Portrait")
        }
        "EU Advanced Tech Lab" => {
            ("BuildButtons/Active/EU/ES_TechStructureB", "UnitPortraits/EU/Structures/ES_TechStructureB_Portrait")
        }
        "GLA Salvage Bay" => {
            ("BuildButtons/Active/GLA/GS_TechStructureB", "UnitPortraits/GLA/Structures/GS_TechStructureB_Portrait")
        }
        "EU Airfield" => ("BuildButtons/Active/EU/ES_Airfield", "UnitPortraits/EU/Structures/ES_Airfield_Portrait"),
        "APA Airfield" => ("BuildButtons/Active/APA/AS_Airfield", "UnitPortraits/APA/Structures/AS_Airfield_Portrait"),
        "EU Valkyrie" => {
            ("BuildButtons/Active/EU/EU_HeavyRocketHelo", "UnitPortraits/EU/Vehicles/EU_HeavyRocketHelo_Portrait")
        }
        "APA Mantis" => {
            ("BuildButtons/Active/APA/AU_LightGunHelo", "UnitPortraits/APA/Vehicles/AU_LightGunHelo_Portrait")
        }
        "GLA Toxin Chopper" => {
            ("BuildButtons/Active/GLA/GU_LightChemicalHelo", "UnitPortraits/GLA/Vehicles/GU_LightChemicalHelo_Portrait")
        }
        "EU Bomber" => ("BuildButtons/Active/EU/EU_Airbomber", "UnitPortraits/EU/Vehicles/EU_Airbomber_Portrait"),
        "APA Bomber" => ("BuildButtons/Active/APA/AU_AirBomber", "UnitPortraits/APA/Vehicles/AU_AirBomber_Portrait"),
        "APA Fighter" => ("BuildButtons/Active/APA/AU_AirFighter", "UnitPortraits/APA/Vehicles/AU_AirFighter_Portrait"),
        "GLA Scud Storm" => {
            ("BuildButtons/Active/GLA/GS_ScudStorm", "UnitPortraits/GLA/Structures/GS_ScudStorm_Portrait")
        }
        "EU Gun Turret" => {
            ("BuildButtons/Active/EU/ES_BaseDefenseGun", "UnitPortraits/EU/Structures/ES_BaseDefenseGun_Portrait")
        }
        "APA Flame Turret" => {
            ("BuildButtons/Active/APA/AS_BaseDefenseFlame", "UnitPortraits/APA/Structures/AS_BaseDefenseFlame_Portrait")
        }
        "GLA Rocket Site" => (
            "BuildButtons/Active/GLA/GS_BaseDefenseRocket",
            "UnitPortraits/GLA/Structures/GS_BaseDefenseRocket_Portrait",
        ),
        _ => return None,
    })
}

/// The alpha's research button for each of our upgrades (its `ButtonIcon`), under `ui/images/`.
pub fn upgrade_icon(upgrade: &str) -> Option<&'static str> {
    Some(match upgrade {
        "Hightech Alloys" => "AbilitiesUpgrades/AdvancedAlloys",
        "Improved Ballistics" => "AbilitiesUpgrades/AdvancedBallistics",
        "Senior Warfighter" => "AbilitiesUpgrades/AdvancedWarfighter",
        "High Explosives" => "AbilitiesUpgrades/AdvancedOrdinance",
        "Hollow Point Ammo" => "AbilitiesUpgrades/HollowPoint",
        "Population Control" => "AbilitiesUpgrades/PopulationControl",
        "Shortened Fuses" => "AbilitiesUpgrades/shortenedFuses",
        "Scrap Repair" => "AbilitiesUpgrades/ScrapRepair",
        "Firestorm Shells" => "AbilitiesUpgrades/FirestormShells",
        _ => return None,
    })
}

/// The general's portrait in the bottom bar: the alpha's "basic" general of each faction.
fn general_portrait(faction: Faction) -> &'static str {
    match faction {
        Faction::Eu => "UserPortraits/EG_Basic_01_UP",
        Faction::Apa => "UserPortraits/AG_Basic_01_UP",
        Faction::Gla => "UserPortraits/GG_Basic_01_UP",
    }
}

/// An image under the `alpha://ui/images/` asset source.
pub fn image(server: &AssetServer, rel: &str) -> Handle<Image> {
    let rel = rel.strip_suffix(".png").unwrap_or(rel);
    server.load(bevy::asset::AssetPath::from(format!("ui/images/{rel}.png")).with_source("alpha"))
}

/// What a named layout window becomes: our field, marker or live image in that slot.
#[derive(Clone, Copy)]
enum Slot {
    Field(fn() -> Field),
    Minimap,
    Portrait,
    BuildGrid,
}

/// Windows by their layout name that our HUD fills. Everything else is drawn as the art.
fn slot(name: &str) -> Option<Slot> {
    Some(match name {
        // The alpha shows one of two gold texts, with or without power beside it; ours has
        // its own power field, so the "with power" one carries the money.
        "Gold Text with power" => Slot::Field(|| Field::Money),
        "Power Container" => Slot::Field(|| Field::PowerFigures),
        "Minimap container" => Slot::Minimap,
        "Timer" => Slot::Field(|| Field::Clock),
        "GeneralPortrait" => Slot::Portrait,
        "Button Container" => Slot::BuildGrid,
        _ => return None,
    })
}

/// Windows that only exist for systems we lack, or that hold debug and placeholder art;
/// they and everything under them are skipped.
fn skipped(name: &str) -> bool {
    matches!(
        name,
        "ALPHA PREVIEW"
            | "Pause Menu"
            | "Debug Unit Spawner"
            | "Taunt Button Container"
            | "House Color Toggle"
            | "Player Ping Button"
            | "Gold text without power"
            | "Experience bar"
            | "Allies"
            | "Enemies"
            | "Ally Taunt"
            | "Enemy Taunt"
            | "Objective Display"
            | "Text Display"
            | "GameMessageWidget"
            | "Status Display"
            | "Game End Screen"
            | "Onslaught Wave Announcement"
            | "Select Unit Buttons"
            | "Build Queue Select"
            | "Medium grid background"
            | "Short grid background"
            | "Menu Button Container"
    ) || name.starts_with("Edge Scroll")
}

/// Marks the general-portrait node so the selection can swap in a unit portrait.
#[derive(Component)]
pub struct PortraitSlot(pub Handle<Image>);

/// Builds the alpha bottom bar for `screen` (logical pixels): the art as nodes, our
/// fields, minimap and build grid in its slots. Returns false when the layout lacks a
/// slot this game needs, so the caller falls back to its own HUD.
pub fn spawn(commands: &mut Commands, layout: &Layout, server: &AssetServer, faction: Faction, screen: Vec2) -> bool {
    let hud = layout.bottom_bar.find("Heads up display").unwrap_or(&layout.bottom_bar);
    let needed = ["Minimap container", "Timer", "GeneralPortrait", "Button Container", "Power Container"];
    if needed.iter().any(|n| hud.find(n).is_none()) {
        warn!("alpha HUD layout lacks a slot this game fills, using our own HUD");
        return false;
    }
    let font = server.load(bevy::asset::AssetPath::from("ui/fonts/EurostileLTStdBold.otf").with_source("alpha"));
    let general = image(server, general_portrait(faction));
    let mut ctx = Ctx { commands, server, font, general, screen: (screen.x, screen.y) };
    for w in &hud.children {
        ctx.window(w, (0.0, 0.0), 1.0);
    }
    true
}

struct Ctx<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    server: &'a AssetServer,
    font: Handle<Font>,
    general: Handle<Image>,
    screen: (f32, f32),
}

impl Ctx<'_, '_, '_> {
    /// Spawns `w` and its subtree as root-level nodes, each placed in screen space: the
    /// alpha's layouts nest for authoring, not for layout flow, so no node is parented.
    fn window(&mut self, w: &Window, origin: (f32, f32), scale: f32) {
        if skipped(&w.name) {
            return;
        }
        let ([x, y, wd, ht], s) = utfwin::place(w, origin, scale, self.screen);
        let slot = slot(&w.name);
        let mut node = Node {
            position_type: PositionType::Absolute,
            left: px(x),
            top: px(y),
            width: px(wd.max(0.0)),
            height: px(ht.max(0.0)),
            ..default()
        };
        if matches!(slot, Some(Slot::Field(_))) {
            node.align_items = AlignItems::Center;
            node.justify_content = JustifyContent::Center;
        }
        let mut e = self.commands.spawn((crate::menu::MatchEntity, crate::hud::HudRoot, node));
        // Anything the alpha draws (art, a fill, a slot we fill) owns the screen under it;
        // pure group windows, often spanning the whole screen, do not.
        if slot.is_some() || w.image.is_some() || w.fill.is_some() {
            e.insert(crate::hud::HudPanel);
        }
        if let Some(fill) = w.fill {
            let [a, r, g, b] = fill.to_be_bytes();
            e.insert(BackgroundColor(Color::srgba_u8(r, g, b, a)));
        }
        match slot {
            Some(Slot::Portrait) => {
                e.insert((ImageNode::new(self.general.clone()), PortraitSlot(self.general.clone())));
            }
            Some(Slot::Minimap) => {
                e.insert((BackgroundColor(Color::srgb(0.10, 0.11, 0.08)), crate::hud::MinimapView));
            }
            Some(Slot::BuildGrid) => {
                e.with_children(|g| crate::hud::alpha_grid(g, wd, ht));
            }
            Some(Slot::Field(f)) => {
                let color = match f() {
                    Field::Money | Field::Power | Field::PowerFigures => Color::srgb_u8(0x00, 0xfd, 0x77),
                    _ => Color::WHITE,
                };
                let font = TextFont {
                    font: self.font.clone().into(),
                    font_size: FontSize::Px((16.0 * s).max(9.0)),
                    ..default()
                };
                // The alpha's own text child, when there is one, marks where the text goes
                // (beside the power icon, inside the timer's plate); else the whole slot.
                let [tx, ty, tw, th] = w.children.iter().find(|c| c.text.is_some()).map_or([0.0, 0.0, wd, ht], |c| {
                    let ([cx, cy, cw, ch], _) = utfwin::place(c, (x, y), s, self.screen);
                    [cx - x, cy - y, cw, ch]
                });
                let area = Node {
                    position_type: PositionType::Absolute,
                    left: px(tx),
                    top: px(ty),
                    width: px(tw.max(0.0)),
                    height: px(th.max(0.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                };
                e.with_children(|t| {
                    t.spawn((area, GlobalZIndex(1))).with_children(|t| {
                        t.spawn((Text::new(""), TextLayout::no_wrap(), font, TextColor(color), f()));
                    });
                });
            }
            None => {
                if let Some(img) = &w.image {
                    let mut node = ImageNode::new(image(self.server, &img.to_string_lossy()));
                    if w.tiled {
                        node.image_mode = NodeImageMode::Tiled { tile_x: true, tile_y: true, stretch_value: 1.0 };
                    }
                    e.insert(node);
                }
            }
        }
        // Our content replaces whatever the alpha drew inside a filled slot (minimap pins,
        // its own text, its button art); a field keeps the plates behind its text (the
        // timer's dark backing), with the text raised above them.
        match slot {
            Some(Slot::Minimap | Slot::BuildGrid) => {}
            Some(Slot::Field(_)) => {
                for c in w.children.iter().filter(|c| c.image.is_some() && c.text.is_none()) {
                    self.window(c, (x, y), s);
                }
            }
            _ => {
                for c in &w.children {
                    self.window(c, (x, y), s);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_def_with_a_faction_has_an_icon_and_portrait() {
        for def in g2_rules::rules().defs.iter().filter(|d| d.faction.is_some()) {
            assert!(icon_for(def.name).is_some(), "{} has no alpha icon", def.name);
        }
        for u in g2_rules::rules().upgrades {
            assert!(upgrade_icon(u.name).is_some(), "{} has no alpha icon", u.name);
        }
    }

    /// Against a real import (`G2_ALPHA_CACHE`; skipped when unset): the layout parses, has
    /// every slot this game fills, and every icon, portrait and image it names exists.
    #[test]
    fn the_imported_layout_and_icons_resolve() {
        let Some(dir) = std::env::var_os("G2_ALPHA_CACHE") else { return };
        let cache = Cache(std::path::PathBuf::from(dir));
        let layout = Layout::load(&cache).expect("layout loads");
        for name in ["Minimap container", "Timer", "GeneralPortrait", "Button Container", "Power Container"] {
            assert!(layout.bottom_bar.find(name).is_some(), "{name}");
        }
        let images = cache.0.join("ui/images");
        let exists = |rel: &str| images.join(format!("{rel}.png")).is_file();
        for def in g2_rules::rules().defs.iter().filter(|d| d.faction.is_some()) {
            let (icon, portrait) = icon_for(def.name).unwrap();
            assert!(exists(icon), "{}: {icon}", def.name);
            assert!(exists(portrait), "{}: {portrait}", def.name);
        }
        for u in g2_rules::rules().upgrades {
            let icon = upgrade_icon(u.name).unwrap();
            assert!(exists(icon), "{}: {icon}", u.name);
        }
        for f in [Faction::Eu, Faction::Apa, Faction::Gla] {
            assert!(exists(general_portrait(f)), "{f:?}");
        }
    }
}
