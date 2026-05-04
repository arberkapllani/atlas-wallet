//! APDU (Application Protocol Data Unit) encoder / decoder. Pure
//! data — no I/O.
//!
//! Outgoing command layout (short-form, ≤ 255 bytes payload):
//! ```text
//! +-----+-----+----+----+-----+----------+
//! | CLA | INS | P1 | P2 | LC  | data ... |
//! +-----+-----+----+----+-----+----------+
//! ```
//!
//! Incoming response:
//! ```text
//! +----------+--------+--------+
//! | data ... | SW1    | SW2    |
//! +----------+--------+--------+
//! ```

use thiserror::Error;

/// Maximum payload length for a short-form APDU.
pub const MAX_PAYLOAD: usize = 255;

/// One outgoing APDU command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApduCommand {
    pub cla: u8,
    pub ins: u8,
    pub p1: u8,
    pub p2: u8,
    pub data: Vec<u8>,
}

impl ApduCommand {
    /// Build a new command. Returns `None` if `data` is longer than
    /// the short-form limit (255 bytes).
    pub fn new(cla: u8, ins: u8, p1: u8, p2: u8, data: Vec<u8>) -> Option<Self> {
        if data.len() > MAX_PAYLOAD {
            return None;
        }
        Some(Self {
            cla,
            ins,
            p1,
            p2,
            data,
        })
    }

    /// Encode to wire bytes (short-form).
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(5 + self.data.len());
        buf.push(self.cla);
        buf.push(self.ins);
        buf.push(self.p1);
        buf.push(self.p2);
        buf.push(self.data.len() as u8);
        buf.extend_from_slice(&self.data);
        buf
    }
}

/// One decoded response. The status word is split into a
/// human-readable [`StatusWord`] so handlers can pattern-match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApduResponse {
    pub data: Vec<u8>,
    pub sw: StatusWord,
}

impl ApduResponse {
    /// Returns `true` iff the device replied `0x9000` (success).
    pub fn is_ok(&self) -> bool {
        matches!(self.sw, StatusWord::Ok)
    }

    /// Decode raw bytes received from the device.
    pub fn decode(bytes: &[u8]) -> Result<Self, ApduError> {
        if bytes.len() < 2 {
            return Err(ApduError::Truncated);
        }
        let split = bytes.len() - 2;
        let sw = StatusWord::from_u16(u16::from_be_bytes([bytes[split], bytes[split + 1]]));
        Ok(Self {
            data: bytes[..split].to_vec(),
            sw,
        })
    }
}

/// Recognised Ledger status words. Anything we don't match
/// explicitly falls into [`StatusWord::Other`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusWord {
    /// `0x9000` — Success.
    Ok,
    /// `0x6985` — User rejected on-device prompt.
    UserRejected,
    /// `0x6982` — Device is locked / pin required.
    Locked,
    /// `0x6E00` — Wrong CLA: app on the device doesn't match.
    WrongApp,
    /// `0x6D00` — INS not supported by this app.
    UnknownIns,
    /// Anything else.
    Other(u16),
}

impl StatusWord {
    pub fn from_u16(v: u16) -> Self {
        match v {
            0x9000 => Self::Ok,
            0x6985 => Self::UserRejected,
            0x6982 => Self::Locked,
            0x6E00 => Self::WrongApp,
            0x6D00 => Self::UnknownIns,
            other => Self::Other(other),
        }
    }

    pub fn as_u16(&self) -> u16 {
        match self {
            Self::Ok => 0x9000,
            Self::UserRejected => 0x6985,
            Self::Locked => 0x6982,
            Self::WrongApp => 0x6E00,
            Self::UnknownIns => 0x6D00,
            Self::Other(v) => *v,
        }
    }
}

#[derive(Debug, Error)]
pub enum ApduError {
    #[error("response too short to contain a status word")]
    Truncated,
    #[error("payload exceeds {MAX_PAYLOAD} bytes")]
    PayloadTooLarge,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_round_trip() {
        let cmd = ApduCommand::new(0xE0, 0x02, 0x00, 0x00, vec![0x01, 0x02, 0x03]).unwrap();
        let wire = cmd.encode();
        assert_eq!(wire, vec![0xE0, 0x02, 0x00, 0x00, 0x03, 0x01, 0x02, 0x03]);
    }

    #[test]
    fn rejects_oversize_payload() {
        let big = vec![0u8; MAX_PAYLOAD + 1];
        assert!(ApduCommand::new(0, 0, 0, 0, big).is_none());
    }

    #[test]
    fn decode_response_ok() {
        let bytes = vec![0xAA, 0xBB, 0x90, 0x00];
        let r = ApduResponse::decode(&bytes).unwrap();
        assert_eq!(r.data, vec![0xAA, 0xBB]);
        assert_eq!(r.sw, StatusWord::Ok);
        assert!(r.is_ok());
    }

    #[test]
    fn decode_response_user_rejected() {
        let r = ApduResponse::decode(&[0x69, 0x85]).unwrap();
        assert_eq!(r.sw, StatusWord::UserRejected);
        assert!(!r.is_ok());
        assert!(r.data.is_empty());
    }

    #[test]
    fn decode_truncated_response_errors() {
        assert!(ApduResponse::decode(&[0x90]).is_err());
        assert!(ApduResponse::decode(&[]).is_err());
    }

    #[test]
    fn status_word_round_trip() {
        for code in [0x9000u16, 0x6985, 0x6982, 0x6E00, 0x6D00, 0x6F01] {
            assert_eq!(StatusWord::from_u16(code).as_u16(), code);
        }
    }
}
