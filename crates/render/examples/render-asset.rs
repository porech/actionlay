//! Rasterize a repository SVG with the bundled Roboto faces.
use std::{fs, path::PathBuf};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 3, "usage: render-asset input.svg output.png");
    let mut options = resvg::usvg::Options::default();
    let fonts = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/fonts");
    for name in ["Roboto-Regular.ttf", "Roboto-Bold.ttf", "Roboto-Medium.ttf"] {
        options
            .fontdb_mut()
            .load_font_data(fs::read(fonts.join(name)).unwrap());
    }
    let tree = resvg::usvg::Tree::from_data(&fs::read(&args[1]).unwrap(), &options).unwrap();
    let size = tree.size().to_int_size();
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height()).unwrap();
    resvg::render(
        &tree,
        tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    pixmap.save_png(&args[2]).unwrap();
}
