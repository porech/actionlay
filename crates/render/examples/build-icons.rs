//! Regenerate PNG icon sizes from the editable SVG masters, using the app's renderer.
use std::{fs, path::PathBuf};
fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons");
    for name in ["actionlay", "dmg"] {
        let svg = fs::read(root.join(format!("{name}.svg"))).unwrap();
        let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default()).unwrap();
        for size in [16, 24, 32, 48, 64, 128, 256, 512, 1024] {
            let mut pixmap = tiny_skia::Pixmap::new(size, size).unwrap();
            let scale = size as f32 / tree.size().width();
            resvg::render(
                &tree,
                tiny_skia::Transform::from_scale(scale, scale),
                &mut pixmap.as_mut(),
            );
            pixmap
                .save_png(root.join(format!("{name}-{size}.png")))
                .unwrap();
        }
    }
}
