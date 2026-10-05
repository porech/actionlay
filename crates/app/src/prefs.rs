//! Global preferences (spec §6.2). M2 keeps only the last layout used.
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_layout: Option<PathBuf>,
}

/// `prefs.json` in the OS config dir, e.g.
/// `~/Library/Application Support/org.ActionLay.ActionLay` on macOS,
/// `~/.config/actionlay` on Linux, `%APPDATA%\ActionLay\ActionLay\config` on Windows.
pub fn default_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("org", "ActionLay", "ActionLay")
        .map(|d| d.config_dir().join("prefs.json"))
}

impl Prefs {
    /// Missing or unreadable preferences are not an error: defaults are used.
    pub fn load(path: &Path) -> Prefs {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Prefs::default();
        };
        serde_json::from_str(&text).unwrap_or_else(|e| {
            log::warn!("{}: {e}; using default preferences", path.display());
            Prefs::default()
        })
    }

    /// Atomic: writes a temporary file next to `path`, then renames it over `path`.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        std::fs::write(&tmp, text + "\n")?;
        std::fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("actionlay-prefs-{}-{name}", std::process::id()))
            .join("prefs.json")
    }

    #[test]
    fn save_and_load_round_trip_creating_the_directory() {
        let path = temp("rt");
        let prefs = Prefs {
            last_layout: Some(PathBuf::from("/layouts/mine.ovl.json")),
        };
        prefs.save(&path).unwrap();
        assert_eq!(Prefs::load(&path), prefs);
        assert!(!path.with_extension("json.tmp").exists());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn missing_or_corrupt_file_gives_defaults() {
        assert_eq!(Prefs::load(&temp("missing")), Prefs::default());
        let path = temp("corrupt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(Prefs::load(&path), Prefs::default());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn unknown_fields_are_tolerated() {
        let path = temp("unknown");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"last_layout": "/a.ovl.json", "future": 1}"#).unwrap();
        assert_eq!(
            Prefs::load(&path).last_layout,
            Some(PathBuf::from("/a.ovl.json"))
        );
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn default_path_is_in_the_config_dir() {
        let p = default_path().expect("a config dir exists on desktop OSes");
        assert!(p.ends_with("prefs.json"));
        // Linux lowercases the application name (`~/.config/actionlay`).
        let lower = p.to_string_lossy().to_ascii_lowercase();
        assert!(lower.contains("actionlay"), "{}", p.display());
    }
}
