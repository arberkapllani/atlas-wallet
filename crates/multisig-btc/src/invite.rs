//! Co-signer invite envelopes — the shareable side of multisig setup.
//!
//! When Alice wants to bootstrap a 2-of-3 with Bob and Carol, she
//! doesn't need their seeds — she only needs each co-signer's
//! account-level xpub plus its key origin so the descriptor can include
//! `[fp/origin]`. Atlas exchanges that data through an invite URI:
//!
//! ```text
//! atlas-msig://signer?v=1&payload=<base64url(json)>
//! ```
//!
//! The payload JSON carries a single `MultisigSigner` plus an optional
//! human label and a free-form policy hint ("2-of-3 cold storage").
//! Receiving wallets parse it, verify the xpub decodes, and offer to
//! add it to a draft policy. Nothing in the envelope is secret; it's
//! safe to share over Signal, email, or printed QR.

use serde::{Deserialize, Serialize};

use crate::{MultisigError, MultisigSigner};

/// Wire format Atlas exchanges with a co-signer wallet (Sparrow,
/// Specter, another Atlas instance).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct CoSignerInvite {
    /// Wire-format version. Bump if the field set changes.
    pub v: u32,
    /// The co-signer's xpub + origin + fingerprint.
    pub signer: MultisigSigner,
    /// Optional human label ("Alice's Coldcard").
    pub label: Option<String>,
    /// Optional policy hint ("2-of-3", "treasury cold storage").
    pub policy_hint: Option<String>,
}

const SCHEME: &str = "atlas-msig";
const PATH: &str = "signer";

/// Encode a `CoSignerInvite` as an `atlas-msig://signer?...` URI. The
/// payload is base64url JSON so the entire URI is ASCII and survives
/// QR codes / messengers without further encoding.
pub fn build_invite_uri(invite: &CoSignerInvite) -> Result<String, MultisigError> {
    let json = serde_json::to_vec(invite)
        .map_err(|e| MultisigError::InvalidInput(format!("serialize invite: {e}")))?;
    let payload = base64url_encode(&json);
    Ok(format!(
        "{scheme}://{path}?v={v}&payload={payload}",
        scheme = SCHEME,
        path = PATH,
        v = invite.v,
        payload = payload
    ))
}

/// Parse a previously-built invite URI. Strict: rejects unknown
/// schemes, missing `payload`, or anything that doesn't decode into a
/// `CoSignerInvite`.
pub fn parse_invite_uri(uri: &str) -> Result<CoSignerInvite, MultisigError> {
    let trimmed = uri.trim();
    let prefix = format!("{}://{}?", SCHEME, PATH);
    let rest = trimmed
        .strip_prefix(&prefix)
        .ok_or_else(|| MultisigError::InvalidInput(format!("expected {} URI", prefix)))?;

    let mut payload: Option<&str> = None;
    for kv in rest.split('&') {
        let mut split = kv.splitn(2, '=');
        let k = split.next().unwrap_or("");
        let v = split.next().unwrap_or("");
        if k == "payload" {
            payload = Some(v);
        }
    }
    let payload = payload.ok_or_else(|| {
        MultisigError::InvalidInput("invite URI missing payload= parameter".into())
    })?;

    let bytes = base64url_decode(payload)?;
    let invite: CoSignerInvite = serde_json::from_slice(&bytes)
        .map_err(|e| MultisigError::InvalidInput(format!("decode invite: {e}")))?;

    // Re-validate the embedded signer's xpub by trying to parse it as a
    // BIP-32 extended public key. Catches obvious tampering early.
    use std::str::FromStr;
    bitcoin::bip32::Xpub::from_str(invite.signer.xpub.trim())
        .map_err(|e| MultisigError::InvalidInput(format!("invite xpub: {e}")))?;

    Ok(invite)
}

// ----- base64url (no padding) -----------------------------------------------

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn base64url_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut chunks = input.chunks_exact(3);
    for ch in &mut chunks {
        let n = (u32::from(ch[0]) << 16) | (u32::from(ch[1]) << 8) | u32::from(ch[2]);
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        out.push(ALPHABET[(n & 0x3f) as usize] as char);
    }
    let rem = chunks.remainder();
    match rem.len() {
        0 => {}
        1 => {
            let n = u32::from(rem[0]) << 16;
            out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
            out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        }
        2 => {
            let n = (u32::from(rem[0]) << 16) | (u32::from(rem[1]) << 8);
            out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
            out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
            out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        }
        _ => unreachable!(),
    }
    out
}

fn base64url_decode(s: &str) -> Result<Vec<u8>, MultisigError> {
    // Build a reverse lookup table once per call — invites are small.
    let mut decode = [255u8; 256];
    for (i, &c) in ALPHABET.iter().enumerate() {
        decode[c as usize] = i as u8;
    }
    let bytes: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for ch in bytes.chunks(4) {
        if ch.len() < 2 {
            return Err(MultisigError::InvalidInput("payload truncated".into()));
        }
        let mut buf = [0u8; 4];
        for (i, &b) in ch.iter().enumerate() {
            let v = decode[b as usize];
            if v == 255 {
                return Err(MultisigError::InvalidInput(format!(
                    "payload contains non-base64url byte {:#x}",
                    b
                )));
            }
            buf[i] = v;
        }
        let n = (u32::from(buf[0]) << 18)
            | (u32::from(buf[1]) << 12)
            | (u32::from(buf[2]) << 6)
            | u32::from(buf[3]);
        out.push(((n >> 16) & 0xff) as u8);
        if ch.len() >= 3 {
            out.push(((n >> 8) & 0xff) as u8);
        }
        if ch.len() == 4 {
            out.push((n & 0xff) as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::{
        bip32::{ChildNumber, Xpriv, Xpub},
        secp256k1::Secp256k1,
        Network,
    };

    fn sample_invite() -> CoSignerInvite {
        let secp = Secp256k1::new();
        let seed = [0x42u8; 32];
        let master = Xpriv::new_master(Network::Bitcoin, &seed).unwrap();
        let path = vec![
            ChildNumber::from_hardened_idx(48).unwrap(),
            ChildNumber::from_hardened_idx(0).unwrap(),
            ChildNumber::from_hardened_idx(0).unwrap(),
            ChildNumber::from_hardened_idx(2).unwrap(),
        ];
        let derived = master.derive_priv(&secp, &path).unwrap();
        let xpub = Xpub::from_priv(&secp, &derived);

        CoSignerInvite {
            v: 1,
            signer: MultisigSigner {
                fingerprint: "deadbeef".into(),
                origin: "m/48h/0h/0h/2h".into(),
                xpub: xpub.to_string(),
            },
            label: Some("Alice's Coldcard".into()),
            policy_hint: Some("2-of-3 cold storage".into()),
        }
    }

    #[test]
    fn round_trip_invite_uri() {
        let inv = sample_invite();
        let uri = build_invite_uri(&inv).unwrap();
        assert!(uri.starts_with("atlas-msig://signer?"));
        assert!(uri.contains("payload="));
        let back = parse_invite_uri(&uri).unwrap();
        assert_eq!(back.v, inv.v);
        assert_eq!(back.signer.xpub, inv.signer.xpub);
        assert_eq!(back.signer.fingerprint, inv.signer.fingerprint);
        assert_eq!(back.label.as_deref(), Some("Alice's Coldcard"));
    }

    #[test]
    fn parse_rejects_wrong_scheme() {
        let err = parse_invite_uri("bitcoin:bc1qxyz?amount=1").unwrap_err();
        assert!(matches!(err, MultisigError::InvalidInput(_)));
    }

    #[test]
    fn parse_rejects_missing_payload() {
        let err = parse_invite_uri("atlas-msig://signer?v=1").unwrap_err();
        assert!(matches!(err, MultisigError::InvalidInput(_)));
    }

    #[test]
    fn parse_rejects_corrupt_xpub() {
        // Replace the embedded xpub with a deliberately broken one.
        let mut inv = sample_invite();
        inv.signer.xpub = "xpubGARBAGE".into();
        let uri = build_invite_uri(&inv).unwrap();
        let err = parse_invite_uri(&uri).unwrap_err();
        assert!(matches!(err, MultisigError::InvalidInput(_)));
    }

    #[test]
    fn parse_rejects_garbled_payload() {
        let err = parse_invite_uri("atlas-msig://signer?payload=!!!!").unwrap_err();
        assert!(matches!(err, MultisigError::InvalidInput(_)));
    }

    #[test]
    fn base64url_round_trips() {
        for n in 0..32usize {
            let input: Vec<u8> = (0..n).map(|i| (i * 37 % 256) as u8).collect();
            let enc = base64url_encode(&input);
            let dec = base64url_decode(&enc).unwrap();
            assert_eq!(dec, input, "round-trip failed at len {}", n);
        }
    }
}
