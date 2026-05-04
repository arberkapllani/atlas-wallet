//! Trezor message-type catalogue (subset Atlas actually issues)
//! plus minimal protobuf wire helpers.
//!
//! Numeric ids match the Trezor `messages-management.proto` and
//! `messages-bitcoin.proto` / `messages-ethereum.proto`
//! definitions used by every shipping firmware (1.x and 2.x).

use thiserror::Error;

/// Numeric message-type ids. Only the ones Atlas issues are
/// listed; everything else is intentionally absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum MessageType {
    // ── management ───────────────────────────────────────────
    Initialize = 0,
    Features = 17,
    Ping = 1,
    Success = 2,
    Failure = 3,
    ButtonRequest = 26,
    ButtonAck = 27,
    PinMatrixRequest = 18,
    PinMatrixAck = 19,
    PassphraseRequest = 41,
    PassphraseAck = 42,

    // ── bitcoin ──────────────────────────────────────────────
    GetPublicKey = 11,
    PublicKey = 12,
    GetAddress = 29,
    Address = 30,

    // ── ethereum ─────────────────────────────────────────────
    EthereumGetAddress = 56,
    EthereumAddress = 57,
}

impl MessageType {
    /// Convert from a raw numeric id; returns `None` for ids
    /// outside the catalogue.
    pub fn from_u16(v: u16) -> Option<Self> {
        Some(match v {
            0 => Self::Initialize,
            17 => Self::Features,
            1 => Self::Ping,
            2 => Self::Success,
            3 => Self::Failure,
            26 => Self::ButtonRequest,
            27 => Self::ButtonAck,
            18 => Self::PinMatrixRequest,
            19 => Self::PinMatrixAck,
            41 => Self::PassphraseRequest,
            42 => Self::PassphraseAck,
            11 => Self::GetPublicKey,
            12 => Self::PublicKey,
            29 => Self::GetAddress,
            30 => Self::Address,
            56 => Self::EthereumGetAddress,
            57 => Self::EthereumAddress,
            _ => return None,
        })
    }

    pub fn as_u16(self) -> u16 {
        self as u16
    }
}

// -------------------------------------------------------------
// Minimal protobuf wire helpers
// -------------------------------------------------------------
//
// Trezor uses standard `proto2` encoding, so every field is a
// (tag, wire-type) varint header followed by a value. Atlas only
// needs three wire types:
//
//   - varint  (wire type 0)
//   - 64-bit  (wire type 1) — unused
//   - length-delimited (wire type 2) — bytes / string / message
//
// `tag` is the protobuf field number (1..=N).

/// Wire-type values defined by the protobuf spec.
pub const WIRE_VARINT: u8 = 0;
pub const WIRE_LEN_DELIM: u8 = 2;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WireError {
    #[error("unexpected end of buffer")]
    Eof,
    #[error("varint too long")]
    VarintOverflow,
    #[error("unsupported wire type {0}")]
    UnsupportedWireType(u8),
}

/// Encode an unsigned varint.
pub fn write_varint(buf: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        buf.push(((value as u8) & 0x7F) | 0x80);
        value >>= 7;
    }
    buf.push(value as u8);
}

/// Read an unsigned varint.
pub fn read_varint(bytes: &[u8]) -> Result<(u64, usize), WireError> {
    let mut result: u64 = 0;
    for (i, b) in bytes.iter().take(10).enumerate() {
        result |= ((b & 0x7F) as u64) << (7 * i);
        if b & 0x80 == 0 {
            return Ok((result, i + 1));
        }
    }
    if bytes.len() < 10 {
        Err(WireError::Eof)
    } else {
        Err(WireError::VarintOverflow)
    }
}

/// Write a `(tag, wire-type)` header.
pub fn write_tag(buf: &mut Vec<u8>, tag: u32, wire: u8) {
    write_varint(buf, ((tag as u64) << 3) | wire as u64);
}

/// Write a length-delimited bytes field.
pub fn write_bytes(buf: &mut Vec<u8>, tag: u32, value: &[u8]) {
    write_tag(buf, tag, WIRE_LEN_DELIM);
    write_varint(buf, value.len() as u64);
    buf.extend_from_slice(value);
}

/// Write a length-delimited UTF-8 string field.
pub fn write_string(buf: &mut Vec<u8>, tag: u32, value: &str) {
    write_bytes(buf, tag, value.as_bytes());
}

/// Write a varint field.
pub fn write_varint_field(buf: &mut Vec<u8>, tag: u32, value: u64) {
    write_tag(buf, tag, WIRE_VARINT);
    write_varint(buf, value);
}

/// Iterate the (tag, wire-type, value-bytes) triples in a payload.
/// Used by response decoders.
pub struct WireReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> WireReader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.pos >= self.buf.len()
    }

    /// Read the next field. Returns `(tag, value)` where `value` is
    /// either a varint or a borrowed byte slice depending on the
    /// wire type. Skips unknown wire types by failing — Atlas only
    /// ever decodes payloads it knows the shape of.
    pub fn next_field(&mut self) -> Result<Option<(u32, FieldValue<'a>)>, WireError> {
        if self.is_empty() {
            return Ok(None);
        }
        let (header, n) = read_varint(&self.buf[self.pos..])?;
        self.pos += n;
        let tag = (header >> 3) as u32;
        let wire = (header & 0x7) as u8;
        match wire {
            WIRE_VARINT => {
                let (v, m) = read_varint(&self.buf[self.pos..])?;
                self.pos += m;
                Ok(Some((tag, FieldValue::Varint(v))))
            }
            WIRE_LEN_DELIM => {
                let (len, m) = read_varint(&self.buf[self.pos..])?;
                self.pos += m;
                let len = len as usize;
                if self.pos + len > self.buf.len() {
                    return Err(WireError::Eof);
                }
                let slice = &self.buf[self.pos..self.pos + len];
                self.pos += len;
                Ok(Some((tag, FieldValue::Bytes(slice))))
            }
            other => Err(WireError::UnsupportedWireType(other)),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum FieldValue<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_type_round_trip() {
        for v in [0u16, 1, 2, 3, 11, 12, 17, 26, 56, 57] {
            assert_eq!(MessageType::from_u16(v).unwrap().as_u16(), v);
        }
        assert!(MessageType::from_u16(9999).is_none());
    }

    #[test]
    fn varint_round_trip() {
        for v in [0u64, 1, 127, 128, 255, 16384, u32::MAX as u64, u64::MAX] {
            let mut buf = Vec::new();
            write_varint(&mut buf, v);
            let (back, n) = read_varint(&buf).unwrap();
            assert_eq!(back, v);
            assert_eq!(n, buf.len());
        }
    }

    #[test]
    fn varint_overflow_detected() {
        let bad = [0x80u8; 11];
        assert_eq!(read_varint(&bad), Err(WireError::VarintOverflow));
    }

    #[test]
    fn bytes_field_round_trip() {
        let mut buf = Vec::new();
        write_bytes(&mut buf, 7, b"hello");
        let mut r = WireReader::new(&buf);
        let (tag, v) = r.next_field().unwrap().unwrap();
        assert_eq!(tag, 7);
        match v {
            FieldValue::Bytes(b) => assert_eq!(b, b"hello"),
            _ => panic!("expected bytes"),
        }
        assert!(r.next_field().unwrap().is_none());
    }

    #[test]
    fn varint_field_round_trip() {
        let mut buf = Vec::new();
        write_varint_field(&mut buf, 3, 42);
        let mut r = WireReader::new(&buf);
        let (tag, v) = r.next_field().unwrap().unwrap();
        assert_eq!(tag, 3);
        match v {
            FieldValue::Varint(n) => assert_eq!(n, 42),
            _ => panic!("expected varint"),
        }
    }

    #[test]
    fn rejects_unsupported_wire_type() {
        // tag=1, wire-type=5 (32-bit fixed, unused by Atlas)
        let bad = vec![(1u8 << 3) | 5, 1, 2, 3, 4];
        let mut r = WireReader::new(&bad);
        let err = r.next_field().unwrap_err();
        assert_eq!(err, WireError::UnsupportedWireType(5));
    }
}
