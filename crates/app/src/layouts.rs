//! Which layout the overlay uses (bundled default, last used, or dropped by the
//! user) and how it scales on the video.
use std::path::{Path, PathBuf};

use actionlay_layout::geom::ScaleMode;
use actionlay_layout::{FILE_SUFFIX, Layout, default_layout};

use crate::prefs::Prefs;

pub enum Dropped {
    Layout(PathBuf),
    Video(PathBuf),
    Activity(PathBuf),
}

/// A dropped `*.ovl.json` (any case) is a layout; anything else is opened as a video.
pub fn classify(path: PathBuf) -> Dropped {
    let name = path.to_string_lossy().to_ascii_lowercase();
    if name.ends_with(FILE_SUFFIX)
        || name.ends_with(".xml")
        || actionlay_layout::package::is_package(&path)
    {
        Dropped::Layout(path)
    } else if name.ends_with(".gpx") || name.ends_with(".fit") {
        Dropped::Activity(path)
    } else {
        Dropped::Video(path)
    }
}

/// Validate before adding an independent copy to the library. Name conflicts keep
/// both documents; copying never overwrites another user's layout.
pub fn import_package(path: &Path) -> Result<PathBuf, String> {
    let dirs = directories::ProjectDirs::from("org", "ActionLay", "ActionLay")
        .ok_or("Layout library unavailable")?;
    import_package_into(path, &dirs.data_dir().join("layouts"))
}

fn import_package_into(path: &Path, library: &Path) -> Result<PathBuf, String> {
    let loaded = Layout::load(path).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(library).map_err(|e| e.to_string())?;
    if path
        .canonicalize()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        == library.canonicalize().ok()
    {
        return Ok(path.to_path_buf());
    }
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    for index in 1.. {
        let name = if index == 1 {
            stem.to_string()
        } else {
            format!("{stem}-{index}")
        };
        let out = library.join(format!("{name}.actionlay-layout"));
        let placeholder = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&out)
        {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        };
        drop(placeholder);
        if let Err(e) = actionlay_layout::package::save(&loaded.layout, &out) {
            let _ = std::fs::remove_file(&out);
            return Err(e.to_string());
        }
        return Ok(out);
    }
    unreachable!()
}

/// Loads a layout file. Validation warnings and render diagnostics are logged and
/// summarized in a one-line notice for the status bar. The error names the file
/// (`LayoutError::Io` does not) and fits on one line.
pub fn load(path: &Path) -> Result<(Layout, Option<String>), String> {
    let loaded = Layout::load(path).map_err(|e| one_line(&format!("{}: {e}", path.display())))?;
    let mut warnings = loaded.warnings;
    warnings.extend(actionlay_render::diagnose(&loaded.layout));
    for w in &warnings {
        log::warn!("{}: {w}", path.display());
    }
    let name = path.display();
    let notice = match warnings.as_slice() {
        [] => None,
        [w] => Some(format!("{name}: 1 warning: {}: {}", w.path, w.message)),
        [w, ..] => Some(format!(
            "{name}: {} warnings, first: {}: {}",
            warnings.len(),
            w.path,
            w.message
        )),
    };
    Ok((loaded.layout, notice))
}

/// `LayoutError::Invalid` lists one issue per line.
fn one_line(s: &str) -> String {
    s.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
}

pub struct Initial {
    pub layout: Layout,
    pub notice: Option<String>,
    /// The last layout no longer loads: the default is used and the preference should
    /// be forgotten (so the notice does not come back at every launch).
    pub fallback: bool,
}

/// The last layout used if it still loads, else the bundled default.
pub fn initial(prefs: &Prefs) -> Initial {
    let Some(path) = &prefs.last_layout else {
        if let Some(id) = prefs.last_builtin.as_deref() {
            if let Some(preset) = actionlay_layout::catalog::find(id) {
                return Initial {
                    layout: preset.layout(),
                    notice: None,
                    fallback: false,
                };
            }
            return Initial {
                layout: default_layout(),
                notice: Some(format!(
                    "layout `{id}` is no longer included — using the default layout"
                )),
                fallback: true,
            };
        }
        return Initial {
            layout: default_layout(),
            notice: None,
            fallback: false,
        };
    };
    match load(path) {
        Ok((layout, notice)) => Initial {
            layout,
            notice,
            fallback: false,
        },
        Err(e) => {
            log::warn!("{e}");
            Initial {
                layout: default_layout(),
                notice: Some(format!("{e} — using the default layout")),
                fallback: true,
            }
        }
    }
}

/// Appearance changes are applied to a fresh copy of the selected layout, so reset
/// returns to that preset/file's own theme. Preferences are global across layouts.
pub fn apply_appearance(layout: &mut Layout, appearance: Option<&crate::prefs::Appearance>) {
    let Some(a) = appearance else { return };
    if let Some(units) = a.units {
        layout.units = Some(units);
    }
    if a.accent.is_some() || a.panel_opacity.is_some() {
        let theme = layout.theme.get_or_insert_default();
        let mut panel = theme.resolve().panel;
        let palette = theme.palette.get_or_insert_default();
        if let Some(accent) = a.accent {
            palette.accent = Some(accent);
        }
        if let Some(opacity) = a.panel_opacity.filter(|v| v.is_finite()) {
            panel.a = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
            palette.panel = Some(panel);
        }
    }
}

/// Scale mode for a video of `width × height` pixels (spec §4.2): `Fit` only when the
/// layout's sized widgets would leave the frame or collide at the `Height` scale (e.g.
/// 9:16 footage under the default 16:9 layout), see
/// [`actionlay_layout::scale::auto_scale_mode`]. The project setting that overrides this
/// comes later.
pub fn scale_mode_for(width: u32, height: u32, layout: &Layout) -> ScaleMode {
    actionlay_layout::scale::auto_scale_mode(layout, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn importing_packages_keeps_both_names_and_assets() {
        let source = tempfile::tempdir().unwrap();
        let library = tempfile::tempdir().unwrap();
        let path = source.path().join("test.actionlay-layout");
        let mut layout = default_layout();
        actionlay_layout::package::attach(&mut layout, "assets/test.png".into(), vec![1, 2, 3])
            .unwrap();
        actionlay_layout::package::save(&layout, &path).unwrap();
        let first = import_package_into(&path, library.path()).unwrap();
        let second = import_package_into(&path, library.path()).unwrap();
        assert_ne!(first, second);
        assert_eq!(Layout::load(&first).unwrap().layout, layout);
        assert_eq!(Layout::load(&second).unwrap().layout, layout);
        assert_eq!(import_package_into(&first, library.path()).unwrap(), first);
    }

    #[test]
    fn selected_builtin_is_restored_and_unknown_builtin_falls_back() {
        let prefs = Prefs {
            last_builtin: Some("moto".into()),
            ..Default::default()
        };
        let loaded = initial(&prefs);
        assert_eq!(
            loaded.layout,
            actionlay_layout::catalog::find("moto").unwrap().layout()
        );
        assert!(!loaded.fallback);
        let prefs = Prefs {
            last_builtin: Some("removed".into()),
            ..Default::default()
        };
        let loaded = initial(&prefs);
        assert!(loaded.fallback);
        assert_eq!(loaded.layout, default_layout());
    }

    #[test]
    fn appearance_overrides_preserve_source_and_reset_to_its_own_defaults() {
        let original = actionlay_layout::catalog::find("moto").unwrap().layout();
        let mut styled = original.clone();
        let accent = actionlay_layout::color::Color::rgba(10, 20, 30, 255);
        apply_appearance(
            &mut styled,
            Some(&crate::prefs::Appearance {
                accent: Some(accent),
                panel_opacity: Some(0.5),
                units: Some(actionlay_layout::model::Units::Imperial),
            }),
        );
        let theme = styled.theme.unwrap().resolve();
        assert_eq!(theme.accent, accent);
        assert_eq!(theme.panel.a, 128);
        assert_eq!(styled.units, Some(actionlay_layout::model::Units::Imperial));
        let mut reset = original.clone();
        apply_appearance(&mut reset, None);
        assert_eq!(reset, original);
    }

    fn temp_layout(name: &str, text: &str) -> PathBuf {
        let p =
            std::env::temp_dir().join(format!("actionlay-{}-{name}.ovl.json", std::process::id()));
        std::fs::write(&p, text).unwrap();
        p
    }

    #[test]
    fn classifies_drops_by_suffix() {
        assert!(matches!(
            classify(PathBuf::from("/a/b/mine.ovl.json")),
            Dropped::Layout(_)
        ));
        assert!(matches!(
            classify(PathBuf::from("C:\\x\\MINE.OVL.JSON")),
            Dropped::Layout(_)
        ));
        assert!(matches!(
            classify(PathBuf::from("/v/GX010001.MP4")),
            Dropped::Video(_)
        ));
        assert!(matches!(
            classify(PathBuf::from("/v/notes.json")),
            Dropped::Video(_)
        ));
    }

    #[test]
    fn starts_with_the_default_layout() {
        let i = initial(&Prefs::default());
        assert_eq!(i.layout, default_layout());
        assert!(i.notice.is_none());
        assert!(!i.fallback);
    }

    #[test]
    fn starts_with_the_last_layout_when_it_still_loads() {
        let mut mine = default_layout();
        mine.name = Some("mine".into());
        let path = temp_layout("last", &mine.to_json().unwrap());
        let i = initial(&Prefs {
            last_layout: Some(path.clone()),
            ..Default::default()
        });
        std::fs::remove_file(&path).ok();
        assert_eq!(i.layout.name.as_deref(), Some("mine"));
        assert!(i.notice.is_none(), "{:?}", i.notice);
        assert!(!i.fallback);
    }

    #[test]
    fn falls_back_to_default_when_the_last_layout_is_broken_or_gone() {
        let path = temp_layout(
            "broken",
            "{\"version\": 1, \"nodes\": [{\"type\": \"text\"}]}",
        );
        let i = initial(&Prefs {
            last_layout: Some(path.clone()),
            ..Default::default()
        });
        std::fs::remove_file(&path).ok();
        assert_eq!(i.layout, default_layout());
        assert!(i.fallback);
        let notice = i.notice.unwrap();
        assert!(notice.contains(&path.display().to_string()), "{notice}");
        assert!(notice.contains("default layout"), "{notice}");
        assert!(!notice.contains('\n'), "{notice}");

        let gone = initial(&Prefs {
            last_layout: Some(path.clone()),
            ..Default::default()
        });
        assert_eq!(gone.layout, default_layout());
        assert!(gone.fallback);
        let notice = gone.notice.unwrap();
        // LayoutError::Io carries no path: the notice must name the file.
        assert!(notice.contains(&path.display().to_string()), "{notice}");
        assert!(notice.contains("cannot read layout"), "{notice}");
    }

    #[test]
    fn bad_json_is_an_error_naming_the_file() {
        let path = temp_layout("badjson", "{not json");
        let err = load(&path).unwrap_err();
        std::fs::remove_file(&path).ok();
        assert!(err.contains(&path.display().to_string()), "{err}");
        assert!(err.contains("invalid layout JSON"), "{err}");
    }

    #[test]
    fn warnings_become_a_notice() {
        let path = temp_layout(
            "warn",
            r#"{"version": 1, "nodes": [{"type": "metric", "metric": "heartbeat"}, {"type": "moving_map"}]}"#,
        );
        let (layout, notice) = load(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(layout.nodes.len(), 2);
        let notice = notice.unwrap();
        assert!(notice.contains("2 warnings"), "{notice}");
        // the whole path, not only the file name
        assert!(notice.contains(&path.display().to_string()), "{notice}");
    }

    #[test]
    fn auto_scale_mode_of_the_default_layout() {
        let d = default_layout();
        assert_eq!(scale_mode_for(1920, 1440, &d), ScaleMode::Height); // 4:3
        assert_eq!(scale_mode_for(2704, 2028, &d), ScaleMode::Height); // 4:3
        assert_eq!(scale_mode_for(1080, 1920, &d), ScaleMode::Fit); // 9:16
        assert_eq!(scale_mode_for(1920, 1080, &d), ScaleMode::Height); // 16:9
        assert_eq!(scale_mode_for(3840, 2160, &d), ScaleMode::Height); // 16:9
        assert_eq!(scale_mode_for(3840, 1600, &d), ScaleMode::Height); // 2.4:1
        // A degenerate size never panics.
        assert_eq!(scale_mode_for(0, 0, &d), ScaleMode::Height);
    }
}
