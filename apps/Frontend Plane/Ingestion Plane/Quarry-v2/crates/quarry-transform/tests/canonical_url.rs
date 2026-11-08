//! Canonical URL helper and redirect reconciliation (WS14 / E3.7.6).
//!
//! Compile-checked only in the build phase (`cargo check --tests`); the suite
//! is run in the test phase of the wave.

use quarry_transform::canonical_url::{
    canonicalize, reconcile, request_id_for_canonical, request_id_for_url, CanonicalError,
    CanonicalOptions, CANONICAL_VERSION, MAX_INPUT_BYTES, REQUEST_ID_PREFIX,
};

const CONSERVATIVE: CanonicalOptions = CanonicalOptions {
    sort_query: false,
    strip_tracking_params: false,
};

const AGGRESSIVE: CanonicalOptions = CanonicalOptions {
    sort_query: true,
    strip_tracking_params: true,
};

fn canon(input: &str) -> String {
    canonicalize(input, CONSERVATIVE).expect("input should canonicalise")
}

#[test]
fn spellings_of_one_resource_share_a_canonical_form() {
    let cases = [
        (
            "HTTP://Example.COM:80/a/./b/../c?x=1#frag",
            "http://example.com/a/c?x=1",
        ),
        ("https://example.com:443", "https://example.com/"),
        ("https://example.com.", "https://example.com/"),
        ("https://Example.com/%7Euser", "https://example.com/~user"),
        ("https://example.com/a%41b", "https://example.com/aAb"),
        ("https://example.com/a%2fb", "https://example.com/a%2Fb"),
        ("https://example.com/?", "https://example.com/"),
        ("https://example.com/a?b=%7e", "https://example.com/a?b=~"),
        (
            "https://example.com/a?b=1&a=2",
            "https://example.com/a?b=1&a=2",
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(canon(input), expected, "input {input:?}");
    }
}

#[test]
fn conservative_mode_keeps_distinct_resources_distinct() {
    // A trailing slash, `www.`, `index.html` and query order are not folded
    // by default: each can name a different resource on the server.
    assert_ne!(
        canon("https://example.com/a"),
        canon("https://example.com/a/")
    );
    assert_ne!(
        canon("https://example.com/"),
        canon("https://www.example.com/")
    );
    assert_ne!(
        canon("https://example.com/index.html"),
        canon("https://example.com/")
    );
    assert_ne!(
        canon("https://example.com/p?a=1&b=2"),
        canon("https://example.com/p?b=2&a=1")
    );
}

#[test]
fn aggressive_options_sort_and_strip_tracking() {
    let out = canonicalize(
        "https://example.com/p?utm_source=x&b=2&a=1&gclid=9",
        AGGRESSIVE,
    )
    .unwrap();
    assert_eq!(out, "https://example.com/p?a=1&b=2");

    let sort_only = canonicalize(
        "https://example.com/p?utm_source=x&b=2&a=1",
        CanonicalOptions {
            sort_query: true,
            strip_tracking_params: false,
        },
    )
    .unwrap();
    assert_eq!(sort_only, "https://example.com/p?a=1&b=2&utm_source=x");
}

#[test]
fn tracking_keys_are_matched_case_insensitively() {
    let out = canonicalize(
        "https://example.com/p?UTM_Campaign=spring&FBCLID=abc",
        AGGRESSIVE,
    )
    .unwrap();
    assert_eq!(out, "https://example.com/p");
}

#[test]
fn stripping_every_parameter_drops_the_question_mark() {
    let out = canonicalize("https://example.com/p?utm_source=x", AGGRESSIVE).unwrap();
    assert_eq!(out, "https://example.com/p");
}

#[test]
fn refused_inputs_return_typed_classes() {
    assert_eq!(
        canonicalize("ftp://example.com/", CONSERVATIVE),
        Err(CanonicalError::UnsupportedScheme)
    );
    assert_eq!(
        canonicalize("javascript:alert(1)", CONSERVATIVE),
        Err(CanonicalError::UnsupportedScheme)
    );
    assert_eq!(
        canonicalize("mailto:someone@example.com", CONSERVATIVE),
        Err(CanonicalError::UnsupportedScheme)
    );
    assert_eq!(
        canonicalize("http://user:pw@example.com/", CONSERVATIVE),
        Err(CanonicalError::Credentials)
    );
    // The userinfo trick: the host is evil.test, not example.com.
    assert_eq!(
        canonicalize("http://example.com@evil.test/", CONSERVATIVE),
        Err(CanonicalError::Credentials)
    );
    assert_eq!(
        canonicalize("not a url", CONSERVATIVE),
        Err(CanonicalError::Unparseable)
    );
    assert_eq!(
        canonicalize("https://", CONSERVATIVE),
        Err(CanonicalError::Unparseable)
    );
}

#[test]
fn oversized_input_is_refused_before_parsing() {
    let long = format!("https://example.com/{}", "a".repeat(MAX_INPUT_BYTES));
    assert_eq!(
        canonicalize(&long, CONSERVATIVE),
        Err(CanonicalError::TooLong)
    );
}

#[test]
fn error_codes_are_content_free_and_distinct() {
    let codes = [
        CanonicalError::TooLong,
        CanonicalError::Unparseable,
        CanonicalError::UnsupportedScheme,
        CanonicalError::Credentials,
        CanonicalError::NoHost,
    ]
    .map(|e| e.code());
    for (i, a) in codes.iter().enumerate() {
        assert!(a.starts_with("url_"), "{a}");
        for b in &codes[i + 1..] {
            assert_ne!(a, b);
        }
    }
}

#[test]
fn canonicalisation_is_idempotent() {
    let inputs = [
        "HTTP://Example.COM:80/a/./b/../c?x=1#frag",
        "https://Example.com/%7Euser?b=%41&a=%2f",
        "https://example.com./a/",
        "https://example.com/p?utm_source=x&b=2&a=1",
    ];
    for opts in [CONSERVATIVE, AGGRESSIVE] {
        for input in inputs {
            let once = canonicalize(input, opts).unwrap();
            let twice = canonicalize(&once, opts).unwrap();
            assert_eq!(once, twice, "input {input:?}");
        }
    }
}

#[test]
fn request_id_has_the_documented_shape_and_is_deterministic() {
    let id = request_id_for_canonical("https://example.com/");
    assert!(id.starts_with(REQUEST_ID_PREFIX), "{id}");
    assert_eq!(id.len(), REQUEST_ID_PREFIX.len() + 32, "{id}");
    assert!(id[REQUEST_ID_PREFIX.len()..]
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    assert_eq!(id, request_id_for_canonical("https://example.com/"));
    assert!(!CANONICAL_VERSION.is_empty());
}

#[test]
fn equivalent_spellings_get_one_request_id() {
    let a = request_id_for_url("HTTP://Example.com:80/a", CONSERVATIVE).unwrap();
    let b = request_id_for_url("http://example.com/a#top", CONSERVATIVE).unwrap();
    let c = request_id_for_url("http://example.com/a", CONSERVATIVE).unwrap();
    assert_eq!(a, b);
    assert_eq!(b, c);
}

#[test]
fn different_resources_get_different_request_ids() {
    let a = request_id_for_url("https://example.com/a", CONSERVATIVE).unwrap();
    let b = request_id_for_url("https://example.com/a/", CONSERVATIVE).unwrap();
    let c = request_id_for_url("https://example.org/a", CONSERVATIVE).unwrap();
    assert_ne!(a, b);
    assert_ne!(a, c);
}

#[test]
fn request_id_is_derived_from_the_canonical_string_not_the_raw_input() {
    let raw = "HTTP://Example.com:80/a#x";
    let canonical = canonicalize(raw, CONSERVATIVE).unwrap();
    assert_eq!(
        request_id_for_url(raw, CONSERVATIVE).unwrap(),
        request_id_for_canonical(&canonical)
    );
}

#[test]
fn reconcile_without_redirect_yields_one_seen_id() {
    let r = reconcile(
        "https://Example.com/a#x",
        "https://example.com/a",
        CONSERVATIVE,
    )
    .expect("both sides canonicalise");
    assert!(!r.redirected);
    assert!(r.same_host);
    assert_eq!(r.requested, r.final_url);
    assert_eq!(r.requested_request_id, r.final_request_id);
    assert_eq!(r.seen_request_ids(), vec![r.requested_request_id.as_str()]);
}

#[test]
fn reconcile_trailing_dot_host_is_not_a_redirect() {
    let r = reconcile(
        "https://example.com./",
        "https://example.com/",
        CONSERVATIVE,
    )
    .unwrap();
    assert!(!r.redirected);
    assert!(r.same_host);
}

#[test]
fn reconcile_scheme_upgrade_on_same_host_is_a_redirect() {
    let r = reconcile(
        "http://example.com/a",
        "https://example.com/a",
        CONSERVATIVE,
    )
    .unwrap();
    assert!(r.redirected);
    assert!(r.same_host);
    assert_ne!(r.requested_request_id, r.final_request_id);
    assert_eq!(
        r.seen_request_ids(),
        vec![r.requested_request_id.as_str(), r.final_request_id.as_str()]
    );
}

#[test]
fn reconcile_cross_host_redirect_is_flagged() {
    let r = reconcile("https://a.example/x", "https://b.example/x", CONSERVATIVE).unwrap();
    assert!(r.redirected);
    assert!(!r.same_host);
}

#[test]
fn reconcile_www_redirect_is_a_different_host_by_default() {
    let r = reconcile(
        "https://example.com/",
        "https://www.example.com/",
        CONSERVATIVE,
    )
    .unwrap();
    assert!(r.redirected);
    assert!(!r.same_host);
}

#[test]
fn reconcile_trailing_slash_redirect_counts_by_default() {
    let r = reconcile(
        "https://example.com/a",
        "https://example.com/a/",
        CONSERVATIVE,
    )
    .unwrap();
    assert!(r.redirected);
}

#[test]
fn reconcile_tracking_only_redirect_depends_on_options() {
    let landed = "https://example.com/a?utm_source=newsletter";
    let conservative = reconcile("https://example.com/a", landed, CONSERVATIVE).unwrap();
    assert!(conservative.redirected);

    let aggressive = reconcile("https://example.com/a", landed, AGGRESSIVE).unwrap();
    assert!(!aggressive.redirected);
    assert_eq!(aggressive.seen_request_ids().len(), 1);
}

#[test]
fn reconcile_final_id_matches_a_plain_enqueue_of_the_final_url() {
    // A redirect into a page the frontier already holds must map onto that
    // page's key, so the frontier does not fetch it twice.
    let r = reconcile(
        "http://example.com/start",
        "https://example.com/home",
        CONSERVATIVE,
    )
    .unwrap();
    assert_eq!(
        r.final_request_id,
        request_id_for_url("https://example.com/home", CONSERVATIVE).unwrap()
    );
}

#[test]
fn reconcile_refuses_when_either_side_does_not_canonicalise() {
    assert_eq!(
        reconcile("https://example.com/", "mailto:x@example.com", CONSERVATIVE),
        Err(CanonicalError::UnsupportedScheme)
    );
    assert_eq!(
        reconcile("not a url", "https://example.com/", CONSERVATIVE),
        Err(CanonicalError::Unparseable)
    );
    assert_eq!(
        reconcile(
            "https://example.com/",
            "https://user@example.com/",
            CONSERVATIVE
        ),
        Err(CanonicalError::Credentials)
    );
}
