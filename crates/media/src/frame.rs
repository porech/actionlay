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

/// Planar 8-bit 4:2:0 (YUV420P / YUVJ420P) to NV12: Y copied, U and V interleaved, values unchanged.
#[allow(clippy::too_many_arguments)]
pub fn yuv420p_to_nv12(
    width: u32,
    height: u32,
    y: &[u8],
    y_stride: usize,
    u: &[u8],
    u_stride: usize,
    v: &[u8],
    v_stride: usize,
) -> (Vec<u8>, Vec<u8>) {
    let (w, h) = (width as usize, height as usize);
    let (cw, ch) = chroma_dims(width, height);
    let mut out_y = Vec::with_capacity(w * h);
    for row in 0..h {
        out_y.extend_from_slice(&y[row * y_stride..row * y_stride + w]);
    }
    let mut out_uv = Vec::with_capacity(cw * 2 * ch);
    for row in 0..ch {
        let ur = &u[row * u_stride..row * u_stride + cw];
        let vr = &v[row * v_stride..row * v_stride + cw];
        for (a, b) in ur.iter().zip(vr) {
            out_uv.push(*a);
            out_uv.push(*b);
        }
    }
    (out_y, out_uv)
}

/// Planar 10-bit 4:2:0 (YUV420P10LE, 10 bits in the low bits of little-endian u16) to 8-bit NV12 by `>> 2`.
#[allow(clippy::too_many_arguments)]
pub fn yuv420p10_to_nv12(
    width: u32,
    height: u32,
    y: &[u8],
    y_stride: usize,
    u: &[u8],
    u_stride: usize,
    v: &[u8],
    v_stride: usize,
) -> (Vec<u8>, Vec<u8>) {
    let (w, h) = (width as usize, height as usize);
    let (cw, ch) = chroma_dims(width, height);
    let px =
        |line: &[u8], i: usize| (u16::from_le_bytes([line[2 * i], line[2 * i + 1]]) >> 2) as u8;
    let mut out_y = Vec::with_capacity(w * h);
    for row in 0..h {
        let line = &y[row * y_stride..row * y_stride + w * 2];
        out_y.extend((0..w).map(|i| px(line, i)));
    }
    let mut out_uv = Vec::with_capacity(cw * 2 * ch);
    for row in 0..ch {
        let ur = &u[row * u_stride..row * u_stride + cw * 2];
        let vr = &v[row * v_stride..row * v_stride + cw * 2];
        for i in 0..cw {
            out_uv.push(px(ur, i));
            out_uv.push(px(vr, i));
        }
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

    #[test]
    fn yuv420p_interleaves_chroma_odd_size() {
        // 3x3 luma (stride 4), chroma 2x2 (stride 3)
        let y = [1, 2, 3, 0, 4, 5, 6, 0, 7, 8, 9, 0];
        let u = [10, 11, 0, 12, 13, 0];
        let v = [20, 21, 0, 22, 23, 0];
        let (py, puv) = yuv420p_to_nv12(3, 3, &y, 4, &u, 3, &v, 3);
        assert_eq!(py, vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(puv, vec![10, 20, 11, 21, 12, 22, 13, 23]);
    }

    #[test]
    fn yuv420p_keeps_full_range_values() {
        let y = [255, 0, 255, 0];
        let (py, puv) = yuv420p_to_nv12(2, 2, &y, 2, &[255], 1, &[0], 1);
        assert_eq!(py, vec![255, 0, 255, 0]);
        assert_eq!(puv, vec![255, 0]);
    }

    #[test]
    fn yuv420p10_shifts_right_by_two_odd_size() {
        // 3x1 luma (stride 8 bytes, 2 pad), chroma 2x1 (stride 6 bytes, 2 pad)
        let le = |v: u16| v.to_le_bytes();
        let mut y = Vec::new();
        for v in [1023u16, 940, 4] {
            y.extend(le(v));
        }
        y.extend([0, 0]);
        let mut u = Vec::new();
        for v in [512u16, 3] {
            u.extend(le(v));
        }
        u.extend([0, 0]);
        let mut vv = Vec::new();
        for v in [64u16, 1023] {
            vv.extend(le(v));
        }
        vv.extend([0, 0]);
        let (py, puv) = yuv420p10_to_nv12(3, 1, &y, 8, &u, 6, &vv, 6);
        assert_eq!(py, vec![255, 235, 1]);
        assert_eq!(puv, vec![128, 16, 0, 255]);
    }
}
