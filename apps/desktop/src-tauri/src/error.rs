//! Tauri command-result error type, serializable to the frontend.

use serde::Serialize;

#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum CmdError {
    #[error("not initialized: {0}")]
    NotInitialized(String),
    #[error("locked")]
    Locked,
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("wallet core: {0}")]
    Wallet(String),
    #[error("chain: {0}")]
    Chain(String),
    #[error("io: {0}")]
    Io(String),
    #[error("profile: {0}")]
    Profile(String),
}

impl From<atlas_wallet_core::Error> for CmdError {
    fn from(e: atlas_wallet_core::Error) -> Self {
        Self::Wallet(e.to_string())
    }
}

impl From<atlas_chain_traits::ChainError> for CmdError {
    fn from(e: atlas_chain_traits::ChainError) -> Self {
        Self::Chain(e.to_string())
    }
}

impl From<atlas_profile::ProfileError> for CmdError {
    fn from(e: atlas_profile::ProfileError) -> Self {
        Self::Profile(e.to_string())
    }
}

impl From<std::io::Error> for CmdError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

pub type CmdResult<T> = std::result::Result<T, CmdError>;

