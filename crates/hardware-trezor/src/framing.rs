//! Trezor v1 USB-HID framing.
//!
//! Wire layout for one outgoing message (before HID chunking):
//!
//! ```text
//! +----+----+--------+--------+------------+
//! |'?' |'#' |'#'    | type:2 | length:4   | payload:N
//! +----+----+--------+--------+------------+
//! ```
//!
//! - The first byte is `?` (0x3F) — the HID report id.
//! - Then the literal magic `##`.
//! - Then a big-endian u16 message type id.
//! - Then a big-endian u32 payload length.
//! - Then the protobuf-encoded payload.
//!
//! The full envelope is sliced into 64-byte HID frames; every
//! continuation frame is also prefixed with `0x3F`.

use thiserror::Error;

/// HID frame size for Trezor v1.
pub const HID_FRAME_SIZE: usize = 64;
/// Header byte every HID frame starts with (`'?'`).
pub const HID_REPORT_ID: u8 = 0x3F;
/// Magic in the first frame: `##`.
pub const HEADER_MAGIC: [u8; 2] = [b'#', b'#'];

/// One decoded message envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageEnvelope {
    /// Numeric message-type id (see [`crate::messages::MessageType`]).
    pub message_type: u16,
    /// Protobuf-encoded payload.
    pub payload: Vec<u8>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FramingError {
    #[error("first frame missing header magic")]
    BadMagic,
    #[error("frame too short")]
    TruncatedFrame,
    #[error("declared payload length exceeds actual data ({declared} > {actual})")]
    PayloadTooShort { declared: usize, actual: usize },
    #[error("missing report-id prefix on continuation frame")]
    BadContinuation,
}

/// Encode `envelope` into a chain of 64-byte HID frames.
pub fn encode_message(envelope: &MessageEnvelope) -> Vec<[u8; HID_FRAME_SIZE]> {
    // Build the contiguous wire stream first, then chunk.
    let mut stream = Vec::with_capacity(8 + envelope.payload.len());
    stream.extend_from_slice(&HEADER_MAGIC);
    stream.extend_from_slice(&envelope.message_type.to_be_bytes());
    stream.extend_from_slice(&(envelope.payload.len() as u32).to_be_bytes());
    stream.extend_from_slice(&envelope.payload);

    // Each HID frame: [0x3F, ...up-to-63 bytes of stream...] padded with 0x00.
    let mut frames = Vec::new();
    for chunk in stream.chunks(HID_FRAME_SIZE - 1) {
        let mut frame = [0u8; HID_FRAME_SIZE];
        frame[0] = HID_REPORT_ID;
        frame[1..1 + chunk.len()].copy_from_slice(chunk);
        frames.push(frame);
    }
    if frames.is_empty() {
        // even an empty payload still needs a header frame
        let mut frame = [0u8; HID_FRAME_SIZE];
        frame[0] = HID_REPORT_ID;
        frames.push(frame);
    }
    frames
}

/// Decode a sequence of HID frames into one envelope. Caller is
/// responsible for accumulating frames until enough payload bytes
/// are present.
pub fn decode_message(frames: &[[u8; HID_FRAME_SIZE]]) -> Result<MessageEnvelope, FramingError> {
    if frames.is_empty() {
        return Err(FramingError::TruncatedFrame);
    }
    // First frame: 0x3F | '##' | type:2 | len:4 | payload-fragment
    let first = &frames[0];
    if first[0] != HID_REPORT_ID {
        return Err(FramingError::BadContinuation);
    }
    if first[1..3] != HEADER_MAGIC {
        return Err(FramingError::BadMagic);
    }
    let message_type = u16::from_be_bytes([first[3], first[4]]);
    let declared_len = u32::from_be_bytes([first[5], first[6], first[7], first[8]]) as usize;

    let mut payload = Vec::with_capacity(declared_len);
    payload.extend_from_slice(&first[9..]);
    for cont in &frames[1..] {
        if cont[0] != HID_REPORT_ID {
            return Err(FramingError::BadContinuation);
        }
        payload.extend_from_slice(&cont[1..]);
    }
    if payload.len() < declared_len {
        return Err(FramingError::PayloadTooShort {
            declared: declared_len,
            actual: payload.len(),
        });
    }
    payload.truncate(declared_len);
    Ok(MessageEnvelope {
        message_type,
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_payload(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i & 0xFF) as u8).collect()
    }

    #[test]
    fn round_trip_small_payload() {
        let env = MessageEnvelope {
            message_type: 11,
            payload: dummy_payload(4),
        };
        let frames = encode_message(&env);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0][0], HID_REPORT_ID);
        let back = decode_message(&frames).unwrap();
        assert_eq!(back, env);
    }

    #[test]
    fn round_trip_multi_frame_payload() {
        // 200 bytes of payload + 8-byte header = 208 bytes,
        // chunked into ceil(208 / 63) = 4 frames.
        let env = MessageEnvelope {
            message_type: 99,
            payload: dummy_payload(200),
        };
        let frames = encode_message(&env);
        assert_eq!(frames.len(), 4);
        for f in &frames {
            assert_eq!(f[0], HID_REPORT_ID);
            assert_eq!(f.len(), HID_FRAME_SIZE);
        }
        let back = decode_message(&frames).unwrap();
        assert_eq!(back, env);
    }

    #[test]
    fn round_trip_empty_payload() {
        let env = MessageEnvelope {
            message_type: 1,
            payload: Vec::new(),
        };
        let frames = encode_message(&env);
        assert_eq!(frames.len(), 1);
        let back = decode_message(&frames).unwrap();
        assert_eq!(back, env);
    }

    #[test]
    fn rejects_missing_magic() {
        let mut frame = [0u8; HID_FRAME_SIZE];
        frame[0] = HID_REPORT_ID;
        // no '##' magic follows
        assert_eq!(decode_message(&[frame]), Err(FramingError::BadMagic));
    }

    #[test]
    fn rejects_truncated_payload() {
        // Header claims 100 bytes payload, but no continuation frame
        // is supplied → must error.
        let mut frame = [0u8; HID_FRAME_SIZE];
        frame[0] = HID_REPORT_ID;
        frame[1..3].copy_from_slice(&HEADER_MAGIC);
        frame[3..5].copy_from_slice(&7u16.to_be_bytes());
        frame[5..9].copy_from_slice(&100u32.to_be_bytes());
        let res = decode_message(&[frame]);
        assert!(matches!(res, Err(FramingError::PayloadTooShort { .. })));
    }

    #[test]
    fn rejects_bad_continuation_byte() {
        let env = MessageEnvelope {
            message_type: 5,
            payload: dummy_payload(80),
        };
        let mut frames = encode_message(&env);
        frames[1][0] = 0x00; // corrupt continuation prefix
        assert_eq!(decode_message(&frames), Err(FramingError::BadContinuation));
    }
}
