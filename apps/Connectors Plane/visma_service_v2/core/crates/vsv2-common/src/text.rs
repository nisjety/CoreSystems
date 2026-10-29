//! Text that came from outside (upstream bodies, error messages) made safe to log and store.

use std::borrow::Cow;

use serde_json::Value;

/// Longest upstream body shown in an error message. Enough for a JSON error document, far less
/// than an HTML error page or a binary download.
pub const MAX_ERROR_BODY_CHARS: usize = 2048;

/// `text` as Postgres can store it.
///
/// A `text` column rejects NUL bytes and `jsonb` rejects `\u0000`, and upstream bodies (a
/// compressed download that failed to parse, say) can carry them. Without this the write fails,
/// the run or operation stays `running`, and the caller gets a state-store error instead of the
/// failure that actually happened.
pub fn storable_text(text: &str) -> Cow<'_, str> {
    if text.contains('\0') {
        Cow::Owned(text.replace('\0', "\u{FFFD}"))
    } else {
        Cow::Borrowed(text)
    }
}

/// `value` as a Postgres `jsonb` can store it: a NUL in any string (or key) is replaced by U+FFFD.
///
/// Visma data does contain them (an inventory item note did), and `jsonb` rejects `\u0000`, which
/// failed the whole endpoint. Only the local Postgres bronze needs this; it is borrowed unchanged
/// when there is nothing to fix.
pub fn storable_json(value: &Value) -> Cow<'_, Value> {
    if !json_has_nul(value) {
        return Cow::Borrowed(value);
    }
    Cow::Owned(strip_json_nul(value))
}

fn json_has_nul(value: &Value) -> bool {
    match value {
        Value::String(s) => s.contains('\0'),
        Value::Array(items) => items.iter().any(json_has_nul),
        Value::Object(map) => map.iter().any(|(k, v)| k.contains('\0') || json_has_nul(v)),
        _ => false,
    }
}

fn strip_json_nul(value: &Value) -> Value {
    match value {
        Value::String(s) => Value::String(s.replace('\0', "\u{FFFD}")),
        Value::Array(items) => Value::Array(items.iter().map(strip_json_nul).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.replace('\0', "\u{FFFD}"), strip_json_nul(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// The first `max` characters of `text` with control characters (other than whitespace)
/// replaced by `.`, for error messages that end up in logs and in `sync_run.error`.
pub fn printable(text: &str, max: usize) -> String {
    text.chars()
        .take(max)
        .map(|c| {
            if c.is_control() && !c.is_whitespace() {
                '.'
            } else {
                c
            }
        })
        .collect()
}

/// Characters that reorder or hide text: the bidirectional embeddings, overrides and isolates.
fn is_bidi_control(c: char) -> bool {
    matches!(
        c,
        '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}'
    )
}

/// Text from outside (an API's error answer, a person's display name, a company's name) as ONE line with no
/// control characters or bidirectional overrides, at most `max_chars` long (cut with a trailing `…`). For
/// terminals, logs, audit rows and what a model reads: it cannot add lines, move the cursor or pose as
/// something else. [`printable`] keeps whitespace and is for bodies that may legitimately span lines.
pub fn one_line(text: &str, max_chars: usize) -> String {
    let spaced: String = text
        .chars()
        .map(|c| {
            if c.is_control() || is_bidi_control(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    let collapsed = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }
    let mut cut: String = collapsed
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_line_removes_line_breaks_escapes_and_overrides() {
        assert_eq!(
            one_line("a\r\nb\tc\u{1b}[31md\u{202E}e", 100),
            "a b c [31md e"
        );
        assert_eq!(
            one_line("  many   spaces \n here ", 100),
            "many spaces here"
        );
        assert_eq!(one_line("", 6), "");
    }

    #[test]
    fn one_line_cuts_long_text_with_a_mark_and_leaves_short_text_alone() {
        assert_eq!(one_line("abcdef", 6), "abcdef");
        assert_eq!(one_line("abcdefg", 6), "abcde…");
        assert_eq!(one_line("ååååååå", 4).chars().count(), 4);
    }

    #[test]
    fn nul_bytes_are_replaced() {
        assert_eq!(storable_text("a\0b\0"), "a\u{FFFD}b\u{FFFD}");
    }

    #[test]
    fn clean_text_is_borrowed_unchanged() {
        let text = "upstream said 502";
        assert!(matches!(storable_text(text), Cow::Borrowed(t) if t == text));
    }

    #[test]
    fn printable_replaces_control_characters_and_nul() {
        let head = printable("ab\0cd\u{1f}ef\ngh", 100);
        assert_eq!(head, "ab.cd.ef\ngh");
        assert!(!head.contains('\0'));
    }

    #[test]
    fn json_without_nul_is_borrowed() {
        let value = serde_json::json!({"a": ["x", {"b": "y"}], "n": 1});
        assert!(matches!(storable_json(&value), Cow::Borrowed(_)));
    }

    #[test]
    fn json_nul_is_replaced_in_values_and_keys_at_any_depth() {
        let value = serde_json::json!({"note": "a\u{0}b", "items": [{"d": "\u{0}"}], "k\u{0}": 1});
        let clean = storable_json(&value).into_owned();
        assert_eq!(clean["note"], "a\u{FFFD}b");
        assert_eq!(clean["items"][0]["d"], "\u{FFFD}");
        assert_eq!(clean["k\u{FFFD}"], 1);
        assert!(!clean.to_string().contains("\\u0000"));
    }

    #[test]
    fn printable_truncates_on_characters_not_bytes() {
        assert_eq!(printable("æøåæøå", 3), "æøå");
    }
}
