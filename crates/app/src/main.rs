//! ActionLay: plays a video with its telemetry overlay.
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod layouts;
mod menus;
mod overlay;
mod prefs;
mod telemetry_load;
mod transport;
mod video_view;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use actionlay_layout::Layout;
use actionlay_layout::geom::ScaleMode;
use actionlay_media::player::{Player, PlayerOptions};
use actionlay_telemetry::Telemetry;
use eframe::egui;
use overlay::{OverlayKey, OverlayRequest, OverlayWorker, Scheduler};
use video_view::VideoView;

struct App {
    egui_ctx: egui::Context,
    menus: menus::Menus,
    video_path: Option<PathBuf>,
    title_dirty: bool,
    select_layout: bool,
    select_audio: bool,
    audio_devices: Vec<String>,
    map_revision: u64,
    import_draft: Option<(PathBuf, [u32; 2])>,
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
    base_layout: Arc<Layout>,
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
    telemetry_rx: Option<telemetry_load::StreamLoad>,
    loaded_telemetry: Option<Arc<Telemetry>>,
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
    fn audio_dialog(&mut self, ctx: &egui::Context) {
        if !self.select_audio {
            return;
        }
        let mut open = true;
        let mut selected = self.prefs.audio_device.clone();
        egui::Window::new("Audio output")
            .open(&mut open)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.selectable_value(&mut selected, None, "System default");
                for name in &self.audio_devices {
                    ui.selectable_value(&mut selected, Some(name.clone()), name);
                }
                if let Some(name) = &selected
                    && !self.audio_devices.contains(name)
                {
                    ui.colored_label(egui::Color32::YELLOW, format!("Unavailable: {name}"));
                }
                if ui.button("Refresh devices").clicked() {
                    match actionlay_media::audio::AudioOutput::devices() {
                        Ok(names) => self.audio_devices = names,
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
            });
        self.select_audio = open;
        if selected != self.prefs.audio_device {
            if let Some(player) = &mut self.player {
                match player.change_audio_device(selected.as_deref()) {
                    Ok(true) => {
                        self.prefs.audio_device = selected;
                        self.save_prefs();
                        self.error = None;
                        self.video_notice = None;
                        return;
                    }
                    Err(e) => {
                        self.error = Some(format!("Cannot change audio output: {e}"));
                        return;
                    }
                    Ok(false) => {}
                }
            }
            self.prefs.audio_device = selected;
            self.save_prefs();
            if let Some(path) = self.video_path.clone() {
                let (position, paused, speed) =
                    self.player.as_ref().map_or((0.0, true, 1.0), |p| {
                        (p.position(), p.is_paused(), p.speed())
                    });
                if let Some(p) = &mut self.player {
                    p.pause();
                }
                self.open(path);
                if let Some(p) = &mut self.player {
                    p.seek(position, true);
                    p.set_speed(speed);
                    if !paused {
                        p.play();
                    }
                }
            }
        }
    }
    fn appearance_controls(&mut self, ui: &mut egui::Ui) {
        use actionlay_layout::model::Units;
        let theme = self
            .layout
            .theme
            .as_ref()
            .map_or_else(actionlay_layout::style::ResolvedTheme::default, |t| {
                t.resolve()
            });
        let mut appearance = self.prefs.appearance.clone().unwrap_or_default();
        let mut changed = false;
        let mut reset = false;
        ui.collapsing("Appearance · applies to all layouts", |ui| {
            let mut units = self.layout.units.unwrap_or_default();
            egui::ComboBox::from_id_salt("layout-units")
                .selected_text(match units {
                    Units::Metric => "Metric",
                    Units::Imperial => "Imperial",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut units, Units::Metric, "Metric");
                    ui.selectable_value(&mut units, Units::Imperial, "Imperial");
                });
            if units != self.layout.units.unwrap_or_default() {
                appearance.units = Some(units);
                changed = true;
            }
            ui.horizontal(|ui| {
                ui.label("Accent");
                let c = theme.accent;
                let mut accent = egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a);
                if ui.color_edit_button_srgba(&mut accent).changed() {
                    let [r, g, b, a] = accent.to_srgba_unmultiplied();
                    appearance.accent = Some(actionlay_layout::color::Color::rgba(r, g, b, a));
                    changed = true;
                }
            });
            let mut opacity = f32::from(theme.panel.a) / 255.0;
            if ui
                .add(egui::Slider::new(&mut opacity, 0.0..=1.0).text("Panel opacity"))
                .changed()
            {
                appearance.panel_opacity = Some(opacity);
                changed = true;
            }
            ui.small("Widget sizes follow the video when the window is resized.");
            reset = ui.button("Reset appearance").clicked();
        });
        if changed || reset {
            self.prefs.appearance = if reset { None } else { Some(appearance) };
            self.set_layout(self.base_layout.clone());
            self.save_prefs();
        }
    }

    fn import_dialog(&mut self, ctx: &egui::Context) {
        let Some((path, mut size)) = self.import_draft.clone() else {
            return;
        };
        let mut open = true;
        let mut import = false;
        egui::Window::new("Import XML layout")
            .open(&mut open)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label(path.file_name().unwrap_or_default().to_string_lossy());
                ui.label("Reference video resolution");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut size[0])
                            .range(1..=16384)
                            .prefix("Width "),
                    );
                    ui.add(
                        egui::DragValue::new(&mut size[1])
                            .range(1..=16384)
                            .prefix("Height "),
                    );
                });
                import = ui.button("Import into layout library").clicked();
            });
        self.import_draft = if open {
            Some((path.clone(), size))
        } else {
            None
        };
        if import {
            let result = (|| -> Result<(PathBuf, Option<String>), String> {
                let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let converted = actionlay_layout::import::xml(&text, &name, size)?;
                let dir = directories::ProjectDirs::from("org", "ActionLay", "ActionLay")
                    .ok_or("Layout library unavailable")?
                    .data_dir()
                    .join("layouts");
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                let mut out = dir.join(format!("{stem}.ovl.json"));
                let mut i = 2;
                while out.exists() {
                    out = dir.join(format!("{stem}-{i}.ovl.json"));
                    i += 1;
                }
                converted.layout.save(&out).map_err(|e| e.to_string())?;
                let notes = if converted.warnings.is_empty() {
                    None
                } else {
                    Some(
                        converted
                            .warnings
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join("; "),
                    )
                };
                Ok((out, notes))
            })();
            match result {
                Ok((out, notes)) => {
                    self.import_draft = None;
                    self.open_layout(out);
                    self.layout_notice = notes;
                }
                Err(e) => self.error = Some(e),
            }
        }
    }

    fn map_controls(&mut self, ui: &mut egui::Ui) {
        let mut maps = self.prefs.maps.clone().unwrap_or_default();
        let mut changed = false;
        ui.collapsing("Maps and privacy",|ui| {
            changed|=ui.checkbox(&mut maps.online,"Download visible map tiles").changed();
            ui.small("Cached tiles remain available with downloads disabled.");
            ui.horizontal(|ui|{
                for (name,url,attr) in [("OSM","https://tile.openstreetmap.org/{z}/{x}/{y}.png","© OpenStreetMap contributors"),
                    ("CyclOSM","https://a.tile-cyclosm.openstreetmap.fr/cyclosm/{z}/{x}/{y}.png","© CyclOSM · OpenStreetMap contributors"),
                    ("Thunderforest","https://a.tile.thunderforest.com/cycle/{z}/{x}/{y}.png?apikey={api_key}","© Thunderforest · OpenStreetMap contributors"),
                    ("Geoapify","https://maps.geoapify.com/v1/tile/osm-bright/{z}/{x}/{y}.png?apiKey={api_key}","© Geoapify · OpenStreetMap contributors")] {
                    if ui.small_button(name).clicked(){maps.url=url.into();maps.attribution=attr.into();changed=true;}
                }
            });
            ui.label("Tile URL ({z}, {x}, {y}, optional {api_key})");changed|=ui.text_edit_singleline(&mut maps.url).changed();
            ui.label("API key");changed|=ui.add(egui::TextEdit::singleline(&mut maps.api_key).password(true)).changed();
            ui.label("Attribution");changed|=ui.text_edit_singleline(&mut maps.attribution).changed();
            if !maps.valid(){ui.colored_label(egui::Color32::YELLOW,"Downloads wait for a valid URL and attribution.");}
            ui.separator();ui.label("Privacy zones (hide map position and route)");
            let mut remove=None;
            for (i,zone) in maps.privacy.iter_mut().enumerate(){ui.horizontal(|ui|{
                changed|=ui.add(egui::DragValue::new(&mut zone.lat).speed(0.0001).range(-85.0..=85.0).prefix("Lat ")).changed();
                changed|=ui.add(egui::DragValue::new(&mut zone.lon).speed(0.0001).range(-180.0..=180.0).prefix("Lon ")).changed();
                changed|=ui.add(egui::DragValue::new(&mut zone.radius_m).range(1.0..=100000.0).suffix(" m")).changed();
                if ui.small_button("Remove").clicked(){remove=Some(i);}
            });}
            if let Some(i)=remove {maps.privacy.remove(i);changed=true;}
            if ui.button("Add privacy zone at current position").clicked(){let snap=self.loaded_telemetry.as_ref().map(|t|t.sample(self.shown_t.unwrap_or(0.0)));
                maps.privacy.push(actionlay_maps::PrivacyZone{lat:snap.as_ref().and_then(|s|s.get(actionlay_telemetry::Metric::Lat).last_known()).unwrap_or(0.0),lon:snap.as_ref().and_then(|s|s.get(actionlay_telemetry::Metric::Lon).last_known()).unwrap_or(0.0),radius_m:250.0});changed=true;
            }
        });
        if changed {
            self.overlay.maps().configure(maps.clone());
            self.prefs.maps = Some(maps);
            self.save_prefs();
        }
    }

    fn layout_chooser(&mut self, ctx: &egui::Context) {
        if !self.select_layout {
            return;
        }
        let mut visible = true;
        let mut builtin = None;
        let mut selected = None;
        let mut browse = false;
        egui::Window::new("Select Layout")
            .open(&mut visible)
            .collapsible(false)
            .resizable(true)
            .default_width(580.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(650.0)
                    .show(ui, |ui| {
                        ui.heading("Included layouts");
                        for preset in actionlay_layout::catalog::PRESETS {
                            let active = self.prefs.last_layout.is_none()
                                && self.prefs.last_builtin.as_deref().unwrap_or("default")
                                    == preset.id;
                            if ui.selectable_label(active, preset.name).clicked() {
                                builtin = Some(preset.id);
                            }
                            ui.small(preset.description);
                        }
                        ui.collapsing("Upstream layout library", |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(240.0)
                                .show(ui, |ui| {
                                    for preset in actionlay_layout::catalog::UPSTREAM_PRESETS {
                                        if ui
                                            .selectable_label(
                                                self.prefs.last_builtin.as_deref()
                                                    == Some(preset.id),
                                                preset.name,
                                            )
                                            .clicked()
                                        {
                                            builtin = Some(preset.id);
                                        }
                                    }
                                });
                        });
                        ui.separator();
                        self.appearance_controls(ui);
                        self.map_controls(ui);
                        ui.separator();
                        ui.collapsing("Imported layouts", |ui| {
                            if let Some(root) =
                                directories::ProjectDirs::from("org", "ActionLay", "ActionLay")
                            {
                                let mut files: Vec<_> =
                                    std::fs::read_dir(root.data_dir().join("layouts"))
                                        .into_iter()
                                        .flatten()
                                        .filter_map(Result::ok)
                                        .map(|e| e.path())
                                        .filter(|p| {
                                            p.is_file()
                                                && p.to_string_lossy().ends_with(".ovl.json")
                                        })
                                        .collect();
                                files.sort();
                                for path in files {
                                    let name =
                                        path.file_name().unwrap_or_default().to_string_lossy();
                                    if ui
                                        .selectable_label(
                                            self.prefs.last_layout.as_ref() == Some(&path),
                                            name,
                                        )
                                        .clicked()
                                    {
                                        selected = Some(path);
                                    }
                                }
                            }
                        });
                        ui.heading("Recent layouts");
                        egui::ScrollArea::vertical()
                            .max_height(280.0)
                            .show(ui, |ui| {
                                if self.prefs.recent_layouts.is_empty() {
                                    ui.weak("No layouts loaded from file yet.");
                                }
                                for path in &self.prefs.recent_layouts {
                                    let name =
                                        path.file_name().unwrap_or_default().to_string_lossy();
                                    if ui
                                        .selectable_label(
                                            self.prefs.last_layout.as_ref() == Some(path),
                                            name,
                                        )
                                        .on_hover_text(path.display().to_string())
                                        .clicked()
                                    {
                                        selected = Some(path.clone());
                                    }
                                    ui.small(path.display().to_string());
                                }
                            });
                        ui.separator();
                        browse = ui.button("Open layout from file…").clicked();
                    });
            });
        self.select_layout = visible;
        if let Some(id) = builtin {
            self.set_layout(Arc::new(
                actionlay_layout::catalog::find(id).unwrap().layout(),
            ));
            self.prefs.last_layout = None;
            self.prefs.last_builtin = Some(id.into());
            self.layout_notice = None;
            self.error = None;
            self.save_prefs();
            self.select_layout = false;
        } else if let Some(path) = selected {
            self.open_layout(path);
            // Keep the chooser available if a recent file has moved or is invalid.
            self.select_layout = self.error.is_some();
        } else if browse {
            self.command(menus::Command::OpenLayoutFile, ctx);
        }
    }

    fn command(&mut self, command: menus::Command, ctx: &egui::Context) {
        match command {
            menus::Command::SelectLayout => self.select_layout = true,
            menus::Command::AudioSettings => {
                match actionlay_media::audio::AudioOutput::devices() {
                    Ok(names) => self.audio_devices = names,
                    Err(e) => self.error = Some(e.to_string()),
                }
                self.select_audio = true;
            }
            menus::Command::OpenRecentVideo(index) => {
                if let Some(path) = self.prefs.recent_videos.get(index).cloned() {
                    self.open(path);
                }
            }
            menus::Command::ClearRecentVideos => {
                self.prefs.recent_videos.clear();
                self.save_prefs();
            }
            menus::Command::OpenVideo | menus::Command::OpenLayoutFile => {
                // Native dialogs are modal. Pause the clock so cancel resumes at
                // the same position, without a burst of queued video frames.
                let resume = self.player.as_ref().is_some_and(|p| !p.is_paused());
                if let Some(p) = &mut self.player {
                    p.pause();
                }
                let layout = command == menus::Command::OpenLayoutFile;
                let mut dialog = if layout {
                    rfd::FileDialog::new()
                        .set_title("Open Layout")
                        .add_filter("Overlay layout (.ovl.json or XML)", &["json", "xml"])
                } else {
                    rfd::FileDialog::new().set_title("Open Video").add_filter(
                        "Video",
                        &["mp4", "mov", "m4v", "mkv", "avi", "webm", "mts", "m2ts"],
                    )
                }
                .add_filter("All files", &["*"]);
                let previous = if layout {
                    self.prefs.last_layout.as_ref()
                } else {
                    self.video_path.as_ref()
                };
                if let Some(parent) = previous.and_then(|p| p.parent()) {
                    dialog = dialog.set_directory(parent);
                }
                let selected = dialog.pick_file();
                if resume && let Some(p) = &mut self.player {
                    p.play();
                }
                if let Some(path) = selected {
                    if layout {
                        self.open_layout(path);
                    } else {
                        self.open(path);
                    }
                }
            }
            menus::Command::CloseVideo => {
                self.player = None;
                self.video_path = None;
                self.title_dirty = true;
                self.telemetry_rx = None;
                self.loaded_telemetry = None;
                self.video_notice = if self
                    .player
                    .as_ref()
                    .is_some_and(|p| p.info().audio.is_some() && !p.stats().audio_active)
                {
                    Some("Audio output unavailable; playback is silent. Select a device in Settings → Audio.".into())
                } else {
                    None
                };
                self.failure_notice = None;
                self.error = None;
                self.shown_t = None;
                self.scrub = Default::default();
                self.view.clear_video();
                self.overlay_ready = false;
                self.overlay_ms = None;
                self.scheduler.reset();
                self.overlay.set_telemetry(None, self.scale_mode);
                self.telemetry_rev += 1;
            }
            menus::Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
        ctx.request_repaint();
    }

    fn open(&mut self, path: PathBuf) {
        match Player::open_with_audio_device(
            &path,
            PlayerOptions::default(),
            self.prefs.audio_device.as_deref(),
        ) {
            Ok(p) => {
                let info = p.info();
                let metadata = info.telemetry.clone();
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
                self.video_path = Some(path.clone());
                prefs::remember(&mut self.prefs.recent_videos, path.clone());
                self.save_prefs();
                self.title_dirty = true;
                self.view.clear_video();
                self.scrub = Default::default();
                self.error = None;
                // the previous video's telemetry, notices and overlay are gone for good:
                // its loader's result (if any) is discarded with the receiver
                self.telemetry_rx = None;
                self.loaded_telemetry = None;
                self.video_notice = None;
                self.shown_t = None;
                self.overlay_ready = false;
                self.overlay_ms = None;
                self.scheduler.reset();
                self.overlay
                    .set_telemetry(Some(Arc::new(Telemetry::empty(duration))), self.scale_mode);
                self.telemetry_rev += 1;
                let ctx = self.egui_ctx.clone();
                self.telemetry_rx = metadata.and_then(|meta| {
                    self.player.as_mut().unwrap().take_telemetry().map(|rx| {
                        telemetry_load::spawn(rx, duration, meta.packet_count, move || {
                            ctx.request_repaint()
                        })
                    })
                });
            }
            Err(e) => self.error = Some(format!("{}: {e}", path.display())),
        }
    }

    fn open_layout(&mut self, path: PathBuf) {
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("xml"))
        {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let size = actionlay_layout::import::reference_size(&name).unwrap_or([1920, 1080]);
            self.import_draft = Some((path, size));
            return;
        }

        // remembered as an absolute path: the app may be started from anywhere
        let path = std::path::absolute(&path).unwrap_or(path);
        match layouts::load(&path) {
            Ok((layout, notice)) => {
                self.select_layout = false;
                self.set_layout(Arc::new(layout));
                self.layout_notice = notice;
                self.error = None;
                prefs::remember(&mut self.prefs.recent_layouts, path.clone());
                self.prefs.last_layout = Some(path);
                self.prefs.last_builtin = None;
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
        self.base_layout = layout;
        let mut styled = (*self.base_layout).clone();
        layouts::apply_appearance(&mut styled, self.prefs.appearance.as_ref());
        let layout = Arc::new(styled);
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
        let Some(loaded) = rx.take_update() else {
            return;
        };
        if let Some(w) = loaded.warning {
            log::warn!("{w}");
            self.video_notice = Some(w);
        }
        let telemetry = Arc::new(loaded.telemetry);
        self.loaded_telemetry = Some(telemetry.clone());
        self.overlay.set_telemetry(Some(telemetry), self.scale_mode);
        self.telemetry_rev += 1;
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
        let mut status = match (self.overlay_visible, self.overlay_ms) {
            (false, _) => "overlay off (O)".into(),
            (true, Some(ms)) => format!("overlay {ms:.1} ms"),
            (true, None) => "overlay …".into(),
        };
        if self
            .player
            .as_ref()
            .is_some_and(|p| p.info().telemetry.is_some())
            && self
                .loaded_telemetry
                .as_ref()
                .is_none_or(|t| !t.is_loaded_at(self.shown_t.unwrap_or(0.0)))
        {
            status.push_str(" · loading telemetry…");
        }
        status
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
        if let Some(command) = self.menus.show(ui, self.player.is_some(), &self.prefs) {
            self.command(command, ui.ctx());
        }
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
        let map_revision = self.overlay.maps().revision();
        if map_revision != self.map_revision {
            self.map_revision = map_revision;
            self.layout_rev += 1;
        }

        if !ui.ctx().egui_wants_keyboard_input()
            && ui
                .ctx()
                .input(|i| i.modifiers.is_none() && i.key_pressed(egui::Key::O))
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
        });
        self.audio_dialog(ui.ctx());
        let mut open_clicked = false;
        egui::CentralPanel::default().show(ui, |ui| {
            let rect = ui.available_rect_before_wrap();
            if self.player.is_none() {
                let response = ui
                    .allocate_rect(rect, egui::Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Open a video")
                });
                ui.painter().text(
                    rect.center() - egui::vec2(0.0, 32.0),
                    egui::Align2::CENTER_CENTER,
                    "Open a video",
                    egui::FontId::proportional(28.0),
                    ui.visuals().text_color(),
                );
                ui.painter().text(
                    rect.center() + egui::vec2(0.0, 8.0),
                    egui::Align2::CENTER_CENTER,
                    "Click here or drag a video into the window",
                    egui::FontId::proportional(16.0),
                    ui.visuals().weak_text_color(),
                );
                ui.painter().text(
                    rect.center() + egui::vec2(0.0, 38.0),
                    egui::Align2::CENTER_CENTER,
                    if cfg!(target_os = "macos") {
                        "File / Open Video…   ·   ⌘O"
                    } else {
                        "File / Open Video…   ·   Ctrl+O"
                    },
                    egui::FontId::proportional(14.0),
                    ui.visuals().weak_text_color(),
                );
                open_clicked = response.clicked();
                return;
            }
            // snapped once: the paint viewport and the overlay texture share its size
            let video = self.view.video_rect(rect, ui.ctx().pixels_per_point());
            if let Some(video) = video {
                self.request_overlay(ui.ctx(), video);
            }
            self.view
                .show(ui, rect, video, self.overlay_visible && self.overlay_ready);
        });
        if open_clicked {
            self.command(menus::Command::OpenVideo, ui.ctx());
        }
        self.layout_chooser(ui.ctx());
        self.import_dialog(ui.ctx());
        if self.title_dirty {
            let title = self
                .video_path
                .as_ref()
                .and_then(|p| p.file_name())
                .map_or_else(
                    || "ActionLay".into(),
                    |p| format!("{} — ActionLay", p.to_string_lossy()),
                );
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Title(title));
            self.title_dirty = false;
        }

        if let Some(p) = &self.player {
            if !p.is_paused() {
                ui.ctx().request_repaint();
            } else if p.is_awaiting_frame() {
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
            let base_layout = Arc::new(initial.layout);
            let mut styled = (*base_layout).clone();
            layouts::apply_appearance(&mut styled, prefs.appearance.as_ref());
            let layout = Arc::new(styled);
            let ctx = cc.egui_ctx.clone();
            let overlay = OverlayWorker::spawn(layout.clone(), move || ctx.request_repaint());
            overlay
                .maps()
                .configure(prefs.maps.clone().unwrap_or_default());
            let mut app = App {
                menus: menus::Menus::new(&cc.egui_ctx)?,
                video_path: None,
                title_dirty: false,
                select_layout: false,
                select_audio: false,
                audio_devices: Vec::new(),
                map_revision: 0,
                import_draft: None,
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
                base_layout,
                scale_mode: ScaleMode::Height,
                overlay_visible: true,
                overlay_ready: false,
                overlay_ms: None,
                failures_seen: 0,
                layout_rev: 0,
                telemetry_rev: 0,
                telemetry_rx: None,
                loaded_telemetry: None,
                egui_ctx: cc.egui_ctx.clone(),
                shown_t: None,
                prefs,
                prefs_path,
                max_texture,
                logged_overlay_size: None,
            };
            if initial.fallback {
                // the notice says it once; do not repeat it at every launch
                app.prefs.last_layout = None;
                app.prefs.last_builtin = None;
                app.save_prefs();
            }
            if let Some(path) = path {
                app.open(path);
            }
            Ok(Box::new(app))
        }),
    )
}
