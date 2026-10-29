//! Verification of the Control-owned `model.schedule.step` decision.
//!
//! This verifier is intentionally independent from the Orchestrator verifier.
//! Orchestrator obtains the decision, but Execution Core is the effect owner and
//! must verify the direct-hop bearer again immediately before any model or
//! provider work.  The signed token binds the complete scheduled-step tuple and
//! the current authority evidence; a caller cannot turn a valid token into a
//! different step by changing the gRPC request.

use std::time::Duration;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
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
pub(crate) const REQUIRED_PERMISSION: &str = "schedule:step";
const MAX_TOKEN_BYTES: usize = 16_384;
const MAX_FUTURE_ISSUED_SKEW: Duration = Duration::from_secs(60);

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
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded_key)
            .map_err(|_| anyhow::anyhow!("scheduled-step Control public key is not base64url"))?;
        let bytes: [u8; 32] = bytes.try_into().map_err(|_| {
            anyhow::anyhow!("scheduled-step Control public key must contain 32 bytes")
        })?;
        let key = VerifyingKey::from_bytes(&bytes)
            .map_err(|_| anyhow::anyhow!("scheduled-step Control public key is invalid"))?;
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
        let matches = claims.org_id == request.org_id
            && claims.space_ref == request.space_id
            && claims.subject_id == request.subject_id
            && claims.service_audience == SERVICE_AUDIENCE
            && claims.action_id == ACTION_ID
            && claims.action_schema_hash == ACTION_SCHEMA_HASH
            && claims.idempotency_key == request.idempotency_key
            && claims.payload_digest == payload_digest(&claims, request)
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

fn first_env(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    })
}

fn payload_digest(claims: &Claims, request: &ExecuteScheduledStepRequest) -> String {
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
        ("action_schema_hash", ACTION_SCHEMA_HASH),
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
    format!("sha256:{:x}", digest.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let mut claims = serde_json::json!({
            "decision_ref":"decision_1", "org_id":request.org_id, "space_ref":request.space_id,
            "subject_id":request.subject_id, "service_audience":SERVICE_AUDIENCE,
            "action_id":ACTION_ID, "action_schema_hash":ACTION_SCHEMA_HASH,
            "payload_digest":"", "idempotency_key":request.idempotency_key,
            "recipient_audience_ref":"aud_1", "recipient_audience_hash":"hash_1",
            "privacy_policy_ref":"privacy_1", "resource_authorization_ref":"resource_1",
            "authority_revision":1, "membership_revision":1, "privacy_revision":1,
            "recipient_audience_revision":1, "entitlement_revision":1,
            "permissions":[REQUIRED_PERMISSION], "zero_data_retention":false,
            "nonce":"nonce_1", "issued_at":now, "expires_at":now + chrono::Duration::minutes(2),
        });
        let parsed: Claims = serde_json::from_value(claims.clone()).expect("claims");
        claims["payload_digest"] = serde_json::Value::String(payload_digest(&parsed, request));
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
}
