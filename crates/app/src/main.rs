//! ActionLay: plays a video with its telemetry overlay.
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod advanced;
mod editor;
mod export;
mod export_ui;
mod i18n;
mod integration;
mod layouts;
mod menus;
mod overlay;
mod prefs;
mod telemetry_load;
mod transport;
mod video_view;
mod window;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use actionlay_layout::Layout;
use actionlay_layout::geom::ScaleMode;
use actionlay_media::chapters::ChapterPlayer as Player;
use actionlay_media::player::PlayerOptions;
use actionlay_telemetry::Telemetry;
use eframe::egui;
use overlay::{OverlayKey, OverlayRequest, OverlayWorker, Scheduler};
use video_view::VideoView;

struct App {
    window: window::State,
    interface: i18n::Interface,
    regional_visible: bool,
    advanced_visible: bool,
    #[cfg(target_os = "macos")]
    _file_open_handler: objc2::rc::Retained<integration::macos::FileOpenHandler>,
    #[cfg(not(target_os = "macos"))]
    integration: integration::Dialog,
    export_dialog: Option<export_ui::Dialog>,
    export_job: Option<export::Job>,
    editor: Option<editor::Editor>,
    pending_edit_action: Option<EditAction>,
    egui_ctx: egui::Context,
    menus: menus::Menus,
    video_path: Option<PathBuf>,
    title_dirty: bool,
    select_layout: bool,
    select_audio: bool,
    select_maps: bool,
    select_privacy: bool,
    select_sources: bool,
    camera_telemetry: Option<Arc<Telemetry>>,
    activity: Option<actionlay_telemetry::external::Activity>,
    activity_rx:
        Option<std::sync::mpsc::Receiver<Result<actionlay_telemetry::external::Activity, String>>>,
    source_settings: prefs::SourceSettings,
    source_notice: Option<String>,
    chapter_offer: Option<usize>,
    audio_devices: Vec<String>,
    volume: f32,
    muted: bool,
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
    route_requested_until: f64,
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

enum EditAction {
    Exit,
    New,
    Command(menus::Command),
    OpenLayout(PathBuf),
    Builtin(String),
}

impl App {
    fn begin_edit(&mut self, new: bool) {
        self.window.set_fullscreen(&self.egui_ctx, false);
        if self.editor.is_some() {
            return;
        }
        if let Some(player) = &mut self.player {
            player.pause();
        }
        let size = self
            .player
            .as_ref()
            .map(|p| [p.info().video.width, p.info().video.height]);
        self.select_layout = false;
        self.editor = Some(editor::Editor::new(
            if new {
                editor::Editor::blank()
            } else {
                (*self.base_layout).clone()
            },
            if new {
                None
            } else {
                self.prefs.last_layout.clone()
            },
            size,
            new,
        ));
    }

    fn save_edit(&mut self, save_as: bool) -> bool {
        let Some(editor) = &mut self.editor else {
            return false;
        };
        let path = if !save_as { editor.path.clone() } else { None };
        let path = match path {
            Some(path) => path,
            None => {
                let name = editor.draft.name.as_deref().unwrap_or("Untitled");
                let name: String = name
                    .chars()
                    .map(|c| if "/\\:*?\"<>|".contains(c) { '_' } else { c })
                    .collect();
                let name = if editor.is_system_copy() {
                    format!("{name} copy.ovl.json")
                } else {
                    format!("{name}.ovl.json")
                };
                let mut dialog = rfd::FileDialog::new()
                    .add_filter(
                        crate::i18n::native_text("Portable ActionLay layout"),
                        &["actionlay-layout"],
                    )
                    .add_filter(crate::i18n::native_text("ActionLay layout"), &["json"])
                    .set_file_name(if editor.draft.loaded_assets.is_empty() {
                        name
                    } else {
                        name.replace(".ovl.json", ".actionlay-layout")
                    });
                if let Some(dirs) = directories::ProjectDirs::from("org", "ActionLay", "ActionLay")
                {
                    let library = dirs.data_dir().join("layouts");
                    if std::fs::create_dir_all(&library).is_ok() {
                        dialog = dialog.set_directory(library);
                    }
                }
                let Some(mut path) = dialog.save_file() else {
                    return false;
                };
                if path.extension().is_none() {
                    path.set_extension("ovl.json");
                }
                path
            }
        };
        if let Err(e) = editor.save_to(&path) {
            editor.error = Some(format!("Cannot save layout: {e}"));
            return false;
        }
        let layout = Arc::new(editor.draft.clone());
        self.set_layout(layout);
        prefs::remember(&mut self.prefs.recent_layouts, path.clone());
        self.prefs.last_layout = Some(path);
        self.prefs.last_builtin = None;
        self.save_prefs();
        true
    }

    fn export_layout(&mut self) {
        let Some(editor) = &mut self.editor else {
            return;
        };
        let name = editor.draft.name.as_deref().unwrap_or("Untitled");
        let name: String = name
            .chars()
            .map(|c| if "/\\:*?\"<>|".contains(c) { '_' } else { c })
            .collect();
        let Some(path) = rfd::FileDialog::new()
            .set_title(crate::i18n::native_text("Export layout package"))
            .add_filter(
                crate::i18n::native_text("ActionLay layout package"),
                &["actionlay-layout"],
            )
            .set_file_name(format!("{name}.actionlay-layout"))
            .save_file()
        else {
            return;
        };
        if editor.invalid_parameters() {
            editor.error = Some("Fix invalid widget parameters before exporting".into());
            return;
        }
        if editor.include_fonts && !rfd::MessageDialog::new()
            .set_title(crate::i18n::native_text("Include custom fonts"))
            .set_description("Only share fonts you are licensed to redistribute. Including fonts embeds their files in the layout package; it does not install them.")
            .set_buttons(rfd::MessageButtons::OkCancel)
            .show().eq(&rfd::MessageDialogResult::Ok) { return; }
        let mut layout = editor.draft.clone();
        let result = (|| -> Result<(), String> {
            if editor.include_fonts {
                for warning in actionlay_render::fonts::embed_used(&mut layout)? {
                    log::warn!("{warning}");
                }
            } else {
                actionlay_render::fonts::omit_fonts(&mut layout)?;
            }
            let path = if path.extension().is_none() {
                path.with_extension("actionlay-layout")
            } else {
                path
            };
            actionlay_layout::package::save(&layout, &path).map_err(|e| e.to_string())
        })();
        editor.error = result.err();
    }

    fn import_layout_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title(crate::i18n::native_text("Import into layout library"))
            .add_filter(
                crate::i18n::native_text("ActionLay layouts"),
                &["actionlay-layout", "json", "xml"],
            )
            .pick_file()
        {
            if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("xml"))
            {
                self.open_layout(path);
            } else {
                match layouts::import_package(&path) {
                    Ok(path) => self.open_layout(path),
                    Err(error) => self.error = Some(error),
                }
            }
        }
    }

    fn request_edit_action(&mut self, action: EditAction, ctx: &egui::Context) {
        if self.pending_edit_action.is_some() {
            return;
        }
        if self.editor.as_ref().is_some_and(editor::Editor::dirty) {
            self.pending_edit_action = Some(action);
        } else {
            self.finish_edit_action(action, ctx);
        }
    }

    fn finish_edit_action(&mut self, action: EditAction, ctx: &egui::Context) {
        self.editor = None;
        match action {
            EditAction::Exit => {}
            EditAction::New => self.begin_edit(true),
            EditAction::Command(command) => self.command(command, ctx),
            EditAction::OpenLayout(path) => self.open_layout(path),
            EditAction::Builtin(id) => {
                if let Some(preset) = actionlay_layout::catalog::find(&id) {
                    self.set_layout(Arc::new(preset.layout()));
                    self.prefs.last_layout = None;
                    self.prefs.last_builtin = Some(id);
                    self.layout_notice = None;
                    self.save_prefs();
                    self.select_layout = false;
                }
            }
        }
    }

    fn edit_confirmation(&mut self, ctx: &egui::Context) {
        if self.pending_edit_action.is_none() {
            return;
        }
        let mut decision = None;
        let save_label = if matches!(
            self.pending_edit_action,
            Some(EditAction::Exit | EditAction::Command(menus::Command::Quit))
        ) {
            "Save and exit"
        } else {
            "Save and continue"
        };
        let response = egui::Modal::new(egui::Id::new("unsaved-layout")).show(ctx, |ui| {
            ui.heading(crate::i18n::ui_text(ui, "Save layout changes?"));
            ui.label(crate::i18n::ui_text(ui, "Your layout has unsaved changes."));
            if let Some(e) = self.editor.as_ref().and_then(|e| e.error.as_deref()) {
                ui.colored_label(egui::Color32::LIGHT_RED, crate::i18n::ui_text(ui, e));
            }
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::ui_text(ui, save_label)).clicked() {
                    decision = Some(0);
                }
                if ui
                    .button(crate::i18n::ui_text(ui, "Discard changes"))
                    .clicked()
                {
                    decision = Some(1);
                }
                if ui.button(crate::i18n::ui_text(ui, "Cancel")).clicked() {
                    decision = Some(2);
                }
            });
        });
        if response.should_close() {
            decision = Some(2);
        }
        match decision {
            Some(0) if self.save_edit(false) => {
                let action = self.pending_edit_action.take().unwrap();
                self.finish_edit_action(action, ctx);
            }
            Some(1) => {
                let action = self.pending_edit_action.take().unwrap();
                self.finish_edit_action(action, ctx);
            }
            Some(2) => self.pending_edit_action = None,
            _ => {}
        }
    }
    fn audio_dialog(&mut self, ctx: &egui::Context) {
        if !self.select_audio {
            return;
        }
        let mut open = true;
        let mut selected = self.prefs.audio_device.clone();
        egui::Window::new(crate::i18n::text("Audio output"))
            .open(&mut open)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.selectable_value(
                    &mut selected,
                    None,
                    crate::i18n::ui_text(ui, "System default"),
                );
                for name in &self.audio_devices {
                    ui.selectable_value(
                        &mut selected,
                        Some(name.clone()),
                        crate::i18n::user_text(ui, name),
                    );
                }
                if let Some(name) = &selected
                    && !self.audio_devices.contains(name)
                {
                    ui.colored_label(
                        egui::Color32::YELLOW,
                        crate::i18n::ui_text(ui, format!("Unavailable: {name}")),
                    );
                }
                if ui
                    .button(crate::i18n::ui_text(ui, "Refresh devices"))
                    .clicked()
                {
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
        ui.collapsing(
            crate::i18n::ui_text(ui, "Appearance · applies to all layouts"),
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::ui_text(ui, "Accent"));
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
                    .add(
                        egui::Slider::new(&mut opacity, 0.0..=1.0)
                            .text(crate::i18n::ui_text(ui, "Panel opacity")),
                    )
                    .changed()
                {
                    appearance.panel_opacity = Some(opacity);
                    changed = true;
                }
                ui.small(crate::i18n::ui_text(
                    ui,
                    "Widget sizes follow the video when the window is resized.",
                ));
                reset = ui
                    .button(crate::i18n::ui_text(ui, "Reset appearance"))
                    .clicked();
            },
        );
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
        egui::Window::new(crate::i18n::text("Import XML layout"))
            .open(&mut open)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label(crate::i18n::user_text(
                    ui,
                    path.file_name().unwrap_or_default().to_string_lossy(),
                ));
                ui.label(crate::i18n::ui_text(ui, "Reference video resolution"));
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut size[0])
                            .range(1..=16384)
                            .prefix(crate::i18n::ui_text(ui, "Width ")),
                    );
                    ui.add(
                        egui::DragValue::new(&mut size[1])
                            .range(1..=16384)
                            .prefix(crate::i18n::ui_text(ui, "Height ")),
                    );
                });
                import = ui
                    .button(crate::i18n::ui_text(ui, "Import into layout library"))
                    .clicked();
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
        ui.collapsing(crate::i18n::ui_text(ui, "Map service"),|ui| {
            changed|=ui.checkbox(&mut maps.online,crate::i18n::ui_text(ui, "Download visible map tiles")).changed();
            ui.small(crate::i18n::ui_text(ui, "Cached tiles remain available with downloads disabled."));
            ui.horizontal(|ui|{
                for (name,url,attr) in [("OSM","https://tile.openstreetmap.org/{z}/{x}/{y}.png","© OpenStreetMap contributors"),
                    ("CyclOSM","https://a.tile-cyclosm.openstreetmap.fr/cyclosm/{z}/{x}/{y}.png","© CyclOSM · OpenStreetMap contributors"),
                    ("Thunderforest","https://a.tile.thunderforest.com/cycle/{z}/{x}/{y}.png?apikey={api_key}","© Thunderforest · OpenStreetMap contributors"),
                    ("Geoapify","https://maps.geoapify.com/v1/tile/osm-bright/{z}/{x}/{y}.png?apiKey={api_key}","© Geoapify · OpenStreetMap contributors")] {
                    if ui.small_button(crate::i18n::ui_text(ui, name)).clicked(){maps.url=url.into();maps.attribution=attr.into();changed=true;}
                }
            });
            ui.label(crate::i18n::ui_text(ui, "Tile URL ({z}, {x}, {y}, optional {api_key})"));changed|=ui.text_edit_singleline(&mut maps.url).changed();
            ui.label(crate::i18n::ui_text(ui, "API key"));changed|=ui.add(egui::TextEdit::singleline(&mut maps.api_key).password(true)).changed();
            ui.label(crate::i18n::ui_text(ui, "Attribution"));changed|=ui.text_edit_singleline(&mut maps.attribution).changed();
            if !maps.valid(){ui.colored_label(egui::Color32::YELLOW,crate::i18n::ui_text(ui, "Downloads wait for a valid URL and attribution."));}
        });
        if changed {
            self.overlay.maps().configure(maps.clone());
            self.prefs.maps = Some(maps);
            self.save_prefs();
        }
    }

    fn layout_toolbar(&mut self, ui: &mut egui::Ui) {
        let mut command = None;
        let mut builtin = None;
        let mut recent = None;
        let selected = self
            .prefs
            .last_builtin
            .as_deref()
            .or_else(|| self.prefs.last_layout.is_none().then_some("default"))
            .and_then(actionlay_layout::catalog::find)
            .map(|preset| preset.name)
            .unwrap_or_else(|| self.layout.name.as_deref().unwrap_or("Layout"));
        ui.add_enabled_ui(self.pending_edit_action.is_none(), |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(crate::i18n::ui_text(ui, "Open video…"))
                    .on_hover_text(crate::i18n::ui_text(ui, "Open a video file"))
                    .clicked()
                {
                    command = Some(menus::Command::OpenVideo);
                }
                if ui
                    .add_enabled(
                        self.video_path.is_some(),
                        egui::Button::new(crate::i18n::text("Export...")),
                    )
                    .clicked()
                {
                    command = Some(menus::Command::ExportVideo);
                }
                ui.separator();
                ui.label(crate::i18n::ui_text(ui, "Layout"));
                egui::ComboBox::from_id_salt("toolbar-layout")
                    .selected_text(if self.prefs.last_layout.is_some() {
                        crate::i18n::user_text(ui, selected)
                    } else {
                        crate::i18n::ui_text(ui, selected)
                    })
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        ui.weak(crate::i18n::ui_text(ui, "Included presets"));
                        for preset in actionlay_layout::catalog::PRESETS {
                            let active = self.prefs.last_layout.is_none()
                                && self.prefs.last_builtin.as_deref().unwrap_or("default")
                                    == preset.id;
                            if ui
                                .selectable_label(active, crate::i18n::ui_text(ui, preset.name))
                                .on_hover_text(crate::i18n::ui_text(ui, preset.description))
                                .clicked()
                            {
                                builtin = Some(preset.id);
                                ui.close();
                            }
                        }
                        if !self.prefs.recent_layouts.is_empty() {
                            ui.separator();
                            ui.weak(crate::i18n::ui_text(ui, "Recent layouts"));
                            for path in &self.prefs.recent_layouts {
                                let name = path.file_name().unwrap_or_default().to_string_lossy();
                                if ui
                                    .selectable_label(
                                        self.prefs.last_layout.as_ref() == Some(path),
                                        crate::i18n::user_text(ui, name),
                                    )
                                    .on_hover_text(crate::i18n::user_text(
                                        ui,
                                        path.display().to_string(),
                                    ))
                                    .clicked()
                                {
                                    recent = Some(path.clone());
                                    ui.close();
                                }
                            }
                        }
                        ui.separator();
                        if ui
                            .button(crate::i18n::ui_text(ui, "Open layout from file…"))
                            .clicked()
                        {
                            command = Some(menus::Command::OpenLayoutFile);
                            ui.close();
                        }
                        if ui
                            .button(crate::i18n::ui_text(ui, "More layouts and settings…"))
                            .clicked()
                        {
                            command = Some(menus::Command::SelectLayout);
                            ui.close();
                        }
                    });
                ui.separator();
                if ui.button(crate::i18n::ui_text(ui, "Edit layout")).clicked() {
                    command = Some(menus::Command::EditLayout);
                }
                if ui.button(crate::i18n::ui_text(ui, "New layout…")).clicked() {
                    command = Some(menus::Command::NewLayout);
                }
                ui.separator();
                let mute_changed = ui
                    .selectable_label(
                        self.muted,
                        crate::i18n::ui_text(ui, if self.muted { "Unmute" } else { "Mute" }),
                    )
                    .on_hover_text(crate::i18n::ui_text(
                        ui,
                        "Mute audio without pausing playback",
                    ))
                    .clicked();
                if mute_changed {
                    self.muted = !self.muted;
                }
                let volume_changed = ui
                    .add(
                        egui::Slider::new(&mut self.volume, 0.0..=1.0)
                            .text(crate::i18n::ui_text(ui, "Volume"))
                            .custom_formatter(|value, _| format!("{:.0}%", value * 100.0)),
                    )
                    .changed();
                if (mute_changed || volume_changed)
                    && let Some(player) = &self.player
                {
                    player.set_volume(if self.muted { 0.0 } else { self.volume });
                }
            });
        });
        if let Some(id) = builtin {
            self.request_edit_action(EditAction::Builtin(id.into()), ui.ctx());
        } else if let Some(path) = recent {
            self.open_layout(path);
        } else if let Some(command) = command {
            self.command(command, ui.ctx());
        }
    }

    fn privacy_dialog(&mut self, ctx: &egui::Context) {
        if !self.select_privacy {
            return;
        }
        let mut visible = true;
        let mut maps = self.prefs.maps.clone().unwrap_or_default();
        let mut changed = false;
        egui::Window::new(crate::i18n::text("Privacy zones"))
            .open(&mut visible)
            .default_width(600.0)
            .show(ctx, |ui| {
                ui.label(crate::i18n::ui_text(ui, "Hide private places, such as your home or workplace, on map overlays. Inside each zone, the position marker and route are hidden."));
                ui.small(crate::i18n::ui_text(ui, "Your zones are saved in user configuration and apply to every video and layout."));
                ui.separator();
                ui.label(crate::i18n::ui_text(ui, "Set the center using latitude and longitude, then choose a radius in meters. You can also use the GPS position at the current video frame."));
                ui.small(crate::i18n::text("This affects map overlays only. The video image, original GPS data and telemetry shown by other widgets remain available."));
                ui.separator();
                let mut remove = None;
                for (i, zone) in maps.privacy.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut zone.lat)
                                    .speed(0.0001)
                                    .range(-85.0..=85.0)
                                    .prefix(crate::i18n::ui_text(ui, "Lat ")),
                            )
                            .changed();
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut zone.lon)
                                    .speed(0.0001)
                                    .range(-180.0..=180.0)
                                    .prefix(crate::i18n::ui_text(ui, "Lon ")),
                            )
                            .changed();
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut zone.radius_m)
                                    .range(1.0..=100000.0)
                                    .prefix(crate::i18n::ui_text(ui, "Radius "))
                                    .suffix(crate::i18n::ui_text(ui, " m")),
                            )
                            .changed();
                        if ui.small_button(crate::i18n::ui_text(ui, "Remove")).clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    maps.privacy.remove(i);
                    changed = true;
                }
                if ui.button(crate::i18n::ui_text(ui, "Add zone")).clicked() {
                    maps.privacy.push(actionlay_maps::PrivacyZone {
                        lat: 0.0,
                        lon: 0.0,
                        radius_m: 250.0,
                    });
                    changed = true;
                }
                let pos = self
                    .loaded_telemetry
                    .as_ref()
                    .map(|t| t.sample(self.shown_t.unwrap_or(0.0)))
                    .and_then(|s| {
                        s.get(actionlay_telemetry::Metric::Lat)
                            .present()
                            .zip(s.get(actionlay_telemetry::Metric::Lon).present())
                    });
                if ui
                    .add_enabled(
                        pos.is_some(),
                        egui::Button::new(crate::i18n::text("Add zone at current position")),
                    )
                    .clicked()
                    && let Some((lat, lon)) = pos
                {
                    maps.privacy.push(actionlay_maps::PrivacyZone {
                        lat,
                        lon,
                        radius_m: 250.0,
                    });
                    changed = true;
                }
            });
        self.select_privacy = visible;
        if changed {
            self.overlay.maps().configure(maps.clone());
            self.prefs.maps = Some(maps);
            self.save_prefs();
        }
    }

    fn maps_dialog(&mut self, ctx: &egui::Context) {
        if !self.select_maps {
            return;
        }
        let mut visible = true;
        egui::Window::new(crate::i18n::text("Maps"))
            .open(&mut visible)
            .default_width(580.0)
            .show(ctx, |ui| {
                ui.label(crate::i18n::ui_text(
                    ui,
                    "User preferences · apply to every video and layout",
                ));
                self.map_controls(ui);
            });
        self.select_maps = visible;
    }

    fn save_sources(&mut self) {
        if let Some(path) = &self.video_path {
            self.prefs
                .video_sources
                .insert(prefs::video_identity(path), self.source_settings.clone());
            self.save_prefs();
        }
    }
    fn link_activity(&mut self, path: PathBuf) {
        if self.player.is_none() {
            self.error = Some("Open a video before linking an activity".into());
            return;
        }
        self.activity = None;
        self.refresh_sources();
        let ctx = self.egui_ctx.clone();
        self.activity_rx = Some(telemetry_load::spawn_activity(path.clone(), move || {
            ctx.request_repaint()
        }));
        self.source_settings.activity = Some(path);
        self.source_notice = Some("Reading activity…".into());
    }
    fn poll_activity(&mut self) {
        let Some(rx) = &self.activity_rx else {
            return;
        };
        let Ok(result) = rx.try_recv() else {
            return;
        };
        self.activity_rx = None;
        match result {
            Ok(activity) => {
                self.activity = Some(activity);
                self.refresh_sources();
                self.save_sources();
            }
            Err(e) => {
                self.source_notice = Some(e);
            }
        }
    }
    fn refresh_sources(&mut self) {
        let Some(player) = &self.player else {
            return;
        };
        let duration = player.info().duration;
        let mut tel = self
            .camera_telemetry
            .as_ref()
            .map_or_else(|| Telemetry::empty(duration), |t| (**t).clone());
        if let Some(activity) = &self.activity {
            let origin = if self.source_settings.video_utc.trim().is_empty() {
                tel.start_utc()
            } else {
                chrono::DateTime::parse_from_rfc3339(self.source_settings.video_utc.trim())
                    .ok()
                    .map(|u| u.with_timezone(&chrono::Utc))
            };
            match activity.align(origin, duration, self.source_settings.offset) {
                Ok(external) => {
                    tel = tel.merge_external(&external, duration);
                    self.source_notice =
                        Some(format!("{} activity samples linked", activity.points.len()));
                }
                Err(e) => self.source_notice = Some(e.to_string()),
            }
        }
        let tel = Arc::new(tel);
        self.loaded_telemetry = Some(tel.clone());
        self.overlay.set_telemetry(Some(tel), self.scale_mode);
        self.telemetry_rev += 1;
    }
    fn sources_dialog(&mut self, ctx: &egui::Context) {
        if !self.select_sources {
            return;
        }
        let mut visible = true;
        let mut link = false;
        let mut unlink = false;
        let mut changed = false;
        let mut mode_changed = false;
        egui::Window::new(crate::i18n::text("Video sources")).open(&mut visible).default_width(550.0).show(ctx, |ui| {
            let Some(player) = &self.player else { ui.label(crate::i18n::ui_text(ui, "Open a video to link sources.")); return; };
            ui.label(crate::i18n::ui_text(ui, format!("{} video chapter(s)",player.timeline().chapters.len())));
            mode_changed = ui.checkbox(&mut self.source_settings.open_alone,crate::i18n::ui_text(ui, "Open this file alone (disable automatic chapters)")).changed();
            ui.add_enabled_ui(!self.source_settings.open_alone, |ui| {
                mode_changed |= ui.checkbox(&mut self.source_settings.load_sequence,crate::i18n::ui_text(ui, "Load the complete GoPro sequence from its first chapter")).changed();
            });
            ui.separator();
            if let Some(path) = &self.source_settings.activity { ui.label(crate::i18n::user_text(ui, path.display().to_string())); }
            ui.horizontal(|ui| { link = ui.button(crate::i18n::ui_text(ui, "Link GPX/FIT…")).clicked(); unlink = ui.add_enabled(self.source_settings.activity.is_some(),egui::Button::new(crate::i18n::text("Unlink"))).clicked(); });
            changed |= ui.add(egui::DragValue::new(&mut self.source_settings.offset).speed(0.1).suffix(crate::i18n::ui_text(ui, " s")).prefix(crate::i18n::ui_text(ui, "Activity offset "))).changed();
            ui.small(crate::i18n::ui_text(ui, "Positive offset moves activity data later in the video."));
            ui.label(crate::i18n::ui_text(ui, "UTC of the first video frame (optional if the camera has GPS)"));
            changed |= ui.add(egui::TextEdit::singleline(&mut self.source_settings.video_utc).hint_text(crate::i18n::ui_text(ui, "2026-09-27T12:15:30Z"))).changed();
            if let Some(utc) = self.camera_telemetry.as_ref().and_then(|t|t.start_utc()) { ui.small(crate::i18n::ui_text(ui, format!("Camera UTC: {utc}"))); }
            if let Some(notice) = &self.source_notice { ui.label(crate::i18n::ui_text(ui, notice)); }
            ui.small(crate::i18n::ui_text(ui, "Links and offsets are remembered for this video. External data fills available metrics; camera data fills its gaps."));
        });
        self.select_sources = visible;
        if link
            && let Some(path) = rfd::FileDialog::new()
                .set_title(crate::i18n::native_text("Link activity"))
                .add_filter(crate::i18n::native_text("Activity"), &["gpx", "fit"])
                .pick_file()
        {
            self.link_activity(path);
        }
        if unlink {
            self.activity = None;
            self.activity_rx = None;
            self.source_settings.activity = None;
            self.source_notice = None;
            changed = true;
        }
        if changed {
            self.refresh_sources();
            self.save_sources();
        }
        if mode_changed {
            self.save_sources();
            if let Some(path) = self.video_path.clone() {
                self.open(path);
            }
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
        let mut import = false;
        egui::Window::new(crate::i18n::text("Select Layout"))
            .open(&mut visible)
            .collapsible(false)
            .resizable(true)
            .default_width(580.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(650.0)
                    .show(ui, |ui| {
                        ui.heading(crate::i18n::ui_text(ui, "Included layouts"));
                        for preset in actionlay_layout::catalog::PRESETS {
                            let active = self.prefs.last_layout.is_none()
                                && self.prefs.last_builtin.as_deref().unwrap_or("default")
                                    == preset.id;
                            if ui
                                .selectable_label(active, crate::i18n::ui_text(ui, preset.name))
                                .clicked()
                            {
                                builtin = Some(preset.id);
                            }
                            ui.small(crate::i18n::ui_text(ui, preset.description));
                        }
                        ui.collapsing(crate::i18n::ui_text(ui, "Upstream layout library"), |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(240.0)
                                .show(ui, |ui| {
                                    for preset in actionlay_layout::catalog::UPSTREAM_PRESETS {
                                        if ui
                                            .selectable_label(
                                                self.prefs.last_builtin.as_deref()
                                                    == Some(preset.id),
                                                crate::i18n::ui_text(ui, preset.name),
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
                        ui.separator();
                        ui.collapsing(crate::i18n::ui_text(ui, "Imported layouts"), |ui| {
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
                                                && (p
                                                    .to_string_lossy()
                                                    .to_ascii_lowercase()
                                                    .ends_with(".ovl.json")
                                                    || actionlay_layout::package::is_package(p))
                                        })
                                        .collect();
                                files.sort();
                                for path in files {
                                    let name =
                                        path.file_name().unwrap_or_default().to_string_lossy();
                                    if ui
                                        .selectable_label(
                                            self.prefs.last_layout.as_ref() == Some(&path),
                                            crate::i18n::user_text(ui, name),
                                        )
                                        .clicked()
                                    {
                                        selected = Some(path);
                                    }
                                }
                            }
                        });
                        ui.heading(crate::i18n::ui_text(ui, "Recent layouts"));
                        egui::ScrollArea::vertical()
                            .max_height(280.0)
                            .show(ui, |ui| {
                                if self.prefs.recent_layouts.is_empty() {
                                    ui.weak(crate::i18n::ui_text(
                                        ui,
                                        "No layouts loaded from file yet.",
                                    ));
                                }
                                for path in &self.prefs.recent_layouts {
                                    let name =
                                        path.file_name().unwrap_or_default().to_string_lossy();
                                    if ui
                                        .selectable_label(
                                            self.prefs.last_layout.as_ref() == Some(path),
                                            crate::i18n::user_text(ui, name),
                                        )
                                        .on_hover_text(crate::i18n::user_text(
                                            ui,
                                            path.display().to_string(),
                                        ))
                                        .clicked()
                                    {
                                        selected = Some(path.clone());
                                    }
                                    ui.small(crate::i18n::user_text(
                                        ui,
                                        path.display().to_string(),
                                    ));
                                }
                            });
                        ui.separator();
                        browse = ui
                            .button(crate::i18n::ui_text(ui, "Open layout from file…"))
                            .clicked();
                        import = ui
                            .button(crate::i18n::ui_text(ui, "Import layout into library…"))
                            .clicked();
                    });
            });
        self.select_layout = visible;
        if let Some(id) = builtin {
            self.request_edit_action(EditAction::Builtin(id.into()), ctx);
        } else if let Some(path) = selected {
            self.open_layout(path);
            // Keep the chooser available if a recent file has moved or is invalid.
            self.select_layout = self.error.is_some();
        } else if browse {
            self.command(menus::Command::OpenLayoutFile, ctx);
        } else if import {
            self.import_layout_file();
        }
    }

    fn command(&mut self, command: menus::Command, ctx: &egui::Context) {
        if self.pending_edit_action.is_some() {
            return;
        }
        if self.editor.is_some()
            && matches!(
                command,
                menus::Command::Quit
                    | menus::Command::SelectLayout
                    | menus::Command::OpenLayoutFile
                    | menus::Command::NewLayout
            )
        {
            self.request_edit_action(EditAction::Command(command), ctx);
            return;
        }
        match command {
            #[cfg(not(target_os = "macos"))]
            menus::Command::FileAssociations => self.integration.open = true,
            menus::Command::ExportVideo => {
                if self.video_path.is_some()
                    && self
                        .export_job
                        .as_ref()
                        .is_none_or(|j| j.progress.lock().unwrap().done)
                {
                    if let Some(p) = &mut self.player {
                        p.pause();
                    }
                    let duration = self.player.as_ref().map_or(0.0, |p| p.info().duration);
                    self.export_dialog = Some(export_ui::Dialog::new(
                        self.prefs.export.clone().unwrap_or_default(),
                        duration,
                    ));
                }
            }
            menus::Command::EditLayout => self.begin_edit(false),
            menus::Command::NewLayout => self.begin_edit(true),
            menus::Command::SaveLayout => {
                self.save_edit(false);
            }
            menus::Command::SaveLayoutAs => {
                self.save_edit(true);
            }
            menus::Command::ExportLayout => self.export_layout(),
            menus::Command::ExitEditor => self.request_edit_action(EditAction::Exit, ctx),
            menus::Command::SelectLayout => self.select_layout = true,
            menus::Command::MapSettings => self.select_maps = true,
            menus::Command::PrivacySettings => self.select_privacy = true,
            menus::Command::InterfaceSettings => self.interface.visible = true,
            menus::Command::RegionalSettings => self.regional_visible = true,
            menus::Command::AdvancedSettings => self.advanced_visible = true,
            menus::Command::Sources => self.select_sources = self.player.is_some(),
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
                        .set_title(crate::i18n::native_text("Open Layout"))
                        .add_filter(
                            crate::i18n::native_text("Overlay layout"),
                            &["actionlay-layout", "json", "xml"],
                        )
                } else {
                    rfd::FileDialog::new()
                        .set_title(crate::i18n::native_text("Open Video"))
                        .add_filter(
                            crate::i18n::native_text("Video"),
                            &[
                                "mp4", "mov", "m4v", "mkv", "avi", "webm", "mts", "m2ts", "insv",
                                "lrv",
                            ],
                        )
                }
                .add_filter(crate::i18n::native_text("All files"), &["*"]);
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
                self.route_requested_until = 0.0;
                self.loaded_telemetry = None;
                self.camera_telemetry = None;
                self.activity = None;
                self.activity_rx = None;
                self.source_notice = None;
                self.chapter_offer = None;
                self.video_notice = None;
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
        let settings = self
            .prefs
            .video_sources
            .get(&prefs::video_identity(&path))
            .cloned()
            .unwrap_or_default();
        let first = actionlay_media::chapters::is_first_chapter(&path);
        let join = !settings.open_alone && (first || settings.load_sequence);
        let offer = if !first && !settings.open_alone && !settings.load_sequence {
            let chapters = actionlay_media::chapters::discover(&path);
            (chapters.len() > 1).then_some(chapters.len())
        } else {
            None
        };
        match Player::open_mode(
            &path,
            PlayerOptions {
                buffering: self.prefs.buffering,
                ..Default::default()
            },
            self.prefs.audio_device.as_deref(),
            join,
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
                p.set_volume(if self.muted { 0.0 } else { self.volume });
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
                self.route_requested_until = 0.0;
                self.loaded_telemetry = None;
                self.camera_telemetry = None;
                self.activity = None;
                self.activity_rx = None;
                self.source_notice = None;
                self.video_notice = self.player.as_ref().and_then(|p| {
                    (p.info().audio.is_some() && !p.stats().audio_active).then(|| {
                        "Audio output unavailable; playback is silent. Select a device in Settings → Audio.".into()
                    })
                });
                self.shown_t = None;
                self.overlay_ready = false;
                self.overlay_ms = None;
                self.scheduler.reset();
                self.overlay
                    .set_telemetry(Some(Arc::new(Telemetry::empty(duration))), self.scale_mode);
                self.telemetry_rev += 1;
                self.source_settings = settings;
                self.chapter_offer = offer;
                let ctx = self.egui_ctx.clone();
                self.telemetry_rx = metadata.and_then(|meta| {
                    self.player.as_mut().unwrap().take_telemetry().map(|rx| {
                        telemetry_load::spawn(rx, duration, meta.packet_count, move || {
                            ctx.request_repaint()
                        })
                    })
                });
                if self.telemetry_rx.is_none() {
                    let ctx = self.egui_ctx.clone();
                    self.telemetry_rx = Some(telemetry_load::spawn_camera(
                        path.clone(),
                        duration,
                        move || ctx.request_repaint(),
                    ));
                }
                if let Some(activity) = self.source_settings.activity.clone() {
                    self.link_activity(activity);
                }
            }
            Err(e) => self.error = Some(format!("{}: {e}", path.display())),
        }
    }

    fn open_layout(&mut self, path: PathBuf) {
        if self.editor.is_some() {
            let ctx = self.egui_ctx.clone();
            self.request_edit_action(EditAction::OpenLayout(path), &ctx);
            return;
        }
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
        let path = if actionlay_layout::package::is_package(&path) {
            match layouts::import_package(&path) {
                Ok(path) => path,
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            }
        } else {
            path
        };
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

    fn request_route_metadata(&mut self) {
        if self
            .editor
            .as_ref()
            .map_or(!self.overlay_visible, |editor| !editor.video_background)
        {
            return;
        }
        use actionlay_layout::model::{MapRoute, Node, Widget};
        fn route(nodes: &[Node]) -> MapRoute {
            let mut result = MapRoute::None;
            for node in nodes {
                if let Node::Known(widget) = node {
                    if widget.common().visible == Some(false) {
                        continue;
                    }
                    let mode = match widget {
                        Widget::Map(map) if map.needs_full_track() => MapRoute::Full,
                        Widget::Map(map) => map.route_mode.unwrap_or_default(),
                        _ => route(widget.children()),
                    };
                    if mode == MapRoute::Full {
                        return mode;
                    }
                    if mode == MapRoute::Past {
                        result = mode;
                    }
                }
            }
            result
        }
        let (Some(player), Some(load)) = (&self.player, &self.telemetry_rx) else {
            return;
        };
        let nodes = self
            .editor
            .as_ref()
            .map_or(&self.layout.nodes, |editor| &editor.draft.nodes);
        let route_mode = route(nodes);
        let end = match route_mode {
            MapRoute::None => return,
            MapRoute::Full => player.info().duration,
            MapRoute::Past => self.shown_t.unwrap_or(0.0),
        };
        if end <= self.route_requested_until + 0.001 {
            return;
        }
        // Full-route zoom needs a validated complete source. Past-only drawing
        // can reuse the played prefix and backfill just its unread gaps.
        if self.camera_telemetry.as_ref().is_some_and(|t| {
            if route_mode == MapRoute::Full {
                t.is_complete()
            } else {
                t.is_loaded_through(end)
            }
        }) {
            return;
        }
        let Some(telemetry) = &self.camera_telemetry else {
            return;
        };
        let ranges = if route_mode == MapRoute::Full {
            vec![(0.0, end)]
        } else {
            telemetry
                .unread_ranges(end)
                .into_iter()
                .filter_map(|(start, stop)| {
                    let start = start.max(self.route_requested_until);
                    (stop > start + 0.001).then_some((start, stop))
                })
                .collect::<Vec<_>>()
        };
        if !ranges.is_empty() && load.request_route(player.timeline().clone(), ranges) {
            self.route_requested_until = end;
        }
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
        self.camera_telemetry = Some(telemetry);
        self.refresh_sources();
    }

    fn poll_overlay(&mut self) {
        if let Some(frame) = self.overlay.take_frame() {
            if self.overlay_ready
                && self
                    .loaded_telemetry
                    .as_ref()
                    .is_some_and(|tel| !tel.is_loaded_at(frame.t))
            {
                self.overlay.recycle(frame.pixmap);
                return;
            }
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
        // Unread metadata is buffering, not a missing GPS sample. Keep the last
        // displayed overlay until this video timestamp has actually been decoded.
        if self.overlay_ready
            && self
                .loaded_telemetry
                .as_ref()
                .is_some_and(|tel| !tel.is_loaded_at(t))
        {
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
    fn on_exit(&mut self) {
        self.save_prefs();
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if self.window.capture(frame, &mut self.prefs.window) {
            self.save_prefs();
        }
        ui.ctx().request_repaint_after(Duration::from_millis(500));
        let can_fullscreen = self.player.is_some() && self.editor.is_none();
        self.window.sync_fullscreen(frame, ui.ctx(), can_fullscreen);
        if !can_fullscreen {
            self.window.set_fullscreen(ui.ctx(), false);
        }
        if can_fullscreen {
            let (toggle, exit) = ui.ctx().input_mut(|i| {
                let toggle = !i.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::Key {
                            key: egui::Key::F11,
                            repeat: true,
                            ..
                        }
                    )
                }) && i.consume_key(egui::Modifiers::NONE, egui::Key::F11);
                (
                    toggle,
                    self.window.fullscreen
                        && i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
                )
            });
            if (toggle && !ui.ctx().egui_wants_keyboard_input()) || (exit && self.window.fullscreen)
            {
                self.window
                    .set_fullscreen(ui.ctx(), !exit && !self.window.fullscreen);
            }
        }
        let fullscreen = self.window.fullscreen;
        let controls_visible =
            !fullscreen || self.window.controls_visible(ui.ctx(), self.scrub.dragging);
        let language_changed = self.interface.refresh(&self.prefs, ui.ctx());
        if self.prefs.regional_units.is_none() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs(1));
        }
        if !fullscreen
            && advanced::show(
                ui.ctx(),
                &mut self.advanced_visible,
                &mut self.prefs.buffering,
            )
        {
            if let Some(player) = &mut self.player {
                player.set_buffering(self.prefs.buffering);
            }
            self.save_prefs();
        }
        let regional_changed = !fullscreen
            && i18n::regional_settings(ui.ctx(), &mut self.regional_visible, &mut self.prefs);
        if actionlay_render::regional::configure(self.prefs.regional_units) || regional_changed {
            self.set_layout(self.base_layout.clone());
            self.save_prefs();
        }
        let settings_changed = !fullscreen && self.interface.show(ui.ctx(), &mut self.prefs);
        if settings_changed {
            self.save_prefs();
        }
        let selection_changed = settings_changed && i18n::activate(self.prefs.language.as_deref());
        if language_changed || selection_changed {
            i18n::install_fonts(ui.ctx());
            match menus::Menus::new(ui.ctx()) {
                Ok(menus) => self.menus = menus,
                Err(error) => log::warn!("cannot update interface menus: {error}"),
            }
        }
        #[cfg(not(target_os = "macos"))]
        if !fullscreen && self.integration.show(ui.ctx(), &mut self.prefs) {
            self.save_prefs();
        }
        #[cfg(target_os = "macos")]
        for path in integration::macos::take_files() {
            self.open(path);
        }
        if !fullscreen {
            self.show_export(ui.ctx());
        }
        if ui.ctx().input(|i| i.viewport().close_requested())
            && self
                .export_job
                .as_ref()
                .is_some_and(|j| !j.progress.lock().unwrap().done)
        {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.error = Some("Cancel the export and wait for it to finish before closing".into());
        }
        if ui.ctx().input(|i| i.viewport().close_requested())
            && self.editor.as_ref().is_some_and(editor::Editor::dirty)
        {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request_edit_action(EditAction::Command(menus::Command::Quit), ui.ctx());
        }
        if !fullscreen
            && let Some(command) = self.menus.show(
                ui,
                self.player.is_some(),
                self.editor.is_some(),
                &self.prefs,
            )
        {
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
                layouts::Dropped::Activity(p) => {
                    self.link_activity(p);
                    self.select_sources = true;
                }
            }
        }
        self.request_route_metadata();
        self.poll_telemetry();
        self.poll_activity();
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
            if !ui.ctx().egui_wants_keyboard_input()
                && self.pending_edit_action.is_none()
                && self.editor.as_ref().is_none_or(|e| e.video_background)
            {
                transport::handle_keys(ui.ctx(), p, &self.scrub);
            }
            if let Some(e) = p.take_error() {
                self.error = Some(e);
            }
            if let Some(frame) = p.poll_frame() {
                self.shown_t = Some(frame.pts);
                let color = p.info().video.color;
                self.view.upload(frame, color);
            }
        }
        self.poll_overlay();

        let status = self.overlay_status();
        let notices = self.notices();
        let mut load_sequence = false;
        let mut dismiss_chapters = false;
        let mut toggle_fullscreen = false;
        if !fullscreen {
            egui::Panel::bottom("transport").show(ui, |ui| {
                let enabled = self.pending_edit_action.is_none()
                    && self.editor.as_ref().is_none_or(|e| e.video_background);
                if let Some(p) = &mut self.player {
                    ui.add_enabled_ui(enabled, |ui| {
                        toggle_fullscreen = transport::show(
                            ui,
                            p,
                            &mut self.scrub,
                            &status,
                            self.prefs.show_diagnostic_data,
                            self.editor.is_none().then_some(false),
                        )
                    });
                }
                if let Some(count) = self.chapter_offer {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(crate::i18n::ui_text(
                        ui,
                        format!(
                            "This is an intermediate GoPro chapter. {count} chapters are available."
                        ),
                    ));
                        load_sequence = ui
                            .add_enabled(
                                enabled,
                                egui::Button::new(crate::i18n::text(
                                    "Load sequence from first chapter",
                                )),
                            )
                            .clicked();
                        dismiss_chapters = ui
                            .button(crate::i18n::ui_text(ui, "Keep this file"))
                            .clicked();
                    });
                }
                if let Some(e) = &self.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, crate::i18n::ui_text(ui, e));
                }
                if let Some(n) = &notices {
                    ui.small(crate::i18n::ui_text(ui, n));
                }
            });
        }
        if dismiss_chapters {
            self.chapter_offer = None;
        }
        if load_sequence {
            self.source_settings.load_sequence = true;
            self.source_settings.open_alone = false;
            self.save_sources();
            if let Some(path) = self.video_path.clone() {
                self.open(path);
            }
        }
        if !fullscreen {
            self.audio_dialog(ui.ctx());
            self.maps_dialog(ui.ctx());
            self.privacy_dialog(ui.ctx());
            self.sources_dialog(ui.ctx());
        }
        if let Some(mut editor) = self.editor.take() {
            editor.set_maps(self.overlay.maps().clone());
            let video_size = self
                .player
                .as_ref()
                .map(|p| [p.info().video.width, p.info().video.height]);
            let action = ui
                .add_enabled_ui(self.pending_edit_action.is_none(), |ui| {
                    editor.ui(
                        ui,
                        Some(&mut self.view),
                        video_size,
                        self.loaded_telemetry.as_deref(),
                        self.shown_t.unwrap_or(0.0),
                    )
                })
                .inner;
            self.editor = Some(editor);
            if self.editor.as_ref().is_some_and(|e| !e.video_background)
                && let Some(player) = &mut self.player
            {
                player.pause();
            }
            match action {
                Some(editor::Action::Save) => {
                    self.save_edit(false);
                }
                Some(editor::Action::SaveAs) => {
                    self.save_edit(true);
                }
                Some(editor::Action::Export) => self.export_layout(),
                Some(editor::Action::Exit) => self.request_edit_action(EditAction::Exit, ui.ctx()),
                Some(editor::Action::New) => self.request_edit_action(EditAction::New, ui.ctx()),
                None => {}
            }
        } else {
            if !fullscreen {
                egui::Panel::top("layout-toolbar").show(ui, |ui| {
                    self.layout_toolbar(ui);
                });
            }
            let mut open_clicked = false;
            let panel = if fullscreen {
                egui::CentralPanel::default().frame(egui::Frame::NONE.fill(egui::Color32::BLACK))
            } else {
                egui::CentralPanel::default()
            };
            panel.show(ui, |ui| {
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
                        crate::i18n::text("Open a video"),
                        egui::FontId::proportional(28.0),
                        ui.visuals().text_color(),
                    );
                    ui.painter().text(
                        rect.center() + egui::vec2(0.0, 8.0),
                        egui::Align2::CENTER_CENTER,
                        crate::i18n::text("Click here or drag a video into the window"),
                        egui::FontId::proportional(16.0),
                        ui.visuals().weak_text_color(),
                    );
                    ui.painter().text(
                        rect.center() + egui::vec2(0.0, 38.0),
                        egui::Align2::CENTER_CENTER,
                        if cfg!(target_os = "macos") {
                            crate::i18n::text("File / Open Video…   ·   ⌘O")
                        } else {
                            crate::i18n::text("File / Open Video…   ·   Ctrl+O")
                        },
                        egui::FontId::proportional(14.0),
                        ui.visuals().weak_text_color(),
                    );
                    open_clicked = response.clicked();
                    return;
                }
                if ui
                    .interact(
                        rect,
                        egui::Id::new("video-fullscreen"),
                        egui::Sense::click(),
                    )
                    .double_clicked()
                {
                    toggle_fullscreen = true;
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
        }
        if fullscreen && controls_visible {
            egui::Area::new(egui::Id::new("fullscreen-transport"))
                .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -16.0])
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_width((ui.ctx().content_rect().width() - 48.0).max(100.0));
                        if let Some(player) = &mut self.player {
                            toggle_fullscreen |= transport::show(
                                ui,
                                player,
                                &mut self.scrub,
                                &status,
                                self.prefs.show_diagnostic_data,
                                Some(true),
                            );
                        }
                    });
                });
        }
        if toggle_fullscreen {
            self.window.set_fullscreen(ui.ctx(), !fullscreen);
        }
        if fullscreen && !controls_visible && !toggle_fullscreen {
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
        }
        if !fullscreen {
            self.layout_chooser(ui.ctx());
            self.import_dialog(ui.ctx());
            self.edit_confirmation(ui.ctx());
        }
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
    #[cfg(target_os = "windows")]
    if std::env::args().nth(1).as_deref() == Some("export") {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn AttachConsole(process_id: u32) -> i32;
        }
        // SAFETY: attach to the invoking console if present; redirected handles
        // are retained by Windows. A missing parent console is harmless.
        unsafe {
            AttachConsole(u32::MAX);
        }
    }
    env_logger::init();
    let startup_preferences = prefs::default_path()
        .as_deref()
        .map(prefs::Prefs::load)
        .unwrap_or_default();
    actionlay_render::regional::configure(startup_preferences.regional_units);
    if std::env::args().nth(1).as_deref() == Some("export") {
        if let Err(error) = export_ui::cli() {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
        return Ok(());
    }
    let path = std::env::args().nth(1).map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let file_open_handler = integration::macos::install();
    eframe::run_native(
        "ActionLay",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([960.0, 640.0])
                .with_min_inner_size([720.0, 480.0])
                .with_icon(
                    eframe::icon_data::from_png_bytes(include_bytes!(
                        "../../../assets/icons/actionlay-256.png"
                    ))
                    .expect("embedded app icon"),
                ),
            centered: true,
            persist_window: false,
            ..Default::default()
        },
        Box::new(move |cc| {
            let rs = cc
                .wgpu_render_state
                .as_ref()
                .expect("wgpu renderer required");
            let max_texture = rs.device.limits().max_texture_dimension_2d;
            let prefs_path = prefs::default_path();
            let mut prefs = prefs_path
                .as_deref()
                .map(prefs::Prefs::load)
                .unwrap_or_default();
            let window = window::State::new(cc, &mut prefs.window);
            actionlay_render::regional::configure(prefs.regional_units);
            i18n::activate(prefs.language.as_deref());
            i18n::install_fonts(&cc.egui_ctx);
            let initial = layouts::initial(&prefs);
            let base_layout = Arc::new(initial.layout);
            let mut styled = (*base_layout).clone();
            layouts::apply_appearance(&mut styled, prefs.appearance.as_ref());
            let layout = Arc::new(styled);
            let ctx = cc.egui_ctx.clone();
            #[cfg(target_os = "macos")]
            integration::macos::attach(&ctx, &file_open_handler);
            let overlay = OverlayWorker::spawn(layout.clone(), move || ctx.request_repaint());
            overlay
                .maps()
                .configure(prefs.maps.clone().unwrap_or_default());
            let mut app = App {
                window,
                interface: Default::default(),
                regional_visible: false,
                advanced_visible: false,
                #[cfg(target_os = "macos")]
                _file_open_handler: file_open_handler,
                #[cfg(not(target_os = "macos"))]
                integration: integration::Dialog::startup(&prefs),
                export_dialog: None,
                export_job: None,
                editor: None,
                pending_edit_action: None,
                menus: menus::Menus::new(&cc.egui_ctx)?,
                video_path: None,
                title_dirty: false,
                select_layout: false,
                select_audio: false,
                select_maps: false,
                select_privacy: false,
                select_sources: false,
                camera_telemetry: None,
                activity: None,
                activity_rx: None,
                source_settings: Default::default(),
                source_notice: None,
                chapter_offer: None,
                audio_devices: Vec::new(),
                volume: 1.0,
                muted: false,
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
                route_requested_until: 0.0,
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
                match layouts::classify(path) {
                    layouts::Dropped::Layout(path) => app.open_layout(path),
                    layouts::Dropped::Video(path) => app.open(path),
                    layouts::Dropped::Activity(path) => app.link_activity(path),
                }
            }
            Ok(Box::new(app))
        }),
    )
}
