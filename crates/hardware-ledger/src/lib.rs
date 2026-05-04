//! Hardware-wallet support for **Ledger Nano S / S+ / X**.
//!
//! Ledger devices speak [APDU] over a transport (HID, BLE, U2F).
//! Each application running on the device (Bitcoin, Ethereum,
//! Cosmos, …) defines its own command set on top of that base
//! protocol.
//!
//! This crate is split into three layers:
//!
//! 1. [`apdu`] — pure-data APDU encoder / decoder. No I/O.
//! 2. [`transport`] — the [`Transport`] trait that platform code
//!    (hidapi on desktop, WebHID on browser, etc.) implements.
//! 3. [`apps`] — high-level helpers per Ledger app, currently
//!    Ethereum and Bitcoin.
//!
//! The platform transport is intentionally **not** part of this
//! crate so that the protocol layer compiles + runs on every CI
//! target (including `cargo test` without a device attached).
//!
//! [APDU]: https://en.wikipedia.org/wiki/Smart_card_application_protocol_data_unit

#![forbid(unsafe_code)]

pub mod apdu;
pub mod apps;
pub mod path;
pub mod transport;

pub use apdu::{ApduCommand, ApduResponse, StatusWord};
pub use path::{Bip32Path, PathError};
pub use transport::{MockTransport, Transport, TransportError};

/// Re-export the recognised Ledger app cookies so caller code can
/// be explicit (`apps::eth::CLA` etc.) without magic numbers.
pub mod cla {
    /// Ethereum app CLA byte.
    pub const ETH: u8 = 0xE0;
    /// Bitcoin app CLA byte.
    pub const BTC: u8 = 0xE0;
}
