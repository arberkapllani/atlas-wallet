//! Price oracle backed by CoinGecko's free public API. 5-minute TTL.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod aggregate;
pub mod coingecko;

pub use aggregate::{aggregate_prices, AggregateError, AggregatedPrice, SourceQuote};
pub use coingecko::{PriceOracle, PricePoint};
