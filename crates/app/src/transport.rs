use actionlay_media::player::Player;
use eframe::egui;

#[derive(Default)]
pub struct ScrubState;

pub fn show(ui: &mut egui::Ui, player: &mut Player, _scrub: &mut ScrubState) {
    if ui
        .button(if player.is_paused() { "Play" } else { "Pause" })
        .clicked()
    {
        player.toggle();
    }
}
