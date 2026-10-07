//! Rasterize a repository SVG with the bundled Roboto faces.
use std::{fs, path::PathBuf};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(
        args.len(),
        3,
        "usage: render-asset input.svg output.png|output.bmp"
    );
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
    if PathBuf::from(&args[2])
        .extension()
        .is_some_and(|ext| ext == "bmp")
    {
        // Uncompressed 24-bit BMP keeps installer artwork compatible with Inno 6.
        // BMP rows are bottom-up and padded to a multiple of four bytes.
        let width = pixmap.width();
        let height = pixmap.height();
        let stride = (width * 3 + 3) & !3;
        let mut bmp = Vec::with_capacity((54 + stride * height) as usize);
        bmp.extend_from_slice(b"BM");
        bmp.extend_from_slice(&(54 + stride * height).to_le_bytes());
        bmp.extend_from_slice(&[0; 4]);
        bmp.extend_from_slice(&54u32.to_le_bytes());
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&width.to_le_bytes());
        bmp.extend_from_slice(&height.to_le_bytes());
        bmp.extend_from_slice(&1u16.to_le_bytes());
        bmp.extend_from_slice(&24u16.to_le_bytes());
        bmp.extend_from_slice(&[0; 24]);
        for row in pixmap.pixels().chunks(width as usize).rev() {
            for pixel in row {
                assert_eq!(
                    pixel.alpha(),
                    255,
                    "BMP artwork must have an opaque background"
                );
                bmp.extend_from_slice(&[pixel.blue(), pixel.green(), pixel.red()]);
            }
            bmp.extend(std::iter::repeat_n(0, (stride - width * 3) as usize));
        }
        fs::write(&args[2], bmp).unwrap();
    } else {
        pixmap.save_png(&args[2]).unwrap();
    }
}
