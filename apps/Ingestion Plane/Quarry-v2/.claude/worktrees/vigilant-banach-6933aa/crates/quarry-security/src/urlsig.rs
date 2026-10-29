//! URL signature heuristics — port of donor `internal/security/heur/urlsig.go`.
//!
//! Produces a structured `UrlSignature` summarising every signal we can
//! cheaply compute from the URL alone (no DNS, no fetching). The aggregated
//! `suspicion_score` lets the caller decide whether to allow, escalate, or
//! block. The Rust port is intentionally faithful to the Go original; deltas
//! are limited to idiomatic types (enums for severity, `Url` for parsing).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

fn severity_for(score: f64) -> Severity {
    match score {
        s if s >= 0.9 => Severity::Critical,
        s if s >= 0.7 => Severity::High,
        s if s >= 0.4 => Severity::Medium,
        _ => Severity::Low,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlCharacteristic {
    pub kind: String,
    pub value: String,
    pub score: f64,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskFactor {
    pub category: String,
    pub severity: Severity,
    pub description: String,
    pub impact: f64,
    pub confidence: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UrlSignature {
    pub url: String,
    pub domain: String,
    pub path: String,
    pub query: String,
    pub fragment: String,
    pub scheme: String,
    pub port: String,
    pub tld: String,
    pub subdomain_count: usize,
    pub path_segments: Vec<String>,
    pub query_params: BTreeMap<String, String>,
    pub characteristics: Vec<UrlCharacteristic>,
    pub risk_factors: Vec<RiskFactor>,
    pub suspicion_score: f64,
}

/// Analyze a URL and return a fully populated `UrlSignature`. Returns `None`
/// when the input cannot be parsed.
pub fn analyze(target: &str) -> Option<UrlSignature> {
    let parsed = Url::parse(target).ok()?;
    let mut sig = UrlSignature {
        url: target.to_string(),
        domain: parsed.host_str().unwrap_or("").to_string(),
        path: parsed.path().to_string(),
        query: parsed.query().unwrap_or("").to_string(),
        fragment: parsed.fragment().unwrap_or("").to_string(),
        scheme: parsed.scheme().to_string(),
        port: parsed.port().map(|p| p.to_string()).unwrap_or_default(),
        ..Default::default()
    };

    for (k, v) in parsed.query_pairs() {
        sig.query_params
            .entry(k.into_owned())
            .or_insert_with(|| v.into_owned());
    }

    extract_structural(&mut sig);
    analyze_scheme(&mut sig);
    analyze_domain(&mut sig);
    analyze_path(&mut sig);
    analyze_query(&mut sig);
    analyze_length(&mut sig);
    analyze_characters(&mut sig);
    analyze_patterns(&mut sig);
    analyze_phishing_keywords(&mut sig);

    sig.suspicion_score = aggregate_score(&sig);
    Some(sig)
}

fn extract_structural(sig: &mut UrlSignature) {
    let parts: Vec<&str> = sig.domain.split('.').collect();
    if let Some(last) = parts.last() {
        sig.tld = last.to_string();
    }
    sig.subdomain_count = parts.len().saturating_sub(2);

    if !sig.path.is_empty() && sig.path != "/" {
        let trimmed = sig.path.trim_matches('/');
        sig.path_segments = trimmed
            .split('/')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
    }
}

fn analyze_scheme(sig: &mut UrlSignature) {
    match sig.scheme.as_str() {
        "https" => sig.characteristics.push(UrlCharacteristic {
            kind: "scheme".into(),
            value: "https".into(),
            score: 0.0,
            description: "Uses secure HTTPS protocol".into(),
        }),
        "http" => sig.risk_factors.push(RiskFactor {
            category: "encryption".into(),
            severity: Severity::Medium,
            description: "Uses unencrypted HTTP protocol".into(),
            impact: 0.6,
            confidence: 1.0,
        }),
        s @ ("ftp" | "file" | "data") => sig.risk_factors.push(RiskFactor {
            category: "scheme".into(),
            severity: Severity::Medium,
            description: format!("Uses unusual protocol: {s}"),
            impact: 0.5,
            confidence: 0.8,
        }),
        _ => {}
    }
}

fn analyze_domain(sig: &mut UrlSignature) {
    let domain_lc = sig.domain.to_ascii_lowercase();

    if is_ip_literal(&sig.domain) {
        sig.risk_factors.push(RiskFactor {
            category: "domain".into(),
            severity: Severity::High,
            description: "Uses IP address instead of domain name".into(),
            impact: 0.8,
            confidence: 1.0,
        });
    }

    let suspicious_tlds: &[(&str, f64)] = &[
        ("tk", 0.9),
        ("ml", 0.9),
        ("ga", 0.9),
        ("cf", 0.8),
        ("pw", 0.7),
        ("top", 0.6),
        ("click", 0.8),
        ("download", 0.9),
        ("science", 0.7),
        ("work", 0.5),
        ("party", 0.6),
        ("racing", 0.7),
        ("win", 0.6),
        ("bid", 0.7),
        ("loan", 0.8),
        ("cricket", 0.6),
    ];
    if let Some(score) = suspicious_tlds
        .iter()
        .find(|(t, _)| t == &sig.tld.as_str())
        .map(|(_, s)| *s)
    {
        sig.risk_factors.push(RiskFactor {
            category: "domain".into(),
            severity: severity_for(score),
            description: format!("Uses suspicious TLD: .{}", sig.tld),
            impact: score,
            confidence: 0.8,
        });
    }

    if has_homograph(&domain_lc) {
        sig.risk_factors.push(RiskFactor {
            category: "domain".into(),
            severity: Severity::High,
            description: "Contains non-Latin characters (possible homograph attack)".into(),
            impact: 0.8,
            confidence: 0.7,
        });
    }

    if sig.subdomain_count > 3 {
        let extra = (sig.subdomain_count - 3) as f64;
        sig.risk_factors.push(RiskFactor {
            category: "domain".into(),
            severity: Severity::Medium,
            description: format!("Excessive subdomains (level {})", sig.subdomain_count + 2),
            impact: 0.4 + extra * 0.1,
            confidence: 0.6,
        });
    }

    check_typosquat(sig, &domain_lc);
}

fn check_typosquat(sig: &mut UrlSignature, domain: &str) {
    const POPULAR: &[&str] = &[
        "google",
        "facebook",
        "amazon",
        "microsoft",
        "apple",
        "twitter",
        "linkedin",
        "instagram",
        "youtube",
        "netflix",
        "paypal",
        "ebay",
        "yahoo",
        "reddit",
        "wikipedia",
        "github",
        "stackoverflow",
    ];
    let parts: Vec<&str> = domain.split('.').collect();
    if parts.len() < 2 {
        return;
    }
    let main = parts[parts.len() - 2];
    for popular in POPULAR {
        if is_likely_typosquat(main, popular) {
            sig.risk_factors.push(RiskFactor {
                category: "domain".into(),
                severity: Severity::High,
                description: format!("Possible typosquatting of {popular}"),
                impact: 0.9,
                confidence: 0.8,
            });
            return;
        }
    }
}

fn is_likely_typosquat(domain: &str, popular: &str) -> bool {
    if domain == popular {
        return false;
    }
    if domain.len() > popular.len() + 3 {
        return false;
    }
    let len_diff = (domain.len() as isize - popular.len() as isize).abs();
    if len_diff > 2 {
        return false;
    }

    // Character substitution patterns the donor uses verbatim.
    let subs: &[(char, &[char])] = &[
        ('o', &['0']),
        ('i', &['1', 'l']),
        ('e', &['3']),
        ('a', &['@']),
        ('s', &['$']),
    ];
    for (orig, repls) in subs {
        for r in *repls {
            let modified: String = popular
                .chars()
                .map(|c| if c == *orig { *r } else { c })
                .collect();
            if domain == modified {
                return true;
            }
        }
    }

    if domain.len() > 4 && one_edit_distance(domain, popular) {
        return true;
    }
    false
}

fn one_edit_distance(s1: &str, s2: &str) -> bool {
    let (a, b) = if s1.len() <= s2.len() {
        (s1, s2)
    } else {
        (s2, s1)
    };
    if b.len() - a.len() > 1 {
        return false;
    }
    let av: Vec<char> = a.chars().collect();
    let bv: Vec<char> = b.chars().collect();
    for i in 0..av.len() {
        if av[i] != bv[i] {
            if a.len() == b.len() {
                return av[i + 1..] == bv[i + 1..]; // replace
            } else {
                return av[i..] == bv[i + 1..]; // insert
            }
        }
    }
    a.len() + 1 == b.len() // trailing insert
}

fn analyze_path(sig: &mut UrlSignature) {
    let path_lc = sig.path.to_ascii_lowercase();
    let suspicious: &[(&str, f64)] = &[
        ("admin", 0.3),
        ("login", 0.2),
        ("secure", 0.4),
        ("verify", 0.6),
        ("update", 0.5),
        ("confirm", 0.5),
        ("account", 0.4),
        ("billing", 0.4),
        ("payment", 0.5),
        ("download", 0.3),
        ("redirect", 0.7),
        ("r.php", 0.8),
        ("go.php", 0.8),
        ("link.php", 0.7),
    ];
    for (pattern, score) in suspicious {
        if path_lc.contains(pattern) {
            sig.risk_factors.push(RiskFactor {
                category: "path".into(),
                severity: severity_for(*score),
                description: format!("Suspicious path component: {pattern}"),
                impact: *score,
                confidence: 0.6,
            });
        }
    }
    if sig.path_segments.len() > 5 {
        sig.risk_factors.push(RiskFactor {
            category: "path".into(),
            severity: Severity::Low,
            description: "Deeply nested path structure".into(),
            impact: 0.3,
            confidence: 0.4,
        });
    }
}

fn analyze_query(sig: &mut UrlSignature) {
    if sig.query.is_empty() {
        return;
    }
    let suspicious_params: &[(&str, f64)] = &[
        ("redirect", 0.8),
        ("url", 0.6),
        ("link", 0.6),
        ("goto", 0.7),
        ("target", 0.5),
        ("ref", 0.3),
        ("return", 0.4),
    ];
    for (param, score) in suspicious_params {
        for k in sig.query_params.keys() {
            if k.eq_ignore_ascii_case(param) {
                sig.risk_factors.push(RiskFactor {
                    category: "query".into(),
                    severity: severity_for(*score),
                    description: format!("Suspicious query parameter: {k}"),
                    impact: *score,
                    confidence: 0.7,
                });
            }
        }
    }
    for value in sig.query_params.values() {
        if value.contains("http") || value.contains("%3A%2F%2F") {
            sig.risk_factors.push(RiskFactor {
                category: "query".into(),
                severity: Severity::Medium,
                description: "Query parameter contains URL (possible redirect)".into(),
                impact: 0.6,
                confidence: 0.8,
            });
            // Donor flags once per matching value; we mirror that loosely here.
            break;
        }
    }
}

fn analyze_length(sig: &mut UrlSignature) {
    let len = sig.url.len();
    sig.characteristics.push(UrlCharacteristic {
        kind: "length".into(),
        value: len.to_string(),
        score: length_score(len),
        description: "URL length analysis".into(),
    });
    if len > 2048 {
        sig.risk_factors.push(RiskFactor {
            category: "structure".into(),
            severity: Severity::High,
            description: "Extremely long URL (possible attack)".into(),
            impact: 0.8,
            confidence: 0.9,
        });
    } else if len > 1000 {
        sig.risk_factors.push(RiskFactor {
            category: "structure".into(),
            severity: Severity::Medium,
            description: "Very long URL".into(),
            impact: 0.4,
            confidence: 0.6,
        });
    }
}

fn analyze_characters(sig: &mut UrlSignature) {
    let mut digits = 0usize;
    let mut letters = 0usize;
    let mut unicode = 0usize;
    let total = sig.url.chars().count();
    for c in sig.url.chars() {
        match c {
            '0'..='9' => digits += 1,
            'a'..='z' | 'A'..='Z' => letters += 1,
            c if (c as u32) > 127 => unicode += 1,
            _ => {}
        }
    }
    if total == 0 {
        return;
    }
    let _ = letters; // donor records but does not act on letter count

    let digit_ratio = digits as f64 / total as f64;
    if digit_ratio > 0.3 {
        sig.risk_factors.push(RiskFactor {
            category: "characters".into(),
            severity: Severity::Low,
            description: "High ratio of digits in URL".into(),
            impact: 0.3,
            confidence: 0.5,
        });
    }
    if unicode > 0 {
        sig.risk_factors.push(RiskFactor {
            category: "characters".into(),
            severity: Severity::Medium,
            description: "Contains Unicode characters".into(),
            impact: 0.5,
            confidence: 0.7,
        });
    }
}

fn analyze_patterns(sig: &mut UrlSignature) {
    let url_lc = sig.url.to_ascii_lowercase();
    const SHORTENERS: &[&str] = &[
        "bit.ly", "tinyurl", "goo.gl", "t.co", "short", "redirect", "r.php", "go.php",
    ];
    if SHORTENERS.iter().any(|s| url_lc.contains(s)) {
        sig.risk_factors.push(RiskFactor {
            category: "pattern".into(),
            severity: Severity::Medium,
            description: "Contains URL shortener or redirect service".into(),
            impact: 0.6,
            confidence: 0.8,
        });
    }

    let parts: Vec<&str> = sig.domain.split('.').collect();
    if parts.len() > 2 {
        let subdomains = &parts[..parts.len() - 2];
        let numeric_subs = subdomains
            .iter()
            .filter(|s| s.chars().any(|c| c.is_ascii_digit()))
            .count();
        if numeric_subs > 1 {
            sig.risk_factors.push(RiskFactor {
                category: "pattern".into(),
                severity: Severity::Medium,
                description: "Multiple numeric subdomains".into(),
                impact: 0.5,
                confidence: 0.6,
            });
        }
    }
}

fn analyze_phishing_keywords(sig: &mut UrlSignature) {
    const TRUSTED: &[&str] = &[
        "google.com",
        "microsoft.com",
        "apple.com",
        "amazon.com",
        "github.com",
        "facebook.com",
        "twitter.com",
        "linkedin.com",
        "instagram.com",
        "youtube.com",
        "paypal.com",
        "ebay.com",
        "netflix.com",
        "adobe.com",
        "stackoverflow.com",
    ];
    let domain_lc = sig.domain.to_ascii_lowercase();
    if TRUSTED.iter().any(|d| domain_lc.contains(d)) {
        return;
    }
    let url_lc = sig.url.to_ascii_lowercase();
    let phishing: &[(&str, f64)] = &[
        ("account-suspended", 0.9),
        ("verify-account", 0.8),
        ("account-locked", 0.9),
        ("urgent-action", 0.8),
        ("security-alert", 0.8),
        ("confirm-identity", 0.7),
        ("update-payment", 0.8),
        ("billing-suspended", 0.8),
        ("phishing", 0.9),
        ("malware", 0.9),
        ("virus", 0.9),
        ("freemoney", 0.8),
        ("bitcoin-generator", 0.9),
    ];
    for (kw, score) in phishing {
        if url_lc.contains(kw) {
            sig.risk_factors.push(RiskFactor {
                category: "content".into(),
                severity: severity_for(*score),
                description: format!("Contains phishing keyword: {kw}"),
                impact: *score,
                confidence: 0.8,
            });
        }
    }
}

fn aggregate_score(sig: &UrlSignature) -> f64 {
    if sig.risk_factors.is_empty() {
        return 0.0;
    }
    let (sum, weight): (f64, f64) = sig
        .risk_factors
        .iter()
        .map(|r| (r.impact * r.confidence, r.confidence))
        .fold((0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
    if weight == 0.0 {
        return 0.0;
    }
    (sum / weight).min(1.0)
}

fn length_score(len: usize) -> f64 {
    if len < 30 {
        0.0
    } else if len < 100 {
        0.1
    } else if len < 500 {
        0.3
    } else if len < 1000 {
        0.5
    } else {
        0.8
    }
}

fn is_ip_literal(host: &str) -> bool {
    host.parse::<std::net::IpAddr>().is_ok() || host.starts_with('[') // bracketed IPv6
}

fn has_homograph(s: &str) -> bool {
    s.chars().any(|c| {
        let n = c as u32;
        // Cyrillic, Georgian, Greek per donor.
        (0x0400..=0x04FF).contains(&n)
            || (0x10A0..=0x10FF).contains(&n)
            || (0x0370..=0x03FF).contains(&n)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_clean_url_scores_zero() {
        let s = analyze("https://example.com/article/intro").unwrap();
        assert_eq!(s.suspicion_score, 0.0);
    }

    #[test]
    fn http_alone_is_a_risk_factor() {
        let s = analyze("http://example.com/").unwrap();
        assert!(s.risk_factors.iter().any(|r| r.category == "encryption"));
        assert!(s.suspicion_score > 0.0);
    }

    #[test]
    fn ip_literal_host_flagged() {
        let s = analyze("https://192.168.1.1/foo").unwrap();
        assert!(s
            .risk_factors
            .iter()
            .any(|r| r.description.contains("IP address")));
    }

    #[test]
    fn suspicious_tld_flagged() {
        let s = analyze("https://offer.tk/promo").unwrap();
        assert!(s
            .risk_factors
            .iter()
            .any(|r| r.description.contains("suspicious TLD")));
    }

    #[test]
    fn typosquat_paypal() {
        let s = analyze("https://paypa1.com/login").unwrap();
        assert!(s
            .risk_factors
            .iter()
            .any(|r| r.description.contains("typosquatting")));
    }

    #[test]
    fn phishing_keyword_in_path() {
        let s = analyze("https://nope.example/account-suspended/now").unwrap();
        assert!(s.risk_factors.iter().any(|r| r.category == "content"));
    }

    #[test]
    fn trusted_domains_skip_phishing_keyword_pass() {
        let s = analyze("https://github.com/account-suspended-page").unwrap();
        assert!(!s.risk_factors.iter().any(|r| r.category == "content"));
    }

    #[test]
    fn excessive_subdomains_flagged() {
        let s = analyze("https://a.b.c.d.e.f.example.com/").unwrap();
        assert!(s
            .risk_factors
            .iter()
            .any(|r| r.description.contains("Excessive subdomains")));
    }

    #[test]
    fn suspicion_score_capped_at_one() {
        let url = format!(
            "http://account-suspended.verify-account.tk/{}",
            "a".repeat(2200)
        );
        let s = analyze(&url).unwrap();
        assert!(s.suspicion_score <= 1.0);
        assert!(s.suspicion_score > 0.5);
    }

    #[test]
    fn unparsable_url_returns_none() {
        assert!(analyze("not a url").is_none());
    }
}
