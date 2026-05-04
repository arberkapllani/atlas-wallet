//! Shamir Secret Sharing over GF(256) — the cryptographic core of SLIP-39.
//!
//! Splits a secret byte string into `total` shares such that any
//! `threshold` of them recover the secret, and any fewer reveal nothing.
//!
//! Each share is a (`x`, `y_bytes`) pair. The polynomial coefficients
//! are sampled fresh per byte position from the supplied RNG, so the
//! same secret will produce different shares on every call.
//!
//! GF(256) uses the AES / SLIP-39 reduction polynomial
//! `x^8 + x^4 + x^3 + x + 1` (0x11b).
//!
//! This crate intentionally exposes only the algebraic primitive so it
//! can later be wrapped by the full SLIP-39 mnemonic + RS1024 +
//! master-secret-encryption layers.

use rand::RngCore;
use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ShamirError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("threshold {threshold} exceeds total {total}")]
    BadThreshold { threshold: u8, total: u8 },
    #[error("duplicate share index {0}")]
    DuplicateIndex(u8),
    #[error("inconsistent share lengths")]
    InconsistentLength,
    #[error("not enough shares to recover (need {need}, got {got})")]
    NotEnoughShares { need: u8, got: usize },
    #[error("hex decode error: {0}")]
    Hex(#[from] hex::FromHexError),
}

/// One Shamir share. `x` is the evaluation point (1..=255, never 0)
/// and `y` is the per-byte polynomial value at `x`.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct Share {
    pub x: u8,
    /// Hex-encoded ciphertext bytes; same length as the original secret.
    pub y_hex: String,
}

// ---------- GF(256) arithmetic ----------------------------------------

#[inline]
fn gf_add(a: u8, b: u8) -> u8 {
    a ^ b
}

#[inline]
fn gf_mul(mut a: u8, mut b: u8) -> u8 {
    let mut r: u8 = 0;
    for _ in 0..8 {
        if b & 1 != 0 {
            r ^= a;
        }
        let hi = a & 0x80;
        a <<= 1;
        if hi != 0 {
            a ^= 0x1b; // reduction: x^8 = x^4 + x^3 + x + 1
        }
        b >>= 1;
    }
    r
}

fn gf_pow(mut base: u8, mut exp: u32) -> u8 {
    let mut acc: u8 = 1;
    while exp > 0 {
        if exp & 1 == 1 {
            acc = gf_mul(acc, base);
        }
        base = gf_mul(base, base);
        exp >>= 1;
    }
    acc
}

#[inline]
fn gf_inv(a: u8) -> u8 {
    // a != 0; Fermat: a^(2^8 - 2) = a^254
    gf_pow(a, 254)
}

/// Evaluate polynomial with coefficients `coeffs[0] + coeffs[1]*x + ...`
/// at point `x` using Horner's method.
fn eval_poly(coeffs: &[u8], x: u8) -> u8 {
    let mut acc: u8 = 0;
    for c in coeffs.iter().rev() {
        acc = gf_add(gf_mul(acc, x), *c);
    }
    acc
}

// ---------- Public API ------------------------------------------------

/// Split `secret` into `total` shares with recovery `threshold`.
///
/// `threshold` must be in 1..=`total`, `total` <= 255, and `secret`
/// must be non-empty.
pub fn split(
    secret: &[u8],
    threshold: u8,
    total: u8,
    rng: &mut impl RngCore,
) -> Result<Vec<Share>, ShamirError> {
    if secret.is_empty() {
        return Err(ShamirError::InvalidInput("empty secret".into()));
    }
    if total == 0 || threshold == 0 {
        return Err(ShamirError::InvalidInput(
            "threshold and total must be >= 1".into(),
        ));
    }
    if threshold > total {
        return Err(ShamirError::BadThreshold { threshold, total });
    }

    let mut shares: Vec<Share> = Vec::with_capacity(total as usize);
    // For each byte of the secret build a degree-(threshold-1) polynomial
    // with constant term = secret byte, fresh random higher-order coeffs.
    let n = secret.len();
    let mut ys: Vec<Vec<u8>> = (0..total).map(|_| vec![0u8; n]).collect();

    for (byte_idx, &s) in secret.iter().enumerate() {
        let mut coeffs = vec![0u8; threshold as usize];
        coeffs[0] = s;
        if threshold > 1 {
            let mut buf = vec![0u8; (threshold - 1) as usize];
            rng.fill_bytes(&mut buf);
            for (i, &b) in buf.iter().enumerate() {
                coeffs[i + 1] = b;
            }
        }
        for i in 0..total {
            let x = i + 1; // x must never be 0
            ys[i as usize][byte_idx] = eval_poly(&coeffs, x);
        }
    }

    for (i, y) in ys.into_iter().enumerate() {
        shares.push(Share {
            x: (i as u8) + 1,
            y_hex: hex::encode(y),
        });
    }
    Ok(shares)
}

/// Recover the secret given at least `threshold` valid shares.
///
/// All supplied shares must have the same byte length, distinct `x`s
/// (1..=255), and decode as hex.
pub fn combine(shares: &[Share]) -> Result<Vec<u8>, ShamirError> {
    if shares.is_empty() {
        return Err(ShamirError::NotEnoughShares { need: 1, got: 0 });
    }

    let mut decoded: Vec<(u8, Vec<u8>)> = Vec::with_capacity(shares.len());
    let mut seen_x: Vec<u8> = Vec::with_capacity(shares.len());
    let mut secret_len: Option<usize> = None;
    for s in shares {
        if s.x == 0 {
            return Err(ShamirError::InvalidInput("share x cannot be 0".into()));
        }
        if seen_x.contains(&s.x) {
            return Err(ShamirError::DuplicateIndex(s.x));
        }
        seen_x.push(s.x);
        let y = hex::decode(&s.y_hex)?;
        match secret_len {
            None => secret_len = Some(y.len()),
            Some(l) if l == y.len() => {}
            _ => return Err(ShamirError::InconsistentLength),
        }
        decoded.push((s.x, y));
    }
    let n = secret_len.unwrap_or(0);
    if n == 0 {
        return Err(ShamirError::InvalidInput("empty share".into()));
    }

    // Lagrange interpolation at x=0 per byte.
    let mut out = vec![0u8; n];
    for byte_idx in 0..n {
        let mut acc: u8 = 0;
        for (i, (xi, yi)) in decoded.iter().enumerate() {
            let mut num: u8 = 1;
            let mut den: u8 = 1;
            for (j, (xj, _)) in decoded.iter().enumerate() {
                if i == j {
                    continue;
                }
                // basis_i(0) = prod_{j!=i} (-xj) / (xi - xj); in GF(256) -x = x.
                num = gf_mul(num, *xj);
                den = gf_mul(den, gf_add(*xi, *xj));
            }
            if den == 0 {
                return Err(ShamirError::DuplicateIndex(*xi));
            }
            let basis = gf_mul(num, gf_inv(den));
            acc = gf_add(acc, gf_mul(yi[byte_idx], basis));
        }
        out[byte_idx] = acc;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn rng() -> StdRng {
        StdRng::seed_from_u64(0xA71A5)
    }

    #[test]
    fn round_trip_2_of_3() {
        let secret = b"correct horse battery staple";
        let shares = split(secret, 2, 3, &mut rng()).unwrap();
        assert_eq!(shares.len(), 3);
        assert_eq!(combine(&shares[..2]).unwrap(), secret);
        assert_eq!(combine(&shares[1..]).unwrap(), secret);
        assert_eq!(
            combine(&[shares[0].clone(), shares[2].clone()]).unwrap(),
            secret
        );
    }

    #[test]
    fn round_trip_3_of_5() {
        let secret: Vec<u8> = (0..32u8).collect();
        let shares = split(&secret, 3, 5, &mut rng()).unwrap();
        assert_eq!(combine(&shares[..3]).unwrap(), secret);
        assert_eq!(combine(&shares[2..5]).unwrap(), secret);
    }

    #[test]
    fn round_trip_1_of_1_is_identity() {
        let secret = b"single";
        let shares = split(secret, 1, 1, &mut rng()).unwrap();
        assert_eq!(shares.len(), 1);
        assert_eq!(combine(&shares).unwrap(), secret);
    }

    #[test]
    fn fewer_than_threshold_does_not_recover() {
        let secret = b"top secret payload";
        let shares = split(secret, 3, 5, &mut rng()).unwrap();
        let recovered = combine(&shares[..2]).unwrap();
        assert_ne!(recovered, secret);
    }

    #[test]
    fn rejects_threshold_above_total() {
        let err = split(b"x", 4, 3, &mut rng()).unwrap_err();
        assert!(matches!(err, ShamirError::BadThreshold { .. }));
    }

    #[test]
    fn rejects_empty_secret() {
        assert!(split(&[], 2, 3, &mut rng()).is_err());
    }

    #[test]
    fn rejects_zero_threshold() {
        assert!(split(b"x", 0, 3, &mut rng()).is_err());
    }

    #[test]
    fn combine_rejects_duplicate_indices() {
        let secret = b"dup-test";
        let shares = split(secret, 2, 3, &mut rng()).unwrap();
        let dup = vec![shares[0].clone(), shares[0].clone()];
        let err = combine(&dup).unwrap_err();
        assert!(matches!(err, ShamirError::DuplicateIndex(_)));
    }

    #[test]
    fn combine_rejects_inconsistent_lengths() {
        let mut shares = split(b"abcdef", 2, 3, &mut rng()).unwrap();
        shares[1].y_hex = "00".to_string();
        let err = combine(&shares[..2]).unwrap_err();
        assert!(matches!(err, ShamirError::InconsistentLength));
    }

    #[test]
    fn gf_mul_identity_and_inverse() {
        for a in 1u8..=255 {
            assert_eq!(gf_mul(a, 1), a);
            assert_eq!(gf_mul(a, gf_inv(a)), 1);
        }
    }

    #[test]
    fn shares_differ_across_calls() {
        // Same secret + same threshold should produce different shares
        // on independent RNGs (proving fresh randomness per call).
        let secret = b"freshness";
        let a = split(secret, 2, 3, &mut StdRng::seed_from_u64(1)).unwrap();
        let b = split(secret, 2, 3, &mut StdRng::seed_from_u64(2)).unwrap();
        assert_ne!(a[0].y_hex, b[0].y_hex);
        assert_eq!(combine(&a[..2]).unwrap(), secret);
        assert_eq!(combine(&b[..2]).unwrap(), secret);
    }
}
