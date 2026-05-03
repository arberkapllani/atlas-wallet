//! BIP-39 mnemonic generation and import.

use crate::error::{Error, Result};
use bip39::{Language, Mnemonic as Bip39Mnemonic};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Number of words in a mnemonic. Exodus 2 supports the two canonical sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MnemonicLength {
    /// 12 words → 128 bits of entropy. Default for new wallets.
    Words12,
    /// 24 words → 256 bits of entropy.
    Words24,
}

impl MnemonicLength {
    fn entropy_bytes(self) -> usize {
        match self {
            MnemonicLength::Words12 => 16,
            MnemonicLength::Words24 => 32,
        }
    }
}

/// A validated BIP-39 mnemonic. Wraps the canonical phrase in a zeroizing buffer
/// so the bytes are wiped from memory when the value is dropped.
#[derive(ZeroizeOnDrop)]
pub struct Mnemonic {
    phrase: String,
    /// 64-byte BIP-39 seed (PBKDF2-HMAC-SHA512 of phrase || passphrase).
    /// Re-derived lazily; cached for the lifetime of the value.
    seed: [u8; 64],
}

impl Mnemonic {
    /// Generate a fresh mnemonic using the OS CSPRNG.
    pub fn generate(length: MnemonicLength) -> Result<Self> {
        let mut entropy = vec![0u8; length.entropy_bytes()];
        getrandom::getrandom(&mut entropy)?;
        let bip = Bip39Mnemonic::from_entropy_in(Language::English, &entropy)
            .map_err(|e| Error::InvalidMnemonic(e.to_string()))?;
        entropy.zeroize();
        Ok(Self::from_bip39(bip, ""))
    }

    /// Parse a user-supplied phrase.  Whitespace is normalized.
    /// `passphrase` is the optional BIP-39 25th-word; pass `""` for none.
    pub fn from_phrase(phrase: &str, passphrase: &str) -> Result<Self> {
        let normalized = phrase.split_whitespace().collect::<Vec<_>>().join(" ");
        let bip = Bip39Mnemonic::parse_in(Language::English, &normalized)
            .map_err(|e| Error::InvalidMnemonic(e.to_string()))?;
        Ok(Self::from_bip39(bip, passphrase))
    }

    fn from_bip39(bip: Bip39Mnemonic, passphrase: &str) -> Self {
        let seed = bip.to_seed(passphrase);
        Self {
            phrase: bip.to_string(),
            seed,
        }
    }

    /// The validated phrase. Treat as highly sensitive.
    pub fn phrase(&self) -> &str {
        &self.phrase
    }

    /// 64-byte BIP-39 seed used for BIP-32 derivation.
    pub fn seed(&self) -> &[u8; 64] {
        &self.seed
    }

    /// Number of words (12 or 24).
    pub fn word_count(&self) -> usize {
        self.phrase.split_whitespace().count()
    }
}

impl std::fmt::Debug for Mnemonic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mnemonic")
            .field("word_count", &self.word_count())
            .field("phrase", &"***redacted***")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_12_words() {
        let m = Mnemonic::generate(MnemonicLength::Words12).unwrap();
        assert_eq!(m.word_count(), 12);
        assert_eq!(m.seed().len(), 64);
    }

    #[test]
    fn generates_24_words() {
        let m = Mnemonic::generate(MnemonicLength::Words24).unwrap();
        assert_eq!(m.word_count(), 24);
    }

    #[test]
    fn rejects_invalid_phrase() {
        let r = Mnemonic::from_phrase("not a real mnemonic phrase at all here please", "");
        assert!(r.is_err());
    }

    /// Canonical BIP-39 test vector (Trezor) — verifies our seed derivation.
    #[test]
    fn known_vector_seed() {
        let phrase =
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let m = Mnemonic::from_phrase(phrase, "TREZOR").unwrap();
        let expected = "c55257c360c07c72029aebc1b53c05ed0362ada38ead3e3e9efa3708e53495531f09a6987599d18264c1e1c92f2cf141630c7a3c4ab7c81b2f001698e7463b04";
        assert_eq!(hex::encode(m.seed()), expected);
    }
}
