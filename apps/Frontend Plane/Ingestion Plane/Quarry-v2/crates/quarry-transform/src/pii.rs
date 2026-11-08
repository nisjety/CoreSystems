//! PII detection for the fail-closed redaction stage (E3.6-1, F-t2-050).
//!
//! A pure library: no I/O, no logging, no environment reads, nothing wired.
//! The stage that calls it (between markdown derivation and every sink) is a
//! later slice. A candidate is found with a bounded regular expression and kept
//! only when its validator passes, so a random 16-digit order number is not a
//! card and a random 11-digit number is not a national identity number.
//!
//! Kinds and their validators:
//! * [`PiiKind::Email`]: an ASCII local part and a dotted domain with a
//!   letters-only top-level domain.
//! * [`PiiKind::Phone`]: an international `+` form of 8 to 15 digits, or a
//!   Norwegian national number of 8 digits starting 2 to 9, grouped 3-2-3 or
//!   2-2-2-2.
//! * [`PiiKind::Iban`]: the country length (for the common European countries)
//!   and the mod-97 check.
//! * [`PiiKind::PaymentCard`]: WS12's card rule,
//!   [`quarry_core::redaction::luhn::is_pan`] (issuer prefix, length, Luhn).
//! * [`PiiKind::Fodselsnummer`]: the Norwegian 11-digit national identity
//!   number: date fields (D-number and H-number offsets) and both mod-11
//!   control digits.
//!
//! The scanner only reports what it finds. Deciding what a page does when a
//! redaction step fails is the caller's job (E3.6-4, not built).

use std::sync::LazyLock;

use quarry_core::redaction::luhn::is_pan;
use regex::Regex;

/// The kind of personal data a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PiiKind {
    /// An email address.
    Email,
    /// A telephone number (international or Norwegian national form).
    Phone,
    /// An international bank account number.
    Iban,
    /// A payment card number.
    PaymentCard,
    /// A Norwegian national identity number (fodselsnummer, D-number, H-number).
    Fodselsnummer,
}

impl PiiKind {
    /// Stable name used in fixtures and reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::Phone => "phone",
            Self::Iban => "iban",
            Self::PaymentCard => "card",
            Self::Fodselsnummer => "fodselsnummer",
        }
    }

    /// The text that replaces a match: `[redacted:pii.<label>]`.
    #[must_use]
    pub const fn marker(self) -> &'static str {
        match self {
            Self::Email => "[redacted:pii.email]",
            Self::Phone => "[redacted:pii.phone]",
            Self::Iban => "[redacted:pii.iban]",
            Self::PaymentCard => "[redacted:pii.card]",
            Self::Fodselsnummer => "[redacted:pii.fodselsnummer]",
        }
    }
}

/// One validated match: a byte range of the scanned text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PiiFinding {
    /// What the match is.
    pub kind: PiiKind,
    /// Byte offset of the first byte of the match.
    pub start: usize,
    /// Byte offset just past the last byte of the match.
    pub end: usize,
}

/// Email: a local part of 1 to 64 characters from `[A-Za-z0-9._%+-]` with no
/// leading, trailing or doubled dot; a domain of at least two labels, each 1 to
/// 63 letters, digits or hyphens (no hyphen at an edge), and a letters-only
/// top-level domain of 2 to 24 characters. Narrower than RFC 5322 on purpose:
/// quoted local parts and non-ASCII letters are not accepted.
#[must_use]
pub fn is_email(s: &str) -> bool {
    let Some((local, domain)) = s.rsplit_once('@') else {
        return false;
    };
    if local.is_empty() || local.len() > 64 || domain.len() > 253 {
        return false;
    }
    if local.starts_with('.') || local.ends_with('.') || local.contains("..") {
        return false;
    }
    if !local
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'%' | b'+' | b'-'))
    {
        return false;
    }
    let mut count = 0usize;
    let mut tld = "";
    for label in domain.split('.') {
        count += 1;
        tld = label;
        let edge_hyphen = label.starts_with('-') || label.ends_with('-');
        if label.is_empty()
            || label.len() > 63
            || edge_hyphen
            || !label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return false;
        }
    }
    count >= 2 && (2..=24).contains(&tld.len()) && tld.bytes().all(|b| b.is_ascii_alphabetic())
}

/// Phone: either an international form (`+`, then 8 to 15 digits whose first
/// digit is not 0, with spaces, dots, hyphens and brackets allowed between
/// digits), or a Norwegian national number of exactly 8 digits starting 2 to 9.
#[must_use]
pub fn is_phone(s: &str) -> bool {
    let trimmed = s.trim();
    let (international, body) = match trimmed.strip_prefix('+') {
        Some(rest) => (true, rest),
        None => (false, trimmed),
    };
    if !body
        .bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b' ' | b'-' | b'.' | b'(' | b')'))
    {
        return false;
    }
    let digits: Vec<u8> = body.bytes().filter(u8::is_ascii_digit).collect();
    let first = digits.first().copied();
    if international {
        (8..=15).contains(&digits.len()) && first.is_some_and(|d| d != b'0')
    } else {
        digits.len() == 8 && first.is_some_and(|d| (b'2'..=b'9').contains(&d))
    }
}

/// Length of an IBAN for the countries this module knows. Other countries are
/// accepted at any length from 15 to 34 that the mod-97 check confirms.
fn iban_length(country: &str) -> Option<usize> {
    Some(match country {
        "AT" => 20,
        "BE" => 16,
        "CH" => 21,
        "DE" => 22,
        "DK" => 18,
        "ES" => 24,
        "FI" => 18,
        "FR" => 27,
        "GB" => 22,
        "IE" => 22,
        "IT" => 27,
        "LU" => 20,
        "NL" => 18,
        "NO" => 15,
        "PL" => 28,
        "SE" => 24,
        _ => return None,
    })
}

/// IBAN: spaces are ignored; two letters, two check digits, then alphanumerics.
/// The country length is checked when known, and the mod-97 check must give 1.
#[must_use]
pub fn is_iban(s: &str) -> bool {
    let compact: Vec<u8> = s
        .bytes()
        .filter(|b| *b != b' ')
        .map(|b| b.to_ascii_uppercase())
        .collect();
    if !(15..=34).contains(&compact.len()) {
        return false;
    }
    if !compact[..2].iter().all(|b| b.is_ascii_uppercase())
        || !compact[2..4].iter().all(|b| b.is_ascii_digit())
        || !compact.iter().all(|b| b.is_ascii_alphanumeric())
    {
        return false;
    }
    let Ok(country) = std::str::from_utf8(&compact[..2]) else {
        return false;
    };
    if iban_length(country).is_some_and(|len| len != compact.len()) {
        return false;
    }
    // Move the first four characters to the end and read the result mod 97.
    let mut rem: u32 = 0;
    for b in compact[4..].iter().chain(compact[..4].iter()) {
        rem = if b.is_ascii_digit() {
            (rem * 10 + u32::from(b - b'0')) % 97
        } else {
            (rem * 100 + u32::from(b - b'A') + 10) % 97
        };
    }
    rem == 1
}

/// Payment card: WS12's card rule, [`quarry_core::redaction::luhn::is_pan`]. A
/// 13 to 19 digit number with a known issuer prefix and length that passes Luhn.
/// Single spaces and dashes between digits are ignored.
#[must_use]
pub fn is_payment_card(s: &str) -> bool {
    is_pan(s)
}

/// Norwegian national identity number: 11 digits written `DDMMYY IIISKK`, with
/// an optional single space after the date. A D-number adds 40 to the day and
/// an H-number adds 40 to the month. Both control digits are mod-11 checks, and
/// a remainder of 10 is never a valid control digit.
#[must_use]
pub fn is_fodselsnummer(s: &str) -> bool {
    let bytes = s.as_bytes();
    let digits: Vec<u8> = match bytes.len() {
        11 => bytes.to_vec(),
        12 if bytes[6] == b' ' => bytes
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 6)
            .map(|(_, b)| *b)
            .collect(),
        _ => return false,
    };
    if !digits.iter().all(u8::is_ascii_digit) {
        return false;
    }
    let d: Vec<u32> = digits.iter().map(|b| u32::from(b - b'0')).collect();
    let day = d[0] * 10 + d[1];
    let month = d[2] * 10 + d[3];
    let day = if day > 40 { day - 40 } else { day };
    let month = if month > 40 { month - 40 } else { month };
    if !(1..=31).contains(&day) || !(1..=12).contains(&month) {
        return false;
    }
    if control(&d[..9], &[3, 7, 6, 1, 8, 9, 4, 5, 2]) != Some(d[9]) {
        return false;
    }
    control(&d[..10], &[5, 4, 3, 2, 7, 6, 5, 4, 3, 2]) == Some(d[10])
}

/// One mod-11 control digit over `digits` with `weights`; `None` when the
/// remainder is 10, which no control digit can express.
fn control(digits: &[u32], weights: &[u32]) -> Option<u32> {
    let sum: u32 = digits.iter().zip(weights).map(|(d, w)| d * w).sum();
    match 11 - sum % 11 {
        11 => Some(0),
        10 => None,
        k => Some(k),
    }
}

/// Compile a constant pattern. The patterns are fixed in this file and the
/// golden tests compile every one of them, so a failure is a programming error.
#[allow(clippy::expect_used)]
fn compile(pattern: &str) -> Regex {
    Regex::new(pattern).expect("static PII pattern compiles")
}

// Candidate patterns. Every one is bounded (no unbounded repetition across a
// whole run), so a scan costs about the text length times a constant.
static EMAIL_RE: LazyLock<Regex> = LazyLock::new(|| {
    compile(
        r"[A-Za-z0-9._%+-]{1,64}@[A-Za-z0-9-]{1,63}(?:\.[A-Za-z0-9-]{1,63}){0,4}\.[A-Za-z]{2,24}",
    )
});
// Upper-case only: IBAN is printed in capitals. Groups of 1 to 4 characters,
// optionally space-separated; a trailing word is trimmed by `iban_prefix_len`.
static IBAN_RE: LazyLock<Regex> =
    LazyLock::new(|| compile(r"\b[A-Z]{2}[0-9]{2}(?: ?[A-Z0-9]{1,4}){2,12}\b"));
static CARD_RE: LazyLock<Regex> = LazyLock::new(|| compile(r"\b[0-9](?:[ -]?[0-9]){12,18}\b"));
static FNR_RE: LazyLock<Regex> = LazyLock::new(|| compile(r"\b[0-9]{6} ?[0-9]{5}\b"));
static PHONE_INTL_RE: LazyLock<Regex> = LazyLock::new(|| compile(r"\+[0-9][0-9 ().-]{5,24}[0-9]"));
static PHONE_NATIONAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    compile(r"\b[2-9][0-9]{2} [0-9]{2} [0-9]{3}\b|\b[2-9][0-9] [0-9]{2} [0-9]{2} [0-9]{2}\b")
});

/// The longest prefix of an IBAN candidate that is a valid IBAN. The regular
/// expression is greedy, so a word that follows the number is trimmed here one
/// space-separated token at a time.
fn iban_prefix_len(candidate: &str) -> Option<usize> {
    let mut end = candidate.len();
    loop {
        let current = &candidate[..end];
        if is_iban(current) {
            return Some(end);
        }
        end = current.rfind(' ')?;
    }
}

/// The next character boundary after `start`, so a rejected candidate does not
/// hide a valid one that begins inside it.
fn next_boundary(text: &str, start: usize) -> usize {
    start + text[start..].chars().next().map_or(1, char::len_utf8)
}

fn overlaps(found: &[PiiFinding], start: usize, end: usize) -> bool {
    found.iter().any(|f| start < f.end && f.start < end)
}

/// Visit every candidate of `re` in `text`. A candidate the validator accepts
/// claims its span (`validate` returns the accepted length from the candidate's
/// start). A rejected candidate is retried from the next character, so a valid
/// number that begins inside a longer rejected run is still found.
fn scan_kind(
    text: &str,
    re: &Regex,
    kind: PiiKind,
    validate: impl Fn(&str) -> Option<usize>,
    found: &mut Vec<PiiFinding>,
) {
    let mut pos = 0;
    while let Some(m) = re.find_at(text, pos) {
        let start = m.start();
        match validate(&text[start..m.end()]) {
            Some(len) => {
                let end = start + len;
                if !overlaps(found, start, end) {
                    found.push(PiiFinding { kind, start, end });
                }
                pos = end;
            }
            None => pos = next_boundary(text, start),
        }
        if pos >= text.len() {
            break;
        }
    }
}

/// Every validated finding in `text`, sorted by start and non-overlapping.
///
/// The kinds are scanned in a fixed order (email, IBAN, card, national identity
/// number, international phone, national phone). An earlier kind claims its
/// bytes, and a later candidate that overlaps a claimed span is dropped.
#[must_use]
pub fn scan(text: &str) -> Vec<PiiFinding> {
    let mut found = Vec::new();
    scan_kind(
        text,
        &EMAIL_RE,
        PiiKind::Email,
        |s| is_email(s).then_some(s.len()),
        &mut found,
    );
    scan_kind(text, &IBAN_RE, PiiKind::Iban, iban_prefix_len, &mut found);
    scan_kind(
        text,
        &CARD_RE,
        PiiKind::PaymentCard,
        |s| is_payment_card(s).then_some(s.len()),
        &mut found,
    );
    scan_kind(
        text,
        &FNR_RE,
        PiiKind::Fodselsnummer,
        |s| is_fodselsnummer(s).then_some(s.len()),
        &mut found,
    );
    scan_kind(
        text,
        &PHONE_INTL_RE,
        PiiKind::Phone,
        |s| is_phone(s).then_some(s.len()),
        &mut found,
    );
    scan_kind(
        text,
        &PHONE_NATIONAL_RE,
        PiiKind::Phone,
        |s| is_phone(s).then_some(s.len()),
        &mut found,
    );
    found.sort_by_key(|f| f.start);
    found
}

/// `text` with every finding replaced by its marker. Text outside the findings
/// is copied byte for byte.
#[must_use]
pub fn redact(text: &str) -> String {
    let findings = scan(text);
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for f in &findings {
        out.push_str(&text[last..f.start]);
        out.push_str(f.kind.marker());
        last = f.end;
    }
    out.push_str(&text[last..]);
    out
}
