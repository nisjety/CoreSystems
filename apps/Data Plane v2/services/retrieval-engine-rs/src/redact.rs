//! §16.4.7 — PII redaction for log + OTel span attributes.
//!
//! The retrieval query is the most leaky free-text field on the wire; users
//! routinely paste emails, phone numbers, and the occasional payment-card-like
//! string into it. The previous tracing line emitted the first 60 chars
//! verbatim, which is enough to ship a real email or phone number to OTLP /
//! Loki / Datadog. This module gives us a single chokepoint to scrub those
//! patterns before they leave the process.
//!
//! Patterns covered:
//!   - email addresses        → `[REDACTED_EMAIL]`
//!   - phone-ish digit runs   → `[REDACTED_PHONE]` (8+ contiguous digits with
//!                              optional `+`, `-`, ` `, `(`, `)` separators)
//!   - 13-19 digit card-like  → `[REDACTED_CARDLIKE]`
//!
//! After redaction we still truncate to `max_len` so a giant query doesn't
//! blow up a log line. Callers should treat the result as opaque — it is *not*
//! a stable identifier.

use once_cell::sync::Lazy;
use regex::Regex;

static EMAIL_RE: Lazy<Regex> = Lazy::new(|| {
    // RFC-5322-lite — enough to catch real addresses in free text.
    Regex::new(r"(?i)\b[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}\b").unwrap()
});

static CARD_RE: Lazy<Regex> = Lazy::new(|| {
    // 13–19 contiguous digits, optionally split by spaces or dashes in groups
    // of 3–6 digits. Order matters: card check runs before generic phone so
    // 16-digit numbers get the more specific label.
    Regex::new(r"\b(?:\d[ -]?){13,19}\b").unwrap()
});

static PHONE_RE: Lazy<Regex> = Lazy::new(|| {
    // International-friendly phone: optional leading `+`, then 8+ digits with
    // optional spaces / dashes / parens.
    Regex::new(r"\+?\(?\d{1,3}\)?[\d \-]{7,}\d").unwrap()
});

/// Redact PII patterns from `s` and truncate to `max_len` characters.
/// Returns an owned `String` because the substitutions change length.
pub fn redact_query(s: &str, max_len: usize) -> String {
    let stage1 = EMAIL_RE.replace_all(s, "[REDACTED_EMAIL]");
    let stage2 = CARD_RE.replace_all(&stage1, "[REDACTED_CARDLIKE]");
    let stage3 = PHONE_RE.replace_all(&stage2, "[REDACTED_PHONE]");
    if stage3.chars().count() <= max_len {
        stage3.into_owned()
    } else {
        let mut out = String::with_capacity(max_len + 1);
        for (i, c) in stage3.chars().enumerate() {
            if i >= max_len {
                break;
            }
            out.push(c);
        }
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_email() {
        let out = redact_query("contact alice@example.com about it", 200);
        assert!(out.contains("[REDACTED_EMAIL]"));
        assert!(!out.contains("alice@example.com"));
    }

    #[test]
    fn redacts_phone() {
        let out = redact_query("call +1 (415) 555-2671 today", 200);
        assert!(out.contains("[REDACTED_PHONE]"));
        assert!(!out.contains("555-2671"));
    }

    #[test]
    fn redacts_cardlike() {
        let out = redact_query("test 4111 1111 1111 1111 number", 200);
        assert!(out.contains("[REDACTED_CARDLIKE]"));
        assert!(!out.contains("4111 1111 1111 1111"));
    }

    #[test]
    fn truncates_long_query() {
        let long = "a".repeat(500);
        let out = redact_query(&long, 60);
        // 60 chars + ellipsis
        assert!(out.chars().count() <= 61);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn passes_clean_query_through() {
        let out = redact_query("how does the auth flow work", 200);
        assert_eq!(out, "how does the auth flow work");
    }
}
