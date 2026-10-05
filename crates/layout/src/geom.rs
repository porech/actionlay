//! Anchoring geometry in layout units: 1 unit = 1/1080 of the video height (spec §4.1, §4.2).
use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Height of the reference frame: layout values are 1080p pixels.
pub const REFERENCE_HEIGHT: f32 = 1080.0;

/// One of the 9 points of a box a node is attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Anchor {
    #[default]
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Anchor {
    pub const ALL: [Anchor; 9] = [
        Anchor::TopLeft,
        Anchor::Top,
        Anchor::TopRight,
        Anchor::Left,
        Anchor::Center,
        Anchor::Right,
        Anchor::BottomLeft,
        Anchor::Bottom,
        Anchor::BottomRight,
    ];

    /// Position of the anchor inside a box: 0 = left/top, 0.5 = centre, 1 = right/bottom.
    pub fn fractions(self) -> (f32, f32) {
        match self {
            Anchor::TopLeft => (0.0, 0.0),
            Anchor::Top => (0.5, 0.0),
            Anchor::TopRight => (1.0, 0.0),
            Anchor::Left => (0.0, 0.5),
            Anchor::Center => (0.5, 0.5),
            Anchor::Right => (1.0, 0.5),
            Anchor::BottomLeft => (0.0, 1.0),
            Anchor::Bottom => (0.5, 1.0),
            Anchor::BottomRight => (1.0, 1.0),
        }
    }
}

/// Axis-aligned box (top-left corner + size).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
}

/// How layout units map to pixels (spec §4.2). Chosen by the project, not the layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScaleMode {
    /// `H / 1080`
    #[default]
    Height,
    /// `min(H / 1080, W / (1080 × design_aspect))`: vertical or narrow videos.
    Fit,
}

/// Pixels per layout unit.
pub fn scale_factor(mode: ScaleMode, width_px: f32, height_px: f32, design_aspect: f32) -> f32 {
    let by_height = height_px / REFERENCE_HEIGHT;
    match mode {
        ScaleMode::Height => by_height,
        ScaleMode::Fit => by_height.min(width_px / (REFERENCE_HEIGHT * design_aspect)),
    }
}

/// The whole video frame, in layout units.
pub fn root_box(width_px: f32, height_px: f32, scale: f32) -> Rect {
    let s = scale.max(f32::MIN_POSITIVE);
    Rect::new(0.0, 0.0, width_px / s, height_px / s)
}

/// Places a box of `size` so that its `anchor` point sits on the parent's `anchor`
/// point moved by `offset`. A right-anchored box therefore grows to the left.
pub fn place(parent: Rect, anchor: Anchor, offset: [f32; 2], size: [f32; 2]) -> Rect {
    let (fx, fy) = anchor.fractions();
    let ax = parent.x + parent.w * fx + offset[0];
    let ay = parent.y + parent.h * fy + offset[1];
    Rect::new(ax - size[0] * fx, ay - size[1] * fy, size[0], size[1])
}

/// Design aspect ratio of a layout, written `"16:9"`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aspect {
    pub w: f32,
    pub h: f32,
}

impl Aspect {
    pub const WIDESCREEN: Aspect = Aspect { w: 16.0, h: 9.0 };

    pub fn ratio(self) -> f32 {
        self.w / self.h
    }

    pub fn parse(s: &str) -> Option<Aspect> {
        let (w, h) = s.split_once(':')?;
        let (w, h): (f32, f32) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
        (w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0).then_some(Aspect { w, h })
    }
}

impl Serialize for Aspect {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{}:{}", self.w, self.h))
    }
}

impl<'de> Deserialize<'de> for Aspect {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Aspect::parse(&s).ok_or_else(|| {
            serde::de::Error::custom(format!("invalid aspect `{s}`: expected W:H, e.g. 16:9"))
        })
    }
}

impl JsonSchema for Aspect {
    fn schema_name() -> Cow<'static, str> {
        "Aspect".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "pattern": "^[0-9]+(\\.[0-9]+)?:[0-9]+(\\.[0-9]+)?$",
            "description": "Design aspect ratio W:H, e.g. \"16:9\""
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn height_mode_scales_with_video_height() {
        assert!(close(
            scale_factor(ScaleMode::Height, 1920.0, 1080.0, 16.0 / 9.0),
            1.0
        ));
        assert!(close(
            scale_factor(ScaleMode::Height, 3840.0, 2160.0, 16.0 / 9.0),
            2.0
        ));
        assert!(close(
            scale_factor(ScaleMode::Height, 1920.0, 1440.0, 16.0 / 9.0),
            1440.0 / 1080.0
        ));
    }

    #[test]
    fn fit_mode_shrinks_for_narrow_video() {
        // vertical 9:16 video with a 16:9 layout: limited by width
        assert!(close(
            scale_factor(ScaleMode::Fit, 1080.0, 1920.0, 16.0 / 9.0),
            0.5625
        ));
        // 4:3 video with a 16:9 layout
        assert!(close(
            scale_factor(ScaleMode::Fit, 1440.0, 1080.0, 16.0 / 9.0),
            0.75
        ));
        // same aspect: identical to height mode
        assert!(close(
            scale_factor(ScaleMode::Fit, 1920.0, 1080.0, 16.0 / 9.0),
            1.0
        ));
    }

    #[test]
    fn root_box_is_measured_in_units() {
        let r = root_box(3840.0, 2160.0, 2.0);
        assert_eq!(r, Rect::new(0.0, 0.0, 1920.0, 1080.0));
        let r = root_box(1440.0, 1080.0, 1.0);
        assert_eq!(r, Rect::new(0.0, 0.0, 1440.0, 1080.0));
    }

    #[test]
    fn bottom_right_stays_bottom_right_on_16_9_and_4_3() {
        for width in [1920.0, 1440.0] {
            let root = Rect::new(0.0, 0.0, width, 1080.0);
            let r = place(root, Anchor::BottomRight, [-24.0, -24.0], [420.0, 200.0]);
            assert_eq!(
                r,
                Rect::new(width - 24.0 - 420.0, 1080.0 - 24.0 - 200.0, 420.0, 200.0)
            );
        }
    }

    #[test]
    fn every_anchor_aligns_the_matching_point_of_the_box() {
        let parent = Rect::new(100.0, 50.0, 1000.0, 500.0);
        let size = [100.0, 40.0];
        for anchor in Anchor::ALL {
            let (fx, fy) = anchor.fractions();
            let r = place(parent, anchor, [10.0, -5.0], size);
            // the anchor point of the child equals the anchor point of the parent plus the offset
            assert!(
                close(r.x + r.w * fx, parent.x + parent.w * fx + 10.0),
                "{anchor:?}"
            );
            assert!(
                close(r.y + r.h * fy, parent.y + parent.h * fy - 5.0),
                "{anchor:?}"
            );
        }
        let c = place(parent, Anchor::Center, [0.0, 0.0], size);
        assert_eq!(c, Rect::new(550.0, 280.0, 100.0, 40.0));
    }

    #[test]
    fn zero_size_box_sits_on_the_anchor_point() {
        let parent = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        let r = place(parent, Anchor::BottomLeft, [24.0, -24.0], [0.0, 0.0]);
        assert_eq!(r, Rect::new(24.0, 1056.0, 0.0, 0.0));
    }

    #[test]
    fn anchors_and_aspect_use_the_file_spelling() {
        assert_eq!(
            serde_json::to_string(&Anchor::BottomRight).unwrap(),
            "\"bottom-right\""
        );
        for a in Anchor::ALL {
            let s = serde_json::to_string(&a).unwrap();
            assert_eq!(serde_json::from_str::<Anchor>(&s).unwrap(), a);
        }
        let a: Aspect = serde_json::from_str("\"4:3\"").unwrap();
        assert!(close(a.ratio(), 4.0 / 3.0));
        assert_eq!(serde_json::to_string(&a).unwrap(), "\"4:3\"");
        assert!(serde_json::from_str::<Aspect>("\"16x9\"").is_err());
        assert!(serde_json::from_str::<Aspect>("\"0:9\"").is_err());
    }
}
