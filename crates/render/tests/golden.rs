//! Golden images of the renderer. Regenerate with
//! `ACTIONLAY_UPDATE_GOLDENS=1 cargo test -p actionlay-render --test golden`
//! and review the PNGs before committing. Failures write the actual image and a
//! diff (differing pixels in red) to target/golden-diff/.
use std::path::{Path, PathBuf};

use actionlay_layout::geom::ScaleMode;
use actionlay_layout::scale::auto_scale_mode;
use actionlay_layout::{Layout, default_layout};
use actionlay_render::tiny_skia::Pixmap;
use actionlay_render::{Renderer, Zone};
use actionlay_telemetry::metric::Metric;
use actionlay_telemetry::{GpsLock, Snapshot, Telemetry, Value};
use chrono::{FixedOffset, TimeZone, Utc};

/// A channel difference above this counts as a differing pixel. Goldens are made on
/// macOS arm64 and checked on x86_64 too: tiny-skia's NEON and SSE/AVX pipelines and the
/// platform libm (SVG arcs) can move an antialiased edge by one coverage step (~16 levels).
const CHANNEL_TOLERANCE: u8 = 24;
/// Differing pixels allowed: max(16, pixels / 10000).
fn allowed_bad_pixels(total: usize) -> usize {
    (total / 10_000).max(16)
}

fn snapshot(values: &[(&str, Value)], lock: GpsLock) -> Snapshot {
    let values: Vec<(Metric, Value)> = values
        .iter()
        .map(|(id, v)| (Metric::from_id(id).unwrap(), *v))
        .collect();
    let utc = Utc.with_ymd_and_hms(2026, 9, 27, 12, 15, 30).unwrap();
    Snapshot::for_test(754.2, Some(utc), lock, &values)
}

fn full() -> Snapshot {
    snapshot(
        &[
            ("speed", Value::Present(13.4)),   // 48 km/h
            ("alt", Value::Present(312.4)),    // 312 m
            ("gradient", Value::Present(5.2)), // +5.2 % (percent)
            ("odo", Value::Present(12_345.6)), // 12.35 km
            ("lat", Value::Present(45.464_213)),
            ("lon", Value::Present(9.190_123)),
            ("temp", Value::Present(21.5)),
        ],
        GpsLock::Lock3d,
    )
}

fn render(layout: &Layout, snap: &Snapshot, w: u32, h: u32) -> Pixmap {
    let mut r = Renderer::new();
    r.set_zone(Zone::Fixed(FixedOffset::east_opt(2 * 3600).unwrap()));
    r.render(layout, snap, w, h)
}

fn diff_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/golden-diff")
}

fn check(name: &str, rendered: &Pixmap) {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/golden/{name}.png"));
    let actual = Pixmap::decode_png(&rendered.encode_png().unwrap()).unwrap();
    if std::env::var("ACTIONLAY_UPDATE_GOLDENS").is_ok_and(|v| v == "1") {
        std::fs::create_dir_all(golden.parent().unwrap()).unwrap();
        actual.save_png(&golden).unwrap();
        return;
    }
    let expected = Pixmap::load_png(&golden).unwrap_or_else(|e| {
        panic!(
            "{}: {e}; generate with ACTIONLAY_UPDATE_GOLDENS=1",
            golden.display()
        )
    });
    if (actual.width(), actual.height()) != (expected.width(), expected.height()) {
        let dir = diff_dir();
        std::fs::create_dir_all(&dir).unwrap();
        actual
            .save_png(dir.join(format!("{name}.actual.png")))
            .unwrap();
        panic!(
            "{name}: size {}x{} but golden is {}x{}; see {}",
            actual.width(),
            actual.height(),
            expected.width(),
            expected.height(),
            dir.display()
        );
    }
    let mut diff = Pixmap::new(actual.width(), actual.height()).unwrap();
    let mut bad = 0usize;
    let mut worst = 0u8;
    for ((a, e), d) in actual
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(expected.data().as_chunks::<4>().0.iter())
        .zip(diff.data_mut().as_chunks_mut::<4>().0.iter_mut())
    {
        let delta = a.iter().zip(e).map(|(x, y)| x.abs_diff(*y)).max().unwrap();
        worst = worst.max(delta);
        if delta > CHANNEL_TOLERANCE {
            bad += 1;
            d.copy_from_slice(&[255, 0, 0, 255]);
        }
    }
    let allowed = allowed_bad_pixels(actual.width() as usize * actual.height() as usize);
    eprintln!(
        "{name}: worst channel delta {worst}, {bad} pixels over {CHANNEL_TOLERANCE} (allowed {allowed})"
    );
    if bad > allowed {
        let dir = diff_dir();
        std::fs::create_dir_all(&dir).unwrap();
        actual
            .save_png(dir.join(format!("{name}.actual.png")))
            .unwrap();
        diff.save_png(dir.join(format!("{name}.diff.png"))).unwrap();
        panic!(
            "{name}: {bad} pixels differ by more than {CHANNEL_TOLERANCE} (allowed {allowed}, worst {worst}); see {}",
            dir.display()
        );
    }
}

/// Exact probes (premultiplied RGBA, +-2 per channel) in flat regions that are stable across
/// platforms. The image tolerance above would let a small global colour or alpha shift
/// through; these fail when a theme constant changes.
fn probe(p: &Pixmap, x: u32, y: u32, expected: [u8; 4]) {
    let c = p.pixel(x, y).unwrap();
    let got = [c.red(), c.green(), c.blue(), c.alpha()];
    assert!(
        got.iter().zip(&expected).all(|(a, b)| a.abs_diff(*b) <= 2),
        "pixel ({x},{y}): got {got:?}, expected {expected:?} +-2"
    );
}

fn max_alpha(p: &Pixmap, xs: std::ops::Range<u32>, ys: std::ops::Range<u32>) -> u8 {
    ys.flat_map(|y| xs.clone().map(move |x| (x, y)))
        .map(|(x, y)| p.pixel(x, y).unwrap().alpha())
        .max()
        .unwrap()
}

/// Panel fill of the default layout (inside each of the four panels, clear of text).
fn probe_panels(p: &Pixmap) {
    for (x, y) in [(145, 40), (170, 500), (800, 55), (930, 520)] {
        probe(p, x, y, [6, 8, 11, 140]);
    }
}

#[test]
fn default_full_16x9() {
    let p = render(&default_layout(), &full(), 960, 540);
    probe_panels(&p);
    // undimmed digits of the speed value reach full alpha
    assert_eq!(max_alpha(&p, 20..95, 465..525), 255);
    check(
        "default_full_16x9",
        &render(&default_layout(), &full(), 960, 540),
    );
}

#[test]
fn default_full_4x3() {
    // Rendered with the mode the app picks for 4:3 footage: the default layout fits a
    // 4:3 frame at the Height scale, so the widgets keep their 16:9 size (the renderer's
    // default mode is Height).
    assert_eq!(
        auto_scale_mode(&default_layout(), 720, 540),
        ScaleMode::Height
    );
    assert_eq!(
        auto_scale_mode(&default_layout(), 1920, 1440),
        ScaleMode::Height
    );
    check(
        "default_full_4x3",
        &render(&default_layout(), &full(), 720, 540),
    );
}

/// The shipped defaults over a bright (sky-blue) video frame: legibility check.
#[test]
fn default_on_bright_background() {
    let overlay = render(&default_layout(), &full(), 480, 270);
    let mut frame = Pixmap::new(480, 270).unwrap();
    frame.fill(actionlay_render::tiny_skia::Color::from_rgba8(
        235, 245, 255, 255,
    ));
    frame.draw_pixmap(
        0,
        0,
        overlay.as_ref(),
        &actionlay_render::tiny_skia::PixmapPaint::default(),
        actionlay_render::tiny_skia::Transform::identity(),
        None,
    );
    check("default_bright_bg", &frame);
}

#[test]
fn default_no_telemetry() {
    let empty = Telemetry::empty(60.0).sample(12.0);
    check(
        "default_no_telemetry",
        &render(&default_layout(), &empty, 960, 540),
    );
}

#[test]
fn default_stale() {
    let snap = snapshot(
        &[
            (
                "speed",
                Value::Stale {
                    value: 13.4,
                    age: 1.0,
                },
            ), // dimmed value
            (
                "alt",
                Value::Stale {
                    value: 312.4,
                    age: 5.0,
                },
            ), // past 3 s: empty state
            ("gradient", Value::Present(-3.1)),
            (
                "lat",
                Value::Stale {
                    value: 45.464_213,
                    age: 0.5,
                },
            ),
            (
                "lon",
                Value::Stale {
                    value: 9.190_123,
                    age: 0.5,
                },
            ),
        ],
        GpsLock::NoLock,
    );
    let p = render(&default_layout(), &snap, 960, 540);
    probe_panels(&p);
    // dimmed speed digits: fill at dim_opacity composited over the panel
    let a = max_alpha(&p, 20..95, 465..525);
    assert!(
        a.abs_diff(222) <= 2,
        "dimmed speed max alpha {a}, expected 222 +-2"
    );
    check("default_stale", &p);
}

#[test]
fn widgets() {
    let mut nodes = vec![serde_json::json!({
        "type": "frame", "anchor": "center", "size": [1500, 760], "radius": 40,
        "border": {"width": 6}, "opacity": 0.9,
        "children": [
            {"type": "text", "text": "Outline + shadow", "anchor": "top", "offset": [0, 40],
             "size": 90, "weight": "bold", "outline": {"width": 5, "color": "#c00000"},
             "shadow": {"offset": [6, 6]}},
            {"type": "text", "text": "Plain regular", "anchor": "top", "offset": [0, 160],
             "size": 60, "outline": {"width": 0}, "shadow": {"offset": [0, 0]}},
            {"type": "gps_lock_icon", "anchor": "bottom-left", "offset": [40, -40], "size": 90},
            {"type": "metric", "metric": "temp", "format": "{value:.1}{unit}", "anchor": "bottom-right",
             "offset": [-40, -40], "size": 90},
            {"type": "metric", "metric": "hr", "anchor": "bottom", "offset": [0, -40], "size": 90}
        ]
    })];
    let icons = [
        "speed",
        "altitude",
        "gradient",
        "distance",
        "temperature",
        "gps",
        "gps-off",
        "clock",
        "location",
    ];
    let row: Vec<serde_json::Value> = icons
        .iter()
        .enumerate()
        .map(|(i, name)| {
            serde_json::json!({"type": "icon", "icon": name, "anchor": "center",
                               "offset": [-480.0 + 120.0 * i as f64, 0], "size": 80})
        })
        .collect();
    nodes.push(serde_json::json!({"type": "group", "anchor": "center", "offset": [0, 20], "children": row}));
    let text = serde_json::json!({
        "version": 1,
        "theme": {"palette": {"accent": "#00c8ff"}},
        "nodes": nodes
    })
    .to_string();
    let layout = Layout::from_json(&text).unwrap().layout;
    check("widgets", &render(&layout, &full(), 640, 360));
}

#[test]
fn rendering_is_deterministic_in_process() {
    let a = render(&default_layout(), &full(), 480, 270);
    let b = render(&default_layout(), &full(), 480, 270);
    assert_eq!(a.data(), b.data());
}
