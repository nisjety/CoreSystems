//! WS7.J: the org web-policy claim, read at intake and applied as the org layer.
//!
//! These tests drive the pure path the scrape and agent routes take: the token's claim, the
//! baseline's enforcement mode, the claim mode and the request's ask, through
//! `PolicyBaseline::org_input` and `PolicyBaseline::decide_with_org`. The HTTP-level tests (real
//! tokens, the scrape and agent routes) belong to the test phase of the wave; see the W2-U35
//! progress note.
//!
//! The plan's ten tests run with the claim mode `enforce` (the state after the one env flip);
//! the W2-CP-U35 tests below them pin the `observe` default, the `off` mode, D2-053 (an empty
//! `domains.allow` is no restriction) and D2-054 (`policy_rev` is the 12 hex digest on the wire).
//!
//! Run: `cargo test -p quarry-edge --test web_policy_claim --test zdr_claim_floor`.

use quarry_core::error::ErrorCode;
use quarry_core::web_policy::{CapKey, Mode, PolicyDenial, Principal, RequestAsk};
use quarry_edge::auth::Claims;
use quarry_edge::policy_baseline::{
    Enforcement, PolicyBaseline, CLAIM_ENFORCEMENT_ENV, ENFORCEMENT_ENV, REQUIRE_CLAIM_ENV,
};
use quarry_edge::web_policy_edge::{gate_url, policy_rev_hex, quarry_error};
use serde_json::json;

/// A token's claims. `principal` is `Some("service")` for a service token (`execution-core`
/// here) and `Some("user")` or `None` for a user token.
fn token(principal: Option<&str>, web_policy: Option<serde_json::Value>) -> Claims {
    let service = principal == Some("service");
    Claims {
        sub: if service {
            "service:execution-core".to_owned()
        } else {
            "user-1".to_owned()
        },
        iss: "auth-core".to_owned(),
        exp: 4_102_444_800,
        org_id: "org-a".to_owned(),
        user_id: if service {
            String::new()
        } else {
            "user-1".to_owned()
        },
        principal_type: principal.map(str::to_owned),
        service_id: service.then(|| "service:execution-core".to_owned()),
        nbf: None,
        aud: Some("quarry".to_owned()),
        scopes: Vec::new(),
        zdr: None,
        web_policy,
        key_id: None,
        acting_user_id: None,
    }
}

/// A version 1 claim with the given capability modes and no domain rules.
fn claim(caps: serde_json::Value) -> serde_json::Value {
    json!({
        "v": 1,
        "rev": "a1b2c3d4e5f6",
        "caps": caps,
        "domains": { "allow": [], "deny": [] }
    })
}

/// The baseline in one enforcement mode, with the claim requirement set by `require` and the
/// claim mode `enforce` (the plan's ten tests describe the claim once it binds).
fn baseline(enforcement: &str, require: bool) -> PolicyBaseline {
    baseline_with_claim(enforcement, Some("enforce"), require)
}

/// The baseline in one enforcement mode and one claim mode (`None`: the variable is unset, so
/// the default applies).
fn baseline_with_claim(enforcement: &str, claim_mode: Option<&str>, require: bool) -> PolicyBaseline {
    let enforcement = enforcement.to_owned();
    let claim_mode = claim_mode.map(str::to_owned);
    PolicyBaseline::from_lookup(move |name| match name {
        ENFORCEMENT_ENV => Some(enforcement.clone()),
        CLAIM_ENFORCEMENT_ENV => claim_mode.clone(),
        REQUIRE_CLAIM_ENV if require => Some("1".to_owned()),
        _ => None,
    })
    .expect("a valid baseline configuration")
}

/// A version 1 claim with only domain rules.
fn domain_claim(rev: &str, domains: serde_json::Value) -> serde_json::Value {
    json!({ "v": 1, "rev": rev, "caps": {}, "domains": domains })
}

/// Decide one request for the token, the way the edge does: the claim as the org layer.
fn decide(
    baseline: &PolicyBaseline,
    token: &Claims,
    principal: Principal,
    ask: Option<&RequestAsk>,
) -> Result<quarry_edge::policy_baseline::Decision, PolicyDenial> {
    let org = baseline.org_input(token.web_policy_claim());
    let request = ask.map(|ask| ask.into_layer().expect("a well-formed ask"));
    baseline.decide_with_org(principal, false, org, request)
}

#[test]
fn user_claim_disables_upload_even_when_the_body_asks() {
    let baseline = baseline("enforce", false);
    let user = token(None, Some(claim(json!({ "browser.act.upload": "disabled" }))));
    // The body asks for a host allow list and a disable of something else: it cannot restore upload.
    let ask = RequestAsk {
        allow_domains: Some(vec!["example.com".to_owned()]),
        disable: vec!["browser.act.download".to_owned()],
        ..RequestAsk::default()
    };
    let decision = decide(&baseline, &user, Principal::Human, Some(&ask)).expect("the run is decided");
    assert!(!decision.policy().allows(CapKey::ActUpload));
    let denial = decision
        .policy()
        .require(CapKey::ActUpload, "upload_ref")
        .expect_err("upload is denied by the org layer");
    assert_eq!(denial.details()["decision_source"], json!("org_policy"));
}

#[test]
fn service_claim_applies_to_execution_core_tokens() {
    let baseline = baseline("enforce", false);
    let service = token(
        Some("service"),
        Some(claim(json!({ "browser.act.upload": "disabled" }))),
    );
    let decision = decide(&baseline, &service, Principal::Autonomous, None).expect("the run is decided");
    assert!(!decision.policy().allows(CapKey::ActUpload));
}

#[test]
fn service_zdr_claim_is_still_ignored() {
    // A service token's `zdr` is a registry property, never a floor (Claims::zdr_floor). The web
    // policy claim is separate: it does not change the ZDR floor either.
    let mut service = token(
        Some("service"),
        Some(claim(json!({ "browser.act.upload": "disabled" }))),
    );
    service.zdr = Some(true);
    assert!(!service.zdr_floor());
}

#[test]
fn absent_claim_is_baseline_unless_required() {
    let token_without = token(None, None);
    let lenient = baseline("enforce", false);
    let decision = decide(&lenient, &token_without, Principal::Human, None).expect("no claim, no org layer");
    assert!(decision.policy().allows(CapKey::ActUpload));

    let strict = baseline("enforce", true);
    let denial = decide(&strict, &token_without, Principal::Human, None).expect_err("required claim missing");
    assert_eq!(denial.code(), "policy_unreadable");
}

#[test]
fn required_claim_missing_is_403() {
    let strict = baseline("enforce", true);
    let denial = decide(&strict, &token(None, None), Principal::Human, None).expect_err("refused");
    assert!(matches!(denial, PolicyDenial::PolicyUnreadable));
    assert!(matches!(denial.error_code(), ErrorCode::Forbidden));
}

#[test]
fn unparseable_claim_fails_closed() {
    let bad_mode = token(None, Some(claim(json!({ "browser.act.upload": "sometimes" }))));
    assert!(bad_mode.web_policy_claim().is_err());
    let forced_ability = token(None, Some(claim(json!({ "browser.act.upload": "forced" }))));
    assert!(forced_ability.web_policy_claim().is_err());
    let not_an_object = token(None, Some(json!("disabled")));
    assert!(not_an_object.web_policy_claim().is_err());
    let bad_rev = token(
        None,
        Some(json!({ "v": 1, "rev": "not-hex", "caps": {} })),
    );
    assert!(bad_rev.web_policy_claim().is_err());

    let enforce = baseline("enforce", false);
    let denial = decide(&enforce, &bad_mode, Principal::Human, None).expect_err("refused under enforce");
    assert!(matches!(denial, PolicyDenial::PolicyUnreadable));
}

#[test]
fn unknown_version_fails_closed() {
    let v2 = token(
        None,
        Some(json!({ "v": 2, "rev": "a1b2c3", "caps": {} })),
    );
    assert!(v2.web_policy_claim().is_err());
    let no_version = token(None, Some(json!({ "rev": "a1b2c3", "caps": {} })));
    assert!(no_version.web_policy_claim().is_err());
}

#[test]
fn unknown_key_is_ignored_and_counted() {
    let newer = token(
        None,
        Some(claim(json!({
            "browser.future.thing": "disabled",
            "browser.act.upload": "disabled"
        }))),
    );
    let read = newer
        .web_policy_claim()
        .expect("a readable claim")
        .expect("the claim is present");
    assert_eq!(read.unknown_keys, 1);
    assert_eq!(read.layer.mode(CapKey::ActUpload), Some(Mode::Disabled));
}

#[test]
fn ask_cannot_loosen_the_claim() {
    let baseline = baseline("enforce", false);
    let user = token(None, Some(claim(json!({ "browser.act.upload": "disabled" }))));
    // Asks can only disable or deny; an ask that names upload leaves the org disable in place.
    let ask = RequestAsk {
        disable: vec!["browser.act.upload".to_owned()],
        ..RequestAsk::default()
    };
    let decision = decide(&baseline, &user, Principal::Human, Some(&ask)).expect("decided");
    assert!(!decision.policy().allows(CapKey::ActUpload));
}

#[test]
fn claim_rev_is_in_the_denial_details() {
    let baseline = baseline("enforce", false);
    let user = token(None, Some(claim(json!({ "browser.act.upload": "disabled" }))));
    let decision = decide(&baseline, &user, Principal::Human, None).expect("decided");
    let denial = decision
        .policy()
        .require(CapKey::ActUpload, "upload_ref")
        .expect_err("denied by the org layer");
    // The core keeps the number; the envelope carries the digest the claim carried (D2-054).
    assert_eq!(denial.details()["policy_rev"], json!(0xa1b2_c3d4_e5f6_u64));
    let wire = quarry_error(&denial).details.expect("the envelope has details");
    assert_eq!(wire["policy_rev"], json!("a1b2c3d4e5f6"));
    assert_eq!(wire["decision_source"], json!("org_policy"));
}

#[test]
fn observe_counts_an_unreadable_claim_and_never_refuses() {
    let observe = baseline("observe", false);
    let bad = token(None, Some(claim(json!({ "browser.act.upload": "sometimes" }))));
    let decision = decide(&observe, &bad, Principal::Human, None).expect("observe does not refuse");
    // The run proceeds as under off; the would-be refusal is the shadow denial.
    assert!(decision.policy().allows(CapKey::ActUpload));
    assert!(matches!(decision.shadow_denial(), Some(PolicyDenial::PolicyUnreadable)));
}

#[test]
fn off_ignores_the_claim_entirely() {
    let off = baseline("off", false);
    let user = token(None, Some(claim(json!({ "browser.act.upload": "disabled" }))));
    let decision = decide(&off, &user, Principal::Human, None).expect("off decides");
    assert!(decision.policy().allows(CapKey::ActUpload));
    assert!(decision.shadow().is_none());
    assert!(decision.shadow_denial().is_none());
}

// ---- W2-CP-U35: enforce-ready intake (observe default, D2-053, D2-054) ----

#[test]
fn claim_is_observed_by_default() {
    // The claim variable is unset: the claim is checked and counted beside the run, never binding.
    let default = baseline_with_claim("enforce", None, false);
    assert_eq!(default.claim_mode(), Enforcement::Observe);
    let user = token(None, Some(claim(json!({ "browser.act.upload": "disabled" }))));
    let decision = decide(&default, &user, Principal::Human, None).expect("observe does not refuse");
    assert!(decision.policy().allows(CapKey::ActUpload), "the run is not bound by the claim");
    let shadow = decision.shadow().expect("the claim is the shadow");
    assert!(!shadow.allows(CapKey::ActUpload), "enforce would disable upload");
    let would = shadow
        .require(CapKey::ActUpload, "upload_ref")
        .expect_err("the would-be denial");
    assert_eq!(would.details()["decision_source"], json!("org_policy"));
}

#[test]
fn enforce_is_one_flip_of_the_claim_mode() {
    let user = token(None, Some(claim(json!({ "browser.act.upload": "disabled" }))));
    let observed = baseline_with_claim("enforce", Some("observe"), false);
    let enforced = baseline_with_claim("enforce", Some("enforce"), false);
    let before = decide(&observed, &user, Principal::Human, None).expect("decided");
    let after = decide(&enforced, &user, Principal::Human, None).expect("decided");
    assert!(before.policy().allows(CapKey::ActUpload));
    assert!(!after.policy().allows(CapKey::ActUpload));
    assert!(after.shadow().is_none(), "an enforced claim needs no shadow");
    // The shadow of the observed run is exactly the policy of the enforced one.
    assert_eq!(
        before.shadow().expect("shadow").digest(),
        after.policy().digest()
    );
}

#[test]
fn claim_mode_parses_strictly() {
    let mode = |value: &'static str| {
        PolicyBaseline::from_lookup(move |name| (name == CLAIM_ENFORCEMENT_ENV).then(|| value.to_owned()))
    };
    assert_eq!(mode("off").unwrap().claim_mode(), Enforcement::Off);
    assert_eq!(mode("observe").unwrap().claim_mode(), Enforcement::Observe);
    assert_eq!(mode("enforce").unwrap().claim_mode(), Enforcement::Enforce);
    assert!(mode("on").is_err(), "an unknown value fails the boot");
    assert!(mode("Enforce").is_err());
}

#[test]
fn observed_claim_counts_an_unreadable_or_missing_required_claim_and_never_refuses() {
    let observed = baseline_with_claim("enforce", None, false);
    let bad = token(None, Some(claim(json!({ "browser.act.upload": "sometimes" }))));
    let decision = decide(&observed, &bad, Principal::Human, None).expect("observe does not refuse");
    assert!(matches!(decision.shadow_denial(), Some(PolicyDenial::PolicyUnreadable)));

    let strict_observed = baseline_with_claim("enforce", None, true);
    let decision = decide(&strict_observed, &token(None, None), Principal::Human, None)
        .expect("a missing required claim is only counted while the claim is observed");
    assert!(matches!(decision.shadow_denial(), Some(PolicyDenial::PolicyUnreadable)));
}

#[test]
fn claim_mode_off_ignores_the_claim_and_the_requirement() {
    let off = baseline_with_claim("enforce", Some("off"), true);
    let user = token(None, Some(claim(json!({ "browser.act.upload": "disabled" }))));
    let decision = decide(&off, &user, Principal::Human, None).expect("decided");
    assert!(decision.policy().allows(CapKey::ActUpload));
    assert!(decision.shadow().is_none());
    let decision = decide(&off, &token(None, None), Principal::Human, None)
        .expect("the requirement is not read with the claim off");
    assert!(decision.shadow_denial().is_none());
}

#[test]
fn baseline_claim_needs_no_shadow() {
    // Every token carries `{"v":1,"rev":"0"}` until WS7.I is switched on: nothing to observe.
    let observed = baseline_with_claim("enforce", None, false);
    let user = token(None, Some(json!({ "v": 1, "rev": "0" })));
    let decision = decide(&observed, &user, Principal::Human, None).expect("decided");
    assert!(decision.shadow().is_none());
    assert!(decision.shadow_denial().is_none());
}

#[test]
fn empty_allow_list_is_no_allow_restriction() {
    // D2-053: the issuer lists only entries beyond the baseline, so `allow: []` means no allow
    // rule, never "allow nothing". Allow lists bind autonomous principals, so a service runs here.
    let enforced = baseline("enforce", false);
    let empty = token(
        Some("service"),
        Some(domain_claim("a1b2c3d4e5f6", json!({ "allow": [], "deny": [] }))),
    );
    let read = empty.web_policy_claim().expect("readable").expect("present");
    assert_eq!(read.layer, quarry_core::web_policy::PolicyLayer::default());
    let decision = decide(&enforced, &empty, Principal::Autonomous, None).expect("decided");
    assert!(decision.policy().check_url("https://anything.example/page").is_ok());
    assert!(gate_url(&decision, "https://another.example/").is_ok());

    // An absent `domains` reads the same way.
    let absent = token(Some("service"), Some(json!({ "v": 1, "rev": "a1b2c3d4e5f6", "caps": {} })));
    let decision = decide(&enforced, &absent, Principal::Autonomous, None).expect("decided");
    assert!(decision.policy().check_url("https://anything.example/").is_ok());
}

#[test]
fn non_empty_allow_list_binds_autonomous_runs() {
    let enforced = baseline("enforce", false);
    let listed = token(
        Some("service"),
        Some(domain_claim("a1b2c3d4e5f6", json!({ "allow": ["example.com"] }))),
    );
    let decision = decide(&enforced, &listed, Principal::Autonomous, None).expect("decided");
    assert!(decision.policy().check_url("https://example.com/page").is_ok());
    let denial = decision
        .policy()
        .check_url("https://other.example/")
        .expect_err("off the org allow list");
    assert!(matches!(
        denial,
        PolicyDenial::DomainNotAllowed {
            decision_source: quarry_core::web_policy::DecisionSource::OrgPolicy,
            ..
        }
    ));
    let error = gate_url(&decision, "https://other.example/").expect_err("a 403");
    assert!(matches!(error.code, ErrorCode::Forbidden));
    assert_eq!(error.details.expect("details")["capability"], json!("domains.allow"));
}

#[test]
fn observed_claim_never_refuses_a_url() {
    let observed = baseline_with_claim("enforce", None, false);
    let deny = token(
        Some("service"),
        Some(domain_claim("a1b2c3d4e5f6", json!({ "deny": ["blocked.example"] }))),
    );
    let decision = decide(&observed, &deny, Principal::Autonomous, None).expect("decided");
    assert!(gate_url(&decision, "https://blocked.example/").is_ok(), "observe only counts");
    let shadow = decision.shadow().expect("the claim is the shadow");
    assert!(shadow.check_url("https://blocked.example/").is_err());
}

#[test]
fn policy_rev_is_the_twelve_hex_digest_on_the_wire() {
    // D2-054: org-core mints 12 lowercase hex digits (or "0"); a leading zero survives.
    assert_eq!(policy_rev_hex(0), "0");
    assert_eq!(policy_rev_hex(0x0123_4567_89ab), "0123456789ab");
    assert_eq!(policy_rev_hex(0xa1b2_c3d4_e5f6), "a1b2c3d4e5f6");

    let enforced = baseline("enforce", false);
    let user = token(
        None,
        Some(json!({ "v": 1, "rev": "0123456789ab", "caps": { "browser.act.upload": "disabled" } })),
    );
    let decision = decide(&enforced, &user, Principal::Human, None).expect("decided");
    let denial = decision
        .policy()
        .require(CapKey::ActUpload, "upload_ref")
        .expect_err("denied by the org layer");
    let wire = quarry_error(&denial).details.expect("details");
    assert_eq!(wire["policy_rev"], json!("0123456789ab"), "equals the stored rev as a string");
}

#[test]
fn unknown_top_level_and_domain_keys_are_counted() {
    let newer = token(
        None,
        Some(json!({
            "v": 1,
            "rev": "a1b2c3d4e5f6",
            "caps": { "browser.future.thing": "disabled" },
            "future_field": true,
            "domains": { "allow": [], "deny": [], "maybe": ["x.example"] }
        })),
    );
    let read = newer.web_policy_claim().expect("readable").expect("present");
    assert_eq!(read.unknown_keys, 3, "one cap, one top-level field, one domains field");
    assert_eq!(read.layer, quarry_core::web_policy::PolicyLayer::default(), "none of them applied");
}
