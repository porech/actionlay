//! Desktop commands shared by menus, keyboard shortcuts and the welcome screen.
use eframe::egui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    OpenVideo,
    SelectLayout,
    NewLayout,
    EditLayout,
    SaveLayout,
    SaveLayoutAs,
    ExportLayout,
    ExportVideo,
    ExitEditor,
    AudioSettings,
    MapSettings,
    PrivacySettings,
    InterfaceSettings,
    RegionalSettings,
    AdvancedSettings,
    #[cfg(not(target_os = "macos"))]
    FileAssociations,
    Sources,
    OpenLayoutFile,
    OpenRecentVideo(usize),
    ClearRecentVideos,
    CloseVideo,
    Quit,
}

#[cfg(target_os = "macos")]
pub struct Menus {
    // Keep the native menu and its items alive for the application's lifetime.
    _menu: muda::Menu,
    close: muda::MenuItem,
    export_video: muda::MenuItem,
    has_video: std::cell::Cell<bool>,
    editor_items: [muda::MenuItem; 4],
    edit: muda::MenuItem,
    recent: muda::Submenu,
    recent_paths: Option<Vec<std::path::PathBuf>>,
    commands: std::sync::mpsc::Receiver<Command>,
}

#[cfg(target_os = "macos")]
impl Menus {
    pub fn new(ctx: &egui::Context) -> muda::Result<Self> {
        use muda::accelerator::{Accelerator, Code, Modifiers};
        use muda::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};
        let key = |code, shift| {
            Some(Accelerator::new(
                Some(if shift {
                    Modifiers::META | Modifiers::SHIFT
                } else {
                    Modifiers::META
                }),
                code,
            ))
        };
        let open = MenuItem::with_id(
            "open-video",
            crate::i18n::native_text("Open Video…"),
            true,
            key(Code::KeyO, false),
        );
        let layout = MenuItem::with_id(
            "select-layout",
            crate::i18n::native_text("Select Layout…"),
            true,
            key(Code::KeyO, true),
        );
        let close = MenuItem::with_id(
            "close-video",
            crate::i18n::native_text("Close Video"),
            false,
            key(Code::KeyW, false),
        );
        let quit = MenuItem::with_id(
            "quit",
            crate::i18n::native_text("Quit ActionLay"),
            true,
            key(Code::KeyQ, false),
        );
        let app = Submenu::with_items(
            crate::i18n::native_text("ActionLay"),
            true,
            &[
                &PredefinedMenuItem::about(
                    Some(crate::i18n::native_text("About ActionLay")),
                    Some(AboutMetadata {
                        name: Some("ActionLay".into()),
                        version: Some(env!("CARGO_PKG_VERSION").into()),
                        ..Default::default()
                    }),
                ),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(Some(crate::i18n::native_text("Hide ActionLay"))),
                &PredefinedMenuItem::hide_others(Some(crate::i18n::native_text("Hide Others"))),
                &PredefinedMenuItem::show_all(Some(crate::i18n::native_text("Show All"))),
                &PredefinedMenuItem::separator(),
                &quit,
            ],
        )?;
        let recent = Submenu::new(crate::i18n::native_text("Recent Videos"), true);
        let export_video = MenuItem::with_id(
            "export-video",
            crate::i18n::native_text("Export Video…"),
            false,
            None,
        );
        let file = Submenu::with_items(
            crate::i18n::native_text("File"),
            true,
            &[
                &open,
                &recent,
                &layout,
                &export_video,
                &PredefinedMenuItem::separator(),
                &close,
            ],
        )?;
        let audio = MenuItem::with_id(
            "audio-settings",
            crate::i18n::native_text("Audio…"),
            true,
            None,
        );
        let maps = MenuItem::with_id(
            "map-settings",
            crate::i18n::native_text("Maps…"),
            true,
            None,
        );
        let sources = MenuItem::with_id(
            "sources",
            crate::i18n::native_text("Video sources…"),
            true,
            None,
        );
        file.append(&sources)?;
        let privacy = MenuItem::with_id(
            "privacy-settings",
            crate::i18n::native_text("Privacy zones…"),
            true,
            None,
        );
        let interface = MenuItem::with_id(
            "interface-settings",
            crate::i18n::native_text("Interface…"),
            true,
            None,
        );
        let regional = MenuItem::with_id(
            "regional-settings",
            crate::i18n::native_text("Regional settings…"),
            true,
            None,
        );
        let advanced = MenuItem::with_id(
            "advanced-settings",
            crate::i18n::native_text("Advanced…"),
            true,
            None,
        );
        let settings = Submenu::with_items(
            crate::i18n::native_text("Settings"),
            true,
            &[
                &audio,
                &maps,
                &privacy,
                &PredefinedMenuItem::separator(),
                &interface,
                &regional,
                &advanced,
            ],
        )?;
        let new = MenuItem::with_id(
            "new-layout",
            crate::i18n::native_text("New Layout…"),
            true,
            key(Code::KeyN, false),
        );
        let edit = MenuItem::with_id(
            "edit-layout",
            crate::i18n::native_text("Edit Layout"),
            true,
            key(Code::KeyE, false),
        );
        let save = MenuItem::with_id(
            "save-layout",
            crate::i18n::native_text("Save"),
            false,
            key(Code::KeyS, false),
        );
        let save_as = MenuItem::with_id(
            "save-layout-as",
            crate::i18n::native_text("Save As…"),
            false,
            key(Code::KeyS, true),
        );
        let export = MenuItem::with_id(
            "export-layout",
            crate::i18n::native_text("Export package…"),
            false,
            None,
        );
        let exit = MenuItem::with_id(
            "exit-editor",
            crate::i18n::native_text("Exit Editor"),
            false,
            None,
        );
        let layouts = Submenu::with_items(
            crate::i18n::native_text("Layout"),
            true,
            &[
                &new,
                &edit,
                &PredefinedMenuItem::separator(),
                &save,
                &save_as,
                &export,
                &exit,
            ],
        )?;
        let menu = Menu::with_items(&[&app, &file, &layouts, &settings])?;
        let (tx, commands) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        muda::MenuEvent::set_event_handler(Some(move |event: muda::MenuEvent| {
            let command = match event.id.0.as_str() {
                "open-video" => Command::OpenVideo,
                "select-layout" => Command::SelectLayout,
                "new-layout" => Command::NewLayout,
                "edit-layout" => Command::EditLayout,
                "save-layout" => Command::SaveLayout,
                "save-layout-as" => Command::SaveLayoutAs,
                "export-layout" => Command::ExportLayout,
                "export-video" => Command::ExportVideo,
                "exit-editor" => Command::ExitEditor,
                "audio-settings" => Command::AudioSettings,
                "map-settings" => Command::MapSettings,
                "privacy-settings" => Command::PrivacySettings,
                "interface-settings" => Command::InterfaceSettings,
                "regional-settings" => Command::RegionalSettings,
                "advanced-settings" => Command::AdvancedSettings,
                "sources" => Command::Sources,
                "clear-recent-videos" => Command::ClearRecentVideos,
                "close-video" => Command::CloseVideo,
                "quit" => Command::Quit,
                id => {
                    let Some(index) = id
                        .strip_prefix("recent-video-")
                        .and_then(|s| s.parse().ok())
                    else {
                        return;
                    };
                    Command::OpenRecentVideo(index)
                }
            };
            let _ = tx.send(command);
            ctx.request_repaint();
        }));
        menu.init_for_nsapp();
        Ok(Self {
            _menu: menu,
            close,
            export_video,
            has_video: std::cell::Cell::new(false),
            editor_items: [save, save_as, export, exit],
            edit,
            commands,
            recent,
            recent_paths: None,
        })
    }

    pub fn show(
        &mut self,
        _ui: &mut egui::Ui,
        has_video: bool,
        editing: bool,
        prefs: &crate::prefs::Prefs,
    ) -> Option<Command> {
        for item in &self.editor_items {
            item.set_enabled(editing);
        }
        self.edit.set_enabled(!editing);
        self.export_video.set_enabled(has_video);
        if self.has_video.replace(has_video) != has_video {
            self.close.set_enabled(has_video);
        }
        if self.recent_paths.as_ref() != Some(&prefs.recent_videos) {
            while self.recent.remove_at(0).is_some() {}
            let result = (|| -> muda::Result<()> {
                if prefs.recent_videos.is_empty() {
                    self.recent.append(&muda::MenuItem::new(
                        crate::i18n::native_text("No Recent Videos"),
                        false,
                        None,
                    ))?;
                }
                for (index, path) in prefs.recent_videos.iter().enumerate() {
                    self.recent.append(&muda::MenuItem::with_id(
                        format!("recent-video-{index}"),
                        path.to_string_lossy(),
                        true,
                        None,
                    ))?;
                }
                self.recent.append(&muda::PredefinedMenuItem::separator())?;
                self.recent.append(&muda::MenuItem::with_id(
                    "clear-recent-videos",
                    crate::i18n::native_text("Clear Recent Videos"),
                    !prefs.recent_videos.is_empty(),
                    None,
                ))?;
                Ok(())
            })();
            if let Err(e) = result {
                log::warn!("cannot update recent videos menu: {e}");
            }
            self.recent_paths = Some(prefs.recent_videos.clone());
        }
        self.commands.try_recv().ok()
    }
}

#[cfg(not(target_os = "macos"))]
pub struct Menus;

#[cfg(not(target_os = "macos"))]
impl Menus {
    pub fn new(_ctx: &egui::Context) -> Result<Self, std::convert::Infallible> {
        Ok(Self)
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        has_video: bool,
        editing: bool,
        prefs: &crate::prefs::Prefs,
    ) -> Option<Command> {
        let mut command = ui.ctx().input_mut(|i| {
            // Consume the more specific shortcut first.
            if i.consume_key(egui::Modifiers::COMMAND, egui::Key::N) {
                Some(Command::NewLayout)
            } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::E) && !editing {
                Some(Command::EditLayout)
            } else if i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::O,
            ) {
                Some(Command::SelectLayout)
            } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::O) {
                Some(Command::OpenVideo)
            } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::W) && has_video {
                Some(Command::CloseVideo)
            } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::Q) {
                Some(Command::Quit)
            } else {
                None
            }
        });
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button(crate::i18n::ui_text(ui, "File"), |ui| {
                    ui.menu_button(crate::i18n::ui_text(ui, "Recent Videos"), |ui| {
                        if prefs.recent_videos.is_empty() {
                            ui.add_enabled(
                                false,
                                egui::Button::new(crate::i18n::text("No Recent Videos")),
                            );
                        }
                        for (index, path) in prefs.recent_videos.iter().enumerate() {
                            if ui
                                .button(crate::i18n::user_text(ui, path.to_string_lossy()))
                                .clicked()
                            {
                                command = Some(Command::OpenRecentVideo(index));
                                ui.close();
                            }
                        }
                        ui.separator();
                        if ui
                            .add_enabled(
                                !prefs.recent_videos.is_empty(),
                                egui::Button::new(crate::i18n::text("Clear Recent Videos")),
                            )
                            .clicked()
                        {
                            command = Some(Command::ClearRecentVideos);
                            ui.close();
                        }
                    });
                    for (label, action, enabled) in [
                        ("Open Video…    Ctrl+O", Command::OpenVideo, true),
                        ("Video sources…", Command::Sources, has_video),
                        (
                            "Select Layout…    Ctrl+Shift+O",
                            Command::SelectLayout,
                            true,
                        ),
                        ("Close Video    Ctrl+W", Command::CloseVideo, has_video),
                        ("Export Video…", Command::ExportVideo, has_video),
                        ("Quit    Ctrl+Q", Command::Quit, true),
                    ] {
                        if ui
                            .add_enabled(enabled, egui::Button::new(crate::i18n::text(label)))
                            .clicked()
                        {
                            command = Some(action);
                            ui.close();
                        }
                    }
                });
                ui.menu_button(crate::i18n::ui_text(ui, "Settings"), |ui| {
                    if ui
                        .button(crate::i18n::ui_text(ui, "File associations…"))
                        .clicked()
                    {
                        command = Some(Command::FileAssociations);
                        ui.close();
                    }
                    if ui.button(crate::i18n::ui_text(ui, "Interface…")).clicked() {
                        command = Some(Command::InterfaceSettings);
                        ui.close();
                    }
                    if ui.button(crate::i18n::text("Regional settings…")).clicked() {
                        command = Some(Command::RegionalSettings);
                        ui.close();
                    }
                    if ui.button(crate::i18n::text("Advanced…")).clicked() {
                        command = Some(Command::AdvancedSettings);
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .button(crate::i18n::ui_text(ui, "Privacy zones…"))
                        .clicked()
                    {
                        command = Some(Command::PrivacySettings);
                        ui.close();
                    }
                    if ui.button(crate::i18n::ui_text(ui, "Maps…")).clicked() {
                        command = Some(Command::MapSettings);
                        ui.close();
                    }
                    if ui.button(crate::i18n::ui_text(ui, "Audio…")).clicked() {
                        command = Some(Command::AudioSettings);
                        ui.close();
                    }
                });
                ui.menu_button(crate::i18n::ui_text(ui, "Layout"), |ui| {
                    for (label, command_to_run, enabled) in [
                        ("New Layout…", Command::NewLayout, true),
                        ("Edit Layout", Command::EditLayout, !editing),
                        ("Save", Command::SaveLayout, editing),
                        ("Save As…", Command::SaveLayoutAs, editing),
                        ("Export package…", Command::ExportLayout, editing),
                        ("Exit Editor", Command::ExitEditor, editing),
                    ] {
                        if ui
                            .add_enabled(enabled, egui::Button::new(crate::i18n::text(label)))
                            .clicked()
                        {
                            command = Some(command_to_run);
                            ui.close();
                        }
                    }
                });
            });
        });
        command
    }
}
