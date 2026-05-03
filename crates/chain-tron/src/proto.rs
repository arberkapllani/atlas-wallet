//! Minimal hand-rolled protobuf wire-format encoder.
//!
//! Tron's transaction format is a Google protobuf 3 message. Pulling in
//! `prost` plus its build-time code generator just to encode three fixed
//! shapes would balloon our dependency surface; the pieces we actually
//! need fit in a few dozen lines of pure Rust.
//!
//! We only support the wire types Tron transactions exercise:
//!
//! * **Wire type 0** — `varint` (used for `int64` / `enum` fields).
//! * **Wire type 2** — length-delimited (used for `bytes`, `string`,
//!   and embedded messages).
//!
//! All field numbers are encoded as `(field_number << 3) | wire_type`,
//! matching the protobuf spec.

/// Append a base-128 varint of `value` to `out` (little-endian, MSB
/// continuation bit). Same encoding used by every protobuf integer type
/// we touch.
pub fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(((value as u8) & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Write a `varint` field (wire type 0). Used for `int64` / `enum` /
/// `bool` fields. Tron's `int64` fields are always non-negative in the
/// shapes we build, so a plain `u64` cast is correct.
pub fn write_varint_field(out: &mut Vec<u8>, field: u32, value: u64) {
    write_varint(out, (field as u64) << 3);
    write_varint(out, value);
}

/// Write a length-delimited field (wire type 2). Used for `bytes`,
/// `string`, and embedded messages.
pub fn write_bytes_field(out: &mut Vec<u8>, field: u32, value: &[u8]) {
    write_varint(out, ((field as u64) << 3) | 2);
    write_varint(out, value.len() as u64);
    out.extend_from_slice(value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_zero() {
        let mut v = Vec::new();
        write_varint(&mut v, 0);
        assert_eq!(v, vec![0]);
    }

    #[test]
    fn varint_one_byte_max() {
        let mut v = Vec::new();
        write_varint(&mut v, 127);
        assert_eq!(v, vec![0x7f]);
    }

    #[test]
    fn varint_two_bytes() {
        let mut v = Vec::new();
        write_varint(&mut v, 300);
        // 300 = 0b10_1010_1100 -> 0xac, 0x02
        assert_eq!(v, vec![0xac, 0x02]);
    }

    #[test]
    fn varint_large() {
        let mut v = Vec::new();
        write_varint(&mut v, u64::MAX);
        // 10 bytes of 0xff... last byte is 0x01.
        assert_eq!(v.len(), 10);
        assert_eq!(*v.last().unwrap(), 0x01);
    }

    #[test]
    fn varint_field_tag() {
        let mut v = Vec::new();
        write_varint_field(&mut v, 3, 150);
        // tag = (3 << 3) | 0 = 0x18, then varint(150) = 0x96 0x01
        assert_eq!(v, vec![0x18, 0x96, 0x01]);
    }

    #[test]
    fn bytes_field_tag_and_length_prefix() {
        let mut v = Vec::new();
        write_bytes_field(&mut v, 1, b"hi");
        // tag = (1 << 3) | 2 = 0x0a, len = 2
        assert_eq!(v, vec![0x0a, 0x02, b'h', b'i']);
    }

    #[test]
    fn bytes_field_empty_payload() {
        let mut v = Vec::new();
        write_bytes_field(&mut v, 2, b"");
        assert_eq!(v, vec![0x12, 0x00]);
    }
}
