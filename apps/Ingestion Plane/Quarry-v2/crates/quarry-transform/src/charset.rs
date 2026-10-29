//! Charset detection + body re-decoding when the HTTP `Content-Type`
//! header lies about the encoding.
//!
//! Common case: CMS-driven sites (older WordPress, custom PHP, regional
//! shops) declare `charset=utf-8` but actually serve `windows-1252` or
//! `ISO-8859-1`. Without re-decoding, Norwegian `æ ø å`, German umlauts,
//! and most non-ASCII Latin characters arrive as mojibake — the
//! downstream readability extractor builds nonsense markdown and the
//! LLM gets garbage in the snippet excerpt.
//!
//! Strategy:
//! 1. Trust the header **only if** the declared charset is a well-known
//!    real label AND a quick sanity decode of the first ~4 KB succeeds
//!    without replacement characters.
//! 2. Otherwise sniff with `chardetng` (the algorithm shipping in
//!    Firefox's charset detector) over the first 64 KB.
//! 3. Decode the body using `encoding_rs` with the chosen encoding
//!    label, lossy on undecodable bytes (replaced with `U+FFFD`) so we
//!    never panic on corrupt input.
//!
//! Returns the decoded string + the encoding label that was actually
//! used, so callers can record telemetry and skip re-decoding when the
//! header was already correct.

use chardetng::EncodingDetector;
use encoding_rs::{Encoding, UTF_8};

/// Result of a decode pass — the text and the encoding label that was
/// applied. `from_header` is `true` when we trusted the HTTP header,
/// `false` when we sniffed.
#[derive(Debug, Clone)]
pub struct DecodedBody {
    pub text: String,
    pub encoding: &'static str,
    pub from_header: bool,
}

/// Decode `body` to a `String`, picking the best encoding given an
/// optional declared charset from the `Content-Type` header.
///
/// The declared charset is matched case-insensitively against
/// `encoding_rs`'s WHATWG-label table — values like `iso-8859-1`,
/// `windows-1252`, `gbk`, `shift_jis`, `utf-8` all resolve correctly.
/// Anything unknown forces a sniff.
pub fn decode(body: &[u8], declared_charset: Option<&str>) -> DecodedBody {
    if let Some(label) = declared_charset {
        if let Some(enc) = Encoding::for_label(label.trim().as_bytes()) {
            // Quick sanity check on a small prefix: if decoding the
            // first 4 KB introduces ≥ 2 % replacement characters, the
            // declared label is probably a lie. Fall through to
            // sniffing in that case.
            let prefix_len = body.len().min(4096);
            let (cow, _, had_errors) = enc.decode(&body[..prefix_len]);
            if !had_errors || replacement_ratio(&cow) < 0.02 {
                let (text_cow, _, _) = enc.decode(body);
                return DecodedBody {
                    text: text_cow.into_owned(),
                    encoding: enc.name(),
                    from_header: true,
                };
            }
        }
    }
    // Sniff. `chardetng` wants ≥ a few KB to make a confident call.
    let sniff_window = body.len().min(64 * 1024);
    let mut det = EncodingDetector::new();
    det.feed(&body[..sniff_window], true);
    let enc = det.guess(None, true);
    let (text_cow, _, _) = enc.decode(body);
    DecodedBody {
        text: text_cow.into_owned(),
        encoding: enc.name(),
        from_header: false,
    }
}

/// Convenience: decode assuming UTF-8 unless the body has a BOM or
/// looks obviously non-UTF-8. Used when there's no Content-Type at all
/// (e.g. file://, fixture data, or servers that omit the header).
pub fn decode_unknown(body: &[u8]) -> DecodedBody {
    decode(body, None)
}

fn replacement_ratio(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let bad = s.chars().filter(|c| *c == '\u{FFFD}').count();
    bad as f64 / s.chars().count() as f64
}

/// Extract the `charset=` parameter from a `Content-Type` header value
/// if present. Tolerant of whitespace and quoting.
///
/// Examples:
///   `text/html; charset=utf-8`         → `Some("utf-8")`
///   `text/html;charset="ISO-8859-1"`   → `Some("ISO-8859-1")`
///   `text/html`                         → `None`
pub fn charset_from_content_type(ct: &str) -> Option<&str> {
    for part in ct.split(';') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("charset=") {
            let s = rest.trim().trim_matches('"').trim_matches('\'');
            if !s.is_empty() {
                return Some(s);
            }
        }
        if let Some(rest) = part.strip_prefix("CHARSET=") {
            let s = rest.trim().trim_matches('"').trim_matches('\'');
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}

/// Force-suppress unused-import warnings when this module is compiled
/// into a build that doesn't exercise UTF_8 elsewhere. Reads the
/// static, has no side effect.
#[allow(dead_code)]
fn _keep_utf8_referenced() -> &'static Encoding {
    UTF_8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_charset_from_content_type() {
        assert_eq!(
            charset_from_content_type("text/html; charset=utf-8"),
            Some("utf-8")
        );
        assert_eq!(
            charset_from_content_type("text/html;charset=\"ISO-8859-1\""),
            Some("ISO-8859-1")
        );
        assert_eq!(charset_from_content_type("text/html"), None);
        assert_eq!(charset_from_content_type("application/json"), None);
    }

    #[test]
    fn decodes_utf8_when_declared_correctly() {
        let body = "Norsk: æ ø å. Tysk: ä ö ü.".as_bytes();
        let out = decode(body, Some("utf-8"));
        assert_eq!(out.text, "Norsk: æ ø å. Tysk: ä ö ü.");
        assert_eq!(out.encoding, "UTF-8");
        assert!(out.from_header);
    }

    #[test]
    fn sniffs_when_header_lies() {
        // Bytes for `æ ø å` in Windows-1252: 0xE6 0x20 0xF8 0x20 0xE5
        // Same bytes interpreted as UTF-8 would produce replacement
        // characters because they're not valid UTF-8 lead bytes.
        let body = b"\xe6 \xf8 \xe5 caf\xe9";
        let out = decode(body, Some("utf-8"));
        // Either windows-1252 or ISO-8859-1 may be guessed — both
        // produce the same printable output for this byte set.
        assert!(out.text.contains('æ'));
        assert!(out.text.contains('é'));
        assert!(!out.from_header);
    }

    #[test]
    fn decode_unknown_handles_pure_utf8() {
        let body = "hello world".as_bytes();
        let out = decode_unknown(body);
        assert_eq!(out.text, "hello world");
    }

    #[test]
    fn never_panics_on_empty() {
        let out = decode(&[], None);
        assert_eq!(out.text, "");
    }
}
