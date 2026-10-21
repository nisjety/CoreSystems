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

/// HTTP header names whose value is a credential, with the marker kind each
/// gets. Unlike `KEY_RULES` this runs under both profiles: a header dumped as
/// JSON (`{"Authorization": "Bearer x"}`, or a `{"name": "Cookie", "value":
/// "..."}` pair as HAR and CDP write it) has no header word inside the string
/// leaf, so the text rules cannot see the header context, and the name is
/// unambiguous.
///
/// The whole value is replaced, whatever its shape: an object or array under one
/// of these keys (a structured `authorization` config, say) is masked as a
/// unit rather than walked, because a credential can sit in any child.
const HEADER_KEYS: [(&str, MarkerKind); 6] = [
    ("authorization", MarkerKind::Credential),
    ("proxy-authorization", MarkerKind::Credential),
    ("cookie", MarkerKind::Secret),
    ("set-cookie", MarkerKind::Secret),
    ("x-api-key", MarkerKind::Key),
    ("x-auth-token", MarkerKind::Token),
];
/// Keys that hold the header name in a `{name, value}` pair.
const HEADER_NAME_KEYS: [&str; 2] = ["name", "key"];

/// The marker kind for a credential header name, in any case.
fn header_kind(s: &str) -> Option<MarkerKind> {
    let s = s.trim();
    HEADER_KEYS
        .iter()
        .find(|(h, _)| s.eq_ignore_ascii_case(h))
        .map(|(_, kind)| *kind)
}

/// If this object looks like `{"name": "Cookie", "value": ...}`, the kind to
/// mask its `value` with.
fn header_pair_kind(map: &serde_json::Map<String, Value>) -> Option<MarkerKind> {
    map.iter().find_map(|(k, v)| {
        if HEADER_NAME_KEYS.iter().any(|n| k.eq_ignore_ascii_case(n)) {
            v.as_str().and_then(header_kind)
        } else {
            None
        }
    })
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
            let pair = header_pair_kind(map);
            for (key, v) in map.iter_mut() {
                let header = header_kind(key).or(if key.eq_ignore_ascii_case("value") {
                    pair
                } else {
                    None
                });
                if let Some(kind) = header {
                    // Always settles: a flag or an absent value is left, the rest is replaced.
                    mask_keyed_value(v, kind, tally);
                    continue;
                }
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
    let mut parts = vec![
        format!("json.max_depth|{MAX_DEPTH}"),
        format!(
            "json.header_keys|{}",
            HEADER_KEYS
                .iter()
                .map(|(h, k)| format!("{h}:{k:?}"))
                .collect::<Vec<_>>()
                .join(",")
        ),
        format!("json.header_name_keys|{}", HEADER_NAME_KEYS.join(",")),
    ];
    parts.extend(
        KEY_RULES
            .into_iter()
            .map(|(k, p)| format!("json.key|{k:?}|{p}")),
    );
    parts.push(format!("json.behaviour|{}", behaviour_probe()));
    parts
}

/// What the walk does to a fixed set of values under both profiles, as text.
/// Part of the drift fingerprint, so a change in how keys, header pairs, depth
/// or flags are treated counts as a rule change even when no data table moved.
fn behaviour_probe() -> String {
    use serde_json::json;
    let probes = [
        json!({"Authorization": "Bearer abcdefghijklmnopqrstuvwx"}),
        json!({"proxy-authorization": "Basic dXNlcjpwYXNz", "x": 1}),
        json!({"headers": [{"name": "Authorization", "value": "Bearer abcdefghijklmnopqrstuvwx"}, {"name": "Accept", "value": "text/html"}]}),
        json!({"key": "authorization", "value": 7}),
        json!({"authorization": true, "Authorization": null, "AUTHORIZATION": ""}),
        json!({"password": "hunter two", "max_tokens": 4096, "accessToken": "abc", "tokens": ["a"]}),
        json!({"nested": [{"secret": {"a": 1}}, "password=hunter2"]}),
        json!({"Cookie": "sid=abcdefgh", "set-cookie": ["a=b"], "X-Api-Key": "kkkkkkkkkkkk", "x-auth-token": 7, "cookies_enabled": true}),
        json!({"headers": [{"name": "Cookie", "value": "sid=abcdefgh"}, {"key": "X-API-KEY", "value": "kkkkkkkkkkkk"}, {"name": "Accept", "value": "text/html"}]}),
        json!({"authorization": {"scheme": "Bearer", "credentials": ["abcdefgh"]}}),
    ];
    let mut out = String::new();
    for profile in [Profile::Live, Profile::Durable] {
        for p in &probes {
            let mut v = p.clone();
            let mut t = Tally::new();
            redact_value(&mut v, profile, &mut t);
            out.push_str(&format!("{profile:?}={v}/{};", t.values_masked()));
        }
    }
    out
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

    fn live(mut v: Value) -> (Value, Tally) {
        let mut t = Tally::new();
        redact_value(&mut v, Profile::Live, &mut t);
        (v, t)
    }

    #[test]
    fn an_authorization_header_object_is_masked_under_both_profiles() {
        // Letters only after the scheme: the text rules need a digit or a
        // symbol to call a bare token a secret, so only the key can catch these.
        for input in [
            json!({"Authorization": "Bearer abcdefghijklmnopqrstuvwxyz"}),
            json!({"authorization": "Basic dXNlcjpwYXNz"}),
            json!({"Proxy-Authorization": "Basic dXNlcjpwYXNz"}),
            json!({"headers": {"AUTHORIZATION": "Bearer abcdefghijklmnopqrstuvwxyz", "Accept": "text/html"}}),
        ] {
            let (l, lt) = live(input.clone());
            let (d, dt) = durable(input.clone());
            assert_eq!(l, d, "{input}");
            assert_eq!(lt.by_rule().get("json.key"), Some(&1), "{input}");
            assert_eq!(dt.by_rule().get("json.key"), Some(&1), "{input}");
            assert!(l.to_string().contains("[redacted:credential]"), "{l}");
            assert!(!l.to_string().contains("abcdefgh"), "{l}");
            assert!(!l.to_string().contains("dXNlcjpw"), "{l}");
        }
    }

    #[test]
    fn a_name_value_header_pair_masks_the_value_only() {
        let (v, t) = live(json!({"headers": [
            {"name": "Authorization", "value": "Bearer abcdefghijklmnopqrstuvwxyz"},
            {"name": "Accept", "value": "text/html"},
            {"key": "Proxy-Authorization", "value": "Basic dXNlcjpwYXNz"},
        ]}));
        assert_eq!(v["headers"][0]["value"], "[redacted:credential]");
        assert_eq!(v["headers"][0]["name"], "Authorization");
        assert_eq!(v["headers"][1]["value"], "text/html");
        assert_eq!(v["headers"][2]["value"], "[redacted:credential]");
        assert_eq!(t.by_rule().get("json.key"), Some(&2));
    }

    #[test]
    fn cookie_and_api_key_headers_are_masked_under_both_profiles_with_their_own_kind() {
        let input = json!({
            "Cookie": "sid=abcdefghijklmnop",
            "set-cookie": "a=b; HttpOnly",
            "X-Api-Key": "kkkkkkkkkkkkkkkk",
            "X-AUTH-TOKEN": "tttttttttttttttt",
            "Accept": "text/html",
            "cookies_enabled": true,
        });
        let (l, lt) = live(input.clone());
        let (d, dt) = durable(input);
        assert_eq!(l, d);
        assert_eq!(l["Cookie"], "[redacted:secret]");
        assert_eq!(l["set-cookie"], "[redacted:secret]");
        assert_eq!(l["X-Api-Key"], "[redacted:key]");
        assert_eq!(l["X-AUTH-TOKEN"], "[redacted:token]");
        assert_eq!(l["Accept"], "text/html");
        assert_eq!(l["cookies_enabled"], true);
        assert_eq!(lt.by_rule().get("json.key"), Some(&4));
        assert_eq!(dt.by_rule().get("json.key"), Some(&4));
    }

    #[test]
    fn a_cookie_name_value_pair_masks_the_value_only() {
        let (v, t) = live(json!({"headers": [
            {"name": "Cookie", "value": "sid=abcdefghijklmnop"},
            {"key": "X-API-KEY", "value": "kkkkkkkkkkkkkkkk"},
            {"name": "Accept", "value": "text/html"},
        ]}));
        assert_eq!(v["headers"][0]["name"], "Cookie");
        assert_eq!(v["headers"][0]["value"], "[redacted:secret]");
        assert_eq!(v["headers"][1]["value"], "[redacted:key]");
        assert_eq!(v["headers"][2]["value"], "text/html");
        assert_eq!(t.by_rule().get("json.key"), Some(&2));
    }

    #[test]
    fn a_structured_value_under_a_header_key_is_masked_whole() {
        // Over-masking by design: a credential can sit in any child, and the
        // key name is the only context the walk has.
        for profile in [Profile::Live, Profile::Durable] {
            let mut v = json!({
                "authorization": {"scheme": "Bearer", "credentials": ["abcdefgh"]},
                "cookie": ["a=b", "c=d"],
                "other": {"kept": "yes"},
            });
            let mut t = Tally::new();
            redact_value(&mut v, profile, &mut t);
            assert_eq!(v["authorization"], "[redacted:credential]", "{profile:?}");
            assert_eq!(v["cookie"], "[redacted:secret]", "{profile:?}");
            assert_eq!(v["other"]["kept"], "yes", "{profile:?}");
            assert_eq!(t.by_rule().get("json.key"), Some(&2), "{profile:?}");
        }
    }

    #[test]
    fn header_keys_with_nothing_to_hide_and_other_names_are_left_alone() {
        let input = json!({
            "authorization": true,
            "Authorization": null,
            "AUTHORIZATION": "",
            "authorization_required": "yes",
            "name": "Content-Type",
            "value": "text/html",
        });
        let (v, t) = live(input.clone());
        assert_eq!(v, input);
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
