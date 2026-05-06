//! atlas-node-config — sovereign node policy.
//!
//! The user's stated direction is: "ki prarasy qe ne cdo opsion apo
//! kod te ketij projekti te jem i lidhur drejtperdrejt me blockchain,
//! m pak fjale mos te jem i varur nga pale te treta." Translated: every
//! piece of the wallet must be able to talk directly to a node the
//! user controls (or one routed through Tor). This crate encodes the
//! policy that decides whether an RPC endpoint URL is acceptable.
//!
//! ## Model
//!
//! - [`NodePolicy`] is the user-controlled gate. It carries:
//!   - `require_local`: when `true`, only loopback (`127.0.0.1`,
//!     `[::1]`, `localhost`) and explicitly trusted hosts are
//!     allowed. Public RPC defaults are refused.
//!   - `allow_tor_onion`: when `true`, `*.onion` hosts are accepted
//!     even under `require_local` (because the user reaches them
//!     through their own Tor circuit, not a third-party API).
//!   - `trusted_hosts`: the user's allow-list of additional hosts
//!     (case-insensitive). Useful for a node on another machine in
//!     the user's LAN, a Tailscale peer, or a friend's node.
//! - [`endpoint_decision`] is the pure function the rest of the
//!   wallet calls. Given a URL string and a policy, it returns
//!   [`NodeDecision::Allow`] or [`NodeDecision::Block`] with a
//!   human-readable reason. The wallet never builds a provider
//!   against a Blocked URL.
//!
//! ## What this crate is *not*
//!
//! This is policy only. It does not start a node, does not validate
//! TLS certificates, and does not check that the node is honest
//! (that is the job of the verifying full-node logic). It exists so
//! the user can flip a single switch and be sure no command path
//! silently dialed `mainnet.infura.io`.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

/// User-controlled policy describing which RPC endpoint URLs the
/// wallet is allowed to dial.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct NodePolicy {
    /// When `true`, only loopback hosts, `*.onion` (if
    /// `allow_tor_onion`), and entries in `trusted_hosts` are
    /// accepted. When `false`, the policy is permissive and only
    /// rejects malformed URLs.
    pub require_local: bool,
    /// When `true`, `*.onion` hosts are accepted under a strict
    /// policy. The user reaches them through their own Tor circuit
    /// so they are not "third parties" in the same sense as a
    /// hosted RPC SaaS.
    pub allow_tor_onion: bool,
    /// User-curated allow-list of additional hosts. Case-insensitive
    /// exact match against the URL's hostname.
    pub trusted_hosts: Vec<String>,
}

impl Default for NodePolicy {
    fn default() -> Self {
        Self {
            // Permissive by default so existing public-RPC defaults
            // keep working; the user opts in to strict mode from the
            // settings card. Once enabled, the policy is sticky.
            require_local: false,
            allow_tor_onion: true,
            trusted_hosts: Vec::new(),
        }
    }
}

/// Outcome of running [`endpoint_decision`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeDecision {
    /// Endpoint is acceptable. Carries the resolved hostname so the
    /// UI can show what the policy matched.
    Allow {
        /// Lower-cased hostname the URL parsed to.
        host: String,
        /// Why it was allowed (loopback / onion / trusted host /
        /// permissive).
        reason: String,
    },
    /// Endpoint is refused; the wallet must not build a provider
    /// against it.
    Block {
        /// Lower-cased hostname the URL parsed to (or the raw URL if
        /// the host could not be extracted).
        host: String,
        /// Human-readable explanation surfaced to the user.
        reason: String,
    },
}

impl NodeDecision {
    /// Convenience for callers that just need a yes/no.
    pub fn is_allowed(&self) -> bool {
        matches!(self, NodeDecision::Allow { .. })
    }
}

/// Errors raised while parsing a candidate URL. These are *not* the
/// same thing as a `Block` decision — a malformed URL is always a
/// hard error regardless of policy.
#[derive(Debug, Clone, Error)]
pub enum NodeConfigError {
    /// The candidate URL did not parse as a valid absolute URL.
    #[error("invalid URL: {0}")]
    Invalid(String),
    /// The URL parsed but had no hostname (e.g. `file:///`).
    #[error("URL has no host")]
    NoHost,
    /// The URL used an unsupported scheme. Only `http`, `https`, and
    /// `ws`/`wss` are accepted for RPC endpoints.
    #[error("unsupported scheme: {0}")]
    UnsupportedScheme(String),
}

const ALLOWED_SCHEMES: &[&str] = &["http", "https", "ws", "wss"];

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1")
}

fn is_onion_host(host: &str) -> bool {
    host.ends_with(".onion")
}

fn matches_trusted(host: &str, trusted: &[String]) -> bool {
    let h = host.to_ascii_lowercase();
    trusted
        .iter()
        .any(|t| t.trim().to_ascii_lowercase() == h && !t.trim().is_empty())
}

/// Parse `url` and return the policy decision. Malformed URLs and
/// unsupported schemes always error rather than returning a
/// `Block` decision so the UI can show a precise validation error
/// to the user.
pub fn endpoint_decision(url: &str, policy: &NodePolicy) -> Result<NodeDecision, NodeConfigError> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(NodeConfigError::Invalid("empty URL".into()));
    }
    let parsed = Url::parse(trimmed).map_err(|e| NodeConfigError::Invalid(e.to_string()))?;
    let scheme = parsed.scheme().to_ascii_lowercase();
    if !ALLOWED_SCHEMES.contains(&scheme.as_str()) {
        return Err(NodeConfigError::UnsupportedScheme(scheme));
    }
    let host = parsed
        .host_str()
        .ok_or(NodeConfigError::NoHost)?
        .to_ascii_lowercase();

    if !policy.require_local {
        return Ok(NodeDecision::Allow {
            host,
            reason: "permissive policy (require_local = false)".into(),
        });
    }

    if is_loopback_host(&host) {
        return Ok(NodeDecision::Allow {
            host,
            reason: "loopback host".into(),
        });
    }

    if policy.allow_tor_onion && is_onion_host(&host) {
        return Ok(NodeDecision::Allow {
            host,
            reason: "Tor hidden service".into(),
        });
    }

    if matches_trusted(&host, &policy.trusted_hosts) {
        return Ok(NodeDecision::Allow {
            host,
            reason: "user-trusted host".into(),
        });
    }

    Ok(NodeDecision::Block {
        host,
        reason: "host is not loopback, .onion, or in trusted_hosts".into(),
    })
}

/// Convenience wrapper used by [`NodePolicy::set_trusted_hosts`] —
/// trims, lowercases, dedupes, and removes empties so the persisted
/// list is canonical.
pub fn normalise_hosts(input: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = input
        .into_iter()
        .map(|h| h.trim().to_ascii_lowercase())
        .filter(|h| !h.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

impl NodePolicy {
    /// Replace the trusted-hosts list with a canonicalised version
    /// of `hosts`.
    pub fn set_trusted_hosts(&mut self, hosts: impl IntoIterator<Item = String>) {
        self.trusted_hosts = normalise_hosts(hosts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strict() -> NodePolicy {
        NodePolicy {
            require_local: true,
            allow_tor_onion: true,
            trusted_hosts: vec!["my-node.lan".into()],
        }
    }

    #[test]
    fn permissive_allows_public_rpc() {
        let p = NodePolicy::default();
        let d = endpoint_decision("https://mainnet.infura.io/v3/abc", &p).unwrap();
        assert!(d.is_allowed());
    }

    #[test]
    fn strict_blocks_public_rpc() {
        let d = endpoint_decision("https://mainnet.infura.io/v3/abc", &strict()).unwrap();
        match d {
            NodeDecision::Block { host, .. } => assert_eq!(host, "mainnet.infura.io"),
            _ => panic!("expected Block"),
        }
    }

    #[test]
    fn strict_allows_loopback_localhost() {
        let d = endpoint_decision("http://localhost:8332", &strict()).unwrap();
        assert!(d.is_allowed());
    }

    #[test]
    fn strict_allows_loopback_127() {
        let d = endpoint_decision("http://127.0.0.1:8545", &strict()).unwrap();
        assert!(d.is_allowed());
    }

    #[test]
    fn strict_allows_loopback_v6() {
        let d = endpoint_decision("http://[::1]:8545", &strict()).unwrap();
        assert!(d.is_allowed());
    }

    #[test]
    fn strict_allows_onion_when_flag_on() {
        let d = endpoint_decision(
            "http://exampleabcdefghijklmnopqrstuvwxyz0123456789ab.onion",
            &strict(),
        )
        .unwrap();
        assert!(d.is_allowed());
    }

    #[test]
    fn strict_blocks_onion_when_flag_off() {
        let mut p = strict();
        p.allow_tor_onion = false;
        let d = endpoint_decision(
            "http://exampleabcdefghijklmnopqrstuvwxyz0123456789ab.onion",
            &p,
        )
        .unwrap();
        assert!(!d.is_allowed());
    }

    #[test]
    fn strict_allows_trusted_host_case_insensitive() {
        let d = endpoint_decision("http://MY-NODE.LAN:8545", &strict()).unwrap();
        assert!(d.is_allowed());
    }

    #[test]
    fn strict_blocks_untrusted_host() {
        let d = endpoint_decision("http://other-node.lan:8545", &strict()).unwrap();
        assert!(!d.is_allowed());
    }

    #[test]
    fn empty_url_errors() {
        let err = endpoint_decision("   ", &strict()).unwrap_err();
        assert!(matches!(err, NodeConfigError::Invalid(_)));
    }

    #[test]
    fn malformed_url_errors() {
        let err = endpoint_decision("not a url", &strict()).unwrap_err();
        assert!(matches!(err, NodeConfigError::Invalid(_)));
    }

    #[test]
    fn unsupported_scheme_errors() {
        let err = endpoint_decision("ftp://example.com/", &strict()).unwrap_err();
        assert!(matches!(err, NodeConfigError::UnsupportedScheme(_)));
    }

    #[test]
    fn ws_scheme_allowed_under_loopback() {
        let d = endpoint_decision("ws://127.0.0.1:8546", &strict()).unwrap();
        assert!(d.is_allowed());
    }

    #[test]
    fn wss_scheme_allowed_under_trusted() {
        let d = endpoint_decision("wss://my-node.lan/rpc", &strict()).unwrap();
        assert!(d.is_allowed());
    }

    #[test]
    fn normalise_hosts_dedupes_and_lowercases() {
        let out = normalise_hosts(vec![
            "Foo.LAN".into(),
            "foo.lan".into(),
            "  ".into(),
            "bar".into(),
        ]);
        assert_eq!(out, vec!["bar".to_string(), "foo.lan".to_string()]);
    }

    #[test]
    fn set_trusted_hosts_canonicalises() {
        let mut p = NodePolicy::default();
        p.set_trusted_hosts(vec!["A".into(), "a".into(), "B".into()]);
        assert_eq!(p.trusted_hosts, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn serde_round_trip() {
        let p = strict();
        let json = serde_json::to_string(&p).unwrap();
        let back: NodePolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }
}
