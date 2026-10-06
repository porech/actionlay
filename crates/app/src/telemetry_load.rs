//! Reads a video's GPMF track and builds its telemetry off the UI thread (spec §3).
use actionlay_media::player::TelemetryEvent;
use std::collections::BTreeMap;
#[cfg(test)]
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

use actionlay_media::gpmf::GpmfPacket;
#[cfg(test)]
use actionlay_media::gpmf::read_gpmf_packets;
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
#[cfg(test)]
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

/// The latest snapshot only; a hidden window cannot accumulate heavy snapshots.
/// Dropping the handle cancels its decoder, without blocking another video's load.
type RouteRequest = (actionlay_media::chapters::Timeline, Vec<(f64, f64)>);

pub struct StreamLoad {
    latest: Arc<Mutex<Option<Loaded>>>,
    cancelled: Arc<AtomicBool>,
    route_busy: Arc<AtomicBool>,
    route_requests: Option<mpsc::Sender<RouteRequest>>,
}

impl StreamLoad {
    pub fn request_route(
        &self,
        timeline: actionlay_media::chapters::Timeline,
        ranges: Vec<(f64, f64)>,
    ) -> bool {
        let Some(tx) = &self.route_requests else {
            return false;
        };
        if self.route_busy.swap(true, Ordering::SeqCst) {
            return false;
        }
        if tx.send((timeline, ranges)).is_err() {
            self.route_busy.store(false, Ordering::SeqCst);
            return false;
        }
        true
    }

    pub fn take_update(&self) -> Option<Loaded> {
        self.latest.lock().unwrap().take()
    }
}

impl Drop for StreamLoad {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
}

pub fn spawn(
    input: mpsc::Receiver<TelemetryEvent>,
    duration: f64,
    expected: Option<usize>,
    wake: impl Fn() + Send + 'static,
) -> StreamLoad {
    let latest = Arc::new(Mutex::new(None));
    let cancelled = Arc::new(AtomicBool::new(false));
    let (route_tx, route_rx) = mpsc::channel::<RouteRequest>();
    let (extra_tx, extra_rx) = mpsc::channel();
    let route_cancelled = cancelled.clone();
    let route_busy = Arc::new(AtomicBool::new(false));
    let reader_busy = route_busy.clone();
    std::thread::spawn(move || {
        let mut readers = BTreeMap::new();
        while let Ok((timeline, ranges)) = route_rx.recv() {
            if route_cancelled.load(Ordering::SeqCst) {
                return;
            }
            for (start, end) in ranges {
                for chapter in &timeline.chapters {
                    if chapter.start > end || chapter.start + chapter.info.duration < start {
                        continue;
                    }
                    let reader = match readers.entry(chapter.path.clone()) {
                        std::collections::btree_map::Entry::Occupied(e) => e.into_mut(),
                        std::collections::btree_map::Entry::Vacant(e) => {
                            match actionlay_media::gpmf::GpmfReader::open(
                                &chapter.path,
                                route_cancelled.clone(),
                            ) {
                                Ok(reader) => e.insert(reader),
                                Err(e) => {
                                    let _ = extra_tx.send(Err(format!(
                                        "Route metadata could not be opened: {e}"
                                    )));
                                    continue;
                                }
                            }
                        }
                    };
                    match reader.read_range((start - chapter.start).max(0.0), end - chapter.start) {
                        Ok(packets) => {
                            for mut packet in packets {
                                packet.pts += chapter.start;
                                let timestamp = (packet.pts * 1_000_000.0).round() as i64;
                                if extra_tx
                                    .send(Ok(TelemetryEvent::Packet { timestamp, packet }))
                                    .is_err()
                                {
                                    return;
                                }
                            }
                        }
                        Err(e) => {
                            let _ = extra_tx
                                .send(Err(format!("Route metadata could not be read: {e}")));
                        }
                    }
                }
            }
            reader_busy.store(false, Ordering::SeqCst);
        }
    });
    let load = StreamLoad {
        latest: latest.clone(),
        cancelled: cancelled.clone(),
        route_busy,
        route_requests: Some(route_tx),
    };
    std::thread::Builder::new()
        .name("telemetry".into())
        .spawn(move || {
            let mut packets = BTreeMap::new();
            let mut dirty = false;
            let mut complete = false;
            let mut published = false;
            let mut last = Instant::now();
            let mut route_warning = None;
            loop {
                if cancelled.load(Ordering::SeqCst) {
                    return;
                }
                for event in extra_rx.try_iter() {
                    if let Err(warning) = &event {
                        route_warning = Some(warning.clone());
                        dirty = true;
                    }
                    if let Ok(TelemetryEvent::Packet { timestamp, packet }) = event
                        && let std::collections::btree_map::Entry::Vacant(e) =
                            packets.entry(timestamp)
                    {
                        e.insert(packet);
                        dirty = true;
                    }
                }
                complete |= expected.is_some_and(|n| packets.len() >= n);
                let event = input.recv_timeout(Duration::from_millis(50));
                let disconnected = matches!(event, Err(mpsc::RecvTimeoutError::Disconnected));
                match event {
                    Ok(TelemetryEvent::Packet { timestamp, packet }) => {
                        if let std::collections::btree_map::Entry::Vacant(e) =
                            packets.entry(timestamp)
                        {
                            e.insert(packet);
                            dirty = true;
                        }
                        complete |= expected.is_some_and(|n| packets.len() >= n);
                    }
                    Ok(TelemetryEvent::End { from_start }) => {
                        let all = expected.map_or(from_start, |n| packets.len() >= n);
                        if all && !complete {
                            dirty = true;
                            complete = true;
                        }
                    }
                    Err(_) => {}
                }
                if dirty
                    && (!published
                        || complete
                        || disconnected
                        || last.elapsed() >= Duration::from_millis(250))
                {
                    let mut raw = to_raw(packets.values().cloned().collect());
                    // Some muxers omit durations. Never fill across an unread seek gap.
                    for i in 0..raw.len() {
                        if raw[i].duration <= 0.0 {
                            raw[i].duration = if i + 1 < raw.len() {
                                (raw[i + 1].pts - raw[i].pts).clamp(0.0, 1.0)
                            } else {
                                1.0
                            };
                        }
                    }
                    let telemetry = if complete {
                        Telemetry::from_gpmf_packets_with(&raw, &options(duration))
                    } else {
                        Telemetry::from_gpmf_packets_progressive(&raw)
                    };
                    let update = match telemetry {
                        Ok(telemetry) => Loaded {
                            telemetry,
                            warning: route_warning.take(),
                        },
                        Err(e) => Loaded {
                            telemetry: Telemetry::empty(duration),
                            warning: Some(format!("telemetry not decoded: {e}")),
                        },
                    };
                    if cancelled.load(Ordering::SeqCst) {
                        return;
                    }
                    *latest.lock().unwrap() = Some(update);
                    wake();
                    dirty = false;
                    published = true;
                    last = Instant::now();
                }
                if disconnected {
                    return;
                }
            }
        })
        .expect("spawn telemetry thread");
    load
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

    fn update(loader: &StreamLoad) -> Loaded {
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(update) = loader.take_update() {
                return update;
            }
            assert!(Instant::now() < end, "no progressive update");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn progressive_packets_show_early_deduplicate_seeks_and_match_full_track() {
        let Some(path) = sample("gopro/hero5.mp4") else {
            return;
        };
        let packets = read_gpmf_packets(&path).unwrap();
        let duration = actionlay_media::probe::probe(&path).unwrap().duration;
        let (tx, rx) = mpsc::channel();
        let loader = spawn(rx, duration, Some(packets.len()), || {});
        let send = |i: usize| {
            tx.send(TelemetryEvent::Packet {
                timestamp: i as i64,
                packet: packets[i].clone(),
            })
            .unwrap()
        };
        send(0);
        let first = update(&loader);
        assert!(first.telemetry.is_loaded_at(0.1));
        assert!(!first.telemetry.is_loaded_at(15.0));
        send(20);
        let skipped = update(&loader);
        assert!(skipped.telemetry.is_loaded_at(packets[20].pts + 0.1));
        assert!(!skipped.telemetry.is_loaded_at(10.0));
        assert_eq!(
            skipped
                .telemetry
                .sample(packets[20].pts + 0.1)
                .get(Metric::Odo),
            Value::Absent
        );
        for i in 0..packets.len() {
            send(i);
            send(i);
        }
        tx.send(TelemetryEvent::End { from_start: false }).unwrap();
        drop(tx);
        // Decoder coalesces updates: allow it to consume all queued packets.
        let end = Instant::now() + Duration::from_secs(3);
        let mut final_update = update(&loader);
        while !final_update.telemetry.is_loaded_at(duration - 0.01) {
            assert!(Instant::now() < end);
            final_update = update(&loader);
        }
        let reference = load(&path, duration).telemetry;
        assert_eq!(final_update.telemetry.gps_points(), reference.gps_points());
        for t in [0.1, 10.0, 20.0, duration - 0.01] {
            assert_eq!(final_update.telemetry.sample(t), reference.sample(t));
        }
    }
    #[test]
    fn replacing_a_waiting_stream_never_blocks_the_next_file() {
        let Some(path) = sample("gopro/hero5.mp4") else {
            return;
        };
        let packets = read_gpmf_packets(&path).unwrap();
        let (_slow, rx) = mpsc::channel();
        let old = spawn(rx, 34.0, Some(34), || {});
        drop(old);
        let (tx, rx) = mpsc::channel();
        let current = spawn(rx, 34.0, Some(34), || {});
        tx.send(TelemetryEvent::Packet {
            timestamp: 0,
            packet: packets[0].clone(),
        })
        .unwrap();
        assert!(update(&current).telemetry.is_loaded_at(0.1));
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
        let loaded = load(&path, duration);
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

/// Other cameras have metadata outside GoPro's demux stream. Read it off the UI thread.
pub fn spawn_camera(
    path: std::path::PathBuf,
    duration: f64,
    wake: impl Fn() + Send + 'static,
) -> StreamLoad {
    let latest = Arc::new(Mutex::new(None));
    let cancelled = Arc::new(AtomicBool::new(false));
    let load = StreamLoad {
        latest: latest.clone(),
        cancelled: cancelled.clone(),
        route_busy: Arc::new(AtomicBool::new(false)),
        route_requests: None,
    };
    std::thread::spawn(move || {
        let result = actionlay_telemetry::camera::read(&path, duration, cancelled.clone());
        if cancelled.load(Ordering::SeqCst) {
            return;
        }
        let update = match result {
            Ok(telemetry) => Loaded {
                telemetry,
                warning: None,
            },
            Err(e) => Loaded {
                telemetry: Telemetry::empty(duration),
                warning: Some(format!(
                    "No usable camera telemetry: {e}. You can link a GPX/FIT file."
                )),
            },
        };
        *latest.lock().unwrap() = Some(update);
        wake();
    });
    load
}

pub fn spawn_activity(
    path: std::path::PathBuf,
    wake: impl Fn() + Send + 'static,
) -> mpsc::Receiver<Result<actionlay_telemetry::external::Activity, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result =
            actionlay_telemetry::external::Activity::read(&path).map_err(|e| e.to_string());
        if tx.send(result).is_ok() {
            wake();
        }
    });
    rx
}

#[cfg(test)]
mod route_tests {
    use super::*;
    #[test]
    fn indexed_backfill_merges_with_playback_packets_without_loading_the_future() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/gopro/hero5.mp4");
        if !path.exists() {
            return;
        }
        let all = read_gpmf_packets(&path).unwrap();
        let timeline = actionlay_media::chapters::Timeline::open(&path, false).unwrap();
        let (tx, rx) = mpsc::channel();
        let load = spawn(rx, timeline.duration, Some(all.len()), || {});
        for packet in all.iter().take(2).chain(all.iter().skip(20).take(2)) {
            tx.send(TelemetryEvent::Packet {
                timestamp: (packet.pts * 1e6).round() as i64,
                packet: packet.clone(),
            })
            .unwrap();
        }
        assert!(load.request_route(timeline.clone(), vec![(0.0, 5.0)]));
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(update) = load.take_update() {
                assert!(update.warning.is_none());
                if update.telemetry.is_loaded_through(5.0) {
                    assert!(!update.telemetry.is_loaded_at(10.0));
                    let reference = load_reference(&all, timeline.duration);
                    assert_eq!(
                        update
                            .telemetry
                            .sample(3.0)
                            .get(actionlay_telemetry::Metric::Lat),
                        reference.sample(3.0).get(actionlay_telemetry::Metric::Lat)
                    );
                    break;
                }
            }
            assert!(Instant::now() < deadline, "route backfill timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(load.request_route(timeline.clone(), vec![(5.0, timeline.duration)]));
        loop {
            if let Some(update) = load.take_update()
                && update.telemetry.is_loaded_through(timeline.duration)
            {
                let reference = load_reference(&all, timeline.duration);
                assert_eq!(update.telemetry.track(), reference.track());
                break;
            }
            assert!(Instant::now() < deadline, "full route timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn load_reference(packets: &[GpmfPacket], duration: f64) -> Telemetry {
        Telemetry::from_gpmf_packets_with(&to_raw(packets.to_vec()), &options(duration)).unwrap()
    }
}
