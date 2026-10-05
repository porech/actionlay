//! Small drawing helpers.
use actionlay_layout::color::Color;
use tiny_skia::{Paint, Path, PathBuilder, Rect};

/// Solid antialiased paint with the colour's alpha multiplied by `alpha`.
pub(crate) fn paint(c: Color, alpha: f32) -> Paint<'static> {
    let c = c.with_alpha_mul(alpha);
    let mut p = Paint::default();
    p.set_color_rgba8(c.r, c.g, c.b, c.a);
    p.anti_alias = true;
    p
}

/// Rectangle with circular corners of radius `r` (clamped to half the shorter side).
pub(crate) fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    if !(w > 0.0 && h > 0.0) {
        return None;
    }
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    if r == 0.0 {
        return Some(PathBuilder::from_rect(Rect::from_xywh(x, y, w, h)?));
    }
    // cubic approximation of a quarter circle
    let k = r * 0.552_284_8;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::{FillRule, Pixmap, Transform};

    #[test]
    fn rounded_rect_has_transparent_corners_and_a_filled_middle() {
        let mut p = Pixmap::new(100, 60).unwrap();
        let path = rounded_rect(0.0, 0.0, 100.0, 60.0, 20.0).unwrap();
        let b = path.bounds();
        assert_eq!(
            (b.left(), b.top(), b.right(), b.bottom()),
            (0.0, 0.0, 100.0, 60.0)
        );
        p.fill_path(
            &path,
            &paint(Color::rgba(255, 255, 255, 255), 1.0),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
        assert_eq!(p.pixel(1, 1).unwrap().alpha(), 0);
        assert_eq!(p.pixel(98, 58).unwrap().alpha(), 0);
        assert_eq!(p.pixel(50, 30).unwrap().alpha(), 255);
        assert_eq!(p.pixel(50, 1).unwrap().alpha(), 255);
        assert!(rounded_rect(0.0, 0.0, 0.0, 10.0, 2.0).is_none());
        // radius larger than the box: a stadium, still inside the box
        let b = rounded_rect(0.0, 0.0, 100.0, 20.0, 50.0).unwrap().bounds();
        assert_eq!((b.width(), b.height()), (100.0, 20.0));
    }

    #[test]
    fn paint_multiplies_the_colour_alpha() {
        let p = paint(Color::rgba(255, 0, 0, 200), 0.5);
        let c = p.shader;
        let tiny_skia::Shader::SolidColor(c) = c else {
            panic!("solid")
        };
        assert_eq!(c.to_color_u8().alpha(), 100);
    }
}
