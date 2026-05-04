//! Trezor `GetAddress` flow for Bitcoin (and BTC siblings).
//!
//! Request shape (`messages-bitcoin.proto`, message 29):
//!
//! ```protobuf
//! message GetAddress {
//!     repeated uint32 address_n    = 1;
//!     optional string coin_name    = 2;  // "Bitcoin", "Litecoin", …
//!     optional bool   show_display = 3;
//!     optional uint32 script_type  = 5;  // SPENDADDRESS=0, SPENDP2SHWITNESS=1, SPENDWITNESS=3, SPENDTAPROOT=4
//! }
//! ```
//!
//! Response (`Address`, message 30):
//!
//! ```protobuf
//! message Address {
//!     required string address = 1;
//! }
//! ```

use crate::framing::MessageEnvelope;
use crate::messages::{self, FieldValue, MessageType, WireReader};
use crate::path::Bip32Path;
use crate::transport::{Transport, TransportError};
use thiserror::Error;

/// `script_type` values from `messages-bitcoin.proto`.
#[derive(Debug, Clone, Copy)]
#[repr(u32)]
pub enum ScriptType {
    /// Legacy P2PKH (`1...`).
    SpendAddress = 0,
    /// Wrapped segwit (`3...`).
    SpendP2shWitness = 1,
    /// Native segwit (`bc1q...`).
    SpendWitness = 3,
    /// Taproot (`bc1p...`).
    SpendTaproot = 4,
}

#[derive(Debug, Error)]
pub enum BtcError {
    #[error("transport error: {0}")]
    Transport(#[from] TransportError),
    #[error("device returned a Failure: code={code} message={message}")]
    Failure { code: u32, message: String },
    #[error("device returned an unexpected message type: {0:#06x}")]
    Unexpected(u16),
    #[error("malformed response: {0}")]
    Malformed(String),
    #[error("user interaction required (button / pin / passphrase)")]
    InteractionRequired,
}

/// Build a `GetAddress` request envelope.
pub fn build_get_address(
    path: &Bip32Path,
    coin_name: &str,
    script_type: ScriptType,
    show_display: bool,
) -> MessageEnvelope {
    let mut buf = Vec::new();
    for component in path.as_slice() {
        messages::write_varint_field(&mut buf, 1, *component as u64);
    }
    messages::write_string(&mut buf, 2, coin_name);
    if show_display {
        messages::write_varint_field(&mut buf, 3, 1);
    }
    messages::write_varint_field(&mut buf, 5, script_type as u64);
    MessageEnvelope {
        message_type: MessageType::GetAddress.as_u16(),
        payload: buf,
    }
}

/// Decode an `Address` response.
pub fn decode_address(env: &MessageEnvelope) -> Result<String, BtcError> {
    match MessageType::from_u16(env.message_type) {
        Some(MessageType::Address) => {}
        Some(MessageType::Failure) => return Err(decode_failure(env)),
        Some(
            MessageType::ButtonRequest
            | MessageType::PinMatrixRequest
            | MessageType::PassphraseRequest,
        ) => return Err(BtcError::InteractionRequired),
        _ => return Err(BtcError::Unexpected(env.message_type)),
    }
    let mut r = WireReader::new(&env.payload);
    while let Some((tag, value)) = r
        .next_field()
        .map_err(|e| BtcError::Malformed(e.to_string()))?
    {
        if tag == 1 {
            if let FieldValue::Bytes(b) = value {
                let s = std::str::from_utf8(b)
                    .map_err(|_| BtcError::Malformed("address not utf8".into()))?;
                return Ok(s.to_string());
            }
        }
    }
    Err(BtcError::Malformed("no address field in response".into()))
}

fn decode_failure(env: &MessageEnvelope) -> BtcError {
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
    BtcError::Failure { code, message }
}

/// Convenience: send and decode in one call.
pub async fn get_address<T: Transport + ?Sized>(
    transport: &T,
    path: &Bip32Path,
    coin_name: &str,
    script_type: ScriptType,
    show_display: bool,
) -> Result<String, BtcError> {
    let req = build_get_address(path, coin_name, script_type, show_display);
    let resp = transport.exchange(&req).await?;
    decode_address(&resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::write_string;
    use crate::transport::MockTransport;

    fn ok_address_response(addr: &str) -> MessageEnvelope {
        let mut payload = Vec::new();
        write_string(&mut payload, 1, addr);
        MessageEnvelope {
            message_type: MessageType::Address.as_u16(),
            payload,
        }
    }

    #[test]
    fn builds_request_with_path_coin_and_script_type() {
        let path = Bip32Path::parse("m/84'/0'/0'/0/0").unwrap();
        let env = build_get_address(&path, "Bitcoin", ScriptType::SpendWitness, false);
        assert_eq!(env.message_type, MessageType::GetAddress.as_u16());
        let mut components = Vec::new();
        let mut coin = String::new();
        let mut script = None;
        let mut show = None;
        let mut r = WireReader::new(&env.payload);
        while let Some((tag, v)) = r.next_field().unwrap() {
            match (tag, v) {
                (1, FieldValue::Varint(c)) => components.push(c as u32),
                (2, FieldValue::Bytes(b)) => coin = String::from_utf8(b.to_vec()).unwrap(),
                (3, FieldValue::Varint(d)) => show = Some(d == 1),
                (5, FieldValue::Varint(s)) => script = Some(s as u32),
                _ => panic!("unexpected field"),
            }
        }
        assert_eq!(components, path.as_slice());
        assert_eq!(coin, "Bitcoin");
        assert_eq!(script, Some(ScriptType::SpendWitness as u32));
        assert!(show.is_none(), "show_display=false must omit the field");
    }

    #[test]
    fn decodes_address_response() {
        let r = ok_address_response("bc1q...");
        assert_eq!(decode_address(&r).unwrap(), "bc1q...");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn end_to_end_with_mock() {
        let t = MockTransport::new(vec![Ok(ok_address_response(
            "bc1qxyz0000000000000000000000000000000000",
        ))]);
        let path = Bip32Path::parse("m/84'/0'/0'/0/0").unwrap();
        let addr = get_address(&t, &path, "Bitcoin", ScriptType::SpendWitness, false)
            .await
            .unwrap();
        assert!(addr.starts_with("bc1q"));
        assert_eq!(t.history().len(), 1);
        assert_eq!(
            t.history()[0].message_type,
            MessageType::GetAddress.as_u16()
        );
    }
}
