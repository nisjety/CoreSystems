//! Luhn check and payment-card number (PAN) recognition.
//!
//! A PAN here is 13 to 19 digits that start with a known issuer prefix, have a
//! length that issuer uses, and pass the Luhn checksum. All three conditions
//! are needed: Luhn alone passes one random number in ten, so order numbers,
//! hashes and identifiers would be masked at random.

use std::ops::Range;

/// The Luhn checksum over a string of ASCII digits. An empty string or any
/// other character fails.
#[must_use]
pub fn luhn_ok(digits: &str) -> bool {
    if digits.is_empty() {
        return false;
    }
    let mut sum = 0u32;
    let mut double = false;
    for b in digits.bytes().rev() {
        if !b.is_ascii_digit() {
            return false;
        }
        let mut d = u32::from(b - b'0');
        if double {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
        double = !double;
    }
    sum % 10 == 0
}

/// Issuer prefix and length table: Visa 4, Mastercard 51-55 and 2221-2720,
/// Amex 34/37, Discover 6011/65/644-649, JCB 3528-3589, Diners 30/36/38,
/// UnionPay 62.
fn issuer_ok(d: &str) -> bool {
    let len = d.len();
    let num = |n: usize| d[..n].parse::<u32>().unwrap_or(0);
    if d.starts_with('4') {
        return matches!(len, 13 | 16 | 19);
    }
    if len == 16 && ((51..=55).contains(&num(2)) || (2221..=2720).contains(&num(4))) {
        return true;
    }
    if len == 15 && matches!(num(2), 34 | 37) {
        return true;
    }
    if (16..=19).contains(&len) && (num(4) == 6011 || num(2) == 65 || (644..=649).contains(&num(3)))
    {
        return true;
    }
    if (16..=19).contains(&len) && (3528..=3589).contains(&num(4)) {
        return true;
    }
    if (14..=19).contains(&len) && matches!(num(2), 30 | 36 | 38) {
        return true;
    }
    (16..=19).contains(&len) && num(2) == 62
}

/// Is this text a payment card number? Single spaces and dashes between digits
/// are ignored; any other character makes it a no.
#[must_use]
pub fn is_pan(s: &str) -> bool {
    let mut digits = String::with_capacity(19);
    for c in s.chars() {
        match c {
            '0'..='9' => {
                digits.push(c);
                if digits.len() > 19 {
                    return false;
                }
            }
            ' ' | '-' => {}
            _ => return false,
        }
    }
    (13..=19).contains(&digits.len()) && issuer_ok(&digits) && luhn_ok(&digits)
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// One maximal run of ASCII digits.
#[derive(Clone, Copy)]
struct Group {
    start: usize,
    end: usize,
}

impl Group {
    fn len(self) -> usize {
        self.end - self.start
    }
}

fn judge_chain(text: &str, chain: &[Group], out: &mut Vec<Range<usize>>) {
    let mut i = 0;
    while i < chain.len() {
        let mut found = None;
        let last = (chain.len() - 1).min(i + 6);
        for j in (i..=last).rev() {
            let window = &chain[i..=j];
            let total: usize = window.iter().map(|g| g.len()).sum();
            if total < 13 {
                break;
            }
            if total > 19 {
                continue;
            }
            // A single group is a plain digit run. Several groups must look
            // like card grouping (4-4-4-4, 4-6-5, 4-4-4-4-3), which keeps the
            // chunks of a UUID or a date range out.
            if window.len() > 1 && window.iter().any(|g| !(3..=6).contains(&g.len())) {
                continue;
            }
            let (start, end) = (window[0].start, window[window.len() - 1].end);
            let before = text[..start].chars().next_back();
            let after = text[end..].chars().next();
            // A digit run glued to a letter is part of an identifier or a hash.
            if before.is_some_and(is_word) || after.is_some_and(is_word) {
                continue;
            }
            if is_pan(&text[start..end]) {
                found = Some((j, start..end));
                break;
            }
        }
        match found {
            Some((j, range)) => {
                out.push(range);
                i = j + 1;
            }
            None => i += 1,
        }
    }
}

/// Byte ranges of the payment card numbers in `text`, in order and without
/// overlap. A card number may be written as one digit run or as groups joined
/// by a single space or dash.
#[must_use]
pub fn find_pan_runs(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut chain: Vec<Group> = Vec::new();
    let mut p = 0;
    while p < bytes.len() {
        if !bytes[p].is_ascii_digit() {
            p += 1;
            continue;
        }
        let start = p;
        while p < bytes.len() && bytes[p].is_ascii_digit() {
            p += 1;
        }
        chain.push(Group { start, end: p });
        let joined =
            p + 1 < bytes.len() && matches!(bytes[p], b' ' | b'-') && bytes[p + 1].is_ascii_digit();
        if joined {
            p += 1;
        } else {
            judge_chain(text, &chain, &mut out);
            chain.clear();
        }
    }
    if !chain.is_empty() {
        judge_chain(text, &chain, &mut out);
    }
    out
}

/// Group layouts of two Luhn-valid numbers (4111111111111111 and
/// 378282246310005) at the edges of the card-like group-size rule, and what
/// `find_pan_runs` does with each, as text. Part of the drift fingerprint.
pub(crate) fn group_behaviour_probe() -> String {
    const LAYOUTS: &[&str] = &[
        "4111 1111 1111 1111",
        "411 111 111 111 111 1",
        "41 11 11 11 11 11 11 11",
        "41111 11111 11111 1",
        "411111 111111 1111",
        "4111111 11111 1111",
        "4111-1111-1111-1111",
        "4111  1111 1111 1111",
        "378 282 246 310 005",
        "3782 822463 10005",
        "37 8282 2463 1000 5",
        "3782822 463 10005",
        "378282246310005",
    ];
    LAYOUTS
        .iter()
        .map(|t| format!("{t}={:?};", find_pan_runs(t)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_sizes_of_three_to_six_are_card_like_and_two_or_seven_are_not() {
        let runs = |t: &str| find_pan_runs(t).len();
        // 3 and 6 digit groups are accepted inside a multi-group run.
        assert_eq!(runs("378 282 246 310 005"), 1, "five groups of 3");
        assert_eq!(runs("411111 111111 1111"), 1, "6-6-4");
        assert_eq!(runs("3782 822463 10005"), 1, "Amex 4-6-5");
        // A 2 digit group or a 7 digit group makes the run an identifier.
        assert_eq!(runs("37 8282 2463 1000 5"), 0, "a group of 2");
        assert_eq!(runs("4111111 11111 1111"), 0, "a group of 7");
        assert_eq!(runs("3782822 463 10005"), 0, "a group of 7");
        assert_eq!(runs("41 11 11 11 11 11 11 11"), 0, "groups of 2");
        // The same digits as one run are a card either way.
        assert_eq!(runs("378282246310005"), 1);
        assert_eq!(runs("4111111111111111"), 1);
    }

    #[test]
    fn luhn_accepts_published_test_numbers_and_rejects_a_changed_digit() {
        assert!(luhn_ok("4111111111111111"));
        assert!(luhn_ok("378282246310005"));
        assert!(!luhn_ok("4111111111111112"));
        assert!(!luhn_ok(""));
        assert!(!luhn_ok("41x1"));
    }

    #[test]
    fn is_pan_needs_issuer_length_and_checksum() {
        assert!(is_pan("4111 1111 1111 1111"));
        assert!(is_pan("4012-8888-8888-1881"));
        assert!(is_pan("5555555555554444"));
        assert!(is_pan("378282246310005"));
        // Luhn fails.
        assert!(!is_pan("4111111111111112"));
        // Luhn passes (all zeros do) but no issuer owns the prefix.
        assert!(!is_pan("0000000000000000"));
        // Amex prefix with a Visa length.
        assert!(!is_pan("3782822463100050"));
        assert!(!is_pan("4111 1111 1111 111x"));
    }

    #[test]
    fn find_runs_keeps_the_text_around_a_number() {
        let t = "card 4111 1111 1111 1111 exp 12/30";
        let r = find_pan_runs(t);
        assert_eq!(r.len(), 1);
        assert_eq!(&t[r[0].clone()], "4111 1111 1111 1111");
    }

    #[test]
    fn find_runs_leaves_a_trailing_number_alone() {
        let t = "4111 1111 1111 1111 42";
        let r = find_pan_runs(t);
        assert_eq!(r.len(), 1);
        assert_eq!(&t[r[0].clone()], "4111 1111 1111 1111");
    }

    #[test]
    fn find_runs_ignores_identifiers_and_dates() {
        for t in [
            "id4111111111111111",
            "a4111111111111111b",
            "4111111111111111x",
            "123e4567-e89b-12d3-a456-426614174000",
            "2026-10-21 12:30:45",
            "4111111111111112",
        ] {
            assert!(find_pan_runs(t).is_empty(), "{t}");
        }
    }

    #[test]
    fn find_runs_handles_a_long_chain_without_blowing_up() {
        let t = "1 ".repeat(50_000);
        assert!(find_pan_runs(&t).is_empty());
    }
}
