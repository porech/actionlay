use crate::export::{self, Format, Mode, Request, Settings};
use clap::Parser;
use eframe::egui;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub struct Dialog {
    pub settings: Settings,
    start: f64,
    end: f64,
    duration: f64,
    error: Option<String>,
}
impl Dialog {
    pub fn new(mut settings: Settings, duration: f64) -> Self {
        settings.mode = Mode::Video;
        Self {
            settings,
            start: 0.0,
            end: duration,
            duration,
            error: None,
        }
    }
    pub fn show(&mut self, ctx: &egui::Context) -> (bool, Option<PathBuf>) {
        let mut open = true;
        let mut choose = false;
        egui::Window::new(crate::i18n::text("Export video / overlay")).open(&mut open).resizable(false).show(ctx,|ui| {
            egui::ComboBox::from_id_salt("export-mode").selected_text(crate::i18n::ui_text(ui, match self.settings.mode { Mode::Video=>"Video with overlay",Mode::Transparent=>"Overlay · transparent",Mode::Solid=>"Overlay · solid background" })).show_ui(ui,|ui| {
                ui.selectable_value(&mut self.settings.mode,Mode::Video,crate::i18n::ui_text(ui, "Video with overlay"));
                ui.selectable_value(&mut self.settings.mode,Mode::Solid,crate::i18n::ui_text(ui, "Overlay · solid background"));
                ui.selectable_value(&mut self.settings.mode,Mode::Transparent,crate::i18n::ui_text(ui, "Overlay · transparent"));
            });
            if self.settings.mode==Mode::Transparent && matches!(self.settings.format,Format::H264|Format::H265) { self.settings.format=Format::Prores; }
            if self.settings.mode==Mode::Solid { ui.horizontal(|ui| { ui.label(crate::i18n::ui_text(ui, "Background")); ui.color_edit_button_srgb(&mut self.settings.color); if ui.button(crate::i18n::ui_text(ui, "Green screen")).clicked() {self.settings.color=[0,255,0];} }); }
            egui::ComboBox::from_id_salt("export-format").selected_text(crate::i18n::ui_text(ui, self.settings.format.codec())).show_ui(ui,|ui| {
                if self.settings.mode!=Mode::Transparent { ui.selectable_value(&mut self.settings.format,Format::H264,crate::i18n::ui_text(ui, "H.264 · MP4")); ui.selectable_value(&mut self.settings.format,Format::H265,crate::i18n::ui_text(ui, "H.265 · MP4")); }
                ui.selectable_value(&mut self.settings.format,Format::Prores,crate::i18n::ui_text(ui, "ProRes 4444 · MOV"));
                ui.selectable_value(&mut self.settings.format,Format::Png,crate::i18n::ui_text(ui, "PNG sequence"));
            });
            ui.checkbox(&mut self.settings.hardware,crate::i18n::ui_text(ui, "Prefer hardware encoder"));
            ui.horizontal(|ui| { ui.label(crate::i18n::ui_text(ui, "In (seconds)")); ui.add(egui::DragValue::new(&mut self.start).speed(0.1).range(0.0..=self.duration)); });
            ui.horizontal(|ui| { ui.label(crate::i18n::ui_text(ui, "Out (seconds)")); ui.add(egui::DragValue::new(&mut self.end).speed(0.1).range(0.0..=self.duration)); });
            ui.label(crate::i18n::ui_text(ui, "Original resolution and frame timestamps. Video-file audio is copied; overlay-only and PNG exports are silent."));
            if let Some(error)=&self.error {ui.colored_label(egui::Color32::RED,crate::i18n::ui_text(ui, error));}
            if ui.button(crate::i18n::ui_text(ui, "Choose destination and export…")).clicked() {
                if self.end>self.start {choose=true;} else {self.error=Some("Out must be after In".into());}
            }
        });
        let output = if choose {
            let dialog = rfd::FileDialog::new()
                .set_title(crate::i18n::native_text("New export destination"));
            if self.settings.format == Format::Png {
                dialog.set_file_name("overlay-frames").save_file()
            } else {
                dialog
                    .add_filter(
                        crate::i18n::native_text("Export"),
                        &[self.settings.format.extension()],
                    )
                    .set_file_name(format!("export.{}", self.settings.format.extension()))
                    .save_file()
            }
        } else {
            None
        };
        if let Some(path) = &output
            && path.exists()
        {
            self.error = Some("Choose a destination that does not already exist".into());
            return (open, None);
        }
        (open, output)
    }
}

impl crate::App {
    pub fn show_export(&mut self, ctx: &egui::Context) {
        if let Some(dialog) = &mut self.export_dialog {
            let (open, output) = dialog.show(ctx);
            if let Some(output) = output {
                let settings = dialog.settings.clone();
                let start = dialog.start;
                let end = dialog.end;
                if self.editor.as_ref().is_some_and(|e| e.invalid_parameters()) {
                    self.error = Some("Fix invalid widget properties before exporting".into());
                    return;
                }
                let layout = self
                    .editor
                    .as_ref()
                    .map_or_else(|| self.layout.clone(), |e| Arc::new(e.draft.clone()));
                if let Some(source) = self.video_path.clone() {
                    let request = Request {
                        source,
                        output,
                        layout,
                        settings: settings.clone(),
                        start,
                        end: Some(end),
                        scale: self.scale_mode,
                        maps: self.overlay.maps().clone(),
                    };
                    let wake = ctx.clone();
                    self.export_job =
                        Some(export::Job::spawn(request, move || wake.request_repaint()));
                    self.prefs.export = Some(settings);
                    self.save_prefs();
                    self.export_dialog = None;
                }
            } else if !open {
                self.export_dialog = None;
            }
        }
        if let Some(job) = &self.export_job {
            let state = job.progress.lock().unwrap().clone();
            if !state.done {
                ctx.request_repaint_after(std::time::Duration::from_secs(1));
            }
            let mut dismiss = false;
            egui::Window::new(crate::i18n::text("Export progress"))
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(crate::i18n::ui_text(ui, &state.status));
                    if state.cancelled {
                        ui.label(crate::i18n::ui_text(
                            ui,
                            "The export contains the completed frames only.",
                        ));
                    }
                    ui.add(egui::ProgressBar::new(state.fraction).show_percentage());
                    ui.label(crate::i18n::ui_text(
                        ui,
                        format!(
                            "{} frames · {} elapsed",
                            state.frames,
                            duration_text(if state.done {
                                state.elapsed
                            } else {
                                job.started.elapsed().as_secs_f64()
                            })
                        ),
                    ));
                    if let Some(remaining) = state.remaining {
                        ui.label(crate::i18n::ui_text(
                            ui,
                            format!("About {} remaining", duration_text(remaining)),
                        ));
                    }
                    if let Some(error) = &state.error {
                        ui.colored_label(egui::Color32::RED, crate::i18n::ui_text(ui, error));
                    }
                    if state.done {
                        dismiss = ui.button(crate::i18n::ui_text(ui, "Close")).clicked();
                    } else if ui
                        .button(crate::i18n::ui_text(ui, "Cancel export"))
                        .clicked()
                    {
                        job.cancel.store(true, Ordering::Relaxed);
                    }
                });
            if dismiss {
                self.export_job = None;
            }
        }
    }
}

#[derive(Parser)]
#[command(
    name = "actionlay export",
    about = "Export using the same renderer as the desktop preview"
)]
struct Args {
    #[arg(long)]
    layout: Option<PathBuf>,
    #[arg(long)]
    out: PathBuf,
    #[arg(long, value_enum, default_value = "video")]
    mode: Mode,
    #[arg(long, value_enum)]
    format: Option<Format>,
    #[arg(long, default_value = "00FF00")]
    background: String,
    #[arg(long, default_value_t = 0.0)]
    start: f64,
    #[arg(long)]
    end: Option<f64>,
    #[arg(long)]
    software: bool,
    video: PathBuf,
}
pub fn cli() -> anyhow::Result<()> {
    let args = Args::parse_from(
        std::iter::once("actionlay export".to_string()).chain(std::env::args().skip(2)),
    );
    let color = parse_color(&args.background)?;
    let prefs = crate::prefs::default_path()
        .as_deref()
        .map(crate::prefs::Prefs::load)
        .unwrap_or_default();
    let layout = if let Some(path) = args.layout {
        {
            let loaded = actionlay_layout::Layout::load(&path)?;
            for warning in &loaded.warnings {
                log::warn!("{warning}");
            }
            loaded.layout
        }
    } else {
        let mut layout = crate::layouts::initial(&prefs).layout;
        crate::layouts::apply_appearance(&mut layout, prefs.appearance.as_ref());
        layout
    };
    let maps = actionlay_maps::TileStore::new(
        prefs.maps.unwrap_or_default(),
        actionlay_maps::TileStore::default_cache_dir(),
        || {},
    );
    let request = Request {
        source: args.video,
        output: args.out,
        layout: Arc::new(layout),
        settings: Settings {
            mode: args.mode,
            format: args.format.unwrap_or(if args.mode == Mode::Transparent {
                Format::Prores
            } else {
                Format::H264
            }),
            color,
            hardware: !args.software,
        },
        start: args.start,
        end: args.end,
        scale: actionlay_layout::geom::ScaleMode::Height,
        maps,
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    ctrlc::set_handler(move || c.store(true, Ordering::Relaxed))?;
    let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
    export::run(request, cancel, |state| {
        if state.done || last.elapsed().as_secs_f64() > 0.5 {
            eprintln!(
                "{} · {:.1}% · {} frames",
                state.status,
                state.fraction * 100.0,
                state.frames
            );
            last = std::time::Instant::now();
        }
    })
}
fn parse_color(value: &str) -> anyhow::Result<[u8; 3]> {
    let value = value.strip_prefix('#').unwrap_or(value);
    if value.len() != 6 || !value.is_ascii() {
        anyhow::bail!("background must be RRGGBB");
    }
    Ok([
        u8::from_str_radix(&value[0..2], 16)?,
        u8::from_str_radix(&value[2..4], 16)?,
        u8::from_str_radix(&value[4..6], 16)?,
    ])
}

/// Compact elapsed/remaining time, rounded down at unit boundaries.
fn duration_text(seconds: f64) -> String {
    let total = if seconds.is_finite() {
        seconds.max(0.0) as u64
    } else {
        0
    };
    if total < 60 {
        format!("{total} s")
    } else if total < 3600 {
        format!("{} min {} s", total / 60, total % 60)
    } else {
        format!(
            "{} h {} min {} s",
            total / 3600,
            total / 60 % 60,
            total % 60
        )
    }
}

#[cfg(test)]
mod display_tests {
    use super::*;
    #[test]
    fn duration_boundaries() {
        for (seconds, expected) in [
            (0.0, "0 s"),
            (59.9, "59 s"),
            (60.0, "1 min 0 s"),
            (3599.0, "59 min 59 s"),
            (3600.0, "1 h 0 min 0 s"),
            (7384.0, "2 h 3 min 4 s"),
        ] {
            assert_eq!(duration_text(seconds), expected);
        }
    }
    #[test]
    fn new_export_defaults_to_video_with_overlay() {
        let saved = Settings {
            mode: Mode::Transparent,
            ..Settings::default()
        };
        assert!(matches!(
            Dialog::new(saved, 10.0).settings.mode,
            Mode::Video
        ));
    }
}
