//! Transient historical-widget loading badges. Never included in overlay pixels.
use actionlay_layout::{
    Layout,
    geom::{ScaleMode, scale_factor},
};
use actionlay_render::loading::LoadingRegion;
use eframe::egui;

pub fn show_regions(
    ui: &egui::Ui,
    video: egui::Rect,
    scale_mode: ScaleMode,
    layout: &Layout,
    regions: &[LoadingRegion],
    progress: Option<crate::telemetry_load::RouteProgress>,
) {
    let scale = scale_factor(
        scale_mode,
        video.width(),
        video.height(),
        layout
            .design_aspect
            .unwrap_or(actionlay_layout::geom::Aspect::WIDESCREEN)
            .ratio(),
    );
    for region in regions {
        let rect = region.rect;
        let rect = egui::Rect::from_min_size(
            video.min + egui::vec2(rect.x * scale, rect.y * scale),
            egui::vec2(rect.w * scale, rect.h * scale),
        )
        .intersect(video);
        if rect.width() < 1.0 || rect.height() < 1.0 {
            continue;
        }
        let fraction = progress
            .filter(|p| !p.failed)
            .map_or(region.fraction, |progress| {
                if region.requirement.full {
                    region
                        .fraction
                        .max(f64::from(progress.fraction.unwrap_or(0.0)))
                } else {
                    let newly_read: f64 = region
                        .missing
                        .iter()
                        .map(|&(a, b)| {
                            (b.min(progress.read_end) - a.max(progress.read_start)).max(0.0)
                        })
                        .sum();
                    (region.fraction
                        + newly_read
                            / (region.requirement.end - region.requirement.start).max(1e-9))
                    .min(1.0)
                }
            });
        let galley = ui.painter().layout(
            format!("{:.0}%", (fraction * 100.0).floor()),
            egui::FontId::proportional(13.0),
            egui::Color32::WHITE,
            (video.width() - 48.0).max(1.0),
        );
        let size = egui::vec2(
            (galley.size().x + 44.0).min(video.width()),
            (galley.size().y + 16.0).max(34.0).min(video.height()),
        );
        let badge = egui::Rect::from_center_size(rect.center(), size);
        let painter = ui.painter().with_clip_rect(video);
        painter.rect_filled(badge, 6.0, egui::Color32::from_black_alpha(210));
        let spinner = egui::Rect::from_center_size(
            egui::pos2(badge.left() + 16.0, badge.center().y),
            egui::vec2(18.0, 18.0),
        );
        egui::Spinner::new()
            .color(egui::Color32::WHITE)
            .paint_at(ui, spinner);
        painter.galley(
            egui::pos2(
                badge.left() + 32.0,
                badge.center().y - galley.size().y / 2.0,
            ),
            galley,
            egui::Color32::WHITE,
        );
    }
}
