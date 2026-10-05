//! Render the bundled presets over a neutral background, at landscape and portrait
//! sizes. Run: cargo run -p actionlay-render --example preview-presets
use actionlay_layout::{catalog::PRESETS, scale::auto_scale_mode};
use actionlay_render::{
    Renderer, Zone,
    tiny_skia::{Pixmap, PixmapPaint, Transform},
};
use actionlay_telemetry::{GpsLock, Snapshot, Value, metric::Metric};
use chrono::{FixedOffset, TimeZone, Utc};

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/preset-previews".into());
    std::fs::create_dir_all(&dir).unwrap();
    let values = [
        ("speed", 27.3),
        ("alt", 438.0),
        ("gradient", 4.6),
        ("odo", 12540.0),
        ("lat", 45.0),
        ("lon", 9.0),
        ("accel", -1.5),
        ("hr", 168.0),
        ("power", 238.0),
        ("cadence", 87.0),
        ("cog", 65.0),
    ]
    .map(|(m, v)| (Metric::from_id(m).unwrap(), Value::Present(v)));
    let snapshot = Snapshot::for_test(
        42.0,
        Some(Utc.with_ymd_and_hms(2026, 10, 5, 10, 30, 0).unwrap()),
        GpsLock::Lock3d,
        &values,
    );
    for preset in PRESETS {
        let layout = preset.layout();
        for (w, h) in [(1280, 720), (960, 720), (720, 1280)] {
            let mut renderer = Renderer::new();
            renderer.set_zone(Zone::Fixed(FixedOffset::east_opt(7200).unwrap()));
            renderer.set_scale_mode(auto_scale_mode(&layout, w, h));
            let overlay = renderer.render(&layout, &snapshot, w, h);
            let mut preview = Pixmap::new(w, h).unwrap();
            preview.fill(actionlay_render::tiny_skia::Color::from_rgba8(
                71, 91, 107, 255,
            ));
            preview.draw_pixmap(
                0,
                0,
                overlay.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
            preview
                .save_png(format!("{dir}/{}-{w}x{h}.png", preset.id))
                .unwrap();
        }
    }
    println!("Previews saved in {dir}");
}
