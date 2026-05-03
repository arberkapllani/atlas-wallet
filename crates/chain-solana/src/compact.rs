//! Solana's compact-u16 (a.k.a. `ShortU16`) length encoder.
//!
//! Solana serialises every variable-length array with a 1-3 byte
//! length prefix where each byte stores 7 bits of value and uses the
//! high bit as a continuation flag.

/// Encode `value` (must fit in u16) into 1..=3 bytes.
pub fn encode(value: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(3);
    let mut rem = value as u32;
    loop {
        let mut byte = (rem & 0x7f) as u8;
        rem >>= 7;
        if rem == 0 {
            out.push(byte);
            return out;
        }
        byte |= 0x80;
        out.push(byte);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_values_one_byte() {
        assert_eq!(encode(0), vec![0]);
        assert_eq!(encode(1), vec![1]);
        assert_eq!(encode(0x7f), vec![0x7f]);
    }

    #[test]
    fn medium_values_two_bytes() {
        assert_eq!(encode(0x80), vec![0x80, 0x01]);
        assert_eq!(encode(0xff), vec![0xff, 0x01]);
        assert_eq!(encode(0x3fff), vec![0xff, 0x7f]);
    }

    #[test]
    fn large_values_three_bytes() {
        assert_eq!(encode(0x4000), vec![0x80, 0x80, 0x01]);
        assert_eq!(encode(u16::MAX), vec![0xff, 0xff, 0x03]);
    }
}
