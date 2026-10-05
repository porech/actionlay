//! Measures complete history-widget rendering without a player or network.
mod common;
use actionlay_layout::catalog::{PRESETS, UPSTREAM_PRESETS};
use actionlay_render::{Renderer, tiny_skia::Pixmap};
fn main() {
    let tel = common::telemetry();
    for preset in PRESETS.iter().chain(
        UPSTREAM_PRESETS
            .iter()
            .filter(|p| p.id == "upstream-example"),
    ) {
        for (w, h) in [(1920, 1440), (3840, 2160)] {
            let mut r = Renderer::new();
            let mut target = Pixmap::new(w, h).unwrap();
            let mut times = Vec::new();
            for i in 0..130 {
                r.render_telemetry_into(
                    &preset.layout(),
                    &tel,
                    40.0 + i as f64 / 30.0,
                    &mut target,
                );
                if i >= 10 {
                    times.push(r.last_stats().total.as_secs_f64() * 1000.0);
                }
            }
            times.sort_by(f64::total_cmp);
            println!(
                "{} {w}x{h}: p50 {:.2}ms p95 {:.2}ms max {:.2}ms",
                preset.id, times[60], times[114], times[119]
            );
        }
    }
}
