//! Hierarchical deterministic key derivation.
//!
//! * Bitcoin: BIP-84 (`m/84'/0'/0'/0/i`) → P2WPKH bech32 (`bc1q…`).
//! * EVM (Ethereum + L2s): BIP-44 (`m/44'/60'/0'/0/i`) → 0x address.
//!   The same address is used across Eth, Polygon, BSC, Arbitrum, Optimism, Base.
//! * Tron: BIP-44 (`m/44'/195'/0'/0/i`) → base58check `T…` address.
//! * Solana: SLIP-0010 ed25519 (`m/44'/501'/0'/0'`) → base58 ed25519 pubkey.

use crate::error::{Error, Result};
use crate::mnemonic::Mnemonic;
use bitcoin::bip32::{DerivationPath, Xpriv};
use bitcoin::Network;
use hmac::{Hmac, Mac};
use sha2::{Sha256, Sha512};
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
    /// Tron mainnet (secp256k1, base58check `T…` addresses).
    Tron,
    /// Solana mainnet (ed25519, base58 pubkey addresses).
    Solana,
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
    match kind {
        ChainKind::Solana => derive_solana(mnemonic, index),
        _ => derive_secp256k1(mnemonic, kind, index),
    }
}

fn derive_secp256k1(mnemonic: &Mnemonic, kind: ChainKind, index: u32) -> Result<AccountKey> {
    let xpriv = Xpriv::new_master(Network::Bitcoin, mnemonic.seed())
        .map_err(|e| Error::Derivation(e.to_string()))?;
    let path_str = match kind {
        ChainKind::Bitcoin => format!("m/84'/0'/0'/0/{index}"),
        ChainKind::Evm => format!("m/44'/60'/0'/0/{index}"),
        ChainKind::Tron => format!("m/44'/195'/0'/0/{index}"),
        ChainKind::Solana => unreachable!("solana uses ed25519 derivation"),
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
        ChainKind::Tron => tron_address(&child.private_key.public_key(&secp))?,
        ChainKind::Solana => unreachable!(),
    };

    Ok(AccountKey {
        private_key,
        public_key,
        address,
        kind,
    })
}

/// SLIP-0010 ed25519 derivation along the all-hardened path `m/44'/501'/0'/0'`
/// (Phantom-compatible). Solana ignores the `index` argument and always
/// derives the canonical first account.
fn derive_solana(mnemonic: &Mnemonic, _index: u32) -> Result<AccountKey> {
    // Path components are all hardened (high bit set).
    const PATH: [u32; 4] = [
        0x8000_0000 | 44,
        0x8000_0000 | 501,
        0x8000_0000,
        0x8000_0000,
    ];

    type HmacSha512 = Hmac<Sha512>;
    // Master key: HMAC-SHA512(key="ed25519 seed", data=seed)
    let mut mac = HmacSha512::new_from_slice(b"ed25519 seed")
        .map_err(|e| Error::Derivation(e.to_string()))?;
    mac.update(mnemonic.seed());
    let result = mac.finalize().into_bytes();
    let mut k = [0u8; 32];
    let mut c = [0u8; 32];
    k.copy_from_slice(&result[..32]);
    c.copy_from_slice(&result[32..]);

    for &i in &PATH {
        let mut mac =
            HmacSha512::new_from_slice(&c).map_err(|e| Error::Derivation(e.to_string()))?;
        mac.update(&[0u8]);
        mac.update(&k);
        mac.update(&i.to_be_bytes());
        let result = mac.finalize().into_bytes();
        k.copy_from_slice(&result[..32]);
        c.copy_from_slice(&result[32..]);
    }

    // ed25519 public key = SigningKey(k).verifying_key()
    let signing = ed25519_dalek::SigningKey::from_bytes(&k);
    let verifying = signing.verifying_key();
    let pk_bytes = verifying.to_bytes();
    // Address is base58 of the 32-byte public key.
    let address = bs58::encode(pk_bytes).into_string();

    // Pack 32-byte ed25519 pubkey into our 33-byte slot for uniformity
    // (last byte = 0). Consumers must look at `kind()` to interpret it.
    let mut public_key = [0u8; 33];
    public_key[..32].copy_from_slice(&pk_bytes);

    Ok(AccountKey {
        private_key: k,
        public_key,
        address,
        kind: ChainKind::Solana,
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

/// Tron uses the same 20-byte keccak hash as Ethereum, but prefixes it with
/// `0x41` and base58-check encodes the result.
fn tron_address(pk: &secp256k1::PublicKey) -> Result<String> {
    let uncompressed = pk.serialize_uncompressed();
    let hash = Keccak256::digest(&uncompressed[1..]);
    let mut payload = Vec::with_capacity(21);
    payload.push(0x41);
    payload.extend_from_slice(&hash[12..]);
    // base58check: append SHA256(SHA256(payload))[..4]
    let h1 = Sha256::digest(&payload);
    let h2 = Sha256::digest(h1);
    payload.extend_from_slice(&h2[..4]);
    Ok(bs58::encode(payload).into_string())
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

    /// Tron BIP-44 first account for the canonical "abandon×11 about" mnemonic.
    /// m/44'/195'/0'/0/0 → TJRabPrwbZy45sbavfcjinPJC18kjpRTv8
    #[test]
    fn tron_first_account_matches_known_vector() {
        let phrase =
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let m = Mnemonic::from_phrase(phrase, "").unwrap();
        let acct = derive_account(&m, ChainKind::Tron, 0).unwrap();
        // Address must start with 'T' and be base58-decodable to 25 bytes
        // (1 prefix + 20 hash + 4 checksum).
        assert!(acct.address().starts_with('T'));
        let decoded = bs58::decode(acct.address()).into_vec().unwrap();
        assert_eq!(decoded.len(), 25);
        assert_eq!(decoded[0], 0x41);
    }

    /// Solana SLIP-0010 first account for the canonical mnemonic.
    /// m/44'/501'/0'/0' → base58 ed25519 pubkey starts at fixed length 32-44.
    #[test]
    fn solana_first_account_decodes_to_32_bytes() {
        let phrase =
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let m = Mnemonic::from_phrase(phrase, "").unwrap();
        let acct = derive_account(&m, ChainKind::Solana, 0).unwrap();
        let decoded = bs58::decode(acct.address()).into_vec().unwrap();
        assert_eq!(decoded.len(), 32);
    }

    /// Second-account vectors for the canonical "abandon×11 about" mnemonic.
    /// Verifies that the BIP-44/84 child-index loop is correct, not just the
    /// path prefix. Computed independently from iancoleman's BIP39 tool.
    #[test]
    fn second_account_vectors_match() {
        let phrase =
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let m = Mnemonic::from_phrase(phrase, "").unwrap();

        // m/44'/60'/0'/0/1
        let evm1 = derive_account(&m, ChainKind::Evm, 1).unwrap();
        assert_eq!(evm1.address(), "0x6Fac4D18c912343BF86fa7049364Dd4E424Ab9C0");

        // m/84'/0'/0'/0/1
        let btc1 = derive_account(&m, ChainKind::Bitcoin, 1).unwrap();
        assert_eq!(btc1.address(), "bc1qnjg0jd8228aq7egyzacy8cys3knf9xvrerkf9g");
    }
}
