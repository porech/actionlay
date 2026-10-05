//! Renderer behaviour checked on pixels (golden images are in tests/golden.rs).
use actionlay_layout::geom::{Rect, place, root_box};
use actionlay_layout::model::{Node, Units, Widget};
use actionlay_layout::{Layout, default_layout};
use actionlay_render::tiny_skia::Pixmap;
use actionlay_render::{Renderer, Zone, diagnose};
use actionlay_telemetry::metric::Metric;
use actionlay_telemetry::{GpsLock, Snapshot, Telemetry, Value};
use chrono::{FixedOffset, TimeZone, Utc};
use serde_json::json;

fn layout(nodes: serde_json::Value) -> Layout {
    Layout::from_json(&json!({"version": 1, "nodes": nodes}).to_string())
        .unwrap()
        .layout
}

fn m(id: &str) -> Metric {
    Metric::from_id(id).unwrap()
}

fn snap(lock: GpsLock, values: &[(&str, Value)]) -> Snapshot {
    let values: Vec<(Metric, Value)> = values.iter().map(|(id, v)| (m(id), *v)).collect();
    let utc = Utc.with_ymd_and_hms(2026, 9, 27, 12, 15, 30).unwrap();
    Snapshot::for_test(12.0, Some(utc), lock, &values)
}

fn renderer() -> Renderer {
    let mut r = Renderer::new();
    r.set_zone(Zone::Fixed(FixedOffset::east_opt(2 * 3600).unwrap()));
    r
}

fn draw(l: &Layout, s: &Snapshot) -> Pixmap {
    renderer().render(l, s, 640, 360)
}

fn max_alpha(p: &Pixmap) -> u8 {
    p.pixels().iter().map(|c| c.alpha()).max().unwrap_or(0)
}

fn rightmost_drawn_column(p: &Pixmap, rows: std::ops::Range<u32>) -> Option<u32> {
    (0..p.width())
        .rev()
        .find(|&x| rows.clone().any(|y| p.pixel(x, y).unwrap().alpha() > 0))
}

fn lowest_drawn_row(p: &Pixmap) -> Option<u32> {
    (0..p.height())
        .rev()
        .find(|&y| (0..p.width()).any(|x| p.pixel(x, y).unwrap().alpha() > 0))
}

/// Upper bound of alpha for dimmed widgets: fill (255 × 0.45), outline (217 × 0.45) and
/// shadow (89 × 0.45) composite to at most ~183; undimmed text reaches 255.
const DIM: u8 = 190;

#[test]
fn empty_snapshot_draws_dashes_and_no_lock() {
    let empty = Telemetry::empty(10.0).sample(1.0);
    let speed = layout(json!([{"type": "metric", "metric": "speed", "size": 120}]));
    let p = draw(&speed, &empty);
    let a = max_alpha(&p);
    assert!(a > 0 && a <= DIM, "dimmed dash expected, max alpha {a}");
    let full = draw(
        &speed,
        &snap(GpsLock::Lock3d, &[("speed", Value::Present(13.4))]),
    );
    assert_eq!(max_alpha(&full), 255);

    let gps = layout(json!([{"type": "gps_lock_icon", "size": 80}]));
    let off = draw(&gps, &empty);
    assert!(max_alpha(&off) > 0 && max_alpha(&off) <= DIM);
    let on = draw(&gps, &snap(GpsLock::Lock3d, &[]));
    // accent amber #ffb300 fully opaque somewhere
    assert!(on.pixels().iter().any(|c| c.alpha() == 255
        && c.red() > 240
        && c.green() > 150
        && c.green() < 200
        && c.blue() < 30));

    // the default layout still draws its panels and dashes
    assert!(max_alpha(&renderer().render(&default_layout(), &empty, 640, 360)) > 0);
}

#[test]
fn hide_applies_only_to_metrics_the_video_never_has() {
    let hidden = layout(json!([{"type": "metric", "metric": "speed", "when_absent": "hide"}]));
    let shown = layout(json!([{"type": "metric", "metric": "speed"}]));
    // the video has no speed at all: hidden
    let empty = Telemetry::empty(10.0).sample(1.0);
    assert_eq!(max_alpha(&draw(&hidden, &empty)), 0);
    // the video has speed, but not yet (before the first sample): empty state, not hidden
    let before_first = snap(GpsLock::NoLock, &[("speed", Value::Absent)]);
    let p = draw(&hidden, &before_first);
    assert!(max_alpha(&p) > 0 && max_alpha(&p) <= DIM);
    assert_eq!(p.data(), draw(&shown, &before_first).data());
    // a long gap: empty state, not hidden
    let gap = snap(
        GpsLock::NoLock,
        &[(
            "speed",
            Value::Stale {
                value: 13.4,
                age: 60.0,
            },
        )],
    );
    assert_eq!(draw(&hidden, &gap).data(), p.data());
    // unknown metric ids are never available
    let unknown_hidden =
        layout(json!([{"type": "metric", "metric": "heartbeat", "when_absent": "hide"}]));
    assert_eq!(max_alpha(&draw(&unknown_hidden, &before_first)), 0);
    let unknown = layout(json!([{"type": "metric", "metric": "heartbeat"}]));
    assert_eq!(draw(&unknown, &before_first).data(), p.data());
    // metric_unit and datetime follow the same rule
    let unit = layout(json!([{"type": "metric_unit", "metric": "speed", "when_absent": "hide"}]));
    assert_eq!(max_alpha(&draw(&unit, &empty)), 0);
    assert!(max_alpha(&draw(&unit, &before_first)) > 0);
    let date = layout(json!([{"type": "datetime", "when_absent": "hide"}]));
    assert_eq!(max_alpha(&draw(&date, &empty)), 0, "no UTC in this video");
    assert!(max_alpha(&draw(&date, &before_first)) > 0);
}

/// Spec §4.4.1 (ruling R6): during a gap the last value is shown dimmed for the grace
/// time (`stale_secs`, default 3 s), then the empty state.
#[test]
fn stale_values_are_dimmed_then_become_the_empty_state() {
    let l = layout(json!([{"type": "metric", "metric": "speed", "size": 120, "stale_secs": 3}]));
    let at = |v: Value| draw(&l, &snap(GpsLock::NoLock, &[("speed", v)]));
    let stale = |age| Value::Stale { value: 13.4, age };
    let present = at(Value::Present(13.4));
    let absent = at(Value::Absent);
    assert_eq!(max_alpha(&present), 255);
    let recent = at(stale(1.0));
    assert!(max_alpha(&recent) <= DIM);
    assert_ne!(
        recent.data(),
        absent.data(),
        "recent stale value is still shown"
    );
    assert_ne!(
        recent.data(),
        present.data(),
        "recent stale value is dimmed"
    );
    // dimmed = the same value at dim_opacity
    let faded =
        layout(json!([{"type": "metric", "metric": "speed", "size": 120, "opacity": 0.45}]));
    let faded = draw(
        &faded,
        &snap(GpsLock::NoLock, &[("speed", Value::Present(13.4))]),
    );
    assert_eq!(recent.data(), faded.data());
    assert_eq!(
        at(stale(3.0)).data(),
        recent.data(),
        "the grace includes its end"
    );
    let old = at(stale(10.0));
    assert_eq!(
        old.data(),
        absent.data(),
        "old stale value shows the empty state"
    );
    assert_eq!(at(stale(3.1)).data(), absent.data());
    // default grace is 3 s
    let d = layout(json!([{"type": "metric", "metric": "speed", "size": 120}]));
    let at_d = |v: Value| draw(&d, &snap(GpsLock::NoLock, &[("speed", v)]));
    assert_eq!(at_d(stale(2.9)).data(), recent.data());
    assert_eq!(at_d(stale(3.1)).data(), absent.data());
    // stale_secs 0: the empty state immediately
    let z = layout(json!([{"type": "metric", "metric": "speed", "size": 120, "stale_secs": 0}]));
    let at_z = |v: Value| draw(&z, &snap(GpsLock::NoLock, &[("speed", v)]));
    assert_eq!(at_z(stale(0.1)).data(), absent.data());
    assert_eq!(at_z(stale(0.0)).data(), absent.data());
    // the unit next to the value dims with it (metric_unit uses the default grace)
    let u = layout(json!([{"type": "metric_unit", "metric": "speed", "size": 120}]));
    let at_u = |v: Value| draw(&u, &snap(GpsLock::NoLock, &[("speed", v)]));
    assert!(max_alpha(&at_u(Value::Present(13.4))) > DIM);
    let unit_recent = at_u(stale(1.0));
    assert!(max_alpha(&unit_recent) <= DIM);
    assert_eq!(
        at_u(stale(5.0)).data(),
        unit_recent.data(),
        "unit stays, dimmed"
    );
}

#[test]
fn metric_text_is_converted_and_formatted() {
    let style = json!({"size": 80, "weight": "bold", "color": "primary"});
    let mut metric = json!({"type": "metric", "metric": "speed", "format": "{value:.0} {unit}"});
    let mut text = json!({"type": "text", "text": "48 km/h"});
    let mut empty_text = json!({"type": "text", "text": "— km/h"});
    for (k, v) in style.as_object().unwrap() {
        metric[k] = v.clone();
        text[k] = v.clone();
        empty_text[k] = v.clone();
    }
    let s = snap(GpsLock::Lock3d, &[("speed", Value::Present(13.4))]); // 48.24 km/h
    assert_eq!(
        draw(&layout(json!([metric.clone()])), &s).data(),
        draw(&layout(json!([text])), &s).data()
    );
    // the empty state keeps the literal parts of the format, dimmed
    let gone = snap(GpsLock::Lock3d, &[]);
    let mut dimmed_text = empty_text;
    dimmed_text["opacity"] = json!(0.45);
    assert_eq!(
        draw(&layout(json!([metric])), &gone).data(),
        draw(&layout(json!([dimmed_text])), &gone).data()
    );
}

/// A metric whose format does not parse (only possible in a layout built in code:
/// loading rejects it) shows its empty state, and `diagnose` reports it.
#[test]
fn bad_formats_render_the_empty_state_and_are_diagnosed() {
    let present = snap(GpsLock::Lock3d, &[("speed", Value::Present(13.4))]);
    let absent = snap(GpsLock::Lock3d, &[("speed", Value::Absent)]);
    let good = layout(json!([{"type": "metric", "metric": "speed", "size": 100}]));
    let mut bad = good.clone();
    let Node::Known(Widget::Metric(metric)) = &mut bad.nodes[0] else {
        unreachable!()
    };
    metric.format = Some("{value:.x}".into());
    let p = draw(&bad, &present);
    assert!(max_alpha(&p) > 0 && max_alpha(&p) <= DIM);
    assert_eq!(p.data(), draw(&good, &absent).data());
    let issues: Vec<String> = diagnose(&bad).iter().map(ToString::to_string).collect();
    assert_eq!(issues.len(), 1, "{issues:#?}");
    assert!(issues[0].contains("{value:.x}"), "{issues:#?}");

    let good_dt = layout(json!([{"type": "datetime", "size": 60}]));
    let mut bad_dt = good_dt.clone();
    let Node::Known(Widget::Datetime(dt)) = &mut bad_dt.nodes[0] else {
        unreachable!()
    };
    dt.format = Some("%H:%Q".into());
    let no_time = Snapshot::for_test(0.0, None, GpsLock::NoLock, &[]);
    let p = draw(&bad_dt, &present);
    assert!(max_alpha(&p) > 0 && max_alpha(&p) <= DIM);
    assert_eq!(p.data(), draw(&good_dt, &no_time).data());
    let issues: Vec<String> = diagnose(&bad_dt).iter().map(ToString::to_string).collect();
    assert_eq!(issues.len(), 1, "{issues:#?}");
    assert!(issues[0].contains("%H:%Q"), "{issues:#?}");
}

#[test]
fn datetime_uses_the_zone_for_local_and_utc_when_asked() {
    let s = snap(GpsLock::Lock3d, &[]);
    let same = |dt: serde_json::Value, txt: &str| {
        let style = json!({"size": 60, "weight": "bold"});
        let mut a = dt;
        let mut b = json!({"type": "text", "text": txt});
        for (k, v) in style.as_object().unwrap() {
            a[k] = v.clone();
            b[k] = v.clone();
        }
        draw(&layout(json!([a])), &s).data() == draw(&layout(json!([b])), &s).data()
    };
    assert!(same(
        json!({"type": "datetime", "format": "%H:%M"}),
        "14:15"
    ));
    assert!(same(
        json!({"type": "datetime", "format": "%H:%M", "timezone": "utc"}),
        "12:15"
    ));
    assert!(!same(
        json!({"type": "datetime", "format": "%H:%M"}),
        "12:15"
    ));
}

#[test]
fn renders_at_degenerate_and_large_sizes() {
    let l = default_layout();
    let s = snap(
        GpsLock::Lock3d,
        &[
            ("speed", Value::Present(13.4)),
            ("alt", Value::Present(312.0)),
        ],
    );
    let mut r = renderer();
    for (w, h) in [(1, 1), (2, 2), (16, 9), (64, 2000), (3840, 2160)] {
        let p = r.render(&l, &s, w, h);
        assert_eq!((p.width(), p.height()), (w, h));
    }
    assert!(max_alpha(&r.render(&l, &s, 3840, 2160)) == 255);
    let p = r.render(&l, &s, 0, 0);
    assert_eq!(
        (p.width(), p.height()),
        (1, 1),
        "zero size is clamped, never a panic"
    );
}

#[test]
fn size_changes_and_reused_targets_give_the_same_pixels() {
    let l = default_layout();
    let s = snap(
        GpsLock::Lock3d,
        &[
            ("speed", Value::Present(13.4)),
            ("alt", Value::Present(312.0)),
        ],
    );
    let fresh = renderer().render(&l, &s, 640, 360);
    let mut r = renderer();
    r.render(&l, &s, 640, 360);
    r.render(&l, &s, 1280, 720);
    let mut target = Pixmap::new(640, 360).unwrap();
    // stale content of the target is cleared
    target.fill(actionlay_render::tiny_skia::Color::WHITE);
    r.render_into(&l, &s, &mut target);
    assert_eq!(target.data(), fresh.data());
    let stats = r.last_stats();
    assert!(stats.total > std::time::Duration::ZERO);
    assert!(stats.total >= stats.text + stats.icons + stats.shapes + stats.clear);
}

#[test]
fn output_is_premultiplied() {
    let s = snap(GpsLock::Lock3d, &[("speed", Value::Present(13.4))]);
    let p = renderer().render(&default_layout(), &s, 960, 540);
    assert!(
        p.pixels()
            .iter()
            .all(|c| c.red() <= c.alpha() && c.green() <= c.alpha() && c.blue() <= c.alpha())
    );
}

#[test]
fn anchored_frames_follow_the_video_edges_on_16_9_and_4_3() {
    let s = snap(GpsLock::Lock3d, &[]);
    for (w, h) in [(1920, 1080), (1440, 1080), (3840, 2160)] {
        let p = renderer().render(&default_layout(), &s, w, h);
        let scale = h as f32 / 1080.0;
        // bottom-right frame: right edge 24 units from the right, at the bottom
        let x =
            rightmost_drawn_column(&p, (h - (60.0 * scale) as u32)..(h - (40.0 * scale) as u32))
                .unwrap();
        let expected = w as f32 - 24.0 * scale;
        assert!(
            (x as f32 - expected).abs() <= 2.0 * scale,
            "{w}x{h}: edge at {x}, expected {expected}"
        );
    }
}

/// Glyph ink overflows the line box (~0.11 em below, ~0.21 em above) and the outline
/// and shadow add a few units: inside the default 24-unit margin nothing is cut.
#[test]
fn descenders_near_the_bottom_edge_are_not_cut() {
    let mut mph = layout(
        json!([{"type": "metric_unit", "metric": "speed", "anchor": "bottom-right",
        "offset": [-24, -24], "size": 60}]),
    );
    mph.units = Some(Units::Imperial);
    let parens = layout(
        json!([{"type": "text", "text": "(gjpq)", "anchor": "bottom-left",
        "offset": [24, -24], "size": 100}]),
    );
    let s = snap(GpsLock::Lock3d, &[("speed", Value::Present(13.4))]);
    for l in [&mph, &parens] {
        for (w, h) in [(1920, 1080), (1280, 720), (3840, 2160)] {
            let p = renderer().render(l, &s, w, h);
            let box_bottom = h as f32 - 24.0 * h as f32 / 1080.0;
            let lowest = lowest_drawn_row(&p).unwrap();
            assert!(
                lowest as f32 > box_bottom,
                "{w}x{h}: descenders are drawn below the line box ({lowest} vs {box_bottom})"
            );
            assert!(lowest + 2 < h, "{w}x{h}: ink reaches the edge ({lowest})");
        }
    }
}

/// With descender-heavy values (a month with a "p", negative coordinates, imperial
/// units) every pixel of the default layout stays inside its four panels.
#[test]
fn default_layout_ink_stays_inside_its_panels() {
    let mut l = default_layout();
    let utc = Utc.with_ymd_and_hms(2026, 9, 27, 12, 15, 30).unwrap();
    let values: Vec<(Metric, Value)> = [
        ("speed", 88.8),
        ("alt", -1234.5),
        ("gradient", -18.8),
        ("odo", 98765.4),
        ("lat", -33.888_88),
        ("lon", -151.888_88),
    ]
    .iter()
    .map(|&(id, v)| (m(id), Value::Present(v)))
    .collect();
    let s = Snapshot::for_test(12.0, Some(utc), GpsLock::Lock3d, &values);
    for units in [Units::Metric, Units::Imperial] {
        l.units = Some(units);
        for (w, h) in [(1920, 1080), (1440, 1080), (1280, 720)] {
            let p = renderer().render(&l, &s, w, h);
            let scale = h as f32 / 1080.0;
            let root = root_box(w as f32, h as f32, scale);
            let panels: Vec<Rect> = l
                .nodes
                .iter()
                .filter_map(|n| match n {
                    Node::Known(Widget::Frame(f)) => {
                        let c = &f.common;
                        let r = place(
                            root,
                            c.anchor.unwrap_or_default(),
                            c.offset.unwrap_or_default(),
                            f.size,
                        );
                        Some(Rect::new(
                            r.x * scale,
                            r.y * scale,
                            r.w * scale,
                            r.h * scale,
                        ))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(panels.len(), 4);
            for y in 0..h {
                for x in 0..w {
                    if p.pixel(x, y).unwrap().alpha() == 0 {
                        continue;
                    }
                    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                    assert!(
                        panels.iter().any(|r| fx > r.x - 1.0
                            && fx < r.x + r.w + 1.0
                            && fy > r.y - 1.0
                            && fy < r.y + r.h + 1.0),
                        "{units:?} {w}x{h}: ink outside the panels at ({x}, {y})"
                    );
                }
            }
        }
    }
}

#[test]
fn theme_change_restyles_widgets_without_overrides() {
    let nodes = json!([{"type": "text", "text": "TEST", "size": 100, "outline": {"width": 0}, "shadow": {"offset": [0, 0]}}]);
    let mut l = layout(nodes);
    let s = snap(GpsLock::Lock3d, &[]);
    let white = draw(&l, &s);
    assert!(
        white
            .pixels()
            .iter()
            .any(|c| c.alpha() == 255 && c.red() == 255 && c.green() == 255)
    );
    l.theme = Some(serde_json::from_value(json!({"palette": {"primary": "#ff0000"}})).unwrap());
    let red = draw(&l, &s);
    assert!(
        red.pixels()
            .iter()
            .any(|c| c.alpha() == 255 && c.red() == 255 && c.green() == 0)
    );
    assert!(!red.pixels().iter().any(|c| c.green() > 0));
}

#[test]
fn opacity_multiplies_down_the_tree() {
    let s = snap(GpsLock::Lock3d, &[]);
    let text = json!({"type": "text", "text": "TEST", "size": 100, "outline": {"width": 0},
        "shadow": {"offset": [0, 0]}});
    let nested = layout(json!([{"type": "group", "opacity": 0.5, "children": [
        {"type": "group", "opacity": 0.5, "children": [text.clone()]}]}]));
    let a = max_alpha(&draw(&nested, &s));
    assert!((60..=68).contains(&a), "0.25 × 255 expected, got {a}");
    let mut flat = text;
    flat["opacity"] = json!(0.25);
    assert_eq!(
        draw(&nested, &s).data(),
        draw(&layout(json!([flat])), &s).data()
    );
}

#[test]
fn unknown_node_types_and_invisible_nodes_are_not_drawn() {
    let l = layout(json!([
        {"type": "moving_map", "size": 300},
        {"type": "text", "text": "hidden", "visible": false},
        {"type": "text", "text": "transparent", "opacity": 0},
        {"type": "frame", "size": [100, 100], "visible": false,
            "children": [{"type": "text", "text": "child"}]},
        {"type": "icon", "icon": "rocket"}
    ]));
    assert_eq!(max_alpha(&draw(&l, &snap(GpsLock::Lock3d, &[]))), 0);
}

#[test]
fn missing_glyphs_are_counted() {
    let mut r = renderer();
    let s = snap(GpsLock::Lock3d, &[]);
    r.render(&layout(json!([{"type": "text", "text": "AB"}])), &s, 64, 36);
    assert_eq!(r.missing_glyphs(), 0);
    r.render(
        &layout(json!([{"type": "text", "text": "A東"}])),
        &s,
        64,
        36,
    );
    assert_eq!(r.missing_glyphs(), 1);
}

#[test]
fn diagnose_reports_unknown_names() {
    assert!(
        diagnose(&default_layout()).is_empty(),
        "{:?}",
        diagnose(&default_layout())
    );
    let l = layout(json!([
        {"type": "metric", "id": "a", "metric": "heartbeat"},
        {"type": "metric", "id": "b", "metric": "speed", "units": "furlong"},
        {"type": "metric_unit", "id": "c", "metric": "speed", "units": "ft"},
        {"type": "icon", "id": "d", "icon": "rocket"},
        {"type": "frame", "size": [10, 10], "children": [
            {"type": "text", "id": "e", "text": "x", "font": "Comic Sans"}
        ]}
    ]));
    let issues: Vec<String> = diagnose(&l).iter().map(ToString::to_string).collect();
    assert_eq!(issues.len(), 5, "{issues:#?}");
    for needle in ["heartbeat", "furlong", "`ft`", "rocket", "Comic Sans"] {
        assert!(
            issues.iter().any(|i| i.contains(needle)),
            "{needle}: {issues:#?}"
        );
    }
    assert!(
        issues.iter().any(|i| i.contains("children[0] (e)")),
        "{issues:#?}"
    );
}

#[test]
fn gps_lock_icon_picks_the_icon_and_colour_by_fix() {
    let gps = layout(json!([{"type": "gps_lock_icon", "size": 80}]));
    let icon = |name: &str, color: &str, opacity: f64| {
        let l = layout(
            json!([{"type": "icon", "icon": name, "color": color, "size": 80,
            "opacity": opacity}]),
        );
        draw(&l, &snap(GpsLock::Lock3d, &[]))
    };
    let at = |lock| draw(&gps, &snap(lock, &[]));
    assert_eq!(
        at(GpsLock::Lock3d).data(),
        icon("gps", "accent", 1.0).data()
    );
    assert_eq!(
        at(GpsLock::Lock2d).data(),
        icon("gps", "primary", 1.0).data()
    );
    // no fix: the crossed-out satellite, dimmed (not a dimmed `gps`)
    let off = icon("gps-off", "primary", 0.45);
    assert_ne!(off.data(), icon("gps", "primary", 0.45).data());
    assert_eq!(at(GpsLock::NoLock).data(), off.data());
    assert_eq!(at(GpsLock::Unknown).data(), off.data());
}

#[test]
fn renderer_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<Renderer>();
}

fn bar_pixels(mut node: serde_json::Value, value: Value) -> Pixmap {
    node["metric"] = json!("alt");
    node["offset"] = json!([20, 20]);
    node["show_value"] = json!(false);
    node["track"] = json!("#00000000");
    node["radius"] = json!(12);
    renderer().render(
        &layout(json!([node])),
        &snap(GpsLock::Lock3d, &[("alt", value)]),
        200,
        1080,
    )
}

#[test]
fn bars_clip_fill_to_corners_and_clamp_after_unit_conversion() {
    let b = json!({"type":"bar","size":[100,40],"fill":"#ff0000","max":100});
    let half = bar_pixels(b.clone(), Value::Present(50.0));
    assert_eq!(half.pixel(40, 40).unwrap().red(), 255);
    assert_eq!(half.pixel(100, 40).unwrap().alpha(), 0);
    assert_eq!(half.pixel(20, 20).unwrap().alpha(), 0);
    let full = bar_pixels(b.clone(), Value::Present(150.0));
    assert_eq!(full.pixel(110, 40).unwrap().red(), 255);
    assert_eq!(full.pixel(119, 20).unwrap().alpha(), 0);
    assert_eq!(max_alpha(&bar_pixels(b.clone(), Value::Present(-1.0))), 0);
    let feet = bar_pixels(
        json!({"type":"bar","size":[100,40],"fill":"#ff0000","units":"ft","max":100}),
        Value::Present(15.24),
    );
    assert_eq!(feet.data(), half.data());
    let stale = bar_pixels(
        b.clone(),
        Value::Stale {
            value: 50.0,
            age: 1.0,
        },
    );
    assert!((110..=116).contains(&stale.pixel(40, 40).unwrap().alpha()));
    assert_eq!(
        max_alpha(&bar_pixels(
            b,
            Value::Stale {
                value: 50.0,
                age: 4.0
            }
        )),
        0
    );
}

#[test]
fn negative_bars_grow_from_zero_and_reverse_and_vertical_directions_work() {
    let brake = json!({"type":"bar","size":[100,40],"fill":"#ff0000","min":-3,"max":0});
    assert_eq!(
        max_alpha(&bar_pixels(brake.clone(), Value::Present(0.0))),
        0
    );
    let half = bar_pixels(brake, Value::Present(-1.5));
    assert_eq!(half.pixel(30, 40).unwrap().alpha(), 0);
    assert_eq!(half.pixel(110, 40).unwrap().red(), 255);
    let reverse = bar_pixels(
        json!({"type":"bar","size":[100,40],"direction":"right_to_left","fill":"#ff0000"}),
        Value::Present(50.0),
    );
    assert_eq!(reverse.data(), half.data());
    for (direction, filled, empty) in [
        ("bottom_to_top", (40, 100), (40, 30)),
        ("top_to_bottom", (40, 30), (40, 100)),
    ] {
        let image = bar_pixels(
            json!({"type":"bar","size":[40,100],"direction":direction,"fill":"#ff0000"}),
            Value::Present(50.0),
        );
        assert_eq!(image.pixel(filled.0, filled.1).unwrap().red(), 255);
        assert_eq!(image.pixel(empty.0, empty.1).unwrap().alpha(), 0);
    }
}

#[test]
fn zone_bars_keep_threshold_colours_and_missing_data_policy() {
    let node = json!({"type":"zone_bar","size":[100,40],"zones":[{"up_to":50,"color":"#00ff00"},{"up_to":100,"color":"#ff0000"}]});
    let image = bar_pixels(node, Value::Present(80.0));
    assert_eq!(image.pixel(40, 40).unwrap().green(), 255);
    assert_eq!(image.pixel(90, 40).unwrap().red(), 255);
    assert_eq!(image.pixel(110, 40).unwrap().alpha(), 0);
    let hidden = layout(json!([{"type":"zone_bar","metric":"hr","when_absent":"hide"}]));
    assert_eq!(
        max_alpha(&draw(&hidden, &Telemetry::empty(10.0).sample(1.0))),
        0
    );
    let present_gap = snap(GpsLock::NoLock, &[("hr", Value::Absent)]);
    assert!(max_alpha(&draw(&hidden, &present_gap)) > 0);
}

#[test]
fn presets_render_proportionally_when_the_video_is_resized() {
    for preset in actionlay_layout::catalog::PRESETS {
        assert!(diagnose(&preset.layout()).is_empty(), "{}", preset.id);
        let l = layout(
            json!([{"type":"bar","metric":"alt","max":100,"size":[300,60],"anchor":"bottom-right","offset_relative":[-0.05,-0.05],"show_value":false,"fill":"#ff0000","track":"#00000000","radius":0}]),
        );
        let snap = snap(GpsLock::Lock3d, &[("alt", Value::Present(100.0))]);
        let small = renderer().render(&l, &snap, 640, 360);
        let large = renderer().render(&l, &snap, 1280, 720);
        // right/bottom inset is 5% of video dimensions; bar grows 2x with the video.
        assert_eq!(small.pixel(607, 341).unwrap().red(), 255);
        assert_eq!(large.pixel(1215, 683).unwrap().red(), 255);
        assert_eq!(small.pixel(609, 341).unwrap().alpha(), 0);
        assert_eq!(large.pixel(1217, 683).unwrap().alpha(), 0);
        assert_eq!(small.pixel(507, 331).unwrap().alpha(), 0);
        assert_eq!(large.pixel(1015, 663).unwrap().alpha(), 0);
    }
}
