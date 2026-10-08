//! Circular instruments share the same placement, units and missing-data policy as metrics.
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use actionlay_layout::color::{Color, ColorRef};
use actionlay_layout::format;
use actionlay_layout::geom::{Anchor, Rect, place};
use actionlay_layout::model::{CompassMode, CompassNode, DialNode, GaugeMode, GaugeNode};
use actionlay_layout::style::{TextKind, TextStyleOpt};
use actionlay_telemetry::units::Quantity;
use tiny_skia::{FillRule, LineCap, PathBuilder, Stroke, Transform};

use crate::scene::{Ctx, Painter, Placement, draw_text};
use crate::shapes::paint;
use crate::value::{self, Resolved, Shown};

/// Clockwise screen angle, from the positive X axis.
fn point(center: [f32; 2], radius: f32, degrees: f32) -> [f32; 2] {
    let a = degrees.to_radians();
    [center[0] + radius * a.cos(), center[1] + radius * a.sin()]
}

fn heading(value: f64) -> f32 {
    value.rem_euclid(360.0) as f32
}

fn fraction(value: f64, min: f64, max: f64) -> f32 {
    ((value - min) / (max - min)).clamp(0.0, 1.0) as f32
}

fn state(d: &DialNode, ctx: &Ctx, angular: bool) -> (Option<Resolved>, Shown) {
    let resolved = value::resolve(&d.metric, d.units.as_deref(), ctx.system)
        .filter(|r| !angular || r.metric.quantity() == Quantity::Angle);
    let policy = d.when_absent.unwrap_or_default();
    let shown = resolved.map_or_else(
        || value::shown_unknown(policy),
        |r| {
            value::shown(
                ctx.snap.get(r.metric),
                ctx.snap.is_available(r.metric),
                d.stale_secs.unwrap_or(3.0) as f64,
                policy,
            )
        },
    );
    (resolved, shown)
}

fn display(resolved: Option<Resolved>, shown: Shown) -> Option<f64> {
    match shown {
        Shown::Value(v) | Shown::Dimmed(v) => {
            resolved.map(|r| r.display(v)).filter(|v| v.is_finite())
        }
        _ => None,
    }
}

fn line(p: &mut Painter, from: [f32; 2], to: [f32; 2], width: f32, color: Color, alpha: f32) {
    let mut b = PathBuilder::new();
    b.move_to(from[0], from[1]);
    b.line_to(to[0], to[1]);
    if let Some(path) = b.finish() {
        p.pixmap.stroke_path(
            &path,
            &paint(color, alpha),
            &Stroke {
                width,
                line_cap: LineCap::Round,
                ..Default::default()
            },
            Transform::identity(),
            None,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn arc(
    p: &mut Painter,
    center: [f32; 2],
    radius: f32,
    angles: (f32, f32),
    width: f32,
    color: Color,
    alpha: f32,
) {
    let (start, sweep) = angles;
    if sweep == 0.0 {
        return;
    }
    // Cubic segments of at most 45 degrees: resolution independent and smooth at 4K.
    let steps = (sweep.abs() / 45.0).ceil() as u32;
    let step = sweep / steps as f32;
    let mut b = PathBuilder::new();
    let first = point(center, radius, start);
    b.move_to(first[0], first[1]);
    for i in 0..steps {
        let a = (start + step * i as f32).to_radians();
        let z = a + step.to_radians();
        let k = 4.0 / 3.0 * ((z - a) / 4.0).tan();
        let [cx, cy] = center;
        b.cubic_to(
            cx + radius * (a.cos() - k * a.sin()),
            cy + radius * (a.sin() + k * a.cos()),
            cx + radius * (z.cos() + k * z.sin()),
            cy + radius * (z.sin() - k * z.cos()),
            cx + radius * z.cos(),
            cy + radius * z.sin(),
        );
    }
    if sweep.abs() == 360.0 {
        b.close();
    }
    if let Some(path) = b.finish() {
        p.pixmap.stroke_path(
            &path,
            &paint(color, alpha),
            &Stroke {
                width,
                line_cap: LineCap::Round,
                ..Default::default()
            },
            Transform::identity(),
            None,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn label(
    p: &mut Painter,
    text: &str,
    pos: [f32; 2],
    rect: Rect,
    opt: Option<&TextStyleOpt>,
    size: f32,
    dim: bool,
    opacity: f32,
    ctx: &Ctx,
) {
    p.scratch.clear();
    p.scratch.push_str(text);
    text_at(p, pos, rect, opt, size, dim, opacity, TextKind::Text, ctx);
}

#[allow(clippy::too_many_arguments)]
fn text_at(
    p: &mut Painter,
    pos: [f32; 2],
    rect: Rect,
    opt: Option<&TextStyleOpt>,
    size: f32,
    dim: bool,
    opacity: f32,
    kind: TextKind,
    ctx: &Ctx,
) {
    let fallback = TextStyleOpt::default();
    let opt = opt.unwrap_or(&fallback);
    let mut style = opt.resolve(kind, ctx.theme);
    if opt.size.is_none() {
        style.size = size;
    }
    draw_text(
        p,
        &style,
        dim,
        Placement {
            parent: Rect::new(rect.x + pos[0], rect.y + pos[1], 0.0, 0.0),
            anchor: Anchor::Center,
            offset: [0.0, 0.0],
            opacity,
        },
        ctx,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_value(
    p: &mut Painter,
    d: &DialNode,
    value: Option<f64>,
    symbol: &str,
    rect: Rect,
    y: f32,
    dim: bool,
    opacity: f32,
    ctx: &Ctx,
) {
    if d.show_value == Some(false) {
        return;
    }
    p.scratch.clear();
    if let Some(pieces) = p
        .formats
        .get(d.format.as_deref().unwrap_or("{value:.0} {unit}"))
    {
        format::apply(p.scratch, pieces, value, symbol);
    } else {
        p.scratch.push_str(format::EMPTY);
    }
    text_at(
        p,
        [rect.w / 2.0, y],
        rect,
        d.value_style.as_ref(),
        rect.w * 0.12,
        dim,
        opacity,
        TextKind::Metric,
        ctx,
    );
}

pub(crate) fn draw_gauge(p: &mut Painter, g: &GaugeNode, at: Placement, ctx: &Ctx) {
    let d = &g.dial;
    let (resolved, shown) = state(d, ctx, false);
    if shown == Shown::Hidden {
        return;
    }
    let (min, max) = g.range();
    if !(min.is_finite() && max.is_finite() && max > min) {
        return;
    }
    let value = display(resolved, shown);
    let dim = !matches!(shown, Shown::Value(_));
    let alpha = at.opacity * if dim { ctx.theme.dim_opacity } else { 1.0 };
    let diameter = d.diameter();
    let rect = place(at.parent, at.anchor, at.offset, [diameter; 2]);
    let s = ctx.scale;
    let center = [(rect.x + diameter / 2.0) * s, (rect.y + diameter / 2.0) * s];
    let radius = diameter * 0.42 * s;
    let width = d.thickness() * s;
    let fill = d.fill.map_or(ctx.theme.accent, |c| ctx.theme.color(c));
    let track = d
        .track
        .map_or(Color::rgba(255, 255, 255, 38), |c| ctx.theme.color(c));
    let mode = g.mode.unwrap_or_default();
    let (start, sweep) = g.angles();
    let sweep = if g.clockwise == Some(false) {
        -sweep
    } else {
        sweep
    };
    let t = Instant::now();
    arc(p, center, radius, (start, sweep), width, track, alpha);
    if mode != GaugeMode::Needle
        && let Some(v) = value
    {
        arc(
            p,
            center,
            radius,
            (start, sweep * fraction(v, min, max)),
            width,
            fill,
            alpha,
        );
    }
    let ticks = g
        .ticks
        .unwrap_or(if mode == GaugeMode::Donut { 0 } else { 10 })
        .min(72);
    for i in 0..=ticks {
        if ticks == 0 || (sweep == 360.0 && i == ticks) {
            break;
        }
        let a = start + sweep * i as f32 / ticks as f32;
        line(
            p,
            point(center, radius - width * 0.9, a),
            point(center, radius - width * 0.9 - diameter * s * 0.035, a),
            diameter * s * 0.007,
            ctx.theme.secondary,
            alpha,
        );
    }
    if matches!(mode, GaugeMode::Needle | GaugeMode::Marker) {
        // An empty gauge parks the dimmed needle at the minimum, and shows a dash.
        let a = start + sweep * value.map_or(0.0, |v| fraction(v, min, max));
        if mode == GaugeMode::Marker {
            let tip = point(center, radius, a);
            if let Some(path) = PathBuilder::from_circle(tip[0], tip[1], width * 0.65) {
                p.pixmap.fill_path(
                    &path,
                    &paint(fill, alpha),
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
        } else {
            line(
                p,
                point(center, -diameter * s * 0.05, a),
                point(center, radius * 0.69, a),
                diameter * s * 0.018,
                fill,
                alpha,
            );
            if let Some(path) = PathBuilder::from_circle(center[0], center[1], diameter * s * 0.025)
            {
                p.pixmap.fill_path(
                    &path,
                    &paint(fill, alpha),
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
        }
    }
    p.stats.shapes += t.elapsed();
    if ticks > 0 && g.show_labels != Some(false) {
        for i in 0..=ticks {
            if i % 2 != 0 && i != ticks {
                continue;
            }
            if sweep.abs() == 360.0 && i == ticks {
                break;
            }
            let a = start + sweep * i as f32 / ticks as f32;
            let xy = point([diameter / 2.0; 2], diameter * 0.30, a);
            let text = format!("{:.0}", min + (max - min) * i as f64 / ticks as f64);
            label(
                p,
                &text,
                xy,
                rect,
                d.label_style.as_ref(),
                diameter * 0.055,
                dim,
                at.opacity,
                ctx,
            );
        }
    }
    draw_value(
        p,
        d,
        value,
        resolved.map_or("", |r| r.symbol),
        rect,
        diameter * if mode == GaugeMode::Donut { 0.5 } else { 0.91 },
        dim,
        at.opacity,
        ctx,
    );
}

pub(crate) fn draw_compass(p: &mut Painter, c: &CompassNode, at: Placement, ctx: &Ctx) {
    let d = &c.dial;
    let (resolved, shown) = state(d, ctx, true);
    if shown == Shown::Hidden {
        return;
    }
    let mut value = display(resolved, shown).map(|v| heading(v) as f64);
    if let (Some(tel), Some(filter), Some(r), Some(_)) = (
        ctx.telemetry,
        c.smoothing.as_ref().filter(|f| f.enabled != Some(false)),
        resolved,
        value,
    ) {
        value = p
            .headings
            .sample(tel, r.metric, ctx.snap.t, filter)
            .or(value);
    }
    let dim = !matches!(shown, Shown::Value(_));
    let alpha = at.opacity * if dim { ctx.theme.dim_opacity } else { 1.0 };
    let diameter = d.diameter();
    let rect = place(at.parent, at.anchor, at.offset, [diameter; 2]);
    let s = ctx.scale;
    let center = [(rect.x + diameter / 2.0) * s, (rect.y + diameter / 2.0) * s];
    let radius = diameter * 0.42 * s;
    let fill = d.fill.map_or(ctx.theme.accent, |r| ctx.theme.color(r));
    let track = d
        .track
        .map_or(Color::rgba(255, 255, 255, 38), |r| ctx.theme.color(r));
    let rotation = if c.rotate_rose == Some(true) {
        -value.unwrap_or(0.0) as f32
    } else {
        0.0
    };
    let rose = c.mode.unwrap_or_default() == CompassMode::Rose;
    let t = Instant::now();
    arc(
        p,
        center,
        radius,
        (-90.0, 360.0),
        d.thickness() * s,
        track,
        alpha,
    );
    if rose {
        for i in 0..24 {
            let a = i as f32 * 15.0 - 90.0 + rotation;
            line(
                p,
                point(center, radius * 0.88, a),
                point(center, radius * if i % 6 == 0 { 0.76 } else { 0.83 }, a),
                diameter * s * 0.006,
                ctx.theme.secondary,
                alpha,
            );
        }
    }
    if let Some(v) = value {
        let a = v as f32 - 90.0 + rotation;
        let tip = point(center, radius * 0.58, a);
        let left = point(center, diameter * s * 0.075, a + 125.0);
        let right = point(center, diameter * s * 0.075, a - 125.0);
        let mut b = PathBuilder::new();
        b.move_to(tip[0], tip[1]);
        b.line_to(left[0], left[1]);
        b.line_to(center[0], center[1]);
        b.line_to(right[0], right[1]);
        b.close();
        if let Some(path) = b.finish() {
            p.pixmap.fill_path(
                &path,
                &paint(fill, alpha),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
    p.stats.shapes += t.elapsed();
    for (i, name) in ["N", "E", "S", "W"].iter().enumerate() {
        if !rose && i != 0 {
            continue;
        }
        let xy = point(
            [diameter / 2.0; 2],
            diameter * 0.32,
            i as f32 * 90.0 - 90.0 + rotation,
        );
        let mut opt = d.label_style.clone().unwrap_or_default();
        if i == 0 && opt.color.is_none() {
            opt.color = Some(ColorRef::Color(fill));
        }
        label(
            p,
            name,
            xy,
            rect,
            Some(&opt),
            diameter * 0.075,
            dim,
            at.opacity,
            ctx,
        );
    }
    draw_value(
        p,
        d,
        value,
        "°",
        rect,
        diameter * 0.66,
        dim,
        at.opacity,
        ctx,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn headings_wrap_and_ranges_saturate() {
        assert_eq!(heading(-1.0), 359.0);
        assert_eq!(heading(721.0), 1.0);
        assert_eq!(fraction(-10.0, 0.0, 100.0), 0.0);
        assert_eq!(fraction(120.0, 0.0, 100.0), 1.0);
        assert_eq!(fraction(0.0, -3.0, 3.0), 0.5);
    }
}
