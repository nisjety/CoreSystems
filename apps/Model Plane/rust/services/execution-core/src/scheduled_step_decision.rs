//! Verification of the Control-owned `model.schedule.step` decision.
//!
//! This verifier is intentionally independent from the Orchestrator verifier.
//! Orchestrator obtains the decision, but Execution Core is the effect owner and
//! must verify the direct-hop bearer again immediately before any model or
//! provider work.  The signed token binds the complete scheduled-step tuple and
//! the current authority evidence; a caller cannot turn a valid token into a
//! different step by changing the gRPC request.

use std::time::Duration;

use base64::{
    alphabet,
    engine::{
        general_purpose::{GeneralPurpose, NO_PAD, PAD, URL_SAFE_NO_PAD},
        Engine as _,
    },
};
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use mp_contracts::model_plane::v1::ExecuteScheduledStepRequest;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tonic::Status;

pub(crate) const DECISION_VERSION: &str = "v2";
pub(crate) const ACTION_ID: &str = "model.schedule.step";
pub(crate) const SERVICE_AUDIENCE: &str = "model-plane-execution-core";
pub(crate) const ACTION_SCHEMA_HASH: &str = "sha256:space-scheduled-step-v1";
// A step decision that carries a stored-consent contract binds the contract
// digest and expiry, so it uses its own schema hash; v1 stays byte-identical.
pub(crate) const ACTION_SCHEMA_HASH_CONTRACT: &str = "sha256:space-scheduled-step-v2";
// Default off. When on, a step decision without a stored-consent contract is
// refused; only the literal "true" enables it, as the other switches do.
const CONTRACT_REQUIRED_ENV: &str = "EXECUTION_CORE_SCHEDULED_CONTRACT_REQUIRED";
pub(crate) const REQUIRED_PERMISSION: &str = "schedule:step";
const MAX_TOKEN_BYTES: usize = 16_384;
const MAX_FUTURE_ISSUED_SKEW: Duration = Duration::from_secs(60);
// Go's `encoding/base64` decodes non-strictly: the unused low bits of a final character may be
// set and are ignored, where Rust's engines refuse them unless told otherwise. These four
// engines allow them and keep the padding mode of the predefined engine each one mirrors
// (`PAD` requires canonical padding, `NO_PAD` requires none), so the accepted padding is
// unchanged. Token segments are not key material and keep the strict `URL_SAFE_NO_PAD`.
const GO_URL_SAFE_NO_PAD: GeneralPurpose = GeneralPurpose::new(
    &alphabet::URL_SAFE,
    NO_PAD.with_decode_allow_trailing_bits(true),
);
const GO_URL_SAFE: GeneralPurpose = GeneralPurpose::new(
    &alphabet::URL_SAFE,
    PAD.with_decode_allow_trailing_bits(true),
);
const GO_STANDARD: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    PAD.with_decode_allow_trailing_bits(true),
);
const GO_STANDARD_NO_PAD: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    NO_PAD.with_decode_allow_trailing_bits(true),
);

#[derive(Clone)]
pub(crate) struct ScheduledStepDecisionVerifier {
    key_id: String,
    key: VerifyingKey,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Claims {
    decision_ref: String,
    org_id: String,
    space_ref: String,
    subject_id: String,
    service_audience: String,
    action_id: String,
    action_schema_hash: String,
    payload_digest: String,
    idempotency_key: String,
    recipient_audience_ref: String,
    recipient_audience_hash: String,
    privacy_policy_ref: String,
    resource_authorization_ref: String,
    authority_revision: i64,
    membership_revision: i64,
    privacy_revision: i64,
    recipient_audience_revision: i64,
    entitlement_revision: i64,
    permissions: Vec<String>,
    zero_data_retention: bool,
    nonce: String,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    // Absent on every decision issued before the consent contract existed; the
    // serde default keeps those tokens parseable under deny_unknown_fields.
    #[serde(default)]
    contract_digest: Option<String>,
    #[serde(default)]
    contract_expires_at: Option<DateTime<Utc>>,
}

impl ScheduledStepDecisionVerifier {
    pub(crate) fn from_env() -> anyhow::Result<Self> {
        let key_id = first_env(&[
            "EXECUTION_CORE_SCHEDULED_STEP_DECISION_KEY_ID",
            "CONTROL_SPACE_DECISION_KEY_ID",
        ])
        .ok_or_else(|| anyhow::anyhow!("scheduled-step Control decision key ID is required"))?;
        let encoded_key = first_env(&[
            "EXECUTION_CORE_SCHEDULED_STEP_DECISION_PUBLIC_KEY_BASE64",
            "CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64",
        ])
        .ok_or_else(|| anyhow::anyhow!("scheduled-step Control decision public key is required"))?;
        Self::from_parts(key_id, &encoded_key)
    }

    /// The env-free half of [`Self::from_env`]. The key is Control's, so it follows the rule
    /// every Model Plane consumer of the Control decision keys shares: unpadded base64url is
    /// preferred; padded base64url and standard base64 are accepted with a boot warning;
    /// anything else is refused. Decoding is non-strict, as Go's is (see the `GO_*` engines).
    /// Never narrow this: a decoder that understood only one form once silently disabled the
    /// whole decision lane while other services started fine.
    fn from_parts(key_id: String, encoded_key: &str) -> anyhow::Result<Self> {
        // Go's decoder skips every CR and LF inside the value.
        let encoded_key: String = encoded_key
            .trim()
            .chars()
            .filter(|c| !matches!(c, '\r' | '\n'))
            .collect();
        let encoded_key = encoded_key.as_str();
        let (bytes, canonical) = if let Ok(bytes) = GO_URL_SAFE_NO_PAD.decode(encoded_key) {
            (bytes, true)
        } else {
            let bytes = GO_URL_SAFE
                .decode(encoded_key)
                .or_else(|_| GO_STANDARD.decode(encoded_key))
                .or_else(|_| GO_STANDARD_NO_PAD.decode(encoded_key))
                .map_err(|_| anyhow::anyhow!("scheduled-step Control public key is not base64url"))?;
            (bytes, false)
        };
        let bytes: [u8; 32] = bytes.try_into().map_err(|_| {
            anyhow::anyhow!("scheduled-step Control public key must contain 32 bytes")
        })?;
        let key = VerifyingKey::from_bytes(&bytes)
            .map_err(|_| anyhow::anyhow!("scheduled-step Control public key is invalid"))?;
        if !canonical {
            tracing::warn!(
                "the scheduled-step Control decision public key is not unpadded base64url; it was \
                 accepted, but Control emits unpadded base64url and other planes may refuse this form"
            );
        }
        Ok(Self { key_id, key })
    }

    #[cfg(test)]
    pub(crate) fn for_test(key_id: &str, key: VerifyingKey) -> Self {
        Self {
            key_id: key_id.to_owned(),
            key,
        }
    }

    #[allow(clippy::result_large_err)]
    pub(crate) fn verify(
        &self,
        request: &ExecuteScheduledStepRequest,
        now: DateTime<Utc>,
    ) -> Result<(), Status> {
        self.verify_with_contract_policy(request, now, contract_required())
    }

    #[allow(clippy::result_large_err)]
    fn verify_with_contract_policy(
        &self,
        request: &ExecuteScheduledStepRequest,
        now: DateTime<Utc>,
        contract_required: bool,
    ) -> Result<(), Status> {
        let token = request.control_decision_token.trim();
        if token.is_empty() || token.len() > MAX_TOKEN_BYTES {
            return Err(Status::permission_denied(
                "Control scheduled-step decision is missing or too large",
            ));
        }
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 4 || parts[0] != DECISION_VERSION {
            return Err(Status::permission_denied(
                "invalid Control scheduled-step decision envelope",
            ));
        }
        let key_id = URL_SAFE_NO_PAD.decode(parts[1]).map_err(|_| {
            Status::permission_denied("untrusted Control scheduled-step decision key")
        })?;
        if key_id.as_slice() != self.key_id.as_bytes() {
            return Err(Status::permission_denied(
                "untrusted Control scheduled-step decision key",
            ));
        }
        let payload = URL_SAFE_NO_PAD.decode(parts[2]).map_err(|_| {
            Status::permission_denied("invalid Control scheduled-step decision payload")
        })?;
        let signature_bytes = URL_SAFE_NO_PAD.decode(parts[3]).map_err(|_| {
            Status::permission_denied("invalid Control scheduled-step decision signature")
        })?;
        let signature = Signature::from_slice(&signature_bytes).map_err(|_| {
            Status::permission_denied("invalid Control scheduled-step decision signature")
        })?;
        self.key
            .verify(
                format!("{}.{}.{}", parts[0], parts[1], parts[2]).as_bytes(),
                &signature,
            )
            .map_err(|_| {
                Status::permission_denied("invalid Control scheduled-step decision signature")
            })?;
        let claims: Claims = serde_json::from_slice(&payload).map_err(|_| {
            Status::permission_denied("invalid Control scheduled-step decision claims")
        })?;

        let references_present = [
            claims.decision_ref.as_str(),
            claims.nonce.as_str(),
            claims.recipient_audience_ref.as_str(),
            claims.recipient_audience_hash.as_str(),
            claims.privacy_policy_ref.as_str(),
            claims.resource_authorization_ref.as_str(),
        ]
        .iter()
        .all(|value| !value.trim().is_empty());
        let revisions_present = claims.authority_revision > 0
            && claims.membership_revision > 0
            && claims.privacy_revision > 0
            && claims.recipient_audience_revision > 0
            && claims.entitlement_revision > 0;
        check_contract(&claims, now)?;
        if contract_required && claims.contract_digest.is_none() {
            return Err(Status::failed_precondition("scheduled_contract_required"));
        }
        let expected_schema = if claims.contract_digest.is_some() {
            ACTION_SCHEMA_HASH_CONTRACT
        } else {
            ACTION_SCHEMA_HASH
        };
        let matches = claims.org_id == request.org_id
            && claims.space_ref == request.space_id
            && claims.subject_id == request.subject_id
            && claims.service_audience == SERVICE_AUDIENCE
            && claims.action_id == ACTION_ID
            && claims.action_schema_hash == expected_schema
            && claims.idempotency_key == request.idempotency_key
            && claims.payload_digest == payload_digest(&claims, request, expected_schema)
            && claims.permissions.iter().any(|p| p == REQUIRED_PERMISSION)
            && !claims.zero_data_retention
            && references_present
            && revisions_present
            && claims.expires_at > now
            && claims.issued_at
                <= now
                    + chrono::Duration::from_std(MAX_FUTURE_ISSUED_SKEW)
                        .expect("constant duration is valid");
        if !matches {
            return Err(Status::permission_denied(
                "Control decision does not authorize this scheduled step",
            ));
        }
        Ok(())
    }
}

fn contract_required() -> bool {
    std::env::var(CONTRACT_REQUIRED_ENV).ok().as_deref() == Some("true")
}

fn first_env(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    })
}

/// A contract-bound decision must carry both fields and the contract must still
/// be live; a half-bound decision is refused rather than treated as legacy.
#[allow(clippy::result_large_err)]
fn check_contract(claims: &Claims, now: DateTime<Utc>) -> Result<(), Status> {
    match (&claims.contract_digest, claims.contract_expires_at) {
        (None, None) => Ok(()),
        (Some(digest), Some(expires_at)) => {
            if !is_sha256_digest(digest) {
                return Err(Status::failed_precondition("scheduled_contract_invalid"));
            }
            if expires_at <= now {
                return Err(Status::failed_precondition("scheduled_contract_expired"));
            }
            Ok(())
        }
        _ => Err(Status::failed_precondition("scheduled_contract_invalid")),
    }
}

fn is_sha256_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn payload_digest(
    claims: &Claims,
    request: &ExecuteScheduledStepRequest,
    schema: &str,
) -> String {
    let mut digest = Sha256::new();
    digest.update(format!("{ACTION_ID}\0v1\0").as_bytes());
    for (name, value) in [
        ("org_id", claims.org_id.as_str()),
        ("user_id", claims.subject_id.as_str()),
        ("space_id", claims.space_ref.as_str()),
        ("run_id", request.run_id.as_str()),
        ("thread_id", request.thread_id.as_str()),
        ("schedule_id", request.schedule_id.as_str()),
        ("fire_key", request.fire_key.as_str()),
        ("template_digest", request.template_digest.as_str()),
        ("step_id", request.step_id.as_str()),
        ("policy_digest", request.policy_digest.as_str()),
        ("idempotency_key", request.idempotency_key.as_str()),
        (
            "recipient_audience_ref",
            claims.recipient_audience_ref.as_str(),
        ),
        (
            "recipient_audience_hash",
            claims.recipient_audience_hash.as_str(),
        ),
        ("privacy_policy_ref", claims.privacy_policy_ref.as_str()),
        (
            "resource_authorization_ref",
            claims.resource_authorization_ref.as_str(),
        ),
        ("action_schema_hash", schema),
    ] {
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    for (name, value) in [
        ("step_index", i64::from(request.step_index)),
        ("authority_revision", claims.authority_revision),
        ("membership_revision", claims.membership_revision),
        ("privacy_revision", claims.privacy_revision),
        (
            "recipient_audience_revision",
            claims.recipient_audience_revision,
        ),
        ("entitlement_revision", claims.entitlement_revision),
    ] {
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update(value.to_be_bytes());
    }
    if let (Some(contract_digest), Some(expires_at)) =
        (&claims.contract_digest, claims.contract_expires_at)
    {
        digest.update(b"contract_digest\0");
        digest.update((contract_digest.len() as u64).to_be_bytes());
        digest.update(contract_digest.as_bytes());
        digest.update(b"contract_expires_at\0");
        digest.update(expires_at.timestamp().to_be_bytes());
    }
    format!("sha256:{:x}", digest.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE};
    use ed25519_dalek::{Signer, SigningKey};
    use mp_contracts::model_plane::v1::ExecuteScheduledStepRequest;

    fn request() -> ExecuteScheduledStepRequest {
        ExecuteScheduledStepRequest {
            run_id: "run_1".to_owned(),
            thread_id: "thread_1".to_owned(),
            org_id: "org_1".to_owned(),
            space_id: "space_1".to_owned(),
            subject_id: "user_1".to_owned(),
            schedule_id: "schedule_1".to_owned(),
            fire_key: "fire_1".to_owned(),
            template_digest: format!("sha256:{}", "a".repeat(64)),
            step_id: "run_1:step:0".to_owned(),
            step_index: 0,
            policy_digest: format!("sha256:{}", "b".repeat(64)),
            idempotency_key: "fire_1:step:0".to_owned(),
            control_decision_token: String::new(),
        }
    }

    fn token(
        signing: &SigningKey,
        key_id: &str,
        request: &ExecuteScheduledStepRequest,
        now: DateTime<Utc>,
    ) -> String {
        token_with_contract(signing, key_id, request, now, None)
    }

    fn token_with_contract(
        signing: &SigningKey,
        key_id: &str,
        request: &ExecuteScheduledStepRequest,
        now: DateTime<Utc>,
        contract: Option<(&str, DateTime<Utc>)>,
    ) -> String {
        let schema = if contract.is_some() {
            ACTION_SCHEMA_HASH_CONTRACT
        } else {
            ACTION_SCHEMA_HASH
        };
        let mut claims = serde_json::json!({
            "decision_ref":"decision_1", "org_id":request.org_id, "space_ref":request.space_id,
            "subject_id":request.subject_id, "service_audience":SERVICE_AUDIENCE,
            "action_id":ACTION_ID, "action_schema_hash":schema,
            "payload_digest":"", "idempotency_key":request.idempotency_key,
            "recipient_audience_ref":"aud_1", "recipient_audience_hash":"hash_1",
            "privacy_policy_ref":"privacy_1", "resource_authorization_ref":"resource_1",
            "authority_revision":1, "membership_revision":1, "privacy_revision":1,
            "recipient_audience_revision":1, "entitlement_revision":1,
            "permissions":[REQUIRED_PERMISSION], "zero_data_retention":false,
            "nonce":"nonce_1", "issued_at":now, "expires_at":now + chrono::Duration::minutes(2),
        });
        if let Some((digest, expires_at)) = contract {
            claims["contract_digest"] = serde_json::Value::String(digest.to_owned());
            claims["contract_expires_at"] = serde_json::json!(expires_at);
        }
        let parsed: Claims = serde_json::from_value(claims.clone()).expect("claims");
        claims["payload_digest"] = serde_json::Value::String(payload_digest(&parsed, request, schema));
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("payload"));
        let encoded_key = URL_SAFE_NO_PAD.encode(key_id.as_bytes());
        let signing_input = format!("{DECISION_VERSION}.{encoded_key}.{payload}");
        let signature = URL_SAFE_NO_PAD.encode(signing.sign(signing_input.as_bytes()).to_bytes());
        format!("{signing_input}.{signature}")
    }

    #[test]
    fn verifies_exact_control_decision_and_digest() {
        let now = Utc::now();
        let signing = SigningKey::from_bytes(&[7; 32]);
        let verifier =
            ScheduledStepDecisionVerifier::for_test("control-key", signing.verifying_key());
        let mut req = request();
        req.control_decision_token = token(&signing, "control-key", &req, now);
        verifier.verify(&req, now).expect("valid decision");
    }

    #[test]
    fn verifies_contract_bound_decision_and_refuses_expired_contract() {
        let now = Utc::now();
        let signing = SigningKey::from_bytes(&[9; 32]);
        let verifier =
            ScheduledStepDecisionVerifier::for_test("control-key", signing.verifying_key());
        let digest = format!("sha256:{}", "c".repeat(64));
        let mut req = request();
        req.control_decision_token = token_with_contract(
            &signing,
            "control-key",
            &req,
            now,
            Some((&digest, now + chrono::Duration::hours(1))),
        );
        verifier.verify(&req, now).expect("live contract verifies");

        req.control_decision_token = token_with_contract(
            &signing,
            "control-key",
            &req,
            now,
            Some((&digest, now - chrono::Duration::seconds(1))),
        );
        let status = verifier.verify(&req, now).expect_err("expired contract");
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
        assert_eq!(status.message(), "scheduled_contract_expired");
    }

    #[test]
    fn refuses_malformed_contract_digest() {
        let now = Utc::now();
        let signing = SigningKey::from_bytes(&[10; 32]);
        let verifier =
            ScheduledStepDecisionVerifier::for_test("control-key", signing.verifying_key());
        let mut req = request();
        req.control_decision_token = token_with_contract(
            &signing,
            "control-key",
            &req,
            now,
            Some(("sha256:short", now + chrono::Duration::hours(1))),
        );
        let status = verifier.verify(&req, now).expect_err("malformed digest");
        assert_eq!(status.message(), "scheduled_contract_invalid");
    }

    #[test]
    fn contract_required_switch_refuses_a_decision_without_a_contract() {
        let now = Utc::now();
        let signing = SigningKey::from_bytes(&[11; 32]);
        let verifier =
            ScheduledStepDecisionVerifier::for_test("control-key", signing.verifying_key());
        let mut req = request();
        req.control_decision_token = token(&signing, "control-key", &req, now);
        verifier
            .verify_with_contract_policy(&req, now, false)
            .expect("legacy decision verifies while the switch is off");
        let status = verifier
            .verify_with_contract_policy(&req, now, true)
            .expect_err("legacy decision refused while the switch is on");
        assert_eq!(status.message(), "scheduled_contract_required");

        let digest = format!("sha256:{}", "d".repeat(64));
        req.control_decision_token = token_with_contract(
            &signing,
            "control-key",
            &req,
            now,
            Some((&digest, now + chrono::Duration::hours(1))),
        );
        verifier
            .verify_with_contract_policy(&req, now, true)
            .expect("contract-bound decision verifies while the switch is on");
    }

    #[test]
    fn rejects_tampered_binding_expired_and_wrong_key() {
        let now = Utc::now();
        let signing = SigningKey::from_bytes(&[8; 32]);
        let verifier =
            ScheduledStepDecisionVerifier::for_test("control-key", signing.verifying_key());
        let mut req = request();
        req.control_decision_token = token(
            &signing,
            "control-key",
            &req,
            now - chrono::Duration::minutes(3),
        );
        assert!(verifier.verify(&req, now).is_err());
        req.control_decision_token = token(&signing, "other-key", &req, now);
        assert!(verifier.verify(&req, now).is_err());
        req.control_decision_token = token(&signing, "control-key", &req, now);
        req.step_id = "run_1:step:1".to_owned();
        assert!(verifier.verify(&req, now).is_err());
    }

    /// A key whose standard-base64 form differs from its URL-safe form, so the encodings cannot
    /// be confused with each other.
    fn key_with_url_specific_chars() -> SigningKey {
        (0..=255u8)
            .map(|seed| SigningKey::from_bytes(&[seed; 32]))
            .find(|signing| {
                STANDARD_NO_PAD
                    .encode(signing.verifying_key().to_bytes())
                    .contains(['+', '/'])
            })
            .expect("a key with + or / in standard base64")
    }

    #[test]
    fn control_public_key_is_accepted_in_every_encoding_and_verifies_the_same_decision() {
        let now = Utc::now();
        let signing = key_with_url_specific_chars();
        let public = signing.verifying_key().to_bytes();
        let mut req = request();
        req.control_decision_token = token(&signing, "control-key", &req, now);
        for (name, encoded) in [
            ("unpadded base64url", URL_SAFE_NO_PAD.encode(public)),
            ("padded base64url", URL_SAFE.encode(public)),
            ("standard base64", STANDARD.encode(public)),
            ("unpadded standard base64", STANDARD_NO_PAD.encode(public)),
            ("whitespace padded", format!("  {}\n", URL_SAFE_NO_PAD.encode(public))),
        ] {
            let verifier = ScheduledStepDecisionVerifier::from_parts("control-key".to_owned(), &encoded)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            verifier
                .verify(&req, now)
                .unwrap_or_else(|status| panic!("{name}: {}", status.message()));
        }
    }

    #[test]
    fn control_public_key_refuses_garbage_and_wrong_length_with_the_original_messages() {
        let garbage = ScheduledStepDecisionVerifier::from_parts("k".to_owned(), "not base64 at all!!")
            .err()
            .expect("garbage is refused");
        assert_eq!(
            garbage.to_string(),
            "scheduled-step Control public key is not base64url"
        );
        let short = ScheduledStepDecisionVerifier::from_parts(
            "k".to_owned(),
            &URL_SAFE_NO_PAD.encode(b"too short"),
        )
        .err()
        .expect("a short key is refused");
        assert_eq!(
            short.to_string(),
            "scheduled-step Control public key must contain 32 bytes"
        );
    }

    /// `key 2` in Go's canonical base64url form, and the four encodings `from_parts` tries, each
    /// with the unused low bits of its final character set (`Q` became `R`). Go's
    /// `DecodePublicKey` accepts all four and yields the key's 32 bytes. These vectors were
    /// checked against Go, not produced by this decoder.
    #[test]
    fn control_public_key_accepts_non_zero_trailing_bits_as_go_does() {
        let now = Utc::now();
        let signing = SigningKey::from_bytes(&[2; 32]);
        assert_eq!(
            URL_SAFE_NO_PAD.encode(signing.verifying_key().to_bytes()),
            "gTl3Dqh9F19Wo1Rmw0x-zMuNipG07jeiXfYPW4_Js5Q",
            "the fixture is key 2 in Go's canonical form"
        );
        let mut req = request();
        req.control_decision_token = token(&signing, "control-key", &req, now);
        for (name, encoded) in [
            (
                "unpadded base64url",
                "gTl3Dqh9F19Wo1Rmw0x-zMuNipG07jeiXfYPW4_Js5R",
            ),
            (
                "padded base64url",
                "gTl3Dqh9F19Wo1Rmw0x-zMuNipG07jeiXfYPW4_Js5R=",
            ),
            (
                "padded standard base64",
                "gTl3Dqh9F19Wo1Rmw0x+zMuNipG07jeiXfYPW4/Js5R=",
            ),
            (
                "unpadded standard base64",
                "gTl3Dqh9F19Wo1Rmw0x+zMuNipG07jeiXfYPW4/Js5R",
            ),
        ] {
            let verifier =
                ScheduledStepDecisionVerifier::from_parts("control-key".to_owned(), encoded)
                    .unwrap_or_else(|error| panic!("{name}: {error}"));
            verifier
                .verify(&req, now)
                .unwrap_or_else(|status| panic!("{name}: {}", status.message()));
        }
    }

    #[test]
    fn control_public_key_skips_embedded_line_breaks_as_go_does() {
        let now = Utc::now();
        let signing = SigningKey::from_bytes(&[2; 32]);
        let canonical = "gTl3Dqh9F19Wo1Rmw0x-zMuNipG07jeiXfYPW4_Js5Q";
        let mut req = request();
        req.control_decision_token = token(&signing, "control-key", &req, now);
        for encoded in [
            format!("{}\n{}", &canonical[..20], &canonical[20..]),
            format!("{}\r\n{}", &canonical[..20], &canonical[20..]),
        ] {
            let verifier =
                ScheduledStepDecisionVerifier::from_parts("control-key".to_owned(), &encoded)
                    .expect("line break is skipped");
            verifier.verify(&req, now).expect("verifies");
        }
    }

    #[test]
    fn control_public_key_refuses_over_padding_and_a_short_key_as_go_does() {
        let canonical = "gTl3Dqh9F19Wo1Rmw0x-zMuNipG07jeiXfYPW4_Js5Q";
        for encoded in [
            format!("{canonical}=="),
            format!("{canonical}==="),
            format!("{} {}", &canonical[..20], &canonical[20..]),
        ] {
            assert!(
                ScheduledStepDecisionVerifier::from_parts("k".to_owned(), &encoded).is_err(),
                "{encoded} must be refused"
            );
        }
        let short = ScheduledStepDecisionVerifier::from_parts("k".to_owned(), &canonical[..42])
            .err()
            .expect("42 characters is refused");
        assert_eq!(
            short.to_string(),
            "scheduled-step Control public key must contain 32 bytes"
        );
    }
}
