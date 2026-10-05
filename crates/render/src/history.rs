//! Charts, maps and G-meter: all rendering is a function of telemetry time, never UI history.
use crate::scene::{Ctx, Painter, Placement, draw_text};
use crate::shapes::{paint, rounded_rect};
use crate::value;
use actionlay_layout::color::Color;
use actionlay_layout::geom::{Anchor, Rect, place};
use actionlay_layout::model::{ChartNode, GMeterNode, HeadingFilter, MapMode, MapNode, WhenAbsent};
use actionlay_layout::style::{TextKind, TextStyleOpt};
use actionlay_telemetry::{Metric, Telemetry, Value};
use std::collections::HashMap;
use tiny_skia::{FillRule, Mask, PathBuilder, Pixmap, PixmapPaint, Stroke, Transform};

type FilterKey = (u64, Metric, [u64; 4]);
#[derive(Default)]
pub(crate) struct HeadingCache {
    curves: HashMap<FilterKey, Vec<(f64, f64)>>,
}
impl HeadingCache {
    pub fn sample(
        &mut self,
        tel: &Telemetry,
        m: Metric,
        t: f64,
        opt: &HeadingFilter,
    ) -> Option<f64> {
        if opt.enabled == Some(false) {
            return tel.sample_metric(m, t).present();
        }
        let values = [
            opt.seconds.unwrap_or(0.5),
            opt.deadband.unwrap_or(1.5),
            opt.max_rate.unwrap_or(120.0),
            opt.min_speed.unwrap_or(1.5),
        ];
        let key = (tel.identity(), m, values.map(f64::to_bits));
        if !self.curves.contains_key(&key) {
            if self.curves.len() >= 16 {
                self.curves.clear();
            }
            let mut curve = Vec::new();
            let mut last: Option<(f64, f64)> = None;
            for (x, v) in tel.metric_points(m) {
                let Some(raw) = v else {
                    last = None;
                    continue;
                };
                let angle = if let Some((old_t, old)) = last.filter(|(old_t, _)| x - old_t <= 2.0) {
                    let dt = x - old_t;
                    let stopped = matches!(m, Metric::Heading | Metric::Cog)
                        && matches!(tel.sample_metric(Metric::Speed,x),Value::Present(speed) if speed<values[3]);
                    let diff = (raw - old + 180.0).rem_euclid(360.0) - 180.0;
                    let delta = if stopped || diff.abs() <= values[1] {
                        0.0
                    } else {
                        diff * (if values[0] > 0.0 {
                            1.0 - (-dt / values[0]).exp()
                        } else {
                            1.0
                        })
                    };
                    let delta = if values[2] > 0.0 {
                        delta.clamp(-values[2] * dt, values[2] * dt)
                    } else {
                        delta
                    };
                    (old + delta).rem_euclid(360.0)
                } else {
                    raw.rem_euclid(360.0)
                };
                last = Some((x, angle));
                curve.push((x, angle));
            }
            self.curves.insert(key, curve);
        }
        let curve = &self.curves[&key];
        let i = curve.partition_point(|p| p.0 <= t);
        let &(x, a) = curve.get(i.checked_sub(1)?)?;
        if let Some(&(y, b)) = curve.get(i).filter(|p| p.0 - x <= 2.0 && p.0 > x) {
            Some(
                (a + ((b - a + 180.0).rem_euclid(360.0) - 180.0)
                    * ((t - x) / (y - x)).clamp(0.0, 1.0))
                .rem_euclid(360.0),
            )
        } else {
            Some(a)
        }
    }
}

/// Bounded raster cache for static map layers and instrument grids. Pixel size,
/// theme, telemetry identity and tile revisions are part of the key.
#[derive(Default)]
pub(crate) struct StaticCache {
    layers: HashMap<String, Pixmap>,
    bytes: usize,
}
impl StaticCache {
    pub(crate) fn get(&self, key: &str) -> Option<Pixmap> {
        self.layers.get(key).cloned()
    }
    pub(crate) fn insert(&mut self, key: String, pixmap: &Pixmap) {
        let bytes = pixmap.data().len();
        if bytes > 32 * 1024 * 1024 {
            return;
        }
        if self.bytes + bytes > 32 * 1024 * 1024 || self.layers.len() >= 512 {
            self.layers.clear();
            self.bytes = 0;
        }
        self.bytes += bytes;
        self.layers.insert(key, pixmap.clone());
    }
}

fn held(value: Value, grace: Option<f64>) -> Option<f64> {
    match value {
        Value::Present(v) if v.is_finite() => Some(v),
        Value::Stale { value, age }
            if value.is_finite() && age <= grace.unwrap_or(3.0) && grace.unwrap_or(3.0) > 0.0 =>
        {
            Some(value)
        }
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)] // Shared text placement helper.
fn label(
    p: &mut Painter,
    text: &str,
    rect: Rect,
    anchor: Anchor,
    offset: [f32; 2],
    opacity: f32,
    style: Option<&TextStyleOpt>,
    ctx: &Ctx,
) {
    p.scratch.clear();
    p.scratch.push_str(text);
    let default = TextStyleOpt::default();
    let opt = style.unwrap_or(&default);
    let mut style = opt.resolve(TextKind::Text, ctx.theme);
    if opt.size.is_none() {
        style.size = 18.0;
    }
    draw_text(
        p,
        &style,
        false,
        Placement {
            parent: rect,
            anchor,
            offset,
            opacity,
        },
        ctx,
    );
}
fn compose(p: &mut Painter, mut surface: Pixmap, rect: Rect, radius: f32, opacity: f32, ctx: &Ctx) {
    if radius > 0.0
        && let Some(path) = rounded_rect(
            0.0,
            0.0,
            surface.width() as f32,
            surface.height() as f32,
            radius * ctx.scale,
        )
    {
        let mut mask = Mask::new(surface.width(), surface.height()).unwrap();
        mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
        surface.apply_mask(&mask);
    }
    p.pixmap.draw_pixmap(
        (rect.x * ctx.scale).round() as i32,
        (rect.y * ctx.scale).round() as i32,
        surface.as_ref(),
        &PixmapPaint {
            opacity,
            ..Default::default()
        },
        Transform::identity(),
        None,
    );
}
fn surface(size: [f32; 2], s: f32) -> Option<Pixmap> {
    Pixmap::new(
        (size[0] * s).round().clamp(1.0, 4096.0) as u32,
        (size[1] * s).round().clamp(1.0, 4096.0) as u32,
    )
}
fn stroke(pm: &mut Pixmap, points: &[[f32; 2]], width: f32, color: Color, alpha: f32) {
    if points.len() < 2 {
        return;
    }
    let mut b = PathBuilder::new();
    b.move_to(points[0][0], points[0][1]);
    for xy in &points[1..] {
        b.line_to(xy[0], xy[1]);
    }
    if let Some(path) = b.finish() {
        pm.stroke_path(
            &path,
            &paint(color, alpha),
            &Stroke {
                width,
                ..Default::default()
            },
            Transform::identity(),
            None,
        );
    }
}

pub(crate) fn draw_chart(p: &mut Painter, c: &ChartNode, gradient: bool, at: Placement, ctx: &Ctx) {
    let resolved = value::resolve(&c.metric, c.units.as_deref(), ctx.system);
    let tel = ctx.telemetry;
    let available = resolved.is_some_and(|r| ctx.snap.is_available(r.metric));
    if !available && c.when_absent == Some(WhenAbsent::Hide) {
        return;
    }
    let size = c.size();
    let rect = place(at.parent, at.anchor, at.offset, size);
    let s = ctx.scale;
    let Some(mut pm) = surface(size, s) else {
        return;
    };
    let bg = c.background.map_or(ctx.theme.panel, |v| ctx.theme.color(v));
    pm.fill(tiny_skia::Color::from_rgba8(bg.r, bg.g, bg.b, bg.a));
    let t = ctx.snap.t;
    let secs = c.seconds.unwrap_or(60.0);
    let (start, end) = if c.journey == Some(true) {
        (0.0, tel.map_or(t.max(1.0), Telemetry::duration).max(1.0))
    } else {
        ((t - secs).max(0.0), t.max(secs))
    };
    let count = c.samples.unwrap_or(256).clamp(2, 2048);
    let points: Vec<_> = (0..count)
        .map(|i| {
            let time = start + (end - start) * i as f64 / (count - 1) as f64;
            let v =
                tel.zip(resolved)
                    .and_then(|(tel, r)| match tel.sample_metric(r.metric, time) {
                        Value::Present(v) => Some(r.display(v)),
                        _ => None,
                    });
            (time, v.filter(|v| v.is_finite()))
        })
        .collect();
    let observed_min = points
        .iter()
        .filter_map(|p| p.1)
        .fold(f64::INFINITY, f64::min);
    let observed_max = points
        .iter()
        .filter_map(|p| p.1)
        .fold(f64::NEG_INFINITY, f64::max);
    let span = (observed_max - observed_min).max(1.0);
    let mut min = c.min.unwrap_or(observed_min - span * 0.05);
    let mut max = c.max.unwrap_or(observed_max + span * 0.05);
    if !(max > min && min.is_finite() && max.is_finite() && (max - min).is_finite()) {
        min = c.min.unwrap_or(0.0);
        max = c.max.unwrap_or(min + 1.0);
    }
    let w = pm.width() as f32;
    let h = pm.height() as f32;
    let pad = 12.0 * s;
    let plot = [
        pad,
        30.0 * s,
        (w - 2.0 * pad).max(1.0),
        (h - 44.0 * s).max(1.0),
    ];
    let xy = |(time, v): (f64, f64)| {
        [
            plot[0] + ((time - start) / (end - start)) as f32 * plot[2],
            plot[1] + (1.0 - ((v - min) / (max - min)).clamp(0.0, 1.0) as f32) * plot[3],
        ]
    };
    for i in 0..=2 {
        let y = plot[1] + plot[3] * i as f32 / 2.0;
        stroke(
            &mut pm,
            &[[plot[0], y], [plot[0] + plot[2], y]],
            s,
            ctx.theme.secondary,
            0.16,
        );
    }
    let line = c.stroke.map_or(ctx.theme.accent, |v| ctx.theme.color(v));
    let fill = c
        .fill
        .map_or(Color { a: 55, ..line }, |v| ctx.theme.color(v));
    let connected = |a: f64, b: f64| {
        tel.zip(resolved).is_some_and(|(tel, r)| {
            !tel.availability()
                .gaps(r.metric)
                .iter()
                .any(|(x, y)| *x < b && *y > a)
                && tel.is_loaded_at(a)
                && tel.is_loaded_at(b)
        })
    };
    let mut run = Vec::new();
    let mut run_color = line;
    let flush = |pm: &mut Pixmap, run: &mut Vec<[f32; 2]>, color: Color| {
        if run.len() < 2 {
            run.clear();
            return;
        }
        let bottom = plot[1] + plot[3];
        let mut path = PathBuilder::new();
        path.move_to(run[0][0], bottom);
        for q in run.iter() {
            path.line_to(q[0], q[1]);
        }
        path.line_to(run.last().unwrap()[0], bottom);
        path.close();
        if let Some(path) = path.finish() {
            pm.fill_path(
                &path,
                &paint(
                    if gradient {
                        Color { a: 70, ..color }
                    } else {
                        fill
                    },
                    1.0,
                ),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        stroke(pm, run, c.stroke_width.unwrap_or(2.0) * s, color, 1.0);
        run.clear();
    };
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (Some(v), Some(z)) = (a.1, b.1) else {
            flush(&mut pm, &mut run, run_color);
            continue;
        };
        if !connected(a.0, b.0) {
            flush(&mut pm, &mut run, run_color);
            continue;
        }
        let (a, b) = (xy((a.0, v)), xy((b.0, z)));
        let color = if gradient {
            let slope = tel
                .and_then(|t| t.sample_metric(Metric::Gradient, pair[1].0).last_known())
                .unwrap_or(0.0);
            if slope < 0.0 {
                c.negative
                    .map_or(Color::rgba(100, 200, 255, 255), |v| ctx.theme.color(v))
            } else {
                c.positive.map_or(ctx.theme.accent, |v| ctx.theme.color(v))
            }
        } else {
            line
        };
        if color != run_color {
            flush(&mut pm, &mut run, run_color);
        }
        run_color = color;
        if run.is_empty() {
            run.push(a);
        }
        run.push(b);
    }
    flush(&mut pm, &mut run, run_color);
    let marker = xy((t.clamp(start, end), min))[0];
    stroke(
        &mut pm,
        &[[marker, plot[1]], [marker, plot[1] + plot[3]]],
        s,
        ctx.theme.primary,
        0.45,
    );
    let any = points.iter().any(|p| p.1.is_some());
    compose(p, pm, rect, c.radius.unwrap_or(12.0), at.opacity, ctx);
    if !any {
        label(
            p,
            "No data",
            rect,
            Anchor::Center,
            [0.0, 0.0],
            at.opacity * ctx.theme.dim_opacity,
            None,
            ctx,
        );
    }
    if c.show_value != Some(false) {
        let v =
            resolved.and_then(|r| held(ctx.snap.get(r.metric), c.stale_secs).map(|v| r.display(v)));
        let text = v.map_or_else(
            || "—".into(),
            |v| format!("{v:.1} {}", resolved.map_or("", |r| r.symbol)),
        );
        label(
            p,
            &text,
            rect,
            Anchor::TopRight,
            [-12.0, 6.0],
            at.opacity,
            c.value_style.as_ref(),
            ctx,
        );
    }
}

pub(crate) fn draw_map(p: &mut Painter, m: &MapNode, at: Placement, ctx: &Ctx) {
    let settings = ctx.maps.settings();
    let size = m.size();
    let rect = place(at.parent, at.anchor, at.offset, size);
    let s = ctx.scale;
    let pos = held(ctx.snap.get(Metric::Lat), m.stale_secs)
        .zip(held(ctx.snap.get(Metric::Lon), m.stale_secs));
    let mut at = at;
    if matches!(ctx.snap.get(Metric::Lat), Value::Stale { .. })
        || matches!(ctx.snap.get(Metric::Lon), Value::Stale { .. })
    {
        at.opacity *= ctx.theme.dim_opacity;
    }
    let masked =
        pos.is_some_and(|(lat, lon)| settings.privacy.iter().any(|z| z.contains(lat, lon)));
    if pos.is_none()
        && (!ctx.snap.is_available(Metric::Lat) || !ctx.snap.is_available(Metric::Lon))
        && m.when_absent == Some(WhenAbsent::Hide)
    {
        return;
    }
    let Some(mut pm) = surface(size, s) else {
        return;
    };
    let bg = m.background.map_or(ctx.theme.panel, |v| ctx.theme.color(v));
    pm.fill(tiny_skia::Color::from_rgba8(bg.r, bg.g, bg.b, bg.a));
    let mode = m.mode.unwrap_or_default();
    let track = ctx.telemetry.map_or(&[][..], Telemetry::track);
    let visible: Vec<_> = track
        .iter()
        .filter(|q| !settings.privacy.iter().any(|z| z.contains(q.lat, q.lon)))
        .map(|q| actionlay_maps::project(q.lat, q.lon))
        .collect();
    let overview = matches!(mode, MapMode::Journey | MapMode::Circuit);
    let mut zoom = m.zoom.unwrap_or(15);
    let mut center = pos.map(|(a, b)| actionlay_maps::project(a, b));
    if overview && !visible.is_empty() {
        let reference = visible[0][0];
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for q in &visible {
            let q = [
                reference + actionlay_maps::wrap_delta(q[0] - reference),
                q[1],
            ];
            for k in 0..2 {
                min[k] = min[k].min(q[k]);
                max[k] = max[k].max(q[k]);
            }
        }
        center = Some([(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0]);
        zoom = 19;
        while zoom > 0
            && ((max[0] - min[0]) * 256.0 * 2_f64.powi(zoom as i32) > size[0] as f64 * 0.78
                || (max[1] - min[1]) * 256.0 * 2_f64.powi(zoom as i32) > size[1] as f64 * 0.7)
        {
            zoom -= 1;
        }
    }
    if let Some(center) = center.filter(|_| !masked && (pos.is_some() || overview)) {
        let world = 256.0 * 2_f64.powi(zoom as i32);
        let ww = pm.width() as f32;
        let hh = pm.height() as f32;
        // Do not repaint a moving map until its centre changes by a pixel.
        let center = [
            (center[0] * world * s as f64).round() / (world * s as f64),
            (center[1] * world * s as f64).round() / (world * s as f64),
        ];
        let cache_key = format!(
            "map:{m:?}:{center:?}:{zoom}:{s}:{:?}:{}:{}:{settings:?}",
            ctx.theme,
            ctx.telemetry.map_or(0, Telemetry::identity),
            ctx.maps.revision()
        );
        let cached = p.statics.get(&cache_key);
        let mut cacheable = true;
        let xy = |q: [f64; 2]| {
            [
                (actionlay_maps::wrap_delta(q[0] - center[0]) * world) as f32 * s + ww / 2.0,
                ((q[1] - center[1]) * world) as f32 * s + hh / 2.0,
            ]
        };
        if let Some(cached) = cached {
            pm = cached;
        } else {
            if mode != MapMode::Circuit {
                let left = center[0] * world - size[0] as f64 / 2.0;
                let top = center[1] * world - size[1] as f64 / 2.0;
                let x0 = (left / 256.0).floor() as i64;
                let y0 = (top / 256.0).floor() as i64;
                for y in y0..=y0 + (size[1] / 256.0).ceil().min(12.0) as i64 {
                    for x in x0..=x0 + (size[0] / 256.0).ceil().min(12.0) as i64 {
                        if let Some(key) = actionlay_maps::Tile::at(zoom, x, y) {
                            let Some(tile) = ctx.maps.get(key) else {
                                cacheable &= !settings.online;
                                continue;
                            };
                            let tr = Transform::from_scale(s, s).post_translate(
                                ((x as f64 * 256.0 - left) * s as f64) as f32,
                                ((y as f64 * 256.0 - top) * s as f64) as f32,
                            );
                            pm.draw_pixmap(
                                0,
                                0,
                                tile.as_ref().as_ref(),
                                &PixmapPaint {
                                    opacity: m.opacity_tiles.unwrap_or(0.85),
                                    ..Default::default()
                                },
                                tr,
                                None,
                            );
                        }
                    }
                }
            }
            if mode != MapMode::Moving {
                let route = m.route.map_or(ctx.theme.accent, |v| ctx.theme.color(v));
                let mut path = PathBuilder::new();
                for points in track.windows(2) {
                    let (a, b) = (points[0], points[1]);
                    if b.t - a.t > 2.0
                        || settings
                            .privacy
                            .iter()
                            .any(|z| z.crosses((a.lat, a.lon), (b.lat, b.lon)))
                    {
                        continue;
                    }
                    let a = xy(actionlay_maps::project(a.lat, a.lon));
                    let b = xy(actionlay_maps::project(b.lat, b.lon));
                    if (a[0] - b[0]).abs() > ww * 4.0 || (a[1] - b[1]).abs() > hh * 4.0 {
                        continue;
                    }
                    if (a[0] < 0.0 && b[0] < 0.0)
                        || (a[0] > ww && b[0] > ww)
                        || (a[1] < 0.0 && b[1] < 0.0)
                        || (a[1] > hh && b[1] > hh)
                    {
                        continue;
                    }
                    path.move_to(a[0], a[1]);
                    path.line_to(b[0], b[1]);
                }
                if let Some(path) = path.finish() {
                    pm.stroke_path(
                        &path,
                        &paint(route, 1.0),
                        &Stroke {
                            width: m.route_width.unwrap_or(3.0) * s,
                            ..Default::default()
                        },
                        Transform::identity(),
                        None,
                    );
                }
            }
            if cacheable {
                p.statics.insert(cache_key, &pm);
            }
        }
        if let Some((lat, lon)) = pos {
            let q = xy(actionlay_maps::project(lat, lon));
            if let Some(path) = PathBuilder::from_circle(q[0], q[1], 5.0 * s) {
                pm.fill_path(
                    &path,
                    &paint(
                        m.marker.map_or(ctx.theme.primary, |v| ctx.theme.color(v)),
                        1.0,
                    ),
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
        }
    }
    compose(p, pm, rect, m.radius.unwrap_or(14.0), at.opacity, ctx);
    if masked || (pos.is_none() && (!overview || visible.is_empty())) {
        label(
            p,
            if masked { "Privacy zone" } else { "No GPS" },
            rect,
            Anchor::Center,
            [0.0, 0.0],
            at.opacity,
            m.label_style.as_ref(),
            ctx,
        );
    }
    if mode != MapMode::Circuit {
        let mut style = m.label_style.clone().unwrap_or_default();
        style.size.get_or_insert(13.0);
        label(
            p,
            &settings.attribution,
            rect,
            Anchor::BottomRight,
            [-8.0, -6.0],
            at.opacity,
            Some(&style),
            ctx,
        );
    }
}

pub(crate) fn draw_g_meter(p: &mut Painter, g: &GMeterNode, at: Placement, ctx: &Ctx) {
    let x = held(ctx.snap.get(Metric::AccelLat), g.stale_secs);
    let y = held(ctx.snap.get(Metric::AccelLon), g.stale_secs);
    let mut at = at;
    if matches!(ctx.snap.get(Metric::AccelLat), Value::Stale { .. })
        || matches!(ctx.snap.get(Metric::AccelLon), Value::Stale { .. })
    {
        at.opacity *= ctx.theme.dim_opacity;
    }
    if x.is_none()
        && y.is_none()
        && (!ctx.snap.is_available(Metric::AccelLon) || !ctx.snap.is_available(Metric::AccelLat))
        && g.when_absent == Some(WhenAbsent::Hide)
    {
        return;
    }
    let d = g.diameter();
    let rect = place(at.parent, at.anchor, at.offset, [d; 2]);
    let s = ctx.scale;
    let Some(mut pm) = surface([d; 2], s) else {
        return;
    };
    let center = d * s / 2.0;
    let r = d * s * 0.40;
    let resolved = value::resolve("accel.lon", g.units.as_deref().or(Some("g")), ctx.system);
    let scale = |v: f64| resolved.map_or(v, |r| r.display(v));
    let limit = g.range.unwrap_or(1.5);
    let color = g.fill.map_or(ctx.theme.accent, |v| ctx.theme.color(v));
    let track = g.track.map_or(ctx.theme.secondary, |v| ctx.theme.color(v));
    let coord = |x: f64, y: f64| {
        let angle = g.rotation.unwrap_or(0.0).to_radians() as f64;
        let (a, b) = (
            x * angle.cos() - y * angle.sin(),
            x * angle.sin() + y * angle.cos(),
        );
        let length = a.hypot(b);
        let clamp = if length > limit { limit / length } else { 1.0 };
        [
            center + (a * clamp / limit) as f32 * r,
            center - (b * clamp / limit) as f32 * r,
        ]
    };
    let grid_key = format!("g-grid:{d}:{s}:{:?}:{track:?}", g.rings);
    if let Some(cached) = p.statics.get(&grid_key) {
        pm = cached;
    } else {
        for i in 1..=g.rings.unwrap_or(3).clamp(1, 12) {
            let radius = r * i as f32 / g.rings.unwrap_or(3).clamp(1, 12) as f32;
            if let Some(path) = PathBuilder::from_circle(center, center, radius) {
                pm.stroke_path(
                    &path,
                    &paint(track, 0.25),
                    &Stroke {
                        width: s,
                        ..Default::default()
                    },
                    Transform::identity(),
                    None,
                );
            }
        }
        stroke(
            &mut pm,
            &[[center - r, center], [center + r, center]],
            s,
            track,
            0.25,
        );
        stroke(
            &mut pm,
            &[[center, center - r], [center, center + r]],
            s,
            track,
            0.25,
        );
        p.statics.insert(grid_key, &pm);
    }
    if let Some(tel) = ctx.telemetry {
        let secs = g.trail_secs.unwrap_or(2.0);
        let n = 80;
        let mut previous = None;
        for i in 0..=n {
            let t = ctx.snap.t - secs + secs * i as f64 / n as f64;
            let xy = tel
                .sample_metric(Metric::AccelLat, t)
                .present()
                .zip(tel.sample_metric(Metric::AccelLon, t).present());
            if let Some((x, y)) = xy {
                let q = coord(scale(x), scale(y));
                if let Some(old) = previous {
                    stroke(
                        &mut pm,
                        &[old, q],
                        2.0 * s,
                        color,
                        i as f32 / n as f32 * 0.6,
                    );
                }
                previous = Some(q);
            } else {
                previous = None;
            }
        }
        if g.show_peaks == Some(true) {
            let mut peaks = [0.0_f64; 2];
            for (t, v) in tel
                .metric_points(Metric::AccelLon)
                .take_while(|p| p.0 <= ctx.snap.t)
            {
                if let Some(v) = v {
                    let l = tel
                        .sample_metric(Metric::AccelLat, t)
                        .last_known()
                        .unwrap_or(0.0);
                    if v.abs() > peaks[1].abs() {
                        peaks[1] = v;
                    }
                    if l.abs() > peaks[0].abs() {
                        peaks[0] = l;
                    }
                }
            }
            for (x, y) in [(peaks[0], 0.0), (0.0, peaks[1])] {
                let q = coord(scale(x), scale(y));
                if let Some(path) = PathBuilder::from_circle(q[0], q[1], 3.0 * s) {
                    pm.fill_path(
                        &path,
                        &paint(color, 0.4),
                        FillRule::Winding,
                        Transform::identity(),
                        None,
                    );
                }
            }
        }
    }
    if let Some((x, y)) = x.zip(y) {
        let q = coord(scale(x), scale(y));
        if let Some(path) = PathBuilder::from_circle(q[0], q[1], 5.0 * s) {
            pm.fill_path(
                &path,
                &paint(color, 1.0),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
    compose(p, pm, rect, 0.0, at.opacity, ctx);
    let text = x.zip(y).map_or_else(
        || "—".into(),
        |(x, y)| {
            format!(
                "{:.2} {}",
                scale(x.hypot(y)),
                resolved.map_or("", |r| r.symbol)
            )
        },
    );
    label(
        p,
        &text,
        rect,
        Anchor::Bottom,
        [0.0, -4.0],
        at.opacity,
        g.value_style.as_ref(),
        ctx,
    );
    let source = if ctx
        .telemetry
        .and_then(Telemetry::imu_calibrated_at)
        .is_some_and(|t| ctx.snap.t >= t)
    {
        if ctx.snap.is_available(Metric::GravX) {
            "IMU"
        } else {
            "IMU · estimated gravity"
        }
    } else {
        "GPS estimate"
    };
    label(
        p,
        source,
        rect,
        Anchor::Top,
        [0.0, 2.0],
        at.opacity * 0.7,
        None,
        ctx,
    );
}

#[cfg(test)]
mod heading_tests {
    use super::*;

    fn telemetry(headings: Vec<(f64, f64)>, speed: f64) -> Telemetry {
        Telemetry::for_test(
            4.0,
            &[
                (Metric::Heading, headings),
                (Metric::Speed, vec![(0.0, speed), (3.0, speed)]),
            ],
        )
    }

    #[test]
    fn disabling_filter_preserves_raw_heading_and_keeps_its_thresholds() {
        let tel = telemetry(vec![(0.0, 0.0), (1.0, 90.0)], 10.0);
        let mut cache = HeadingCache::default();
        let mut filter = HeadingFilter {
            seconds: Some(2.0),
            ..Default::default()
        };
        let smoothed = cache.sample(&tel, Metric::Heading, 1.0, &filter).unwrap();
        assert!(smoothed > 0.0 && smoothed < 90.0);
        filter.enabled = Some(false);
        assert_eq!(
            cache.sample(&tel, Metric::Heading, 1.0, &filter),
            Some(90.0)
        );
        filter.enabled = Some(true);
        assert_eq!(
            cache.sample(&tel, Metric::Heading, 1.0, &filter),
            Some(smoothed)
        );
    }

    #[test]
    fn smoothing_crosses_north_by_the_short_path_and_is_seek_deterministic() {
        let tel = telemetry(vec![(0.0, 359.0), (1.0, 1.0), (2.0, 30.0)], 10.0);
        let mut cache = HeadingCache::default();
        let filter = HeadingFilter::default();
        let at_one = cache.sample(&tel, Metric::Heading, 1.0, &filter).unwrap();
        assert!(at_one < 2.0 || at_one > 358.0);
        cache.sample(&tel, Metric::Heading, 2.0, &filter);
        cache.sample(&tel, Metric::Heading, 0.0, &filter);
        assert_eq!(
            cache.sample(&tel, Metric::Heading, 1.0, &filter),
            Some(at_one)
        );
    }

    #[test]
    fn configurable_deadband_rate_limit_and_low_speed_hold_are_respected() {
        let tel = telemetry(vec![(0.0, 0.0), (1.0, 2.0), (2.0, 90.0)], 10.0);
        let filter = HeadingFilter {
            seconds: Some(0.0),
            deadband: Some(3.0),
            max_rate: Some(20.0),
            ..Default::default()
        };
        let mut cache = HeadingCache::default();
        assert_eq!(cache.sample(&tel, Metric::Heading, 1.0, &filter), Some(0.0));
        assert_eq!(
            cache.sample(&tel, Metric::Heading, 2.0, &filter),
            Some(20.0)
        );
        let stopped = telemetry(vec![(0.0, 40.0), (1.0, 90.0)], 0.5);
        assert_eq!(
            cache.sample(&stopped, Metric::Heading, 1.0, &filter),
            Some(40.0)
        );
    }
}
