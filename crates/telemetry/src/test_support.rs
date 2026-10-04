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
