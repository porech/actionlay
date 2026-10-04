//! GoPro Metadata Format (GPMF) KLV parser.
//!
//! Every item has an 8-byte header: a 4-byte key (FourCC), a 1-byte type, a
//! 1-byte structure size and a 2-byte big-endian repeat count. The payload is
//! `struct_size × repeat` bytes, padded to a multiple of 4. Type 0 means the
//! payload is itself a list of items (`DEVC`, `STRM`).
//! Reference: <https://github.com/gopro/gpmf-parser> (Apache-2.0).
use std::fmt;

use chrono::{DateTime, NaiveDateTime, Utc};

/// Deepest nesting accepted (real files use 2: DEVC → STRM).
const MAX_DEPTH: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FourCc(pub [u8; 4]);

impl FourCc {
    pub const fn new(key: &[u8; 4]) -> FourCc {
        FourCc(*key)
    }
}

impl fmt::Display for FourCc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 {
            let c = if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                '?'
            };
            write!(f, "{c}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for FourCc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FourCc({self})")
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum GpmfError {
    #[error("{key}: item at offset {offset} needs {need} bytes, only {have} left")]
    Truncated {
        key: FourCc,
        offset: usize,
        need: usize,
        have: usize,
    },
    #[error("{key}: nesting deeper than {MAX_DEPTH} levels")]
    TooDeep { key: FourCc },
    #[error("{key}: unknown type '{}'", *type_char as char)]
    UnknownType { key: FourCc, type_char: u8 },
    #[error("{key}: structure size {struct_size} does not fit type '{}'", *type_char as char)]
    BadStructSize {
        key: FourCc,
        type_char: u8,
        struct_size: u8,
    },
    #[error("{key}: item is not numeric")]
    NotNumeric { key: FourCc },
    #[error("{key}: complex item needs a TYPE")]
    MissingType { key: FourCc },
    #[error("{key}: SCAL contains a zero or non-finite value")]
    BadScale { key: FourCc },
    #[error("{key}: SCAL has {scal} values for {elements} elements")]
    ScaleMismatch {
        key: FourCc,
        scal: usize,
        elements: usize,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Payload {
    Nested(Vec<Klv>),
    /// Exactly `struct_size × repeat` bytes, padding removed.
    Data(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Klv {
    pub key: FourCc,
    /// GPMF type character (`b'L'`, `b's'`, …); 0 for nested items.
    pub type_char: u8,
    pub struct_size: u8,
    pub repeat: u16,
    pub payload: Payload,
}

/// Parses one GPMF payload (one demuxed `gpmd` packet).
pub fn parse(data: &[u8]) -> Result<Vec<Klv>, GpmfError> {
    parse_level(data, 0, 0)
}

fn parse_level(data: &[u8], base: usize, depth: usize) -> Result<Vec<Klv>, GpmfError> {
    let mut items = Vec::new();
    let mut off = 0;
    while off < data.len() {
        let rest = &data[off..];
        if rest.len() < 8 {
            // Trailing zero padding is harmless; anything else is a cut item.
            if rest.iter().all(|&b| b == 0) {
                break;
            }
            return Err(GpmfError::Truncated {
                key: FourCc(*b"????"),
                offset: base + off,
                need: 8,
                have: rest.len(),
            });
        }
        let key = FourCc([rest[0], rest[1], rest[2], rest[3]]);
        // A null key is padding: end of data at this level.
        if key.0 == [0; 4] {
            break;
        }
        let type_char = rest[4];
        let struct_size = rest[5];
        let repeat = u16::from_be_bytes([rest[6], rest[7]]);
        let len = usize::from(struct_size) * usize::from(repeat);
        let padded = len.div_ceil(4) * 4;
        // The last item may omit its padding.
        if rest.len() - 8 < len {
            return Err(GpmfError::Truncated {
                key,
                offset: base + off,
                need: 8 + len,
                have: rest.len(),
            });
        }
        let body = &rest[8..8 + len];
        let payload = if type_char == 0 {
            if depth + 1 >= MAX_DEPTH {
                return Err(GpmfError::TooDeep { key });
            }
            Payload::Nested(parse_level(body, base + off + 8, depth + 1)?)
        } else {
            if type_size(type_char).is_none() && type_char != b'?' {
                return Err(GpmfError::UnknownType { key, type_char });
            }
            Payload::Data(body.to_vec())
        };
        items.push(Klv {
            key,
            type_char,
            struct_size,
            repeat,
            payload,
        });
        off += (8 + padded).min(rest.len());
    }
    Ok(items)
}

/// Size in bytes of one element of a GPMF type, None for unknown types and
/// for `?` (complex, sized by its TYPE).
pub fn type_size(t: u8) -> Option<usize> {
    Some(match t {
        b'b' | b'B' | b'c' => 1,
        b's' | b'S' => 2,
        b'f' | b'F' | b'l' | b'L' | b'q' => 4,
        b'd' | b'j' | b'J' | b'Q' => 8,
        b'G' | b'U' => 16,
        _ => return None,
    })
}

fn element(t: u8, b: &[u8]) -> Option<f64> {
    Some(match t {
        b'b' => f64::from(b[0] as i8),
        b'B' => f64::from(b[0]),
        b's' => f64::from(i16::from_be_bytes([b[0], b[1]])),
        b'S' => f64::from(u16::from_be_bytes([b[0], b[1]])),
        b'l' => f64::from(i32::from_be_bytes([b[0], b[1], b[2], b[3]])),
        b'L' => f64::from(u32::from_be_bytes([b[0], b[1], b[2], b[3]])),
        b'f' => f64::from(f32::from_be_bytes([b[0], b[1], b[2], b[3]])),
        // Q15.16 signed fixed point
        b'q' => f64::from(i32::from_be_bytes([b[0], b[1], b[2], b[3]])) / 65_536.0,
        b'd' => f64::from_be_bytes(b[..8].try_into().ok()?),
        b'j' => i64::from_be_bytes(b[..8].try_into().ok()?) as f64,
        b'J' => u64::from_be_bytes(b[..8].try_into().ok()?) as f64,
        // Q31.32 signed fixed point
        b'Q' => i64::from_be_bytes(b[..8].try_into().ok()?) as f64 / 4_294_967_296.0,
        _ => return None,
    })
}

impl Klv {
    pub fn data(&self) -> Option<&[u8]> {
        match &self.payload {
            Payload::Data(d) => Some(d),
            Payload::Nested(_) => None,
        }
    }

    pub fn children(&self) -> &[Klv] {
        match &self.payload {
            Payload::Nested(c) => c,
            Payload::Data(_) => &[],
        }
    }

    /// Decodes a numeric item into `repeat` rows of elements. `complex` is
    /// the stream's TYPE string, used only when the type is `?`.
    pub fn numbers(&self, complex: Option<&[u8]>) -> Result<Vec<Vec<f64>>, GpmfError> {
        let key = self.key;
        let data = self.data().ok_or(GpmfError::NotNumeric { key })?;
        let layout: Vec<u8> = if self.type_char == b'?' {
            complex.ok_or(GpmfError::MissingType { key })?.to_vec()
        } else {
            let size = type_size(self.type_char).ok_or(GpmfError::NotNumeric { key })?;
            if size == 0 || usize::from(self.struct_size) % size != 0 {
                return Err(GpmfError::BadStructSize {
                    key,
                    type_char: self.type_char,
                    struct_size: self.struct_size,
                });
            }
            vec![self.type_char; usize::from(self.struct_size) / size]
        };
        let mut row_size = 0;
        for &t in &layout {
            if matches!(t, b'c' | b'U' | b'F' | b'G') {
                return Err(GpmfError::NotNumeric { key });
            }
            row_size += type_size(t).ok_or(GpmfError::UnknownType { key, type_char: t })?;
        }
        if row_size != usize::from(self.struct_size) {
            return Err(GpmfError::BadStructSize {
                key,
                type_char: self.type_char,
                struct_size: self.struct_size,
            });
        }
        let rows = data
            .chunks_exact(row_size.max(1))
            .take(usize::from(self.repeat))
            .map(|row| {
                let mut out = Vec::with_capacity(layout.len());
                let mut at = 0;
                for &t in &layout {
                    let n = type_size(t).unwrap_or(0);
                    out.push(element(t, &row[at..at + n]).unwrap_or(f64::NAN));
                    at += n;
                }
                out
            })
            .collect();
        Ok(rows)
    }

    /// Text of a `c` item (Latin-1, as GoPro writes "m/s²"), trailing NULs
    /// and spaces removed.
    pub fn text(&self) -> Option<String> {
        if self.type_char != b'c' {
            return None;
        }
        let s: String = self.data()?.iter().map(|&b| b as char).collect();
        Some(s.trim_end_matches(['\0', ' ']).to_string())
    }

    /// Parses a `U` item (`yymmddhhmmss.sss`, UTC). None when malformed,
    /// which some firmware does (gpmf-parser issue #162).
    pub fn utc(&self) -> Option<DateTime<Utc>> {
        if self.type_char != b'U' {
            return None;
        }
        let s = std::str::from_utf8(self.data()?).ok()?;
        let s = s.trim_end_matches('\0');
        NaiveDateTime::parse_from_str(s, "%y%m%d%H%M%S%.f")
            .ok()
            .map(|n| n.and_utc())
    }
}

/// Divides decoded rows by SCAL: one value for every element, or one per
/// element of a row. A zero or non-finite scale (damaged file) is an error
/// and leaves `rows` untouched; callers should skip that stream's payload
/// rather than fail the whole file.
pub fn apply_scale(key: FourCc, rows: &mut [Vec<f64>], scal: &[f64]) -> Result<(), GpmfError> {
    if scal.iter().any(|s| *s == 0.0 || !s.is_finite()) {
        return Err(GpmfError::BadScale { key });
    }
    for row in rows.iter_mut() {
        if scal.len() == 1 {
            row.iter_mut().for_each(|v| *v /= scal[0]);
        } else if scal.len() == row.len() {
            row.iter_mut().zip(scal).for_each(|(v, s)| *v /= s);
        } else {
            return Err(GpmfError::ScaleMismatch {
                key,
                scal: scal.len(),
                elements: row.len(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{item, nested};

    #[test]
    fn parses_nested_items_and_padding() {
        let strm = nested(
            b"STRM",
            &[
                item(b"STNM", b'c', 1, 3, b"abc"), // 3 bytes, padded to 4
                item(b"TSMP", b'L', 4, 1, &7u32.to_be_bytes()),
            ],
        );
        let data = nested(b"DEVC", &[strm]);
        let items = parse(&data).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].key, FourCc::new(b"DEVC"));
        let strm = &items[0].children()[0];
        assert_eq!(strm.key, FourCc::new(b"STRM"));
        assert_eq!(strm.children()[0].text().as_deref(), Some("abc"));
        assert_eq!(strm.children()[1].numbers(None).unwrap(), vec![vec![7.0]]);
    }

    #[test]
    fn decodes_every_numeric_type() {
        let cases: Vec<(u8, Vec<u8>, f64)> = vec![
            (b'b', vec![0xff], -1.0),
            (b'B', vec![0xff], 255.0),
            (b's', (-2i16).to_be_bytes().to_vec(), -2.0),
            (b'S', 65_535u16.to_be_bytes().to_vec(), 65_535.0),
            (b'l', (-3i32).to_be_bytes().to_vec(), -3.0),
            (
                b'L',
                4_000_000_000u32.to_be_bytes().to_vec(),
                4_000_000_000.0,
            ),
            (b'f', 1.5f32.to_be_bytes().to_vec(), 1.5),
            (b'd', 2.25f64.to_be_bytes().to_vec(), 2.25),
            (b'j', (-5i64).to_be_bytes().to_vec(), -5.0),
            (b'J', 6u64.to_be_bytes().to_vec(), 6.0),
            (b'q', (3 * 65_536i32 + 32_768).to_be_bytes().to_vec(), 3.5),
            (b'Q', (-(1i64 << 32) / 4).to_be_bytes().to_vec(), -0.25),
        ];
        for (t, bytes, want) in cases {
            let data = item(b"TEST", t, bytes.len() as u8, 1, &bytes);
            let klv = &parse(&data).unwrap()[0];
            assert_eq!(
                klv.numbers(None).unwrap(),
                vec![vec![want]],
                "type {}",
                t as char
            );
        }
    }

    #[test]
    fn decodes_rows_and_complex_types() {
        // 2 rows of 3 i16
        let mut bytes = Vec::new();
        for v in [1i16, 2, 3, 4, 5, 6] {
            bytes.extend(v.to_be_bytes());
        }
        let klv = &parse(&item(b"ACCL", b's', 6, 2, &bytes)).unwrap()[0];
        assert_eq!(
            klv.numbers(None).unwrap(),
            vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]
        );

        // complex: TYPE "lS" → 6-byte rows
        let mut bytes = (-7i32).to_be_bytes().to_vec();
        bytes.extend(9u16.to_be_bytes());
        let klv = &parse(&item(b"CPLX", b'?', 6, 1, &bytes)).unwrap()[0];
        assert_eq!(klv.numbers(Some(b"lS")).unwrap(), vec![vec![-7.0, 9.0]]);
        assert_eq!(
            klv.numbers(None),
            Err(GpmfError::MissingType {
                key: FourCc::new(b"CPLX")
            })
        );
        assert!(matches!(
            klv.numbers(Some(b"ll")),
            Err(GpmfError::BadStructSize { .. })
        ));
    }

    #[test]
    fn zero_repeat_is_empty() {
        let klv = &parse(&item(b"FACE", b'?', 28, 0, &[])).unwrap()[0];
        assert!(klv.numbers(Some(b"Lffffff")).unwrap().is_empty());
    }

    #[test]
    fn scale_single_and_per_element() {
        let key = FourCc::new(b"GPS5");
        let mut rows = vec![vec![10.0, 20.0], vec![30.0, 40.0]];
        apply_scale(key, &mut rows, &[10.0]).unwrap();
        assert_eq!(rows, vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
        apply_scale(key, &mut rows, &[1.0, 2.0]).unwrap();
        assert_eq!(rows, vec![vec![1.0, 1.0], vec![3.0, 2.0]]);
        assert_eq!(
            apply_scale(key, &mut rows, &[1.0, 2.0, 3.0]),
            Err(GpmfError::ScaleMismatch {
                key,
                scal: 3,
                elements: 2
            })
        );
    }

    #[test]
    fn utc_and_latin1_text() {
        let klv = &parse(&item(b"GPSU", b'U', 16, 1, b"170417173103.985")).unwrap()[0];
        assert_eq!(
            klv.utc().unwrap().to_rfc3339(),
            "2017-04-17T17:31:03.985+00:00"
        );
        let bad = &parse(&item(b"GPSU", b'U', 16, 1, b"000000000000.000")).unwrap()[0];
        assert_eq!(bad.utc(), None);
        let siun = &parse(&item(b"SIUN", b'c', 4, 1, b"m/s\xb2")).unwrap()[0];
        assert_eq!(siun.text().as_deref(), Some("m/s²"));
    }

    #[test]
    fn rejects_truncated_input_at_every_length() {
        let strm = nested(b"STRM", &[item(b"GPS5", b'l', 20, 2, &[1u8; 40])]);
        let data = nested(b"DEVC", &[strm]);
        assert!(parse(&data).is_ok());
        for cut in 1..data.len() {
            let r = parse(&data[..cut]);
            assert!(
                matches!(r, Err(GpmfError::Truncated { .. })),
                "cut at {cut}: {r:?}"
            );
        }
    }

    #[test]
    fn accepts_trailing_zero_bytes_and_rejects_garbage() {
        let mut data = item(b"TSMP", b'L', 4, 1, &1u32.to_be_bytes());
        data.extend([0, 0, 0, 0]);
        assert_eq!(parse(&data).unwrap().len(), 1);
        let mut data = item(b"TSMP", b'L', 4, 1, &1u32.to_be_bytes());
        data.extend([1, 2, 3]);
        assert!(matches!(parse(&data), Err(GpmfError::Truncated { .. })));
    }

    #[test]
    fn rejects_unknown_types_and_deep_nesting() {
        let r = parse(&item(b"WHAT", b'z', 1, 1, &[0]));
        assert!(matches!(
            r,
            Err(GpmfError::UnknownType {
                type_char: b'z',
                ..
            })
        ));

        let mut data = item(b"LEAF", b'B', 1, 1, &[1]);
        for _ in 0..MAX_DEPTH {
            data = nested(b"NEST", &[data]);
        }
        assert!(matches!(parse(&data), Err(GpmfError::TooDeep { .. })));
    }

    #[test]
    fn non_numeric_items_refuse_numbers() {
        let klv = &parse(&item(b"STNM", b'c', 1, 2, b"ab")).unwrap()[0];
        assert_eq!(
            klv.numbers(None),
            Err(GpmfError::NotNumeric {
                key: FourCc::new(b"STNM")
            })
        );
    }

    #[test]
    fn rejects_zero_and_non_finite_scale() {
        let key = FourCc::new(b"GPS5");
        let mut rows = vec![vec![10.0, 20.0]];
        for scal in [&[0.0][..], &[1.0, 0.0], &[f64::NAN], &[f64::INFINITY]] {
            assert_eq!(
                apply_scale(key, &mut rows, scal),
                Err(GpmfError::BadScale { key })
            );
        }
        assert_eq!(rows, vec![vec![10.0, 20.0]]);
    }

    #[test]
    fn null_key_ends_the_level() {
        let mut data = item(b"TSMP", b'L', 4, 1, &1u32.to_be_bytes());
        data.extend([0u8; 16]);
        assert_eq!(parse(&data).unwrap().len(), 1);
    }
}
