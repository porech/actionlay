use actionlay_media::player::BufferingOptions;
use eframe::egui;

pub fn show(ctx: &egui::Context, visible: &mut bool, options: &mut BufferingOptions) -> bool {
    let before = *options;
    egui::Window::new(crate::i18n::text("Advanced"))
        .open(visible)
        .default_width(440.0)
        .show(ctx, |ui| {
            ui.heading(crate::i18n::text("Buffering"));
            ui.small(crate::i18n::ui_text(ui, "Read-ahead starts when the video opens, even while paused."));
            ui.label(crate::i18n::text("Read-ahead (seconds)"));
            ui.add(egui::DragValue::new(&mut options.read_ahead_seconds).range(0.1..=120.0).speed(0.1));
            ui.label(crate::i18n::text("Buffer before playback (seconds)"));
            ui.add(egui::DragValue::new(&mut options.start_buffer_seconds).range(0.0..=options.read_ahead_seconds).speed(0.1));
            ui.small(crate::i18n::ui_text(ui, "Playback resumes when this amount is available, the buffer is full, or the video ends."));
            ui.label(crate::i18n::text("Compressed packet memory (MiB)"));
            ui.add(egui::DragValue::new(&mut options.packet_memory_mib).range(1..=1024));
            ui.small(crate::i18n::ui_text(ui, "These are upper limits. Packet count and decoded frames also limit buffering; total memory use can be higher."));
            if ui.button(crate::i18n::text("Restore defaults")).clicked() {
                *options = BufferingOptions::default();
            }
        });
    *options = options.normalized();
    *options != before
}
