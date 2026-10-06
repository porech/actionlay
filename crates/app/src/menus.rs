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
        let open = MenuItem::with_id("open-video", "Open Video…", true, key(Code::KeyO, false));
        let layout = MenuItem::with_id(
            "select-layout",
            "Select Layout…",
            true,
            key(Code::KeyO, true),
        );
        let close = MenuItem::with_id("close-video", "Close Video", false, key(Code::KeyW, false));
        let quit = MenuItem::with_id("quit", "Quit ActionLay", true, key(Code::KeyQ, false));
        let app = Submenu::with_items(
            "ActionLay",
            true,
            &[
                &PredefinedMenuItem::about(
                    None,
                    Some(AboutMetadata {
                        name: Some("ActionLay".into()),
                        version: Some(env!("CARGO_PKG_VERSION").into()),
                        ..Default::default()
                    }),
                ),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &PredefinedMenuItem::separator(),
                &quit,
            ],
        )?;
        let recent = Submenu::new("Recent Videos", true);
        let export_video = MenuItem::with_id("export-video", "Export Video…", false, None);
        let file = Submenu::with_items(
            "File",
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
        let audio = MenuItem::with_id("audio-settings", "Audio…", true, None);
        let settings = Submenu::with_items("Settings", true, &[&audio])?;
        let new = MenuItem::with_id("new-layout", "New Layout…", true, key(Code::KeyN, false));
        let edit = MenuItem::with_id("edit-layout", "Edit Layout", true, key(Code::KeyE, false));
        let save = MenuItem::with_id("save-layout", "Save", false, key(Code::KeyS, false));
        let save_as = MenuItem::with_id("save-layout-as", "Save As…", false, key(Code::KeyS, true));
        let export = MenuItem::with_id("export-layout", "Export package…", false, None);
        let exit = MenuItem::with_id("exit-editor", "Exit Editor", false, None);
        let layouts = Submenu::with_items(
            "Layout",
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
                    self.recent
                        .append(&muda::MenuItem::new("No Recent Videos", false, None))?;
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
                    "Clear Recent Videos",
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
                ui.menu_button("File", |ui| {
                    ui.menu_button("Recent Videos", |ui| {
                        if prefs.recent_videos.is_empty() {
                            ui.add_enabled(false, egui::Button::new("No Recent Videos"));
                        }
                        for (index, path) in prefs.recent_videos.iter().enumerate() {
                            if ui.button(path.to_string_lossy()).clicked() {
                                command = Some(Command::OpenRecentVideo(index));
                                ui.close();
                            }
                        }
                        ui.separator();
                        if ui
                            .add_enabled(
                                !prefs.recent_videos.is_empty(),
                                egui::Button::new("Clear Recent Videos"),
                            )
                            .clicked()
                        {
                            command = Some(Command::ClearRecentVideos);
                            ui.close();
                        }
                    });
                    for (label, action, enabled) in [
                        ("Open Video…    Ctrl+O", Command::OpenVideo, true),
                        (
                            "Select Layout…    Ctrl+Shift+O",
                            Command::SelectLayout,
                            true,
                        ),
                        ("Close Video    Ctrl+W", Command::CloseVideo, has_video),
                        ("Export Video…", Command::ExportVideo, has_video),
                        ("Quit    Ctrl+Q", Command::Quit, true),
                    ] {
                        if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                            command = Some(action);
                            ui.close();
                        }
                    }
                });
                ui.menu_button("Settings", |ui| {
                    if ui.button("Audio…").clicked() {
                        command = Some(Command::AudioSettings);
                        ui.close();
                    }
                });
                ui.menu_button("Layout", |ui| {
                    for (label, command_to_run, enabled) in [
                        ("New Layout…", Command::NewLayout, true),
                        ("Edit Layout", Command::EditLayout, !editing),
                        ("Save", Command::SaveLayout, editing),
                        ("Save As…", Command::SaveLayoutAs, editing),
                        ("Export package…", Command::ExportLayout, editing),
                        ("Exit Editor", Command::ExitEditor, editing),
                    ] {
                        if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
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
