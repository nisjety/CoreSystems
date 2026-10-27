//! WS7.C: the operator baseline, the enforcement modes and the per-request decision
//! of the scrape family, without a server. Written in the build phase; run in the
//! wave's test phase (`cargo test -p quarry-edge --test policy_baseline`).
//!
//! `observe_never_denies_but_counts` asserts the shadow decision that the edge
//! counts as `web_policy_would_deny_total`; the counter itself is a metric and is
//! not read here.

use quarry_core::web_policy::{CapKey, DecisionSource, Mode, Principal, RequestAsk};
use quarry_edge::policy_baseline::{Enforcement, PolicyBaseline, BASELINE_ENV, ENFORCEMENT_ENV};
use quarry_edge::web_policy_edge::{decide_for_principal, gate_url, is_default};
use quarry_runtime::{policy_fingerprint, RunPolicy};

/// A lookup over a fixed set of variables.
fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let owned: Vec<(String, String)> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |name: &str| {
        owned
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    }
}

#[test]
fn default_baseline_equals_todays_behaviour() {
    let baseline = PolicyBaseline::default();
    assert_eq!(baseline.enforcement(), Enforcement::Enforce);

    let decision = decide_for_principal(&baseline, Principal::Human, false, None).expect("default decides");
    assert!(decision.shadow().is_none());
    gate_url(&decision, "https://example.org/").expect("the default admits any URL");
    assert!(is_default(&decision));

    // The run's fingerprint is the one a run without web policy had.
    assert_eq!(
        policy_fingerprint(&RunPolicy::from(decision.policy())),
        policy_fingerprint(&RunPolicy::best_effort())
    );
}

#[test]
fn bad_json_fails_from_env() {
    // A baseline that does not parse must fail the boot, never fall back to no layer.
    let bad_json = env(&[(BASELINE_ENV, "{not json")]);
    assert!(PolicyBaseline::from_lookup(bad_json).is_err());

    // An unknown capability key in the baseline is refused too.
    let unknown_key = env(&[(BASELINE_ENV, r#"{"modes":{"browser.nope":"disabled"}}"#)]);
    assert!(PolicyBaseline::from_lookup(unknown_key).is_err());

    // A mode outside off, observe, enforce is refused.
    let bad_mode = env(&[(ENFORCEMENT_ENV, "sometimes")]);
    assert!(PolicyBaseline::from_lookup(bad_mode).is_err());

    // A valid baseline parses.
    let valid = env(&[(BASELINE_ENV, r#"{"domains":{"deny":["example.net"]}}"#)]);
    let baseline = PolicyBaseline::from_lookup(valid).expect("a valid baseline parses");
    assert_eq!(baseline.enforcement(), Enforcement::Enforce);
}

#[test]
fn observe_never_denies_but_counts() {
    let baseline = PolicyBaseline::from_lookup(env(&[
        (ENFORCEMENT_ENV, "observe"),
        (BASELINE_ENV, r#"{"domains":{"deny":["example.net"]}}"#),
    ]))
    .expect("observe parses");
    assert_eq!(baseline.enforcement(), Enforcement::Observe);

    let decision = decide_for_principal(&baseline, Principal::Human, false, None).expect("observe decides");
    // The run is admitted: observe never denies.
    gate_url(&decision, "https://example.net/").expect("observe admits the denied host");
    // The decision enforce would take is kept beside the run, and it denies.
    let shadow = decision
        .shadow()
        .expect("observe keeps the enforce decision beside the run");
    assert!(shadow.check_url("https://example.net/").is_err());
}

#[test]
fn off_still_enforces_invariants() {
    let baseline = PolicyBaseline::from_lookup(env(&[
        (ENFORCEMENT_ENV, "off"),
        (BASELINE_ENV, r#"{"domains":{"deny":["example.net"]}}"#),
    ]))
    .expect("off parses");

    // Off ignores the baseline and the ask, so neither host is refused.
    let ask = RequestAsk {
        deny_domains: vec!["example.org".into()],
        ..Default::default()
    };
    let decision = decide_for_principal(&baseline, Principal::Human, false, Some(&ask)).expect("off decides");
    gate_url(&decision, "https://example.org/").expect("off ignores the ask");
    gate_url(&decision, "https://example.net/").expect("off ignores the baseline");

    // The invariant layer still applies: raw script is never allowed.
    let (mode, source) = decision
        .policy()
        .mode(CapKey::ActScript)
        .expect("the script key applies to a human");
    assert_eq!(mode, Mode::Disabled);
    assert_eq!(source, DecisionSource::Invariant);

    // The ZDR floor still applies: profile persistence is off under ZDR.
    let zdr = decide_for_principal(&baseline, Principal::Human, true, None).expect("off decides under zdr");
    assert!(!zdr.policy().allows(CapKey::ProfilePersistent));
}

#[test]
fn enforce_denies_an_asked_domain_with_the_request_as_source() {
    let baseline = PolicyBaseline::default();
    let ask = RequestAsk {
        deny_domains: vec!["example.org".into()],
        ..Default::default()
    };
    let decision = decide_for_principal(&baseline, Principal::Human, false, Some(&ask)).expect("enforce decides");

    let error = gate_url(&decision, "https://example.org/page")
        .expect_err("enforce refuses the asked host");
    assert_eq!(error.code.http_status(), 403);
    let details = error.details.expect("the denial carries details");
    // A host refusal is the typed `domain_denied` (WS7.B closed reason set, build plan WS7.B),
    // not the capability reason `policy_denied`; the edge adds the `domains.deny` capability.
    assert_eq!(details["reason"], "domain_denied");
    assert_eq!(details["capability"], "domains.deny");
    assert_eq!(details["decision_source"], "request");

    // The same ask under observe is admitted.
    let observe = PolicyBaseline::from_lookup(env(&[(ENFORCEMENT_ENV, "observe")])).unwrap();
    let decision = decide_for_principal(&observe, Principal::Human, false, Some(&ask_for("example.org")))
        .expect("observe decides");
    gate_url(&decision, "https://example.org/page").expect("observe admits");
}

#[test]
fn an_unknown_capability_in_the_ask_is_a_bad_request_in_every_mode() {
    for mode in ["off", "observe", "enforce"] {
        let baseline =
            PolicyBaseline::from_lookup(env(&[(ENFORCEMENT_ENV, mode)])).expect("mode parses");
        let ask = RequestAsk {
            disable: vec!["browser.not_a_key".into()],
            ..Default::default()
        };
        let error = decide_for_principal(&baseline, Principal::Human, false, Some(&ask))
            .expect_err("a malformed ask is refused");
        assert_eq!(error.code.http_status(), 400, "mode={mode}");
        let details = error.details.expect("the refusal carries details");
        assert_eq!(details["reason"], "unknown_capability", "mode={mode}");
    }
}

/// An ask that denies one host, as the shape the edge converts.
fn ask_for(host: &str) -> RequestAsk {
    RequestAsk {
        deny_domains: vec![host.into()],
        ..Default::default()
    }
}
