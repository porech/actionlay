//! Shared app/CLI export pipeline. Bounded decoding and parallel render stages.
use actionlay_layout::{Layout, geom::ScaleMode};
use actionlay_media::{export as media, frame::Nv12Frame};
use actionlay_render::{Renderer, tiny_skia::Pixmap};
use actionlay_telemetry::{RawPacket, Telemetry, TelemetryOptions};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
pub enum Mode {
    #[default]
    Video,
    Transparent,
    Solid,
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
pub enum Format {
    #[default]
    H264,
    H265,
    Prores,
    Png,
}
impl Format {
    pub fn codec(self) -> &'static str {
        match self {
            Self::H264 => "h264",
            Self::H265 => "h265",
            Self::Prores => "prores",
            Self::Png => "png",
        }
    }
    pub fn extension(self) -> &'static str {
        match self {
            Self::H264 | Self::H265 => "mp4",
            Self::Prores => "mov",
            Self::Png => "",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub mode: Mode,
    pub format: Format,
    pub color: [u8; 3],
    pub hardware: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: Mode::Video,
            format: Format::H264,
            color: [0, 255, 0],
            hardware: true,
        }
    }
}
#[derive(Clone)]
pub struct Request {
    pub source: PathBuf,
    pub output: PathBuf,
    pub layout: Arc<Layout>,
    pub settings: Settings,
    pub start: f64,
    pub end: Option<f64>,
    pub scale: ScaleMode,
    pub maps: actionlay_maps::TileStore,
}
#[derive(Debug, Default, Clone)]
pub struct Progress {
    pub fraction: f32,
    pub frames: usize,
    pub elapsed: f64,
    pub remaining: Option<f64>,
    pub status: String,
    pub done: bool,
    pub cancelled: bool,
    pub error: Option<String>,
}
pub struct Job {
    pub cancel: Arc<AtomicBool>,
    pub progress: Arc<Mutex<Progress>>,
}
impl Job {
    pub fn spawn(request: Request, wake: impl Fn() + Send + 'static) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new(Progress::default()));
        let c = cancel.clone();
        let p = progress.clone();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run(request, c, |update| {
                    *p.lock().unwrap() = update;
                    wake();
                })
            }))
            .unwrap_or_else(|_| Err(anyhow::anyhow!("export worker failed")));
            let mut state = p.lock().unwrap();
            state.done = true;
            if let Err(e) = result {
                state.error = Some(format!("{e:#}"));
                state.status = "Export failed".into();
            }
            drop(state);
            wake();
        });
        Self { cancel, progress }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub fn validate(request: &Request, duration: f64) -> Result<f64> {
    let errors: Vec<_> = actionlay_layout::validate::validate(&request.layout)
        .into_iter()
        .filter(|issue| issue.severity == actionlay_layout::validate::Severity::Error)
        .collect();
    if !errors.is_empty() {
        bail!(
            "invalid layout: {}",
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
    if request.end.is_some_and(|end| !end.is_finite()) {
        bail!("out point must be finite");
    }
    let end = request.end.unwrap_or(duration).min(duration);
    if !request.start.is_finite() || !end.is_finite() || request.start < 0.0 || end <= request.start
    {
        bail!("choose a finite range within the source duration");
    }
    if request.output.exists() {
        bail!("destination already exists; choose a new file or directory");
    }
    if request.settings.mode == Mode::Transparent
        && matches!(request.settings.format, Format::H264 | Format::H265)
    {
        bail!("transparency requires ProRes 4444 or PNG");
    }
    Ok(end)
}

pub fn run(
    request: Request,
    cancel: Arc<AtomicBool>,
    mut notify: impl FnMut(Progress),
) -> Result<()> {
    let started = Instant::now();
    let info = actionlay_media::probe::probe(&request.source)?;
    let end = validate(&request, info.duration)?;
    if info.video.width % 2 != 0
        || info.video.height % 2 != 0
        || !info.video.fps.is_finite()
        || info.video.fps <= 0.0
    {
        bail!("export needs even source dimensions and a valid frame rate");
    }
    notify(Progress {
        status: "Reading telemetry…".into(),
        ..Default::default()
    });
    let packets =
        actionlay_media::gpmf::read_gpmf_packets_with_cancel(&request.source, cancel.clone())?;
    let raw: Vec<_> = packets
        .into_iter()
        .map(|p| RawPacket {
            pts: p.pts,
            duration: p.duration,
            data: p.data,
        })
        .collect();
    let telemetry = if raw.is_empty() {
        Telemetry::empty(info.duration)
    } else {
        match Telemetry::from_gpmf_packets_with(
            &raw,
            &TelemetryOptions {
                video_duration: Some(info.duration),
                ..Default::default()
            },
        ) {
            Ok(telemetry) => telemetry,
            Err(error) => {
                log::warn!("export telemetry unavailable: {error}");
                Telemetry::empty(info.duration)
            }
        }
    };
    let telemetry = Arc::new(telemetry);
    let parent = request
        .output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temp = tempfile::Builder::new()
        .prefix(".actionlay-export-")
        .tempdir_in(parent)?;
    let movie = temp
        .path()
        .join(format!("video.{}", request.settings.format.extension()));
    let workers = std::thread::available_parallelism()
        .map_or(2, |n| n.get())
        .clamp(1, 4);
    let (jobs_tx, jobs_rx) = crossbeam_channel::bounded::<(usize, Nv12Frame)>(workers);
    let (results_tx, results_rx) = crossbeam_channel::bounded(workers);
    // Tokens bound the reorder buffer as well as the work queues.
    let (tokens_tx, tokens_rx) = crossbeam_channel::bounded(workers * 2);
    for _ in 0..workers * 2 {
        tokens_tx.send(()).unwrap();
    }
    let mut writer = None;
    let mut frames = 0;
    let mut last_t = request.start;
    let mut first_t = request.start;
    let mut backend = "PNG sequence".to_string();
    let mut timestamps = Vec::new();
    let result: Result<()> = std::thread::scope(|scope| {
        let c = cancel.clone();
        let source = request.source.clone();
        let start = request.start;
        let decoder = scope.spawn(move || {
            let mut index = 0;
            media::decode(&source, start, end, c.clone(), |frame| {
                while !c.load(Ordering::Relaxed) {
                    match tokens_rx.recv_timeout(Duration::from_millis(50)) {
                        Ok(()) => break,
                        Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                        Err(_) => return Ok(()),
                    }
                }
                if c.load(Ordering::Relaxed) {
                    return Ok(());
                }
                if jobs_tx.send((index, frame)).is_err() {
                    return Ok(());
                }
                index += 1;
                Ok(())
            })
        });
        for _ in 0..workers {
            let jobs = jobs_rx.clone();
            let results = results_tx.clone();
            let layout = request.layout.clone();
            let telemetry = telemetry.clone();
            let maps = request.maps.clone();
            let settings = request.settings.clone();
            let scale = request.scale;
            let color = info.video.color;
            scope.spawn(move || {
                let mut renderer = Renderer::new();
                renderer.set_maps(maps);
                renderer.set_scale_mode(scale);
                let mut target =
                    Pixmap::new(info.video.width, info.video.height).expect("probed dimensions");
                while let Ok((index, frame)) = jobs.recv() {
                    let pixels = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        renderer.render_telemetry_into(&layout, &telemetry, frame.pts, &mut target);
                        let background = if settings.mode == Mode::Video {
                            Some(media::rgba(&frame, color))
                        } else {
                            None
                        };
                        composite(target.data(), background.as_deref(), &settings)
                    }))
                    .map_err(|_| anyhow::anyhow!("overlay renderer failed"));
                    if results.send((index, frame.pts, pixels)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(jobs_rx);
        drop(results_tx);
        let mut pending = BTreeMap::new();
        let processing: Result<()> = (|| {
            while let Ok((index, t, pixels)) = results_rx.recv() {
                pending.insert(index, (t, pixels?));
                while let Some((t, pixels)) = pending.remove(&frames) {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    if frames == 0 {
                        first_t = t;
                        if request.settings.format != Format::Png {
                            let created = media::Writer::new(
                                &request.source,
                                &movie,
                                request.settings.format.codec(),
                                request.settings.hardware,
                                t,
                                request.settings.mode == Mode::Video,
                            )?;
                            backend = created.backend.clone();
                            writer = Some(created);
                        }
                    }
                    if let Some(writer) = &mut writer {
                        writer.write(&pixels, info.video.width, info.video.height, t)?;
                    } else {
                        image::save_buffer_with_format(
                            temp.path().join(format!("frame-{:06}.png", frames)),
                            &pixels,
                            info.video.width,
                            info.video.height,
                            image::ColorType::Rgba8,
                            image::ImageFormat::Png,
                        )?;
                    }
                    frames += 1;
                    timestamps.push(t - first_t);
                    last_t = t;
                    let fraction = ((t - request.start + 1.0 / info.video.fps)
                        / (end - request.start))
                        .clamp(0.0, 1.0);
                    let elapsed = started.elapsed().as_secs_f64();
                    notify(Progress {
                        fraction: fraction as f32,
                        frames,
                        elapsed,
                        remaining: Some(elapsed * (1.0 - fraction) / fraction.max(0.001)),
                        status: format!("Exporting · {backend}"),
                        ..Default::default()
                    });
                    let _ = tokens_tx.try_send(());
                }
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
            }
            Ok(())
        })();
        if processing.is_err() {
            cancel.store(true, Ordering::Relaxed);
        }
        drop(results_rx);
        drop(tokens_tx);
        let decoding = decoder
            .join()
            .map_err(|_| anyhow::anyhow!("decoder thread failed"))?;
        processing?;
        decoding?;
        Ok(())
    });
    result?;
    let cancelled = cancel.load(Ordering::Relaxed);
    if frames > 0 {
        if let Some(writer) = writer {
            writer.finish((last_t + 1.0 / info.video.fps).min(end))?;
        }
        if request.settings.format == Format::Png {
            std::fs::write(
                temp.path().join("sequence.json"),
                serde_json::to_vec_pretty(
                    &serde_json::json!({"frames":frames,"fps":info.video.fps,"source_start":first_t,"timestamps":timestamps,"cancelled":cancelled}),
                )?,
            )?;
            if request.output.exists() {
                bail!("destination appeared during export");
            }
            std::fs::rename(temp.path(), &request.output)?;
        } else {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&request.output)?;
            drop(file);
            if let Err(e) = std::fs::rename(&movie, &request.output) {
                let _ = std::fs::remove_file(&request.output);
                return Err(e.into());
            }
        }
    } else if !cancelled {
        bail!("the selected range contains no video frames");
    }
    notify(Progress {
        fraction: if cancelled {
            ((last_t - request.start) / (end - request.start)) as f32
        } else {
            1.0
        },
        frames,
        elapsed: started.elapsed().as_secs_f64(),
        status: if cancelled {
            format!("Cancelled; completed frames retained · {backend}")
        } else {
            format!("Export complete · {backend}")
        },
        done: true,
        cancelled,
        ..Default::default()
    });
    Ok(())
}

/// Renderer pixels are premultiplied; PNG and ProRes require straight alpha.
pub fn composite(overlay: &[u8], background: Option<&[u8]>, settings: &Settings) -> Vec<u8> {
    let mut result = Vec::with_capacity(overlay.len());
    for (i, p) in overlay.as_chunks::<4>().0.iter().enumerate() {
        if settings.mode == Mode::Transparent {
            for &c in &p[..3] {
                result.push(if p[3] == 0 {
                    0
                } else {
                    ((u16::from(c) * 255 + u16::from(p[3]) / 2) / u16::from(p[3])).min(255) as u8
                });
            }
            result.push(p[3]);
        } else {
            let bg = background.map_or(settings.color.as_slice(), |b| &b[i * 4..i * 4 + 3]);
            for c in 0..3 {
                result.push(
                    (u16::from(p[c]) + (u16::from(bg[c]) * (255 - u16::from(p[3])) + 127) / 255)
                        .min(255) as u8,
                );
            }
            result.push(255);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(output: PathBuf) -> Request {
        Request {
            source: Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/export-source.mp4"),
            output,
            layout: Arc::new(crate::editor::Editor::blank()),
            settings: Settings {
                hardware: false,
                ..Default::default()
            },
            start: 0.4,
            end: Some(0.7),
            scale: ScaleMode::Height,
            maps: actionlay_maps::TileStore::offline(),
        }
    }
    #[test]
    fn ranges_and_existing_destinations_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut req = request(dir.path().join("export.mp4"));
        req.end = Some(f64::NAN);
        assert!(validate(&req, 3.0).is_err());
        req.end = Some(0.2);
        assert!(validate(&req, 3.0).is_err());
        req.end = Some(0.7);
        req.settings.mode = Mode::Transparent;
        assert!(validate(&req, 3.0).is_err());
        req.settings.mode = Mode::Video;
        std::fs::write(&req.output, b"original").unwrap();
        assert!(validate(&req, 3.0).is_err());
        assert_eq!(std::fs::read(&req.output).unwrap(), b"original");
        req.output = req.source.clone();
        assert!(validate(&req, 3.0).is_err());
    }
    #[test]
    fn encoders_preserve_range_dimensions_and_audio() {
        let dir = tempfile::tempdir().unwrap();
        for format in [Format::H264, Format::H265] {
            let mut req = request(dir.path().join(format!("{}.mp4", format.codec())));
            req.settings.format = format;
            let output = req.output.clone();
            let mut final_state = Progress::default();
            run(req, Arc::new(AtomicBool::new(false)), |p| final_state = p).unwrap();
            assert_eq!(final_state.frames, 3);
            assert!(final_state.done);
            let probe = actionlay_media::probe::probe(&output).unwrap();
            assert_eq!((probe.video.width, probe.video.height), (160, 90));
            assert!(probe.audio.is_some());
            assert!((probe.duration - 0.3).abs() < 0.05, "{probe:?}");
            let mut times = Vec::new();
            media::decode(&output, 0.0, 1.0, Arc::new(AtomicBool::new(false)), |f| {
                times.push(f.pts);
                Ok(())
            })
            .unwrap();
            assert_eq!(times.len(), 3);
            assert!(times[0].abs() < 1e-6);
            assert!((times[2] - 0.2).abs() < 1e-6);
        }
    }
    #[test]
    fn png_alpha_matches_renderer_and_solid_background_is_green() {
        let dir = tempfile::tempdir().unwrap();
        for mode in [Mode::Transparent, Mode::Solid] {
            let mut req = request(dir.path().join(format!("{mode:?}")));
            req.settings.format = Format::Png;
            req.settings.mode = mode;
            let output = req.output.clone();
            run(req, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
            let image = image::open(output.join("frame-000000.png"))
                .unwrap()
                .to_rgba8();
            assert!(image.pixels().all(|p| p.0
                == if mode == Mode::Transparent {
                    [0, 0, 0, 0]
                } else {
                    [0, 255, 0, 255]
                }));
            let manifest: serde_json::Value =
                serde_json::from_slice(&std::fs::read(output.join("sequence.json")).unwrap())
                    .unwrap();
            assert_eq!(manifest["frames"], 3);
            assert_eq!(manifest["timestamps"].as_array().unwrap().len(), 3);
        }
    }
    #[test]
    fn cancellation_finalizes_a_playable_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let mut req = request(dir.path().join("partial.mp4"));
        req.start = 0.0;
        req.end = Some(3.0);
        let output = req.output.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let c = cancel.clone();
        let mut state = Progress::default();
        run(req, cancel, |p| {
            if p.frames >= 3 {
                c.store(true, Ordering::Relaxed);
            }
            state = p;
        })
        .unwrap();
        assert!(state.cancelled);
        assert_eq!(state.frames, 3);
        let probe = actionlay_media::probe::probe(&output).unwrap();
        assert!(probe.duration > 0.2 && probe.duration < 0.5);
        assert!(probe.audio.is_some());
    }
    #[test]
    fn png_pixels_match_preview_with_partial_opacity() {
        let dir = tempfile::tempdir().unwrap();
        let mut req = request(dir.path().join("frames"));
        req.layout=Arc::new(Layout::from_json(r##"{"version":1,"nodes":[{"type":"frame","size":[960,1080],"fill":"#ff0000","opacity":0.5}]}"##).unwrap().layout);
        req.settings.mode = Mode::Transparent;
        req.settings.format = Format::Png;
        let output = req.output.clone();
        let layout = req.layout.clone();
        run(req, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        let mut renderer = Renderer::new();
        let mut target = Pixmap::new(160, 90).unwrap();
        renderer.render_telemetry_into(&layout, &Telemetry::empty(3.0), 0.4, &mut target);
        let expected = composite(
            target.data(),
            None,
            &Settings {
                mode: Mode::Transparent,
                ..Default::default()
            },
        );
        let actual = image::open(output.join("frame-000000.png"))
            .unwrap()
            .to_rgba8();
        assert_eq!(actual.as_raw(), &expected);
        assert!(actual.pixels().any(|p| p[3] > 0 && p[3] < 255));
        assert!(actual.pixels().any(|p| p[3] == 0));
    }
    #[test]
    fn prores_overlay_is_a_silent_playable_video() {
        let dir = tempfile::tempdir().unwrap();
        let mut req = request(dir.path().join("overlay.mov"));
        req.settings.mode = Mode::Transparent;
        req.settings.format = Format::Prores;
        let output = req.output.clone();
        run(req, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        let info = actionlay_media::probe::probe(&output).unwrap();
        assert_eq!(info.video.codec, "prores");
        assert!(info.audio.is_none());
        assert!((info.video.fps - 10.0).abs() < 1e-6);
        let mut frames = 0;
        media::decode(&output, 0.0, 1.0, Arc::new(AtomicBool::new(false)), |_| {
            frames += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(frames, 3);
    }
    #[test]
    fn alpha_and_green_background_are_correct() {
        let overlay = [64, 0, 0, 128, 0, 0, 0, 0];
        let mut settings = Settings {
            mode: Mode::Transparent,
            ..Default::default()
        };
        assert_eq!(
            composite(&overlay, None, &settings),
            [128, 0, 0, 128, 0, 0, 0, 0]
        );
        settings.mode = Mode::Solid;
        assert_eq!(
            composite(&overlay, None, &settings),
            [64, 127, 0, 255, 0, 255, 0, 255]
        );
    }
}
