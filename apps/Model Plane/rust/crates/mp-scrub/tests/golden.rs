//! Conformance of `mp-scrub`'s Live set to Quarry's authored golden (WS12.S4).
//!
//! `tests/fixtures/secret-shapes.v1.json` is a byte-identical copy of
//! `apps/Ingestion Plane/Quarry-v2/crates/quarry-core/tests/fixtures/redaction/secret-shapes.v1.json`
//! (rule version `qr-redact/4`). Quarry owns it; the `zdr-contract-sync` script pins
//! both copies to one digest. This crate owes the cases whose `applies_to` names `mp`
//! (the high-confidence shapes, which is what the Live set is), and the negative
//! cases that say ordinary prose must survive.
//!
//! Quarry's markers are typed (`[redacted:token]`); this crate's is the Model Plane
//! marker `[REDACTED]`. The harness maps the one to the other in the expectations and
//! nowhere else.
//!
//! Every secret in the file is stored as fragments that the loader joins, so no
//! secret-shaped literal exists in the repository.

use std::collections::BTreeMap;

use mp_scrub::{scrub_live, scrub_live_json, scrub_url, MARKER};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

const GOLDEN_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/secret-shapes.v1.json"
);
const PIN_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/secret-shapes.v1.json.sha256"
);

/// The rule version of the golden this crate was written against. A re-vendored
/// golden with a new version fails here until someone has read what changed.
const VENDORED_RULE_VERSION: &str = "qr-redact/4";

/// Cases that name `mp` at vendoring time (12 positive + 18 negative = 30). A re-vendor may
/// add cases; it may not silently drop ours.
const MIN_MP_POSITIVE: usize = 12;
const MIN_MP_NEGATIVE: usize = 18;

#[derive(Deserialize)]
struct Golden {
    rule_version: String,
    cases: Vec<Case>,
    negative_cases: Vec<Negative>,
    json_cases: Vec<Value>,
    fields: Vec<Value>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    profile: String,
    applies_to: Vec<String>,
    template: String,
    secrets: BTreeMap<String, Vec<String>>,
    must_not_contain: Vec<String>,
    must_contain: Vec<String>,
}

#[derive(Deserialize)]
struct Negative {
    id: String,
    applies_to: Vec<String>,
    text: String,
    #[serde(default)]
    secrets: BTreeMap<String, Vec<String>>,
    must_contain: Vec<String>,
    identical: bool,
}

fn golden_text() -> String {
    std::fs::read_to_string(GOLDEN_PATH)
        .unwrap_or_else(|e| panic!("cannot read {GOLDEN_PATH}: {e}"))
}

fn golden() -> Golden {
    serde_json::from_str(&golden_text())
        .unwrap_or_else(|e| panic!("the golden does not load: {e}"))
}

fn joined(secrets: &BTreeMap<String, Vec<String>>) -> BTreeMap<String, String> {
    secrets
        .iter()
        .map(|(k, frags)| (format!("{{{k}}}"), frags.concat()))
        .collect()
}

fn fill(s: &str, secrets: &BTreeMap<String, String>) -> String {
    secrets
        .iter()
        .fold(s.to_owned(), |acc, (k, v)| acc.replace(k, v))
}

/// Quarry's typed marker (`[redacted:token]`, `[redacted:key]`, ...) as ours.
fn to_mp_marker(expected: &str) -> String {
    let mut out = String::with_capacity(expected.len());
    let mut rest = expected;
    while let Some(start) = rest.find("[redacted:") {
        out.push_str(&rest[..start]);
        match rest[start..].find(']') {
            Some(end) => {
                out.push_str(MARKER);
                rest = &rest[start + end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn is_mp(applies_to: &[String]) -> bool {
    applies_to.iter().any(|a| a == "mp")
}

// ---- the vendored file itself ----------------------------------------------

#[test]
fn golden_matches_its_pin_and_is_lf() {
    let bytes = std::fs::read(GOLDEN_PATH).expect("the vendored golden is missing");
    assert!(
        !bytes.contains(&b'\r'),
        "the golden is hashed byte for byte: LF only"
    );
    let actual = format!("{:x}", Sha256::digest(&bytes));
    let pin = std::fs::read_to_string(PIN_PATH).expect("the .sha256 pin is missing");
    let mut parts = pin.split_whitespace();
    let pinned = parts.next().expect("empty pin file");
    assert_eq!(
        parts.next(),
        Some("secret-shapes.v1.json"),
        "the pin names the file"
    );
    assert_eq!(
        actual, pinned,
        "the vendored golden differs from its pin: re-vendor BOTH files byte for byte from Quarry \
         (apps/Ingestion Plane/Quarry-v2/crates/quarry-core/tests/fixtures/redaction/) and update the \
         digest in scripts/tests/zdr-contract-sync.test.sh"
    );
}

#[test]
fn golden_rule_version_is_the_one_this_crate_was_written_against() {
    assert_eq!(golden().rule_version, VENDORED_RULE_VERSION);
}

#[test]
fn golden_keeps_secrets_as_fragments() {
    let file = golden_text();
    let g = golden();
    let mut checked = 0;
    for secrets in g
        .cases
        .iter()
        .map(|c| &c.secrets)
        .chain(g.negative_cases.iter().map(|c| &c.secrets))
    {
        for (name, value) in joined(secrets) {
            if value.len() >= 8 {
                assert!(
                    !file.contains(&value),
                    "secret {name} appears joined in the golden"
                );
                checked += 1;
            }
        }
    }
    assert!(checked >= 14, "only {checked} secrets checked");
    // The sections this crate does not exercise still have to be there, so a
    // truncated vendoring cannot pass.
    assert!(!g.json_cases.is_empty() && !g.fields.is_empty());
}

#[test]
fn golden_names_our_cases_and_only_live_ones() {
    let g = golden();
    let positive = g.cases.iter().filter(|c| is_mp(&c.applies_to)).count();
    let negative = g
        .negative_cases
        .iter()
        .filter(|c| is_mp(&c.applies_to))
        .count();
    assert!(positive >= MIN_MP_POSITIVE, "mp positive cases: {positive}");
    assert!(negative >= MIN_MP_NEGATIVE, "mp negative cases: {negative}");
    for c in g.cases.iter().filter(|c| is_mp(&c.applies_to)) {
        assert_eq!(c.profile, "live", "{}: an mp case must be a live shape", c.id);
    }
    for c in g.cases.iter().chain(std::iter::empty()) {
        for a in &c.applies_to {
            assert!(matches!(a.as_str(), "quarry" | "mp"), "{}: consumer {a}", c.id);
        }
    }
}

// ---- the cases we owe ------------------------------------------------------

fn check_positive(c: &Case) {
    let secrets = joined(&c.secrets);
    let input = fill(&c.template, &secrets);
    let out = scrub_live(&input);
    for bad in &c.must_not_contain {
        assert!(
            !out.contains(&fill(bad, &secrets)),
            "{}: secret survived in {out:?}",
            c.id
        );
    }
    for good in &c.must_contain {
        let want = to_mp_marker(&fill(good, &secrets));
        assert!(out.contains(&want), "{}: missing {want:?} in {out:?}", c.id);
    }
    // Idempotent: the output is its own fixed point.
    assert_eq!(scrub_live(&out), out, "{}: not idempotent", c.id);
    // The same text as a JSON string leaf.
    let mut v = Value::String(input.clone());
    scrub_live_json(&mut v);
    assert_eq!(v, Value::String(out.clone()), "{}: json leaf differs", c.id);
    // As a URL-shaped string (no whitespace), the shapes still hold.
    if !input.contains(char::is_whitespace) {
        let url_out = scrub_url(&input);
        for bad in &c.must_not_contain {
            assert!(
                !url_out.contains(&fill(bad, &secrets)),
                "{}: secret survived scrub_url in {url_out:?}",
                c.id
            );
        }
    }
}

#[test]
fn mp_positive_cases_mask() {
    let g = golden();
    let mut ran = 0;
    for c in g.cases.iter().filter(|c| is_mp(&c.applies_to)) {
        check_positive(c);
        ran += 1;
    }
    assert!(ran >= MIN_MP_POSITIVE, "ran {ran}");
}

/// Quarry-only Live cases name shapes this crate also has rules for (so a page cannot
/// route a secret around the Model Plane by choosing a vendor Quarry covers and we
/// do not). They are not part of the contract: if Quarry changes one, this is the
/// test to update.
#[test]
fn quarry_only_live_shape_cases_also_mask_here() {
    let g = golden();
    let shapes = [
        "jwt-lower-cased-by-a-normaliser",
        "pem-unterminated",
        "aws-asia",
        "github-fine-grained",
        "gitlab-pat",
        "npm-publish",
        "whsec",
    ];
    for id in shapes {
        let c = g
            .cases
            .iter()
            .find(|c| c.id == id)
            .unwrap_or_else(|| panic!("the golden has no case {id}"));
        assert!(!is_mp(&c.applies_to), "{id} became an mp case: move it to the list above");
        check_positive(c);
    }
}

#[test]
fn mp_negative_cases_survive() {
    let g = golden();
    let mut ran = 0;
    for c in g.negative_cases.iter().filter(|c| is_mp(&c.applies_to)) {
        let secrets = joined(&c.secrets);
        let input = fill(&c.text, &secrets);
        let out = scrub_live(&input);
        for good in &c.must_contain {
            assert!(
                out.contains(&fill(good, &secrets)),
                "{}: lost {good:?}: {out:?}",
                c.id
            );
        }
        if c.identical {
            assert_eq!(out, input, "{} must be byte-identical", c.id);
            let mut v = Value::String(input.clone());
            scrub_live_json(&mut v);
            assert_eq!(v, Value::String(input.clone()), "{}: json leaf", c.id);
            // A URL-shaped negative must also survive the URL scrub.
            if !input.contains(char::is_whitespace) {
                assert_eq!(scrub_url(&input), input, "{}: scrub_url changed it", c.id);
            }
        }
        ran += 1;
    }
    assert!(ran >= MIN_MP_NEGATIVE, "ran {ran}");
}

/// The marker mapping is the only liberty the harness takes; pin it.
#[test]
fn marker_mapping_is_exactly_the_typed_marker_to_ours() {
    assert_eq!(MARKER, "[REDACTED]");
    assert_eq!(
        to_mp_marker("a [redacted:token] b [redacted:key] c [redacted:credential]"),
        "a [REDACTED] b [REDACTED] c [REDACTED]"
    );
    assert_eq!(to_mp_marker("no marker [redacted: unterminated"), "no marker [redacted: unterminated");
}
