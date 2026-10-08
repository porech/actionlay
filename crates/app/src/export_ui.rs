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
    input: export::InputProperties,
    advanced_open: bool,
    start: f64,
    end: f64,
    duration: f64,
    error: Option<String>,
}

fn option_row(
    ui: &mut egui::Ui,
    label: &str,
    modified: bool,
    edit: impl FnOnce(&mut egui::Ui),
) -> bool {
    ui.push_id(label, |ui| {
        ui.horizontal_wrapped(|ui| {
            let label = crate::i18n::text(label);
            if modified {
                ui.colored_label(ui.visuals().warn_fg_color, label);
            } else {
                ui.label(label);
            }
            edit(ui);
            modified
                && ui
                    .small_button(crate::i18n::text("Set preset default"))
                    .clicked()
        })
        .inner
    })
    .inner
}

fn megabits(ui: &mut egui::Ui, value: &mut u32, minimum: f64, maximum: f64) {
    let mut displayed = f64::from(*value) / 1000.0;
    if ui
        .add(
            egui::DragValue::new(&mut displayed)
                .range(minimum..=maximum)
                .speed(0.1),
        )
        .changed()
    {
        *value = (displayed * 1000.0).round() as u32;
    }
}

impl Dialog {
    pub fn new(settings: Settings, duration: f64, input: export::InputProperties) -> Self {
        Self {
            advanced_open: settings.advanced_edited || settings.is_custom(),
            settings,
            input,
            start: 0.0,
            end: duration,
            duration,
            error: None,
        }
    }
    fn select_preset(&mut self, preset: export::QualityPreset) {
        self.settings.apply_preset(preset);
        self.advanced_open = false;
        self.error = None;
    }
    fn advanced(&mut self, ui: &mut egui::Ui) {
        use actionlay_media::export::{EncodingSpeed, RateControl};
        use export::Container;
        let before = self.settings.clone();
        let defaults = Settings::preset_defaults(self.settings.preset);
        let format = self.settings.resolved_format(&self.input);
        if option_row(
            ui,
            "Format",
            self.settings.format != defaults.format,
            |ui| {
                let selected = if self.settings.format == Format::Input {
                    format!(
                        "{} · {}",
                        crate::i18n::text("Copy from input"),
                        format.codec()
                    )
                } else {
                    self.settings.format.codec().to_owned()
                };
                egui::ComboBox::from_id_salt("export-format")
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.settings.format,
                            Format::Input,
                            crate::i18n::text("Copy from input"),
                        );
                        if self.settings.mode != Mode::Transparent {
                            ui.selectable_value(&mut self.settings.format, Format::H264, "H.264");
                            ui.selectable_value(&mut self.settings.format, Format::H265, "H.265");
                        }
                        ui.selectable_value(
                            &mut self.settings.format,
                            Format::Prores,
                            "ProRes 4444",
                        );
                        ui.selectable_value(
                            &mut self.settings.format,
                            Format::Png,
                            crate::i18n::text("PNG sequence"),
                        );
                    });
            },
        ) {
            self.settings.format = defaults.format;
        }
        if self.settings.format == Format::Input
            && self.input.format.is_none()
            && self.settings.mode != Mode::Transparent
        {
            ui.small(crate::i18n::text("Input codec unavailable"));
        }
        let format = self.settings.resolved_format(&self.input);
        if !matches!(format, Format::Png | Format::Prores) {
            if option_row(
                ui,
                "File container",
                self.settings.container != defaults.container,
                |ui| {
                    let selected = if self.settings.container == Container::Input {
                        format!(
                            "{} · {}",
                            crate::i18n::text("Copy from input"),
                            self.settings.resolved_container(&self.input).extension()
                        )
                    } else {
                        self.settings.container.extension().to_owned()
                    };
                    egui::ComboBox::from_id_salt("export-container")
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.settings.container,
                                Container::Input,
                                crate::i18n::text("Copy from input"),
                            );
                            ui.selectable_value(
                                &mut self.settings.container,
                                Container::Mp4,
                                "MP4",
                            );
                            ui.selectable_value(
                                &mut self.settings.container,
                                Container::Mov,
                                "MOV",
                            );
                        });
                },
            ) {
                self.settings.container = defaults.container;
            }
            if self.settings.container == Container::Input && self.input.container.is_none() {
                ui.small(crate::i18n::text("Input container unavailable"));
            }
        }
        if option_row(
            ui,
            "Resolution",
            self.settings.resolution != defaults.resolution,
            |ui| {
                let mut copy = self.settings.resolution.is_none();
                if ui
                    .checkbox(&mut copy, crate::i18n::text("Copy from input"))
                    .changed()
                {
                    self.settings.resolution = (!copy).then_some(self.input.dimensions);
                }
                let mut dimensions = self.settings.resolution.unwrap_or(self.input.dimensions);
                ui.add_enabled_ui(!copy, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut dimensions[0])
                            .range(2..=8192)
                            .speed(2.0),
                    );
                    ui.label("×");
                    ui.add(
                        egui::DragValue::new(&mut dimensions[1])
                            .range(2..=8192)
                            .speed(2.0),
                    );
                });
                if !copy {
                    self.settings.resolution = Some(dimensions);
                }
            },
        ) {
            self.settings.resolution = defaults.resolution;
        }
        if matches!(format, Format::H264 | Format::H265) {
            if option_row(
                ui,
                "Rate control",
                self.settings.encoding.rate_control != defaults.encoding.rate_control,
                |ui| {
                    let label = |rate| {
                        crate::i18n::text(match rate {
                            RateControl::Input => "Copy from input",
                            RateControl::Quality => "Constant quality",
                            RateControl::Bitrate => "Target bitrate",
                        })
                    };
                    egui::ComboBox::from_id_salt("export-rate")
                        .selected_text(label(self.settings.encoding.rate_control))
                        .show_ui(ui, |ui| {
                            for rate in [
                                RateControl::Input,
                                RateControl::Quality,
                                RateControl::Bitrate,
                            ] {
                                ui.add_enabled_ui(
                                    rate != RateControl::Quality || !self.settings.hardware,
                                    |ui| {
                                        ui.selectable_value(
                                            &mut self.settings.encoding.rate_control,
                                            rate,
                                            label(rate),
                                        );
                                    },
                                );
                            }
                        });
                },
            ) {
                self.settings.encoding.rate_control = defaults.encoding.rate_control;
                if defaults.encoding.rate_control == RateControl::Quality {
                    self.settings.hardware = false;
                }
            }
            match self.settings.encoding.rate_control {
                RateControl::Input => {
                    if self.input.bit_rate > 0 {
                        ui.label(format!(
                            "{}: {:.2} Mbps",
                            crate::i18n::text("Target bitrate"),
                            self.input.bit_rate as f64 / 1_000_000.0
                        ));
                    } else {
                        ui.small(crate::i18n::text("Input bitrate unavailable"));
                    }
                }
                RateControl::Quality => {
                    if option_row(
                        ui,
                        "Quality (CRF)",
                        self.settings.encoding.quality != defaults.encoding.quality,
                        |ui| {
                            ui.add(
                                egui::DragValue::new(&mut self.settings.encoding.quality)
                                    .range(0..=51),
                            );
                        },
                    ) {
                        self.settings.encoding.quality = defaults.encoding.quality;
                    }
                }
                RateControl::Bitrate => {
                    if option_row(
                        ui,
                        "Video bitrate (Mbps)",
                        self.settings.encoding.bitrate_kbps != defaults.encoding.bitrate_kbps,
                        |ui| {
                            megabits(ui, &mut self.settings.encoding.bitrate_kbps, 0.1, 500.0);
                        },
                    ) {
                        self.settings.encoding.bitrate_kbps = defaults.encoding.bitrate_kbps;
                    }
                }
            }
            if option_row(
                ui,
                "Maximum bitrate (Mbps)",
                self.settings.encoding.max_bitrate_kbps != defaults.encoding.max_bitrate_kbps,
                |ui| {
                    let previous = self.settings.encoding.max_bitrate_kbps;
                    megabits(ui, &mut self.settings.encoding.max_bitrate_kbps, 0.0, 500.0);
                    if previous != self.settings.encoding.max_bitrate_kbps {
                        self.settings.encoding.buffer_kbits =
                            self.settings.encoding.max_bitrate_kbps.saturating_mul(2);
                    }
                },
            ) {
                self.settings.encoding.max_bitrate_kbps = defaults.encoding.max_bitrate_kbps;
                self.settings.encoding.buffer_kbits = defaults.encoding.buffer_kbits;
            }
            if option_row(
                ui,
                "Buffer size (Mbit)",
                self.settings.encoding.buffer_kbits != defaults.encoding.buffer_kbits,
                |ui| {
                    megabits(ui, &mut self.settings.encoding.buffer_kbits, 0.0, 1000.0);
                },
            ) {
                self.settings.encoding.buffer_kbits = defaults.encoding.buffer_kbits;
                self.settings.encoding.max_bitrate_kbps = defaults.encoding.max_bitrate_kbps;
            }
            if option_row(
                ui,
                "Keyframe interval (frames)",
                self.settings.encoding.keyframe_frames != defaults.encoding.keyframe_frames,
                |ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.settings.encoding.keyframe_frames)
                            .range(0..=10_000),
                    );
                },
            ) {
                self.settings.encoding.keyframe_frames = defaults.encoding.keyframe_frames;
            }
            ui.small(crate::i18n::text("0 = automatic"));
            if option_row(
                ui,
                "Encoding speed",
                self.settings.encoding.speed != defaults.encoding.speed,
                |ui| {
                    let label = |speed| {
                        crate::i18n::text(match speed {
                            EncodingSpeed::Veryfast => "Fast export",
                            EncodingSpeed::Fast => "Balanced",
                            EncodingSpeed::Slow => "High quality",
                        })
                    };
                    egui::ComboBox::from_id_salt("export-speed")
                        .selected_text(label(self.settings.encoding.speed))
                        .show_ui(ui, |ui| {
                            for speed in [
                                EncodingSpeed::Veryfast,
                                EncodingSpeed::Fast,
                                EncodingSpeed::Slow,
                            ] {
                                ui.selectable_value(
                                    &mut self.settings.encoding.speed,
                                    speed,
                                    label(speed),
                                );
                            }
                        });
                },
            ) {
                self.settings.encoding.speed = defaults.encoding.speed;
            }
            if option_row(
                ui,
                "Prefer hardware encoder",
                self.settings.hardware != defaults.hardware,
                |ui| {
                    if ui.checkbox(&mut self.settings.hardware, "").changed()
                        && self.settings.hardware
                        && self.settings.encoding.rate_control == RateControl::Quality
                    {
                        self.settings.encoding.rate_control = RateControl::Input;
                    }
                },
            ) {
                self.settings.hardware = defaults.hardware;
            }
        }
        if self.settings != before {
            self.settings.advanced_edited = true;
        }
    }
    pub fn show(&mut self, ctx: &egui::Context) -> (bool, Option<PathBuf>) {
        let mut open = true;
        let mut choose = false;
        egui::Window::new(crate::i18n::text("Export video / overlay")).open(&mut open).default_width(620.0).resizable(true).vscroll(true).show(ctx, |ui| {
            egui::ComboBox::from_id_salt("export-mode").selected_text(crate::i18n::ui_text(ui, match self.settings.mode { Mode::Video => "Video with overlay", Mode::Transparent => "Overlay · transparent", Mode::Solid => "Overlay · solid background" })).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.settings.mode, Mode::Video, crate::i18n::text("Video with overlay"));
                ui.selectable_value(&mut self.settings.mode, Mode::Solid, crate::i18n::text("Overlay · solid background"));
                ui.selectable_value(&mut self.settings.mode, Mode::Transparent, crate::i18n::text("Overlay · transparent"));
            });
            if self.settings.mode == Mode::Solid { ui.horizontal(|ui| { ui.label(crate::i18n::text("Background")); ui.color_edit_button_srgb(&mut self.settings.color); if ui.button(crate::i18n::text("Green screen")).clicked() { self.settings.color = [0,255,0]; } }); }
            ui.horizontal(|ui| {
                ui.label(crate::i18n::text("Quality preset"));
                let mut selected = None;
                egui::ComboBox::from_id_salt("export-quality-preset").selected_text(crate::i18n::text(self.settings.preset.label())).show_ui(ui, |ui| {
                    for preset in [export::QualityPreset::Balanced, export::QualityPreset::HighQuality, export::QualityPreset::Fast] {
                        if ui.selectable_label(self.settings.preset == preset, crate::i18n::text(preset.label())).clicked() { selected = Some(preset); }
                    }
                });
                if let Some(preset) = selected { self.select_preset(preset); }
            });
            let format = self.settings.resolved_format(&self.input);
            let dimensions = self.settings.resolution.unwrap_or(self.input.dimensions);
            ui.label(format!("{} · {} · {}×{}", format.codec(), if format == Format::Png { "PNG" } else { self.settings.resolved_container(&self.input).extension() }, dimensions[0], dimensions[1]));
            let id = ui.make_persistent_id("export-advanced");
            let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ctx, id, self.advanced_open);
            state.set_open(self.advanced_open);
            let mut toggle = false;
            state.show_header(ui, |ui| { toggle = ui.selectable_label(false, crate::i18n::text("Advanced")).clicked(); }).body(|ui| self.advanced(ui));
            self.advanced_open = egui::collapsing_header::CollapsingState::load(ctx, id).is_some_and(|s| s.is_open());
            if toggle { self.advanced_open = !self.advanced_open; }
            ui.horizontal(|ui| { ui.label(crate::i18n::text("In (seconds)")); ui.add(egui::DragValue::new(&mut self.start).speed(0.1).range(0.0..=self.duration)); });
            ui.horizontal(|ui| { ui.label(crate::i18n::text("Out (seconds)")); ui.add(egui::DragValue::new(&mut self.end).speed(0.1).range(0.0..=self.duration)); });
            ui.small(crate::i18n::text("Frame timestamps are preserved. Video-file audio is copied; overlay-only and PNG exports are silent."));
            let valid = self.settings.encoding.valid() && dimensions.iter().all(|d| *d > 0 && *d <= 8192 && *d % 2 == 0);
            if !valid { ui.colored_label(ui.visuals().error_fg_color, crate::i18n::text("Invalid export encoding settings")); }
            if let Some(error) = &self.error { ui.colored_label(ui.visuals().error_fg_color, crate::i18n::ui_text(ui, error)); }
            if ui.add_enabled(valid, egui::Button::new(crate::i18n::text("Choose destination and export…"))).clicked() {
                if self.end > self.start { choose = true; } else { self.error = Some("Out must be after In".into()); }
            }
        });
        let output = if choose {
            let dialog = rfd::FileDialog::new()
                .set_title(crate::i18n::native_text("New export destination"));
            let format = self.settings.resolved_format(&self.input);
            if format == Format::Png {
                dialog.set_file_name("overlay-frames").save_file()
            } else {
                let extension = self.settings.resolved_container(&self.input).extension();
                dialog
                    .add_filter(crate::i18n::native_text("Export"), &[extension])
                    .set_file_name(format!("export.{extension}"))
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
                        activity: self.activity.as_ref().map(|activity| {
                            let duration = self.player.as_ref().unwrap().info().duration;
                            let (origin, _) = activity.sync_origin(self.video_utc(), duration);
                            (activity.clone(), origin, self.source_settings.offset)
                        }),
                        rotation: self.source_settings.rotation,
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
                    if state.cancelled && state.frames > 0 {
                        ui.label(crate::i18n::ui_text(
                            ui,
                            "The export contains the completed frames only.",
                        ));
                    }
                    if state.preparing_metadata {
                        match state.metadata_fraction {
                            Some(fraction) => {
                                ui.add(egui::ProgressBar::new(fraction).show_percentage());
                            }
                            None => {
                                ui.add(egui::ProgressBar::new(0.0).animate(!state.done));
                            }
                        }
                    } else {
                        ui.add(egui::ProgressBar::new(state.fraction).show_percentage());
                    }
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
    /// Clockwise video rotation; auto follows the source display matrix.
    #[arg(long, value_enum, default_value = "auto")]
    rotation: crate::rotation::Rotation,
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
    #[arg(long, conflicts_with = "software")]
    hardware: bool,
    #[arg(long, value_enum, default_value = "balanced")]
    preset: export::QualityPreset,
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
        activity: None,
        rotation: args.rotation,
        source: args.video,
        output: args.out,
        layout: Arc::new(layout),
        settings: Settings {
            mode: args.mode,
            format: args.format.unwrap_or(Format::Input),
            color,
            hardware: args.hardware,
            ..Settings::preset_defaults(args.preset)
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
            if state.preparing_metadata {
                let percent = state
                    .metadata_fraction
                    .map_or_else(|| "…".into(), |f| format!("{:.1}%", f * 100.0));
                eprintln!("{} · {percent}", state.status);
            } else {
                eprintln!(
                    "{} · {:.1}% · {} frames",
                    state.status,
                    state.fraction * 100.0,
                    state.frames
                );
            }
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
    fn advanced_reset_button_restores_only_its_option() {
        let settings = Settings {
            format: Format::H265,
            resolution: Some([1280, 720]),
            advanced_edited: true,
            ..Settings::default()
        };
        let input = export::InputProperties {
            dimensions: [1920, 1080],
            format: Some(Format::H264),
            container: Some(export::Container::Mp4),
            bit_rate: 20_000_000,
        };
        let mut dialog = Dialog::new(settings, 10.0, input);
        let ctx = egui::Context::default();
        let draw = |dialog: &mut Dialog, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, 1000.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    dialog.show(ui.ctx());
                },
            );
            output.textures_delta.clear();
            output
        };
        draw(&mut dialog, vec![]);
        let output = draw(&mut dialog, vec![]);
        fn button(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Pos2> {
            match shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(|s| button(s, label)),
                _ => None,
            }
        }
        let pos = output
            .shapes
            .iter()
            .find_map(|shape| button(&shape.shape, crate::i18n::text("Set preset default")))
            .expect("modified format has a reset button");
        draw(
            &mut dialog,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        draw(
            &mut dialog,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(dialog.settings.format, Format::Input);
        assert_eq!(dialog.settings.resolution, Some([1280, 720]));
    }

    #[test]
    fn export_remembers_mode_and_modified_advanced_settings() {
        let saved = Settings {
            mode: Mode::Transparent,
            advanced_edited: true,
            ..Settings::default()
        };
        let input = export::InputProperties {
            dimensions: [1920, 1080],
            format: Some(Format::H264),
            container: Some(export::Container::Mp4),
            bit_rate: 20_000_000,
        };
        let mut dialog = Dialog::new(saved, 10.0, input);
        assert!(matches!(dialog.settings.mode, Mode::Transparent));
        assert!(dialog.advanced_open);
        dialog.settings.resolution = Some([640, 480]);
        dialog.select_preset(export::QualityPreset::Balanced);
        assert!(!dialog.advanced_open);
        assert!(!dialog.settings.advanced_edited);
        assert_eq!(dialog.settings.resolution, None);
        assert!(!dialog.settings.is_custom());
    }
}
