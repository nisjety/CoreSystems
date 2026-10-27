//! Conformance harness for the secret-redaction rules (WS12.S0).
//!
//! `tests/fixtures/redaction/secret-shapes.v1.json` is an AUTHORED golden: it is
//! the specification the rules are held to, here and (WS12.S4) in the Model
//! Plane `mp-scrub` crate. Every secret in it is an array of fragments that this
//! loader joins, so no secret-shaped literal exists in the repository.
//!
//! The tests here are named `redaction_*` so that
//! `cargo test -p quarry-core redaction` selects them and the unit tests.
//!
//! Nothing in this file sets the process-wide mode except where a test says so:
//! every call passes `Mode::Enforce` explicitly.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use proptest::prelude::*;
use quarry_core::redaction::field::{classify, mask_with, FieldFacts, FieldKind};
use quarry_core::redaction::{
    init_mode, mode, redact_json_with, redact_text_with, redact_url_with, rule_fingerprint, Mode,
    Profile, RedactionStamp, Tally, RULE_IDS, RULE_VERSION,
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

const GOLDEN_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/redaction/secret-shapes.v1.json"
);
const PIN_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/redaction/secret-shapes.v1.json.sha256"
);

/// The fingerprint of the rule set, per `RULE_VERSION`. Changing any rule (a
/// pattern, a key list, a field label, the card table) changes the fingerprint,
/// and this test then fails until `RULE_VERSION` is bumped and the new version
/// is pinned here: that is the version-bump policy of `docs/SECRET_REDACTION.md`.
const PINNED_FINGERPRINTS: &[(&str, &str)] = &[(
    "qr-redact/4",
    "dbb6e9c0699c562a21df9231f2ff8f754597a2cffdbfd61d91387bf903ef7c66",
)];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Golden {
    rule_version: String,
    #[allow(dead_code)]
    description: String,
    cases: Vec<Case>,
    negative_cases: Vec<Negative>,
    json_cases: Vec<JsonCase>,
    fields: Vec<FieldCase>,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
enum CaseProfile {
    Live,
    Durable,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    profile: CaseProfile,
    applies_to: Vec<String>,
    template: String,
    secrets: BTreeMap<String, Vec<String>>,
    must_not_contain: Vec<String>,
    must_contain: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Negative {
    id: String,
    applies_to: Vec<String>,
    text: String,
    #[serde(default)]
    secrets: BTreeMap<String, Vec<String>>,
    must_contain: Vec<String>,
    identical: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonCase {
    id: String,
    profile: CaseProfile,
    applies_to: Vec<String>,
    value: Value,
    secrets: BTreeMap<String, Vec<String>>,
    must_not_contain: Vec<String>,
    must_contain: Vec<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct Facts {
    input_type: Option<String>,
    autocomplete: Option<String>,
    name_attr: Option<String>,
    id_attr: Option<String>,
    aria_label: Option<String>,
    placeholder: Option<String>,
    accessible_name: Option<String>,
    value_secrets: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldCase {
    id: String,
    facts: Facts,
    expect: String,
    #[serde(default)]
    expect_rule: Option<String>,
}

fn golden_text() -> String {
    std::fs::read_to_string(GOLDEN_PATH)
        .unwrap_or_else(|e| panic!("cannot read {GOLDEN_PATH}: {e}"))
}

fn golden() -> Golden {
    serde_json::from_str(&golden_text())
        .unwrap_or_else(|e| panic!("the golden does not load strictly: {e}"))
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

fn text(mode: Mode, input: &str, profile: Profile) -> (String, Tally) {
    let mut t = Tally::new();
    let out = redact_text_with(mode, input, profile, &mut t).into_owned();
    (out, t)
}

fn enforce(input: &str, profile: Profile) -> (String, Tally) {
    text(Mode::Enforce, input, profile)
}

fn profile_of(p: CaseProfile) -> Profile {
    match p {
        CaseProfile::Live => Profile::Live,
        CaseProfile::Durable => Profile::Durable,
    }
}

fn check_consumers(id: &str, applies_to: &[String]) {
    assert!(!applies_to.is_empty(), "{id}: applies_to is empty");
    for a in applies_to {
        assert!(
            matches!(a.as_str(), "quarry" | "mp"),
            "{id}: unknown consumer {a}"
        );
    }
}

fn kind_of(s: &str) -> FieldKind {
    match s {
        "password" => FieldKind::Password,
        "otp" => FieldKind::Otp,
        "card" => FieldKind::Card,
        "phone" => FieldKind::Phone,
        "secret" => FieldKind::Secret,
        "unknown" => FieldKind::Unknown,
        other => panic!("unknown field kind {other}"),
    }
}

// ---- the golden itself ------------------------------------------------------

#[test]
fn redaction_golden_matches_its_pin_and_is_lf() {
    let bytes = std::fs::read(GOLDEN_PATH).expect("golden is missing");
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
        "the golden changed without its pin: refresh {PIN_PATH} and the digest in scripts/tests/zdr-contract-sync.test.sh"
    );
}

#[test]
fn redaction_golden_rule_version_is_the_library_version() {
    assert_eq!(golden().rule_version, RULE_VERSION);
}

#[test]
fn redaction_golden_keeps_secrets_as_fragments() {
    let file = golden_text();
    let g = golden();
    let mut checked = 0;
    let all: Vec<&BTreeMap<String, Vec<String>>> = g
        .cases
        .iter()
        .map(|c| &c.secrets)
        .chain(g.negative_cases.iter().map(|c| &c.secrets))
        .chain(g.json_cases.iter().map(|c| &c.secrets))
        .collect();
    for secrets in all {
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
    assert!(checked >= 25, "only {checked} secrets checked");
}

#[test]
fn redaction_golden_is_well_formed_and_covers_every_rule() {
    let g = golden();
    assert!(g.cases.len() >= 35, "cases: {}", g.cases.len());
    assert!(
        g.negative_cases.len() >= 24,
        "negative cases: {}",
        g.negative_cases.len()
    );
    assert!(g.fields.len() >= 28, "fields: {}", g.fields.len());
    let mut ids = BTreeSet::new();
    for id in g
        .cases
        .iter()
        .map(|c| &c.id)
        .chain(g.negative_cases.iter().map(|c| &c.id))
        .chain(g.json_cases.iter().map(|c| &c.id))
        .chain(g.fields.iter().map(|c| &c.id))
    {
        assert!(ids.insert(id.clone()), "duplicate case id {id}");
    }
    for c in &g.cases {
        check_consumers(&c.id, &c.applies_to);
        // A consumer outside Quarry (the Model Plane scrubber) only owes the
        // high-confidence shapes, which is what Live means.
        if c.applies_to.iter().any(|a| a == "mp") {
            assert_eq!(
                c.profile,
                CaseProfile::Live,
                "{}: mp cases must be live shapes",
                c.id
            );
        }
    }
    for c in &g.negative_cases {
        check_consumers(&c.id, &c.applies_to);
    }
    for c in &g.json_cases {
        check_consumers(&c.id, &c.applies_to);
    }

    // Every rule id is exercised by at least one positive case.
    let mut seen: BTreeSet<&'static str> = BTreeSet::new();
    for c in &g.cases {
        let input = fill(&c.template, &joined(&c.secrets));
        let (_, t) = enforce(&input, Profile::Durable);
        seen.extend(t.by_rule().keys().copied());
    }
    for c in &g.json_cases {
        let secrets = joined(&c.secrets);
        let mut v: Value = serde_json::from_str(&fill(&c.value.to_string(), &secrets)).unwrap();
        let mut t = Tally::new();
        redact_json_with(Mode::Enforce, &mut v, Profile::Durable, &mut t);
        seen.extend(t.by_rule().keys().copied());
    }
    for f in &g.fields {
        let facts = to_facts(&f.facts);
        let c = classify(&facts);
        if let Some(rule) = c.rule {
            seen.insert(rule);
        }
    }
    let missing: Vec<&&str> = RULE_IDS.iter().filter(|r| !seen.contains(**r)).collect();
    assert!(
        missing.is_empty(),
        "rules with no positive case in the golden: {missing:?}"
    );
}

// ---- text rules -------------------------------------------------------------

#[test]
fn redaction_live_cases_mask_on_both_profiles() {
    let g = golden();
    let mut ran = 0;
    for c in g.cases.iter().filter(|c| c.profile == CaseProfile::Live) {
        let secrets = joined(&c.secrets);
        let input = fill(&c.template, &secrets);
        for profile in [Profile::Live, Profile::Durable] {
            let (out, t) = enforce(&input, profile);
            for bad in &c.must_not_contain {
                assert!(
                    !out.contains(&fill(bad, &secrets)),
                    "{} [{profile:?}]: secret survived in {out:?}",
                    c.id
                );
            }
            for good in &c.must_contain {
                assert!(
                    out.contains(&fill(good, &secrets)),
                    "{} [{profile:?}]: missing {good:?} in {out:?}",
                    c.id
                );
            }
            assert!(!t.is_empty(), "{} [{profile:?}]: nothing counted", c.id);
        }
        ran += 1;
    }
    assert!(ran >= 15, "ran {ran}");
}

#[test]
fn redaction_durable_cases_mask_on_durable_and_leave_live_alone() {
    let g = golden();
    let mut ran = 0;
    for c in g.cases.iter().filter(|c| c.profile == CaseProfile::Durable) {
        let secrets = joined(&c.secrets);
        let input = fill(&c.template, &secrets);
        let (out, t) = enforce(&input, Profile::Durable);
        for bad in &c.must_not_contain {
            assert!(
                !out.contains(&fill(bad, &secrets)),
                "{}: secret survived in {out:?}",
                c.id
            );
        }
        for good in &c.must_contain {
            assert!(
                out.contains(&fill(good, &secrets)),
                "{}: missing {good:?} in {out:?}",
                c.id
            );
        }
        assert!(!t.is_empty(), "{}: nothing counted", c.id);
        let (live, lt) = enforce(&input, Profile::Live);
        assert_eq!(
            live, input,
            "{}: Live must leave a contextual secret alone",
            c.id
        );
        assert!(lt.is_empty(), "{}", c.id);
        ran += 1;
    }
    assert!(ran >= 15, "ran {ran}");
}

#[test]
fn redaction_negative_cases_survive_on_both_profiles() {
    let g = golden();
    for c in &g.negative_cases {
        let secrets = joined(&c.secrets);
        let input = fill(&c.text, &secrets);
        for profile in [Profile::Live, Profile::Durable] {
            let (out, t) = enforce(&input, profile);
            for good in &c.must_contain {
                assert!(
                    out.contains(&fill(good, &secrets)),
                    "{} [{profile:?}]: lost {good:?}: {out:?}",
                    c.id
                );
            }
            if c.identical {
                assert_eq!(out, input, "{} [{profile:?}] must be byte-identical", c.id);
                assert!(
                    t.is_empty(),
                    "{} [{profile:?}]: counted {:?}",
                    c.id,
                    t.by_rule()
                );
            }
        }
    }
}

#[test]
fn redaction_json_cases() {
    let g = golden();
    for c in &g.json_cases {
        let secrets = joined(&c.secrets);
        let filled = fill(&serde_json::to_string(&c.value).unwrap(), &secrets);
        let input: Value = serde_json::from_str(&filled).unwrap();
        let mut v = input.clone();
        let mut t = Tally::new();
        redact_json_with(Mode::Enforce, &mut v, profile_of(c.profile), &mut t);
        let out = serde_json::to_string(&v).unwrap();
        for bad in &c.must_not_contain {
            assert!(
                !out.contains(&fill(bad, &secrets)),
                "{}: secret survived in {out}",
                c.id
            );
        }
        for good in &c.must_contain {
            assert!(
                out.contains(&fill(good, &secrets)),
                "{}: missing {good:?} in {out}",
                c.id
            );
        }
        assert!(!t.is_empty(), "{}", c.id);
        if c.profile == CaseProfile::Live {
            // Durable is a superset of Live: a Live case holds there too.
            let mut dur = input.clone();
            let mut dt = Tally::new();
            redact_json_with(Mode::Enforce, &mut dur, Profile::Durable, &mut dt);
            let dur_out = serde_json::to_string(&dur).unwrap();
            for bad in &c.must_not_contain {
                assert!(
                    !dur_out.contains(&fill(bad, &secrets)),
                    "{}: secret survived under Durable in {dur_out}",
                    c.id
                );
            }
        }
        if c.profile == CaseProfile::Durable {
            let mut live = input.clone();
            let mut lt = Tally::new();
            redact_json_with(Mode::Enforce, &mut live, Profile::Live, &mut lt);
            assert_eq!(live, input, "{}: Live leaves a keyed secret alone", c.id);
        }
        // Idempotent.
        let mut again = v.clone();
        let mut t2 = Tally::new();
        redact_json_with(Mode::Enforce, &mut again, profile_of(c.profile), &mut t2);
        assert_eq!(again, v, "{}", c.id);
        assert!(t2.is_empty(), "{}", c.id);
    }
}

// ---- fields -----------------------------------------------------------------

fn to_facts(f: &Facts) -> FieldFacts {
    FieldFacts {
        input_type: f.input_type.clone(),
        autocomplete: f.autocomplete.clone(),
        name_attr: f.name_attr.clone(),
        id_attr: f.id_attr.clone(),
        aria_label: f.aria_label.clone(),
        placeholder: f.placeholder.clone(),
        accessible_name: f.accessible_name.clone(),
        value: f.value_secrets.as_ref().map(|frags| frags.concat()),
    }
}

#[test]
fn redaction_fields_classify_and_mask() {
    let g = golden();
    for f in &g.fields {
        let facts = to_facts(&f.facts);
        let c = classify(&facts);
        assert_eq!(c.kind, kind_of(&f.expect), "{}: {c:?}", f.id);
        if let Some(rule) = &f.expect_rule {
            assert_eq!(c.rule, Some(rule.as_str()), "{}", f.id);
        }
        let value = facts
            .value
            .clone()
            .unwrap_or_else(|| "sample-value".to_owned());
        let mut t = Tally::new();
        let masked = mask_with(Mode::Enforce, c, &value, &mut t).into_owned();
        if c.kind.is_sensitive() {
            assert!(
                masked.starts_with("[redacted:") && masked.ends_with(']'),
                "{}: {masked}",
                f.id
            );
            assert!(!masked.contains(&value), "{}", f.id);
            assert_eq!(t.values_masked(), 1, "{}", f.id);
        } else {
            assert_eq!(masked, value, "{}", f.id);
            assert!(t.is_empty(), "{}", f.id);
        }
    }
}

// ---- modes, stamp, version --------------------------------------------------

#[test]
fn redaction_default_mode_is_enforce_and_the_first_read_pins_it() {
    // This binary never calls init_mode anywhere else, so this is the only
    // writer; the library unit tests (a separate process) cover init-first.
    assert_eq!(mode(), Mode::Enforce);
    assert_eq!(init_mode(Mode::Shadow), Err(Mode::Enforce));
    assert_eq!(mode(), Mode::Enforce);
}

#[test]
fn redaction_shadow_counts_without_changing_and_off_does_nothing() {
    let input = "Authorization: Bearer abcdef and password=hunter2";
    let (enforced, et) = text(Mode::Enforce, input, Profile::Durable);
    assert_ne!(enforced, input);
    let (shadow, st) = text(Mode::Shadow, input, Profile::Durable);
    assert_eq!(shadow, input, "shadow returns the input unchanged");
    assert_eq!(st, et, "shadow counts what enforce would have masked");
    let (off, ot) = text(Mode::Off, input, Profile::Durable);
    assert_eq!(off, input);
    assert!(ot.is_empty());

    let mut v = serde_json::json!({"password": "x", "note": "password=hunter2"});
    let before = v.clone();
    let mut t = Tally::new();
    redact_json_with(Mode::Shadow, &mut v, Profile::Durable, &mut t);
    assert_eq!(v, before);
    assert!(!t.is_empty());
    let mut t = Tally::new();
    assert_eq!(
        redact_url_with(
            Mode::Shadow,
            "https://h.example/p?token=abc123",
            Profile::Durable,
            &mut t
        ),
        "https://h.example/p?token=abc123"
    );
    assert!(!t.is_empty());
}

#[test]
fn redaction_stamp_carries_counts_and_ids_only() {
    let secret = "hunter2-very-secret";
    let input = format!("password={secret} and Authorization: Bearer tok_8f3a1d9");
    let (_, t) = enforce(&input, Profile::Durable);
    let stamp = t.stamp(Profile::Durable, Mode::Enforce);
    let json = serde_json::to_string(&stamp).unwrap();
    assert!(!json.contains(secret));
    assert!(!json.contains("tok_8f3a1d9"));
    assert!(!json.contains("password=") && !json.contains("Bearer"));
    assert_eq!(stamp.rule_version, RULE_VERSION);
    assert_eq!(stamp.values_masked, 2);
    for rule in stamp.by_rule.keys() {
        assert!(RULE_IDS.contains(&rule.as_str()), "{rule}");
    }
    let back: RedactionStamp = serde_json::from_str(&json).unwrap();
    assert_eq!(back, stamp);
}

#[test]
fn redaction_rule_drift_requires_a_version_bump() {
    let actual = rule_fingerprint();
    let pinned = PINNED_FINGERPRINTS
        .iter()
        .find(|(v, _)| *v == RULE_VERSION)
        .unwrap_or_else(|| {
            panic!("RULE_VERSION {RULE_VERSION} has no pinned fingerprint: add (\"{RULE_VERSION}\", \"{actual}\") to PINNED_FINGERPRINTS")
        })
        .1;
    assert_eq!(
        actual, pinned,
        "a redaction rule changed without a version bump. Bump RULE_VERSION, add the new pair \
         (\"<new version>\", \"{actual}\") to PINNED_FINGERPRINTS, regenerate the golden pin and every vendored copy \
         (docs/SECRET_REDACTION.md, 'Changing a rule'). If the change is deliberate and the version is already bumped, \
         the pin for the new version is missing."
    );
}

// ---- properties -------------------------------------------------------------

fn corpus() -> Vec<String> {
    let g = golden();
    let mut c: Vec<String> = Vec::new();
    for case in &g.cases {
        c.push(fill(&case.template, &joined(&case.secrets)));
    }
    for case in &g.negative_cases {
        c.push(fill(&case.text, &joined(&case.secrets)));
    }
    for s in [
        "password=",
        "Bearer ",
        "Authorization: Basic ",
        "otp ",
        "OTP: ",
        "4111 1111 1111 1111",
        "4111-1111-1111-1111",
        "?token=",
        "&code=",
        "#access_token=",
        "-----BEGIN PRIVATE KEY-----",
        "-----END PRIVATE KEY-----",
        "eyJ",
        "AKIA",
        "sk-",
        "ghp_",
        "xoxb-",
        "://user:",
        "@host",
        "[redacted:token]",
        "[redacted:",
        "]",
        "\"password\": \"",
        "'",
        "\"",
        "x-api-key: ",
        "is your verification code",
        "Engangskode ",
        "123456",
        "482913",
        "5555 5555 5555 4444",
        "\u{2013}",
        "ø",
        "\u{0}",
    ] {
        c.push(s.to_owned());
    }
    c
}

fn piece() -> impl Strategy<Value = String> {
    let corpus = corpus();
    prop_oneof![
        3 => "[ -~]{0,24}",
        1 => "\\PC{0,12}",
        4 => proptest::sample::select(corpus),
    ]
}

fn sep() -> impl Strategy<Value = &'static str> {
    proptest::sample::select(vec![
        " ", "\n", "", "=", ": ", "&", "?", "\"", "'", "-", "/", ".", ",",
    ])
}

fn assemble(parts: &[(String, &str)]) -> String {
    parts.iter().map(|(p, s)| format!("{p}{s}")).collect()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 2000, failure_persistence: None, ..ProptestConfig::default() })]

    /// redact(redact(x)) == redact(x), and the second pass masks nothing.
    #[test]
    fn redaction_is_idempotent(parts in prop::collection::vec((piece(), sep()), 0..14)) {
        let input = assemble(&parts);
        for profile in [Profile::Live, Profile::Durable] {
            let (once, _) = enforce(&input, profile);
            let (twice, t) = enforce(&once, profile);
            prop_assert_eq!(&once, &twice, "profile {:?}, input {:?}", profile, input);
            prop_assert!(t.is_empty(), "second pass counted {:?} on {:?}", t.by_rule(), once);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 500, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn redaction_of_urls_is_idempotent(parts in prop::collection::vec((piece(), sep()), 0..8)) {
        let input = format!("https://u:p@h.example/path?{}#{}", assemble(&parts), assemble(&parts));
        for profile in [Profile::Live, Profile::Durable] {
            let mut t = Tally::new();
            let once = redact_url_with(Mode::Enforce, &input, profile, &mut t).into_owned();
            let mut t2 = Tally::new();
            let twice = redact_url_with(Mode::Enforce, &once, profile, &mut t2).into_owned();
            prop_assert_eq!(&once, &twice, "profile {:?}, input {:?}", profile, input);
        }
    }
}

// ---- performance ------------------------------------------------------------

/// 1 MiB of mixed text: mostly prose, ids and URLs, with a secret of every
/// shape in the golden every few lines.
fn mixed_megabyte() -> String {
    let g = golden();
    let secrets: Vec<String> = g
        .cases
        .iter()
        .map(|c| fill(&c.template, &joined(&c.secrets)))
        .collect();
    let prose = [
        "The quick brown fox jumps over the lazy dog while status=completed path=/usr/bin count=42.",
        "Order 4111111111111112 shipped to 0150 Oslo, build 1.2.3 finished at 2026-10-21 12:30:45.",
        "See https://example.com/search?q=rust&page=2#top and request 123e4567-e89b-12d3-a456-426614174000.",
        "commit 5f4dcc3b5aa765d61d8327deb882cf99ab12cd34 digest 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
        "Forgot your password? Pin the tab and the bearer of the message will verification code sent by email.",
    ];
    let mut out = String::with_capacity(1_100_000);
    let mut i = 0usize;
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    while out.len() < 1 << 20 {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        out.push_str(prose[(seed >> 33) as usize % prose.len()]);
        out.push('\n');
        if i % 4 == 0 {
            out.push_str(&secrets[(seed >> 40) as usize % secrets.len()]);
            out.push('\n');
        }
        i += 1;
    }
    out
}

/// Run with: cargo test -p quarry-core --release redaction_perf_budget -- --ignored --nocapture
#[test]
#[ignore = "release-only timing; run with --release -- --ignored"]
fn redaction_perf_budget() {
    if cfg!(debug_assertions) {
        eprintln!("redaction_perf_budget: skipped in a debug build (run with --release)");
        return;
    }
    let input = mixed_megabyte();
    assert!(input.len() >= 1 << 20);
    for profile in [Profile::Live, Profile::Durable] {
        // The first call compiles the rules (about a millisecond); the budget is for steady state.
        let _ = enforce("warm up password=x", profile);
        let mut best = u128::MAX;
        for _ in 0..3 {
            let start = Instant::now();
            let (out, t) = enforce(&input, profile);
            best = best.min(start.elapsed().as_millis());
            assert!(!t.is_empty() && out.len() != input.len());
        }
        eprintln!(
            "redaction_perf_budget: {profile:?} 1 MiB mixed text in {best} ms (budget 150 ms)"
        );
        assert!(
            best < 150,
            "{profile:?} took {best} ms for 1 MiB (budget 150 ms)"
        );
    }
}
