//! Builders for hand-made GPMF payloads used by the unit tests.

/// One KLV item with a data payload, padded to 4 bytes.
pub fn item(key: &[u8; 4], type_char: u8, struct_size: u8, repeat: u16, data: &[u8]) -> Vec<u8> {
    assert_eq!(data.len(), usize::from(struct_size) * usize::from(repeat));
    let mut out = key.to_vec();
    out.push(type_char);
    out.push(struct_size);
    out.extend(repeat.to_be_bytes());
    out.extend(data);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    out
}

/// A nested item (type 0) containing `children`.
pub fn nested(key: &[u8; 4], children: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = children.concat();
    let mut out = key.to_vec();
    out.push(0);
    out.push(1);
    out.extend(u16::try_from(body.len()).unwrap().to_be_bytes());
    out.extend(body);
    out
}

pub fn i32s(values: &[i32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

pub fn i16s(values: &[i16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

/// A GPS5 point: lat, lon (deg), alt (m), 2D and 3D speed (m/s).
pub type Gps5 = [f64; 5];

const GPS5_SCAL: [i32; 5] = [10_000_000, 10_000_000, 1000, 1000, 100];

/// A `STRM` with GPSF, GPSU, GPSP and GPS5, as HERO5–10 write it.
pub fn gps5_stream(gpsu: &str, fix: u32, dop_x100: u16, points: &[Gps5]) -> Vec<u8> {
    let mut raw = Vec::new();
    for p in points {
        for (v, s) in p.iter().zip(GPS5_SCAL) {
            raw.push((v * f64::from(s)).round() as i32);
        }
    }
    nested(
        b"STRM",
        &[
            item(b"GPSF", b'L', 4, 1, &fix.to_be_bytes()),
            item(b"GPSU", b'U', 16, 1, gpsu.as_bytes()),
            item(b"GPSP", b'S', 2, 1, &dop_x100.to_be_bytes()),
            item(b"SCAL", b'l', 4, 5, &i32s(&GPS5_SCAL)),
            item(b"GPS5", b'l', 20, points.len() as u16, &i32s(&raw)),
        ],
    )
}

/// An accelerometer `STRM`: raw i16 samples divided by `scal` give m/s².
pub fn accl_stream(orin: Option<&str>, scal: i16, tmpc: f32, samples: &[[i16; 3]]) -> Vec<u8> {
    let mut children = vec![
        item(b"SIUN", b'c', 4, 1, b"m/s\xb2"),
        item(b"SCAL", b's', 2, 1, &scal.to_be_bytes()),
        item(b"TMPC", b'f', 4, 1, &tmpc.to_be_bytes()),
    ];
    if let Some(o) = orin {
        children.push(item(b"ORIN", b'c', 1, 3, o.as_bytes()));
    }
    let flat: Vec<i16> = samples.iter().flatten().copied().collect();
    children.push(item(b"ACCL", b's', 6, samples.len() as u16, &i16s(&flat)));
    nested(b"STRM", &children)
}

/// A `DEVC` holding `streams`: one demuxed packet.
pub fn devc(streams: &[Vec<u8>]) -> Vec<u8> {
    nested(b"DEVC", streams)
}
