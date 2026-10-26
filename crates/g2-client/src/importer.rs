//! Importing the player's copy of the 2013 alpha from the menus: the system's file dialog
//! picks its `CnC.exe` (or the folder it's in), then `g2_frostbite`'s importer decodes it into
//! the cache `alpha.rs` reads, as `g2-tools alpha DIR import` does. Both run on a thread, so
//! the menu keeps drawing and shows how far the import is.
//!
//! The cache is read when the game starts (`alpha::register` makes it an asset source before
//! the app is built), so a fresh import shows from the next start on.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;

/// Where an import stands. Lives in a resource the menu reads; the worker thread writes it.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    Idle,
    /// The file dialog is open.
    Choosing,
    /// Decoding; the part under way (sounds, movies, textures, models, ui).
    Importing(String),
    /// Finished: what was written, and where.
    Done {
        summary: String,
        to: PathBuf,
    },
    Failed(String),
}

#[derive(Resource, Clone, Default)]
pub struct Import(pub Arc<Mutex<Status>>);

impl Import {
    pub fn status(&self) -> Status {
        self.0.lock().unwrap().clone()
    }

    fn set(&self, s: Status) {
        *self.0.lock().unwrap() = s;
    }

    /// Opens the file dialog and imports what the player picks, on a thread. Does nothing
    /// while one is already under way.
    pub fn start(&self) {
        if matches!(self.status(), Status::Choosing | Status::Importing(_)) {
            return;
        }
        let Some(out) = g2_frostbite::import::default_cache() else {
            self.set(Status::Failed("no data folder for the import: set G2_ALPHA_CACHE".to_owned()));
            return;
        };
        self.set(Status::Choosing);
        let me = self.clone();
        std::thread::spawn(move || {
            let picked = rfd::FileDialog::new()
                .set_title("Find the 2013 alpha's CnC.exe")
                .add_filter("CnC.exe", &["exe"])
                .pick_file();
            match picked {
                Some(exe) => me.run(&exe, &out),
                None => me.set(Status::Idle),
            }
        });
    }

    /// Imports the install `exe` belongs to into `out`.
    pub fn run(&self, exe: &Path, out: &Path) {
        let result = install_of(exe)
            .and_then(|dir| g2_frostbite::Alpha::open(&dir).map_err(|e| e.to_string()))
            .and_then(|alpha| {
                self.set(Status::Importing("reading".to_owned()));
                g2_frostbite::import::import(&alpha, out, g2_frostbite::import::Parts::default(), &mut |part| {
                    self.set(Status::Importing(part.to_owned()));
                })
                .map_err(|e| e.to_string())
            });
        self.set(match result {
            Ok(report) => Status::Done { summary: summary(&report), to: out.to_path_buf() },
            Err(e) => Status::Failed(e),
        });
    }
}

/// The install folder for a picked file: the alpha's `CnC.exe` sits beside its `Data/`.
/// Any file in that folder works too, and so does the folder itself.
fn install_of(picked: &Path) -> Result<PathBuf, String> {
    let dir = if picked.is_dir() { picked } else { picked.parent().unwrap_or(picked) };
    if dir.join("Data/cas.cat").is_file() || dir.join("cas.cat").is_file() {
        Ok(dir.to_path_buf())
    } else {
        Err(format!("{} is not the alpha's folder: it has no Data/cas.cat beside CnC.exe", dir.display()))
    }
}

/// "9110 sounds, 2154 models, ..." from the importer's counts.
fn summary(report: &g2_frostbite::import::Report) -> String {
    report.written.iter().map(|(k, n)| format!("{n} {k}")).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The picked `CnC.exe` leads to its folder; anything without the alpha's data beside it
    /// is refused before any decoding.
    #[test]
    fn the_install_is_the_folder_holding_cnc_exe() {
        let root = std::env::temp_dir().join(format!("g2-importer-{}", std::process::id()));
        std::fs::create_dir_all(root.join("Data")).unwrap();
        std::fs::write(root.join("Data/cas.cat"), b"").unwrap();
        std::fs::write(root.join("CnC.exe"), b"").unwrap();
        assert_eq!(install_of(&root.join("CnC.exe")), Ok(root.clone()));
        assert_eq!(install_of(&root), Ok(root.clone()));
        assert!(install_of(&root.join("Data/cas.cat")).is_ok(), "a pick inside Data/ is the data folder");
        assert!(install_of(&std::env::temp_dir().join("CnC.exe")).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
