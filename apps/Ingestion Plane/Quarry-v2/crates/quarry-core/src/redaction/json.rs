//! JSON redaction: every string leaf goes through the text rules, and under
//! `Durable` the value of an object key that names a secret is replaced whole.
//!
//! The key rule is ported from the Model Plane scrubber (`scrub.rs`, read only)
//! and narrowed so that counters survive: `max_tokens`, `token_count` and
//! `tokens_used` do not match, `token`, `access_token` and `accessToken` do. It
//! runs under `Durable` only. Under `Live` it would blank legitimate extraction
//! fields such as `password_requirements`.
//!
//! Leaves of any size are scanned whole: the engine is linear-time and the
//! perf budget test covers 1 MiB, so there is no size at which a leaf is
//! skipped or truncated.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

use super::{is_marker, mask, shapes, MarkerKind, Profile, Tally};

/// Objects nested deeper than this are replaced whole (fail closed) rather than
/// walked: the walk is recursive and a hand-built value has no depth limit.
const MAX_DEPTH: usize = 256;

/// Key rules in order, first hit wins. The env-style names the Model Plane
/// scrubber covers (`*_KEY`, `*_TOKEN`, `*_SECRET`, `*_PASSWORD`, `*_PWD`) are
/// all covered here: only `*_KEY` needs its own case-sensitive pattern.
const KEY_RULES: [(MarkerKind, &str); 6] = [
    (
        MarkerKind::Password,
        r"(?i:password|passwd|passord|^pwd$|_pwd$)",
    ),
    (MarkerKind::Credential, r"(?i:credential)"),
    (
        MarkerKind::Key,
        r"(?i:api[_-]?key|private[_-]?key|access[_-]?key)|^[A-Z][A-Z0-9_]*_KEY$",
    ),
    // `token`, `access_token`, `x-csrf-token`, `ID_TOKEN`; not `tokens` or `token_count`.
    (MarkerKind::Token, r"(?i:(?:^|[_\-. ])token$)"),
    // `accessToken`, `idToken`.
    (MarkerKind::Token, r"[a-z]Token$"),
    (MarkerKind::Secret, r"(?i:secret|session[_-]?id|cookie)"),
];

fn key_rules() -> &'static [(MarkerKind, Regex)] {
    static R: OnceLock<Vec<(MarkerKind, Regex)>> = OnceLock::new();
    R.get_or_init(|| {
        KEY_RULES
            .into_iter()
            .map(|(k, p)| {
                (
                    k,
                    Regex::new(p)
                        .unwrap_or_else(|e| panic!("json key rule {p} does not compile: {e}")),
                )
            })
            .collect()
    })
}

/// The marker kind for an object key that names a secret.
fn key_kind(key: &str) -> Option<MarkerKind> {
    key_rules()
        .iter()
        .find(|(_, re)| re.is_match(key))
        .map(|(kind, _)| *kind)
}

/// Replace the value under a sensitive key. Returns `true` when the value is
/// settled (replaced, or nothing to replace) and must not be walked.
fn mask_keyed_value(value: &mut Value, kind: MarkerKind, tally: &mut Tally) -> bool {
    match value {
        // A flag or an absent value holds no secret.
        Value::Bool(_) | Value::Null => true,
        Value::String(s) => {
            if s.is_empty() || is_marker(s) {
                return true;
            }
            *value = Value::String(mask(kind).to_owned());
            tally.add("json.key");
            true
        }
        Value::Number(_) | Value::Array(_) | Value::Object(_) => {
            *value = Value::String(mask(kind).to_owned());
            tally.add("json.key");
            true
        }
    }
}

fn walk(value: &mut Value, profile: Profile, tally: &mut Tally, depth: usize) {
    if depth > MAX_DEPTH {
        *value = Value::String(mask(MarkerKind::Secret).to_owned());
        tally.add("json.key");
        return;
    }
    match value {
        Value::String(s) => {
            let redacted = shapes::redact(s, profile, tally);
            if let std::borrow::Cow::Owned(new) = redacted {
                *s = new;
            }
        }
        Value::Array(items) => {
            for item in items {
                walk(item, profile, tally, depth + 1);
            }
        }
        Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                if profile == Profile::Durable {
                    if let Some(kind) = key_kind(key) {
                        if mask_keyed_value(v, kind, tally) {
                            continue;
                        }
                    }
                }
                walk(v, profile, tally, depth + 1);
            }
        }
        _ => {}
    }
}

/// Redact a JSON value in place in enforce mode (the mode switch is in the
/// parent module).
pub(crate) fn redact_value(value: &mut Value, profile: Profile, tally: &mut Tally) {
    walk(value, profile, tally, 0);
}

pub(crate) fn fingerprint_parts() -> Vec<String> {
    let mut parts = vec![format!("json.max_depth|{MAX_DEPTH}")];
    parts.extend(
        KEY_RULES
            .into_iter()
            .map(|(k, p)| format!("json.key|{k:?}|{p}")),
    );
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn durable(mut v: Value) -> (Value, Tally) {
        let mut t = Tally::new();
        redact_value(&mut v, Profile::Durable, &mut t);
        (v, t)
    }

    #[test]
    fn keys_that_name_secrets_are_replaced_and_counters_survive() {
        let (v, t) = durable(json!({
            "password": "hunter2",
            "accessToken": "abc123",
            "access_token": 123456,
            "max_tokens": 4096,
            "token_count": 12,
            "DB_PASSWORD": "x",
            "OPENAI_API_KEY": "y",
            "name": "visible",
            "cookies_enabled": true,
        }));
        assert_eq!(v["password"], "[redacted:password]");
        assert_eq!(v["accessToken"], "[redacted:token]");
        assert_eq!(v["access_token"], "[redacted:token]");
        assert_eq!(v["max_tokens"], 4096);
        assert_eq!(v["token_count"], 12);
        assert_eq!(v["DB_PASSWORD"], "[redacted:password]");
        assert_eq!(v["OPENAI_API_KEY"], "[redacted:key]");
        assert_eq!(v["name"], "visible");
        assert_eq!(v["cookies_enabled"], true);
        assert_eq!(t.by_rule().get("json.key"), Some(&5));
    }

    #[test]
    fn live_leaves_keys_alone_but_still_masks_shapes_in_leaves() {
        let mut v = json!({"password_requirements": "at least 8 characters", "note": "ping"});
        let mut t = Tally::new();
        redact_value(&mut v, Profile::Live, &mut t);
        assert_eq!(v["password_requirements"], "at least 8 characters");
        assert!(t.is_empty());
    }

    #[test]
    fn a_second_pass_is_a_no_op() {
        let (first, _) =
            durable(json!({"password": "x", "nested": [{"secret": {"a": 1}}, "password=hunter2"]}));
        let (second, t) = durable(first.clone());
        assert_eq!(first, second);
        assert!(t.is_empty());
    }

    #[test]
    fn a_value_nested_past_the_depth_limit_is_replaced_whole() {
        let mut v = json!("leaf");
        for _ in 0..(MAX_DEPTH + 5) {
            v = json!([v]);
        }
        let (out, t) = durable(v);
        assert!(!t.is_empty());
        assert!(out.to_string().contains("[redacted:secret]"));
        assert!(!out.to_string().contains("leaf"));
    }
}
