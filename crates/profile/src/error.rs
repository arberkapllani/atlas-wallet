//! Errors raised by the profile registry.

use thiserror::Error;

/// Convenience result alias.
pub type ProfileResult<T> = std::result::Result<T, ProfileError>;

/// Errors from profile registry operations.
#[derive(Debug, Error)]
pub enum ProfileError {
    /// Underlying I/O error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization failure.
    #[error("registry serde error: {0}")]
    Serde(#[from] serde_json::Error),

    /// The supplied profile id was not found in the registry.
    #[error("profile not found: {0}")]
    NotFound(String),

    /// A profile already exists with the same name.
    #[error("profile name already taken: {0}")]
    NameTaken(String),

    /// Watch-only profiles cannot sign transactions.
    #[error("profile is watch-only")]
    WatchOnly,

    /// The user attempted to delete the only remaining profile.
    #[error("cannot delete the only remaining profile")]
    LastProfile,

    /// Validation failed (empty name, etc.).
    #[error("invalid input: {0}")]
    Invalid(String),
}
