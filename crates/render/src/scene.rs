//! Walks the layout tree, places every node (layout units → pixels) and draws it.
//!
//! Nothing is clipped: glyph ink overflows its line box (~0.11 em below, ~0.21 em
//! above, plus outline and shadow), so clipping to a frame or to the text box would cut
//! descenders. Layouts keep a margin instead (the default layout uses 24 units).
use std::collections::HashMap;
use std::fmt::Write as _;
use std::time::Instant;

use actionlay_layout::color::{Color, ColorRef};
use actionlay_layout::format::{self, Piece};
use actionlay_layout::geom::{Anchor, Rect, place};
use actionlay_layout::model::{
    DateZone, DatetimeNode, FrameNode, GpsLockIconNode, MetricNode, MetricUnitNode, Node,
    WhenAbsent, Widget,
};
use actionlay_layout::style::{ResolvedTheme, TextKind, TextStyle, defaults};
use actionlay_telemetry::units::UnitSystem;
use actionlay_telemetry::{GpsLock, Snapshot};
use chrono::FixedOffset;
use chrono::format::{Item, StrftimeItems};
use tiny_skia::{FillRule, LineJoin, Path, Pixmap, PixmapPaint, Stroke, Transform};

use crate::icons::{IconCache, IconId};
use crate::shapes::{paint, rounded_rect};
use crate::text::TextEngine;
use crate::value::{self, Shown};
use crate::{RenderStats, Zone};

/// Parsed metric formats by format string; `None` for one that does not parse.
/// Formats are parsed once, not per frame.
#[derive(Default)]
pub(crate) struct FormatCache {
    map: HashMap<String, Option<Vec<Piece>>>,
}

/// Bound on cached formats (layouts edited live keep adding new strings).
const MAX_FORMATS: usize = 256;

impl FormatCache {
    fn get(&mut self, fmt: &str) -> Option<&[Piece]> {
        if !self.map.contains_key(fmt) {
            if self.map.len() >= MAX_FORMATS {
                self.map.clear();
            }
            let parsed = match format::parse(fmt) {
                Ok(p) => Some(p),
                Err(e) => {
                    // reported once per format string; `diagnose` lists it too
                    log::warn!("{e}: the widget shows its empty state");
                    None
                }
            };
            self.map.insert(fmt.to_string(), parsed);
        }
        self.map.get(fmt).and_then(|p| p.as_deref())
    }
}

/// Per-frame inputs shared by every node.
pub(crate) struct Ctx<'a> {
    pub snap: &'a Snapshot,
    pub theme: &'a ResolvedTheme,
    pub system: UnitSystem,
    /// Pixels per layout unit.
    pub scale: f32,
    pub zone: Zone,
}

/// Mutable drawing state (split borrows of the renderer).
pub(crate) struct Painter<'p> {
    pub pixmap: &'p mut Pixmap,
    pub text: &'p mut TextEngine,
    pub icons: &'p mut IconCache,
    pub formats: &'p mut FormatCache,
    pub stats: &'p mut RenderStats,
    /// Text of the widget being drawn (reused, no per-frame allocation).
    pub scratch: &'p mut String,
}

/// Where a leaf widget goes: anchored in `parent` (layout units).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Placement {
    parent: Rect,
    anchor: Anchor,
    offset: [f32; 2],
    opacity: f32,
}

pub(crate) fn draw_nodes(p: &mut Painter, nodes: &[Node], parent: Rect, opacity: f32, ctx: &Ctx) {
    for node in nodes {
        // nodes of unknown types are kept in the layout but not drawn
        let Node::Known(w) = node else { continue };
        let c = w.common();
        // opacity multiplies down the tree, applied per primitive (no offscreen layers)
        let opacity = opacity * c.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
        if c.visible == Some(false) || opacity <= 0.0 {
            continue;
        }
        let at = Placement {
            parent,
            anchor: c.anchor.unwrap_or_default(),
            offset: c.offset.unwrap_or([0.0, 0.0]),
            opacity,
        };
        match w {
            Widget::Group(g) => {
                let r = place(parent, at.anchor, at.offset, g.size.unwrap_or([0.0, 0.0]));
                draw_nodes(p, &g.children, r, opacity, ctx);
            }
            Widget::Frame(f) => {
                let r = place(parent, at.anchor, at.offset, f.size);
                draw_frame(p, f, r, opacity, ctx);
                draw_nodes(p, &f.children, r, opacity, ctx);
            }
            Widget::Text(t) => {
                p.scratch.clear();
                p.scratch.push_str(&t.text);
                draw_text(
                    p,
                    &t.style.resolve(TextKind::Text, ctx.theme),
                    false,
                    at,
                    ctx,
                );
            }
            Widget::Metric(m) => draw_metric(p, m, at, ctx),
            Widget::MetricUnit(m) => draw_metric_unit(p, m, at, ctx),
            Widget::Datetime(d) => draw_datetime(p, d, at, ctx),
            Widget::Icon(i) => {
                // unknown icon names are reported by `diagnose` and not drawn
                if let Some(id) = IconId::from_name(&i.icon) {
                    let role = ColorRef::Role(defaults::ICON_ROLE);
                    let color = ctx.theme.color(i.color.unwrap_or(role));
                    draw_icon(p, id, i.size.unwrap_or(defaults::ICON_SIZE), color, at, ctx);
                }
            }
            Widget::GpsLockIcon(g) => draw_gps_lock(p, g, at, ctx),
        }
    }
}

/// Draws `p.scratch` with `style`, anchored by its line box (not by its ink, so values
/// do not jump as their digits change).
fn draw_text(p: &mut Painter, style: &TextStyle, dim: bool, at: Placement, ctx: &Ctx) {
    let start = Instant::now();
    let s = ctx.scale;
    let (w_px, h_px) = p.text.layout(p.scratch, style.size * s, style.weight);
    if w_px > 0.0 {
        let r = place(at.parent, at.anchor, at.offset, [w_px / s, h_px / s]);
        if let Some(path) = p.text.path_at(r.x * s, r.y * s) {
            let alpha = at.opacity * if dim { ctx.theme.dim_opacity } else { 1.0 };
            fill_text(p.pixmap, &path, style, alpha, s);
        }
    }
    p.stats.text += start.elapsed();
}

fn fill_text(pm: &mut Pixmap, path: &Path, style: &TextStyle, alpha: f32, s: f32) {
    let id = Transform::identity();
    let [dx, dy] = style.shadow.offset;
    if style.shadow.color.a > 0 && (dx != 0.0 || dy != 0.0) {
        let shift = Transform::from_translate(dx * s, dy * s);
        pm.fill_path(
            path,
            &paint(style.shadow.color, alpha),
            FillRule::Winding,
            shift,
            None,
        );
    }
    if style.outline.width > 0.0 && style.outline.color.a > 0 {
        // the stroke is centred on the outline: twice the width gives `width` outside
        let stroke = Stroke {
            width: 2.0 * style.outline.width * s,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };
        pm.stroke_path(path, &paint(style.outline.color, alpha), &stroke, id, None);
    }
    pm.fill_path(
        path,
        &paint(style.color, alpha),
        FillRule::Winding,
        id,
        None,
    );
}

fn draw_metric(p: &mut Painter, m: &MetricNode, at: Placement, ctx: &Ctx) {
    let when_absent = m.when_absent.unwrap_or_default();
    let resolved = value::resolve(&m.metric, m.units.as_deref(), ctx.system);
    let grace = f64::from(m.stale_secs.unwrap_or(defaults::STALE_SECS));
    let shown = match resolved {
        Some(r) => value::shown(
            ctx.snap.get(r.metric),
            ctx.snap.is_available(r.metric),
            grace,
            when_absent,
        ),
        None => value::shown_unknown(when_absent),
    };
    let display = match shown {
        Shown::Hidden => return,
        Shown::Value(v) => resolved.map(|r| r.display(v)).filter(|d| d.is_finite()),
        Shown::Empty => None,
    };
    let symbol = resolved.map_or("", |r| r.symbol);
    let fmt = m.format.as_deref().unwrap_or(defaults::METRIC_FORMAT);
    p.scratch.clear();
    // a format that does not parse (only possible in a layout built in code: loading
    // rejects it) shows the plain empty state
    let display = match p.formats.get(fmt) {
        Some(pieces) => {
            format::apply(p.scratch, pieces, display, symbol);
            display
        }
        None => {
            p.scratch.push_str(format::EMPTY);
            None
        }
    };
    let style = m.style.resolve(TextKind::Metric, ctx.theme);
    draw_text(p, &style, display.is_none(), at, ctx);
}

fn draw_metric_unit(p: &mut Painter, m: &MetricUnitNode, at: Placement, ctx: &Ctx) {
    // an unknown metric has no unit to show (`diagnose` reports it)
    let Some(r) = value::resolve(&m.metric, m.units.as_deref(), ctx.system) else {
        return;
    };
    let grace = f64::from(defaults::STALE_SECS);
    let shown = value::shown(
        ctx.snap.get(r.metric),
        ctx.snap.is_available(r.metric),
        grace,
        m.when_absent.unwrap_or_default(),
    );
    let dim = match shown {
        Shown::Hidden => return,
        Shown::Value(v) => !r.display(v).is_finite(),
        Shown::Empty => true,
    };
    if r.symbol.is_empty() {
        return;
    }
    p.scratch.clear();
    p.scratch.push_str(r.symbol);
    let style = m.style.resolve(TextKind::MetricUnit, ctx.theme);
    draw_text(p, &style, dim, at, ctx);
}

/// True when `fmt` is a valid strftime format.
pub(crate) fn strftime_ok(fmt: &str) -> bool {
    !StrftimeItems::new(fmt).any(|i| matches!(i, Item::Error))
}

fn draw_datetime(p: &mut Painter, d: &DatetimeNode, at: Placement, ctx: &Ctx) {
    // `utc` is None only when the video has no time at all (the "never available" case)
    if ctx.snap.utc.is_none() && d.when_absent.unwrap_or_default() == WhenAbsent::Hide {
        return;
    }
    let fmt = d.format.as_deref().unwrap_or(defaults::DATETIME_FORMAT);
    p.scratch.clear();
    let present = match ctx.snap.utc {
        Some(utc) if strftime_ok(fmt) => {
            let offset = match d.timezone.unwrap_or_default() {
                DateZone::Utc => FixedOffset::east_opt(0).expect("zero offset"),
                DateZone::Local => ctx.zone.offset_at(utc),
            };
            // write! (not to_string): a format error is an Err, never a panic
            write!(p.scratch, "{}", utc.with_timezone(&offset).format(fmt)).is_ok()
        }
        _ => false,
    };
    if !present {
        p.scratch.clear();
        p.scratch.push_str(format::EMPTY);
    }
    let style = d.style.resolve(TextKind::Datetime, ctx.theme);
    draw_text(p, &style, !present, at, ctx);
}

fn draw_frame(p: &mut Painter, f: &FrameNode, r: Rect, opacity: f32, ctx: &Ctx) {
    let start = Instant::now();
    let s = ctx.scale;
    let id = Transform::identity();
    let (x, y, w, h) = (r.x * s, r.y * s, r.w * s, r.h * s);
    let radius = f.radius.unwrap_or(defaults::FRAME_RADIUS) * s;
    let fill = ctx
        .theme
        .color(f.fill.unwrap_or(ColorRef::Role(defaults::FRAME_FILL)));
    if fill.a > 0
        && let Some(path) = rounded_rect(x, y, w, h, radius)
    {
        p.pixmap
            .fill_path(&path, &paint(fill, opacity), FillRule::Winding, id, None);
    }
    let border = f.border.as_ref();
    let bw = border
        .and_then(|b| b.width)
        .unwrap_or(defaults::BORDER_WIDTH)
        * s;
    if bw > 0.0 {
        let role = ColorRef::Role(defaults::BORDER_ROLE);
        let color = ctx
            .theme
            .color(border.and_then(|b| b.color).unwrap_or(role));
        // the stroke lies inside the frame's box
        let half = bw / 2.0;
        if let Some(path) =
            rounded_rect(x + half, y + half, w - bw, h - bw, (radius - half).max(0.0))
        {
            let stroke = Stroke {
                width: bw,
                ..Stroke::default()
            };
            p.pixmap
                .stroke_path(&path, &paint(color, opacity), &stroke, id, None);
        }
    }
    p.stats.shapes += start.elapsed();
}

/// Icon `size_units` wide, with a halo in the theme outline colour.
fn draw_icon(p: &mut Painter, id: IconId, size_units: f32, color: Color, at: Placement, ctx: &Ctx) {
    let start = Instant::now();
    let s = ctx.scale;
    let px = (size_units * s).round();
    if px >= 1.0 {
        let r = place(at.parent, at.anchor, at.offset, [size_units, size_units]);
        let (x, y) = ((r.x * s).round() as i32, (r.y * s).round() as i32);
        let pp = PixmapPaint {
            opacity: at.opacity,
            ..PixmapPaint::default()
        };
        let outline = ctx.theme.outline;
        let halo = (outline.width * s).round() as u32;
        if halo > 0
            && outline.color.a > 0
            && let Some(h) = p.icons.get(id, px as u32, outline.color, halo)
        {
            let o = halo as i32;
            p.pixmap
                .draw_pixmap(x - o, y - o, h.as_ref(), &pp, Transform::identity(), None);
        }
        if let Some(icon) = p.icons.get(id, px as u32, color, 0) {
            p.pixmap
                .draw_pixmap(x, y, icon.as_ref(), &pp, Transform::identity(), None);
        }
    }
    p.stats.icons += start.elapsed();
}

/// Satellite in the lock colour with a 3D fix, in the primary colour with a 2D fix;
/// without a fix (or without GPS) the crossed-out satellite, dimmed (its empty state).
fn draw_gps_lock(p: &mut Painter, g: &GpsLockIconNode, mut at: Placement, ctx: &Ctx) {
    let locked = ctx
        .theme
        .color(g.color.unwrap_or(ColorRef::Role(defaults::ICON_ROLE)));
    let (id, color) = match ctx.snap.gps_lock {
        GpsLock::Lock3d => (IconId::Gps, locked),
        GpsLock::Lock2d => (IconId::Gps, ctx.theme.primary),
        GpsLock::NoLock | GpsLock::Unknown => {
            at.opacity *= ctx.theme.dim_opacity;
            (IconId::GpsOff, ctx.theme.primary)
        }
    };
    draw_icon(p, id, g.size.unwrap_or(defaults::ICON_SIZE), color, at, ctx);
}
