//! Helpers for the Ledger **Bitcoin** app (legacy v1.6+ APDUs).
//!
//! Reference:
//! - <https://github.com/LedgerHQ/app-bitcoin-new>
//!
//! Only `GET_WALLET_PUBLIC_KEY` is wired up for now. PSBT-based
//! signing is planned and will live behind its own helper.

use crate::apdu::{ApduCommand, ApduResponse, StatusWord};
use crate::path::Bip32Path;
use crate::transport::{Transport, TransportError};
use thiserror::Error;

/// Bitcoin app CLA.
pub const CLA: u8 = 0xE0;

/// `GET_WALLET_PUBLIC_KEY`.
pub const INS_GET_WALLET_PUBLIC_KEY: u8 = 0x40;

/// Decoded `GET_WALLET_PUBLIC_KEY` response. Address bytes are
/// returned in whichever format the on-device app derives for the
/// requested path (legacy / p2sh-segwit / native segwit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtcPublicKey {
    /// Uncompressed secp256k1 public key.
    pub pubkey: Vec<u8>,
    /// Address as ASCII (e.g. `"bc1q…"` for native segwit).
    pub address: String,
    /// 32-byte BIP-32 chain code.
    pub chain_code: [u8; 32],
}

#[derive(Debug, Error)]
pub enum BtcError {
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
    #[error("payload too large for short-form APDU")]
    PayloadTooLarge,
}

fn map_status(sw: StatusWord) -> Result<(), BtcError> {
    match sw {
        StatusWord::Ok => Ok(()),
        StatusWord::UserRejected => Err(BtcError::UserRejected),
        StatusWord::Locked => Err(BtcError::Locked),
        other => Err(BtcError::Device(other.as_u16())),
    }
}

/// Build the `GET_WALLET_PUBLIC_KEY` APDU.
///
/// `display` flips the "show on screen" mode (P1 = 0x01).
pub fn build_get_wallet_public_key(
    path: &Bip32Path,
    display: bool,
) -> Result<ApduCommand, BtcError> {
    let p1 = if display { 0x01 } else { 0x00 };
    ApduCommand::new(CLA, INS_GET_WALLET_PUBLIC_KEY, p1, 0x00, path.encode())
        .ok_or(BtcError::PayloadTooLarge)
}

/// Decode the response. Layout:
///
/// ```text
/// [pubkey_len:1][pubkey:N][addr_len:1][addr:M][chain_code:32]
/// ```
pub fn decode_wallet_public_key(resp: &ApduResponse) -> Result<BtcPublicKey, BtcError> {
    map_status(resp.sw)?;
    let data = &resp.data;
    if data.is_empty() {
        return Err(BtcError::Malformed("empty payload".into()));
    }
    let pk_len = data[0] as usize;
    if pk_len == 0 || pk_len > 65 || data.len() < 1 + pk_len + 1 {
        return Err(BtcError::Malformed("pubkey length truncated".into()));
    }
    let pubkey = data[1..1 + pk_len].to_vec();
    let addr_len = data[1 + pk_len] as usize;
    let addr_start = 1 + pk_len + 1;
    if data.len() < addr_start + addr_len + 32 {
        return Err(BtcError::Malformed("address / chain-code truncated".into()));
    }
    let addr_bytes = &data[addr_start..addr_start + addr_len];
    let address = std::str::from_utf8(addr_bytes)
        .map_err(|_| BtcError::Malformed("address not utf8".into()))?
        .to_string();
    let mut chain_code = [0u8; 32];
    chain_code.copy_from_slice(&data[addr_start + addr_len..addr_start + addr_len + 32]);
    Ok(BtcPublicKey {
        pubkey,
        address,
        chain_code,
    })
}

/// Convenience wrapper.
pub async fn get_wallet_public_key<T: Transport + ?Sized>(
    transport: &T,
    path: &Bip32Path,
    display: bool,
) -> Result<BtcPublicKey, BtcError> {
    let cmd = build_get_wallet_public_key(path, display)?;
    let resp = transport.exchange(&cmd).await?;
    decode_wallet_public_key(&resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn sample() -> ApduResponse {
        let pk = vec![0x04u8; 65];
        let addr = b"bc1qxyz0000000000000000000000000000000000";
        let chain_code = [0x42u8; 32];
        let mut data = Vec::new();
        data.push(pk.len() as u8);
        data.extend_from_slice(&pk);
        data.push(addr.len() as u8);
        data.extend_from_slice(addr);
        data.extend_from_slice(&chain_code);
        ApduResponse {
            data,
            sw: StatusWord::Ok,
        }
    }

    #[test]
    fn builds_request_with_count_prefixed_path() {
        let path = Bip32Path::parse("m/84'/0'/0'/0/0").unwrap();
        let cmd = build_get_wallet_public_key(&path, false).unwrap();
        assert_eq!(cmd.ins, INS_GET_WALLET_PUBLIC_KEY);
        assert_eq!(cmd.data[0], 5);
        assert_eq!(cmd.data.len(), 1 + 5 * 4);
    }

    #[test]
    fn decodes_pubkey_address_and_chain_code() {
        let pk = decode_wallet_public_key(&sample()).unwrap();
        assert_eq!(pk.pubkey.len(), 65);
        assert!(pk.address.starts_with("bc1q"));
        assert_eq!(pk.chain_code, [0x42u8; 32]);
    }

    #[test]
    fn malformed_truncated_response() {
        let resp = ApduResponse {
            data: vec![65, 1, 2, 3],
            sw: StatusWord::Ok,
        };
        assert!(matches!(
            decode_wallet_public_key(&resp),
            Err(BtcError::Malformed(_))
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn end_to_end_with_mock() {
        let t = MockTransport::new(vec![Ok(sample())]);
        let path = Bip32Path::parse("m/84'/0'/0'/0/0").unwrap();
        let pk = get_wallet_public_key(&t, &path, false).await.unwrap();
        assert!(pk.address.starts_with("bc1q"));
    }
}
