//! Hierarchical deterministic key derivation.
//!
//! * Bitcoin: BIP-84 (`m/84'/0'/0'/0/i`) → P2WPKH bech32 (`bc1q…`).
//! * EVM (Ethereum + L2s): BIP-44 (`m/44'/60'/0'/0/i`) → 0x address.
//!   The same address is used across Eth, Polygon, BSC, Arbitrum, Optimism, Base.

use crate::error::{Error, Result};
use crate::mnemonic::Mnemonic;
use bitcoin::bip32::{DerivationPath, Xpriv};
use bitcoin::Network;
use sha3::{Digest, Keccak256};
use std::str::FromStr;
use zeroize::ZeroizeOnDrop;

/// Which chain family an account belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainKind {
    /// Bitcoin mainnet, BIP-84 native SegWit.
    Bitcoin,
    /// Ethereum & every EVM-compatible L1/L2 (same key, different network).
    Evm,
}

/// A derived single-account key. Holds raw bytes; zeroized on drop.
#[derive(ZeroizeOnDrop)]
pub struct AccountKey {
    /// 32-byte secp256k1 private key.
    private_key: [u8; 32],
    /// 33-byte compressed public key.
    #[zeroize(skip)]
    public_key: [u8; 33],
    /// Chain-appropriate textual address (bech32 for BTC, EIP-55 hex for EVM).
    #[zeroize(skip)]
    address: String,
    #[zeroize(skip)]
    kind: ChainKind,
}

impl AccountKey {
    /// secp256k1 private scalar (32 bytes). Treat as highly sensitive.
    pub fn private_key(&self) -> &[u8; 32] {
        &self.private_key
    }

    /// Compressed secp256k1 public key (33 bytes).
    pub fn public_key(&self) -> &[u8; 33] {
        &self.public_key
    }

    /// The textual address representation (bech32 or 0x…).
    pub fn address(&self) -> &str {
        &self.address
    }

    /// Which chain this key was derived for.
    pub fn kind(&self) -> ChainKind {
        self.kind
    }
}

impl std::fmt::Debug for AccountKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountKey")
            .field("kind", &self.kind)
            .field("address", &self.address)
            .field("private_key", &"***redacted***")
            .finish()
    }
}

/// Derive an account key for the given chain at the given index.
///
/// `index` corresponds to the BIP-44/84 address index (`…/0/i`). The vast
/// majority of users only need `index = 0`.
pub fn derive_account(mnemonic: &Mnemonic, kind: ChainKind, index: u32) -> Result<AccountKey> {
    let xpriv = Xpriv::new_master(Network::Bitcoin, mnemonic.seed())
        .map_err(|e| Error::Derivation(e.to_string()))?;
    let path_str = match kind {
        ChainKind::Bitcoin => format!("m/84'/0'/0'/0/{index}"),
        ChainKind::Evm => format!("m/44'/60'/0'/0/{index}"),
    };
    let path = DerivationPath::from_str(&path_str).map_err(|e| Error::Derivation(e.to_string()))?;
    let secp = secp256k1::Secp256k1::new();
    let child = xpriv
        .derive_priv(&secp, &path)
        .map_err(|e| Error::Derivation(e.to_string()))?;

    let mut private_key = [0u8; 32];
    private_key.copy_from_slice(&child.private_key.secret_bytes());
    let public_key = child.private_key.public_key(&secp).serialize();

    let address = match kind {
        ChainKind::Bitcoin => bitcoin_address(&public_key)?,
        ChainKind::Evm => evm_address(&child.private_key.public_key(&secp))?,
    };

    Ok(AccountKey {
        private_key,
        public_key,
        address,
        kind,
    })
}

fn bitcoin_address(pubkey_bytes: &[u8; 33]) -> Result<String> {
    let pk = bitcoin::PublicKey::from_slice(pubkey_bytes)
        .map_err(|e| Error::Derivation(e.to_string()))?;
    let cpk =
        bitcoin::CompressedPublicKey::try_from(pk).map_err(|e| Error::Derivation(e.to_string()))?;
    let addr = bitcoin::Address::p2wpkh(&cpk, Network::Bitcoin);
    Ok(addr.to_string())
}

fn evm_address(pk: &secp256k1::PublicKey) -> Result<String> {
    // Ethereum addresses are derived from the *uncompressed* pubkey
    // minus the 0x04 prefix (so 64 bytes), keccak256, last 20 bytes.
    let uncompressed = pk.serialize_uncompressed();
    debug_assert_eq!(uncompressed[0], 0x04);
    let hash = Keccak256::digest(&uncompressed[1..]);
    let raw_addr = &hash[12..]; // last 20 bytes
    Ok(to_eip55(raw_addr))
}

/// EIP-55 mixed-case checksum encoding of an Ethereum address.
fn to_eip55(addr: &[u8]) -> String {
    let lower_hex = hex::encode(addr);
    let hash = Keccak256::digest(lower_hex.as_bytes());
    let mut out = String::with_capacity(42);
    out.push_str("0x");
    for (i, ch) in lower_hex.chars().enumerate() {
        if ch.is_ascii_alphabetic() {
            // each hex digit corresponds to one nibble of the hash
            let nibble = (hash[i / 2] >> (4 * (1 - (i % 2)))) & 0xf;
            if nibble >= 8 {
                out.push(ch.to_ascii_uppercase());
            } else {
                out.push(ch);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mnemonic::Mnemonic;

    /// Trezor / iancoleman test vector for the canonical 12-word "abandon × 11 about" mnemonic.
    /// BIP-44, m/44'/60'/0'/0/0 → 0x9858EfFD232B4033E47d90003D41EC34EcaEda94.
    #[test]
    fn evm_first_account_matches_known_vector() {
        let phrase =
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let m = Mnemonic::from_phrase(phrase, "").unwrap();
        let acct = derive_account(&m, ChainKind::Evm, 0).unwrap();
        assert_eq!(acct.address(), "0x9858EfFD232B4033E47d90003D41EC34EcaEda94");
    }

    /// BIP-84 first receive address for the same mnemonic.
    /// m/84'/0'/0'/0/0 → bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu
    #[test]
    fn btc_first_account_matches_known_vector() {
        let phrase =
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let m = Mnemonic::from_phrase(phrase, "").unwrap();
        let acct = derive_account(&m, ChainKind::Bitcoin, 0).unwrap();
        assert_eq!(acct.address(), "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu");
    }
}
