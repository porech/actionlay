//! Which layout the overlay uses (bundled default, last used, or dropped by the
//! user) and how it scales on the video.
use std::path::{Path, PathBuf};

use actionlay_layout::geom::ScaleMode;
use actionlay_layout::{FILE_SUFFIX, Layout, default_layout};

use crate::prefs::Prefs;

pub enum Dropped {
    Layout(PathBuf),
    Video(PathBuf),
}

/// A dropped `*.ovl.json` (any case) is a layout; anything else is opened as a video.
pub fn classify(path: PathBuf) -> Dropped {
    let name = path.to_string_lossy().to_ascii_lowercase();
    if name.ends_with(FILE_SUFFIX) {
        Dropped::Layout(path)
    } else {
        Dropped::Video(path)
    }
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
