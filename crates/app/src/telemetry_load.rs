//! Reads a video's GPMF track and builds its telemetry off the UI thread (spec §3).
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError, mpsc};

use actionlay_media::gpmf::{GpmfPacket, read_gpmf_packets};
use actionlay_telemetry::{RawPacket, Telemetry, TelemetryOptions};

pub struct Loaded {
    pub telemetry: Telemetry,
    /// Shown to the user; None for a video that simply has no telemetry.
    pub warning: Option<String>,
}

/// The telemetry crate does not depend on the media crate: convert the packet structs.
pub fn to_raw(packets: Vec<GpmfPacket>) -> Vec<RawPacket> {
    packets
        .into_iter()
        .map(|p| RawPacket {
            pts: p.pts,
            duration: p.duration,
            data: p.data,
        })
        .collect()
}

/// Decoding options for a video of `duration` seconds: coverage is measured on the
/// whole video and the last values are held through the tail the GPMF track misses.
pub fn options(duration: f64) -> TelemetryOptions {
    TelemetryOptions {
        video_duration: Some(duration),
        ..Default::default()
    }
}

/// Never fails: without usable telemetry the video gets `Telemetry::empty` and the
/// overlay shows its designed empty states. `duration` is the video's (from the probe).
pub fn load(path: &Path, duration: f64) -> Loaded {
    let empty = |warning: Option<String>| Loaded {
        telemetry: Telemetry::empty(duration),
        warning,
    };
    // Lowers FFmpeg's process-wide log level while reading; the player running on
    // other threads may log less meanwhile, which is harmless.
    let packets = match read_gpmf_packets(path) {
        Ok(p) => p,
        Err(e) => return empty(Some(format!("telemetry not read: {e}"))),
    };
    if packets.is_empty() {
        return empty(None);
    }
    match Telemetry::from_gpmf_packets_with(&to_raw(packets), &options(duration)) {
        Ok(telemetry) => {
            for w in telemetry.warnings() {
                log::warn!("{}: {w}", path.display());
            }
            Loaded {
                telemetry,
                warning: None,
            }
        }
        Err(e) => empty(Some(format!("telemetry not decoded: {e}"))),
    }
}

/// Serializes loads: `read_gpmf_packets` lowers and restores FFmpeg's process-wide log
/// level, so two overlapping loads could restore it in the wrong order.
static LOADING: Mutex<()> = Mutex::new(());

/// Loads the telemetry on a background thread; the receiver gets exactly one `Loaded`.
/// Loads run one at a time: a load started while another runs waits for it (off the UI
/// thread); the result of a load whose receiver was dropped is discarded.
pub fn spawn(path: PathBuf, duration: f64) -> mpsc::Receiver<Loaded> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("telemetry".into())
        .spawn(move || {
            let _one_at_a_time = LOADING.lock().unwrap_or_else(PoisonError::into_inner);
            // The receiver may be gone (another video was opened): nothing to do.
            let _ = tx.send(load(&path, duration));
        })
        .expect("spawn telemetry thread");
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use actionlay_telemetry::{Metric, Value};

    /// Sample lookup with skip-when-absent (same rules as the media crate's tests).
    fn sample(rel: &str) -> Option<PathBuf> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples");
        let mut dirs: Vec<PathBuf> = std::env::var_os("ACTIONLAY_SAMPLES")
            .map(PathBuf::from)
            .into_iter()
            .collect();
        dirs.push(root.join("synthetic"));
        dirs.push(root.clone());
        let found = dirs.into_iter().map(|d| d.join(rel)).find(|p| p.exists());
        if found.is_none() {
            eprintln!("sample {rel} not found, skipping");
        }
        found
    }

    fn speed() -> Metric {
        Metric::from_id("speed").unwrap()
    }

    #[test]
    fn to_raw_keeps_every_field() {
        let raw = to_raw(vec![GpmfPacket {
            pts: 1.5,
            duration: 1.001,
            data: vec![1, 2, 3],
        }]);
        assert_eq!(raw.len(), 1);
        assert_eq!(
            (raw[0].pts, raw[0].duration, raw[0].data.as_slice()),
            (1.5, 1.001, &[1u8, 2, 3][..])
        );
    }

    #[test]
    fn load_without_gpmf_returns_empty_telemetry() {
        let Some(path) = sample("hevc8-1080p30-noaudio.mp4") else {
            return;
        };
        let loaded = load(&path, 10.0);
        assert!(loaded.warning.is_none(), "{:?}", loaded.warning);
        assert_eq!(loaded.telemetry.duration(), 10.0);
        assert_eq!(loaded.telemetry.availability().coverage(speed()), 0.0);
    }

    #[test]
    fn unreadable_file_gives_empty_telemetry_and_a_warning() {
        let loaded = load(Path::new("/nonexistent/video.mp4"), 5.0);
        let warning = loaded.warning.expect("a warning for an unreadable file");
        assert!(warning.contains("telemetry"), "{warning}");
        assert_eq!(loaded.telemetry.duration(), 5.0);
    }

    #[test]
    fn options_carry_the_video_duration() {
        assert_eq!(options(42.5).video_duration, Some(42.5));
        assert_eq!(options(42.5).lock, TelemetryOptions::default().lock);
    }

    #[test]
    fn hero5_sample_has_gps_through_the_last_frame() {
        let Some(path) = sample("gopro/hero5.mp4") else {
            return;
        };
        let duration = actionlay_media::probe::probe(&path).unwrap().duration;
        let rx = spawn(path, duration);
        let loaded = rx
            .recv_timeout(std::time::Duration::from_secs(60))
            .expect("telemetry thread answers");
        assert!(loaded.warning.is_none(), "{:?}", loaded.warning);
        let tel = &loaded.telemetry;
        assert!(tel.start_utc().is_some());
        assert!(tel.availability().coverage(speed()) > 0.3);
        // The GPMF track ends before the video; with the video duration the last
        // value is held through the tail.
        assert!(tel.duration() < duration, "{} {duration}", tel.duration());
        let last = tel.sample(duration - 0.01).get(speed());
        assert!(matches!(last, Value::Present(_)), "{last:?}");
    }
}
