//! Transport bar: play/pause, scrubbing, frame step, speed, playback stats.
use actionlay_media::player::Player;
use eframe::egui;

#[derive(Default)]
pub struct ScrubState {
    pub dragging: bool,
    pub was_playing: bool,
}

pub fn format_time(seconds: f64) -> String {
    let hundredths = (seconds * 100.0).floor() as u64;
    format!(
        "{}:{:02}.{:02}",
        hundredths / 6000,
        (hundredths / 100) % 60,
        hundredths % 100
    )
}

const SPEEDS: [f64; 6] = [0.25, 0.5, 1.0, 1.5, 2.0, 4.0];

pub fn show(ui: &mut egui::Ui, player: &mut Player, scrub: &mut ScrubState) {
    let duration = player.info().duration;
    ui.horizontal(|ui| {
        if ui
            .button(if player.is_paused() { "▶" } else { "⏸" })
            .clicked()
        {
            player.toggle();
        }
        if ui.button("⏮ frame").clicked() {
            player.step(-1);
        }
        if ui.button("frame ⏭").clicked() {
            player.step(1);
        }

        let mut pos = player.position();
        ui.label(format_time(pos));
        ui.spacing_mut().slider_width = (ui.available_width() - 260.0).max(100.0);
        let response = ui.add(egui::Slider::new(&mut pos, 0.0..=duration).show_value(false));
        if response.drag_started() {
            scrub.dragging = true;
            scrub.was_playing = !player.is_paused();
            player.pause();
        }
        if response.changed() {
            // keyframe seek while dragging keeps scrubbing responsive
            player.seek(pos, !scrub.dragging);
        }
        if response.drag_stopped() {
            scrub.dragging = false;
            player.seek(pos, true);
            if scrub.was_playing {
                player.play();
            }
        }
        ui.label(format_time(duration));

        let mut speed = player.speed();
        egui::ComboBox::from_id_salt("speed")
            .selected_text(format!("{speed}x"))
            .show_ui(ui, |ui| {
                for s in SPEEDS {
                    ui.selectable_value(&mut speed, s, format!("{s}x"));
                }
            });
        if speed != player.speed() {
            player.set_speed(speed);
        }
    });

    let st = player.stats();
    let v = &player.info().video;
    ui.small(format!(
        "{}x{} {} @ {:.2} fps · decoder: {} · presented {} · dropped {} · audio: {} · A/V {:+.0} ms",
        v.width,
        v.height,
        v.codec,
        v.fps,
        st.backend,
        st.presented,
        st.dropped,
        if st.audio_active { "on" } else { "off" },
        st.av_offset * 1000.0
    ));
}

pub fn handle_keys(ctx: &egui::Context, player: &mut Player) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    ctx.input(|i| {
        if i.key_pressed(egui::Key::Space) {
            player.toggle();
        }
        if i.key_pressed(egui::Key::ArrowRight) {
            player.step(1);
        }
        if i.key_pressed(egui::Key::ArrowLeft) {
            player.step(-1);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::format_time;

    #[test]
    fn formats_minutes_seconds_and_hundredths() {
        assert_eq!(format_time(0.0), "0:00.00");
        assert_eq!(format_time(198.677), "3:18.67");
        assert_eq!(format_time(3725.5), "62:05.50");
    }
}
