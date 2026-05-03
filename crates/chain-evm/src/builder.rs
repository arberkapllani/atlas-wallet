//! EIP-1559 transaction builder + ECDSA signer.
//!
//! Hand-rolled RLP keeps the crate dependency-light. The only external
//! cryptographic primitives are `secp256k1` (signature) and `sha3` (keccak).

use sha3::{Digest, Keccak256};

/// Output of [`sign_eip1559`].
pub struct SignedEvmTx {
    /// `0x`-prefixed hex of the EIP-1559 envelope (`0x02 ‖ rlp([...])`).
    pub raw_hex: String,
    /// `0x`-prefixed transaction hash (keccak256 of the envelope).
    pub tx_hash: String,
}

/// Build, RLP-encode, and ECDSA-sign an EIP-1559 (type-2) transaction.
///
/// `data` is the optional call-data (empty for native transfers).
#[allow(clippy::too_many_arguments)]
pub fn sign_eip1559(
    chain_id: u64,
    nonce: u64,
    max_priority_fee_per_gas: u128,
    max_fee_per_gas: u128,
    gas_limit: u64,
    to: &str,
    value: u128,
    data: &[u8],
    private_key: &[u8; 32],
) -> Result<SignedEvmTx, String> {
    let to_bytes = parse_addr(to)?;

    // Pre-image fields (nine items + empty access list).
    let access_list: Vec<Vec<u8>> = Vec::new();
    let preimage_fields: Vec<RlpItem> = vec![
        RlpItem::Bytes(u64_to_be(chain_id)),
        RlpItem::Bytes(u64_to_be(nonce)),
        RlpItem::Bytes(u128_to_be(max_priority_fee_per_gas)),
        RlpItem::Bytes(u128_to_be(max_fee_per_gas)),
        RlpItem::Bytes(u64_to_be(gas_limit)),
        RlpItem::Bytes(to_bytes.to_vec()),
        RlpItem::Bytes(u128_to_be(value)),
        RlpItem::Bytes(data.to_vec()),
        RlpItem::List(access_list.into_iter().map(RlpItem::Bytes).collect()),
    ];

    let mut preimage = vec![0x02u8];
    preimage.extend_from_slice(&rlp_encode(&RlpItem::List(preimage_fields.clone())));

    let digest = Keccak256::digest(&preimage);

    let secp = secp256k1::Secp256k1::new();
    let sk = secp256k1::SecretKey::from_slice(private_key).map_err(|e| e.to_string())?;
    let msg = secp256k1::Message::from_digest_slice(&digest).map_err(|e| e.to_string())?;
    let recoverable = secp.sign_ecdsa_recoverable(&msg, &sk);
    let (rec_id, sig_bytes) = recoverable.serialize_compact();
    let r = sig_bytes[..32].to_vec();
    let s = sig_bytes[32..].to_vec();
    let y_parity: u8 = rec_id.to_i32() as u8; // 0 or 1

    // Final fields = preimage_fields + [y_parity, r, s]
    let mut final_fields = preimage_fields;
    final_fields.push(RlpItem::Bytes(if y_parity == 0 {
        Vec::new()
    } else {
        vec![y_parity]
    }));
    final_fields.push(RlpItem::Bytes(strip_leading_zeros(&r)));
    final_fields.push(RlpItem::Bytes(strip_leading_zeros(&s)));

    let mut signed = vec![0x02u8];
    signed.extend_from_slice(&rlp_encode(&RlpItem::List(final_fields)));

    let tx_hash = Keccak256::digest(&signed);

    Ok(SignedEvmTx {
        raw_hex: format!("0x{}", hex::encode(&signed)),
        tx_hash: format!("0x{}", hex::encode(tx_hash)),
    })
}

// ---------- minimal RLP ----------

#[derive(Clone)]
enum RlpItem {
    Bytes(Vec<u8>),
    List(Vec<RlpItem>),
}

fn rlp_encode(item: &RlpItem) -> Vec<u8> {
    match item {
        RlpItem::Bytes(b) => encode_bytes(b),
        RlpItem::List(items) => {
            let mut payload = Vec::new();
            for it in items {
                payload.extend_from_slice(&rlp_encode(it));
            }
            encode_length(payload.len(), 0xc0)
                .into_iter()
                .chain(payload)
                .collect()
        }
    }
}

fn encode_bytes(b: &[u8]) -> Vec<u8> {
    if b.len() == 1 && b[0] < 0x80 {
        return vec![b[0]];
    }
    let mut out = encode_length(b.len(), 0x80);
    out.extend_from_slice(b);
    out
}

fn encode_length(len: usize, offset: u8) -> Vec<u8> {
    if len < 56 {
        vec![offset + len as u8]
    } else {
        let len_bytes = strip_leading_zeros(&u64_to_be(len as u64));
        let mut out = vec![offset + 55 + len_bytes.len() as u8];
        out.extend_from_slice(&len_bytes);
        out
    }
}

fn u64_to_be(v: u64) -> Vec<u8> {
    strip_leading_zeros(&v.to_be_bytes())
}

fn u128_to_be(v: u128) -> Vec<u8> {
    strip_leading_zeros(&v.to_be_bytes())
}

fn strip_leading_zeros(b: &[u8]) -> Vec<u8> {
    let first = b.iter().position(|&x| x != 0).unwrap_or(b.len());
    b[first..].to_vec()
}

fn parse_addr(addr: &str) -> Result<[u8; 20], String> {
    let s = addr.strip_prefix("0x").unwrap_or(addr);
    let bytes = hex::decode(s).map_err(|e| e.to_string())?;
    if bytes.len() != 20 {
        return Err(format!("expected 20-byte address, got {}", bytes.len()));
    }
    let mut out = [0u8; 20];
    out.copy_from_slice(&bytes);
    Ok(out)
}
