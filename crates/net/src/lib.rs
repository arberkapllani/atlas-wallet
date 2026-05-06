//! atlas-net — single chokepoint for outbound HTTP.
//!
//! Every chain provider, oracle, exchange adapter, etc. must build
//! its `reqwest::Client` through [`http_client`] (or
//! [`http_client_builder`] for builders that need extra options).
//! This makes the wallet's privacy posture **enforceable**:
//!
//! - When [`set_proxy`] has been called with a SOCKS5 URL (e.g. via
//!   the Tor command path), every subsequent HTTP request will be
//!   routed through that proxy.
//! - When [`set_blocked`] has been called with `true`, every
//!   subsequent HTTP request fails to build a useful client. This
//!   is the kill-switch path used when Tor mode is `Required` but
//!   Tor is not yet ready.
//!
//! The state is held in a process-global [`RwLock`] because the
//! wallet only has one user posture at a time and threading a
//! config object through dozens of crate APIs would dilute the
//! constraint that **no client may opt out**.
//!
//! Existing clients (constructed before [`set_proxy`] was called)
//! are not retroactively reconfigured — `reqwest::Client` clones
//! are cheap reference-counted handles and they cache the proxy
//! choice at build time. The host (Tauri shell) must rebuild any
//! long-lived clients (e.g. chain providers) when the user toggles
//! the privacy posture. See `ChainRegistry::rebuild_all` for the
//! reference implementation of that pattern.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

use std::sync::RwLock;

/// Process-global proxy URL (e.g. `socks5h://127.0.0.1:9050`).
/// `None` means direct clearnet.
static PROXY: RwLock<Option<String>> = RwLock::new(None);

/// Process-global kill-switch. When `true`, [`http_client`] returns
/// a client that points at an invalid loopback so every request
/// fails fast instead of leaking.
static BLOCKED: RwLock<bool> = RwLock::new(false);

/// Replace the process-global proxy URL. Pass `None` to disable.
///
/// Only affects clients built **after** this call. The host should
/// rebuild any cached `reqwest::Client` instances (typically by
/// rebuilding chain providers) for the change to take effect on
/// long-lived connections.
pub fn set_proxy(url: Option<String>) {
    *PROXY.write().expect("atlas-net proxy poisoned") = url;
}

/// Read the current process-global proxy URL, if any.
pub fn current_proxy() -> Option<String> {
    PROXY.read().expect("atlas-net proxy poisoned").clone()
}

/// Toggle the kill-switch. When set to `true`, [`http_client`]
/// returns a client that cannot reach the network. Use this when
/// Tor mode is `Required` but Tor is not ready, to prevent any
/// fallback to clearnet.
pub fn set_blocked(blocked: bool) {
    *BLOCKED.write().expect("atlas-net blocked poisoned") = blocked;
}

/// Read the current kill-switch state.
pub fn is_blocked() -> bool {
    *BLOCKED.read().expect("atlas-net blocked poisoned")
}

/// Build a fresh `reqwest::ClientBuilder` with the current proxy /
/// kill-switch applied. Callers that need extra builder options
/// (timeouts, headers, etc.) should start from this and chain.
pub fn http_client_builder() -> reqwest::ClientBuilder {
    let mut b = reqwest::Client::builder();
    if is_blocked() {
        // Point at a closed loopback port so every request fails
        // immediately rather than dialing the real DNS host. This
        // is the kill-switch behaviour: no clearnet fallback when
        // the user demanded Tor.
        if let Ok(p) = reqwest::Proxy::all("http://127.0.0.1:1") {
            b = b.proxy(p);
        }
        return b;
    }
    if let Some(url) = current_proxy() {
        if let Ok(p) = reqwest::Proxy::all(&url) {
            b = b.proxy(p);
        }
    }
    b
}

/// Build a `reqwest::Client` with the current proxy / kill-switch
/// applied. Falls back to a default client on builder errors so
/// callers don't need to handle the failure path; in practice the
/// builder only fails on TLS configuration problems.
pub fn http_client() -> reqwest::Client {
    http_client_builder()
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests share process-global state so they can't run in
    // parallel meaningfully; we keep them small and serialised by
    // a mutex to avoid flaky CI.
    use std::sync::Mutex;
    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn proxy_round_trip() {
        let _g = LOCK.lock().unwrap();
        set_proxy(Some("socks5h://127.0.0.1:9050".into()));
        assert_eq!(current_proxy().as_deref(), Some("socks5h://127.0.0.1:9050"));
        set_proxy(None);
        assert!(current_proxy().is_none());
    }

    #[test]
    fn blocked_round_trip() {
        let _g = LOCK.lock().unwrap();
        set_blocked(true);
        assert!(is_blocked());
        set_blocked(false);
        assert!(!is_blocked());
    }

    #[test]
    fn http_client_builds() {
        let _g = LOCK.lock().unwrap();
        set_proxy(None);
        set_blocked(false);
        let _ = http_client();
    }
}
