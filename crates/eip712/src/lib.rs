//! EIP-712 typed-data signing-request inspector.
//!
//! When a dApp asks the wallet to sign typed data, the user usually
//! has no idea what the signature actually authorises. The most
//! common dark pattern is asking for a `Permit` or `PermitBatch`
//! signature that grants unlimited spend on an ERC-20 token without
//! ever showing up as an on-chain `approve` transaction.
//!
//! This crate parses an EIP-712 JSON payload and returns a category
//! plus a risk level plus a list of plain-English findings the UI
//! can show before the user clicks "Sign".
//!
//! Pure data â€” no network, no signing.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

#[derive(Debug, Error, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "message")]
pub enum Eip712Error {
    #[error("invalid json: {0}")]
    InvalidJson(String),
    #[error("missing field: {0}")]
    MissingField(String),
}

/// EIP-712 domain separator fields, only the parts we surface.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct Eip712Domain {
    pub name: Option<String>,
    pub version: Option<String>,
    pub chain_id: Option<u64>,
    pub verifying_contract: Option<String>,
}

/// What the signing request authorises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum Eip712Category {
    /// ERC-2612 token spend approval.
    Permit,
    /// Uniswap Permit2 single / batch approval.
    Permit2,
    /// OpenSea Seaport order signature (NFT listing).
    SeaportOrder,
    /// Gnosis Safe multisig transaction.
    SafeTx,
    /// Anything else.
    Generic,
}

/// Coarse risk classification, mirrors `atlas-approvals` but kept
/// distinct so type names don't collide in the TypeScript bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum SignatureRisk {
    Critical,
    High,
    Medium,
    Low,
}

/// Inspector output.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Eip712Report {
    pub primary_type: String,
    pub domain: Eip712Domain,
    pub category: Eip712Category,
    pub risk: SignatureRisk,
    pub findings: Vec<String>,
}

/// Parse and classify a raw EIP-712 typed-data JSON payload.
pub fn classify(json: &str) -> Result<Eip712Report, Eip712Error> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| Eip712Error::InvalidJson(e.to_string()))?;

    let primary_type = v
        .get("primaryType")
        .and_then(|x| x.as_str())
        .ok_or_else(|| Eip712Error::MissingField("primaryType".into()))?
        .to_string();

    let domain_v = v
        .get("domain")
        .ok_or_else(|| Eip712Error::MissingField("domain".into()))?;
    let domain = Eip712Domain {
        name: domain_v
            .get("name")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        version: domain_v
            .get("version")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        chain_id: domain_v.get("chainId").and_then(read_u64),
        verifying_contract: domain_v
            .get("verifyingContract")
            .and_then(|x| x.as_str())
            .map(str::to_string),
    };

    let message = v.get("message").unwrap_or(&serde_json::Value::Null);

    let category = detect_category(&primary_type, &domain, message);

    let mut findings: Vec<String> = Vec::new();
    let risk: SignatureRisk = match category {
        Eip712Category::Permit => {
            if let Some(name) = &domain.name {
                findings.push(format!("Token approval signature for `{name}`."));
            } else {
                findings.push("Token approval signature.".into());
            }
            let value = message.get("value").and_then(read_decimal_string);
            let owner = message.get("owner").and_then(|x| x.as_str());
            let spender = message.get("spender").and_then(|x| x.as_str());
            let deadline = message.get("deadline").and_then(read_u64).unwrap_or(0);

            if let Some(o) = owner {
                findings.push(format!("Owner: {o}"));
            }
            if let Some(s) = spender {
                findings.push(format!("Spender: {s}"));
            }
            let unlimited_value = match &value {
                Some(v) if is_unlimited(v) => {
                    findings.push("Unlimited token allowance requested.".into());
                    true
                }
                Some(v) => {
                    findings.push(format!("Allowance: {v}"));
                    false
                }
                None => false,
            };
            let bad_deadline = deadline > 0 && deadline > unix_far_future();
            if bad_deadline {
                findings.push("Deadline more than 1 year in the future.".into());
            }
            if unlimited_value || bad_deadline {
                SignatureRisk::Critical
            } else {
                SignatureRisk::High
            }
        }
        Eip712Category::Permit2 => {
            findings.push("Uniswap Permit2 token approval.".into());
            // Walk message.details.amount or message.details[*].amount.
            let any_unlimited = if let Some(details) = message.get("details") {
                let amounts: Vec<String> = match details {
                    serde_json::Value::Array(arr) => arr
                        .iter()
                        .filter_map(|d| d.get("amount").and_then(read_decimal_string))
                        .collect(),
                    serde_json::Value::Object(_) => details
                        .get("amount")
                        .and_then(read_decimal_string)
                        .map(|a| vec![a])
                        .unwrap_or_default(),
                    _ => Vec::new(),
                };
                amounts.iter().any(|a| is_unlimited(a))
            } else {
                false
            };
            if any_unlimited {
                findings.push("At least one allowance is unlimited.".into());
            }
            if let Some(spender) = message.get("spender").and_then(|x| x.as_str()) {
                findings.push(format!("Spender: {spender}"));
            }
            if any_unlimited {
                SignatureRisk::Critical
            } else {
                SignatureRisk::High
            }
        }
        Eip712Category::SeaportOrder => {
            findings.push("OpenSea Seaport NFT order signature.".into());
            findings.push("Signing this can transfer NFTs without a separate transaction.".into());
            SignatureRisk::High
        }
        Eip712Category::SafeTx => {
            findings.push("Gnosis Safe multisig transaction signature.".into());
            SignatureRisk::Medium
        }
        Eip712Category::Generic => {
            findings.push(format!("Generic typed-data signature: {primary_type}."));
            SignatureRisk::Low
        }
    };

    Ok(Eip712Report {
        primary_type,
        domain,
        category,
        risk,
        findings,
    })
}

// --- helpers ---------------------------------------------------------

fn detect_category(
    primary_type: &str,
    _domain: &Eip712Domain,
    _message: &serde_json::Value,
) -> Eip712Category {
    match primary_type {
        "Permit" => Eip712Category::Permit,
        "PermitSingle" | "PermitBatch" | "PermitTransferFrom" | "PermitBatchTransferFrom" => {
            Eip712Category::Permit2
        }
        "OrderComponents" => Eip712Category::SeaportOrder,
        "SafeTx" => Eip712Category::SafeTx,
        _ => Eip712Category::Generic,
    }
}

fn read_u64(v: &serde_json::Value) -> Option<u64> {
    if let Some(n) = v.as_u64() {
        return Some(n);
    }
    v.as_str().and_then(|s| {
        let s = s.trim();
        if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            u64::from_str_radix(hex, 16).ok()
        } else {
            s.parse::<u64>().ok()
        }
    })
}

fn read_decimal_string(v: &serde_json::Value) -> Option<String> {
    if let Some(n) = v.as_u64() {
        return Some(n.to_string());
    }
    if let Some(s) = v.as_str() {
        let s = s.trim();
        if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            // Decode arbitrary-width hex to decimal-string the cheap way:
            // u128 covers up to 2^128. For larger we keep the hex form.
            if hex.len() <= 32 {
                if let Ok(n) = u128::from_str_radix(hex, 16) {
                    return Some(n.to_string());
                }
            }
            return Some(s.to_string());
        }
        return Some(s.to_string());
    }
    None
}

/// True iff the decimal string is `2^256 - 1` (unlimited allowance).
fn is_unlimited(value: &str) -> bool {
    const MAX_U256_DEC: &str =
        "115792089237316195423570985008687907853269984665640564039457584007913129639935";
    value.trim() == MAX_U256_DEC
}

/// "1 year in the future" sentinel â€” we use 9999999999 (year 2286)
/// as an obviously-suspect deadline.
fn unix_far_future() -> u64 {
    9_999_999_999
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_json() {
        let err = classify("not json").unwrap_err();
        assert!(matches!(err, Eip712Error::InvalidJson(_)));
    }

    #[test]
    fn rejects_missing_primary_type() {
        let err = classify(r#"{"domain":{}}"#).unwrap_err();
        assert!(matches!(err, Eip712Error::MissingField(_)));
    }

    #[test]
    fn rejects_missing_domain() {
        let err = classify(r#"{"primaryType":"Mail"}"#).unwrap_err();
        assert!(matches!(err, Eip712Error::MissingField(_)));
    }

    #[test]
    fn classifies_generic() {
        let json = r#"{
            "primaryType":"Mail",
            "domain":{"name":"Ether Mail","chainId":1},
            "message":{"contents":"hi"}
        }"#;
        let r = classify(json).unwrap();
        assert_eq!(r.category, Eip712Category::Generic);
        assert_eq!(r.risk, SignatureRisk::Low);
        assert_eq!(r.domain.chain_id, Some(1));
    }

    #[test]
    fn classifies_permit_finite_value() {
        let json = r#"{
            "primaryType":"Permit",
            "domain":{"name":"USD Coin","chainId":1,"verifyingContract":"0xa0b8"},
            "message":{
                "owner":"0xowner",
                "spender":"0xspender",
                "value":"1000000",
                "nonce":0,
                "deadline":2000000000
            }
        }"#;
        let r = classify(json).unwrap();
        assert_eq!(r.category, Eip712Category::Permit);
        assert_eq!(r.risk, SignatureRisk::High);
        assert!(r.findings.iter().any(|f| f.contains("USD Coin")));
        assert!(r.findings.iter().any(|f| f.contains("Allowance: 1000000")));
    }

    #[test]
    fn classifies_permit_unlimited_as_critical() {
        let json = r#"{
            "primaryType":"Permit",
            "domain":{"name":"USDT","chainId":1},
            "message":{
                "owner":"0xowner",
                "spender":"0xspender",
                "value":"115792089237316195423570985008687907853269984665640564039457584007913129639935",
                "nonce":0,
                "deadline":2000000000
            }
        }"#;
        let r = classify(json).unwrap();
        assert_eq!(r.category, Eip712Category::Permit);
        assert_eq!(r.risk, SignatureRisk::Critical);
        assert!(r.findings.iter().any(|f| f.contains("Unlimited")));
    }

    #[test]
    fn classifies_permit_far_future_deadline_as_critical() {
        let json = r#"{
            "primaryType":"Permit",
            "domain":{"name":"USDT","chainId":1},
            "message":{
                "owner":"0xowner",
                "spender":"0xspender",
                "value":"100",
                "nonce":0,
                "deadline":99999999999
            }
        }"#;
        let r = classify(json).unwrap();
        assert_eq!(r.risk, SignatureRisk::Critical);
    }

    #[test]
    fn classifies_permit2_batch_with_unlimited() {
        let json = r#"{
            "primaryType":"PermitBatch",
            "domain":{"name":"Permit2","chainId":1},
            "message":{
                "spender":"0xuni",
                "details":[
                    {"token":"0xa","amount":"1000"},
                    {"token":"0xb","amount":"115792089237316195423570985008687907853269984665640564039457584007913129639935"}
                ]
            }
        }"#;
        let r = classify(json).unwrap();
        assert_eq!(r.category, Eip712Category::Permit2);
        assert_eq!(r.risk, SignatureRisk::Critical);
    }

    #[test]
    fn classifies_permit2_single_finite() {
        let json = r#"{
            "primaryType":"PermitSingle",
            "domain":{"name":"Permit2","chainId":1},
            "message":{
                "spender":"0xuni",
                "details":{"token":"0xa","amount":"1000"}
            }
        }"#;
        let r = classify(json).unwrap();
        assert_eq!(r.category, Eip712Category::Permit2);
        assert_eq!(r.risk, SignatureRisk::High);
    }

    #[test]
    fn classifies_seaport_order() {
        let json = r#"{
            "primaryType":"OrderComponents",
            "domain":{"name":"Seaport","version":"1.5","chainId":1},
            "message":{}
        }"#;
        let r = classify(json).unwrap();
        assert_eq!(r.category, Eip712Category::SeaportOrder);
        assert_eq!(r.risk, SignatureRisk::High);
    }

    #[test]
    fn classifies_safe_tx() {
        let json = r#"{
            "primaryType":"SafeTx",
            "domain":{"name":"Safe","chainId":1,"verifyingContract":"0xsafe"},
            "message":{}
        }"#;
        let r = classify(json).unwrap();
        assert_eq!(r.category, Eip712Category::SafeTx);
        assert_eq!(r.risk, SignatureRisk::Medium);
    }

    #[test]
    fn reads_chain_id_from_hex_string() {
        let json = r#"{
            "primaryType":"Mail",
            "domain":{"chainId":"0x89"},
            "message":{}
        }"#;
        let r = classify(json).unwrap();
        assert_eq!(r.domain.chain_id, Some(0x89));
    }

    #[test]
    fn permit_value_hex_decoded() {
        let json = r#"{
            "primaryType":"Permit",
            "domain":{"name":"X","chainId":1},
            "message":{
                "owner":"0xa","spender":"0xb",
                "value":"0xff","nonce":0,"deadline":2000000000
            }
        }"#;
        let r = classify(json).unwrap();
        assert!(r.findings.iter().any(|f| f.contains("Allowance: 255")));
    }
}
