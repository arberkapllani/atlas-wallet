//! WalletConnect v2 URI parser and namespace data shapes.
//!
//! WalletConnect v2 pairings are bootstrapped from a URI of the form
//! `wc:{topic}@2?relayProtocol=irn&symKey={hex32}&methods=[...]&expiryTimestamp=...`.
//! This crate parses that URI into a strongly-typed `WcUri` and exposes
//! the namespace request / approval shapes used by the Sign API
//! (`session_propose` / `session_approve`).
//!
//! The full relay-client (WebSocket + JSON-RPC) lives in a separate
//! transport crate; this one is pure data and parsing so it can be
//! exercised offline and reused from CLI / test tooling.

use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum WcError {
    #[error("invalid URI: {0}")]
    InvalidUri(String),
    #[error("unsupported WalletConnect version {0} (expected 2)")]
    UnsupportedVersion(String),
    #[error("missing required query parameter: {0}")]
    MissingParam(&'static str),
    #[error("invalid symmetric key: must be 32 bytes hex")]
    InvalidSymKey,
}

/// Parsed `wc:` v2 URI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct WcUri {
    /// 32-byte hex topic (the pairing topic).
    pub topic: String,
    /// Relay protocol (typically `"irn"`).
    pub relay_protocol: String,
    /// Optional relay data (forwarded to the relay).
    pub relay_data: Option<String>,
    /// 32-byte symmetric key, hex-encoded.
    pub sym_key: String,
    /// Optional expiry timestamp (seconds since epoch).
    pub expiry_timestamp: Option<u64>,
    /// Optional methods list as advertised by the dapp.
    pub methods: Vec<String>,
}

/// One namespace entry from a `session_propose` request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Namespace {
    pub chains: Vec<String>,
    pub methods: Vec<String>,
    pub events: Vec<String>,
}

/// Approval payload returned to the dapp.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct NamespaceApproval {
    pub accounts: Vec<String>,
    pub methods: Vec<String>,
    pub events: Vec<String>,
}

/// Parse a WalletConnect v2 URI.
pub fn parse_wc_uri(input: &str) -> Result<WcUri, WcError> {
    if !input.starts_with("wc:") {
        return Err(WcError::InvalidUri("scheme must be wc:".into()));
    }

    // url::Url won't parse `wc:topic@2?...` directly because the part
    // after `wc:` is opaque. Split manually around `?`.
    let body = &input[3..];
    let (path, query) = match body.split_once('?') {
        Some((p, q)) => (p, q),
        None => return Err(WcError::InvalidUri("missing query string".into())),
    };
    let (topic, version) = match path.split_once('@') {
        Some((t, v)) => (t.to_string(), v.to_string()),
        None => {
            return Err(WcError::InvalidUri(
                "missing version (expected `@2`)".into(),
            ))
        }
    };
    if topic.is_empty() {
        return Err(WcError::InvalidUri("empty topic".into()));
    }
    if version != "2" {
        return Err(WcError::UnsupportedVersion(version));
    }

    // Reuse url::Url for query parsing only.
    let dummy = Url::parse(&format!("https://x/?{query}"))
        .map_err(|e| WcError::InvalidUri(e.to_string()))?;
    let mut relay_protocol: Option<String> = None;
    let mut relay_data: Option<String> = None;
    let mut sym_key: Option<String> = None;
    let mut expiry_timestamp: Option<u64> = None;
    let mut methods: Vec<String> = Vec::new();

    for (k, v) in dummy.query_pairs() {
        match k.as_ref() {
            // The spec uses both spellings in the wild; accept either.
            "relay-protocol" | "relayProtocol" => relay_protocol = Some(v.into_owned()),
            "relay-data" | "relayData" => relay_data = Some(v.into_owned()),
            "symKey" | "sym-key" => sym_key = Some(v.into_owned()),
            "expiryTimestamp" => expiry_timestamp = v.parse().ok(),
            "methods" => {
                // methods can arrive as `["a","b"]` or comma-separated.
                let raw = v.into_owned();
                let trimmed = raw.trim_matches(|c| c == '[' || c == ']');
                methods = trimmed
                    .split(',')
                    .map(|s| s.trim_matches(|c: char| c == '"' || c.is_whitespace()))
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect();
            }
            _ => {}
        }
    }

    let relay_protocol = relay_protocol.ok_or(WcError::MissingParam("relay-protocol"))?;
    let sym_key = sym_key.ok_or(WcError::MissingParam("symKey"))?;
    if hex::decode(&sym_key).map(|b| b.len()).unwrap_or(0) != 32 {
        return Err(WcError::InvalidSymKey);
    }

    Ok(WcUri {
        topic,
        relay_protocol,
        relay_data,
        sym_key,
        expiry_timestamp,
        methods,
    })
}

/// Build a `wc:` v2 URI from its components. Inverse of `parse_wc_uri`
/// (modulo parameter ordering).
pub fn build_wc_uri(uri: &WcUri) -> Result<String, WcError> {
    if uri.topic.is_empty() {
        return Err(WcError::InvalidUri("empty topic".into()));
    }
    if hex::decode(&uri.sym_key).map(|b| b.len()).unwrap_or(0) != 32 {
        return Err(WcError::InvalidSymKey);
    }
    let mut out = format!(
        "wc:{}@2?relay-protocol={}&symKey={}",
        uri.topic, uri.relay_protocol, uri.sym_key
    );
    if let Some(rd) = &uri.relay_data {
        out.push_str("&relay-data=");
        out.push_str(rd);
    }
    if let Some(ts) = uri.expiry_timestamp {
        out.push_str(&format!("&expiryTimestamp={ts}"));
    }
    if !uri.methods.is_empty() {
        out.push_str("&methods=[");
        for (i, m) in uri.methods.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push('"');
            out.push_str(m);
            out.push('"');
        }
        out.push(']');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYM_KEY: &str = "0101010101010101010101010101010101010101010101010101010101010101";
    const TOPIC: &str = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

    #[test]
    fn parses_minimal_v2_uri() {
        let uri = format!("wc:{TOPIC}@2?relay-protocol=irn&symKey={SYM_KEY}");
        let parsed = parse_wc_uri(&uri).unwrap();
        assert_eq!(parsed.topic, TOPIC);
        assert_eq!(parsed.relay_protocol, "irn");
        assert_eq!(parsed.sym_key, SYM_KEY);
        assert!(parsed.methods.is_empty());
    }

    #[test]
    fn parses_full_v2_uri_with_methods_and_expiry() {
        let uri = format!(
            "wc:{TOPIC}@2?relay-protocol=irn&symKey={SYM_KEY}&expiryTimestamp=1700000000&methods=[\"eth_sendTransaction\",\"personal_sign\"]"
        );
        let parsed = parse_wc_uri(&uri).unwrap();
        assert_eq!(parsed.expiry_timestamp, Some(1_700_000_000));
        assert_eq!(parsed.methods, vec!["eth_sendTransaction", "personal_sign"]);
    }

    #[test]
    fn accepts_camelcase_relay_protocol() {
        let uri = format!("wc:{TOPIC}@2?relayProtocol=irn&symKey={SYM_KEY}");
        let parsed = parse_wc_uri(&uri).unwrap();
        assert_eq!(parsed.relay_protocol, "irn");
    }

    #[test]
    fn rejects_v1() {
        let uri = format!("wc:{TOPIC}@1?bridge=https://x&key={SYM_KEY}");
        let err = parse_wc_uri(&uri).unwrap_err();
        assert!(matches!(err, WcError::UnsupportedVersion(_)));
    }

    #[test]
    fn rejects_wrong_scheme() {
        assert!(matches!(
            parse_wc_uri("ws://foo").unwrap_err(),
            WcError::InvalidUri(_)
        ));
    }

    #[test]
    fn rejects_missing_sym_key() {
        let uri = format!("wc:{TOPIC}@2?relay-protocol=irn");
        let err = parse_wc_uri(&uri).unwrap_err();
        assert!(matches!(err, WcError::MissingParam("symKey")));
    }

    #[test]
    fn rejects_short_sym_key() {
        let uri = format!("wc:{TOPIC}@2?relay-protocol=irn&symKey=0011");
        let err = parse_wc_uri(&uri).unwrap_err();
        assert!(matches!(err, WcError::InvalidSymKey));
    }

    #[test]
    fn rejects_missing_query() {
        let err = parse_wc_uri(&format!("wc:{TOPIC}@2")).unwrap_err();
        assert!(matches!(err, WcError::InvalidUri(_)));
    }

    #[test]
    fn rejects_missing_version_anchor() {
        let err =
            parse_wc_uri(&format!("wc:{TOPIC}?relay-protocol=irn&symKey={SYM_KEY}")).unwrap_err();
        assert!(matches!(err, WcError::InvalidUri(_)));
    }

    #[test]
    fn build_round_trips_through_parse() {
        let original = WcUri {
            topic: TOPIC.to_string(),
            relay_protocol: "irn".to_string(),
            relay_data: None,
            sym_key: SYM_KEY.to_string(),
            expiry_timestamp: Some(42),
            methods: vec!["eth_sign".to_string(), "eth_chainId".to_string()],
        };
        let uri = build_wc_uri(&original).unwrap();
        let reparsed = parse_wc_uri(&uri).unwrap();
        assert_eq!(reparsed, original);
    }

    #[test]
    fn build_rejects_bad_sym_key() {
        let bad = WcUri {
            topic: TOPIC.to_string(),
            relay_protocol: "irn".to_string(),
            relay_data: None,
            sym_key: "deadbeef".to_string(),
            expiry_timestamp: None,
            methods: vec![],
        };
        assert!(matches!(build_wc_uri(&bad), Err(WcError::InvalidSymKey)));
    }
}
