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

use std::fmt;

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

/// The retrieval query as an operator log line may carry it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryForLog {
    /// The ordinary posture: the PII-scrubbed, truncated text.
    Text(String),
    /// A restrictive posture: the text is not written. The line keeps the byte
    /// length and a correlation hash instead (see [`query_for_log`]).
    Withheld { len: usize, hash: String },
}

impl fmt::Display for QueryForLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(text) => f.write_str(text),
            Self::Withheld { len, hash } => write!(f, "withheld(len={len},hash={hash})"),
        }
    }
}

/// The query as a log line may carry it.
///
/// `withhold` is the restrictive-posture signal: ZDR `reject` or `ephemeral`, or
/// sovereignty required. Under it the text is never written, because a regex scrub
/// cannot make free text safe to keep in operator logs. The line keeps the byte
/// length and a keyed hash instead. Equal queries in one process get equal hashes,
/// so repeats stay visible. The key is random per process and never logged, so a
/// logged hash cannot be checked against a guessed query by anyone who reads the
/// logs. Otherwise the text is scrubbed and truncated, as before.
pub fn query_for_log(query: &str, max_len: usize, withhold: bool) -> QueryForLog {
    if withhold {
        QueryForLog::Withheld {
            len: query.len(),
            hash: correlation_hash(query),
        }
    } else {
        QueryForLog::Text(redact_query(query, max_len))
    }
}

/// Per-process key for [`correlation_hash`]: 32 bytes from the OS random source
/// (two v4 UUIDs), drawn on first use and never logged.
static CORRELATION_KEY: Lazy<[u8; 32]> = Lazy::new(|| {
    let mut key = [0u8; 32];
    key[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    key[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    key
});

/// 16 hex digits of the keyed BLAKE3 digest of `query`. The key is per process, so
/// the hash correlates repeats within one process only, not across restarts or
/// replicas.
fn correlation_hash(query: &str) -> String {
    let hex = blake3::keyed_hash(&CORRELATION_KEY, query.as_bytes()).to_hex();
    hex[..16].to_owned()
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

    #[test]
    fn withheld_query_keeps_only_its_length_and_a_keyed_hash() {
        let out = query_for_log("alice@example.com", 60, true).to_string();
        assert!(out.starts_with("withheld(len=17,hash="), "{out}");
        assert!(!out.contains("alice") && !out.contains("example"), "{out}");
    }

    #[test]
    fn unwithheld_query_is_scrubbed_and_truncated_as_before() {
        assert_eq!(
            query_for_log("contact alice@example.com about it", 200, false).to_string(),
            "contact [REDACTED_EMAIL] about it"
        );
        let long = "a".repeat(500);
        assert_eq!(
            query_for_log(&long, 60, false).to_string(),
            format!("{}…", "a".repeat(60))
        );
    }

    #[test]
    fn withheld_length_counts_bytes_like_the_query_len_span_field() {
        // Three characters, six bytes in UTF-8.
        assert!(query_for_log("æøå", 60, true)
            .to_string()
            .starts_with("withheld(len=6,"));
    }

    #[test]
    fn withheld_hash_repeats_within_a_process_and_is_keyed() {
        let hash_of = |query: &str| match query_for_log(query, 60, true) {
            QueryForLog::Withheld { hash, .. } => hash,
            QueryForLog::Text(_) => panic!("a withheld query carries a hash"),
        };
        let hash = hash_of("pump seals");
        assert_eq!(hash.len(), 16);
        assert_eq!(hash, hash_of("pump seals"), "repeats correlate");
        assert_ne!(hash, hash_of("pump seal"), "other queries differ");
        let plain = blake3::hash(b"pump seals").to_hex().to_string();
        assert_ne!(hash.as_str(), &plain[..16], "the hash is keyed");
    }
}
