//! ActionLay overlay renderer: (layout, snapshot, size) → premultiplied RGBA (spec §4.5).
//! CPU only (tiny-skia), document-scoped fonts and embedded fallback fonts/icons.
mod dials;
pub mod fonts;
mod history;
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
use actionlay_telemetry::metric::Metric;
use actionlay_telemetry::units::{Unit, UnitSystem, units_for};
use actionlay_telemetry::{Snapshot, Telemetry};
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
    boxes: Vec<HitBox>,
    collect_boxes: bool,
    text: TextEngine,
    icons: IconCache,
    formats: FormatCache,
    zone: Zone,
    scale_mode: ScaleMode,
    stats: RenderStats,
    scratch: String,
    last_size: (u32, u32),
    headings: history::HeadingCache,
    statics: history::StaticCache,
    maps: actionlay_maps::TileStore,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            boxes: Vec::new(),
            collect_boxes: false,
            text: TextEngine::new(),
            icons: IconCache::new(),
            formats: FormatCache::default(),
            zone: Zone::System,
            scale_mode: ScaleMode::Height,
            stats: RenderStats::default(),
            scratch: String::new(),
            last_size: (0, 0),
            headings: Default::default(),
            statics: Default::default(),
            maps: actionlay_maps::TileStore::offline(),
        }
    }

    pub fn set_maps(&mut self, maps: actionlay_maps::TileStore) {
        self.maps = maps;
    }

    /// Selection geometry from the same draw traversal, in layout units.
    pub fn hit_boxes(&self) -> &[HitBox] {
        &self.boxes
    }

    pub fn render_editor_into(
        &mut self,
        layout: &Layout,
        telemetry: &Telemetry,
        t: f64,
        target: &mut Pixmap,
    ) {
        self.boxes.clear();
        self.collect_boxes = true;
        self.render_telemetry_into(layout, telemetry, t, target);
        self.collect_boxes = false;
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
        self.render_scene(layout, snap, None, target);
    }

    pub fn render_telemetry_into(
        &mut self,
        layout: &Layout,
        telemetry: &Telemetry,
        t: f64,
        target: &mut Pixmap,
    ) {
        let snap = telemetry.sample(t);
        self.render_scene(layout, &snap, Some(telemetry), target);
    }

    fn render_scene(
        &mut self,
        layout: &Layout,
        snap: &Snapshot,
        telemetry: Option<&Telemetry>,
        target: &mut Pixmap,
    ) {
        let start = Instant::now();
        if self.text.configure(layout) {
            self.statics = Default::default();
        }
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
            telemetry,
            maps: &self.maps,
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
            headings: &mut self.headings,
            statics: &mut self.statics,
            boxes: self.collect_boxes.then_some(&mut self.boxes),
            path: Vec::new(),
            hit_index: None,
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

#[derive(Clone, Debug)]
pub struct HitBox {
    pub path: Vec<usize>,
    pub parent: actionlay_layout::geom::Rect,
    pub rect: actionlay_layout::geom::Rect,
}

/// Warnings about what the renderer cannot honour: unknown metrics, units, icons and
/// fonts, units that do not fit their metric, and formats that do not parse (possible
/// only in layouts built in code). Such widgets render their empty state or fallback.
pub fn diagnose(layout: &Layout) -> Vec<Issue> {
    let mut issues = Vec::new();
    let db = fonts::database(layout);
    issues.extend(
        fonts::warnings(layout)
            .into_iter()
            .filter(|message| message.starts_with("Unusable") || message.contains(" weight "))
            .map(|message| Issue::warning("fonts", message)),
    );
    if let Some(font) = layout.theme.as_ref().and_then(|t| t.font.as_deref()) {
        check_font(font, "theme", &mut issues, &db);
    }
    for (i, node) in layout.nodes.iter().enumerate() {
        diagnose_node(node, format!("nodes[{i}]"), &mut issues, &db);
    }
    issues
}

fn check_font(font: &str, path: &str, issues: &mut Vec<Issue>, db: &fontdb::Database) {
    if fonts::query(db, font, 400).is_none() {
        issues.push(Issue::warning(
            path,
            format!("font `{font}` is not available, using {FAMILY}"),
        ));
    }
}

fn check_style(style: &TextStyleOpt, path: &str, issues: &mut Vec<Issue>, db: &fontdb::Database) {
    if let Some(font) = &style.font {
        check_font(font, path, issues, db);
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

fn diagnose_node(node: &Node, path: String, issues: &mut Vec<Issue>, db: &fontdb::Database) {
    let Node::Known(w) = node else { return };
    let path = match node.id() {
        Some(id) => format!("{path} ({id})"),
        None => path,
    };
    match w {
        Widget::Chart(c) | Widget::GradientChart(c) => {
            check_metric(
                &c.metric,
                c.units.as_deref(),
                "chart shows No data",
                &path,
                issues,
            );
            if let Some(style) = &c.value_style {
                check_style(style, &path, issues, db);
            }
        }
        Widget::GMeter(g) => {
            check_metric(
                "accel.lon",
                g.units.as_deref(),
                "G-meter shows No data",
                &path,
                issues,
            );
        }
        Widget::Gauge(actionlay_layout::model::GaugeNode { dial: d, .. })
        | Widget::Compass(actionlay_layout::model::CompassNode { dial: d, .. }) => {
            check_metric(
                &d.metric,
                d.units.as_deref(),
                "shown as empty or hidden by when_absent",
                &path,
                issues,
            );
            if let Widget::Compass(_) = w
                && Metric::from_id(&d.metric)
                    .is_some_and(|m| m.quantity() != actionlay_telemetry::units::Quantity::Angle)
            {
                issues.push(Issue::warning(
                    &path,
                    "compass requires an angular metric: shown as empty",
                ));
            }
            for style in [&d.value_style, &d.label_style].into_iter().flatten() {
                check_style(style, &path, issues, db);
            }
            if let Some(Err(e)) = d.format.as_deref().map(format::parse) {
                issues.push(Issue::warning(&path, format!("{e}: shown as empty")));
            }
        }
        Widget::Bar(b) | Widget::ZoneBar(actionlay_layout::model::ZoneBarNode { bar: b, .. }) => {
            check_metric(
                &b.metric,
                b.units.as_deref(),
                "shown as empty or hidden by when_absent",
                &path,
                issues,
            );
            if let Some(style) = &b.value_style {
                check_style(style, &path, issues, db);
            }
            if let Some(Err(e)) = b.format.as_deref().map(format::parse) {
                issues.push(Issue::warning(&path, format!("{e}: shown as empty")));
            }
        }
        Widget::Metric(m) => {
            let if_unknown = match m.when_absent.unwrap_or_default() {
                WhenAbsent::Show => "shown as empty",
                WhenAbsent::Hide => "the widget is hidden (when_absent: hide)",
            };
            check_metric(&m.metric, m.units.as_deref(), if_unknown, &path, issues);
            if let Some(Err(e)) = m.format.as_deref().map(format::parse) {
                issues.push(Issue::warning(&path, format!("{e}: shown as empty")));
            }
            check_style(&m.style, &path, issues, db);
        }
        Widget::MetricUnit(m) => {
            let if_unknown = "no unit is drawn";
            check_metric(&m.metric, m.units.as_deref(), if_unknown, &path, issues);
            check_style(&m.style, &path, issues, db);
        }
        Widget::Text(t) => check_style(&t.style, &path, issues, db),
        Widget::Datetime(d) => {
            if let Some(f) = &d.format
                && !format::is_valid_strftime(f)
            {
                issues.push(Issue::warning(
                    &path,
                    format!("invalid strftime format `{f}`: shown as empty"),
                ));
            }
            check_style(&d.style, &path, issues, db);
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
        diagnose_node(child, format!("{path}.children[{i}]"), issues, db);
    }
}
