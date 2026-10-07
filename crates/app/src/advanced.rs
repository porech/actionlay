use actionlay_media::player::BufferingOptions;
use eframe::egui;

pub fn show(ctx: &egui::Context, visible: &mut bool, prefs: &mut crate::prefs::Prefs) -> bool {
    let before = (
        prefs.buffering,
        prefs.show_diagnostic_data,
        prefs.software_video_decoding,
    );
    let options = &mut prefs.buffering;
    egui::Window::new(crate::i18n::text("Advanced"))
        .open(visible)
        .default_width(440.0)
        .show(ctx, |ui| {
            ui.checkbox(&mut prefs.software_video_decoding, crate::i18n::text("Use software video decoding"));
            ui.small(crate::i18n::ui_text(ui, "Applies the next time a video is opened."));
            ui.separator();
            ui.checkbox(&mut prefs.show_diagnostic_data, crate::i18n::text("Show diagnostic data"));
            ui.small(crate::i18n::ui_text(ui, "Show decoder, frame, audio synchronization and rendering statistics below the player."));
            ui.separator();
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
    (
        prefs.buffering,
        prefs.show_diagnostic_data,
        prefs.software_video_decoding,
    ) != before
}
