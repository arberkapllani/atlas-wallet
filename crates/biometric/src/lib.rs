//! Cross-platform biometric prompt for Atlas (Windows Hello,
//! macOS Touch ID, generic stub).
//!
//! Atlas never *replaces* the master password with biometrics —
//! biometrics gate **access** to a previously-unlocked seed cached
//! in memory. The flow is:
//!
//! 1. User unlocks Atlas with their master password (Argon2id).
//! 2. Atlas asks the OS to enroll a biometric protector.
//! 3. On subsequent unlocks within the auto-lock window, Atlas
//!    asks the OS for a biometric confirmation; on success it
//!    re-uses the cached seed without re-prompting for the
//!    password.
//!
//! Because the password remains the source of truth, losing or
//! resetting biometrics never locks the user out of their seed.
//!
//! This crate currently ships a `StubProvider` that always returns
//! `Unsupported`. Real Windows Hello / Touch ID providers will be
//! implemented in follow-up tasks once the IPC + UI flow lands.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

use async_trait::async_trait;

/// Reasons a biometric prompt may fail.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BiometricError {
    /// Platform / hardware does not expose a biometric API.
    #[error("biometric authentication is not available on this platform")]
    Unsupported,
    /// The OS reports the user cancelled the prompt.
    #[error("user cancelled the biometric prompt")]
    Cancelled,
    /// The OS reports the biometric check failed (e.g. wrong fingerprint).
    #[error("biometric authentication failed")]
    Failed,
    /// Underlying OS API returned an error string.
    #[error("biometric platform error: {0}")]
    Platform(String),
}

/// Outcome of `prompt`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BiometricOutcome {
    /// User confirmed identity with biometrics.
    Confirmed,
    /// Caller should fall back to password.
    Fallback,
}

/// A platform-specific biometric provider.
#[async_trait]
pub trait BiometricProvider: Send + Sync {
    /// `true` iff the underlying platform reports a biometric
    /// sensor is enrolled and reachable.
    async fn is_available(&self) -> bool;

    /// Show a biometric prompt with `reason` displayed to the
    /// user. Implementations must respect the user's choice to
    /// fall back to password (return `Ok(Fallback)`).
    async fn prompt(&self, reason: &str) -> Result<BiometricOutcome, BiometricError>;
}

/// Default provider used when no platform implementation is wired
/// in. Always reports `Unsupported`. Useful in tests and as a
/// safe baseline on platforms we haven't audited yet.
#[derive(Debug, Default, Clone, Copy)]
pub struct StubProvider;

#[async_trait]
impl BiometricProvider for StubProvider {
    async fn is_available(&self) -> bool {
        false
    }

    async fn prompt(&self, _reason: &str) -> Result<BiometricOutcome, BiometricError> {
        Err(BiometricError::Unsupported)
    }
}

/// Returns the best provider available for the current build.
/// Today this is always the stub; future builds will swap in
/// `windows::HelloProvider` on Windows and `macos::TouchIdProvider`
/// on macOS behind `cfg` gates.
pub fn default_provider() -> Box<dyn BiometricProvider> {
    Box::new(StubProvider)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn stub_reports_unavailable() {
        let p = StubProvider;
        assert!(!p.is_available().await);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stub_prompt_returns_unsupported() {
        let p = StubProvider;
        let err = p.prompt("Unlock Atlas").await.unwrap_err();
        assert_eq!(err, BiometricError::Unsupported);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn default_provider_is_stub_today() {
        let p = default_provider();
        assert!(!p.is_available().await);
    }
}
