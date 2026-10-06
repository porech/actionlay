//! Optional per-user Open With integration. Never writes default associations.
#[cfg(not(target_os = "macos"))]
use crate::prefs::Prefs;
#[cfg(not(target_os = "macos"))]
use eframe::egui;

#[cfg(any(target_os = "linux", test))]
mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(not(target_os = "macos"))]
#[derive(Default)]
pub struct Dialog {
    pub open: bool,
    registered: bool,
    managed: bool,
    message: Option<String>,
}

#[cfg(not(target_os = "macos"))]
impl Dialog {
    pub fn startup(prefs: &Prefs) -> Self {
        let mut dialog = Self {
            registered: platform::registered(),
            managed: platform::managed(),
            ..Default::default()
        };
        if !dialog.managed
            && dialog.registered
            && (cfg!(target_os = "windows") || prefs.system_integration_enabled)
            && let Err(error) = platform::register()
        {
            dialog.message = Some(format!("Cannot update file integration: {error}"));
            dialog.open = true;
        }
        #[cfg(target_os = "windows")]
        {
            dialog.open |=
                !dialog.managed && !dialog.registered && !prefs.dismiss_association_prompt;
        }
        dialog
    }

    pub fn show(&mut self, ctx: &egui::Context, prefs: &mut Prefs) -> bool {
        if !self.open {
            return false;
        }
        let mut open = true;
        let mut action = None;
        let mut changed = false;
        egui::Window::new(crate::i18n::text("File associations"))
            .open(&mut open)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label(crate::i18n::ui_text(ui, "Add ActionLay to Open With for MP4, MOV, LRV and INSV videos, for your user account only."));
                ui.label(crate::i18n::ui_text(ui, "This does not change your default video player. Keep ActionLay in a permanent location before registering it."));
                if self.managed {
                    ui.label(crate::i18n::ui_text(ui, "File associations are already managed by the installer or system package. Use its uninstaller or your package manager to remove them."));
                    return;
                }
                if let Some(message) = &self.message {
                    ui.label(crate::i18n::ui_text(ui, message));
                }
                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::ui_text(ui, if self.registered { "Update registration" } else { "Add to Open With" })).clicked() {
                        action = Some(true);
                    }
                    if ui.add_enabled(self.registered, egui::Button::new(crate::i18n::text("Remove registration"))).clicked() {
                        action = Some(false);
                    }
                });
                #[cfg(target_os = "windows")]
                {
                    changed |= ui.checkbox(&mut prefs.dismiss_association_prompt, crate::i18n::ui_text(ui, "Don't ask again at startup")).changed();
                }
            });
        self.open = open;
        if let Some(add) = action {
            match if add {
                platform::register()
            } else {
                platform::remove()
            } {
                Ok(()) => {
                    self.registered = add;
                    prefs.system_integration_enabled = add;
                    prefs.dismiss_association_prompt = true;
                    changed = true;
                    self.message = Some(
                        if add {
                            "ActionLay is available in Open With."
                        } else {
                            "ActionLay's registration has been removed."
                        }
                        .into(),
                    );
                }
                Err(error) => {
                    self.message = Some(format!("Cannot change file integration: {error}"))
                }
            }
        }
        changed
    }
}

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "windows")]
use windows as platform;
