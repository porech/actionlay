//! ActionLay: plays a video with its telemetry overlay.
mod layouts;
mod overlay;
mod prefs;
mod telemetry_load;
mod transport;
mod video_view;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use actionlay_layout::Layout;
use actionlay_layout::geom::ScaleMode;
use actionlay_media::player::{Player, PlayerOptions};
use actionlay_telemetry::Telemetry;
use eframe::egui;
use overlay::{OverlayKey, OverlayRequest, OverlayWorker, Scheduler};
use video_view::VideoView;

struct App {
    player: Option<Player>,
    view: VideoView,
    error: Option<String>,
    /// Non-blocking messages: about the layout (kept across videos), about the current
    /// video's telemetry (cleared when another video is opened), about overlay renders.
    layout_notice: Option<String>,
    video_notice: Option<String>,
    failure_notice: Option<String>,
    scrub: transport::ScrubState,
    overlay: OverlayWorker,
    scheduler: Scheduler,
    layout: Arc<Layout>,
    /// Scale mode of the layout on the current video (spec §4.2).
    scale_mode: ScaleMode,
    overlay_visible: bool,
    /// An overlay for the current video has reached the GPU.
    overlay_ready: bool,
    overlay_ms: Option<f32>,
    /// Render failures already reported in `failure_notice`.
    failures_seen: u64,
    layout_rev: u64,
    telemetry_rev: u64,
    /// The only live telemetry load: the one of the current video.
    telemetry_rx: Option<Receiver<telemetry_load::Loaded>>,
    /// pts of the current video's frame on screen (None before its first frame): the
    /// overlay is rendered for this time.
    shown_t: Option<f64>,
    prefs: prefs::Prefs,
    prefs_path: Option<PathBuf>,
    max_texture: u32,
    /// Overlay size last requested, logged when it changes.
    logged_overlay_size: Option<(u32, u32)>,
}

impl App {
    fn open(&mut self, path: PathBuf) {
        match Player::open(&path, PlayerOptions::default()) {
            Ok(p) => {
                let info = p.info();
                let duration = info.duration;
                self.scale_mode =
                    layouts::scale_mode_for(info.video.width, info.video.height, &self.layout);
                log::info!(
                    "{}: {}x{} video, overlay scale mode {:?}",
                    path.display(),
                    info.video.width,
                    info.video.height,
                    self.scale_mode
                );
                self.player = Some(p);
                self.error = None;
                // the previous video's telemetry, notices and overlay are gone for good:
                // its loader's result (if any) is discarded with the receiver
                self.telemetry_rx = None;
                self.video_notice = None;
                self.shown_t = None;
                self.view.clear_overlay();
                self.overlay_ready = false;
                self.overlay_ms = None;
                self.scheduler.reset();
                self.overlay.set_telemetry(None, self.scale_mode);
                self.telemetry_rev += 1;
                self.telemetry_rx = Some(telemetry_load::spawn(path, duration));
            }
            Err(e) => self.error = Some(format!("{}: {e}", path.display())),
        }
    }

    fn open_layout(&mut self, path: PathBuf) {
        // remembered as an absolute path: the app may be started from anywhere
        let path = std::path::absolute(&path).unwrap_or(path);
        match layouts::load(&path) {
            Ok((layout, notice)) => {
                self.set_layout(Arc::new(layout));
                self.layout_notice = notice;
                self.error = None;
                self.prefs.last_layout = Some(path);
                self.save_prefs();
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn save_prefs(&self) {
        if let Some(p) = &self.prefs_path
            && let Err(e) = self.prefs.save(p)
        {
            log::warn!("cannot save preferences to {}: {e}", p.display());
        }
    }

    fn set_layout(&mut self, layout: Arc<Layout>) {
        self.scale_mode = self.player.as_ref().map_or(ScaleMode::Height, |p| {
            let v = &p.info().video;
            layouts::scale_mode_for(v.width, v.height, &layout)
        });
        log::info!("layout changed, overlay scale mode {:?}", self.scale_mode);
        self.overlay.set_layout(layout.clone(), self.scale_mode);
        self.layout = layout;
        self.layout_rev += 1;
    }

    fn poll_telemetry(&mut self) {
        let Some(rx) = &self.telemetry_rx else { return };
        let loaded = match rx.try_recv() {
            Ok(loaded) => loaded,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                // the loader panicked: show the empty states rather than nothing
                let duration = self.player.as_ref().map_or(0.0, |p| p.info().duration);
                telemetry_load::Loaded {
                    telemetry: Telemetry::empty(duration),
                    warning: Some("telemetry loader crashed".into()),
                }
            }
        };
        if let Some(w) = loaded.warning {
            log::warn!("{w}");
            self.video_notice = Some(w);
        }
        self.overlay
            .set_telemetry(Some(Arc::new(loaded.telemetry)), self.scale_mode);
        self.telemetry_rev += 1;
        self.telemetry_rx = None;
    }

    fn poll_overlay(&mut self) {
        if let Some(frame) = self.overlay.take_frame() {
            log::debug!(
                "overlay frame t={:.3} {}x{} rendered in {:.2} ms",
                frame.t,
                frame.pixmap.width(),
                frame.pixmap.height(),
                frame.render_ms
            );
            self.overlay_ms = Some(frame.render_ms);
            self.view.upload_overlay(frame.pixmap);
            self.overlay_ready = true;
        }
        for pixmap in self.view.take_recycled() {
            self.overlay.recycle(pixmap);
        }
        let failures = self.overlay.failures();
        if failures > self.failures_seen {
            self.failures_seen = failures;
            self.failure_notice = Some(format!(
                "overlay rendering failed {failures} time(s), see the log"
            ));
        }
    }

    /// Asks the worker for the overlay of the frame on screen at the on-screen size.
    fn request_overlay(&mut self, ctx: &egui::Context, video: egui::Rect) {
        let (Some(p), Some(t)) = (&self.player, self.shown_t) else {
            return;
        };
        if !self.overlay_visible || !t.is_finite() {
            return;
        }
        let Some((width, height)) = overlay::overlay_size(
            video.width(),
            video.height(),
            ctx.pixels_per_point(),
            self.max_texture,
        ) else {
            return;
        };
        let key = OverlayKey {
            t,
            width,
            height,
            layout_rev: self.layout_rev,
            telemetry_rev: self.telemetry_rev,
        };
        let now = Instant::now();
        if self.scheduler.should_render(now, key, !p.is_paused()) {
            if self.logged_overlay_size != Some((width, height)) {
                self.logged_overlay_size = Some((width, height));
                log::info!(
                    "overlay size {width}x{height} px (video rect {:.2}x{:.2} pt at {} px/pt)",
                    video.width(),
                    video.height(),
                    ctx.pixels_per_point()
                );
            }
            self.overlay.request(OverlayRequest { t, width, height });
        } else if let Some(at) = self.scheduler.retry_at() {
            // e.g. the final size of a resize while paused
            ctx.request_repaint_after(at.saturating_duration_since(now));
        }
    }

    fn overlay_status(&self) -> String {
        match (self.overlay_visible, self.overlay_ms) {
            (false, _) => "overlay off (O)".into(),
            (true, Some(ms)) => format!("overlay {ms:.1} ms"),
            (true, None) => "overlay …".into(),
        }
    }

    fn notices(&self) -> Option<String> {
        let all: Vec<&str> = [
            &self.layout_notice,
            &self.video_notice,
            &self.failure_notice,
        ]
        .into_iter()
        .filter_map(|n| n.as_deref())
        .collect();
        (!all.is_empty()).then(|| all.join(" · "))
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let dropped: Vec<PathBuf> = ui.ctx().input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        for path in dropped {
            match layouts::classify(path) {
                layouts::Dropped::Layout(p) => self.open_layout(p),
                layouts::Dropped::Video(p) => self.open(p),
            }
        }
        self.poll_telemetry();

        if !ui.ctx().egui_wants_keyboard_input() && ui.ctx().input(|i| i.key_pressed(egui::Key::O))
        {
            self.overlay_visible = !self.overlay_visible;
        }

        if let Some(p) = &mut self.player {
            transport::handle_keys(ui.ctx(), p, &self.scrub);
            if let Some(frame) = p.poll_frame() {
                self.shown_t = Some(frame.pts);
                let color = p.info().video.color;
                self.view.upload(frame, color);
            }
        }
        self.poll_overlay();

        let status = self.overlay_status();
        let notices = self.notices();
        egui::Panel::bottom("transport").show(ui, |ui| {
            if let Some(p) = &mut self.player {
                transport::show(ui, p, &mut self.scrub, &status);
            }
            if let Some(e) = &self.error {
                ui.colored_label(egui::Color32::LIGHT_RED, e);
            }
            if let Some(n) = &notices {
                ui.small(n);
            }
            if self.player.is_none() && self.error.is_none() {
                ui.label(
                    "Drop a video (or a .ovl.json layout) here, or pass a video on the command line.",
                );
            }
        });
        egui::CentralPanel::default().show(ui, |ui| {
            let rect = ui.available_rect_before_wrap();
            // snapped once: the paint viewport and the overlay texture share its size
            let video = self.view.video_rect(rect, ui.ctx().pixels_per_point());
            if let Some(video) = video {
                self.request_overlay(ui.ctx(), video);
            }
            self.view
                .show(ui, rect, video, self.overlay_visible && self.overlay_ready);
        });

        if let Some(p) = &self.player {
            if !p.is_paused() {
                ui.ctx().request_repaint();
            } else if p.is_awaiting_frame() || self.telemetry_rx.is_some() {
                // a frame (open/seek/step) or the telemetry is still on its way;
                // finished overlays request a repaint themselves
                ui.ctx().request_repaint_after(Duration::from_millis(16));
            }
        }
    }
}

fn main() -> eframe::Result {
    env_logger::init();
    let path = std::env::args().nth(1).map(PathBuf::from);
    eframe::run_native(
        "ActionLay",
        eframe::NativeOptions::default(),
        Box::new(move |cc| {
            let rs = cc
                .wgpu_render_state
                .as_ref()
                .expect("wgpu renderer required");
            let max_texture = rs.device.limits().max_texture_dimension_2d;
            let prefs_path = prefs::default_path();
            let prefs = prefs_path
                .as_deref()
                .map(prefs::Prefs::load)
                .unwrap_or_default();
            let initial = layouts::initial(&prefs);
            let layout = Arc::new(initial.layout);
            let ctx = cc.egui_ctx.clone();
            let overlay = OverlayWorker::spawn(layout.clone(), move || ctx.request_repaint());
            let mut app = App {
                player: None,
                view: VideoView::new(rs),
                error: None,
                layout_notice: initial.notice,
                video_notice: None,
                failure_notice: None,
                scrub: Default::default(),
                overlay,
                scheduler: Scheduler::default(),
                layout,
                scale_mode: ScaleMode::Height,
                overlay_visible: true,
                overlay_ready: false,
                overlay_ms: None,
                failures_seen: 0,
                layout_rev: 0,
                telemetry_rev: 0,
                telemetry_rx: None,
                shown_t: None,
                prefs,
                prefs_path,
                max_texture,
                logged_overlay_size: None,
            };
            if initial.fallback {
                // the notice says it once; do not repeat it at every launch
                app.prefs.last_layout = None;
                app.save_prefs();
            }
            if let Some(path) = path {
                app.open(path);
            }
            Ok(Box::new(app))
        }),
    )
}
