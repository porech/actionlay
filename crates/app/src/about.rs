//! Branded, platform-independent application information.
use eframe::egui;

#[derive(Default)]
pub struct Dialog {
    pub visible: bool,
    icon: Option<egui::TextureHandle>,
}

impl Dialog {
    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.visible {
            return;
        }
        let icon = self.icon.get_or_insert_with(|| {
            let image =
                image::load_from_memory(include_bytes!("../../../assets/icons/actionlay-256.png"))
                    .expect("bundled application icon")
                    .to_rgba8();
            ctx.load_texture(
                "about-actionlay-icon",
                egui::ColorImage::from_rgba_unmultiplied([256, 256], image.as_raw()),
                egui::TextureOptions::LINEAR,
            )
        });
        let mut close = false;
        egui::Window::new(crate::i18n::text("About ActionLay"))
            .open(&mut self.visible)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(16, 48, 54))
                    .corner_radius(12)
                    .inner_margin(20)
                    .show(ui, |ui| {
                        ui.set_min_width(420.0);
                        ui.horizontal(|ui| {
                            ui.image((icon.id(), egui::vec2(104.0, 104.0)));
                            ui.add_space(12.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    egui::RichText::new("ActionLay")
                                        .size(32.0)
                                        .strong()
                                        .color(egui::Color32::WHITE),
                                );
                                ui.label(
                                    egui::RichText::new(
                                        "Your videos. Your telemetry.\nNo strings attached.",
                                    )
                                    .color(egui::Color32::from_rgb(204, 231, 227)),
                                );
                            });
                        });
                    });
                ui.add_space(12.0);
                ui.label(format!(
                    "{} {}",
                    crate::i18n::text("Version"),
                    env!("ACTIONLAY_BUILD_VERSION")
                ));
                ui.label("© Alessandro Rinaldi");
                ui.horizontal(|ui| {
                    ui.hyperlink_to(
                        "GPL-3.0-or-later",
                        "https://github.com/porech/actionlay/blob/main/LICENSE",
                    );
                    ui.label("·");
                    ui.hyperlink_to(
                        "CC BY-SA 4.0",
                        "https://github.com/porech/actionlay/tree/main/assets/icons",
                    );
                });
                ui.hyperlink("https://github.com/porech/actionlay");
                ui.add_space(8.0);
                close = ui.button(crate::i18n::text("Close")).clicked();
            });
        if close {
            self.visible = false;
        }
    }
}
