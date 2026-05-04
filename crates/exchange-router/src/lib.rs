//! Multi-source swap quote router.
//!
//! Atlas talks to four heterogeneous swap providers:
//!
//! | Provider   | Same-chain | Cross-chain | Output unit       |
//! |------------|------------|-------------|-------------------|
//! | 1inch      | EVM only   | no          | base-units (str)  |
//! | Jupiter    | Solana     | no          | base-units (str)  |
//! | THORChain  | no         | yes         | 1e8 fixed         |
//! | ChangeNOW  | no         | yes         | decimal display   |
//!
//! Rather than make the UI translate units four different ways,
//! this router takes a normalized [`RoutingRequest`] (display-unit
//! decimal amount) and returns ranked [`RoutedQuote`]s where every
//! output is in the destination RoutingAsset's *display* unit (decimal
//! string). Provider-specific opaque payloads are kept under
//! [`RoutedQuote::raw`] so the wallet can build the deposit /
//! swap transaction afterwards.
//!
//! The router fails open: each provider's failure becomes a
//! rejected entry instead of poisoning the whole call. The UI
//! shows "1inch: insufficient liquidity" alongside successful
//! quotes from other providers.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

use serde::{Deserialize, Serialize};

/// RoutingAsset reference. Atlas-wallet identifies assets by an
/// `(chain, ticker)` pair plus an optional contract address for
/// EVM tokens / Solana SPL mints.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct RoutingAsset {
    /// Chain id in Atlas's internal scheme (e.g. `"bitcoin"`,
    /// `"ethereum"`, `"solana"`, `"bsc"`). Lowercase ASCII.
    pub chain: String,
    /// Display ticker, lowercase (e.g. `"btc"`, `"eth"`,
    /// `"usdc"`).
    pub ticker: String,
    /// Optional contract / mint address. Only meaningful for
    /// non-native tokens.
    #[serde(default)]
    pub contract: Option<String>,
    /// Decimal precision (display-unit -> base-units multiplier
    /// is `10^decimals`).
    pub decimals: u8,
}

impl RoutingAsset {
    /// `true` when this RoutingAsset is the chain's native gas token.
    pub fn is_native(&self) -> bool {
        self.contract.is_none()
    }
}

/// Routing request. Amount is given in the *display* unit
/// (e.g. "0.05" BTC, "10.0" USDC) so the UI doesn't have to
/// multiply by `10^decimals` four different ways.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RoutingRequest {
    /// Source RoutingAsset.
    pub from: RoutingAsset,
    /// Destination RoutingAsset.
    pub to: RoutingAsset,
    /// Source amount as a decimal string in display units.
    pub amount: String,
    /// Slippage tolerance in basis points. Capped per provider.
    pub slippage_bps: u32,
    /// User's destination address on the `to.chain`. Required for
    /// cross-chain providers (THORChain, ChangeNOW); optional for
    /// same-chain ones (1inch, Jupiter) which embed the user's
    /// address at swap-tx build time.
    #[serde(default)]
    pub destination_address: Option<String>,
}

/// One routed quote.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RoutedQuote {
    /// Provider name (`"oneinch"`, `"jupiter"`, `"thorchain"`,
    /// `"changenow"`).
    pub provider: String,
    /// Estimated output amount in display units (decimal string).
    /// `None` when the provider rejected the quote.
    pub out_amount: Option<String>,
    /// Whether this is a same-chain or cross-chain route. Useful
    /// for the UI to surface "you'll wait ~10 min" warnings on
    /// cross-chain routes.
    pub cross_chain: bool,
    /// Provider error message, when the quote failed.
    #[serde(default)]
    pub error: Option<String>,
    /// Provider-specific opaque payload; passed back verbatim
    /// when the user picks this route. Surfaced as a JSON string
    /// in TS bindings.
    #[serde(default)]
    #[specta(type = Option<String>)]
    pub raw: Option<serde_json::Value>,
}

/// Result of routing: a list of quotes, sorted with the best
/// (largest `out_amount`) first; failures sink to the bottom.
pub fn rank(mut quotes: Vec<RoutedQuote>) -> Vec<RoutedQuote> {
    quotes.sort_by(|a, b| {
        let av = parse_decimal(a.out_amount.as_deref().unwrap_or("0"));
        let bv = parse_decimal(b.out_amount.as_deref().unwrap_or("0"));
        // Reverse: bigger first. Failures (None -> 0) end up at
        // the bottom naturally.
        bv.partial_cmp(&av).unwrap_or(std::cmp::Ordering::Equal)
    });
    quotes
}

/// Parse a decimal string into f64 for *ranking only*.
/// Precision loss is irrelevant when comparing two quotes that
/// disagree by more than ~15 significant digits.
fn parse_decimal(s: &str) -> f64 {
    s.parse::<f64>().unwrap_or(0.0)
}

/// Multiply a display-unit decimal string by `10^decimals` and
/// return the integer base-units string. Returns `None` if the
/// input is malformed or has more fractional digits than the
/// RoutingAsset supports.
pub fn to_base_units(amount: &str, decimals: u8) -> Option<String> {
    let s = amount.trim();
    if s.is_empty() {
        return None;
    }
    let (int_part, frac_part) = match s.split_once('.') {
        Some((i, f)) => (i, f),
        None => (s, ""),
    };
    if !int_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if !frac_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if frac_part.len() > decimals as usize {
        return None;
    }
    let mut combined = String::with_capacity(int_part.len() + decimals as usize);
    combined.push_str(int_part);
    combined.push_str(frac_part);
    for _ in 0..(decimals as usize - frac_part.len()) {
        combined.push('0');
    }
    let trimmed = combined.trim_start_matches('0');
    Some(if trimmed.is_empty() {
        "0".into()
    } else {
        trimmed.into()
    })
}

/// Inverse of [`to_base_units`].
pub fn from_base_units(base_units: &str, decimals: u8) -> Option<String> {
    let s = base_units.trim();
    if !s.chars().all(|c| c.is_ascii_digit()) || s.is_empty() {
        return None;
    }
    let d = decimals as usize;
    if s.len() <= d {
        let pad = d - s.len();
        let mut out = String::from("0.");
        for _ in 0..pad {
            out.push('0');
        }
        out.push_str(s);
        Some(out.trim_end_matches('0').trim_end_matches('.').to_string()).map(|t| {
            if t.is_empty() {
                "0".into()
            } else {
                t
            }
        })
    } else {
        let split = s.len() - d;
        let (int_part, frac_part) = s.split_at(split);
        if frac_part.is_empty() {
            Some(int_part.into())
        } else {
            let mut out = String::from(int_part);
            out.push('.');
            out.push_str(frac_part);
            Some(
                out.trim_end_matches('0')
                    .trim_end_matches('.')
                    .trim_end_matches('.')
                    .to_string(),
            )
            .map(|t| if t.is_empty() { "0".into() } else { t })
        }
    }
}

/// Single dispatch: query every applicable provider in parallel
/// and return the ranked list. Providers that don't apply
/// (e.g. 1inch on a Bitcoin pair) are silently skipped.
pub async fn route(req: &RoutingRequest) -> Vec<RoutedQuote> {
    let same_chain = req.from.chain == req.to.chain;

    let mut results: Vec<RoutedQuote> = Vec::new();

    // Same-chain providers.
    if same_chain {
        match req.from.chain.as_str() {
            "ethereum" | "bsc" | "polygon" | "arbitrum" | "optimism" | "base" | "avalanche" => {
                results.push(quote_oneinch(req).await);
            }
            "solana" => {
                results.push(quote_jupiter(req).await);
            }
            _ => {}
        }
    } else {
        // Cross-chain providers — run both in parallel via tokio::join!.
        let (thor, change) = tokio::join!(quote_thorchain(req), quote_changenow(req));
        results.push(thor);
        results.push(change);
    }

    rank(results)
}

async fn quote_oneinch(req: &RoutingRequest) -> RoutedQuote {
    // 1inch's atlas-side wrapper is an HTTP client; the router
    // can't sensibly call it without an api-key + chain-id +
    // base-url tuple, all of which live in atlas-desktop's
    // settings. Surface a stub success-shape here that the
    // desktop crate will replace with a real call site. We
    // emit a deliberate "unconfigured" failure so the rank()
    // sort treats this as a non-quote.
    let _ = req;
    RoutedQuote {
        provider: "oneinch".into(),
        out_amount: None,
        cross_chain: false,
        error: Some("oneinch routing requires settings configured by atlas-desktop".into()),
        raw: None,
    }
}

async fn quote_jupiter(req: &RoutingRequest) -> RoutedQuote {
    // Jupiter requires SPL mint addresses, not tickers. The
    // router doesn't ship a ticker -> mint registry; the desktop
    // crate's token-registry wires that up.
    let mint_in = match &req.from.contract {
        Some(c) => c.clone(),
        None => return failed("jupiter", false, "input mint address missing"),
    };
    let mint_out = match &req.to.contract {
        Some(c) => c.clone(),
        None => return failed("jupiter", false, "output mint address missing"),
    };
    let amount_base = match to_base_units(&req.amount, req.from.decimals) {
        Some(v) => v,
        None => return failed("jupiter", false, "invalid amount"),
    };
    let client = atlas_exchange_jupiter::JupiterClient::new();
    let r = atlas_exchange_jupiter::QuoteRequest {
        input_mint: mint_in,
        output_mint: mint_out,
        amount: amount_base,
        slippage_bps: req.slippage_bps.min(5_000),
    };
    match client.quote(&r).await {
        Ok(q) => RoutedQuote {
            out_amount: from_base_units(&q.out_amount, req.to.decimals),
            raw: serde_json::to_value(&q).ok(),
            provider: "jupiter".into(),
            cross_chain: false,
            error: None,
        },
        Err(e) => failed("jupiter", false, &e.to_string()),
    }
}

async fn quote_thorchain(req: &RoutingRequest) -> RoutedQuote {
    let dest = match &req.destination_address {
        Some(d) => d.clone(),
        None => return failed("thorchain", true, "destination address required"),
    };
    let from_asset = thor_asset(&req.from);
    let to_asset = thor_asset(&req.to);
    // THORChain expects 1e8 fixed-point regardless of the RoutingAsset's
    // native precision. We convert display -> 1e8.
    let amount_1e8 = match to_base_units(&req.amount, 8) {
        Some(v) => v,
        None => return failed("thorchain", true, "invalid amount"),
    };
    let client = atlas_exchange_thorchain::ThorchainClient::new();
    let r = atlas_exchange_thorchain::ThorchainQuoteRequest {
        from_asset,
        to_asset,
        amount: amount_1e8,
        destination: dest,
        affiliate: None,
        affiliate_bps: None,
        min_amount_out: None,
    };
    match client.quote(&r).await {
        Ok(q) => RoutedQuote {
            out_amount: from_base_units(&q.expected_amount_out, 8),
            raw: serde_json::to_value(&q).ok(),
            provider: "thorchain".into(),
            cross_chain: true,
            error: None,
        },
        Err(e) => failed("thorchain", true, &e.to_string()),
    }
}

fn thor_asset(a: &RoutingAsset) -> String {
    // THORChain notation: CHAIN.TICKER for native, CHAIN.TICKER-CONTRACT for EVM tokens.
    let chain = match a.chain.as_str() {
        "bitcoin" => "BTC",
        "ethereum" => "ETH",
        "bsc" => "BSC",
        "avalanche" => "AVAX",
        "litecoin" => "LTC",
        "dogecoin" => "DOGE",
        "bitcoin-cash" => "BCH",
        "cosmos" => "GAIA",
        other => other,
    };
    let ticker = a.ticker.to_uppercase();
    match &a.contract {
        Some(c) => format!("{chain}.{ticker}-{c}"),
        None => format!("{chain}.{ticker}"),
    }
}

async fn quote_changenow(req: &RoutingRequest) -> RoutedQuote {
    let client = atlas_exchange_changenow::ChangeNowClient::new();
    let r = atlas_exchange_changenow::EstimateRequest {
        from_currency: req.from.ticker.to_lowercase(),
        to_currency: req.to.ticker.to_lowercase(),
        from_network: Some(cn_network(&req.from.chain)),
        to_network: Some(cn_network(&req.to.chain)),
        from_amount: req.amount.clone(),
        flow: "standard".into(),
    };
    match client.estimate(&r).await {
        Ok(e) => RoutedQuote {
            out_amount: e.to_amount.clone(),
            raw: serde_json::to_value(&e).ok(),
            provider: "changenow".into(),
            cross_chain: true,
            error: None,
        },
        Err(e) => failed("changenow", true, &e.to_string()),
    }
}

fn cn_network(chain: &str) -> String {
    match chain {
        "bitcoin" => "btc",
        "ethereum" => "eth",
        "bsc" => "bsc",
        "polygon" => "matic",
        "arbitrum" => "arbitrum",
        "optimism" => "op",
        "base" => "base",
        "avalanche" => "cchain",
        "solana" => "sol",
        "litecoin" => "ltc",
        "dogecoin" => "doge",
        other => other,
    }
    .to_string()
}

fn failed(provider: &str, cross_chain: bool, err: &str) -> RoutedQuote {
    RoutedQuote {
        provider: provider.into(),
        out_amount: None,
        cross_chain,
        error: Some(err.into()),
        raw: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_base_units_round_trip() {
        assert_eq!(to_base_units("0.05", 8).as_deref(), Some("5000000"));
        assert_eq!(to_base_units("1", 6).as_deref(), Some("1000000"));
        assert_eq!(to_base_units("0", 8).as_deref(), Some("0"));
        // Too-many fractional digits.
        assert_eq!(to_base_units("0.000000001", 8), None);
        assert_eq!(to_base_units("not-a-number", 8), None);
    }

    #[test]
    fn from_base_units_round_trip() {
        assert_eq!(from_base_units("5000000", 8).as_deref(), Some("0.05"));
        assert_eq!(from_base_units("1000000", 6).as_deref(), Some("1"));
        assert_eq!(from_base_units("0", 8).as_deref(), Some("0"));
    }

    #[test]
    fn rank_orders_by_out_amount() {
        let q = vec![
            RoutedQuote {
                provider: "a".into(),
                out_amount: Some("1.0".into()),
                cross_chain: false,
                error: None,
                raw: None,
            },
            RoutedQuote {
                provider: "b".into(),
                out_amount: Some("3.0".into()),
                cross_chain: false,
                error: None,
                raw: None,
            },
            RoutedQuote {
                provider: "c".into(),
                out_amount: None,
                cross_chain: false,
                error: Some("rip".into()),
                raw: None,
            },
            RoutedQuote {
                provider: "d".into(),
                out_amount: Some("2.0".into()),
                cross_chain: false,
                error: None,
                raw: None,
            },
        ];
        let r = rank(q);
        assert_eq!(r[0].provider, "b");
        assert_eq!(r[1].provider, "d");
        assert_eq!(r[2].provider, "a");
        assert_eq!(r[3].provider, "c");
    }

    #[test]
    fn thor_asset_formats() {
        let a = RoutingAsset {
            chain: "bitcoin".into(),
            ticker: "btc".into(),
            contract: None,
            decimals: 8,
        };
        assert_eq!(thor_asset(&a), "BTC.BTC");

        let usdc = RoutingAsset {
            chain: "ethereum".into(),
            ticker: "usdc".into(),
            contract: Some("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".into()),
            decimals: 6,
        };
        assert_eq!(
            thor_asset(&usdc),
            "ETH.USDC-0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
        );
    }
}
