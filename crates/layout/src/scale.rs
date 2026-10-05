//! Automatic choice of the scale mode for a video size (spec §4.2).
use crate::geom::{Aspect, REFERENCE_HEIGHT, Rect, ScaleMode, place, root_box};
use crate::{Layout, Node, Widget};

/// Below this fraction of the design aspect, a layout without any node of known size
/// switches to `Fit` (9:16 under 16:9 does, 4:3 does not).
pub const FALLBACK_FIT_RATIO: f32 = 0.7;

/// Tolerance in layout units for the containment and overlap tests.
const EPS: f32 = 1e-3;

/// Scale mode for a video of `width × height` pixels.
///
/// `Fit` reproduces, horizontally, the geometry of the design frame; `Height` keeps the
/// widgets at their designed size relative to the video height. `Fit` is chosen only when
/// `Height` would break the layout: placing the top-level nodes of known size (frames,
/// sized groups, sized icons) in the video frame at the `Height` scale, one extends
/// outside the frame horizontally, or two of them overlap, where they did not in the
/// design frame. Nodes whose size depends on their text (metrics, labels) are not
/// measured. A layout without any node of known size uses `Fit` when the video aspect is
/// below [`FALLBACK_FIT_RATIO`] × the design aspect.
pub fn auto_scale_mode(layout: &Layout, width: u32, height: u32) -> ScaleMode {
    if width == 0 || height == 0 {
        return ScaleMode::Height;
    }
    let design = layout.design_aspect.unwrap_or(Aspect::WIDESCREEN).ratio();
    let video = width as f32 / height as f32;
    if video >= design {
        // Fit is the same as Height at or above the design aspect
        return ScaleMode::Height;
    }
    let design_root = root_box(REFERENCE_HEIGHT * design, REFERENCE_HEIGHT, 1.0);
    let scale = height as f32 / REFERENCE_HEIGHT;
    let video_root = root_box(width as f32, height as f32, scale);
    let in_design = placed(layout, design_root);
    let in_video = placed(layout, video_root);
    if in_video.is_empty() {
        return if video < FALLBACK_FIT_RATIO * design {
            ScaleMode::Fit
        } else {
            ScaleMode::Height
        };
    }
    let outside = |root: Rect, r: &Rect| r.x < root.x - EPS || r.x + r.w > root.x + root.w + EPS;
    let newly_outside = in_video
        .iter()
        .zip(&in_design)
        .any(|(v, d)| outside(video_root, v) && !outside(design_root, d));
    let newly_overlapping = (0..in_video.len()).any(|i| {
        (i + 1..in_video.len())
            .any(|j| overlap(&in_video[i], &in_video[j]) && !overlap(&in_design[i], &in_design[j]))
    });
    if newly_outside || newly_overlapping {
        ScaleMode::Fit
    } else {
        ScaleMode::Height
    }
}

/// The visible top-level nodes of known size, placed in `root` (layout units).
fn placed(layout: &Layout, root: Rect) -> Vec<Rect> {
    layout
        .nodes
        .iter()
        .filter_map(|node| {
            let Node::Known(w) = node else { return None };
            let c = w.common();
            if c.visible == Some(false) {
                return None;
            }
            let size = known_size(w)?;
            Some(place(
                root,
                c.anchor.unwrap_or_default(),
                c.offset.unwrap_or([0.0, 0.0]),
                size,
            ))
        })
        .collect()
}

fn known_size(w: &Widget) -> Option<[f32; 2]> {
    let size = match w {
        Widget::Frame(f) => f.size,
        Widget::Group(g) => g.size?,
        Widget::Icon(i) => [i.size?; 2],
        Widget::GpsLockIcon(i) => [i.size?; 2],
        Widget::Text(_) | Widget::Metric(_) | Widget::MetricUnit(_) | Widget::Datetime(_) => {
            return None;
        }
    };
    size.iter()
        .all(|v| v.is_finite() && *v > 0.0)
        .then_some(size)
}

/// The two boxes share an area (touching edges do not count).
fn overlap(a: &Rect, b: &Rect) -> bool {
    a.x + EPS < b.x + b.w && b.x + EPS < a.x + a.w && a.y + EPS < b.y + b.h && b.y + EPS < a.y + a.h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_layout;

    fn layout(json: &str) -> Layout {
        Layout::from_json(json).unwrap().layout
    }

    #[test]
    fn default_layout_uses_height_on_4_3_and_16_9_and_fit_on_9_16() {
        let d = default_layout();
        assert_eq!(auto_scale_mode(&d, 1920, 1440), ScaleMode::Height); // 4:3
        assert_eq!(auto_scale_mode(&d, 1440, 1080), ScaleMode::Height); // 4:3
        assert_eq!(auto_scale_mode(&d, 1920, 1080), ScaleMode::Height); // 16:9
        assert_eq!(auto_scale_mode(&d, 3840, 1600), ScaleMode::Height); // 2.4:1
        // 9:16: the frames still fit the width one by one but the top and the bottom
        // pairs overlap
        assert_eq!(auto_scale_mode(&d, 1080, 1920), ScaleMode::Fit);
        assert_eq!(auto_scale_mode(&d, 0, 0), ScaleMode::Height);
    }

    #[test]
    fn a_frame_pushed_outside_the_width_switches_to_fit() {
        // one wide frame: inside a 16:9 frame (1920 units), not inside 4:3 (1440)
        let l = layout(
            r#"{"version": 1, "nodes": [
                {"type": "frame", "anchor": "top-left", "offset": [24, 24], "size": [1600, 100]}]}"#,
        );
        assert_eq!(auto_scale_mode(&l, 1920, 1080), ScaleMode::Height);
        assert_eq!(auto_scale_mode(&l, 1920, 1440), ScaleMode::Fit);
        // a sized group or icon counts too, a hidden frame does not
        let g = layout(
            r#"{"version": 1, "nodes": [
                {"type": "group", "anchor": "right", "size": [1500, 100], "children": []}]}"#,
        );
        assert_eq!(auto_scale_mode(&g, 1920, 1440), ScaleMode::Fit);
        let i = layout(
            r#"{"version": 1, "nodes": [
                {"type": "icon", "icon": "speed", "anchor": "left", "offset": [1400, 0], "size": 100}]}"#,
        );
        assert_eq!(auto_scale_mode(&i, 1920, 1440), ScaleMode::Fit);
        let hidden = layout(
            r#"{"version": 1, "nodes": [
                {"type": "frame", "visible": false, "size": [1600, 100]},
                {"type": "frame", "size": [100, 100]}]}"#,
        );
        assert_eq!(auto_scale_mode(&hidden, 1920, 1440), ScaleMode::Height);
    }

    #[test]
    fn overlap_already_in_the_design_does_not_count() {
        // stacked on purpose: a panel with a badge over its corner
        let l = layout(
            r#"{"version": 1, "nodes": [
                {"type": "frame", "anchor": "top-left", "size": [300, 100]},
                {"type": "frame", "anchor": "top-left", "offset": [250, 0], "size": [100, 50]}]}"#,
        );
        assert_eq!(auto_scale_mode(&l, 1080, 1920), ScaleMode::Height);
    }

    #[test]
    fn without_sized_nodes_fit_starts_below_70_percent_of_the_design_aspect() {
        let l = layout(
            r#"{"version": 1, "nodes": [
                {"type": "metric", "metric": "speed", "anchor": "bottom-left"},
                {"type": "text", "text": "hi", "anchor": "top-right"}]}"#,
        );
        assert_eq!(auto_scale_mode(&l, 1920, 1440), ScaleMode::Height); // 1.33 > 1.24
        assert_eq!(auto_scale_mode(&l, 1080, 1350), ScaleMode::Fit); // 0.8 < 1.24
        assert_eq!(auto_scale_mode(&l, 1080, 1920), ScaleMode::Fit); // 9:16
        let square = layout(
            r#"{"version": 1, "design_aspect": "1:1", "nodes": [{"type": "text", "text": "x"}]}"#,
        );
        assert_eq!(auto_scale_mode(&square, 1440, 1080), ScaleMode::Height);
        assert_eq!(auto_scale_mode(&square, 1080, 1920), ScaleMode::Fit); // 0.56 < 0.7
        assert_eq!(auto_scale_mode(&square, 1080, 1440), ScaleMode::Height); // 0.75
    }
}
