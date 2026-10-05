//! Offline previews of every M3 preset, using generated telemetry only.
mod common;
use actionlay_layout::{
    catalog::{PRESETS, UPSTREAM_PRESETS},
    scale::auto_scale_mode,
};
use actionlay_render::{
    Renderer,
    tiny_skia::{Pixmap, PixmapPaint, Transform},
};
fn main() {
    let dir = "target/m3-previews";
    std::fs::create_dir_all(dir).unwrap();
    let tel = common::telemetry();
    let mut contact = Pixmap::new(1600, 1200).unwrap();
    contact.fill(tiny_skia::Color::from_rgba8(71, 91, 107, 255));
    for (i, preset) in PRESETS.iter().chain(UPSTREAM_PRESETS).enumerate() {
        let layout = preset.layout();
        for (w, h) in [(1280, 720), (960, 720), (720, 1280)] {
            let mut r = Renderer::new();
            r.set_scale_mode(auto_scale_mode(&layout, w, h));
            let mut overlay = Pixmap::new(w, h).unwrap();
            r.render_telemetry_into(&layout, &tel, 42.0, &mut overlay);
            let mut image = Pixmap::new(w, h).unwrap();
            image.fill(tiny_skia::Color::from_rgba8(71, 91, 107, 255));
            image.draw_pixmap(
                0,
                0,
                overlay.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
            image
                .save_png(format!("{dir}/{}-{w}x{h}.png", preset.id))
                .unwrap();
            if w == 1280 {
                contact.draw_pixmap(
                    0,
                    0,
                    image.as_ref(),
                    &PixmapPaint::default(),
                    Transform::from_scale(0.3125, 0.3125)
                        .post_translate((i % 4) as f32 * 400.0, (i / 4) as f32 * 300.0),
                    None,
                );
            }
        }
    }
    contact.save_png(format!("{dir}/contact.png")).unwrap();
    println!("All sixteen layouts previewed at three aspect ratios: {dir}");
}
