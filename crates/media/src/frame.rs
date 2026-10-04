//! Decoded frames, always normalised to tightly packed 8-bit NV12.

#[derive(Debug, Clone)]
pub struct Nv12Frame {
    pub width: u32,
    pub height: u32,
    pub y: Vec<u8>,
    pub uv: Vec<u8>,
    /// Presentation time in seconds of media time.
    pub pts: f64,
}

fn chroma_dims(width: u32, height: u32) -> (usize, usize) {
    (width.div_ceil(2) as usize, height.div_ceil(2) as usize)
}

pub fn pack_nv12(
    width: u32,
    height: u32,
    y: &[u8],
    y_stride: usize,
    uv: &[u8],
    uv_stride: usize,
) -> (Vec<u8>, Vec<u8>) {
    let (w, h) = (width as usize, height as usize);
    let (cw, ch) = chroma_dims(width, height);
    let mut out_y = Vec::with_capacity(w * h);
    for row in 0..h {
        out_y.extend_from_slice(&y[row * y_stride..row * y_stride + w]);
    }
    let mut out_uv = Vec::with_capacity(cw * 2 * ch);
    for row in 0..ch {
        out_uv.extend_from_slice(&uv[row * uv_stride..row * uv_stride + cw * 2]);
    }
    (out_y, out_uv)
}

/// P010 stores 10-bit samples in the top bits of little-endian u16; the high byte is the 8-bit value.
pub fn p010_to_nv12(
    width: u32,
    height: u32,
    y: &[u8],
    y_stride: usize,
    uv: &[u8],
    uv_stride: usize,
) -> (Vec<u8>, Vec<u8>) {
    let (w, h) = (width as usize, height as usize);
    let (cw, ch) = chroma_dims(width, height);
    let mut out_y = Vec::with_capacity(w * h);
    for row in 0..h {
        let line = &y[row * y_stride..row * y_stride + w * 2];
        out_y.extend(line.iter().skip(1).step_by(2).copied());
    }
    let mut out_uv = Vec::with_capacity(cw * 2 * ch);
    for row in 0..ch {
        let line = &uv[row * uv_stride..row * uv_stride + cw * 4];
        out_uv.extend(line.iter().skip(1).step_by(2).copied());
    }
    (out_y, out_uv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_nv12_drops_stride_padding() {
        // 3x2 image: chroma is 2x1 samples (4 bytes), strides padded
        let y = [1, 2, 3, 0, 4, 5, 6, 0];
        let uv = [7, 8, 9, 10, 0, 0];
        let (py, puv) = pack_nv12(3, 2, &y, 4, &uv, 6);
        assert_eq!(py, vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(puv, vec![7, 8, 9, 10]);
    }

    #[test]
    fn p010_keeps_high_byte() {
        // 2x2 luma, 1x1 chroma pair; values are little-endian u16 with 10 bits in the top bits
        let y = [
            0x00, 0xFF, 0x40, 0x80, 0x00, 0x00, /*pad*/ 0x00, 0x10, 0xC0, 0x20, 0, 0,
        ];
        let uv = [0x00, 0x7F, 0x00, 0x81];
        let (py, puv) = p010_to_nv12(2, 2, &y, 6, &uv, 4);
        assert_eq!(py, vec![0xFF, 0x80, 0x10, 0x20]);
        assert_eq!(puv, vec![0x7F, 0x81]);
    }
}
