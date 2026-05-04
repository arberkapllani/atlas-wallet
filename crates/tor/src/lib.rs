//! Tor proxy policy + kill-switch.
//!
//! Atlas routes every outbound network request through Tor by default
//! (mode = `Required`). If Tor is not yet bootstrapped, requests are
//! **blocked** rather than falling back to clearnet — this is the
//! kill-switch. Users may opt down to `Preferred` (best-effort) or
//! `Disabled` (clearnet allowed) for environments where Tor is
//! impractical, but the default is the most private posture.
//!
//! This crate is **offline-pure**: it owns the policy types, the
//! enforcement function, the [`TorProvider`] trait, and a
//! [`StubProvider`] used in tests and as the safe default until a
//! real `arti-client`-backed provider is wired in.
//!
//! The host (Tauri shell) calls [`enforce`] before every outbound
//! request to decide whether to use a SOCKS5 proxy, go direct, or
//! refuse the request entirely.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// Default loopback address `arti` / `tor` listens on for SOCKS5
/// connections. The host may override via [`TorConfig::socks_addr`].
pub const DEFAULT_SOCKS_ADDR: &str = "127.0.0.1:9050";

/// User-selectable network-privacy posture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum TorMode {
    /// Direct clearnet for every request. **Not recommended.**
    /// Available for users who route Tor at the OS level (e.g. Whonix
    /// gateway) and don't want a second hop, or for offline tests.
    Disabled,
    /// Use Tor when bootstrapped, otherwise fall back to clearnet.
    /// **Leaks IP** during bootstrap and on Tor failures.
    Preferred,
    /// Use Tor when bootstrapped, otherwise **block the request**.
    /// This is the kill-switch posture and the default for new
    /// installs.
    #[default]
    Required,
}

/// Snapshot of the embedded Tor client's connection lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TorStatus {
    /// Tor is not running. Either the user disabled it or it has
    /// been stopped. With [`TorMode::Required`] this means the
    /// kill-switch will block all outbound requests.
    #[default]
    Disabled,
    /// Tor is starting up and downloading directory information.
    /// `progress` is 0..=100. With [`TorMode::Required`] requests
    /// are blocked until bootstrap reaches 100.
    Bootstrapping {
        /// 0-100 inclusive.
        progress: u8,
    },
    /// Tor has at least one usable circuit and accepts SOCKS
    /// connections.
    Ready {
        /// Unix seconds since this Ready state began. Used by the
        /// UI to show "connected for 2h 14m".
        since_unix: u64,
    },
    /// Tor failed to start or lost connectivity. The host should
    /// surface `reason` and offer the user a retry button.
    Failed {
        /// Human-readable failure cause.
        reason: String,
    },
}

/// Configuration handed to a [`TorProvider`] when starting.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct TorConfig {
    /// Loopback `host:port` the SOCKS5 listener binds to. Defaults
    /// to [`DEFAULT_SOCKS_ADDR`].
    pub socks_addr: String,
    /// Optional bridge lines (`obfs4 ...`) for users in censored
    /// networks. Ignored by the stub provider.
    pub bridges: Vec<String>,
}

impl Default for TorConfig {
    fn default() -> Self {
        Self {
            socks_addr: DEFAULT_SOCKS_ADDR.to_string(),
            bridges: Vec::new(),
        }
    }
}

/// Decision returned by [`enforce`] for each outbound request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProxyDecision {
    /// Send the request directly without a proxy. Only emitted when
    /// the user has explicitly chosen [`TorMode::Disabled`] or
    /// [`TorMode::Preferred`] with Tor unavailable.
    Direct,
    /// Send the request through a SOCKS5 proxy at `addr`.
    Socks5 {
        /// `host:port` of the SOCKS5 listener.
        addr: String,
    },
    /// Refuse the request. The kill-switch is active.
    Block {
        /// Human-readable reason the host can surface to the user.
        reason: String,
    },
}

/// Decide how (or whether) an outbound request should be sent.
///
/// This is the single chokepoint every HTTP client in the workspace
/// must consult. It is pure, deterministic and trivially testable.
pub fn enforce(mode: TorMode, status: &TorStatus, config: &TorConfig) -> ProxyDecision {
    match mode {
        TorMode::Disabled => ProxyDecision::Direct,
        TorMode::Preferred => match status {
            TorStatus::Ready { .. } => ProxyDecision::Socks5 {
                addr: config.socks_addr.clone(),
            },
            _ => ProxyDecision::Direct,
        },
        TorMode::Required => match status {
            TorStatus::Ready { .. } => ProxyDecision::Socks5 {
                addr: config.socks_addr.clone(),
            },
            TorStatus::Bootstrapping { progress } => ProxyDecision::Block {
                reason: format!("Tor is still bootstrapping ({progress}%)"),
            },
            TorStatus::Disabled => ProxyDecision::Block {
                reason: "Tor is not running and mode = Required".to_string(),
            },
            TorStatus::Failed { reason } => ProxyDecision::Block {
                reason: format!("Tor failed: {reason}"),
            },
        },
    }
}

/// Errors a [`TorProvider`] may return.
#[derive(Debug, thiserror::Error)]
pub enum TorError {
    /// Provider does not implement Tor on this build/platform.
    #[error("Tor is not available on this build")]
    Unsupported,
    /// Tor was already running when start was called.
    #[error("Tor is already running")]
    AlreadyRunning,
    /// Tor is not running but the operation requires it.
    #[error("Tor is not running")]
    NotRunning,
    /// Underlying transport error.
    #[error("Tor transport error: {0}")]
    Transport(String),
}

/// A platform / runtime backend that owns a Tor client.
#[async_trait]
pub trait TorProvider: Send + Sync {
    /// Start the Tor client with `config`. Idempotent: returns
    /// [`TorError::AlreadyRunning`] if already started.
    async fn start(&self, config: TorConfig) -> Result<TorStatus, TorError>;
    /// Gracefully stop the Tor client. Idempotent: a no-op if not
    /// running.
    async fn stop(&self);
    /// Cheap snapshot of the current status.
    async fn status(&self) -> TorStatus;
    /// Force a brand-new circuit (analogous to "New Identity" in
    /// Tor Browser).
    async fn new_circuit(&self) -> Result<(), TorError>;
}

/// Default provider used when no real Tor backend is wired in.
///
/// Behaves like a deterministic state machine:
/// - `start` jumps directly to [`TorStatus::Ready`].
/// - `stop` jumps back to [`TorStatus::Disabled`].
/// - `new_circuit` is a no-op when running.
///
/// Real builds will replace this with an `arti-client`-backed
/// provider in a follow-up commit. Keeping the stub deterministic
/// makes the kill-switch testable today.
pub struct StubProvider {
    state: Mutex<TorStatus>,
}

impl Default for StubProvider {
    fn default() -> Self {
        Self {
            state: Mutex::new(TorStatus::Disabled),
        }
    }
}

impl StubProvider {
    /// Construct a stub already in [`TorStatus::Ready`].
    /// Convenient for tests that need an active proxy without a
    /// real Tor process.
    pub fn ready() -> Self {
        Self {
            state: Mutex::new(TorStatus::Ready { since_unix: 0 }),
        }
    }
}

#[async_trait]
impl TorProvider for StubProvider {
    async fn start(&self, _config: TorConfig) -> Result<TorStatus, TorError> {
        let mut s = self.state.lock().expect("tor stub poisoned");
        if matches!(*s, TorStatus::Ready { .. }) {
            return Err(TorError::AlreadyRunning);
        }
        *s = TorStatus::Ready { since_unix: 0 };
        Ok(s.clone())
    }

    async fn stop(&self) {
        let mut s = self.state.lock().expect("tor stub poisoned");
        *s = TorStatus::Disabled;
    }

    async fn status(&self) -> TorStatus {
        self.state.lock().expect("tor stub poisoned").clone()
    }

    async fn new_circuit(&self) -> Result<(), TorError> {
        let s = self.state.lock().expect("tor stub poisoned");
        match *s {
            TorStatus::Ready { .. } => Ok(()),
            _ => Err(TorError::NotRunning),
        }
    }
}

/// Returns the best provider available for the current build.
/// Today this is always [`StubProvider`]; future builds will swap
/// in an `arti-client`-backed provider behind a `cfg`/feature gate.
pub fn default_provider() -> std::sync::Arc<dyn TorProvider> {
    std::sync::Arc::new(StubProvider::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> TorConfig {
        TorConfig::default()
    }

    #[test]
    fn enforce_disabled_always_direct() {
        let c = cfg();
        for s in [
            TorStatus::Disabled,
            TorStatus::Bootstrapping { progress: 50 },
            TorStatus::Ready { since_unix: 0 },
            TorStatus::Failed { reason: "x".into() },
        ] {
            assert_eq!(enforce(TorMode::Disabled, &s, &c), ProxyDecision::Direct);
        }
    }

    #[test]
    fn enforce_preferred_uses_proxy_when_ready_else_direct() {
        let c = cfg();
        assert_eq!(
            enforce(TorMode::Preferred, &TorStatus::Ready { since_unix: 0 }, &c),
            ProxyDecision::Socks5 {
                addr: DEFAULT_SOCKS_ADDR.into()
            }
        );
        assert_eq!(
            enforce(TorMode::Preferred, &TorStatus::Disabled, &c),
            ProxyDecision::Direct
        );
        assert_eq!(
            enforce(
                TorMode::Preferred,
                &TorStatus::Bootstrapping { progress: 10 },
                &c
            ),
            ProxyDecision::Direct
        );
    }

    #[test]
    fn enforce_required_blocks_when_not_ready() {
        let c = cfg();
        assert_eq!(
            enforce(TorMode::Required, &TorStatus::Ready { since_unix: 1 }, &c),
            ProxyDecision::Socks5 {
                addr: DEFAULT_SOCKS_ADDR.into()
            }
        );
        assert!(matches!(
            enforce(TorMode::Required, &TorStatus::Disabled, &c),
            ProxyDecision::Block { .. }
        ));
        assert!(matches!(
            enforce(
                TorMode::Required,
                &TorStatus::Bootstrapping { progress: 50 },
                &c
            ),
            ProxyDecision::Block { .. }
        ));
        assert!(matches!(
            enforce(
                TorMode::Required,
                &TorStatus::Failed {
                    reason: "boom".into()
                },
                &c
            ),
            ProxyDecision::Block { .. }
        ));
    }

    #[test]
    fn enforce_uses_custom_socks_addr() {
        let mut c = cfg();
        c.socks_addr = "127.0.0.1:19050".into();
        assert_eq!(
            enforce(TorMode::Required, &TorStatus::Ready { since_unix: 0 }, &c),
            ProxyDecision::Socks5 {
                addr: "127.0.0.1:19050".into()
            }
        );
    }

    #[test]
    fn default_mode_is_required() {
        assert_eq!(TorMode::default(), TorMode::Required);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stub_lifecycle() {
        let p = StubProvider::default();
        assert_eq!(p.status().await, TorStatus::Disabled);
        let s = p.start(TorConfig::default()).await.unwrap();
        assert!(matches!(s, TorStatus::Ready { .. }));
        // Starting again errors
        assert!(p.start(TorConfig::default()).await.is_err());
        p.new_circuit().await.unwrap();
        p.stop().await;
        assert_eq!(p.status().await, TorStatus::Disabled);
        assert!(p.new_circuit().await.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stub_ready_constructor() {
        let p = StubProvider::ready();
        assert!(matches!(p.status().await, TorStatus::Ready { .. }));
    }
}
