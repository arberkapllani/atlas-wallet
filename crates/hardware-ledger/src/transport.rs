//! Transport abstraction. The platform layer (HID on desktop,
//! WebHID on browser, BLE for Nano X) implements [`Transport`];
//! this crate is unaware of the actual wire.

use crate::apdu::{ApduCommand, ApduResponse};
use async_trait::async_trait;
use std::sync::Mutex;
use thiserror::Error;

/// Transport-level errors.
#[derive(Debug, Error)]
pub enum TransportError {
    #[error("device not found")]
    DeviceNotFound,
    #[error("I/O error: {0}")]
    Io(String),
    #[error("decode error: {0}")]
    Decode(String),
}

/// One round-trip transport for APDUs.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Send `cmd` and wait for the response. Implementations are
    /// expected to chunk according to their wire (HID frames are
    /// typically 64 bytes), but the caller works in whole APDUs.
    async fn exchange(&self, cmd: &ApduCommand) -> Result<ApduResponse, TransportError>;
}

/// In-memory transport for unit tests. Records every command and
/// returns canned responses in FIFO order.
pub struct MockTransport {
    queue: Mutex<Vec<Result<ApduResponse, TransportError>>>,
    history: Mutex<Vec<ApduCommand>>,
}

impl MockTransport {
    /// Build a mock that will return `responses` in order.
    pub fn new(responses: Vec<Result<ApduResponse, TransportError>>) -> Self {
        Self {
            queue: Mutex::new(responses.into_iter().rev().collect()),
            history: Mutex::new(Vec::new()),
        }
    }

    /// Snapshot of every APDU the SUT sent. Cloned so callers
    /// don't have to fight the mutex.
    pub fn history(&self) -> Vec<ApduCommand> {
        self.history.lock().expect("mutex").clone()
    }
}

#[async_trait]
impl Transport for MockTransport {
    async fn exchange(&self, cmd: &ApduCommand) -> Result<ApduResponse, TransportError> {
        self.history.lock().expect("mutex").push(cmd.clone());
        self.queue
            .lock()
            .expect("mutex")
            .pop()
            .unwrap_or_else(|| Err(TransportError::Io("mock queue exhausted".into())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apdu::StatusWord;

    #[tokio::test(flavor = "current_thread")]
    async fn mock_returns_canned_response_and_records_command() {
        let resp = ApduResponse {
            data: vec![0xDE, 0xAD],
            sw: StatusWord::Ok,
        };
        let t = MockTransport::new(vec![Ok(resp.clone())]);
        let cmd = ApduCommand::new(0xE0, 0x02, 0, 0, vec![]).unwrap();
        let got = t.exchange(&cmd).await.unwrap();
        assert_eq!(got, resp);
        assert_eq!(t.history(), vec![cmd]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_exhaustion_yields_error() {
        let t = MockTransport::new(vec![]);
        let cmd = ApduCommand::new(0xE0, 0x02, 0, 0, vec![]).unwrap();
        let err = t.exchange(&cmd).await.unwrap_err();
        assert!(matches!(err, TransportError::Io(_)));
    }
}
