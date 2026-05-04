//! Payment-URI parser and builder.
//!
//! Supports the two ubiquitous wallet QR formats:
//!   - BIP-21:  `bitcoin:<addr>?amount=<btc>&label=<...>&message=<...>`
//!   - EIP-681: `ethereum:<addr>[@<chainId>][/transfer?address=<to>&uint256=<wei>]`
//!
//! Designed to be the back-end of the Send screen's "Scan QR"
//! flow — read whatever the camera produces, classify it, and
//! return enough structured data for the UI to pre-fill fields.
//!
//! Pure data — no I/O.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

#[derive(Debug, Error, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "message")]
pub enum PayUriError {
    #[error("invalid scheme: {0}")]
    InvalidScheme(String),
    #[error("missing address")]
    MissingAddress,
    #[error("malformed: {0}")]
    Malformed(String),
}

/// Discriminated payment intent.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "kind")]
pub enum PaymentIntent {
    Bitcoin(BitcoinPayment),
    Ethereum(EthereumPayment),
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BitcoinPayment {
    pub address: String,
    /// Amount in BTC as a decimal string (preserves precision).
    pub amount_btc: Option<String>,
    pub label: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct EthereumPayment {
    /// `to` for native, or the contract for `transfer`.
    pub address: String,
    pub chain_id: Option<u64>,
    /// `Some` only when a `transfer` function call is encoded
    /// (ERC-20 token transfers).
    pub function: Option<String>,
    /// For ERC-20: real recipient (decoded from `address` query
    /// parameter on the contract call).
    pub token_recipient: Option<String>,
    /// Amount in wei as a decimal string. For ERC-20 this is the
    /// `uint256` parameter (token base units).
    pub amount_wei: Option<String>,
    /// Optional gas / gasPrice query params, unparsed.
    pub gas: Option<String>,
}

/// Parse any supported payment URI.
pub fn parse(input: &str) -> Result<PaymentIntent, PayUriError> {
    let s = input.trim();
    if let Some(rest) = strip_scheme(s, "bitcoin:") {
        return parse_bitcoin(rest).map(PaymentIntent::Bitcoin);
    }
    if let Some(rest) = strip_scheme(s, "ethereum:") {
        return parse_ethereum(rest).map(PaymentIntent::Ethereum);
    }
    Err(PayUriError::InvalidScheme(s.to_string()))
}

/// Build a BIP-21 URI.
pub fn build_bitcoin(p: &BitcoinPayment) -> String {
    let mut out = format!("bitcoin:{}", p.address);
    let mut params: Vec<(String, String)> = Vec::new();
    if let Some(a) = &p.amount_btc {
        params.push(("amount".into(), a.clone()));
    }
    if let Some(l) = &p.label {
        params.push(("label".into(), percent_encode(l)));
    }
    if let Some(m) = &p.message {
        params.push(("message".into(), percent_encode(m)));
    }
    if !params.is_empty() {
        out.push('?');
        out.push_str(
            &params
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("&"),
        );
    }
    out
}

/// Build an EIP-681 URI for a native ETH (or chain-native) transfer.
/// For ERC-20 / contract calls, callers typically already know the
/// shape they want — keep this helper simple.
pub fn build_ethereum(p: &EthereumPayment) -> String {
    let mut out = format!("ethereum:{}", p.address);
    if let Some(cid) = p.chain_id {
        out.push_str(&format!("@{cid}"));
    }
    if let Some(fname) = &p.function {
        out.push('/');
        out.push_str(fname);
    }
    let mut params: Vec<(String, String)> = Vec::new();
    if let Some(to) = &p.token_recipient {
        params.push(("address".into(), to.clone()));
    }
    if let Some(amt) = &p.amount_wei {
        let key = if p.function.as_deref() == Some("transfer") {
            "uint256"
        } else {
            "value"
        };
        params.push((key.into(), amt.clone()));
    }
    if let Some(g) = &p.gas {
        params.push(("gas".into(), g.clone()));
    }
    if !params.is_empty() {
        out.push('?');
        out.push_str(
            &params
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("&"),
        );
    }
    out
}

// --- internals -------------------------------------------------------

fn strip_scheme<'a>(s: &'a str, scheme: &str) -> Option<&'a str> {
    if s.len() < scheme.len() {
        return None;
    }
    let head = &s[..scheme.len()];
    if head.eq_ignore_ascii_case(scheme) {
        Some(&s[scheme.len()..])
    } else {
        None
    }
}

fn split_query(rest: &str) -> (&str, &str) {
    match rest.split_once('?') {
        Some((a, q)) => (a, q),
        None => (rest, ""),
    }
}

fn parse_kv(query: &str) -> Vec<(String, String)> {
    if query.is_empty() {
        return Vec::new();
    }
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (k.to_string(), percent_decode(v)),
            None => (p.to_string(), String::new()),
        })
        .collect()
}

fn parse_bitcoin(rest: &str) -> Result<BitcoinPayment, PayUriError> {
    let (addr, query) = split_query(rest);
    if addr.is_empty() {
        return Err(PayUriError::MissingAddress);
    }
    let mut p = BitcoinPayment {
        address: addr.to_string(),
        amount_btc: None,
        label: None,
        message: None,
    };
    for (k, v) in parse_kv(query) {
        match k.as_str() {
            "amount" => p.amount_btc = Some(v),
            "label" => p.label = Some(v),
            "message" => p.message = Some(v),
            _ => {}
        }
    }
    Ok(p)
}

fn parse_ethereum(rest: &str) -> Result<EthereumPayment, PayUriError> {
    let (target, query) = split_query(rest);
    if target.is_empty() {
        return Err(PayUriError::MissingAddress);
    }
    // target = <addr>[@<chainId>][/<function>]
    let (addr_chain, function) = match target.split_once('/') {
        Some((a, f)) => (a, Some(f.to_string())),
        None => (target, None),
    };
    let (address, chain_id) = match addr_chain.split_once('@') {
        Some((a, c)) => {
            let parsed: u64 = c
                .parse()
                .map_err(|_| PayUriError::Malformed(format!("chainId: {c}")))?;
            (a.to_string(), Some(parsed))
        }
        None => (addr_chain.to_string(), None),
    };
    if address.is_empty() {
        return Err(PayUriError::MissingAddress);
    }

    let mut p = EthereumPayment {
        address,
        chain_id,
        function,
        token_recipient: None,
        amount_wei: None,
        gas: None,
    };

    for (k, v) in parse_kv(query) {
        match k.as_str() {
            "address" => p.token_recipient = Some(v),
            "uint256" | "value" => p.amount_wei = Some(v),
            "gas" | "gasPrice" => p.gas = Some(v),
            _ => {}
        }
    }
    Ok(p)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let h = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(b) = u8::from_str_radix(h, 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        let safe = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~');
        if safe {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_bitcoin() {
        let p = parse("bitcoin:bc1qexample").unwrap();
        match p {
            PaymentIntent::Bitcoin(b) => {
                assert_eq!(b.address, "bc1qexample");
                assert!(b.amount_btc.is_none());
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parses_bitcoin_with_amount_and_label() {
        let p = parse("BITCOIN:bc1qabc?amount=0.001&label=Coffee%20Shop").unwrap();
        match p {
            PaymentIntent::Bitcoin(b) => {
                assert_eq!(b.address, "bc1qabc");
                assert_eq!(b.amount_btc.as_deref(), Some("0.001"));
                assert_eq!(b.label.as_deref(), Some("Coffee Shop"));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parses_native_ethereum_with_value() {
        let p = parse("ethereum:0xabc@1?value=1000000000000000000").unwrap();
        match p {
            PaymentIntent::Ethereum(e) => {
                assert_eq!(e.address, "0xabc");
                assert_eq!(e.chain_id, Some(1));
                assert!(e.function.is_none());
                assert_eq!(e.amount_wei.as_deref(), Some("1000000000000000000"));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parses_erc20_transfer() {
        let p = parse("ethereum:0xtoken@1/transfer?address=0xrecipient&uint256=12345").unwrap();
        match p {
            PaymentIntent::Ethereum(e) => {
                assert_eq!(e.address, "0xtoken");
                assert_eq!(e.function.as_deref(), Some("transfer"));
                assert_eq!(e.token_recipient.as_deref(), Some("0xrecipient"));
                assert_eq!(e.amount_wei.as_deref(), Some("12345"));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parses_ethereum_without_chain_id() {
        let p = parse("ethereum:0xabc").unwrap();
        match p {
            PaymentIntent::Ethereum(e) => {
                assert_eq!(e.address, "0xabc");
                assert!(e.chain_id.is_none());
            }
            _ => panic!(),
        }
    }

    #[test]
    fn rejects_unknown_scheme() {
        let err = parse("doge:Dabc").unwrap_err();
        assert!(matches!(err, PayUriError::InvalidScheme(_)));
    }

    #[test]
    fn rejects_missing_address() {
        let err = parse("bitcoin:").unwrap_err();
        assert!(matches!(err, PayUriError::MissingAddress));
    }

    #[test]
    fn rejects_bad_chain_id() {
        let err = parse("ethereum:0xabc@notanumber").unwrap_err();
        assert!(matches!(err, PayUriError::Malformed(_)));
    }

    #[test]
    fn build_bitcoin_round_trips() {
        let original = BitcoinPayment {
            address: "bc1qabc".into(),
            amount_btc: Some("0.5".into()),
            label: Some("Tip".into()),
            message: None,
        };
        let uri = build_bitcoin(&original);
        let p = parse(&uri).unwrap();
        match p {
            PaymentIntent::Bitcoin(b) => {
                assert_eq!(b.address, original.address);
                assert_eq!(b.amount_btc, original.amount_btc);
                assert_eq!(b.label, original.label);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn build_eth_native_emits_value_param() {
        let p = EthereumPayment {
            address: "0xrecv".into(),
            chain_id: Some(137),
            function: None,
            token_recipient: None,
            amount_wei: Some("1000".into()),
            gas: None,
        };
        let uri = build_ethereum(&p);
        assert!(uri.contains("@137"));
        assert!(uri.contains("value=1000"));
    }

    #[test]
    fn build_eth_transfer_emits_uint256_param() {
        let p = EthereumPayment {
            address: "0xtoken".into(),
            chain_id: Some(1),
            function: Some("transfer".into()),
            token_recipient: Some("0xrecv".into()),
            amount_wei: Some("999".into()),
            gas: None,
        };
        let uri = build_ethereum(&p);
        assert!(uri.contains("/transfer?"));
        assert!(uri.contains("uint256=999"));
        assert!(uri.contains("address=0xrecv"));
    }

    #[test]
    fn unknown_query_params_ignored() {
        let p = parse("bitcoin:bc1qabc?amount=1&exotic=xx").unwrap();
        match p {
            PaymentIntent::Bitcoin(b) => assert_eq!(b.amount_btc.as_deref(), Some("1")),
            _ => panic!(),
        }
    }

    #[test]
    fn percent_decoding_handles_plus_and_hex() {
        let p = parse("bitcoin:bc1qabc?label=hello+world%21").unwrap();
        match p {
            PaymentIntent::Bitcoin(b) => assert_eq!(b.label.as_deref(), Some("hello world!")),
            _ => panic!(),
        }
    }
}
