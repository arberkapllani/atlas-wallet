//! Fiat on-ramp URL builders for MoonPay's hosted Buy widget.
//!
//! Atlas does not custody fiat — instead it builds a sign-able widget URL
//! that lands the user on MoonPay's KYC + payment flow with their wallet
//! address pre-filled. When a `secret_key` is configured the URL is HMAC-
//! SHA256 signed so MoonPay attributes the conversion to the partner.
//!
//! Sandbox vs. production is selected by the `Environment` enum; the host
//! is `buy-sandbox.moonpay.com` for testing and `buy.moonpay.com` for
//! live KYC + card processing.

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

#[derive(Debug, thiserror::Error)]
pub enum OnrampError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

pub type Result<T> = std::result::Result<T, OnrampError>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    Sandbox,
    Production,
}

impl Environment {
    fn host(self) -> &'static str {
        match self {
            Environment::Sandbox => "buy-sandbox.moonpay.com",
            Environment::Production => "buy.moonpay.com",
        }
    }

    fn sell_host(self) -> &'static str {
        match self {
            Environment::Sandbox => "sell-sandbox.moonpay.com",
            Environment::Production => "sell.moonpay.com",
        }
    }
}

/// Configuration for building a MoonPay Buy URL.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct MoonPayBuyParams {
    /// MoonPay-issued public key (`pk_live_…` or `pk_test_…`). Required.
    pub api_key: String,
    /// Lower-case currency code (`btc`, `eth`, `usdc`, …).
    pub currency_code: String,
    /// Wallet address to credit with the purchased crypto.
    pub wallet_address: String,
    /// Fiat amount the user wants to spend (e.g. "100" USD).
    pub base_currency_amount: Option<String>,
    /// Fiat ISO code (`usd`, `eur`, …).
    pub base_currency_code: Option<String>,
    /// Sandbox vs. production.
    pub environment: Environment,
    /// Where MoonPay redirects on success.
    pub redirect_url: Option<String>,
}

/// Build the unsigned widget URL.
///
/// Pass the result to a system browser. If you want MoonPay to attribute
/// the sale to your partner account, pipe this URL through [`sign_url`]
/// with your `sk_…` secret key first.
pub fn build_buy_url(p: &MoonPayBuyParams) -> Result<String> {
    if p.api_key.trim().is_empty() {
        return Err(OnrampError::InvalidInput("api_key required".into()));
    }
    if p.wallet_address.trim().is_empty() {
        return Err(OnrampError::InvalidInput("wallet_address required".into()));
    }
    if p.currency_code.trim().is_empty() {
        return Err(OnrampError::InvalidInput("currency_code required".into()));
    }

    let mut q: Vec<(&str, String)> = vec![
        ("apiKey", p.api_key.trim().to_string()),
        ("currencyCode", p.currency_code.trim().to_lowercase()),
        ("walletAddress", p.wallet_address.trim().to_string()),
    ];
    if let Some(a) = &p.base_currency_amount {
        if !a.trim().is_empty() {
            q.push(("baseCurrencyAmount", a.trim().to_string()));
        }
    }
    if let Some(c) = &p.base_currency_code {
        if !c.trim().is_empty() {
            q.push(("baseCurrencyCode", c.trim().to_lowercase()));
        }
    }
    if let Some(r) = &p.redirect_url {
        if !r.trim().is_empty() {
            q.push(("redirectURL", r.trim().to_string()));
        }
    }

    let qs = q
        .into_iter()
        .map(|(k, v)| format!("{}={}", k, urlencode(&v)))
        .collect::<Vec<_>>()
        .join("&");

    Ok(format!("https://{}?{}", p.environment.host(), qs))
}

/// Parameters for MoonPay's hosted Sell widget. The user lands on
/// MoonPay, sells crypto for fiat (KYC + payout flow), and receives the
/// proceeds via bank transfer / card refund. Atlas only collects the
/// `refund_wallet_address` so failed payouts come back to the user.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct MoonPaySellParams {
    pub api_key: String,
    /// Lower-case crypto code being sold (`btc`, `eth`, `usdc`, ...).
    pub base_currency_code: String,
    /// Wallet address MoonPay refunds the crypto to if the sell fails.
    pub refund_wallet_address: String,
    /// Crypto amount to sell (decimal string).
    pub base_currency_amount: Option<String>,
    /// Fiat ISO code the user wants paid out in (`usd`, `eur`, ...).
    pub quote_currency_code: Option<String>,
    pub environment: Environment,
    pub redirect_url: Option<String>,
}

/// Build the unsigned Sell widget URL.
pub fn build_sell_url(p: &MoonPaySellParams) -> Result<String> {
    if p.api_key.trim().is_empty() {
        return Err(OnrampError::InvalidInput("api_key required".into()));
    }
    if p.refund_wallet_address.trim().is_empty() {
        return Err(OnrampError::InvalidInput(
            "refund_wallet_address required".into(),
        ));
    }
    if p.base_currency_code.trim().is_empty() {
        return Err(OnrampError::InvalidInput(
            "base_currency_code required".into(),
        ));
    }

    let mut q: Vec<(&str, String)> = vec![
        ("apiKey", p.api_key.trim().to_string()),
        (
            "baseCurrencyCode",
            p.base_currency_code.trim().to_lowercase(),
        ),
        (
            "refundWalletAddress",
            p.refund_wallet_address.trim().to_string(),
        ),
    ];
    if let Some(a) = &p.base_currency_amount {
        if !a.trim().is_empty() {
            q.push(("baseCurrencyAmount", a.trim().to_string()));
        }
    }
    if let Some(c) = &p.quote_currency_code {
        if !c.trim().is_empty() {
            q.push(("quoteCurrencyCode", c.trim().to_lowercase()));
        }
    }
    if let Some(r) = &p.redirect_url {
        if !r.trim().is_empty() {
            q.push(("redirectURL", r.trim().to_string()));
        }
    }

    let qs = q
        .into_iter()
        .map(|(k, v)| format!("{}={}", k, urlencode(&v)))
        .collect::<Vec<_>>()
        .join("&");

    Ok(format!("https://{}?{}", p.environment.sell_host(), qs))
}

/// Sign a built URL with the partner secret key (HMAC-SHA256 over the
/// query string, including the leading "?"). Appends `&signature=…`
/// to the URL.
pub fn sign_url(url: &str, secret_key: &str) -> Result<String> {
    if secret_key.trim().is_empty() {
        return Err(OnrampError::InvalidInput("secret_key required".into()));
    }
    let qpos = url
        .find('?')
        .ok_or_else(|| OnrampError::InvalidInput("url has no query string".into()))?;
    let qs_with_q = &url[qpos..];

    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret_key.as_bytes())
        .map_err(|e| OnrampError::InvalidInput(format!("hmac init: {e}")))?;
    mac.update(qs_with_q.as_bytes());
    let sig = mac.finalize().into_bytes();
    // MoonPay expects base64-url-safe; we convert from hex via a manual encode.
    let sig_b64 = base64_encode(&sig);
    Ok(format!("{}&signature={}", url, urlencode(&sig_b64)))
}

/// RFC 3986 unreserved-character percent encoder. We don't pull a full URL
/// crate just for this — only encode characters MoonPay's parser
/// actually rejects.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Standard base64 (with `+` `/` `=`). MoonPay's docs accept this and the
/// URL-safe variant; the call site percent-encodes any reserved chars.
fn base64_encode(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8) | bytes[i + 2] as u32;
        out.push(A[((n >> 18) & 0x3f) as usize] as char);
        out.push(A[((n >> 12) & 0x3f) as usize] as char);
        out.push(A[((n >> 6) & 0x3f) as usize] as char);
        out.push(A[(n & 0x3f) as usize] as char);
        i += 3;
    }
    let rem = bytes.len() - i;
    if rem == 1 {
        let n = (bytes[i] as u32) << 16;
        out.push(A[((n >> 18) & 0x3f) as usize] as char);
        out.push(A[((n >> 12) & 0x3f) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8);
        out.push(A[((n >> 18) & 0x3f) as usize] as char);
        out.push(A[((n >> 12) & 0x3f) as usize] as char);
        out.push(A[((n >> 6) & 0x3f) as usize] as char);
        out.push('=');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> MoonPayBuyParams {
        MoonPayBuyParams {
            api_key: "pk_test_abc".into(),
            currency_code: "BTC".into(),
            wallet_address: "bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh".into(),
            base_currency_amount: Some("100".into()),
            base_currency_code: Some("USD".into()),
            environment: Environment::Sandbox,
            redirect_url: None,
        }
    }

    #[test]
    fn url_uses_sandbox_host_and_lowercases_codes() {
        let url = build_buy_url(&p()).unwrap();
        assert!(url.starts_with("https://buy-sandbox.moonpay.com?"));
        assert!(url.contains("currencyCode=btc"));
        assert!(url.contains("baseCurrencyCode=usd"));
        assert!(url.contains("apiKey=pk_test_abc"));
        assert!(url.contains("walletAddress=bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh"));
        assert!(url.contains("baseCurrencyAmount=100"));
    }

    #[test]
    fn production_host_when_requested() {
        let mut params = p();
        params.environment = Environment::Production;
        let url = build_buy_url(&params).unwrap();
        assert!(url.starts_with("https://buy.moonpay.com?"));
    }

    #[test]
    fn missing_api_key_rejected() {
        let mut params = p();
        params.api_key = "  ".into();
        assert!(matches!(
            build_buy_url(&params),
            Err(OnrampError::InvalidInput(_))
        ));
    }

    #[test]
    fn missing_wallet_rejected() {
        let mut params = p();
        params.wallet_address = "".into();
        assert!(matches!(
            build_buy_url(&params),
            Err(OnrampError::InvalidInput(_))
        ));
    }

    #[test]
    fn redirect_url_percent_encoded() {
        let mut params = p();
        params.redirect_url = Some("https://atlas.example/done?ok=1".into());
        let url = build_buy_url(&params).unwrap();
        assert!(url.contains("redirectURL=https%3A%2F%2Fatlas.example%2Fdone%3Fok%3D1"));
    }

    #[test]
    fn sign_appends_signature_param() {
        let url = build_buy_url(&p()).unwrap();
        let signed = sign_url(&url, "sk_test_secret").unwrap();
        assert!(signed.contains("&signature="));
        // Signing twice with same secret yields the same suffix.
        let signed2 = sign_url(&url, "sk_test_secret").unwrap();
        assert_eq!(signed, signed2);
        // A different secret yields a different signature.
        let signed3 = sign_url(&url, "sk_test_other").unwrap();
        assert_ne!(signed, signed3);
    }

    #[test]
    fn sign_rejects_empty_secret() {
        let url = build_buy_url(&p()).unwrap();
        assert!(matches!(
            sign_url(&url, "  "),
            Err(OnrampError::InvalidInput(_))
        ));
    }

    #[test]
    fn base64_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
    }

    fn sp() -> MoonPaySellParams {
        MoonPaySellParams {
            api_key: "pk_test_abc".into(),
            base_currency_code: "ETH".into(),
            refund_wallet_address: "0x1234567890abcdef1234567890abcdef12345678".into(),
            base_currency_amount: Some("0.5".into()),
            quote_currency_code: Some("USD".into()),
            environment: Environment::Sandbox,
            redirect_url: None,
        }
    }

    #[test]
    fn sell_url_uses_sandbox_host_and_lowercases_codes() {
        let url = build_sell_url(&sp()).unwrap();
        assert!(url.starts_with("https://sell-sandbox.moonpay.com?"));
        assert!(url.contains("baseCurrencyCode=eth"));
        assert!(url.contains("quoteCurrencyCode=usd"));
        assert!(url.contains("refundWalletAddress=0x1234567890abcdef1234567890abcdef12345678"));
        assert!(url.contains("baseCurrencyAmount=0.5"));
    }

    #[test]
    fn sell_production_host_when_requested() {
        let mut params = sp();
        params.environment = Environment::Production;
        let url = build_sell_url(&params).unwrap();
        assert!(url.starts_with("https://sell.moonpay.com?"));
    }

    #[test]
    fn sell_missing_refund_address_rejected() {
        let mut params = sp();
        params.refund_wallet_address = "  ".into();
        assert!(matches!(
            build_sell_url(&params),
            Err(OnrampError::InvalidInput(_))
        ));
    }

    #[test]
    fn sell_signature_round_trips() {
        let url = build_sell_url(&sp()).unwrap();
        let signed = sign_url(&url, "sk_test_secret").unwrap();
        assert!(signed.contains("&signature="));
    }
}
