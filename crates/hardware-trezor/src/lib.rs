//! Hardware-wallet support for **Trezor One / Model T / Safe 3 / Safe 5**.
//!
//! Trezor speaks a binary protocol on top of USB HID (or WebUSB):
//! every message is a Protocol Buffer record framed with a small
//! header, then re-chunked into 64-byte HID frames.
//!
//! This crate exposes the framing + a small typed message catalog
//! we actually use (`GetPublicKey`, `EthereumGetAddress`, …). We
//! deliberately do **not** depend on `prost`/`protobuf-codegen` or
//! pull the full Trezor IDL — Atlas only needs a handful of
//! request types, so we hand-encode them. This keeps the crate
//! lean, deterministic, and testable without device access.
//!
//! Layers:
//!
//! 1. [`framing`] — header + 64-byte HID chunk (de)serialiser.
//! 2. [`messages`] — typed catalogue of the (request, response)
//!    pairs Atlas issues, plus minimal protobuf wire helpers.
//! 3. [`transport`] — async [`Transport`] trait + `MockTransport`.
//! 4. [`apps::eth`] / [`apps::btc`] — high-level helpers.
//!
//! Compatible with Trezor firmware 1.x / 2.x / Suite messaging
//! protocol v1 (the "messages-v1" packet format used by every
//! shipping device today).

#![forbid(unsafe_code)]

pub mod apps;
pub mod framing;
pub mod messages;
pub mod path;
pub mod transport;

pub use framing::{decode_message, encode_message, FramingError, MessageEnvelope};
pub use messages::MessageType;
pub use path::{Bip32Path, PathError};
pub use transport::{MockTransport, Transport, TransportError};
