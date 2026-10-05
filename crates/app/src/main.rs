//! ActionLay M0 prototype: plays a video with hardware decoding and synced audio.
// Wired into the UI in Task 12.
#[allow(dead_code)]
mod layouts;
#[allow(dead_code)]
mod overlay;
#[allow(dead_code)]
mod prefs;
#[allow(dead_code)]
mod telemetry_load;
mod transport;
mod video_view;

use std::path::PathBuf;

use actionlay_media::player::{Player, PlayerOptions};
use eframe::egui;
use video_view::VideoView;

struct App {
    player: Option<Player>,
    view: VideoView,
    error: Option<String>,
    scrub: transport::ScrubState,
}

impl App {
    fn open(&mut self, path: PathBuf) {
        match Player::open(&path, PlayerOptions::default()) {
            Ok(p) => {
                self.player = Some(p);
                self.error = None;
            }
            Err(e) => self.error = Some(format!("{}: {e}", path.display())),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let dropped = ui
            .ctx()
            .input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()));
        if let Some(path) = dropped {
            self.open(path);
        }

        if let Some(p) = &mut self.player {
            transport::handle_keys(ui.ctx(), p, &self.scrub);
            if let Some(frame) = p.poll_frame() {
                let color = p.info().video.color;
                self.view.upload(frame, color);
            }
        }

        egui::Panel::bottom("transport").show(ui, |ui| {
            if let Some(p) = &mut self.player {
                transport::show(ui, p, &mut self.scrub);
            } else {
                ui.label(
                    self.error
                        .as_deref()
                        .unwrap_or("Drop a video here or pass it on the command line."),
                );
            }
        });
        egui::CentralPanel::default().show(ui, |ui| {
            let rect = ui.available_rect_before_wrap();
            self.view.show(ui, rect);
        });

        // keep repainting while playing; when paused, egui repaints on input
        if let Some(p) = &self.player {
            if !p.is_paused() {
                ui.ctx().request_repaint();
            } else if p.is_awaiting_frame() {
                // paused but a frame (open/seek/step) is still being decoded
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(16));
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
            let mut app = App {
                player: None,
                view: VideoView::new(rs),
                error: None,
                scrub: Default::default(),
            };
            if let Some(path) = path {
                app.open(path);
            }
            Ok(Box::new(app))
        }),
    )
}
