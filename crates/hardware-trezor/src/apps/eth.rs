//! Trezor `EthereumGetAddress` flow.
//!
//! Request shape (`messages-ethereum.proto`, message 56):
//!
//! ```protobuf
//! message EthereumGetAddress {
//!     repeated uint32 address_n = 1;
//!     optional bool   show_display = 2;
//! }
//! ```
//!
//! Response (`EthereumAddress`, message 57):
//!
//! ```protobuf
//! message EthereumAddress {
//!     optional bytes  _old_address = 1;   // pre-2.4.0 firmware
//!     optional string address      = 2;   // 0x-prefixed checksum
//! }
//! ```

use crate::framing::MessageEnvelope;
use crate::messages::{self, FieldValue, MessageType, WireReader};
use crate::path::Bip32Path;
use crate::transport::{Transport, TransportError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EthError {
    #[error("transport error: {0}")]
    Transport(#[from] TransportError),
    #[error("device returned a Failure: code={code} message={message}")]
    Failure { code: u32, message: String },
    #[error("device returned an unexpected message type: {0:#06x}")]
    Unexpected(u16),
    #[error("malformed response: {0}")]
    Malformed(String),
    #[error("user interaction required (button / pin / passphrase) — not implemented yet")]
    InteractionRequired,
}

/// Build an `EthereumGetAddress` request envelope.
pub fn build_get_address(path: &Bip32Path, show_display: bool) -> MessageEnvelope {
    let mut buf = Vec::new();
    // address_n is `repeated uint32` with field number 1. Trezor
    // accepts either packed-repeated or non-packed repeated; we use
    // non-packed for simplicity.
    for component in path.as_slice() {
        messages::write_varint_field(&mut buf, 1, *component as u64);
    }
    if show_display {
        messages::write_varint_field(&mut buf, 2, 1);
    }
    MessageEnvelope {
        message_type: MessageType::EthereumGetAddress.as_u16(),
        payload: buf,
    }
}

/// Decode an `EthereumAddress` response envelope. The device may
/// instead reply with `Failure` (code 3), `ButtonRequest` etc.
pub fn decode_address(env: &MessageEnvelope) -> Result<String, EthError> {
    match MessageType::from_u16(env.message_type) {
        Some(MessageType::EthereumAddress) => {}
        Some(MessageType::Failure) => return Err(decode_failure(env)),
        Some(
            MessageType::ButtonRequest
            | MessageType::PinMatrixRequest
            | MessageType::PassphraseRequest,
        ) => return Err(EthError::InteractionRequired),
        _ => return Err(EthError::Unexpected(env.message_type)),
    }
    let mut r = WireReader::new(&env.payload);
    while let Some((tag, value)) = r
        .next_field()
        .map_err(|e| EthError::Malformed(e.to_string()))?
    {
        if tag == 2 {
            if let FieldValue::Bytes(b) = value {
                let s = std::str::from_utf8(b)
                    .map_err(|_| EthError::Malformed("address not utf8".into()))?;
                return Ok(s.to_string());
            }
        }
    }
    Err(EthError::Malformed("no address field in response".into()))
}

fn decode_failure(env: &MessageEnvelope) -> EthError {
    // Failure { code: uint32 = 1, message: string = 2 }
    let mut code = 0u32;
    let mut message = String::new();
    let mut r = WireReader::new(&env.payload);
    while let Ok(Some((tag, v))) = r.next_field() {
        match (tag, v) {
            (1, FieldValue::Varint(c)) => code = c as u32,
            (2, FieldValue::Bytes(b)) => {
                if let Ok(s) = std::str::from_utf8(b) {
                    message = s.to_string();
                }
            }
            _ => {}
        }
    }
    EthError::Failure { code, message }
}

/// Convenience: send and decode in one call.
pub async fn get_address<T: Transport + ?Sized>(
    transport: &T,
    path: &Bip32Path,
    show_display: bool,
) -> Result<String, EthError> {
    let req = build_get_address(path, show_display);
    let resp = transport.exchange(&req).await?;
    decode_address(&resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::{write_string, write_varint_field};
    use crate::transport::MockTransport;

    fn ok_address_response(addr: &str) -> MessageEnvelope {
        let mut payload = Vec::new();
        write_string(&mut payload, 2, addr);
        MessageEnvelope {
            message_type: MessageType::EthereumAddress.as_u16(),
            payload,
        }
    }

    fn failure_response(code: u32, msg: &str) -> MessageEnvelope {
        let mut payload = Vec::new();
        write_varint_field(&mut payload, 1, code as u64);
        write_string(&mut payload, 2, msg);
        MessageEnvelope {
            message_type: MessageType::Failure.as_u16(),
            payload,
        }
    }

    #[test]
    fn builds_request_with_repeated_path_and_show_display() {
        let path = Bip32Path::parse("m/44'/60'/0'/0/0").unwrap();
        let env = build_get_address(&path, true);
        assert_eq!(env.message_type, MessageType::EthereumGetAddress.as_u16());
        // Each component written as a varint field with tag 1, plus
        // the show_display=1 field tagged 2. Decode round-trip:
        let mut got_components = Vec::new();
        let mut got_display = None;
        let mut r = WireReader::new(&env.payload);
        while let Some((tag, v)) = r.next_field().unwrap() {
            match (tag, v) {
                (1, FieldValue::Varint(c)) => got_components.push(c as u32),
                (2, FieldValue::Varint(d)) => got_display = Some(d == 1),
                _ => panic!("unexpected field"),
            }
        }
        assert_eq!(got_components, path.as_slice());
        assert_eq!(got_display, Some(true));
    }

    #[test]
    fn decodes_well_formed_address() {
        let r = ok_address_response("0xabc1230000000000000000000000000000000000");
        let addr = decode_address(&r).unwrap();
        assert_eq!(addr, "0xabc1230000000000000000000000000000000000");
    }

    #[test]
    fn maps_failure_to_typed_error() {
        let r = failure_response(7, "user cancelled");
        match decode_address(&r) {
            Err(EthError::Failure { code, message }) => {
                assert_eq!(code, 7);
                assert_eq!(message, "user cancelled");
            }
            other => panic!("expected Failure, got {other:?}"),
        }
    }

    #[test]
    fn button_request_is_flagged_as_interaction() {
        let env = MessageEnvelope {
            message_type: MessageType::ButtonRequest.as_u16(),
            payload: vec![],
        };
        assert!(matches!(
            decode_address(&env),
            Err(EthError::InteractionRequired)
        ));
    }

    #[test]
    fn unknown_message_type_errors() {
        let env = MessageEnvelope {
            message_type: 9999,
            payload: vec![],
        };
        assert!(matches!(decode_address(&env), Err(EthError::Unexpected(_))));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn end_to_end_with_mock() {
        let t = MockTransport::new(vec![Ok(ok_address_response("0xdeadbeef"))]);
        let path = Bip32Path::parse("m/44'/60'/0'/0/0").unwrap();
        let addr = get_address(&t, &path, false).await.unwrap();
        assert_eq!(addr, "0xdeadbeef");
        let history = t.history();
        assert_eq!(history.len(), 1);
        assert_eq!(
            history[0].message_type,
            MessageType::EthereumGetAddress.as_u16()
        );
    }
}
