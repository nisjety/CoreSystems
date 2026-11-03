//! WS7.J (observe part): the org web-policy claim, read at intake and applied as the org layer.
//!
//! These tests drive the pure path the scrape and agent routes take: the token's claim, the
//! baseline's enforcement mode and the request's ask, through `PolicyBaseline::org_input` and
//! `PolicyBaseline::decide_with_org`. The HTTP-level tests (real tokens, the scrape and agent
//! routes) belong to the test phase of the wave; see the W2-U35 progress note.
//!
//! Run: `cargo test -p quarry-edge --test web_policy_claim --test zdr_claim_floor`.

use quarry_core::error::ErrorCode;
use quarry_core::web_policy::{CapKey, Mode, PolicyDenial, Principal, RequestAsk};
use quarry_edge::auth::Claims;
use quarry_edge::policy_baseline::{PolicyBaseline, ENFORCEMENT_ENV, REQUIRE_CLAIM_ENV};
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

/// The baseline in one enforcement mode, with the claim requirement set by `require`.
fn baseline(enforcement: &str, require: bool) -> PolicyBaseline {
    let enforcement = enforcement.to_owned();
    PolicyBaseline::from_lookup(move |name| match name {
        ENFORCEMENT_ENV => Some(enforcement.clone()),
        REQUIRE_CLAIM_ENV if require => Some("1".to_owned()),
        _ => None,
    })
    .expect("a valid baseline configuration")
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
    assert_eq!(denial.details()["policy_rev"], json!(0xa1b2_c3d4_e5f6_u64));
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
