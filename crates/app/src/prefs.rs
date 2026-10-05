//! Global preferences and most-recently-used files.
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Appearance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<actionlay_layout::color::Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel_opacity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<actionlay_layout::model::Units>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_layout: Option<PathBuf>,
    #[serde(default)]
    pub recent_videos: Vec<PathBuf>,
    #[serde(default)]
    pub recent_layouts: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_builtin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<Appearance>,
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
        let mut prefs: Self = serde_json::from_str(&text).unwrap_or_else(|e| {
            log::warn!("{}: {e}; using default preferences", path.display());
            Prefs::default()
        });
        normalize(&mut prefs.recent_videos);
        normalize(&mut prefs.recent_layouts);
        // Migrate preferences written before the layout chooser existed.
        if let Some(path) = prefs.last_layout.clone() {
            remember(&mut prefs.recent_layouts, path);
        }
        prefs
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

pub fn remember(recent: &mut Vec<PathBuf>, path: PathBuf) {
    let path =
        std::fs::canonicalize(&path).unwrap_or_else(|_| std::path::absolute(&path).unwrap_or(path));
    recent.retain(|p| p != &path);
    recent.insert(0, path);
    recent.truncate(10);
}

fn normalize(recent: &mut Vec<PathBuf>) {
    let paths = std::mem::take(recent);
    for path in paths.into_iter().rev() {
        remember(recent, path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_and_appearance_survive_immediate_save() {
        let path = temp("appearance");
        let prefs = Prefs {
            last_builtin: Some("training".into()),
            appearance: Some(Appearance {
                accent: Some(actionlay_layout::color::Color::rgba(10, 20, 30, 255)),
                panel_opacity: Some(0.5),
                units: Some(actionlay_layout::model::Units::Imperial),
            }),
            ..Default::default()
        };
        prefs.save(&path).unwrap();
        assert_eq!(Prefs::load(&path), prefs);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn recent_files_are_unique_ordered_and_limited_to_ten() {
        let mut recent = Vec::new();
        for index in 0..12 {
            remember(&mut recent, fixture(&format!("videos/{index}.mp4")));
        }
        assert_eq!(recent.len(), 10);
        assert_eq!(recent[0], fixture("videos/11.mp4"));
        assert!(!recent.contains(&fixture("videos/1.mp4")));
        remember(&mut recent, fixture("videos/5.mp4"));
        assert_eq!(recent[0], fixture("videos/5.mp4"));
        assert_eq!(recent.len(), 10);
        assert_eq!(
            recent
                .iter()
                .filter(|p| **p == fixture("videos/5.mp4"))
                .count(),
            1
        );
    }

    #[test]
    fn recent_videos_survive_save_and_clear_without_shutdown() {
        let path = temp("recent");
        let mut prefs = Prefs::default();
        remember(&mut prefs.recent_videos, fixture("videos/a.mp4"));
        prefs.save(&path).unwrap();
        assert_eq!(Prefs::load(&path).recent_videos, prefs.recent_videos);
        prefs.recent_videos.clear();
        prefs.save(&path).unwrap();
        assert!(Prefs::load(&path).recent_videos.is_empty());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    fn fixture(name: &str) -> PathBuf {
        std::env::temp_dir().join("actionlay-test-paths").join(name)
    }

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("actionlay-prefs-{}-{name}", std::process::id()))
            .join("prefs.json")
    }

    #[test]
    fn save_and_load_round_trip_creating_the_directory() {
        let path = temp("rt");
        let prefs = Prefs {
            last_layout: Some(fixture("layouts/mine.ovl.json")),
            recent_layouts: vec![fixture("layouts/mine.ovl.json")],
            ..Default::default()
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
