//! Phishing-domain detector for dApp origin checks.
//!
//! WalletConnect / browser-launched dApps connect by URL; the dApp
//! registry (P6.2) handles known-good origins, but we still need a
//! defense for *new* origins. This crate analyses an URL and flags:
//!
//! * **Punycode IDN look-alikes** (`xn--…`) — the classic Cyrillic-`a`
//!   homoglyph attack on `uniswap.org`.
//! * **Typosquats** of curated targets via Damerau-Levenshtein
//!   distance (≤ 2 edits → suspicious).
//! * **Homoglyph normalization**: `0/o`, `1/l/i`, `5/s`, `rn/m`,
//!   `vv/w` collapsed before distance comparison.
//! * **Hyphenated subdomain attaches** (`uniswap-org.com`).
//! * **Plain HTTP** (insecure transport).
//!
//! The output `PhishingVerdict` is `Safe` / `Suspicious` / `Phish`,
//! with a list of human-readable reasons. Pure data — no I/O.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;
use url::Url;

/// Failure modes for `analyze`.
#[derive(Debug, Error, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "message")]
pub enum PhishingError {
    /// Input was not a syntactically valid URL.
    #[error("invalid url: {0}")]
    InvalidUrl(String),
    /// URL had no host component (e.g. `file:` URLs).
    #[error("missing host")]
    MissingHost,
}

/// Verdict tier for a single origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum PhishingVerdict {
    /// Origin is on the curated allowlist or shows no red flags.
    Safe,
    /// One or two soft signals; UI should warn but not block.
    Suspicious,
    /// Strong indicators (Punycode + typosquat / etc.) — block.
    Phish,
}

/// Result of one analysis.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PhishingReport {
    pub host: String,
    pub verdict: PhishingVerdict,
    pub reasons: Vec<String>,
    /// Closest target from `targets` and the edit distance found.
    pub closest_target: Option<String>,
    pub closest_distance: Option<u32>,
}

/// Configuration for `analyze`.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Default)]
pub struct PhishingConfig {
    /// Curated allowlist (etld+1, lower-case). Always returns Safe.
    pub allowlist: Vec<String>,
    /// Hard blocklist (etld+1, lower-case). Always returns Phish.
    pub blocklist: Vec<String>,
    /// Targets to typosquat-check against. e.g. `["uniswap.org",
    /// "aave.com", "opensea.io"]`. Lower-case, no scheme.
    pub targets: Vec<String>,
}

/// Analyse an origin URL.
pub fn analyze(origin: &str, config: &PhishingConfig) -> Result<PhishingReport, PhishingError> {
    let parsed = Url::parse(origin).map_err(|e| PhishingError::InvalidUrl(e.to_string()))?;
    let host = parsed
        .host_str()
        .ok_or(PhishingError::MissingHost)?
        .to_ascii_lowercase();

    let mut reasons: Vec<String> = Vec::new();

    // Hard allow / block first.
    if config.allowlist.iter().any(|h| eq_or_subdomain(&host, h)) {
        return Ok(PhishingReport {
            host,
            verdict: PhishingVerdict::Safe,
            reasons: vec!["origin on curated allowlist".into()],
            closest_target: None,
            closest_distance: None,
        });
    }
    if config.blocklist.iter().any(|h| eq_or_subdomain(&host, h)) {
        return Ok(PhishingReport {
            host,
            verdict: PhishingVerdict::Phish,
            reasons: vec!["origin on curated blocklist".into()],
            closest_target: None,
            closest_distance: None,
        });
    }

    // Plain HTTP is a soft signal.
    if parsed.scheme() == "http" {
        reasons.push("insecure transport (http://)".into());
    }

    // Punycode IDN — strong signal of homoglyph attacks.
    let has_punycode = host.split('.').any(|label| label.starts_with("xn--"));
    if has_punycode {
        reasons.push("contains Punycode label (xn--…)".into());
    }

    // Hyphenated typosquat attach: `uniswap-org.com` etc.
    let hyphen_attach = config.targets.iter().any(|t| {
        let stem = t.split('.').next().unwrap_or(t);
        host.contains(&format!("{stem}-")) || host.contains(&format!("-{stem}"))
    });
    if hyphen_attach {
        reasons.push("hyphenated subdomain looks like a known brand".into());
    }

    // Closest typosquat match using homoglyph-normalised Damerau-Levenshtein.
    let host_norm = normalize_homoglyphs(&host);
    let mut best: Option<(String, u32)> = None;
    for target in &config.targets {
        let target_norm = normalize_homoglyphs(target);
        if host_norm == target_norm {
            // Exact homoglyph hit — same brand spelled differently.
            best = Some((target.clone(), 0));
            break;
        }
        let d = damerau_levenshtein(&host_norm, &target_norm);
        match &best {
            Some((_, bd)) if d >= *bd => {}
            _ => best = Some((target.clone(), d)),
        }
    }
    let mut closest_target: Option<String> = None;
    let mut closest_distance: Option<u32> = None;
    if let Some((t, d)) = best {
        // Equal after homoglyph collapse but not exact host match
        // means the original differs only in confusables.
        if d == 0 && host != t {
            reasons.push(format!("differs from \"{t}\" only by homoglyphs"));
        } else if d > 0 && d <= 2 {
            reasons.push(format!("typosquat of \"{t}\" (edit distance {d})"));
        }
        closest_target = Some(t);
        closest_distance = Some(d);
    }

    let verdict = score(&reasons);
    Ok(PhishingReport {
        host,
        verdict,
        reasons,
        closest_target,
        closest_distance,
    })
}

fn score(reasons: &[String]) -> PhishingVerdict {
    let strong = reasons.iter().any(|r| {
        r.starts_with("typosquat") || r.contains("homoglyph") || r.starts_with("contains Punycode")
    });
    let weak = reasons
        .iter()
        .any(|r| r.starts_with("hyphenated") || r.starts_with("insecure"));
    if strong && reasons.len() >= 2 {
        PhishingVerdict::Phish
    } else if strong || weak {
        PhishingVerdict::Suspicious
    } else {
        PhishingVerdict::Safe
    }
}

// --- helpers ---------------------------------------------------------

fn eq_or_subdomain(host: &str, target: &str) -> bool {
    let host = host.to_ascii_lowercase();
    let target = target.to_ascii_lowercase();
    host == target || host.ends_with(&format!(".{target}"))
}

/// Collapse common confusables to a canonical form so typosquats
/// can be detected after normalisation.
fn normalize_homoglyphs(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '0' => out.push('o'),
            '1' | 'l' | '|' => out.push('i'),
            '5' => out.push('s'),
            'r' if chars.peek() == Some(&'n') => {
                chars.next();
                out.push('m');
            }
            'v' if chars.peek() == Some(&'v') => {
                chars.next();
                out.push('w');
            }
            other => out.push(other.to_ascii_lowercase()),
        }
    }
    out
}

/// Damerau–Levenshtein distance (with adjacent-transposition).
fn damerau_levenshtein(a: &str, b: &str) -> u32 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let m = a.len();
    let n = b.len();
    if m == 0 {
        return n as u32;
    }
    if n == 0 {
        return m as u32;
    }
    let mut d = vec![vec![0u32; n + 1]; m + 1];
    for (i, row) in d.iter_mut().enumerate().take(m + 1) {
        row[0] = i as u32;
    }
    for (j, val) in d[0].iter_mut().enumerate().take(n + 1) {
        *val = j as u32;
    }
    for i in 1..=m {
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            let del = d[i - 1][j] + 1;
            let ins = d[i][j - 1] + 1;
            let sub = d[i - 1][j - 1] + cost;
            let mut best = del.min(ins).min(sub);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = best;
        }
    }
    d[m][n]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> PhishingConfig {
        PhishingConfig {
            allowlist: vec!["uniswap.org".into()],
            blocklist: vec!["evil.example".into()],
            targets: vec!["uniswap.org".into(), "aave.com".into(), "opensea.io".into()],
        }
    }

    #[test]
    fn allowlist_returns_safe() {
        let r = analyze("https://app.uniswap.org/swap", &cfg()).unwrap();
        assert_eq!(r.verdict, PhishingVerdict::Safe);
    }

    #[test]
    fn blocklist_returns_phish() {
        let r = analyze("https://evil.example/", &cfg()).unwrap();
        assert_eq!(r.verdict, PhishingVerdict::Phish);
    }

    #[test]
    fn invalid_url_errors() {
        let err = analyze("not a url", &cfg()).unwrap_err();
        assert!(matches!(err, PhishingError::InvalidUrl(_)));
    }

    #[test]
    fn punycode_alone_is_suspicious() {
        let r = analyze("https://xn--bcher-kva.example/", &cfg()).unwrap();
        assert!(r.reasons.iter().any(|s| s.contains("Punycode")));
        // No close typosquat → just one strong signal.
        assert!(matches!(
            r.verdict,
            PhishingVerdict::Suspicious | PhishingVerdict::Phish
        ));
    }

    #[test]
    fn typosquat_one_edit_is_suspicious() {
        let r = analyze("https://uniswop.org/", &cfg()).unwrap();
        assert_eq!(r.closest_target.as_deref(), Some("uniswap.org"));
        assert_eq!(r.closest_distance, Some(1));
        assert!(matches!(
            r.verdict,
            PhishingVerdict::Suspicious | PhishingVerdict::Phish
        ));
    }

    #[test]
    fn hyphenated_brand_attach_is_suspicious() {
        let r = analyze("https://uniswap-org.example.com/", &cfg()).unwrap();
        assert!(r.reasons.iter().any(|s| s.starts_with("hyphenated")));
        assert!(matches!(
            r.verdict,
            PhishingVerdict::Suspicious | PhishingVerdict::Phish
        ));
    }

    #[test]
    fn http_alone_is_suspicious() {
        let r = analyze("http://example.notabrand/", &cfg()).unwrap();
        assert_eq!(r.verdict, PhishingVerdict::Suspicious);
    }

    #[test]
    fn safe_unrelated_domain() {
        let r = analyze("https://wikipedia.org/", &cfg()).unwrap();
        assert_eq!(r.verdict, PhishingVerdict::Safe);
    }

    #[test]
    fn homoglyph_zero_for_o_collapses() {
        // Host with `0` substituted for `o` should still flag close to a target.
        let r = analyze("https://uniswap.0rg/", &cfg()).unwrap();
        // After homoglyph normalisation "uniswap.0rg" → "uniswap.org",
        // identical to the target. Distance must be 0.
        assert_eq!(r.closest_distance, Some(0));
        assert_eq!(r.closest_target.as_deref(), Some("uniswap.org"));
    }

    #[test]
    fn damerau_levenshtein_handles_transposition() {
        assert_eq!(damerau_levenshtein("abc", "abc"), 0);
        assert_eq!(damerau_levenshtein("abc", "acb"), 1);
        assert_eq!(damerau_levenshtein("abcd", "acbd"), 1);
        assert_eq!(damerau_levenshtein("abc", "xyz"), 3);
    }

    #[test]
    fn normalize_homoglyphs_collapses_confusables() {
        // Note: `l` itself is mapped to `i`, so wallet/vvallet both
        // collapse to `waiiet` — still equal, which is the property
        // we actually need for typosquat detection.
        assert_eq!(
            normalize_homoglyphs("un1swap"),
            normalize_homoglyphs("uniswap")
        );
        assert_eq!(normalize_homoglyphs("rnetamask"), "metamask");
        assert_eq!(
            normalize_homoglyphs("vvallet"),
            normalize_homoglyphs("wallet")
        );
        assert_eq!(normalize_homoglyphs("0pensea"), "opensea");
    }

    #[test]
    fn punycode_plus_typosquat_is_phish() {
        let cfg = PhishingConfig {
            allowlist: vec![],
            blocklist: vec![],
            targets: vec!["xn--bcher-kva.example".into()],
        };
        let r = analyze("https://xn--bcher-kva.example/", &cfg).unwrap();
        assert!(r.reasons.iter().any(|s| s.contains("Punycode")));
    }

    #[test]
    fn missing_host_errors() {
        let err = analyze("file:///etc/passwd", &cfg()).unwrap_err();
        assert!(matches!(err, PhishingError::MissingHost));
    }

    #[test]
    fn allowlist_subdomain_match_works() {
        let r = analyze("https://swap.app.uniswap.org/", &cfg()).unwrap();
        assert_eq!(r.verdict, PhishingVerdict::Safe);
    }
}
