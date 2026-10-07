//! Transient player UI. This module is never used by the overlay/export renderer.
use crate::telemetry_load::RouteProgress;
use actionlay_layout::{
    Layout,
    geom::{Rect, ScaleMode, place, root_box, scale_factor},
    model::{MapRoute, Node, Widget},
};
use actionlay_telemetry::Telemetry;
use eframe::egui;

pub fn show(
    ui: &egui::Ui,
    layout: &Layout,
    video: egui::Rect,
    scale_mode: ScaleMode,
    telemetry: Option<&Telemetry>,
    t: f64,
    progress: Option<RouteProgress>,
) {
    if progress.is_some_and(|p| p.failed || (p.finished && p.decoded)) {
        return;
    }
    let scale = scale_factor(
        scale_mode,
        video.width(),
        video.height(),
        layout
            .design_aspect
            .unwrap_or(actionlay_layout::geom::Aspect::WIDESCREEN)
            .ratio(),
    );
    let root = root_box(video.width(), video.height(), scale);
    let mut maps = Vec::new();
    collect(&layout.nodes, root, 1.0, telemetry, t, &mut maps);
    for map in maps {
        let map = egui::Rect::from_min_size(
            video.min + egui::vec2(map.x * scale, map.y * scale),
            egui::vec2(map.w * scale, map.h * scale),
        )
        .intersect(video);
        if map.width() < 24.0 || map.height() < 24.0 {
            continue;
        }
        let text = progress.and_then(|p| p.fraction).map_or_else(
            || {
                crate::i18n::text(if progress.is_some_and(|p| p.finished) {
                    "Preparing telemetry…"
                } else {
                    "Reading telemetry…"
                })
                .to_owned()
            },
            |fraction| format!("{:.0}%", fraction.clamp(0.0, 1.0) * 100.0),
        );
        let font = egui::FontId::proportional(13.0);
        let galley = ui.painter().layout(
            text,
            font,
            egui::Color32::WHITE,
            (map.width() - 48.0).max(1.0),
        );
        let size = egui::vec2(
            (galley.size().x + 44.0).min(map.width()),
            (galley.size().y + 16.0).max(34.0).min(map.height()),
        );
        let badge = egui::Rect::from_center_size(map.center(), size);
        let painter = ui.painter().with_clip_rect(map);
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

fn collect(
    nodes: &[Node],
    parent: Rect,
    opacity: f32,
    telemetry: Option<&Telemetry>,
    t: f64,
    maps: &mut Vec<Rect>,
) {
    for node in nodes {
        let Node::Known(widget) = node else { continue };
        let common = widget.common();
        let opacity = opacity * common.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
        if common.visible == Some(false) || opacity <= 0.0 {
            continue;
        }
        let at = |size| {
            place(
                parent,
                common.anchor.unwrap_or_default(),
                common.offset_in(parent),
                size,
            )
        };
        match widget {
            Widget::Map(map) => {
                let missing = if map.needs_full_track() {
                    telemetry.is_none_or(|t| !t.is_complete())
                } else if map.route_mode == Some(MapRoute::Past) {
                    telemetry.is_none_or(|tel| !tel.is_loaded_through(t))
                } else {
                    false
                };
                if missing {
                    maps.push(at(map.size()));
                }
            }
            Widget::Group(group) => collect(
                &group.children,
                at(group.size.unwrap_or([0.0; 2])),
                opacity,
                telemetry,
                t,
                maps,
            ),
            Widget::Frame(frame) => {
                collect(&frame.children, at(frame.size), opacity, telemetry, t, maps)
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loading_badges_follow_renderer_geometry_and_only_maps_missing_required_data() {
        let layout = Layout::from_json(r#"{"version":1,"nodes":[{"type":"group","anchor":"bottom-right","offset_relative":[-0.1,-0.1],"size":[800,600],"children":[{"type":"frame","offset":[40,30],"size":[600,400],"children":[{"type":"map","anchor":"bottom-right","offset_relative":[-0.03,-0.03],"size":[240,180],"route_mode":"full"}]}]},{"type":"map","route_mode":"none","zoom_mode":"route","size":[100,100]},{"type":"map","route_mode":"past","offset":[200,50],"size":[100,100]},{"type":"map","route_mode":"none","zoom_mode":"fixed"},{"type":"map","visible":false,"route_mode":"full"}]}"#).unwrap().layout;
        let telemetry = Telemetry::preview();
        let mut renderer = actionlay_render::Renderer::new();
        let maps = actionlay_maps::TileStore::offline();
        let mut settings = maps.settings();
        settings.attribution.clear();
        maps.configure(settings);
        renderer.set_maps(maps);
        let mut pixmap = actionlay_render::tiny_skia::Pixmap::new(1920, 1080).unwrap();
        renderer.render_editor_into(&layout, &telemetry, 10.0, &mut pixmap);
        let root = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        let mut badges = Vec::new();
        collect(&layout.nodes, root, 1.0, None, 10.0, &mut badges);
        assert_eq!(badges.len(), 3);
        let expected: Vec<_> = renderer
            .hit_boxes()
            .iter()
            .filter(|hit| [vec![0, 0, 0], vec![1], vec![2]].contains(&hit.path))
            .map(|hit| hit.rect)
            .collect();
        assert_eq!(badges, expected);
        badges.clear();
        collect(
            &layout.nodes,
            root,
            1.0,
            Some(&telemetry),
            10.0,
            &mut badges,
        );
        assert!(badges.is_empty());
        // A seek into a gap must show the past-route indicator, even when the
        // metadata at the target itself is available.
        let source =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/gopro/hero5.mp4");
        if source.exists() {
            let packets = crate::telemetry_load::to_raw(
                actionlay_media::gpmf::read_gpmf_packets(&source).unwrap(),
            );
            let partial = Telemetry::from_gpmf_packets_progressive(&[
                packets[0].clone(),
                packets[20].clone(),
            ])
            .unwrap();
            let target = packets[20].pts + 0.1;
            assert!(partial.is_loaded_at(target));
            assert!(!partial.is_loaded_through(target));
            collect(
                &layout.nodes,
                root,
                1.0,
                Some(&partial),
                target,
                &mut badges,
            );
            assert_eq!(badges.len(), 3);
        }
    }
}
