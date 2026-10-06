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
    #[serde(default)]
    pub buffering: actionlay_media::player::BufferingOptions,
    /// None follows regional measurement settings, independently of UI language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regional_units: Option<actionlay_layout::model::Units>,
    /// None means follow the system language, including future changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default)]
    pub system_integration_enabled: bool,
    #[serde(default)]
    pub dismiss_association_prompt: bool,
    #[serde(default)]
    pub show_diagnostic_data: bool,
    #[serde(default)]
    pub video_sources: std::collections::BTreeMap<String, SourceSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export: Option<crate::export::Settings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_device: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maps: Option<actionlay_maps::Settings>,
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
        // Older versions kept global units among appearance overrides. Move
        // them to regional preferences so explicit layout units retain priority.
        if let Some(old_units) = prefs
            .appearance
            .as_mut()
            .and_then(|appearance| appearance.units.take())
            && prefs.regional_units.is_none()
            && old_units != actionlay_layout::model::Units::Default
        {
            prefs.regional_units = Some(old_units);
        }
        prefs.buffering = prefs.buffering.normalized();
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
    fn buffering_preferences_round_trip_and_normalize_unsafe_limits() {
        let path = temp("buffering");
        let prefs = Prefs {
            buffering: actionlay_media::player::BufferingOptions {
                read_ahead_seconds: 12.0,
                start_buffer_seconds: 4.0,
                packet_memory_mib: 128,
            },
            ..Default::default()
        };
        prefs.save(&path).unwrap();
        assert_eq!(Prefs::load(&path), prefs);
        std::fs::write(&path, r#"{"buffering":{"read_ahead_seconds":0.5,"start_buffer_seconds":4.0,"packet_memory_mib":0}}"#).unwrap();
        let loaded = Prefs::load(&path);
        assert_eq!(loaded.buffering.start_buffer_seconds, 0.5);
        assert_eq!(loaded.buffering.packet_memory_mib, 1);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn legacy_global_units_migrate_without_overriding_layout_units() {
        let path = temp("regional-migration");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"appearance":{"units":"imperial","panel_opacity":0.5}}"#,
        )
        .unwrap();
        let prefs = Prefs::load(&path);
        assert_eq!(
            prefs.regional_units,
            Some(actionlay_layout::model::Units::Imperial)
        );
        assert_eq!(prefs.appearance.as_ref().unwrap().units, None);
        assert_eq!(prefs.appearance.as_ref().unwrap().panel_opacity, Some(0.5));
        prefs.save(&path).unwrap();
        assert_eq!(Prefs::load(&path), prefs);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn preset_and_appearance_survive_immediate_save() {
        let path = temp("appearance");
        let prefs = Prefs {
            system_integration_enabled: true,
            dismiss_association_prompt: true,
            audio_device: Some("BlackHole 2ch".into()),
            last_builtin: Some("training".into()),
            regional_units: Some(actionlay_layout::model::Units::Imperial),
            appearance: Some(Appearance {
                accent: Some(actionlay_layout::color::Color::rgba(10, 20, 30, 255)),
                panel_opacity: Some(0.5),
                units: None,
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

/// Per-video links and synchronization; privacy zones remain global in `maps`.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceSettings {
    pub activity: Option<PathBuf>,
    pub offset: f64,
    pub video_utc: String,
    pub open_alone: bool,
    pub load_sequence: bool,
}

pub fn video_identity(path: &Path) -> String {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let metadata = std::fs::metadata(&path).ok();
    let size = metadata.as_ref().map_or(0, |m| m.len());
    let modified = metadata
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |t| t.as_nanos());
    format!("{}|{size}|{modified}", path.display())
}
