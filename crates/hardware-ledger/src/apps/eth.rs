//! Helpers for the Ledger **Ethereum** app.
//!
//! References:
//! - <https://github.com/LedgerHQ/app-ethereum/blob/develop/doc/ethapp.adoc>

use crate::apdu::{ApduCommand, ApduResponse, StatusWord};
use crate::path::Bip32Path;
use crate::transport::{Transport, TransportError};
use thiserror::Error;

/// Ledger Ethereum app CLA.
pub const CLA: u8 = 0xE0;

/// `GET_PUBLIC_KEY`.
pub const INS_GET_PUBLIC_KEY: u8 = 0x02;
/// `SIGN_TRANSACTION`.
pub const INS_SIGN_TX: u8 = 0x04;
/// `SIGN_PERSONAL_MESSAGE`.
pub const INS_SIGN_PERSONAL: u8 = 0x08;

/// Decoded `GET_PUBLIC_KEY` response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EthPublicKey {
    /// Uncompressed secp256k1 public key (65 bytes, leading
    /// `0x04`).
    pub pubkey: Vec<u8>,
    /// Checksum-cased ASCII address as the device returned it (no
    /// `0x` prefix).
    pub address_ascii: String,
}

/// High-level errors returned by this module.
#[derive(Debug, Error)]
pub enum EthError {
    #[error("transport error: {0}")]
    Transport(#[from] TransportError),
    #[error("device returned status {0:#06x}")]
    Device(u16),
    #[error("user rejected on-device")]
    UserRejected,
    #[error("device locked")]
    Locked,
    #[error("malformed response: {0}")]
    Malformed(String),
    #[error("apdu builder error: payload too large")]
    PayloadTooLarge,
}

fn map_status(sw: StatusWord) -> Result<(), EthError> {
    match sw {
        StatusWord::Ok => Ok(()),
        StatusWord::UserRejected => Err(EthError::UserRejected),
        StatusWord::Locked => Err(EthError::Locked),
        other => Err(EthError::Device(other.as_u16())),
    }
}

/// Build the `GET_PUBLIC_KEY` APDU.
///
/// `display` asks the device to show the address on screen for
/// confirmation; this is the standard ceremony for first-time
/// account import.
pub fn build_get_public_key(path: &Bip32Path, display: bool) -> Result<ApduCommand, EthError> {
    let p1 = if display { 0x01 } else { 0x00 };
    ApduCommand::new(CLA, INS_GET_PUBLIC_KEY, p1, 0x00, path.encode())
        .ok_or(EthError::PayloadTooLarge)
}

/// Decode the response to `GET_PUBLIC_KEY`. Layout:
///
/// ```text
/// [pubkey_len:1][pubkey:65][addr_len:1][addr_ascii:N]
/// ```
///
/// (chaincode bytes are ignored if present)
pub fn decode_public_key(resp: &ApduResponse) -> Result<EthPublicKey, EthError> {
    map_status(resp.sw)?;
    let data = &resp.data;
    if data.is_empty() {
        return Err(EthError::Malformed("empty payload".into()));
    }
    let pk_len = data[0] as usize;
    if pk_len == 0 || pk_len > 65 || data.len() < 1 + pk_len + 1 {
        return Err(EthError::Malformed(
            "pubkey length / payload truncated".into(),
        ));
    }
    let pubkey = data[1..1 + pk_len].to_vec();
    let addr_len = data[1 + pk_len] as usize;
    let start = 1 + pk_len + 1;
    if data.len() < start + addr_len {
        return Err(EthError::Malformed("address truncated".into()));
    }
    let addr_bytes = &data[start..start + addr_len];
    let address_ascii = std::str::from_utf8(addr_bytes)
        .map_err(|_| EthError::Malformed("address not utf8".into()))?
        .to_string();
    Ok(EthPublicKey {
        pubkey,
        address_ascii,
    })
}

/// Convenience: send `GET_PUBLIC_KEY` and decode the response.
pub async fn get_public_key<T: Transport + ?Sized>(
    transport: &T,
    path: &Bip32Path,
    display: bool,
) -> Result<EthPublicKey, EthError> {
    let cmd = build_get_public_key(path, display)?;
    let resp = transport.exchange(&cmd).await?;
    decode_public_key(&resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn sample_pubkey_response() -> ApduResponse {
        // 65-byte uncompressed pubkey + 40-byte ASCII address.
        let mut data = Vec::new();
        data.push(65);
        data.extend(std::iter::repeat_n(0x04u8, 65));
        data.push(40);
        data.extend_from_slice(b"a1b2c3d4e5f6071829303a4b5c6d7e8f90a1b2c3");
        ApduResponse {
            data,
            sw: StatusWord::Ok,
        }
    }

    #[test]
    fn builds_get_public_key_with_correct_header() {
        let path = Bip32Path::parse("m/44'/60'/0'/0/0").unwrap();
        let cmd = build_get_public_key(&path, false).unwrap();
        assert_eq!(cmd.cla, CLA);
        assert_eq!(cmd.ins, INS_GET_PUBLIC_KEY);
        assert_eq!(cmd.p1, 0x00);
        assert_eq!(cmd.p2, 0x00);
        // 5 components × 4 bytes + 1 count byte
        assert_eq!(cmd.data.len(), 21);
        assert_eq!(cmd.data[0], 5);

        let cmd_display = build_get_public_key(&path, true).unwrap();
        assert_eq!(cmd_display.p1, 0x01);
    }

    #[test]
    fn decodes_well_formed_response() {
        let pk = decode_public_key(&sample_pubkey_response()).unwrap();
        assert_eq!(pk.pubkey.len(), 65);
        assert_eq!(pk.address_ascii.len(), 40);
        assert!(pk.address_ascii.starts_with("a1b2c3"));
    }

    #[test]
    fn user_rejection_propagates() {
        let resp = ApduResponse {
            data: vec![],
            sw: StatusWord::UserRejected,
        };
        assert!(matches!(
            decode_public_key(&resp),
            Err(EthError::UserRejected)
        ));
    }

    #[test]
    fn malformed_response_errors() {
        // pk_len claims 65 but data is only 10 bytes
        let resp = ApduResponse {
            data: vec![65, 1, 2, 3, 4, 5, 6, 7, 8, 9],
            sw: StatusWord::Ok,
        };
        assert!(matches!(
            decode_public_key(&resp),
            Err(EthError::Malformed(_))
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn end_to_end_round_trip_with_mock() {
        let transport = MockTransport::new(vec![Ok(sample_pubkey_response())]);
        let path = Bip32Path::parse("m/44'/60'/0'/0/0").unwrap();
        let pk = get_public_key(&transport, &path, false).await.unwrap();
        assert_eq!(pk.address_ascii.len(), 40);
        let history = transport.history();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].ins, INS_GET_PUBLIC_KEY);
    }
}
