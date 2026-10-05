//! Transport bar: play/pause, scrubbing, frame step, speed, playback stats.
use actionlay_media::player::Player;
use eframe::egui;

#[derive(Default)]
pub struct ScrubState {
    pub dragging: bool,
    pub was_playing: bool,
    /// Position under the pointer while dragging (the player position snaps to keyframes).
    pub target: f64,
}

/// What the transport must do to the player after this frame's slider interaction.
#[derive(Debug, Default, PartialEq)]
pub struct ScrubOutcome {
    pub pause: bool,
    /// (position, precise)
    pub seek: Option<(f64, bool)>,
    pub resume: bool,
}

/// Pure scrub decision logic. `value` is the slider value after this frame,
/// `pos` the player position and `playing` its state before it.
pub fn scrub_action(
    state: &mut ScrubState,
    value: f64,
    pos: f64,
    playing: bool,
    drag_started: bool,
    changed: bool,
    drag_stopped: bool,
) -> ScrubOutcome {
    let mut out = ScrubOutcome::default();
    if drag_started {
        state.dragging = true;
        state.was_playing = playing;
        state.target = pos;
        out.pause = true;
    }
    if changed {
        if state.dragging {
            if value != state.target {
                state.target = value;
                out.seek = Some((value, false));
            }
        } else {
            out.seek = Some((value, true));
        }
    }
    if drag_stopped {
        state.dragging = false;
        out.seek = Some((state.target, true));
        out.resume = state.was_playing;
    }
    out
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

/// `overlay_status` ends the stats line (e.g. `overlay 4.2 ms`).
pub fn show(ui: &mut egui::Ui, player: &mut Player, scrub: &mut ScrubState, overlay_status: &str) {
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

        let pos = player.position();
        let mut value = if scrub.dragging { scrub.target } else { pos };
        ui.label(format_time(value));
        ui.spacing_mut().slider_width = (ui.available_width() - 260.0).max(100.0);
        let response = ui.add(egui::Slider::new(&mut value, 0.0..=duration).show_value(false));
        let out = scrub_action(
            scrub,
            value,
            pos,
            !player.is_paused(),
            response.drag_started(),
            response.changed(),
            response.drag_stopped(),
        );
        if out.pause {
            player.pause();
        }
        if let Some((to, precise)) = out.seek {
            player.seek(to, precise);
        }
        if out.resume {
            player.play();
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
        "{}x{} {} @ {:.2} fps · decoder: {} · presented {} · dropped {} · audio: {} · A/V {} · {overlay_status}",
        v.width,
        v.height,
        v.codec,
        v.fps,
        st.backend,
        st.presented,
        st.dropped,
        if st.audio_active { "on" } else { "off" },
        st.av_offset
            .map_or_else(|| "n/a".to_string(), |o| format!("{:+.0} ms", o * 1000.0))
    ));
}

pub fn handle_keys(ctx: &egui::Context, player: &mut Player, scrub: &ScrubState) {
    if scrub.dragging || ctx.egui_wants_keyboard_input() {
        return;
    }
    ctx.input(|i| {
        // Space must not auto-repeat (it would toggle play/pause rapidly)
        let space = i.events.iter().any(|e| {
            matches!(
                e,
                egui::Event::Key {
                    key: egui::Key::Space,
                    pressed: true,
                    repeat: false,
                    ..
                }
            )
        });
        if space {
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
    use super::{ScrubState, format_time, scrub_action};

    fn step(
        st: &mut ScrubState,
        value: f64,
        started: bool,
        changed: bool,
        stopped: bool,
    ) -> super::ScrubOutcome {
        // player position snapped to a keyframe (1.0) regardless of the pointer
        scrub_action(st, value, 1.0, true, started, changed, stopped)
    }

    #[test]
    fn drag_seeks_keyframe_then_precise_on_release_at_pointer() {
        let mut st = ScrubState::default();
        let o = step(&mut st, 5.0, true, true, false);
        assert!(o.pause);
        assert_eq!(o.seek, Some((5.0, false)));
        // hold still: slider shows target, no change, no seeks
        for _ in 0..3 {
            assert_eq!(step(&mut st, 5.0, false, false, false), Default::default());
        }
        let o = step(&mut st, 7.5, false, true, false);
        assert_eq!(o.seek, Some((7.5, false)));
        let o = step(&mut st, 7.5, false, false, true);
        assert_eq!(o.seek, Some((7.5, true)));
        assert!(o.resume);
        assert!(!st.dragging);
    }

    #[test]
    fn click_without_drag_seeks_once_precisely() {
        let mut st = ScrubState::default();
        let o = step(&mut st, 4.0, false, true, false);
        assert_eq!(o.seek, Some((4.0, true)));
        assert!(!o.pause && !o.resume);
    }

    #[test]
    fn press_release_without_move_seeks_to_press_position() {
        let mut st = ScrubState::default();
        step(&mut st, 4.0, true, true, false);
        let o = step(&mut st, 4.0, false, false, true);
        assert_eq!(o.seek, Some((4.0, true)));
    }

    #[test]
    fn formats_minutes_seconds_and_hundredths() {
        assert_eq!(format_time(0.0), "0:00.00");
        assert_eq!(format_time(198.677), "3:18.67");
        assert_eq!(format_time(3725.5), "62:05.50");
    }
}
