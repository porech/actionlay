//! Text shaping (cosmic-text) and glyph outlines as tiny-skia paths.
//! Layout-scoped fonts precede installed fonts, with embedded Roboto fallback
//! (Roboto v2.138, Apache-2.0, see assets/fonts/LICENSE). Locale is fixed.
use actionlay_layout::style::FontWeight;
use cosmic_text::{
    Attrs, Buffer, CacheKey, CacheKeyFlags, Command, Family, FontSystem, Metrics, Shaping,
    SwashCache, Weight, Wrap, fontdb,
};
use std::collections::HashMap;
use tiny_skia::{Path, PathBuilder, Transform};
type RunKey = (String, String, u32, u16);
#[derive(Clone)]
struct ShapedRun {
    size: (f32, f32),
    path: Option<Path>,
    missing: u64,
}

/// Embedded fallback family, available on every machine.
pub const FAMILY: &str = "Roboto";

pub(crate) const FACES: [&[u8]; 3] = [
    include_bytes!("../assets/fonts/Roboto-Regular.ttf"),
    include_bytes!("../assets/fonts/Roboto-Medium.ttf"),
    include_bytes!("../assets/fonts/Roboto-Bold.ttf"),
];

pub(crate) struct TextEngine {
    fonts: FontSystem,
    swash: SwashCache,
    buffer: Buffer,
    /// The last `layout` produced nothing to draw.
    empty: bool,
    runs: HashMap<RunKey, ShapedRun>,
    current: Option<ShapedRun>,
    missing: u64,
    asset_key: Option<Vec<(String, usize)>>,
    warned: std::collections::HashSet<(String, u16)>,
}

impl TextEngine {
    pub fn new() -> Self {
        let mut db = fontdb::Database::new();
        for face in FACES {
            db.load_font_data(face.to_vec());
        }
        let mut fonts = FontSystem::new_with_locale_and_db("en-US".to_string(), db);
        let mut buffer = Buffer::new(&mut fonts, Metrics::new(16.0, 16.0));
        buffer.set_wrap(Wrap::None);
        Self {
            fonts,
            swash: SwashCache::new(),
            buffer,
            empty: true,
            runs: HashMap::new(),
            current: None,
            missing: 0,
            asset_key: None,
            warned: Default::default(),
        }
    }

    /// Shapes `text` (one line per `\n`) at `px` pixels. Returns the box size in pixels:
    /// (advance of the longest line, px × lines). Sizes below 0.5 px lay out nothing.
    #[cfg(test)]
    pub fn layout(&mut self, text: &str, px: f32, weight: FontWeight) -> (f32, f32) {
        self.layout_family(text, px, weight, FAMILY)
    }

    pub fn configure(&mut self, layout: &actionlay_layout::Layout) -> bool {
        let mut key: Vec<_> = layout
            .loaded_assets
            .iter()
            .map(|(name, data)| (name.clone(), std::sync::Arc::as_ptr(data) as usize))
            .collect();
        let mut families = crate::fonts::requested(layout);
        families.insert(FAMILY.into());
        key.extend(families.iter().map(|name| (name.clone(), 0)));
        if self.asset_key.as_ref() == Some(&key) {
            return false;
        }
        let mut db = crate::fonts::database(layout);
        let unused: Vec<_> = db
            .faces()
            .filter(|face| {
                !face
                    .families
                    .iter()
                    .any(|(name, _)| families.contains(name))
            })
            .map(|face| face.id)
            .collect();
        for id in unused {
            db.remove_face(id);
        }
        self.fonts = FontSystem::new_with_locale_and_db("en-US".into(), db);
        self.buffer = Buffer::new(&mut self.fonts, Metrics::new(16.0, 16.0));
        self.buffer.set_wrap(Wrap::None);
        self.reset_cache();
        self.warned.clear();
        self.asset_key = Some(key);
        true
    }

    pub fn layout_family(
        &mut self,
        text: &str,
        px: f32,
        weight: FontWeight,
        family: &str,
    ) -> (f32, f32) {
        if !(px.is_finite() && px >= 0.5) {
            self.empty = true;
            return (0.0, 0.0);
        }
        let face = crate::fonts::query(self.fonts.db(), family, weight.value())
            .and_then(|id| self.fonts.db().face(id));
        let resolved = if face.is_some_and(|face| face.weight.0 == weight.value()) {
            family
        } else {
            if self.warned.insert((family.into(), weight.value())) {
                log::warn!(
                    "font {family} weight {} unavailable, using Roboto",
                    weight.value()
                );
            }
            FAMILY
        };
        let key = (
            text.to_owned(),
            resolved.to_owned(),
            px.to_bits(),
            weight.value(),
        );
        if let Some(run) = self.runs.get(&key) {
            self.empty = false;
            self.current = Some(run.clone());
            return run.size;
        }
        self.buffer
            .set_metrics_and_size(Metrics::new(px, px), None, None);
        let attrs = Attrs::new()
            .family(Family::Name(resolved))
            .weight(Weight(weight.value()));
        self.buffer.set_text(text, &attrs, Shaping::Advanced, None);
        self.buffer.shape_until_scroll(&mut self.fonts, false);
        let (mut width, mut lines) = (0.0f32, 0u32);
        for run in self.buffer.layout_runs() {
            width = width.max(run.line_w);
            lines += 1;
        }
        self.empty = false;
        let size = (width, px * lines as f32);
        let missing = self.missing;
        let path = self.outline_at(0.0, 0.0);
        let run = ShapedRun {
            size,
            path,
            missing: self.missing - missing,
        };
        self.missing = missing;
        if self.runs.len() >= 512 {
            self.runs.clear();
        }
        self.runs.insert(key, run.clone());
        self.current = Some(run);
        size
    }

    /// Outline of the last laid-out text, with the top-left corner of its box at (x, y).
    /// `None` when there is nothing to draw.
    pub fn path_at(&mut self, x: f32, y: f32) -> Option<Path> {
        if self.empty {
            return None;
        }
        let run = self.current.as_ref()?;
        self.missing += run.missing;
        run.path.clone()?.transform(Transform::from_translate(x, y))
    }

    fn outline_at(&mut self, x: f32, y: f32) -> Option<Path> {
        if self.empty {
            return None;
        }
        let mut pb = PathBuilder::new();
        for run in self.buffer.layout_runs() {
            for g in run.glyphs {
                if g.glyph_id == 0 {
                    // not in the embedded font: skip it (no tofu box)
                    self.missing += 1;
                    continue;
                }
                let (key, _, _) = CacheKey::new(
                    g.font_id,
                    g.glyph_id,
                    g.font_size,
                    (0.0, 0.0),
                    g.font_weight,
                    g.cache_key_flags | CacheKeyFlags::DISABLE_HINTING,
                );
                let Some(commands) = self.swash.get_outline_commands(&mut self.fonts, key) else {
                    continue;
                };
                let ox = x + g.x + g.font_size * g.x_offset;
                let oy = y + run.line_y + g.y - g.font_size * g.y_offset;
                push_outline(&mut pb, commands, ox, oy);
            }
        }
        pb.finish()
    }

    /// Glyphs skipped because the embedded font lacks them (since creation).
    pub fn missing_glyphs(&self) -> u64 {
        self.missing
    }

    /// Drops cached glyph outlines (they are per font size; call when the output size changes).
    pub fn reset_cache(&mut self) {
        self.swash = SwashCache::new();
        self.runs.clear();
        self.current = None;
        self.empty = true;
    }
}

/// Appends a glyph outline (y-up, origin on the baseline) at pen position (ox, oy).
fn push_outline(pb: &mut PathBuilder, commands: &[Command], ox: f32, oy: f32) {
    for c in commands {
        match *c {
            Command::MoveTo(p) => pb.move_to(ox + p.x, oy - p.y),
            Command::LineTo(p) => pb.line_to(ox + p.x, oy - p.y),
            Command::QuadTo(c1, p) => pb.quad_to(ox + c1.x, oy - c1.y, ox + p.x, oy - p.y),
            Command::CurveTo(c1, c2, p) => pb.cubic_to(
                ox + c1.x,
                oy - c1.y,
                ox + c2.x,
                oy - c2.y,
                ox + p.x,
                oy - p.y,
            ),
            Command::Close => pb.close(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic_text::fontdb::{Query, Weight as DbWeight};

    #[test]
    fn embedded_faces_resolve_by_weight_and_no_system_fonts_are_loaded() {
        let engine = TextEngine::new();
        let db = engine.fonts.db();
        assert_eq!(db.faces().count(), 3, "only the embedded Roboto faces");
        for (w, name) in [
            (400, "Roboto-Regular"),
            (500, "Roboto-Medium"),
            (700, "Roboto-Bold"),
        ] {
            let id = db
                .query(&Query {
                    families: &[Family::Name(FAMILY)],
                    weight: DbWeight(w),
                    ..Default::default()
                })
                .unwrap_or_else(|| panic!("no face for weight {w}"));
            assert_eq!(db.face(id).unwrap().post_script_name, name);
        }
    }

    #[test]
    fn required_glyphs_are_in_embedded_font() {
        let mut engine = TextEngine::new();
        for weight in [FontWeight::Regular, FontWeight::Medium, FontWeight::Bold] {
            engine.layout("0123456789 —°±+-.,:/%() km/h mph ft °C GPS", 40.0, weight);
            assert!(engine.path_at(0.0, 0.0).is_some());
        }
        assert_eq!(engine.missing_glyphs(), 0);
    }

    #[test]
    fn missing_glyphs_are_skipped_not_drawn_as_boxes() {
        let mut engine = TextEngine::new();
        let (w, _) = engine.layout("東京", 64.0, FontWeight::Regular);
        assert!(w >= 0.0);
        assert!(
            engine.path_at(0.0, 0.0).is_none(),
            "no outline for missing glyphs"
        );
        assert_eq!(engine.missing_glyphs(), 2);
        engine.layout("A東🚀", 64.0, FontWeight::Regular);
        assert!(
            engine.path_at(0.0, 0.0).is_some(),
            "the Latin glyph is still drawn"
        );
        assert!(engine.missing_glyphs() >= 3);
    }

    #[test]
    fn text_box_contains_the_glyphs() {
        let mut engine = TextEngine::new();
        let (w, h) = engine.layout("H8g", 100.0, FontWeight::Bold);
        assert_eq!(h, 100.0);
        assert!(w > 100.0 && w < 250.0, "width {w}");
        let b = engine.path_at(10.0, 20.0).unwrap().bounds();
        assert!(
            b.left() >= 10.0 - 1.0 && b.right() <= 10.0 + w + 1.0,
            "{b:?}"
        );
        // descenders (the 'g') may overshoot the line box by a few percent of the size
        assert!(
            b.top() >= 20.0 - 1.0 && b.bottom() <= 20.0 + h + 10.0,
            "{b:?}"
        );
        assert!(b.height() > 60.0, "glyphs are upright and full size: {b:?}");
        let (_, h2) = engine.layout("a\nb", 50.0, FontWeight::Regular);
        assert_eq!(h2, 100.0, "two lines");
    }

    #[test]
    fn digits_have_equal_advance_so_values_do_not_jitter() {
        let mut engine = TextEngine::new();
        let (w1, _) = engine.layout("111", 80.0, FontWeight::Bold);
        let (w8, _) = engine.layout("888", 80.0, FontWeight::Bold);
        assert!((w1 - w8).abs() < 0.01, "{w1} vs {w8}");
    }

    #[test]
    fn degenerate_sizes_lay_out_nothing() {
        let mut engine = TextEngine::new();
        for px in [0.0, 0.3, -5.0, f32::NAN, f32::INFINITY] {
            assert_eq!(
                engine.layout("12", px, FontWeight::Regular),
                (0.0, 0.0),
                "{px}"
            );
            assert!(engine.path_at(0.0, 0.0).is_none());
        }
    }

    #[test]
    fn text_engine_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<TextEngine>();
    }
}
