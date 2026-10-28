//! The player's camera and input settings, after OpenRA's Display and Input panels
//! (`OpenRA.Game/Settings.cs`, `GameSettings` and `GraphicSettings.ViewportDistance`): the
//! battlefield camera distance, the control scheme, how the map pans by mouse, by the screen
//! edge and at what speed, the zoom speed and its modifier key, and locking the mouse to the
//! window. They change only how the local player looks and clicks, never the sim.
//!
//! Saved as plain text at `$G2_SETTINGS`, else `settings` in the game's data folder, one
//! `key value` per line. A line that doesn't parse keeps its default, so an old or hand-edited
//! file never breaks the game.

use std::path::PathBuf;

use bevy::prelude::*;

/// OpenRA's Battlefield Camera: how much of the battlefield the default view shows. The
/// default view is also the furthest out; the wheel zooms in up to twice as close.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Battlefield {
    /// This game's own framing, tuned against the 2013 footage (docs/STYLE.md).
    #[default]
    Close,
    Medium,
    Far,
}

impl Battlefield {
    pub const ALL: [Battlefield; 3] = [Battlefield::Close, Battlefield::Medium, Battlefield::Far];

    /// Camera distance of the default (furthest) view. OpenRA sizes its views as window
    /// heights in world pixels, Close 480-600, Medium 600-900, Far 900-1300; their midpoints
    /// are 1 : 1.39 : 2.04, so Medium shows 1.4 and Far 2 times as much as Close.
    pub fn distance(self) -> f32 {
        crate::camera::DEFAULT_DISTANCE
            * match self {
                Battlefield::Close => 1.0,
                Battlefield::Medium => 1.4,
                Battlefield::Far => 2.0,
            }
    }

    fn name(self) -> &'static str {
        match self {
            Battlefield::Close => "close",
            Battlefield::Medium => "medium",
            Battlefield::Far => "far",
        }
    }
}

/// OpenRA's Control Scheme. Its third, Other RTS, differs from Modern only in how order modes
/// (attack-move or guard picked from a button, then aimed) are confirmed, and this game has
/// none: attack-move is Ctrl + click.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Scheme {
    /// Left click selects, and with units selected orders them; right click deselects; the
    /// right button pans.
    Classic,
    /// Left click selects, right click orders; the middle button pans.
    #[default]
    Modern,
}

/// OpenRA's Pan Behaviour: what dragging with the pan button does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Pan {
    Disabled,
    /// The battlefield follows the cursor, as if grabbed.
    Standard,
    /// The view follows the cursor: the battlefield moves the other way.
    Inverted,
    /// The view runs toward the cursor at a speed set by how far it has been dragged from
    /// where the button went down (Tiberian Sun's pan). OpenRA's default.
    #[default]
    Joystick,
}

impl Pan {
    pub const ALL: [Pan; 4] = [Pan::Disabled, Pan::Standard, Pan::Inverted, Pan::Joystick];

    fn name(self) -> &'static str {
        match self {
            Pan::Disabled => "disabled",
            Pan::Standard => "standard",
            Pan::Inverted => "inverted",
            Pan::Joystick => "joystick",
        }
    }
}

/// OpenRA's Zoom Modifier: a key that must be held for the wheel to zoom.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Modifier {
    #[default]
    None,
    Alt,
    Ctrl,
    Meta,
    Shift,
}

impl Modifier {
    pub const ALL: [Modifier; 5] = [Modifier::None, Modifier::Alt, Modifier::Ctrl, Modifier::Meta, Modifier::Shift];

    fn name(self) -> &'static str {
        match self {
            Modifier::None => "none",
            Modifier::Alt => "alt",
            Modifier::Ctrl => "ctrl",
            Modifier::Meta => "meta",
            Modifier::Shift => "shift",
        }
    }

    /// Whether it is held (`None` always is).
    pub fn held(self, keys: &ButtonInput<KeyCode>) -> bool {
        match self {
            Modifier::None => true,
            Modifier::Alt => keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]),
            Modifier::Ctrl => keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]),
            Modifier::Meta => keys.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight]),
            Modifier::Shift => keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
        }
    }
}

/// OpenRA's Pan Speed slider: 10 to 50 in seven ticks, default 30.
pub const PAN_SPEEDS: [f32; 7] = [10.0, 16.67, 23.33, 30.0, 36.67, 43.33, 50.0];
/// OpenRA's Zoom Speed slider: exponential from 0.01 to 0.4 in seven ticks; OpenRA's default,
/// 0.04, is the third.
pub const ZOOM_SPEEDS: [f32; 7] = [0.01, 0.02, 0.04, 0.07, 0.12, 0.22, 0.4];

#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Settings {
    pub camera: Battlefield,
    pub scheme: Scheme,
    pub pan: Pan,
    /// Pan with the other button: the middle one under Classic, the right one under Modern.
    pub alternate_pan: bool,
    pub edge_pan: bool,
    pub lock_mouse: bool,
    pub pan_speed: f32,
    /// Fraction of zoom per wheel notch, compounded (`exp`), so in and out are symmetric.
    pub zoom_speed: f32,
    pub zoom_modifier: Modifier,
    /// Where it is saved; `None` keeps it in memory only (tests, headless runs).
    pub path: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            camera: Battlefield::default(),
            scheme: Scheme::default(),
            pan: Pan::default(),
            alternate_pan: false,
            edge_pan: true,
            lock_mouse: false,
            pan_speed: 30.0,
            zoom_speed: 0.04,
            zoom_modifier: Modifier::None,
            path: None,
        }
    }
}

impl Settings {
    /// The default save location.
    pub fn default_path() -> Option<PathBuf> {
        if let Some(p) = std::env::var_os("G2_SETTINGS") {
            return Some(PathBuf::from(p));
        }
        Some(g2_frostbite::import::data_dir()?.join("settings"))
    }

    /// Reads the settings at `path`; a missing file gives the defaults.
    pub fn load(path: Option<PathBuf>) -> Settings {
        let text = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        let mut s = Settings { path, ..Settings::default() };
        for line in text.lines() {
            let Some((key, value)) = line.split_once(' ') else { continue };
            let value = value.trim();
            let pick = |names: &[&str]| names.iter().position(|n| *n == value);
            let on = match value {
                "on" => Some(true),
                "off" => Some(false),
                _ => None,
            };
            let slider = |range: &[f32; 7]| value.parse::<f32>().ok().filter(|v| (range[0]..=range[6]).contains(v));
            match key {
                "camera" => {
                    if let Some(i) = pick(&Battlefield::ALL.map(Battlefield::name)) {
                        s.camera = Battlefield::ALL[i];
                    }
                }
                "scheme" => match value {
                    "classic" => s.scheme = Scheme::Classic,
                    "modern" => s.scheme = Scheme::Modern,
                    _ => {}
                },
                "pan" => {
                    if let Some(i) = pick(&Pan::ALL.map(Pan::name)) {
                        s.pan = Pan::ALL[i];
                    }
                }
                "alternate-pan" => s.alternate_pan = on.unwrap_or(s.alternate_pan),
                "edge-pan" => s.edge_pan = on.unwrap_or(s.edge_pan),
                "lock-mouse" => s.lock_mouse = on.unwrap_or(s.lock_mouse),
                "pan-speed" => s.pan_speed = slider(&PAN_SPEEDS).unwrap_or(s.pan_speed),
                "zoom-speed" => s.zoom_speed = slider(&ZOOM_SPEEDS).unwrap_or(s.zoom_speed),
                "zoom-modifier" => {
                    if let Some(i) = pick(&Modifier::ALL.map(Modifier::name)) {
                        s.zoom_modifier = Modifier::ALL[i];
                    }
                }
                _ => {}
            }
        }
        s
    }

    /// Writes the settings; errors are reported and ignored.
    pub fn save(&self) {
        let Some(path) = &self.path else { return };
        let on = |b: bool| if b { "on" } else { "off" };
        let scheme = match self.scheme {
            Scheme::Classic => "classic",
            Scheme::Modern => "modern",
        };
        let out = format!(
            "camera {}\nscheme {scheme}\npan {}\nalternate-pan {}\nedge-pan {}\nlock-mouse {}\npan-speed {}\nzoom-speed {}\nzoom-modifier {}\n",
            self.camera.name(),
            self.pan.name(),
            on(self.alternate_pan),
            on(self.edge_pan),
            on(self.lock_mouse),
            self.pan_speed,
            self.zoom_speed,
            self.zoom_modifier.name(),
        );
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(path, out) {
            eprintln!("settings: can't save {}: {e}", path.display());
        }
    }

    /// The input settings back at their defaults; the file stays where it is.
    pub fn reset(&mut self) {
        *self = Settings { path: self.path.take(), ..Settings::default() };
    }

    /// The button that pans: OpenRA's `MouseControlStyle == Classic ^ UseAlternateScrollButton`
    /// picks the right one, else the middle.
    pub fn pan_button(&self) -> MouseButton {
        if (self.scheme == Scheme::Classic) ^ self.alternate_pan { MouseButton::Right } else { MouseButton::Middle }
    }

    /// The closest and furthest camera distance: the default view is the furthest, the wheel
    /// zooms in to half of it (OpenRA's `MaxZoomScale` 2). Spectators may also zoom out to twice
    /// it (OpenRA's observer `UnlockMinimumZoom(0.5)`).
    pub fn zoom_range(&self, spectating: bool) -> (f32, f32) {
        let far = self.camera.distance();
        (far / 2.0, if spectating { far * 2.0 } else { far })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Saved settings read back the same; a line with an unknown key or value, or a slider
    /// value outside its range, keeps the default rather than breaking the game.
    #[test]
    fn settings_round_trip_through_the_file() {
        let dir = std::env::temp_dir().join(format!("g2-settings-{}", std::process::id()));
        let path = dir.join("settings");
        let s = Settings {
            camera: Battlefield::Far,
            scheme: Scheme::Classic,
            pan: Pan::Inverted,
            alternate_pan: true,
            edge_pan: false,
            lock_mouse: true,
            pan_speed: PAN_SPEEDS[5],
            zoom_speed: ZOOM_SPEEDS[6],
            zoom_modifier: Modifier::Ctrl,
            path: Some(path.clone()),
        };
        s.save();
        assert_eq!(Settings::load(Some(path.clone())), s);
        std::fs::write(
            &path,
            "camera huge\npan-speed 900\nzoom-speed fast\nedge-pan maybe\nwhat ever\nscheme classic\n",
        )
        .unwrap();
        let junk = Settings::load(Some(path.clone()));
        assert_eq!(junk, Settings { scheme: Scheme::Classic, path: Some(path), ..Settings::default() });
        let _ = std::fs::remove_dir_all(dir);
    }

    /// OpenRA's pan button: right under Classic, middle under Modern, swapped by Alternate.
    #[test]
    fn the_pan_button_follows_scheme_and_alternate() {
        let pan = |scheme, alternate_pan| Settings { scheme, alternate_pan, ..Settings::default() }.pan_button();
        assert_eq!(pan(Scheme::Classic, false), MouseButton::Right);
        assert_eq!(pan(Scheme::Classic, true), MouseButton::Middle);
        assert_eq!(pan(Scheme::Modern, false), MouseButton::Middle);
        assert_eq!(pan(Scheme::Modern, true), MouseButton::Right);
    }
}
