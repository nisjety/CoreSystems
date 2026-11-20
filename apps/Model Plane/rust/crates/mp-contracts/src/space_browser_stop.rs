//! The starter Stop decision of a room browser run, the model-gateway side (owner decision D21h).
//!
//! Control (`user-core` `space_browser_run_stop_decision.go`) signs `model.space.browser_run_stop`
//! for any current member of a shared Space, with no role floor and no entitlement bit, and under
//! Space zero data retention too (D23). It binds the subject, the Space, the run id and an
//! idempotency key. Control cannot see who started a run, so the decision only proves "this member
//! may ask to stop run R in this Space". The caller of [`verify_browser_run_stop_decision`] must
//! then prove that run R belongs to this Space and that its owner is the decision's subject.
//!
//! This module checks the envelope (shared with [`crate::space_browser`]: `v2.<key-id>.<payload>.
//! <signature>`, Ed25519), the claims `{run_id}` read strictly, the binding, the payload digest
//! byte for byte as Control's `browserRunStopPayloadDigest`, pinned by
//! `user-core/internal/spaces/testdata/space-browser-run-stop-decision.golden.json`, the one
//! permission and the window. Action, schema, audience, permission and digest header all differ
//! from `model.space.browser_run` and `model.run.control`, so a stop token is never accepted as
//! either and neither is accepted here.
//!
//! A zero-retention decision is NOT refused here (D23: stopping is always the safe direction). The
//! flag is returned for the route to use.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::space_browser::{
    check_window, open_decision, write_field, write_revision, BrowserRunRefusal,
    ControlDecisionKey, SignedDecision, BROWSER_RUN_AUDIENCE,
};

/// The action the decision authorizes.
pub const BROWSER_RUN_STOP_ACTION_ID: &str = "model.space.browser_run_stop";
/// The audience the decision is minted for: model-gateway verifies it.
pub const BROWSER_RUN_STOP_AUDIENCE: &str = BROWSER_RUN_AUDIENCE;
/// The schema revision the decision and the digest both carry.
pub const BROWSER_RUN_STOP_SCHEMA: &str = "sha256:space-browser-run-stop-v1";
/// The one permission the decision carries.
pub const PERMISSION_STOP: &str = "space:browser:stop";
/// Opens the payload digest. Part of the wire contract with Control.
const STOP_DIGEST_HEADER: &[u8] = b"model.space.browser_run_stop\0v1\0";
/// Control's `maxBrowserRunStopRunIDBytes`.
const MAX_RUN_ID_BYTES: usize = 128;
/// Largest claims document accepted (bounds the parse).
const MAX_CLAIMS_BYTES: usize = 1024;

/// The non-secret claims the digest binds, as Control returns them beside the token. Read strictly:
/// an unknown or missing field refuses the decision.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserRunStopClaims {
    pub run_id: String,
}

/// What the caller verified itself. Each field must equal the decision's binding.
#[derive(Debug, Clone, Copy)]
pub struct BrowserRunStopExpectation<'a> {
    /// The verified organization of the caller.
    pub org_id: &'a str,
    /// The verified subject of the caller (who must also own the run; the caller checks that).
    pub subject_id: &'a str,
    /// The Space named in the route path.
    pub space_ref: &'a str,
    /// The run named in the route path.
    pub run_id: &'a str,
    /// The idempotency key the request carries (the one Control issued the decision under).
    pub idempotency_key: &'a str,
    /// The current time, for the validity window.
    pub now: DateTime<Utc>,
}

/// A stop decision that passed every check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedBrowserRunStop {
    pub decision_ref: String,
    pub run_id: String,
    /// The Space's zero-retention posture as Control read it (D23). Not a refusal: the route decides
    /// what a stop may still write.
    pub zero_data_retention: bool,
    pub expires_at: DateTime<Utc>,
}

/// Verifies a starter stop decision and its claims against the configured Control key and the
/// caller's expectation. Every check fails closed. The order is: the envelope and key id, the
/// signature, the payload shape, the claims, the binding and digest, the permission, the window.
pub fn verify_browser_run_stop_decision(
    key: &ControlDecisionKey,
    token: &str,
    claims_json: &str,
    expect: &BrowserRunStopExpectation<'_>,
) -> Result<VerifiedBrowserRunStop, BrowserRunRefusal> {
    let decision = open_decision(key, token)?;
    let claims = parse_stop_claims(claims_json)?;
    bind_stop(&decision, &claims, expect)?;
    check_window(&decision, expect.now)?;
    Ok(VerifiedBrowserRunStop {
        decision_ref: decision.decision_ref,
        run_id: claims.run_id,
        zero_data_retention: decision.zero_data_retention,
        expires_at: decision.expires_at,
    })
}

/// Reads the claims document strictly and within its size bound. The run id must meet Control's
/// `browserTokenOK`: non-empty, at most 128 bytes, no NUL and no surrounding whitespace.
pub fn parse_stop_claims(claims_json: &str) -> Result<BrowserRunStopClaims, BrowserRunRefusal> {
    if claims_json.len() > MAX_CLAIMS_BYTES {
        return Err(BrowserRunRefusal::Malformed);
    }
    let claims: BrowserRunStopClaims =
        serde_json::from_str(claims_json).map_err(|_| BrowserRunRefusal::Malformed)?;
    let run_id = claims.run_id.as_str();
    if run_id.is_empty()
        || run_id.len() > MAX_RUN_ID_BYTES
        || run_id.contains('\0')
        || run_id.trim() != run_id
    {
        return Err(BrowserRunRefusal::Malformed);
    }
    Ok(claims)
}

/// The decision must name exactly the caller's binding, the digest of its own fields and claims, and
/// exactly the one stop permission.
fn bind_stop(
    decision: &SignedDecision,
    claims: &BrowserRunStopClaims,
    expect: &BrowserRunStopExpectation<'_>,
) -> Result<(), BrowserRunRefusal> {
    let same_binding = decision.org_id == expect.org_id
        && decision.subject_id == expect.subject_id
        && decision.space_ref == expect.space_ref
        && decision.service_audience == BROWSER_RUN_STOP_AUDIENCE
        && decision.action_id == BROWSER_RUN_STOP_ACTION_ID
        && decision.action_schema_hash == BROWSER_RUN_STOP_SCHEMA
        && decision.idempotency_key == expect.idempotency_key
        && claims.run_id == expect.run_id;
    if !same_binding || decision.payload_digest != stop_payload_digest(decision, claims) {
        return Err(BrowserRunRefusal::Mismatch);
    }
    if decision.permissions.len() != 1 || decision.permissions[0] != PERMISSION_STOP {
        return Err(BrowserRunRefusal::Policy);
    }
    Ok(())
}

/// The payload digest of a starter stop, the bytes Control's `browserRunStopPayloadDigest` hashes,
/// recomputed from the decision and the claims.
#[must_use]
pub fn stop_payload_digest(decision: &SignedDecision, claims: &BrowserRunStopClaims) -> String {
    let mut digest = Sha256::new();
    digest.update(STOP_DIGEST_HEADER);
    for (name, value) in [
        ("org_id", decision.org_id.as_str()),
        ("user_id", decision.subject_id.as_str()),
        ("space_id", decision.space_ref.as_str()),
        ("run_id", claims.run_id.as_str()),
        (
            "recipient_audience_ref",
            decision.recipient_audience_ref.as_str(),
        ),
        (
            "recipient_audience_hash",
            decision.recipient_audience_hash.as_str(),
        ),
        ("privacy_policy_ref", decision.privacy_policy_ref.as_str()),
        (
            "resource_authorization_ref",
            decision.resource_authorization_ref.as_str(),
        ),
        ("action_schema_hash", BROWSER_RUN_STOP_SCHEMA),
        ("idempotency_key", decision.idempotency_key.as_str()),
    ] {
        write_field(&mut digest, name, value);
    }
    for (name, value) in [
        ("authority_revision", decision.authority_revision),
        ("membership_revision", decision.membership_revision),
        ("privacy_revision", decision.privacy_revision),
        (
            "recipient_audience_revision",
            decision.recipient_audience_revision,
        ),
        ("entitlement_revision", decision.entitlement_revision),
    ] {
        write_revision(&mut digest, name, value);
    }
    format!("sha256:{:x}", digest.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space_browser::{verify_browser_run_decision, BrowserRunExpectation};
    use base64::engine::{general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::{json, Value};

    /// Control's golden vectors, read at compile time. Control's copy is
    /// `browserRunStopPayloadDigest`; Control's `TestBrowserRunStopDigestGolden` pins the same file.
    const GOLDEN: &str = include_str!(
        "../../../../../Control Plane/user-core/internal/spaces/testdata/space-browser-run-stop-decision.golden.json"
    );
    const KEY_ID: &str = "control-space-decision-test";

    fn at(instant: &str) -> DateTime<Utc> {
        instant.parse().expect("a fixed instant")
    }

    /// Inside the decision's window (issued 10:00:00, expires 10:02:00).
    fn now() -> DateTime<Utc> {
        at("2026-11-20T10:00:30Z")
    }

    fn control_key() -> SigningKey {
        SigningKey::from_bytes(&[7; 32])
    }

    fn trusted(signing: &SigningKey) -> ControlDecisionKey {
        let encoded = URL_SAFE_NO_PAD.encode(signing.verifying_key().to_bytes());
        ControlDecisionKey::from_config(Some(KEY_ID), Some(&encoded))
            .expect("the test public key parses")
    }

    fn expectation(now: DateTime<Utc>) -> BrowserRunStopExpectation<'static> {
        BrowserRunStopExpectation {
            org_id: "org-1",
            subject_id: "user-2",
            space_ref: "space-room-1",
            run_id: "run-browser-1",
            idempotency_key: "stop-1",
            now,
        }
    }

    fn claims() -> BrowserRunStopClaims {
        BrowserRunStopClaims {
            run_id: "run-browser-1".to_owned(),
        }
    }

    /// The signed decision fields Control writes for the golden input. [`with_digest`] sets the digest.
    fn decision_fields() -> Value {
        json!({
            "decision_ref": "decision-stop-1",
            "org_id": "org-1",
            "space_ref": "space-room-1",
            "subject_id": "user-2",
            "service_audience": BROWSER_RUN_STOP_AUDIENCE,
            "action_id": BROWSER_RUN_STOP_ACTION_ID,
            "action_schema_hash": BROWSER_RUN_STOP_SCHEMA,
            "payload_digest": "",
            "idempotency_key": "stop-1",
            "recipient_audience_ref": "audience:space-room-1:4",
            "recipient_audience_hash": "sha256:audience-hash",
            "privacy_policy_ref": "privacy:org-1:5",
            "resource_authorization_ref": "control:space-room-1:browser-run-stop:7",
            "authority_revision": 7,
            "membership_revision": 3,
            "privacy_revision": 5,
            "recipient_audience_revision": 4,
            "entitlement_revision": 6,
            "permissions": [PERMISSION_STOP],
            "purpose": "ai_run",
            "lawful_basis": "contract",
            "privacy_class": "shared_space",
            "retention_class": "ephemeral",
            "residency": "eu",
            "deletion_scope": "run",
            "zero_data_retention": false,
            "issued_at": "2026-11-20T10:00:00Z",
            "expires_at": "2026-11-20T10:02:00Z",
            "nonce": "nonce-1"
        })
    }

    fn decision_of(fields: &Value) -> SignedDecision {
        serde_json::from_value(fields.clone()).expect("the fields are a complete decision")
    }

    fn with_digest(mut fields: Value, claims: &BrowserRunStopClaims) -> Value {
        let digest = stop_payload_digest(&decision_of(&fields), claims);
        fields["payload_digest"] = json!(digest);
        fields
    }

    fn envelope(signing: &SigningKey, key_id: &str, payload: &Value) -> String {
        let signed = format!(
            "v2.{}.{}",
            URL_SAFE_NO_PAD.encode(key_id),
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(payload).expect("the payload serializes"))
        );
        let signature = signing.sign(signed.as_bytes());
        format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()))
    }

    fn signed(fields: Value, claims: &BrowserRunStopClaims) -> String {
        envelope(&control_key(), KEY_ID, &with_digest(fields, claims))
    }

    fn claims_doc(run_id: &str) -> String {
        json!({ "run_id": run_id }).to_string()
    }

    fn verify(
        token: &str,
        claims_json: &str,
        expect: &BrowserRunStopExpectation<'_>,
    ) -> Result<VerifiedBrowserRunStop, BrowserRunRefusal> {
        verify_browser_run_stop_decision(&trusted(&control_key()), token, claims_json, expect)
    }

    #[test]
    fn stop_payload_digest_reproduces_every_control_golden_vector() {
        let golden: Value = serde_json::from_str(GOLDEN).expect("the golden is JSON");
        assert_eq!(golden["action"], BROWSER_RUN_STOP_ACTION_ID);
        assert_eq!(golden["audience"], BROWSER_RUN_STOP_AUDIENCE);
        assert_eq!(golden["schema"], BROWSER_RUN_STOP_SCHEMA);
        assert_eq!(golden["permission"], PERMISSION_STOP);
        assert_eq!(
            golden["header"]
                .as_str()
                .expect("header")
                .replace("\\x00", "\0")
                .as_bytes(),
            STOP_DIGEST_HEADER
        );
        let input = &golden["input"];
        let vectors = golden["vectors"]
            .as_array()
            .expect("the golden has vectors");
        assert_eq!(
            vectors.len(),
            4,
            "starter_stop, other_run, other_member, retry_new_key"
        );
        let mut digests = std::collections::BTreeSet::new();
        for vector in vectors {
            let name = vector["name"].as_str().expect("a vector name");
            let mut fields = decision_fields();
            for (field, golden_field) in [
                ("org_id", "org_id"),
                ("space_ref", "space_id"),
                ("recipient_audience_ref", "recipient_audience_ref"),
                ("recipient_audience_hash", "recipient_audience_hash"),
                ("privacy_policy_ref", "privacy_policy_ref"),
                ("resource_authorization_ref", "resource_authorization_ref"),
                ("authority_revision", "authority_revision"),
                ("membership_revision", "membership_revision"),
                ("privacy_revision", "privacy_revision"),
                ("recipient_audience_revision", "recipient_audience_revision"),
                ("entitlement_revision", "entitlement_revision"),
            ] {
                fields[field] = input[golden_field].clone();
            }
            // Each vector's user_id replaces input.user_id.
            fields["subject_id"] = vector["user_id"].clone();
            fields["idempotency_key"] = vector["idempotency_key"].clone();
            let claims = BrowserRunStopClaims {
                run_id: vector["run_id"].as_str().expect("run_id").to_owned(),
            };
            let digest = stop_payload_digest(&decision_of(&fields), &claims);
            assert_eq!(
                digest,
                vector["payload_digest"].as_str().expect("payload_digest"),
                "{name}"
            );
            assert!(
                digests.insert(digest),
                "{name}: every vector binds a distinct digest"
            );
        }
    }

    #[test]
    fn a_control_signed_stop_decision_verifies() {
        let token = signed(decision_fields(), &claims());
        let verified =
            verify(&token, &claims_doc("run-browser-1"), &expectation(now())).expect("verifies");
        assert_eq!(verified.decision_ref, "decision-stop-1");
        assert_eq!(verified.run_id, "run-browser-1");
        assert!(!verified.zero_data_retention);
        assert_eq!(verified.expires_at, at("2026-11-20T10:02:00Z"));
    }

    /// D23: a stop decision signed under Space zero data retention verifies, and says so.
    #[test]
    fn a_zero_retention_stop_decision_verifies_and_carries_the_flag() {
        let mut fields = decision_fields();
        fields["zero_data_retention"] = json!(true);
        let token = signed(fields, &claims());
        let verified =
            verify(&token, &claims_doc("run-browser-1"), &expectation(now())).expect("verifies");
        assert!(verified.zero_data_retention);
    }

    #[test]
    fn another_run_member_space_org_or_key_is_a_mismatch() {
        let token = signed(decision_fields(), &claims());
        let doc = claims_doc("run-browser-1");
        let base = expectation(now());
        for (label, expect) in [
            (
                "run",
                BrowserRunStopExpectation {
                    run_id: "run-browser-2",
                    ..base
                },
            ),
            (
                "member",
                BrowserRunStopExpectation {
                    subject_id: "user-3",
                    ..base
                },
            ),
            (
                "space",
                BrowserRunStopExpectation {
                    space_ref: "space-room-2",
                    ..base
                },
            ),
            (
                "org",
                BrowserRunStopExpectation {
                    org_id: "org-2",
                    ..base
                },
            ),
            (
                "key",
                BrowserRunStopExpectation {
                    idempotency_key: "stop-2",
                    ..base
                },
            ),
        ] {
            assert_eq!(
                verify(&token, &doc, &expect),
                Err(BrowserRunRefusal::Mismatch),
                "{label}"
            );
        }
        // Claims naming another run than the one the decision's digest binds.
        let path_run2 = BrowserRunStopExpectation {
            run_id: "run-browser-2",
            ..base
        };
        assert_eq!(
            verify(&token, &claims_doc("run-browser-2"), &path_run2),
            Err(BrowserRunRefusal::Mismatch),
            "forged claims"
        );
    }

    #[test]
    fn a_browser_run_start_or_run_control_decision_is_not_a_stop_decision() {
        for (action, schema) in [
            ("model.space.browser_run", "sha256:space-browser-run-v1"),
            ("model.run.control", BROWSER_RUN_STOP_SCHEMA),
            (BROWSER_RUN_STOP_ACTION_ID, "sha256:space-browser-run-v1"),
        ] {
            let mut fields = decision_fields();
            fields["action_id"] = json!(action);
            fields["action_schema_hash"] = json!(schema);
            let token = signed(fields, &claims());
            assert_eq!(
                verify(&token, &claims_doc("run-browser-1"), &expectation(now())),
                Err(BrowserRunRefusal::Mismatch),
                "{action} {schema}"
            );
        }
        // And the other way: a stop decision is never a browser run start.
        let token = signed(decision_fields(), &claims());
        let start = BrowserRunExpectation {
            org_id: "org-1",
            subject_id: "user-2",
            space_ref: "space-room-1",
            thread_id: "",
            goal_digest: "sha256:e3eb81231b5a76dd873dfa95da2a0a0f0c576567a5fed957006fc51b68c405dc",
            idempotency_key: "stop-1",
            now: now(),
        };
        assert!(verify_browser_run_decision(
            &trusted(&control_key()),
            &token,
            &claims_doc("run-browser-1"),
            &start
        )
        .is_err());
    }

    #[test]
    fn the_permission_must_be_exactly_the_stop_permission() {
        for permissions in [
            json!(["space:browser:run"]),
            json!([PERMISSION_STOP, "space:browser:run"]),
            json!(["space:run:control"]),
        ] {
            let mut fields = decision_fields();
            fields["permissions"] = permissions.clone();
            let token = signed(fields, &claims());
            assert_eq!(
                verify(&token, &claims_doc("run-browser-1"), &expectation(now())),
                Err(BrowserRunRefusal::Policy),
                "{permissions}"
            );
        }
    }

    #[test]
    fn untrusted_keys_and_tampering_are_refused() {
        let foreign = SigningKey::from_bytes(&[9; 32]);
        let forged = envelope(&foreign, KEY_ID, &with_digest(decision_fields(), &claims()));
        let doc = claims_doc("run-browser-1");
        assert_eq!(
            verify(&forged, &doc, &expectation(now())),
            Err(BrowserRunRefusal::Untrusted)
        );
        let other_id = envelope(
            &control_key(),
            "other-key",
            &with_digest(decision_fields(), &claims()),
        );
        assert_eq!(
            verify(&other_id, &doc, &expectation(now())),
            Err(BrowserRunRefusal::Untrusted)
        );
        assert_eq!(
            verify("v2.a.b", &doc, &expectation(now())),
            Err(BrowserRunRefusal::Malformed)
        );
    }

    #[test]
    fn the_window_is_enforced() {
        let token = signed(decision_fields(), &claims());
        let doc = claims_doc("run-browser-1");
        assert_eq!(
            verify(&token, &doc, &expectation(at("2026-11-20T10:02:00Z"))),
            Err(BrowserRunRefusal::Window)
        );
        assert_eq!(
            verify(&token, &doc, &expectation(at("2026-11-20T09:58:00Z"))),
            Err(BrowserRunRefusal::Window)
        );
    }

    #[test]
    fn claims_are_read_strictly() {
        for bad in [
            json!({ "run_id": "run-browser-1", "surprise": true }).to_string(),
            json!({}).to_string(),
            json!({ "run_id": "" }).to_string(),
            json!({ "run_id": " run-browser-1" }).to_string(),
            json!({ "run_id": "a".repeat(129) }).to_string(),
            json!({ "run_id": "run\u{0}1" }).to_string(),
            "not json".to_owned(),
        ] {
            assert_eq!(
                parse_stop_claims(&bad),
                Err(BrowserRunRefusal::Malformed),
                "{bad}"
            );
        }
        assert_eq!(
            parse_stop_claims(&claims_doc("run-browser-1")),
            Ok(claims())
        );
    }

    #[test]
    fn a_decision_without_its_zero_retention_flag_is_malformed() {
        let mut fields = with_digest(decision_fields(), &claims());
        fields
            .as_object_mut()
            .expect("object")
            .remove("zero_data_retention");
        let token = envelope(&control_key(), KEY_ID, &fields);
        assert_eq!(
            verify(&token, &claims_doc("run-browser-1"), &expectation(now())),
            Err(BrowserRunRefusal::Malformed)
        );
    }
}
