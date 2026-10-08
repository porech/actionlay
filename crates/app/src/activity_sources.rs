//! Local source discovery runs off the UI thread; a folder always uses strict matching.
use actionlay_telemetry::external::Activity;
use std::{path::PathBuf, sync::mpsc};

pub const FALLBACK: &str =
    "Timestamps could not be matched. Starts were aligned. Adjust the activity offset if needed.";
pub const NO_MATCH: &str =
    "No activity matches the video time. Select a single file to align starts.";
pub const UNKNOWN_TIME: &str = "Video time is unknown. Set Video UTC or select a single file.";
pub const CHOOSE: &str = "Several activities match. Choose one:";

pub struct Candidate {
    pub path: PathBuf,
    pub activity: Activity,
}
pub struct Scan {
    pub candidates: Vec<Candidate>,
    pub errors: Vec<String>,
}

pub fn supported(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| matches!(s.to_ascii_lowercase().as_str(), "gpx" | "fit" | "insgps"))
}

pub fn scan(paths: Vec<PathBuf>) -> Scan {
    let mut files = Vec::new();
    let mut errors = Vec::new();
    for path in paths {
        if path.is_dir() {
            // Traverse actual directories, never follow symlinked directories.
            let mut dirs = vec![path];
            while let Some(dir) = dirs.pop() {
                match std::fs::read_dir(&dir) {
                    Ok(entries) => {
                        for entry in entries {
                            match entry {
                                Ok(entry) => match entry.file_type() {
                                    Ok(kind) if kind.is_dir() => dirs.push(entry.path()),
                                    Ok(kind) if kind.is_file() && supported(&entry.path()) => {
                                        files.push(entry.path())
                                    }
                                    Ok(_) => {}
                                    Err(e) => {
                                        errors.push(format!("{}: {e}", entry.path().display()))
                                    }
                                },
                                Err(e) => errors.push(format!("{}: {e}", dir.display())),
                            }
                        }
                    }
                    Err(e) => errors.push(format!("{}: {e}", dir.display())),
                }
            }
        } else if supported(&path) {
            files.push(path);
        }
    }
    files.sort();
    files.dedup();
    let mut candidates = Vec::new();
    for path in files {
        let result = std::fs::metadata(&path)
            .map_err(|e| e.to_string())
            .and_then(|m| {
                if m.len() > 64 * 1024 * 1024 {
                    Err(crate::i18n::text("Activity file exceeds 64 MB").into())
                } else {
                    Activity::read(&path).map_err(error)
                }
            });
        match result {
            Ok(activity) => candidates.push(Candidate { path, activity }),
            Err(e) => errors.push(format!("{}: {e}", path.display())),
        }
    }
    Scan { candidates, errors }
}

pub fn spawn(paths: Vec<PathBuf>, wake: impl FnOnce() + Send + 'static) -> mpsc::Receiver<Scan> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        if tx.send(scan(paths)).is_ok() {
            wake();
        }
    });
    rx
}

/// Localize actionable errors; parser and operating-system details remain diagnostic.
pub fn error(error: actionlay_telemetry::external::ExternalError) -> String {
    use actionlay_telemetry::external::ExternalError;
    let message = match error {
        ExternalError::Read(detail) => {
            return format!(
                "{}: {}",
                crate::i18n::text("Activity file could not be read"),
                crate::i18n::text(&detail)
            );
        }
        ExternalError::NoSamples => "No timestamped samples.",
        ExternalError::NoOverlap => "No activity data at this offset.",
        ExternalError::InvalidSync => "Invalid activity offset",
        ExternalError::MissingVideoTime => UNKNOWN_TIME,
        ExternalError::InvalidInsgps => "Invalid INSGPS records",
    };
    crate::i18n::text(message).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directory_scan_is_recursive_deduplicated_and_reports_invalid_files() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        let valid = nested.join("ride.GPX");
        std::fs::write(&valid, r#"<gpx><trk><trkseg><trkpt lat="45" lon="7"><time>2026-07-29T14:30:05Z</time></trkpt></trkseg></trk></gpx>"#).unwrap();
        std::fs::write(dir.path().join("broken.insgps"), [1, 2, 3]).unwrap();
        std::fs::write(dir.path().join("notes.txt"), "not an activity").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path(), nested.join("loop")).unwrap();
        let result = scan(vec![dir.path().into(), valid.clone()]);
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].path, valid);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("broken.insgps"));
        let utc = chrono::DateTime::parse_from_rfc3339("2026-07-29T14:30:04Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert!(result.candidates[0].activity.overlaps_video(Some(utc), 4.0));
    }
}
