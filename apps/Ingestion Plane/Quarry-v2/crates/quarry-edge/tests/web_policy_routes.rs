//! WS7.E: web policy on the remaining routes (search, extract, map, crawl, batch and
//! profile create), decided without a server. Written in the build phase; run in the
//! wave's test phase (`cargo test -p quarry-edge --test web_policy_routes`).
//!
//! The decisions each route takes are asserted here. The route-level cases the plan also
//! names (an orchestrator double that records zero handoff calls, the 403 body through
//! the router, the `web_policy_downgraded_total` counter read through a recorder) need the
//! real-token harness of `zdr_claim_floor.rs` and are written in the test phase.

use quarry_core::error::ErrorCode;
use quarry_core::web_policy::{zdr_forces_disabled, CapKey};
use quarry_edge::auth::Claims;
use quarry_edge::policy_baseline::{PolicyBaseline, BASELINE_ENV};
use quarry_edge::web_policy_edge::{decide, gate_url, gate_urls, paid_provider};

fn claims(org_id: &str) -> Claims {
    Claims {
        sub: "u1".into(),
        iss: "auth-core".into(),
        exp: i64::MAX,
        org_id: org_id.into(),
        user_id: "u1".into(),
        principal_type: None,
        service_id: None,
        nbf: None,
        aud: None,
        scopes: Vec::new(),
        zdr: None,
        web_policy: None,
    }
}

/// An operator baseline from its wire document.
fn baseline(json: &str) -> PolicyBaseline {
    PolicyBaseline::from_lookup(|name| (name == BASELINE_ENV).then(|| json.to_string()))
        .expect("the test baseline is a valid layer")
}

#[test]
fn default_policy_changes_nothing_on_the_remaining_routes() {
    let policy = PolicyBaseline::default();
    let decision = decide(&policy, &claims("org_alpha"), false, None).expect("default decides");
    let paid = paid_provider(&decision, true, false);
    assert!(paid.allowed, "a paid-provider request is honoured under the default policy");
    assert_eq!(paid.downgraded_by, None);
    gate_url(&decision, "https://example.org/seed").expect("the default admits the seed");
    gate_urls(&decision, ["https://example.org/a", "https://example.net/b"])
        .expect("the default admits every URL");
    decision
        .policy()
        .require(CapKey::ProfilePersistent, "profile.create")
        .expect("the default allows a persistent profile");
}

#[test]
fn paid_provider_disabled_by_org_is_downgraded_and_disclosed() {
    // The operator layer stands in for the org layer here: both are layers of the same
    // lattice and both name their source in the downgrade.
    let policy = baseline(r#"{"modes":{"search.paid_provider":"disabled"}}"#);
    let decision = decide(&policy, &claims("org_alpha"), false, None).expect("decides");

    let asked = paid_provider(&decision, true, false);
    assert!(!asked.allowed, "the policy removes paid providers");
    assert_eq!(
        asked.downgraded_by,
        Some("baseline"),
        "the downgrade names the layer that removed the permission"
    );

    // A request that did not ask for paid providers has nothing to disclose.
    let not_asked = paid_provider(&decision, false, false);
    assert!(!not_asked.allowed);
    assert_eq!(not_asked.downgraded_by, None);
}

#[test]
fn paid_provider_still_off_under_zdr_whatever_the_policy_says() {
    // Under ZDR the request is never a policy downgrade: the floor was always there.
    let permissive = PolicyBaseline::default();
    let decision = decide(&permissive, &claims("org_alpha"), true, None).expect("decides");
    let paid = paid_provider(&decision, true, true);
    assert!(!paid.allowed);
    assert_eq!(paid.downgraded_by, None, "ZDR is not reported as a policy downgrade");
}

#[test]
fn extract_denied_url_is_403_without_echoing_urls() {
    let policy = baseline(r#"{"domains":{"deny":["example.net"]}}"#);
    let decision = decide(&policy, &claims("org_alpha"), false, None).expect("decides");
    let error = gate_urls(
        &decision,
        [
            "https://example.org/",
            "https://example.net/private/page?token=s3cret",
        ],
    )
    .expect_err("one URL is denied, so the whole request is refused");

    assert_eq!(error.code, ErrorCode::Forbidden);
    let details = error.details.clone().expect("the refusal carries details");
    assert_eq!(details["capability"], "domains.deny");
    assert_eq!(details["denied_count"], 1);

    // No URL, host, path or query in the message or the details.
    let shown = format!("{} {}", error.message, details);
    assert!(!shown.contains("example.net"), "host echoed: {shown}");
    assert!(!shown.contains("private"), "path echoed: {shown}");
    assert!(!shown.contains("s3cret"), "query echoed: {shown}");
}

#[test]
fn crawl_and_batch_deny_a_denied_seed_before_the_handoff() {
    // The routes gate the seed (crawl) and every URL (batch) before the handoff; the
    // orchestrator double of the test phase records zero calls. The decisions are here.
    let policy = baseline(r#"{"domains":{"deny":["example.net"]}}"#);
    let decision = decide(&policy, &claims("org_alpha"), false, None).expect("decides");

    let seed = gate_url(&decision, "https://example.net/seed").expect_err("the seed is denied");
    assert_eq!(seed.code, ErrorCode::Forbidden);

    let batch = gate_urls(&decision, ["https://example.org/a", "https://example.net/b"])
        .expect_err("one batch URL is denied");
    assert_eq!(batch.details.as_ref().expect("details")["denied_count"], 1);

    gate_urls(&decision, ["https://example.org/a"]).expect("an allowed batch passes");
}

#[test]
fn profile_create_denied_when_policy_disables_persistence() {
    let policy = baseline(r#"{"modes":{"browser.profile.persistent":"disabled"}}"#);
    let decision = decide(&policy, &claims("org_alpha"), false, None).expect("decides");
    let denial = decision
        .policy()
        .require(CapKey::ProfilePersistent, "profile.create")
        .expect_err("persistence is disabled by the baseline");
    assert_eq!(denial.code(), "policy_denied");
}

#[test]
fn zdr_implications_match_the_legacy_refusals() {
    // ZDR forces persistence and paid providers off. The legacy refusals (profile create,
    // paid search) stay in place; the policy's ZDR layer says the same thing about them.
    assert!(zdr_forces_disabled(CapKey::ProfilePersistent));
    assert!(zdr_forces_disabled(CapKey::SearchPaidProvider));

    let decision = decide(&PolicyBaseline::default(), &claims("org_alpha"), true, None)
        .expect("decides");
    assert!(!decision.policy().allows(CapKey::ProfilePersistent));
    assert!(!decision.policy().allows(CapKey::SearchPaidProvider));
}
