//! GoPro chapter discovery and a single playback timeline over the original files.
use crate::{
    MediaError,
    frame::Nv12Frame,
    player::{Player, PlayerOptions, PlayerStats, TelemetryEvent},
    probe::{MediaInfo, probe},
};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
struct Name {
    family: String,
    recording: String,
    chapter: u32,
}
fn name(path: &Path) -> Option<Name> {
    if !path.extension()?.eq_ignore_ascii_case("mp4") {
        return None;
    }
    let stem = path.file_stem()?.to_str()?.to_ascii_uppercase();
    let b = stem.as_bytes();
    if b.len() != 8 {
        return None;
    }
    if (&b[..2] == b"GX" || &b[..2] == b"GH") && b[2..].iter().all(u8::is_ascii_digit) {
        let chapter = stem[2..4].parse().ok()?;
        if chapter == 0 {
            return None;
        }
        return Some(Name {
            family: stem[..2].into(),
            recording: stem[4..].into(),
            chapter,
        });
    }
    if &b[..4] == b"GOPR" && b[4..].iter().all(u8::is_ascii_digit) {
        return Some(Name {
            family: "legacy".into(),
            recording: stem[4..].into(),
            chapter: 0,
        });
    }
    if b[0] == b'G' && b[1..].iter().all(u8::is_ascii_digit) {
        return Some(Name {
            family: "legacy".into(),
            recording: stem[4..].into(),
            chapter: stem[1..4].parse().ok()?,
        });
    }
    None
}

/// Automatic joining starts only from the recording's first chapter.
/// Intermediate chapters remain standalone until the user requests the sequence.
pub fn is_first_chapter(path: &Path) -> bool {
    name(path).is_some_and(|n| n.chapter == if n.family == "legacy" { 0 } else { 1 })
}

/// Only a contiguous recording beginning with its first chapter is auto-joined.
/// Missing chapters, duplicate names and unrelated clips stay separate.
pub fn discover(path: &Path) -> Vec<PathBuf> {
    let one = || vec![path.to_path_buf()];
    let Some(target) = name(path) else {
        return one();
    };
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let Ok(entries) = std::fs::read_dir(parent) else {
        return one();
    };
    let mut found: Vec<_> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let p = e.path();
            let n = name(&p)?;
            (p.is_file() && n.family == target.family && n.recording == target.recording)
                .then_some((n.chapter, p))
        })
        .collect();
    found.sort_by_key(|p| p.0);
    let first = if target.family == "legacy" { 0 } else { 1 };
    if found.first().is_none_or(|p| p.0 != first) || found.windows(2).any(|w| w[1].0 != w[0].0 + 1)
    {
        return one();
    }
    found.into_iter().map(|p| p.1).collect()
}

#[derive(Clone)]
pub struct Chapter {
    pub path: PathBuf,
    pub start: f64,
    pub info: MediaInfo,
}
#[derive(Clone)]
pub struct Timeline {
    pub chapters: Vec<Chapter>,
    pub duration: f64,
}
impl Timeline {
    pub fn open(path: &Path, join: bool) -> Result<Self, MediaError> {
        let paths = if join {
            discover(path)
        } else {
            vec![path.to_path_buf()]
        };
        Self::from_paths(paths, None)
    }
    fn from_paths(paths: Vec<PathBuf>, mut first: Option<MediaInfo>) -> Result<Self, MediaError> {
        let mut chapters: Vec<Chapter> = Vec::new();
        let mut duration = 0.0;
        for path in paths {
            let info = match first.take() {
                Some(info) => info,
                None => probe(&path)?,
            };
            if !info.duration.is_finite() || info.duration <= 0.0 {
                return Err(MediaError::Io("chapter has no valid duration".into()));
            }
            if let Some(first) = chapters.first() {
                let v = &first.info.video;
                if info.video.codec != v.codec
                    || info.video.width != v.width
                    || info.video.height != v.height
                    || (info.video.fps - v.fps).abs() > 0.01
                {
                    return Err(MediaError::Io(
                        "GoPro chapters have incompatible video formats; open the file alone"
                            .into(),
                    ));
                }
            }
            chapters.push(Chapter {
                path,
                start: duration,
                info: info.clone(),
            });
            duration += info.duration;
        }
        Ok(Self { chapters, duration })
    }
    pub fn locate(&self, t: f64) -> (usize, f64) {
        let t = t.clamp(0.0, self.duration);
        let index = self
            .chapters
            .partition_point(|c| c.start <= t)
            .saturating_sub(1);
        (index, t - self.chapters[index].start)
    }
}

/// The app transport sees a global timeline; each decoder retains local file time.
pub struct ChapterPlayer {
    timeline: Timeline,
    current: usize,
    player: Player,
    info: MediaInfo,
    options: PlayerOptions,
    device: Option<String>,
    volume: std::cell::Cell<f32>,
    error: Option<String>,
    metadata_rx: Option<std::sync::mpsc::Receiver<TelemetryEvent>>,
    metadata_tx: std::sync::mpsc::Sender<TelemetryEvent>,
}
impl ChapterPlayer {
    pub fn open_with_audio_device(
        path: &Path,
        options: PlayerOptions,
        device: Option<&str>,
    ) -> Result<Self, MediaError> {
        Self::open_mode(path, options, device, is_first_chapter(path))
    }
    pub fn open_mode(
        path: &Path,
        options: PlayerOptions,
        device: Option<&str>,
        join: bool,
    ) -> Result<Self, MediaError> {
        let paths = if join {
            discover(path)
        } else {
            vec![path.to_path_buf()]
        };
        let player = Player::open_with_audio_device(&paths[0], options, device)?;
        let timeline = Timeline::from_paths(paths, Some(player.info().clone()))?;
        let mut info = player.info().clone();
        info.duration = timeline.duration;
        if let Some(meta) = &mut info.telemetry {
            meta.packet_count = timeline.chapters.iter().try_fold(0usize, |n, c| {
                Some(n + c.info.telemetry.as_ref()?.packet_count?)
            });
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let mut out = Self {
            timeline,
            current: 0,
            player,
            info,
            options,
            device: device.map(str::to_owned),
            volume: std::cell::Cell::new(1.0),
            error: None,
            metadata_rx: Some(rx),
            metadata_tx: tx,
        };
        out.relay();
        Ok(out)
    }
    fn relay(&mut self) {
        let Some(rx) = self.player.take_telemetry() else {
            return;
        };
        let tx = self.metadata_tx.clone();
        let offset = self.timeline.chapters[self.current].start;
        let single = self.timeline.chapters.len() == 1;
        std::thread::spawn(move || {
            while let Ok(event) = rx.recv() {
                let event = match event {
                    TelemetryEvent::Packet { mut packet, .. } => {
                        packet.pts += offset;
                        TelemetryEvent::Packet {
                            timestamp: (packet.pts * 1e6).round() as i64,
                            packet,
                        }
                    }
                    TelemetryEvent::End { from_start } => TelemetryEvent::End {
                        from_start: single && from_start,
                    },
                };
                if tx.send(event).is_err() {
                    return;
                }
            }
        });
    }
    fn switch(&mut self, index: usize, t: f64, precise: bool) -> Result<(), MediaError> {
        let playing = !self.player.is_paused();
        let speed = self.player.speed();
        let mut p = Player::open_with_audio_device(
            &self.timeline.chapters[index].path,
            self.options,
            self.device.as_deref(),
        )?;
        p.set_speed(speed);
        p.set_volume(self.volume.get());
        p.seek(t, precise);
        if playing {
            p.play();
        }
        self.player = p;
        self.current = index;
        self.relay();
        Ok(())
    }
    pub fn timeline(&self) -> &Timeline {
        &self.timeline
    }
    pub fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }
    pub fn info(&self) -> &MediaInfo {
        &self.info
    }
    pub fn take_telemetry(&mut self) -> Option<std::sync::mpsc::Receiver<TelemetryEvent>> {
        self.metadata_rx.take()
    }
    pub fn is_paused(&self) -> bool {
        self.player.is_paused()
    }
    pub fn is_buffering(&self) -> bool {
        self.player.is_buffering()
    }
    pub fn buffered_seconds(&self) -> f64 {
        self.player.buffered_seconds()
    }
    pub fn is_awaiting_frame(&self) -> bool {
        self.player.is_awaiting_frame()
    }
    pub fn play(&mut self) {
        if self.at_end() {
            self.seek(0.0, true);
        }
        self.player.play();
    }
    pub fn pause(&mut self) {
        self.player.pause();
    }
    pub fn toggle(&mut self) {
        if self.is_paused() {
            self.play()
        } else {
            self.pause()
        }
    }
    pub fn change_audio_device(&mut self, device: Option<&str>) -> Result<bool, MediaError> {
        let changed = self.player.change_audio_device(device)?;
        self.device = device.map(str::to_owned);
        Ok(changed)
    }
    pub fn set_volume(&self, volume: f32) {
        self.volume.set(volume);
        self.player.set_volume(volume);
    }
    pub fn speed(&self) -> f64 {
        self.player.speed()
    }
    pub fn set_speed(&mut self, speed: f64) {
        self.player.set_speed(speed);
    }
    pub fn seek(&mut self, to: f64, precise: bool) {
        let (index, t) = self.timeline.locate(to);
        if index == self.current {
            self.player.seek(t, precise);
        } else if let Err(e) = self.switch(index, t, precise) {
            self.error = Some(e.to_string());
            self.pause();
        }
    }
    pub fn step(&mut self, frames: i32) {
        let to = self.position() + f64::from(frames) / self.info.video.fps.max(1.0);
        let (i, _) = self.timeline.locate(to);
        if i == self.current {
            self.player.step(frames);
        } else {
            self.pause();
            self.seek(to, true);
        }
    }
    pub fn position(&self) -> f64 {
        self.timeline.chapters[self.current].start + self.player.position()
    }
    pub fn at_end(&self) -> bool {
        self.current + 1 == self.timeline.chapters.len() && self.player.at_end()
    }
    pub fn stats(&self) -> PlayerStats {
        self.player.stats()
    }
    pub fn poll_frame(&mut self) -> Option<Nv12Frame> {
        let playing = !self.player.is_paused();
        let offset = self.timeline.chapters[self.current].start;
        let frame = self.player.poll_frame().map(|mut f| {
            f.pts += offset;
            f
        });
        if self.player.at_end() && self.current + 1 < self.timeline.chapters.len() && playing {
            match self.switch(self.current + 1, 0.0, true) {
                Ok(()) => self.player.play(),
                Err(e) => {
                    self.error = Some(e.to_string());
                    self.pause();
                }
            }
        }
        frame
    }
}
