//! Display formatting helpers for the wallet UI.
//!
//! - `format_currency` — locale-aware money formatting (en-US / de-DE / fr-FR).
//! - `format_compact` — 1234567 → "1.23M" / "$1.23M".
//! - `format_token_amount` — decimal-string base-units → human-readable
//!   token quantity with a bounded number of significant digits.
//! - `truncate_address` — 0x12345…abcd.
//!
//! No I/O. Locale handling intentionally tiny (no ICU dep).

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

#[derive(Debug, Error, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "message")]
pub enum FmtError {
    #[error("invalid amount: {0}")]
    InvalidAmount(String),
}

#[derive(Debug, Clone, Copy)]
struct LocaleSpec {
    decimal: char,
    grouping: char,
}

fn locale(tag: &str) -> LocaleSpec {
    match tag {
        "de-DE" | "de" | "it-IT" | "it" | "es-ES" | "es" => LocaleSpec {
            decimal: ',',
            grouping: '.',
        },
        "fr-FR" | "fr" => LocaleSpec {
            decimal: ',',
            grouping: '\u{00A0}',
        },
        _ => LocaleSpec {
            decimal: '.',
            grouping: ',',
        },
    }
}

fn currency_symbol(code: &str) -> &'static str {
    match code.to_ascii_uppercase().as_str() {
        "USD" => "$",
        "EUR" => "\u{20AC}",
        "GBP" => "\u{00A3}",
        "JPY" => "\u{00A5}",
        "CHF" => "CHF",
        "CAD" => "CA$",
        "AUD" => "A$",
        _ => "",
    }
}

fn group_integer_part(int_part: &str, locale: &LocaleSpec) -> String {
    let bytes: Vec<char> = int_part.chars().collect();
    let mut out = String::with_capacity(bytes.len() + bytes.len() / 3);
    let len = bytes.len();
    for (idx, ch) in bytes.iter().enumerate() {
        let from_end = len - idx;
        if idx > 0 && from_end.is_multiple_of(3) {
            out.push(locale.grouping);
        }
        out.push(*ch);
    }
    out
}

/// Format a USD-ish amount with `decimals` decimal places and a
/// currency symbol. Locales: en-US (default), de-DE, fr-FR, it-IT,
/// es-ES.
pub fn format_currency(amount: f64, code: &str, locale_tag: &str, decimals: u32) -> String {
    let neg = amount < 0.0;
    let abs = amount.abs();
    let loc = locale(locale_tag);
    let formatted = format!("{:.*}", decimals as usize, abs);
    let (int_part, frac_part) = match formatted.split_once('.') {
        Some((i, f)) => (i.to_string(), f.to_string()),
        None => (formatted.clone(), String::new()),
    };
    let grouped = group_integer_part(&int_part, &loc);
    let mut number = grouped;
    if !frac_part.is_empty() {
        number.push(loc.decimal);
        number.push_str(&frac_part);
    }
    let symbol = currency_symbol(code);
    let body = if symbol.is_empty() {
        format!("{number} {}", code.to_ascii_uppercase())
    } else {
        // de-DE / fr-FR put symbol after.
        match locale_tag {
            "de-DE" | "de" | "fr-FR" | "fr" | "it-IT" | "it" | "es-ES" | "es" => {
                format!("{number}\u{00A0}{symbol}")
            }
            _ => format!("{symbol}{number}"),
        }
    };
    if neg {
        format!("-{body}")
    } else {
        body
    }
}

/// 1234 → "1.23K", 1234567 → "1.23M", 1.23e9 → "1.23B".
/// Negative numbers are prefixed with `-`.
pub fn format_compact(value: f64) -> String {
    if !value.is_finite() {
        return "—".into();
    }
    let neg = value < 0.0;
    let abs = value.abs();
    let (num, suffix) = if abs >= 1e12 {
        (abs / 1e12, "T")
    } else if abs >= 1e9 {
        (abs / 1e9, "B")
    } else if abs >= 1e6 {
        (abs / 1e6, "M")
    } else if abs >= 1e3 {
        (abs / 1e3, "K")
    } else {
        (abs, "")
    };
    let body = if suffix.is_empty() {
        if abs >= 1.0 {
            format!("{abs:.2}")
        } else {
            format!("{abs:.4}")
        }
    } else {
        format!("{num:.2}{suffix}")
    };
    if neg {
        format!("-{body}")
    } else {
        body
    }
}

/// Format a base-unit decimal-string into a human-readable token
/// amount with at most `max_significant` significant digits.
///
/// Example: `format_token_amount("1234567890123456789", 18, 6)` →
/// `"1.23457"`.
pub fn format_token_amount(
    base_units: &str,
    decimals: u32,
    max_significant: u32,
) -> Result<String, FmtError> {
    let s = base_units.trim();
    if s.is_empty() {
        return Err(FmtError::InvalidAmount(base_units.into()));
    }
    if !s.chars().all(|c| c.is_ascii_digit()) {
        return Err(FmtError::InvalidAmount(base_units.into()));
    }
    let s = s.trim_start_matches('0');
    let s = if s.is_empty() { "0" } else { s };
    let dec = decimals as usize;
    let mut int_part: String;
    let mut frac_part: String;
    if s.len() <= dec {
        int_part = "0".to_string();
        let pad = dec - s.len();
        frac_part = "0".repeat(pad) + s;
    } else {
        let split = s.len() - dec;
        int_part = s[..split].to_string();
        frac_part = s[split..].to_string();
    }
    // Trim trailing zeros from frac.
    while frac_part.ends_with('0') {
        frac_part.pop();
    }
    if frac_part.is_empty() {
        return Ok(int_part);
    }
    // Apply max_significant cap.
    if max_significant == 0 {
        return Ok(int_part);
    }
    let int_significant = if int_part == "0" {
        0
    } else {
        int_part.trim_start_matches('0').len() as u32
    };
    if int_significant >= max_significant {
        return Ok(int_part);
    }
    let allowed_frac = (max_significant - int_significant) as usize;
    if frac_part.len() > allowed_frac {
        // For "0.000123456" (int_significant=0), we want allowed_frac
        // significant digits, not just chars: skip leading zeros.
        if int_part == "0" {
            let leading_zeros = frac_part.chars().take_while(|c| *c == '0').count();
            let total = leading_zeros + max_significant as usize;
            if frac_part.len() > total {
                frac_part.truncate(total);
            }
        } else {
            frac_part.truncate(allowed_frac);
        }
        while frac_part.ends_with('0') {
            frac_part.pop();
        }
    }
    if frac_part.is_empty() {
        Ok(int_part)
    } else {
        // Drop dummy mut.
        let _ = &mut int_part;
        Ok(format!("{int_part}.{frac_part}"))
    }
}

/// `0xabc...123`. Returns the input unchanged if it's already short.
pub fn truncate_address(addr: &str) -> String {
    let s = addr.trim();
    if s.len() <= 13 {
        return s.to_string();
    }
    let (head, tail) = if let Some(rest) = s.strip_prefix("0x") {
        if rest.len() <= 10 {
            return s.to_string();
        }
        (&s[..6], &s[s.len() - 4..])
    } else if s.len() > 12 {
        (&s[..6], &s[s.len() - 4..])
    } else {
        return s.to_string();
    };
    format!("{head}\u{2026}{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn currency_en_us_default_two_decimals() {
        assert_eq!(format_currency(1234.5, "USD", "en-US", 2), "$1,234.50");
    }

    #[test]
    fn currency_de_de_swaps_separators_and_suffixes_symbol() {
        assert_eq!(
            format_currency(1234.5, "EUR", "de-DE", 2),
            "1.234,50\u{00A0}\u{20AC}"
        );
    }

    #[test]
    fn currency_negative_has_minus_prefix() {
        assert_eq!(format_currency(-12.0, "USD", "en-US", 2), "-$12.00");
    }

    #[test]
    fn currency_unknown_code_uses_iso() {
        assert_eq!(format_currency(1.0, "ZZZ", "en-US", 2), "1.00 ZZZ");
    }

    #[test]
    fn currency_zero_decimals() {
        assert_eq!(format_currency(1234.6, "JPY", "en-US", 0), "\u{00A5}1,235");
    }

    #[test]
    fn compact_thousand() {
        assert_eq!(format_compact(1234.0), "1.23K");
    }

    #[test]
    fn compact_million_and_billion() {
        assert_eq!(format_compact(1_234_567.0), "1.23M");
        assert_eq!(format_compact(1_234_000_000.0), "1.23B");
    }

    #[test]
    fn compact_small_number_has_extra_decimals() {
        assert_eq!(format_compact(0.1234), "0.1234");
    }

    #[test]
    fn compact_negative() {
        assert_eq!(format_compact(-1234.0), "-1.23K");
    }

    #[test]
    fn token_amount_eth_typical() {
        assert_eq!(
            format_token_amount("1234567890123456789", 18, 6).unwrap(),
            "1.23456"
        );
    }

    #[test]
    fn token_amount_below_one() {
        assert_eq!(
            format_token_amount("123456789012345678", 18, 4).unwrap(),
            "0.1234"
        );
    }

    #[test]
    fn token_amount_drops_trailing_zeros() {
        assert_eq!(
            format_token_amount("1000000000000000000", 18, 6).unwrap(),
            "1"
        );
    }

    #[test]
    fn token_amount_zero_units_yields_zero() {
        assert_eq!(format_token_amount("0", 18, 6).unwrap(), "0");
    }

    #[test]
    fn token_amount_rejects_non_digit() {
        let err = format_token_amount("not-a-number", 18, 6).unwrap_err();
        assert!(matches!(err, FmtError::InvalidAmount(_)));
    }

    #[test]
    fn truncate_long_address_is_shortened() {
        assert_eq!(
            truncate_address("0xabcdef1234567890aaaaaaaaaaaaaaaaaaaa1234"),
            "0xabcd\u{2026}1234"
        );
    }

    #[test]
    fn truncate_short_address_unchanged() {
        assert_eq!(truncate_address("0xabc"), "0xabc");
    }
}
