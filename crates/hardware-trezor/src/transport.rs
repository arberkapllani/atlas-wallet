//! Async transport trait + a `MockTransport` for unit tests.

use crate::framing::MessageEnvelope;
use async_trait::async_trait;
use std::sync::Mutex;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("device not found")]
    DeviceNotFound,
    #[error("I/O error: {0}")]
    Io(String),
    #[error("decode error: {0}")]
    Decode(String),
}

#[async_trait]
pub trait Transport: Send + Sync {
    /// Send `msg` and wait for the reply envelope. Implementations
    /// own the chunking into 64-byte HID frames.
    async fn exchange(&self, msg: &MessageEnvelope) -> Result<MessageEnvelope, TransportError>;
}

/// In-memory transport for unit tests.
pub struct MockTransport {
    queue: Mutex<Vec<Result<MessageEnvelope, TransportError>>>,
    history: Mutex<Vec<MessageEnvelope>>,
}

impl MockTransport {
    pub fn new(responses: Vec<Result<MessageEnvelope, TransportError>>) -> Self {
        Self {
            queue: Mutex::new(responses.into_iter().rev().collect()),
            history: Mutex::new(Vec::new()),
        }
    }

    pub fn history(&self) -> Vec<MessageEnvelope> {
        self.history.lock().expect("mutex").clone()
    }
}

#[async_trait]
impl Transport for MockTransport {
    async fn exchange(&self, msg: &MessageEnvelope) -> Result<MessageEnvelope, TransportError> {
        self.history.lock().expect("mutex").push(msg.clone());
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

    #[tokio::test(flavor = "current_thread")]
    async fn mock_records_outgoing_and_returns_canned() {
        let resp = MessageEnvelope {
            message_type: 2,
            payload: vec![0x10],
        };
        let t = MockTransport::new(vec![Ok(resp.clone())]);
        let req = MessageEnvelope {
            message_type: 1,
            payload: vec![],
        };
        let got = t.exchange(&req).await.unwrap();
        assert_eq!(got, resp);
        assert_eq!(t.history(), vec![req]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_exhaustion_yields_io_error() {
        let t = MockTransport::new(vec![]);
        let req = MessageEnvelope {
            message_type: 1,
            payload: vec![],
        };
        assert!(matches!(
            t.exchange(&req).await.unwrap_err(),
            TransportError::Io(_)
        ));
    }
}
