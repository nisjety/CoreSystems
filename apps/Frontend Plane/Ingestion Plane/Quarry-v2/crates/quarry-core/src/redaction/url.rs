//! URL redaction: keep the scheme, host, path and key names, mask the values
//! that carry secrets.
//!
//! * A userinfo password (`scheme://user:SECRET@host`) is masked under both
//!   profiles (`shape.userinfo`).
//! * Under `Durable`, the value of a query key in [`QUERY_KEYS`] (or starting
//!   with `x-amz-`) is masked (`url.query`), and so is the value of a fragment
//!   key in [`FRAGMENT_KEYS`] (`url.fragment`, an OAuth implicit-flow
//!   `#access_token=...`).
//! * Input that is not an absolute URL goes through the text rules, which
//!   carry the same query and fragment rules for URLs embedded in prose.
//!
//! The work is done on the string, never by re-serializing a parsed URL, so a
//! URL that does not change comes back byte-identical and a masked one stays
//! idempotent.

use std::borrow::Cow;

use super::{mask, shapes, MarkerKind, Profile, Tally, MARKER_PREFIX};

/// Query keys whose value is a secret (matched case-insensitively, after
/// percent-decoding the key). Keys starting with [`QUERY_KEY_PREFIXES`] too.
pub const QUERY_KEYS: &[&str] = &[
    "token",
    "access_token",
    "id_token",
    "refresh_token",
    "code",
    "key",
    "api_key",
    "apikey",
    "sig",
    "signature",
    "password",
    "secret",
    "session",
    "sid",
    "otp",
    "jwt",
];

/// Query key prefixes that are always secrets (`X-Amz-Signature`, `X-Amz-Credential`).
pub const QUERY_KEY_PREFIXES: &[&str] = &["x-amz-"];

/// Fragment keys whose value is a secret.
pub const FRAGMENT_KEYS: &[&str] = &["access_token", "id_token", "refresh_token", "token", "code"];

fn is_query_key(k: &str) -> bool {
    QUERY_KEYS.contains(&k) || QUERY_KEY_PREFIXES.iter().any(|p| k.starts_with(p))
}

fn is_fragment_key(k: &str) -> bool {
    FRAGMENT_KEYS.contains(&k)
}

/// Percent-decode and lowercase a key.
fn decode_key(k: &str) -> Option<String> {
    ::url::form_urlencoded::parse(k.as_bytes())
        .next()
        .map(|(k, _)| k.to_ascii_lowercase())
}

/// Mask the secret values of `k=v&k=v` pairs. Returns the new string when any
/// value was masked.
fn mask_pairs(
    pairs: &str,
    is_secret_key: fn(&str) -> bool,
    rule: &'static str,
    tally: &mut Tally,
) -> Option<String> {
    let marker = mask(MarkerKind::Token);
    let mut changed = false;
    let rebuilt: Vec<Cow<'_, str>> = pairs
        .split('&')
        .map(|pair| {
            if let Some((k, v)) = pair.split_once('=') {
                let secret_key = decode_key(k).is_some_and(|k| is_secret_key(&k));
                if secret_key && !v.is_empty() && !v.starts_with(MARKER_PREFIX) {
                    tally.add(rule);
                    changed = true;
                    return Cow::Owned(format!("{k}={marker}"));
                }
            }
            Cow::Borrowed(pair)
        })
        .collect();
    changed.then(|| rebuilt.join("&"))
}

/// Mask query and fragment secrets by structure. `None` when nothing changed.
fn mask_structure(input: &str, tally: &mut Tally) -> Option<String> {
    let (rest, fragment) = match input.split_once('#') {
        Some((a, b)) => (a, Some(b)),
        None => (input, None),
    };
    let (base, query) = match rest.split_once('?') {
        Some((a, b)) => (a, Some(b)),
        None => (rest, None),
    };
    let query2 = query.and_then(|q| mask_pairs(q, is_query_key, "url.query", tally));
    let fragment2 = fragment.and_then(|f| mask_pairs(f, is_fragment_key, "url.fragment", tally));
    if query2.is_none() && fragment2.is_none() {
        return None;
    }
    let mut out = String::with_capacity(input.len() + 16);
    out.push_str(base);
    if let Some(q) = query2.as_deref().or(query) {
        out.push('?');
        out.push_str(q);
    }
    if let Some(f) = fragment2.as_deref().or(fragment) {
        out.push('#');
        out.push_str(f);
    }
    Some(out)
}

/// Redact a URL in enforce mode (the mode switch is in the parent module).
pub(crate) fn redact_url_enforce<'a>(
    input: &'a str,
    profile: Profile,
    tally: &mut Tally,
) -> Cow<'a, str> {
    if ::url::Url::parse(input).is_err() {
        return shapes::redact(input, profile, tally);
    }
    if profile == Profile::Durable {
        if let Some(masked) = mask_structure(input, tally) {
            return Cow::Owned(shapes::redact(&masked, profile, tally).into_owned());
        }
    }
    shapes::redact(input, profile, tally)
}

pub(crate) fn fingerprint_parts() -> Vec<String> {
    vec![
        format!("url.query_keys|{}", QUERY_KEYS.join(",")),
        format!("url.query_key_prefixes|{}", QUERY_KEY_PREFIXES.join(",")),
        format!("url.fragment_keys|{}", FRAGMENT_KEYS.join(",")),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn durable(s: &str) -> (String, Tally) {
        let mut t = Tally::new();
        let out = redact_url_enforce(s, Profile::Durable, &mut t).into_owned();
        (out, t)
    }

    #[test]
    fn query_values_are_masked_and_names_kept() {
        let (out, t) =
            durable("https://api.example.com/v1/items?page=2&access_token=abc123&limit=5");
        assert_eq!(
            out,
            "https://api.example.com/v1/items?page=2&access_token=[redacted:token]&limit=5"
        );
        assert_eq!(t.by_rule().get("url.query"), Some(&1));
    }

    #[test]
    fn fragment_secrets_are_masked() {
        let (out, t) =
            durable("https://app.example.com/cb#access_token=abc123&token_type=bearer&state=xyz");
        assert_eq!(
            out,
            "https://app.example.com/cb#access_token=[redacted:token]&token_type=bearer&state=xyz"
        );
        assert_eq!(t.by_rule().get("url.fragment"), Some(&1));
    }

    #[test]
    fn a_url_that_changes_nothing_is_returned_borrowed() {
        let mut t = Tally::new();
        let s = "https://example.com/search?q=rust&page=2#top";
        assert!(matches!(
            redact_url_enforce(s, Profile::Durable, &mut t),
            Cow::Borrowed(_)
        ));
        assert!(t.is_empty());
    }

    #[test]
    fn live_masks_userinfo_but_not_query() {
        let mut t = Tally::new();
        let out = redact_url_enforce(
            "https://svc:hunter2@h.example/p?token=abc123",
            Profile::Live,
            &mut t,
        );
        assert_eq!(
            out,
            "https://svc:[redacted:credential]@h.example/p?token=abc123"
        );
    }

    #[test]
    fn a_second_pass_is_a_no_op() {
        let (first, _) =
            durable("https://u:pw@h.example/p?code=abc123&X-Amz-Signature=deadbeef#id_token=zzz");
        let (second, t) = durable(&first);
        assert_eq!(first, second);
        assert!(t.is_empty());
    }

    #[test]
    fn a_relative_or_bare_string_goes_through_the_text_rules() {
        let (out, _) = durable("/cb?token=abc123");
        assert_eq!(out, "/cb?token=[redacted:token]");
    }

    #[test]
    fn percent_encoded_keys_are_decoded() {
        let (out, _) = durable("https://h.example/p?access%5Ftoken=abc123");
        assert_eq!(out, "https://h.example/p?access%5Ftoken=[redacted:token]");
    }

    #[test]
    fn the_structural_and_text_rules_agree_on_every_key() {
        for k in QUERY_KEYS
            .iter()
            .chain(&["x-amz-signature", "x-amz-credential"])
        {
            let as_url = durable(&format!("https://h.example/p?{k}=abc123")).0;
            let mut t = Tally::new();
            let as_text = shapes::redact(
                &format!("https://h.example/p?{k}=abc123"),
                Profile::Durable,
                &mut t,
            )
            .into_owned();
            assert_eq!(as_url, as_text, "{k}");
            assert!(as_url.ends_with("[redacted:token]"), "{k}: {as_url}");
        }
        for k in FRAGMENT_KEYS {
            let as_url = durable(&format!("https://h.example/p#{k}=abc123")).0;
            let mut t = Tally::new();
            let as_text = shapes::redact(
                &format!("https://h.example/p#{k}=abc123"),
                Profile::Durable,
                &mut t,
            )
            .into_owned();
            assert_eq!(as_url, as_text, "{k}");
            assert!(as_url.ends_with("[redacted:token]"), "{k}: {as_url}");
        }
    }
}
