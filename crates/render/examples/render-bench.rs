//! Overlay render time of the default layout.
//! Run: cargo run --release -p actionlay-render --example render-bench
use std::time::{Duration, Instant};

use actionlay_layout::default_layout;
use actionlay_render::tiny_skia::Pixmap;
use actionlay_render::{RenderStats, Renderer};
use actionlay_telemetry::metric::Metric;
use actionlay_telemetry::{GpsLock, Snapshot, Value};
use chrono::{TimeZone, Utc};

const FRAMES: usize = 300;

fn snapshot(i: usize) -> Snapshot {
    let f = i as f64;
    let m = |id: &str| Metric::from_id(id).unwrap();
    let utc = Utc.with_ymd_and_hms(2026, 9, 27, 12, 15, 30).unwrap()
        + chrono::TimeDelta::milliseconds(40 * i as i64);
    Snapshot::for_test(
        f * 0.04,
        Some(utc),
        GpsLock::Lock3d,
        &[
            (m("speed"), Value::Present(5.0 + (f * 0.05).sin() * 4.0)),
            (m("alt"), Value::Present(300.0 + f * 0.1)),
            (m("gradient"), Value::Present((f * 0.02).cos() * 8.0)),
            (m("odo"), Value::Present(1000.0 + f * 0.5)),
            (m("lat"), Value::Present(45.464_213 + f * 1e-6)),
            (m("lon"), Value::Present(9.190_123 + f * 1e-6)),
        ],
    )
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn main() {
    let layout = default_layout();
    for (w, h) in [(1920, 1440), (3840, 2160)] {
        let mut renderer = Renderer::new();
        let mut pixmap = Pixmap::new(w, h).unwrap();
        for i in 0..20 {
            renderer.render_into(&layout, &snapshot(i), &mut pixmap); // warm caches
        }
        let mut times = Vec::with_capacity(FRAMES);
        let mut sum = RenderStats::default();
        for i in 0..FRAMES {
            let snap = snapshot(i);
            let start = Instant::now();
            renderer.render_into(&layout, &snap, &mut pixmap);
            times.push(start.elapsed());
            let s = renderer.last_stats();
            sum.clear += s.clear;
            sum.text += s.text;
            sum.icons += s.icons;
            sum.shapes += s.shapes;
        }
        times.sort();
        let mean = times.iter().sum::<Duration>() / FRAMES as u32;
        let n = FRAMES as u32;
        println!(
            "{w}x{h}: mean {:.2} ms, p50 {:.2}, p95 {:.2}, max {:.2} | clear {:.2}, text {:.2}, icons {:.2}, shapes {:.2}",
            ms(mean),
            ms(times[FRAMES / 2]),
            ms(times[FRAMES * 95 / 100]),
            ms(times[FRAMES - 1]),
            ms(sum.clear / n),
            ms(sum.text / n),
            ms(sum.icons / n),
            ms(sum.shapes / n),
        );
    }
}
