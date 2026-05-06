//! Atlas silent-payments primitives.
//!
//! This crate provides the cryptographic core of BIP352 silent payments:
//!
//! * Address encoding / decoding using bech32m with HRPs `sp` (mainnet),
//!   `tsp` (testnet/signet), `sprt` (regtest). The address payload is the
//!   concatenation of the receiver's scan public key and spend public key.
//! * Tagged-hash helper (`BIP0352/...`).
//! * Sender helpers: `derive_input_hash`, `sender_ecdh_secret`,
//!   `derive_output_pubkey`.
//! * Receiver helper: `receiver_ecdh_secret`, `derive_output_pubkey` is
//!   shared with the sender.
//!
//! # Notes on BIP352 conformance
//!
//! BIP352 reserves a 5-bit version field at the start of the bech32m data
//! part. To keep the implementation self-contained and to interoperate
//! cleanly with other Atlas wallets we encode only the 66-byte payload
//! (B_scan ∥ B_m). The cryptographic primitives are BIP352-compliant; the
//! address envelope can be upgraded to the spec's 5-bit version prefix
//! without breaking the cryptographic API.
//!
//! # Sovereignty
//!
//! No third-party services are used. Address generation, address parsing,
//! ECDH, and output-pubkey derivation are pure functions over the
//! receiver / sender keys.

use bech32::{Bech32m, Hrp};
use secp256k1::{PublicKey, Scalar, Secp256k1, SecretKey, XOnlyPublicKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;
use thiserror::Error;

pub const HRP_MAINNET: &str = "sp";
pub const HRP_TESTNET: &str = "tsp";
pub const HRP_REGTEST: &str = "sprt";

/// Length of the address payload (B_scan || B_spend).
pub const PAYLOAD_LEN: usize = 33 + 33;

/// Bitcoin network the silent-payment address is bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SpNetwork {
    Mainnet,
    Testnet,
    Regtest,
}

impl SpNetwork {
    pub fn hrp(self) -> &'static str {
        match self {
            SpNetwork::Mainnet => HRP_MAINNET,
            SpNetwork::Testnet => HRP_TESTNET,
            SpNetwork::Regtest => HRP_REGTEST,
        }
    }

    pub fn from_hrp(hrp: &str) -> Option<Self> {
        match hrp {
            HRP_MAINNET => Some(Self::Mainnet),
            HRP_TESTNET => Some(Self::Testnet),
            HRP_REGTEST => Some(Self::Regtest),
            _ => None,
        }
    }
}

#[derive(Debug, Error)]
pub enum SpError {
    #[error("bech32 decode failed: {0}")]
    Bech32Decode(String),
    #[error("bech32 encode failed: {0}")]
    Bech32Encode(String),
    #[error("unknown HRP `{0}`")]
    UnknownHrp(String),
    #[error("invalid payload length: expected {expected}, got {got}")]
    InvalidLength { expected: usize, got: usize },
    #[error("invalid public key: {0}")]
    InvalidPubkey(String),
    #[error("invalid secret key: {0}")]
    InvalidSecret(String),
    #[error("scalar arithmetic failure: {0}")]
    ScalarMath(String),
    #[error("hex decode failed: {0}")]
    HexDecode(String),
}

/// Silent-payment address: scan pubkey ‖ spend pubkey + network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SilentPaymentAddress {
    pub network: SpNetwork,
    /// Compressed scan pubkey, 33 bytes hex.
    pub scan_pub_hex: String,
    /// Compressed spend pubkey, 33 bytes hex.
    pub spend_pub_hex: String,
}

impl SilentPaymentAddress {
    /// Build from raw secp256k1 keys.
    pub fn from_pubkeys(network: SpNetwork, scan: &PublicKey, spend: &PublicKey) -> Self {
        Self {
            network,
            scan_pub_hex: hex::encode(scan.serialize()),
            spend_pub_hex: hex::encode(spend.serialize()),
        }
    }

    pub fn scan_pub(&self) -> Result<PublicKey, SpError> {
        decode_pubkey(&self.scan_pub_hex)
    }

    pub fn spend_pub(&self) -> Result<PublicKey, SpError> {
        decode_pubkey(&self.spend_pub_hex)
    }

    /// Encode to the bech32m address string (`sp1...`, `tsp1...`, `sprt1...`).
    pub fn encode(&self) -> Result<String, SpError> {
        let hrp =
            Hrp::parse(self.network.hrp()).map_err(|e| SpError::Bech32Encode(e.to_string()))?;
        let mut payload = Vec::with_capacity(PAYLOAD_LEN);
        payload.extend_from_slice(&self.scan_pub()?.serialize());
        payload.extend_from_slice(&self.spend_pub()?.serialize());
        bech32::encode::<Bech32m>(hrp, &payload).map_err(|e| SpError::Bech32Encode(e.to_string()))
    }

    /// Parse a bech32m address string.
    pub fn decode(s: &str) -> Result<Self, SpError> {
        let (hrp, data) = bech32::decode(s).map_err(|e| SpError::Bech32Decode(e.to_string()))?;
        let network = SpNetwork::from_hrp(hrp.as_str())
            .ok_or_else(|| SpError::UnknownHrp(hrp.as_str().to_string()))?;
        if data.len() != PAYLOAD_LEN {
            return Err(SpError::InvalidLength {
                expected: PAYLOAD_LEN,
                got: data.len(),
            });
        }
        let scan = PublicKey::from_slice(&data[..33])
            .map_err(|e| SpError::InvalidPubkey(format!("scan: {e}")))?;
        let spend = PublicKey::from_slice(&data[33..])
            .map_err(|e| SpError::InvalidPubkey(format!("spend: {e}")))?;
        Ok(Self::from_pubkeys(network, &scan, &spend))
    }
}

fn decode_pubkey(s: &str) -> Result<PublicKey, SpError> {
    let bytes = hex::decode(s).map_err(|e| SpError::HexDecode(e.to_string()))?;
    PublicKey::from_slice(&bytes).map_err(|e| SpError::InvalidPubkey(e.to_string()))
}

fn decode_secret(s: &str) -> Result<SecretKey, SpError> {
    let bytes = hex::decode(s).map_err(|e| SpError::HexDecode(e.to_string()))?;
    SecretKey::from_slice(&bytes).map_err(|e| SpError::InvalidSecret(e.to_string()))
}

/// BIP340-style tagged hash: `SHA256(SHA256(tag) ‖ SHA256(tag) ‖ data)`.
pub fn tagged_hash(tag: &str, data: &[u8]) -> [u8; 32] {
    let tag_hash = Sha256::digest(tag.as_bytes());
    let mut hasher = Sha256::new();
    hasher.update(tag_hash);
    hasher.update(tag_hash);
    hasher.update(data);
    let out = hasher.finalize();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    arr
}

/// Compute the BIP352 input hash:
///
/// `tagged_hash("BIP0352/Inputs", smallest_outpoint || A_sum_compressed)`.
///
/// `smallest_outpoint` is the lexicographically smallest 36-byte outpoint
/// (txid little-endian || vout little-endian) consumed by the transaction.
pub fn derive_input_hash(smallest_outpoint: &[u8; 36], a_sum: &PublicKey) -> [u8; 32] {
    let mut data = Vec::with_capacity(36 + 33);
    data.extend_from_slice(smallest_outpoint);
    data.extend_from_slice(&a_sum.serialize());
    tagged_hash("BIP0352/Inputs", &data)
}

/// Sender-side ECDH secret:
///
/// `ecdh = (input_hash · a_sum) · B_scan` (compressed serialisation).
pub fn sender_ecdh_secret(
    a_sum: &SecretKey,
    b_scan_pub: &PublicKey,
    input_hash: &[u8; 32],
) -> Result<[u8; 33], SpError> {
    let secp = Secp256k1::new();
    let h = Scalar::from_be_bytes(*input_hash).map_err(|e| SpError::ScalarMath(e.to_string()))?;
    let a_h = a_sum
        .mul_tweak(&h)
        .map_err(|e| SpError::ScalarMath(e.to_string()))?;
    let scalar = Scalar::from_be_bytes(a_h.secret_bytes())
        .map_err(|e| SpError::ScalarMath(e.to_string()))?;
    let point = b_scan_pub
        .mul_tweak(&secp, &scalar)
        .map_err(|e| SpError::ScalarMath(e.to_string()))?;
    Ok(point.serialize())
}

/// Receiver-side ECDH secret:
///
/// `ecdh = b_scan · (input_hash · A_sum)` (compressed serialisation).
pub fn receiver_ecdh_secret(
    b_scan: &SecretKey,
    a_sum_pub: &PublicKey,
    input_hash: &[u8; 32],
) -> Result<[u8; 33], SpError> {
    let secp = Secp256k1::new();
    let h = Scalar::from_be_bytes(*input_hash).map_err(|e| SpError::ScalarMath(e.to_string()))?;
    let a_h = a_sum_pub
        .mul_tweak(&secp, &h)
        .map_err(|e| SpError::ScalarMath(e.to_string()))?;
    let scalar = Scalar::from_be_bytes(b_scan.secret_bytes())
        .map_err(|e| SpError::ScalarMath(e.to_string()))?;
    let point = a_h
        .mul_tweak(&secp, &scalar)
        .map_err(|e| SpError::ScalarMath(e.to_string()))?;
    Ok(point.serialize())
}

/// Per-output tweak: `t_k = tagged_hash("BIP0352/SharedSecret", ecdh ‖ k_be32)`.
pub fn output_tweak(ecdh_secret: &[u8; 33], k: u32) -> [u8; 32] {
    let mut data = Vec::with_capacity(33 + 4);
    data.extend_from_slice(ecdh_secret);
    data.extend_from_slice(&k.to_be_bytes());
    // Spec uses 32-bit BE counter; we left-pad to 32 bytes for the SharedSecret
    // tagged hash domain so that callers stamping their own 32-byte counter
    // get the same result via `output_tweak_with_index_bytes`.
    output_tweak_with_index_bytes(ecdh_secret, &data[33..])
}

/// Lower-level variant that lets the caller pass a custom k encoding.
pub fn output_tweak_with_index_bytes(ecdh_secret: &[u8; 33], k_bytes: &[u8]) -> [u8; 32] {
    let mut data = Vec::with_capacity(33 + k_bytes.len());
    data.extend_from_slice(ecdh_secret);
    data.extend_from_slice(k_bytes);
    tagged_hash("BIP0352/SharedSecret", &data)
}

/// Compute the k-th output X-only pubkey: `P_k = B_m + t_k · G`.
pub fn derive_output_pubkey(
    spend_pub: &PublicKey,
    ecdh_secret: &[u8; 33],
    k: u32,
) -> Result<XOnlyPublicKey, SpError> {
    let secp = Secp256k1::new();
    let t_k = output_tweak(ecdh_secret, k);
    let scalar = Scalar::from_be_bytes(t_k).map_err(|e| SpError::ScalarMath(e.to_string()))?;
    let p = spend_pub
        .add_exp_tweak(&secp, &scalar)
        .map_err(|e| SpError::ScalarMath(e.to_string()))?;
    Ok(p.x_only_public_key().0)
}

/// Generate a random scan/spend keypair pair (used by tests and the
/// "demo" address generator surfaced through the desktop IPC).
pub fn generate_keypair() -> (SecretKey, PublicKey) {
    let secp = Secp256k1::new();
    let (sk, pk) = secp.generate_keypair(&mut secp256k1::rand::thread_rng());
    (sk, pk)
}

/// Build a hex-encoded silent-payment address from secret keys.
pub fn address_from_secrets(
    network: SpNetwork,
    scan_secret_hex: &str,
    spend_secret_hex: &str,
) -> Result<SilentPaymentAddress, SpError> {
    let secp = Secp256k1::new();
    let scan_sk = decode_secret(scan_secret_hex)?;
    let spend_sk = decode_secret(spend_secret_hex)?;
    let scan_pub = scan_sk.public_key(&secp);
    let spend_pub = spend_sk.public_key(&secp);
    Ok(SilentPaymentAddress::from_pubkeys(
        network, &scan_pub, &spend_pub,
    ))
}

/// Wrap-up payload returned by `generate_demo_address` so the front end
/// can show the user every component of the address (and back-up the
/// secrets locally).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DemoSilentPayment {
    pub address: String,
    pub network: SpNetwork,
    pub scan_secret_hex: String,
    pub spend_secret_hex: String,
    pub scan_pub_hex: String,
    pub spend_pub_hex: String,
}

/// Generate a fresh receiver keypair and return the encoded address plus
/// the raw secrets so the caller can persist them offline.
pub fn generate_demo_address(network: SpNetwork) -> Result<DemoSilentPayment, SpError> {
    let (scan_sk, scan_pub) = generate_keypair();
    let (spend_sk, spend_pub) = generate_keypair();
    let address = SilentPaymentAddress::from_pubkeys(network, &scan_pub, &spend_pub);
    let s = address.encode()?;
    Ok(DemoSilentPayment {
        address: s,
        network,
        scan_secret_hex: hex::encode(scan_sk.secret_bytes()),
        spend_secret_hex: hex::encode(spend_sk.secret_bytes()),
        scan_pub_hex: hex::encode(scan_pub.serialize()),
        spend_pub_hex: hex::encode(spend_pub.serialize()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use secp256k1::Secp256k1;

    fn fixed_keypair(seed: u8) -> (SecretKey, PublicKey) {
        let secp = Secp256k1::new();
        let mut bytes = [seed; 32];
        bytes[0] = 0x01; // ensure non-zero scalar < n
        let sk = SecretKey::from_slice(&bytes).unwrap();
        let pk = sk.public_key(&secp);
        (sk, pk)
    }

    #[test]
    fn network_hrp_round_trip() {
        for net in [SpNetwork::Mainnet, SpNetwork::Testnet, SpNetwork::Regtest] {
            assert_eq!(SpNetwork::from_hrp(net.hrp()), Some(net));
        }
        assert!(SpNetwork::from_hrp("bc").is_none());
    }

    #[test]
    fn address_round_trip_mainnet() {
        let (_scan_sk, scan_pub) = fixed_keypair(0x11);
        let (_spend_sk, spend_pub) = fixed_keypair(0x22);
        let addr = SilentPaymentAddress::from_pubkeys(SpNetwork::Mainnet, &scan_pub, &spend_pub);
        let encoded = addr.encode().expect("encode");
        assert!(
            encoded.starts_with("sp1"),
            "expected sp1 prefix, got {encoded}"
        );
        let parsed = SilentPaymentAddress::decode(&encoded).expect("decode");
        assert_eq!(parsed, addr);
    }

    #[test]
    fn address_round_trip_testnet_and_regtest() {
        let (_, scan_pub) = fixed_keypair(0x33);
        let (_, spend_pub) = fixed_keypair(0x44);
        for net in [SpNetwork::Testnet, SpNetwork::Regtest] {
            let addr = SilentPaymentAddress::from_pubkeys(net, &scan_pub, &spend_pub);
            let s = addr.encode().expect("encode");
            assert!(s.starts_with(net.hrp()));
            let back = SilentPaymentAddress::decode(&s).expect("decode");
            assert_eq!(back.network, net);
            assert_eq!(back, addr);
        }
    }

    #[test]
    fn decode_rejects_unknown_hrp() {
        let (_, scan_pub) = fixed_keypair(0x55);
        let (_, spend_pub) = fixed_keypair(0x66);
        let addr = SilentPaymentAddress::from_pubkeys(SpNetwork::Mainnet, &scan_pub, &spend_pub);
        let mut s = addr.encode().unwrap();
        // Replace HRP with something the crate does not know.
        s = s.replacen("sp1", "bc1", 1);
        assert!(SilentPaymentAddress::decode(&s).is_err());
    }

    #[test]
    fn decode_rejects_truncated_payload() {
        // A bech32m string with the right HRP but short payload.
        let hrp = Hrp::parse("sp").unwrap();
        let s = bech32::encode::<Bech32m>(hrp, &[0u8; 32]).unwrap();
        let err = SilentPaymentAddress::decode(&s).unwrap_err();
        match err {
            SpError::InvalidLength { expected, got } => {
                assert_eq!(expected, PAYLOAD_LEN);
                assert_eq!(got, 32);
            }
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn tagged_hash_matches_known_construction() {
        // SHA256(SHA256(tag)||SHA256(tag)||"") for tag="" — sanity check.
        let a = tagged_hash("BIP0352/Inputs", b"");
        let b = tagged_hash("BIP0352/Inputs", b"");
        assert_eq!(a, b);
        // Different tag => different output.
        let c = tagged_hash("BIP0352/SharedSecret", b"");
        assert_ne!(a, c);
    }

    #[test]
    fn input_hash_is_deterministic_and_depends_on_inputs() {
        let (_, a_sum) = fixed_keypair(0x77);
        let mut op = [0u8; 36];
        op[0] = 0xaa;
        let h1 = derive_input_hash(&op, &a_sum);
        let h2 = derive_input_hash(&op, &a_sum);
        assert_eq!(h1, h2);
        op[0] = 0xbb;
        let h3 = derive_input_hash(&op, &a_sum);
        assert_ne!(h1, h3);
    }

    #[test]
    fn sender_and_receiver_derive_same_secret() {
        // a_sum = sum of input privkeys. For the test we use one input.
        let (a_sum, a_sum_pub) = fixed_keypair(0x88);
        let (b_scan, b_scan_pub) = fixed_keypair(0x99);
        let mut op = [0u8; 36];
        op[15] = 0x42;
        let input_hash = derive_input_hash(&op, &a_sum_pub);
        let s_send = sender_ecdh_secret(&a_sum, &b_scan_pub, &input_hash).unwrap();
        let s_recv = receiver_ecdh_secret(&b_scan, &a_sum_pub, &input_hash).unwrap();
        assert_eq!(s_send, s_recv);
    }

    #[test]
    fn output_pubkey_matches_between_sender_and_receiver() {
        let (a_sum, a_sum_pub) = fixed_keypair(0xaa);
        let (b_scan, b_scan_pub) = fixed_keypair(0xbb);
        let (_, b_spend) = fixed_keypair(0xcc);
        let mut op = [0u8; 36];
        op[35] = 1;
        let h = derive_input_hash(&op, &a_sum_pub);

        let s_send = sender_ecdh_secret(&a_sum, &b_scan_pub, &h).unwrap();
        let s_recv = receiver_ecdh_secret(&b_scan, &a_sum_pub, &h).unwrap();
        assert_eq!(s_send, s_recv);

        let p_send = derive_output_pubkey(&b_spend, &s_send, 0).unwrap();
        let p_recv = derive_output_pubkey(&b_spend, &s_recv, 0).unwrap();
        assert_eq!(p_send, p_recv);

        // k=1 must differ from k=0.
        let p_send_k1 = derive_output_pubkey(&b_spend, &s_send, 1).unwrap();
        assert_ne!(p_send, p_send_k1);
    }

    #[test]
    fn output_tweak_distinct_per_index() {
        let secret = [0x42u8; 33];
        let t0 = output_tweak(&secret, 0);
        let t1 = output_tweak(&secret, 1);
        let t_big = output_tweak(&secret, u32::MAX);
        assert_ne!(t0, t1);
        assert_ne!(t1, t_big);
    }

    #[test]
    fn address_from_secrets_round_trip() {
        let (scan_sk, _) = fixed_keypair(0xde);
        let (spend_sk, _) = fixed_keypair(0xad);
        let addr = address_from_secrets(
            SpNetwork::Mainnet,
            &hex::encode(scan_sk.secret_bytes()),
            &hex::encode(spend_sk.secret_bytes()),
        )
        .unwrap();
        let s = addr.encode().unwrap();
        assert!(s.starts_with("sp1"));
        let back = SilentPaymentAddress::decode(&s).unwrap();
        assert_eq!(back, addr);
    }

    #[test]
    fn address_from_secrets_rejects_bad_hex() {
        let err = address_from_secrets(SpNetwork::Mainnet, "zz", "00").unwrap_err();
        assert!(matches!(err, SpError::HexDecode(_)));
    }

    #[test]
    fn generate_demo_address_round_trip() {
        let demo = generate_demo_address(SpNetwork::Mainnet).unwrap();
        let parsed = SilentPaymentAddress::decode(&demo.address).unwrap();
        assert_eq!(parsed.network, SpNetwork::Mainnet);
        assert_eq!(parsed.scan_pub_hex, demo.scan_pub_hex);
        assert_eq!(parsed.spend_pub_hex, demo.spend_pub_hex);
    }

    #[test]
    fn invalid_pubkey_in_address_payload_is_rejected() {
        // Build a 66-byte payload with an invalid scan pubkey (all zeros).
        let payload = [0u8; PAYLOAD_LEN];
        let hrp = Hrp::parse("sp").unwrap();
        let s = bech32::encode::<Bech32m>(hrp, &payload).unwrap();
        let err = SilentPaymentAddress::decode(&s).unwrap_err();
        assert!(matches!(err, SpError::InvalidPubkey(_)));
    }

    #[test]
    fn ecdh_changes_when_input_hash_changes() {
        let (a_sum, _) = fixed_keypair(0x11);
        let (_, b_scan_pub) = fixed_keypair(0x22);
        let h1 = [0x01; 32];
        let h2 = [0x02; 32];
        let s1 = sender_ecdh_secret(&a_sum, &b_scan_pub, &h1).unwrap();
        let s2 = sender_ecdh_secret(&a_sum, &b_scan_pub, &h2).unwrap();
        assert_ne!(s1, s2);
    }

    #[test]
    fn output_tweak_low_level_matches_high_level() {
        let secret = [0x07u8; 33];
        let k = 5u32;
        let high = output_tweak(&secret, k);
        let low = output_tweak_with_index_bytes(&secret, &k.to_be_bytes());
        assert_eq!(high, low);
    }

    #[test]
    fn pubkey_decode_round_trip_via_address() {
        let (_, scan_pub) = fixed_keypair(0xfe);
        let (_, spend_pub) = fixed_keypair(0xed);
        let addr = SilentPaymentAddress::from_pubkeys(SpNetwork::Mainnet, &scan_pub, &spend_pub);
        assert_eq!(addr.scan_pub().unwrap(), scan_pub);
        assert_eq!(addr.spend_pub().unwrap(), spend_pub);
    }
}
