//! ActionLay overlay renderer: (layout, snapshot, size) → premultiplied RGBA (spec §4.5).
//! CPU only (tiny-skia), embedded fonts and icons: same output on every machine.
mod icons;
mod scene;
mod shapes;
mod text;
mod value;

use std::time::{Duration, Instant};

use actionlay_layout::Layout;
use actionlay_layout::format;
use actionlay_layout::geom::{Aspect, ScaleMode, root_box, scale_factor};
use actionlay_layout::model::{Node, Units, WhenAbsent, Widget};
use actionlay_layout::style::{ResolvedTheme, TextStyleOpt, Theme};
use actionlay_layout::validate::Issue;
use actionlay_telemetry::Snapshot;
use actionlay_telemetry::metric::Metric;
use actionlay_telemetry::units::{Unit, UnitSystem, units_for};
use chrono::{DateTime, FixedOffset, Local, Offset, TimeZone, Utc};
use tiny_skia::Pixmap;

pub use icons::IconId;
pub use text::FAMILY;
pub use tiny_skia;

use icons::IconCache;
use scene::{Ctx, FormatCache, Painter};
use text::TextEngine;

/// Time zone of `datetime` widgets set to `"local"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Zone {
    /// The machine's zone, including DST at the video's date.
    #[default]
    System,
    Fixed(FixedOffset),
}

impl Zone {
    pub fn offset_at(self, utc: DateTime<Utc>) -> FixedOffset {
        match self {
            Zone::System => Local.offset_from_utc_datetime(&utc.naive_utc()).fix(),
            Zone::Fixed(o) => o,
        }
    }
}

/// Time spent in the last `render_into`, by stage.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RenderStats {
    pub total: Duration,
    pub clear: Duration,
    pub text: Duration,
    pub icons: Duration,
    pub shapes: Duration,
}

/// Draws layouts. Holds the font system, glyph and icon caches and reusable buffers:
/// keep one per output and reuse it every frame.
pub struct Renderer {
    text: TextEngine,
    icons: IconCache,
    formats: FormatCache,
    zone: Zone,
    scale_mode: ScaleMode,
    stats: RenderStats,
    scratch: String,
    last_size: (u32, u32),
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            text: TextEngine::new(),
            icons: IconCache::new(),
            formats: FormatCache::default(),
            zone: Zone::System,
            scale_mode: ScaleMode::Height,
            stats: RenderStats::default(),
            scratch: String::new(),
            last_size: (0, 0),
        }
    }

    pub fn set_zone(&mut self, zone: Zone) {
        self.zone = zone;
    }

    pub fn set_scale_mode(&mut self, mode: ScaleMode) {
        self.scale_mode = mode;
    }

    pub fn last_stats(&self) -> RenderStats {
        self.stats
    }

    /// Characters drawn so far that the embedded font lacks (skipped, not drawn as boxes).
    pub fn missing_glyphs(&self) -> u64 {
        self.text.missing_glyphs()
    }

    /// Contract entry point: allocates the pixmap (sizes below 1 are clamped to 1).
    /// Per-frame callers should use [`Renderer::render_into`] with a reused pixmap.
    pub fn render(&mut self, layout: &Layout, snap: &Snapshot, width: u32, height: u32) -> Pixmap {
        let mut pixmap = Pixmap::new(width.max(1), height.max(1)).expect("non-zero pixmap size");
        self.render_into(layout, snap, &mut pixmap);
        pixmap
    }

    /// Clears `target` and draws the overlay at its size (premultiplied RGBA).
    pub fn render_into(&mut self, layout: &Layout, snap: &Snapshot, target: &mut Pixmap) {
        let start = Instant::now();
        let size = (target.width(), target.height());
        if size != self.last_size {
            // glyph outlines and icons are cached per pixel size
            self.text.reset_cache();
            self.icons.clear();
            self.last_size = size;
        }
        target.fill(tiny_skia::Color::TRANSPARENT);
        let mut stats = RenderStats {
            clear: start.elapsed(),
            ..RenderStats::default()
        };
        let (w, h) = (size.0 as f32, size.1 as f32);
        let aspect = layout.design_aspect.unwrap_or(Aspect::WIDESCREEN).ratio();
        let scale = scale_factor(self.scale_mode, w, h, aspect);
        let theme = layout
            .theme
            .as_ref()
            .map_or_else(ResolvedTheme::default, Theme::resolve);
        let system = match layout.units.unwrap_or_default() {
            Units::Metric => UnitSystem::Metric,
            Units::Imperial => UnitSystem::Imperial,
        };
        let ctx = Ctx {
            snap,
            theme: &theme,
            system,
            scale,
            zone: self.zone,
        };
        let mut painter = Painter {
            pixmap: target,
            text: &mut self.text,
            icons: &mut self.icons,
            formats: &mut self.formats,
            stats: &mut stats,
            scratch: &mut self.scratch,
        };
        scene::draw_nodes(
            &mut painter,
            &layout.nodes,
            root_box(w, h, scale),
            1.0,
            &ctx,
        );
        stats.total = start.elapsed();
        self.stats = stats;
    }
}

/// Warnings about what the renderer cannot honour: unknown metrics, units, icons and
/// fonts, units that do not fit their metric, and formats that do not parse (possible
/// only in layouts built in code). Such widgets render their empty state or fallback.
pub fn diagnose(layout: &Layout) -> Vec<Issue> {
    let mut issues = Vec::new();
    if let Some(font) = layout.theme.as_ref().and_then(|t| t.font.as_deref()) {
        check_font(font, "theme", &mut issues);
    }
    for (i, node) in layout.nodes.iter().enumerate() {
        diagnose_node(node, format!("nodes[{i}]"), &mut issues);
    }
    issues
}

fn check_font(font: &str, path: &str, issues: &mut Vec<Issue>) {
    if font != FAMILY {
        issues.push(Issue::warning(
            path,
            format!("font `{font}` is not available, using {FAMILY}"),
        ));
    }
}

fn check_style(style: &TextStyleOpt, path: &str, issues: &mut Vec<Issue>) {
    if let Some(font) = &style.font {
        check_font(font, path, issues);
    }
}

/// `if_unknown`: what the widget does with an unknown metric id (it is never available).
fn check_metric(
    metric: &str,
    units: Option<&str>,
    if_unknown: &str,
    path: &str,
    issues: &mut Vec<Issue>,
) {
    let Some(m) = Metric::from_id(metric) else {
        issues.push(Issue::warning(
            path,
            format!("unknown metric `{metric}`: {if_unknown}"),
        ));
        return;
    };
    let Some(u) = units else { return };
    match Unit::from_id(u) {
        None => issues.push(Issue::warning(
            path,
            format!("unknown unit `{u}`: using the default"),
        )),
        Some(unit) if !units_for(m.quantity()).contains(&unit) => issues.push(Issue::warning(
            path,
            format!("unit `{u}` does not fit metric `{metric}`: using the default"),
        )),
        Some(_) => {}
    }
}

fn diagnose_node(node: &Node, path: String, issues: &mut Vec<Issue>) {
    let Node::Known(w) = node else { return };
    let path = match node.id() {
        Some(id) => format!("{path} ({id})"),
        None => path,
    };
    match w {
        Widget::Metric(m) => {
            let if_unknown = match m.when_absent.unwrap_or_default() {
                WhenAbsent::Show => "shown as empty",
                WhenAbsent::Hide => "the widget is hidden (when_absent: hide)",
            };
            check_metric(&m.metric, m.units.as_deref(), if_unknown, &path, issues);
            if let Some(Err(e)) = m.format.as_deref().map(format::parse) {
                issues.push(Issue::warning(&path, format!("{e}: shown as empty")));
            }
            check_style(&m.style, &path, issues);
        }
        Widget::MetricUnit(m) => {
            let if_unknown = "no unit is drawn";
            check_metric(&m.metric, m.units.as_deref(), if_unknown, &path, issues);
            check_style(&m.style, &path, issues);
        }
        Widget::Text(t) => check_style(&t.style, &path, issues),
        Widget::Datetime(d) => {
            if let Some(f) = &d.format
                && !format::is_valid_strftime(f)
            {
                issues.push(Issue::warning(
                    &path,
                    format!("invalid strftime format `{f}`: shown as empty"),
                ));
            }
            check_style(&d.style, &path, issues);
        }
        Widget::Icon(i) if IconId::from_name(&i.icon).is_none() => {
            let names: Vec<&str> = IconId::ALL.iter().map(|i| i.name()).collect();
            issues.push(Issue::warning(
                &path,
                format!(
                    "unknown icon `{}` (available: {})",
                    i.icon,
                    names.join(", ")
                ),
            ));
        }
        _ => {}
    }
    for (i, child) in w.children().iter().enumerate() {
        diagnose_node(child, format!("{path}.children[{i}]"), issues);
    }
}
