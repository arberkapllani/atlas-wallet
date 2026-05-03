//! Solana legacy transaction encoder + ed25519 signer.
//!
//! Hand-rolled to keep dependencies tiny. Format (legacy, not v0):
//!
//! ```text
//! Transaction = signatures(short_vec<[u8;64]>) || Message
//! Message     = header(3 bytes)
//!            || account_keys(short_vec<[u8;32]>)
//!            || recent_blockhash([u8;32])
//!            || instructions(short_vec<CompiledInstruction>)
//! CompiledInstruction
//!            = program_id_index(u8)
//!            || accounts(short_vec<u8>)
//!            || data(short_vec<u8>)
//! ```

use crate::compact;

/// 32-byte ed25519 public key. Used both for account keys and program ids.
pub type Pubkey = [u8; 32];

/// The Solana System Program id (all-zero pubkey, `11111…111`).
pub const SYSTEM_PROGRAM_ID: Pubkey = [0u8; 32];

/// Build the instruction data for `SystemInstruction::Transfer { lamports }`.
///
/// Layout: `u32 LE = 2` (variant discriminator) followed by `u64 LE = lamports`.
pub fn system_transfer_data(lamports: u64) -> [u8; 12] {
    let mut data = [0u8; 12];
    data[..4].copy_from_slice(&2u32.to_le_bytes());
    data[4..].copy_from_slice(&lamports.to_le_bytes());
    data
}

/// Serialize the message body that the signer will sign.
///
/// Account ordering for a 1-signer SOL transfer:
///   `[0]` = sender   (signer, writable)
///   `[1]` = receiver (writable, no signer)
///   `[2]` = system program (readonly, no signer)
pub fn encode_transfer_message(
    from: &Pubkey,
    to: &Pubkey,
    lamports: u64,
    recent_blockhash: &[u8; 32],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(160);
    // Header: 1 required signature, 0 readonly-signed, 1 readonly-unsigned.
    out.extend_from_slice(&[1u8, 0u8, 1u8]);
    // Account keys (short_vec of pubkeys).
    out.extend_from_slice(&compact::encode(3));
    out.extend_from_slice(from);
    out.extend_from_slice(to);
    out.extend_from_slice(&SYSTEM_PROGRAM_ID);
    // Recent blockhash.
    out.extend_from_slice(recent_blockhash);
    // Instructions (short_vec of CompiledInstruction).
    out.extend_from_slice(&compact::encode(1));
    // Single CompiledInstruction:
    //   program_id_index = 2 (system program)
    out.push(2u8);
    //   accounts = [0, 1]
    out.extend_from_slice(&compact::encode(2));
    out.extend_from_slice(&[0u8, 1u8]);
    //   data = SystemInstruction::Transfer
    let data = system_transfer_data(lamports);
    out.extend_from_slice(&compact::encode(data.len() as u16));
    out.extend_from_slice(&data);
    out
}

/// Output of [`sign_transfer`].
pub struct SignedTransfer {
    /// Full Solana transaction bytes (signatures || message).
    pub raw: Vec<u8>,
    /// Base58-encoded signature, also serves as Solana's txid.
    pub signature_b58: String,
}

/// Sign a single-instruction `system::transfer` and produce the wire bytes.
pub fn sign_transfer(
    secret_key: &[u8; 32],
    to: &Pubkey,
    lamports: u64,
    recent_blockhash: &[u8; 32],
) -> SignedTransfer {
    let sk = ed25519_dalek::SigningKey::from_bytes(secret_key);
    let from: Pubkey = sk.verifying_key().to_bytes();
    let message = encode_transfer_message(&from, to, lamports, recent_blockhash);
    // ed25519 signature over the raw message bytes (no extra hashing).
    let sig: ed25519_dalek::Signature = ed25519_dalek::Signer::sign(&sk, &message);
    let sig_bytes = sig.to_bytes();

    let mut raw = Vec::with_capacity(1 + 64 + message.len());
    raw.extend_from_slice(&compact::encode(1));
    raw.extend_from_slice(&sig_bytes);
    raw.extend_from_slice(&message);

    SignedTransfer {
        raw,
        signature_b58: bs58::encode(sig_bytes).into_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, Verifier};

    #[test]
    fn transfer_data_matches_known_layout() {
        let d = system_transfer_data(1_000);
        assert_eq!(&d[..4], &[2, 0, 0, 0]);
        assert_eq!(&d[4..], &1_000u64.to_le_bytes());
    }

    #[test]
    fn message_layout_is_stable() {
        let from = [0x11u8; 32];
        let to = [0x22u8; 32];
        let bh = [0x33u8; 32];
        let msg = encode_transfer_message(&from, &to, 42, &bh);
        // header(3) + len(1) + 3*pubkey(96) + blockhash(32) + ix-len(1)
        // + program_idx(1) + accts-len(1) + accts(2) + data-len(1) + data(12)
        assert_eq!(msg.len(), 3 + 1 + 96 + 32 + 1 + 1 + 1 + 2 + 1 + 12);
        // Header byte 0: required signatures = 1.
        assert_eq!(msg[0], 1);
        // Program-id index byte sits right after the blockhash + ix-count.
        assert_eq!(msg[3 + 1 + 96 + 32 + 1], 2);
    }

    #[test]
    fn signature_verifies_against_derived_pubkey() {
        // Deterministic seed so the test is reproducible.
        let seed = [7u8; 32];
        let sk = ed25519_dalek::SigningKey::from_bytes(&seed);
        let pk = sk.verifying_key();
        let to = [0x55u8; 32];
        let bh = [0xaau8; 32];
        let signed = sign_transfer(&seed, &to, 1_000_000, &bh);
        // Re-encode the message and verify the embedded signature.
        let msg = encode_transfer_message(&pk.to_bytes(), &to, 1_000_000, &bh);
        let sig_bytes: [u8; 64] = signed.raw[1..1 + 64].try_into().unwrap();
        let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes);
        assert!(pk.verify(&msg, &sig).is_ok());
        // Rebuild the signature with the sk directly and ensure it matches.
        let sig2: ed25519_dalek::Signature = sk.sign(&msg);
        assert_eq!(sig.to_bytes(), sig2.to_bytes());
    }
}
