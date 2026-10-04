//! YUV colour description and the YUV→RGB matrix used by the video shader.
use ffmpeg_next::{color, format::Pixel};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Matrix {
    Bt601,
    Bt709,
    Bt2020,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    Limited,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorInfo {
    pub matrix: Matrix,
    pub range: Range,
}

impl ColorInfo {
    /// Untagged streams: BT.709 from 720 lines up, BT.601 below; `yuvj*` formats are full range.
    pub fn from_ffmpeg(
        space: color::Space,
        range: color::Range,
        pixel: Pixel,
        height: u32,
    ) -> Self {
        let matrix = match space {
            color::Space::BT709 => Matrix::Bt709,
            color::Space::BT2020NCL | color::Space::BT2020CL => Matrix::Bt2020,
            color::Space::BT470BG | color::Space::SMPTE170M => Matrix::Bt601,
            _ if height >= 720 => Matrix::Bt709,
            _ => Matrix::Bt601,
        };
        let range = match range {
            color::Range::JPEG => Range::Full,
            color::Range::MPEG => Range::Limited,
            _ if matches!(pixel, Pixel::YUVJ420P | Pixel::YUVJ422P | Pixel::YUVJ444P) => {
                Range::Full
            }
            _ => Range::Limited,
        };
        Self { matrix, range }
    }
}

/// Row-major 3x4 matrix: `rgb = M · [y, u, v, 1]`, with y/u/v normalised to 0..1
/// as sampled from 8-bit textures.
pub fn yuv_to_rgb(c: ColorInfo) -> [[f32; 4]; 3] {
    let (kr, kb) = match c.matrix {
        Matrix::Bt601 => (0.299_f32, 0.114_f32),
        Matrix::Bt709 => (0.2126, 0.0722),
        Matrix::Bt2020 => (0.2627, 0.0593),
    };
    let kg = 1.0 - kr - kb;
    let (y_scale, y_offset, c_scale) = match c.range {
        Range::Limited => (255.0 / 219.0, 16.0 / 255.0, 255.0 / 224.0),
        Range::Full => (1.0, 0.0, 1.0),
    };
    let c_offset = 128.0 / 255.0;

    let r_v = 2.0 * (1.0 - kr) * c_scale;
    let g_u = -2.0 * kb * (1.0 - kb) / kg * c_scale;
    let g_v = -2.0 * kr * (1.0 - kr) / kg * c_scale;
    let b_u = 2.0 * (1.0 - kb) * c_scale;
    let y0 = -y_scale * y_offset;

    [
        [y_scale, 0.0, r_v, y0 - r_v * c_offset],
        [y_scale, g_u, g_v, y0 - (g_u + g_v) * c_offset],
        [y_scale, b_u, 0.0, y0 - b_u * c_offset],
    ]
}

#[cfg(test)]
mod tests {
    use ffmpeg_next::{color, format::Pixel};

    use super::*;

    fn apply(m: [[f32; 4]; 3], y: u8, u: u8, v: u8) -> [f32; 3] {
        let (y, u, v) = (y as f32 / 255.0, u as f32 / 255.0, v as f32 / 255.0);
        let mut out = [0.0; 3];
        for (i, row) in m.iter().enumerate() {
            out[i] = row[0] * y + row[1] * u + row[2] * v + row[3];
        }
        out
    }

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.02)
    }

    #[test]
    fn limited_bt709_black_white_red() {
        let m = yuv_to_rgb(ColorInfo {
            matrix: Matrix::Bt709,
            range: Range::Limited,
        });
        assert!(close(apply(m, 16, 128, 128), [0.0, 0.0, 0.0]));
        assert!(close(apply(m, 235, 128, 128), [1.0, 1.0, 1.0]));
        assert!(close(apply(m, 63, 102, 240), [1.0, 0.0, 0.0]));
    }

    #[test]
    fn full_range_uses_whole_scale() {
        let m = yuv_to_rgb(ColorInfo {
            matrix: Matrix::Bt709,
            range: Range::Full,
        });
        assert!(close(apply(m, 0, 128, 128), [0.0, 0.0, 0.0]));
        assert!(close(apply(m, 255, 128, 128), [1.0, 1.0, 1.0]));
    }

    #[test]
    fn color_info_gopro_yuvj_is_full_range_bt709() {
        let c = ColorInfo::from_ffmpeg(
            color::Space::Unspecified,
            color::Range::Unspecified,
            Pixel::YUVJ420P,
            1440,
        );
        assert_eq!(
            c,
            ColorInfo {
                matrix: Matrix::Bt709,
                range: Range::Full
            }
        );
    }

    #[test]
    fn color_info_defaults_by_height_and_tags() {
        let sd = ColorInfo::from_ffmpeg(
            color::Space::Unspecified,
            color::Range::Unspecified,
            Pixel::YUV420P,
            480,
        );
        assert_eq!(
            sd,
            ColorInfo {
                matrix: Matrix::Bt601,
                range: Range::Limited
            }
        );
        let uhd = ColorInfo::from_ffmpeg(
            color::Space::BT2020NCL,
            color::Range::MPEG,
            Pixel::YUV420P10LE,
            2160,
        );
        assert_eq!(
            uhd,
            ColorInfo {
                matrix: Matrix::Bt2020,
                range: Range::Limited
            }
        );
        let jpeg = ColorInfo::from_ffmpeg(
            color::Space::BT709,
            color::Range::JPEG,
            Pixel::YUV420P,
            1080,
        );
        assert_eq!(jpeg.range, Range::Full);
    }
}
