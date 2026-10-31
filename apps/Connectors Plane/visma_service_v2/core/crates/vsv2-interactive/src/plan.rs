use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;
use vsv2_common::TenantId;

use crate::error::{InteractiveError, Result};
use crate::request::WriteRequest;

type HmacSha256 = Hmac<Sha256>;

/// What a dry-run produced and what execute will send. Serializable so a caller (an MCP tool, a
/// UI) can hold it between the two steps; tampering is caught by the [`ConfirmSigner`] token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub plan_id: Uuid,
    pub operation: String,
    pub tenant: TenantId,
    pub request: WriteRequest,
    /// Human-readable one-liner of the effect, for the approver.
    pub summary: String,
    pub created_at: DateTime<Utc>,
}

impl Plan {
    /// SHA-256 of the canonical request body.
    pub fn payload_hash(&self) -> String {
        hex::encode(Sha256::digest(
            canonical_json(&self.request.body).as_bytes(),
        ))
    }

    /// Every field, length-prefixed (`len:value`) so no value can bleed into its neighbour. The
    /// summary is included: it is what the approver reads, so it must not be alterable.
    fn signing_input(&self) -> String {
        let fields = [
            self.plan_id.to_string(),
            self.operation.clone(),
            self.tenant.to_string(),
            self.created_at.to_rfc3339(),
            self.request.target.as_str().to_string(),
            self.request.method.as_str().to_string(),
            self.request.path.clone(),
            self.request.if_match.clone().unwrap_or_default(),
            self.summary.clone(),
            canonical_json(&self.request.body),
        ];
        fields.iter().map(|f| format!("{}:{f}", f.len())).collect()
    }
}

/// HMAC over every field of a plan, so execute can prove the plan is the one that was reviewed.
#[derive(Clone)]
pub struct ConfirmSigner {
    secret: Vec<u8>,
}

impl std::fmt::Debug for ConfirmSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ConfirmSigner(..)")
    }
}

impl ConfirmSigner {
    pub fn new(secret: impl Into<Vec<u8>>) -> Result<Self> {
        let secret = secret.into();
        if secret.len() < 16 {
            return Err(InteractiveError::InvalidInput(
                "confirm secret must be at least 16 bytes".into(),
            ));
        }
        Ok(Self { secret })
    }

    pub fn from_env() -> Result<Self> {
        let s = std::env::var("VSV2_WRITE_CONFIRM_SECRET")
            .map_err(|_| InteractiveError::Denied("VSV2_WRITE_CONFIRM_SECRET is not set".into()))?;
        Self::new(s.into_bytes())
    }

    fn mac(&self, plan: &Plan) -> HmacSha256 {
        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("hmac accepts any key size");
        mac.update(plan.signing_input().as_bytes());
        mac
    }

    pub fn sign(&self, plan: &Plan) -> String {
        hex::encode(self.mac(plan).finalize().into_bytes())
    }

    /// Constant-time check.
    pub fn verify(&self, plan: &Plan, token: &str) -> bool {
        hex::decode(token)
            .map(|raw| self.mac(plan).verify_slice(&raw).is_ok())
            .unwrap_or(false)
    }
}

/// Key-sorted JSON, so hashes do not depend on object insertion order.
pub fn canonical_json(v: &Value) -> String {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{}:{}", Value::String(k.clone()), canonical_json(&m[k])))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(a) => {
            let parts: Vec<String> = a.iter().map(canonical_json).collect();
            format!("[{}]", parts.join(","))
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{WriteMethod, WriteTarget};
    use serde_json::json;

    fn plan(body: Value) -> Plan {
        Plan {
            plan_id: Uuid::nil(),
            operation: "op".into(),
            tenant: TenantId::new("t1"),
            request: WriteRequest {
                target: WriteTarget::SalesOrder,
                method: WriteMethod::Put,
                path: "/v3/x".into(),
                body,
                if_match: Some("\"etag1\"".into()),
            },
            summary: "s".into(),
            created_at: DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        }
    }

    fn signer() -> ConfirmSigner {
        ConfirmSigner::new(b"0123456789abcdef-test-secret".to_vec()).unwrap()
    }

    #[test]
    fn canonical_json_ignores_key_order() {
        let a = json!({"b": 1, "a": {"y": [2, 1], "x": null}});
        let b = json!({"a": {"x": null, "y": [2, 1]}, "b": 1});
        assert_eq!(canonical_json(&a), canonical_json(&b));
        assert_ne!(
            canonical_json(&json!([1, 2])),
            canonical_json(&json!([2, 1]))
        );
    }

    #[test]
    fn token_verifies_for_the_same_plan_regardless_of_key_order() {
        let s = signer();
        let token = s.sign(&plan(json!({"a": 1, "b": 2})));
        assert!(s.verify(&plan(json!({"b": 2, "a": 1})), &token));
    }

    #[test]
    fn any_changed_field_invalidates_the_token() {
        let s = signer();
        let base = plan(json!({"qty": 1}));
        let token = s.sign(&base);

        let mut p = base.clone();
        p.request.body = json!({"qty": 2});
        assert!(!s.verify(&p, &token));
        let mut p = base.clone();
        p.request.path = "/v3/y".into();
        assert!(!s.verify(&p, &token));
        let mut p = base.clone();
        p.tenant = TenantId::new("t2");
        assert!(!s.verify(&p, &token));
        let mut p = base.clone();
        p.request.if_match = None;
        assert!(!s.verify(&p, &token));
        let mut p = base.clone();
        p.summary = "a different summary".into();
        assert!(
            !s.verify(&p, &token),
            "the approver-visible summary is bound too"
        );
        let mut p = base;
        p.request.method = WriteMethod::Post;
        assert!(!s.verify(&p, &token));
    }

    #[test]
    fn garbage_or_foreign_tokens_are_rejected() {
        let s = signer();
        let p = plan(json!({}));
        assert!(!s.verify(&p, "not-hex"));
        assert!(!s.verify(&p, ""));
        let other = ConfirmSigner::new(b"another-secret-0123456789".to_vec()).unwrap();
        assert!(!s.verify(&p, &other.sign(&p)));
    }

    #[test]
    fn field_boundaries_cannot_be_shifted_between_neighbours() {
        // Without length prefixes these two plans would serialise to the same byte string.
        let s = signer();
        let mut a = plan(json!({}));
        a.request.path = "/v3/x".into();
        a.request.if_match = Some("|y".into());
        let mut b = a.clone();
        b.request.path = "/v3/x|".into();
        b.request.if_match = Some("y".into());
        assert!(!s.verify(&b, &s.sign(&a)));
    }

    #[test]
    fn short_secrets_are_refused() {
        assert!(ConfirmSigner::new(b"short".to_vec()).is_err());
    }
}
