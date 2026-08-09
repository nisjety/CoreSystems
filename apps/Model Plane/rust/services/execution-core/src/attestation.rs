//! Ed25519 provider-write attestation signer for execution-core.
//!
//! This is the `model-execution` counterpart to conversation-core-go's
//! `internal/attestation/signer.go`. Both sign the same wire contract
//! verified by integration-corev2
//! (`apps/Ingestion Plane/integration-corev2/internal/attestation/verifier.go`),
//! just under a different registered issuer — see that file's
//! `supportedIssuers` allow-list, which must list `model-execution` before
//! any key signed here can verify.
//!
//! # Canonicalization
//!
//! [`payload_sha256`] must byte-for-byte match what integration-corev2
//! independently recomputes server-side, in Go, from the same
//! org_id/connection_id/provider_key/operation/params/body. Go's
//! `encoding/json.Marshal`:
//! - preserves struct field declaration order (not alphabetical) for the six
//!   top-level fields — [`CanonicalPayload`]'s field order mirrors it exactly;
//! - sorts map keys alphabetically, recursively, at every nesting level
//!   inside `params`/`body`. This is free here: serde_json's `Value::Object`
//!   is `BTreeMap`-backed unless the `preserve_order` feature is enabled, and
//!   nothing in this Cargo workspace enables it (confirmed by grep before
//!   this was written — if that ever changes, this digest silently breaks,
//!   since Cargo unifies features workspace-wide);
//! - HTML-escapes `<`, `>`, `&`, U+2028, and U+2029 in every string value by
//!   default, which serde_json does not do and [`html_escape_like_go`]
//!   replicates by hand.
//!
//! Known limitation shared with the Go side, not introduced here: a JSON
//! number that round-trips through Go's `float64` may re-serialize with
//! different digits than serde_json's `Number` for the same input in rare
//! cases (e.g. very large integers). Every existing caller of this scheme
//! (conversation-core included) already carries this risk.
//!
//! # Claims payload is not canonicalized
//!
//! Only `payload_sha256` needs byte-exact cross-language agreement. The JWS
//! claims segment itself is parsed structurally by the Go verifier
//! (`json.Decoder` with `DisallowUnknownFields`), not byte-compared, so
//! [`Claims`] only needs matching field names and matching `omitempty`
//! semantics for `approval_id` (see `Verifier.validateClaims`'s
//! `fields["approval_id"]` presence check) — not a canonical encoding.

#![allow(clippy::missing_errors_doc, clippy::doc_markdown)]

use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use ed25519_dalek::{Signer as _, SigningKey};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const ISSUER_MODEL_EXECUTION: &str = "model-execution";
pub const AUDIENCE_INTEGRATION_CORE: &str = "integration-corev2";
pub const PRESENTER_MODEL_EXECUTION: &str = "model-execution";
pub const AUTHORIZATION_HUMAN_INTENT: &str = "human_intent";
pub const AUTHORIZATION_HUMAN_APPROVED_AI_ACTION: &str = "human_approved_ai_action";

const ATTESTATION_TYPE: &str = "verevon.provider-write-attestation+jwt";
const DEFAULT_TTL_SECONDS: i64 = 30;

#[derive(Serialize)]
struct Header<'a> {
    alg: &'a str,
    typ: &'a str,
    kid: &'a str,
}

/// The signed wire contract verified by integration-corev2. Field names and
/// `approval_id`'s conditional presence must match
/// `attestation.Claims` there exactly.
#[derive(Serialize)]
struct Claims {
    v: i64,
    iss: &'static str,
    aud: &'static str,
    presenter_service: &'static str,
    authorization_kind: String,
    authorization_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    approval_id: Option<String>,
    action_id: String,
    org_id: String,
    connection_id: String,
    provider_key: String,
    operation: String,
    actor_id: String,
    payload_sha256: String,
    idempotency_key: String,
    jti: String,
    iat: i64,
    nbf: i64,
    exp: i64,
}

/// The canonical hash input, field order fixed to match Go's struct
/// declaration order exactly. See the module doc for why this matters.
#[derive(Serialize)]
struct CanonicalPayload<'a> {
    org_id: &'a str,
    connection_id: &'a str,
    provider_key: &'a str,
    operation: &'a str,
    params: &'a Value,
    body: &'a Value,
}

/// The durable, exact provider effect execution-core has authorized. Mirrors
/// conversation-core-go's `attestation.Authorization`.
#[derive(Debug, Clone, Default)]
pub struct Authorization {
    pub authorization_kind: String,
    pub authorization_id: String,
    pub approval_id: String,
    pub action_id: String,
    pub org_id: String,
    pub connection_id: String,
    pub provider_key: String,
    pub operation: String,
    pub actor_id: String,
    pub payload_sha256: String,
    pub idempotency_key: String,
}

/// Signs short-lived, effect-bound provider-write proofs. The private key
/// never leaves this process; integration-corev2 holds only the matching
/// public key.
pub struct Signer {
    signing_key: SigningKey,
    key_id: String,
    ttl_seconds: i64,
}

impl Signer {
    /// `private_key_seed` is the raw 32-byte Ed25519 seed. This is
    /// execution-core's own key material end to end (generated by
    /// `scripts/bootstrap_runtime_environment.sh` and consumed only here), so
    /// it uses the plain ed25519-dalek seed representation rather than Go's
    /// 64-byte `seed || public-key` concatenation — nothing else needs to
    /// parse this specific key with Go's `crypto/ed25519.PrivateKey` type.
    pub fn new(private_key_seed: [u8; 32], key_id: &str) -> Result<Self, String> {
        let key_id = key_id.trim().to_owned();
        if !valid_key_id(&key_id) {
            return Err("provider-write attestation key id is invalid".to_owned());
        }
        Ok(Self {
            signing_key: SigningKey::from_bytes(&private_key_seed),
            key_id,
            ttl_seconds: DEFAULT_TTL_SECONDS,
        })
    }

    /// Reads `EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY`
    /// (standard-base64, 32-byte Ed25519 seed) and
    /// `EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_KEY_ID`. Returns `None` —
    /// not an error — when unconfigured, matching
    /// `IntegrationActionsClient::from_env`'s existing convention: absence
    /// surfaces as an honest tool error when a write is attempted, not a
    /// startup crash.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let encoded = std::env::var("EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())?;
        let key_id = std::env::var("EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_KEY_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())?;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded.trim())
            .ok()?;
        let seed: [u8; 32] = decoded.try_into().ok()?;
        Self::new(seed, &key_id).ok()
    }

    /// Returns a compact Ed25519 JWS with the exact protected header and
    /// effect-bound claims integration-corev2 requires.
    // Ownership is intentional: every real caller constructs an Authorization
    // literal solely to sign it once and then drops it (see
    // IntegrationActionsClient::execute_action), so taking it by value avoids
    // cloning each field that a `&Authorization` parameter would otherwise
    // force at the point the owned Claims are built.
    #[allow(clippy::needless_pass_by_value)]
    pub fn sign(&self, authorization: Authorization) -> Result<String, String> {
        let authorization = normalize(&authorization);
        validate(&authorization)?;

        let mut jti_bytes = [0u8; 16];
        getrandom(&mut jti_bytes);
        let now = now_unix();
        let claims = Claims {
            v: 1,
            iss: ISSUER_MODEL_EXECUTION,
            aud: AUDIENCE_INTEGRATION_CORE,
            presenter_service: PRESENTER_MODEL_EXECUTION,
            authorization_kind: authorization.authorization_kind,
            authorization_id: authorization.authorization_id,
            approval_id: if authorization.approval_id.is_empty() {
                None
            } else {
                Some(authorization.approval_id)
            },
            action_id: authorization.action_id,
            org_id: authorization.org_id,
            connection_id: authorization.connection_id,
            provider_key: authorization.provider_key,
            operation: authorization.operation,
            actor_id: authorization.actor_id,
            payload_sha256: authorization.payload_sha256,
            idempotency_key: authorization.idempotency_key,
            jti: base64_url(&jti_bytes),
            iat: now,
            nbf: now,
            exp: now + self.ttl_seconds,
        };
        let header = Header {
            alg: "EdDSA",
            typ: ATTESTATION_TYPE,
            kid: &self.key_id,
        };
        let header_json = serde_json::to_vec(&header)
            .map_err(|e| format!("encode provider-write attestation header: {e}"))?;
        let claims_json = serde_json::to_vec(&claims)
            .map_err(|e| format!("encode provider-write attestation claims: {e}"))?;
        let protected = format!("{}.{}", base64_url(&header_json), base64_url(&claims_json));
        let signature = self.signing_key.sign(protected.as_bytes());
        Ok(format!("{protected}.{}", base64_url(&signature.to_bytes())))
    }
}

/// Computes the digest embedded in `payload_sha256` and independently
/// recomputed by integration-corev2 from the same fields. See the module doc
/// for the exact canonicalization this must replicate.
pub fn payload_sha256(
    org_id: &str,
    connection_id: &str,
    provider_key: &str,
    operation: &str,
    params: &Value,
    body: &Value,
) -> Result<String, String> {
    let canonical = CanonicalPayload {
        org_id: org_id.trim(),
        connection_id: connection_id.trim(),
        provider_key: provider_key.trim(),
        operation: operation.trim(),
        params,
        body,
    };
    let mut encoded = serde_json::to_vec(&canonical)
        .map_err(|e| format!("marshal provider-write payload: {e}"))?;
    html_escape_like_go(&mut encoded);
    Ok(to_lower_hex(&Sha256::digest(&encoded)))
}

fn normalize(value: &Authorization) -> Authorization {
    Authorization {
        authorization_kind: value.authorization_kind.trim().to_owned(),
        authorization_id: value.authorization_id.trim().to_owned(),
        approval_id: value.approval_id.trim().to_owned(),
        action_id: value.action_id.trim().to_owned(),
        org_id: value.org_id.trim().to_owned(),
        connection_id: value.connection_id.trim().to_owned(),
        provider_key: value.provider_key.trim().to_owned(),
        operation: value.operation.trim().to_owned(),
        actor_id: value.actor_id.trim().to_owned(),
        payload_sha256: value.payload_sha256.trim().to_owned(),
        idempotency_key: value.idempotency_key.trim().to_owned(),
    }
}

fn validate(value: &Authorization) -> Result<(), String> {
    for (name, field) in [
        ("authorization_id", &value.authorization_id),
        ("action_id", &value.action_id),
        ("org_id", &value.org_id),
        ("connection_id", &value.connection_id),
        ("provider_key", &value.provider_key),
        ("operation", &value.operation),
        ("actor_id", &value.actor_id),
        ("idempotency_key", &value.idempotency_key),
    ] {
        if field.is_empty() {
            return Err(format!("provider-write attestation {name} is required"));
        }
    }
    if !is_lower_hex_sha256(&value.payload_sha256) {
        return Err(
            "provider-write attestation payload_sha256 must be lowercase hexadecimal SHA-256"
                .to_owned(),
        );
    }
    match value.authorization_kind.as_str() {
        AUTHORIZATION_HUMAN_INTENT => {
            if !value.approval_id.is_empty() || value.action_id != value.authorization_id {
                return Err(
                    "human intent must omit approval_id and bind action_id to authorization_id"
                        .to_owned(),
                );
            }
        }
        AUTHORIZATION_HUMAN_APPROVED_AI_ACTION => {
            if value.approval_id.is_empty() || value.approval_id != value.action_id {
                return Err(
                    "approved AI action must bind matching approval_id and action_id".to_owned(),
                );
            }
        }
        _ => return Err("provider-write attestation authorization_kind is invalid".to_owned()),
    }
    Ok(())
}

/// Mirrors Go's `^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$` key-id pattern.
fn valid_key_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 {
        return false;
    }
    if !bytes[0].is_ascii_alphanumeric() {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|&b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

fn is_lower_hex_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Post-serialization pass mirroring Go's default `encoding/json` HTML
/// escaping: replace `<`, `>`, `&`, U+2028, and U+2029 with their `\uXXXX`
/// escapes. Safe to apply to the whole buffer, not just inside strings,
/// because none of these bytes ever appear as bare JSON structural syntax
/// from serde_json's encoder — Go's own implementation works the same way,
/// scanning the fully-encoded buffer rather than each string individually.
fn html_escape_like_go(buf: &mut Vec<u8>) {
    let mut out = Vec::with_capacity(buf.len());
    let mut i = 0;
    while i < buf.len() {
        match buf[i] {
            b'<' => {
                out.extend_from_slice(b"\\u003c");
                i += 1;
            }
            b'>' => {
                out.extend_from_slice(b"\\u003e");
                i += 1;
            }
            b'&' => {
                out.extend_from_slice(b"\\u0026");
                i += 1;
            }
            0xE2 if buf[i + 1..].starts_with(&[0x80, 0xA8]) => {
                out.extend_from_slice(b"\\u2028");
                i += 3;
            }
            0xE2 if buf[i + 1..].starts_with(&[0x80, 0xA9]) => {
                out.extend_from_slice(b"\\u2029");
                i += 3;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    *buf = out;
}

fn to_lower_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn base64_url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn now_unix() -> i64 {
    // Unix seconds fit in i64 until year 292,277,026,596 — an intentional,
    // domain-safe cast, not a truncation risk.
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().cast_signed())
        .unwrap_or(0)
}

/// `OsRng::fill_bytes` is infallible in this API — no `Result` to propagate.
fn getrandom(buf: &mut [u8]) {
    use rand::RngCore as _;
    rand::rngs::OsRng.fill_bytes(buf);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact test vector from
    /// `apps/Ingestion Plane/integration-corev2/testdata/provider_write_attestation_v1.json`,
    /// hardcoded rather than read from disk (this crate has no reason to
    /// depend on integration-corev2's working directory at test time). This
    /// is cross-language ground truth: `canonical_payload_json` and
    /// `payload_sha256` were produced by Go's `encoding/json` +
    /// `crypto/sha256`, independently of anything in this crate. If this test
    /// passes, this module's canonicalization matches Go's exactly for this
    /// input.
    const FIXTURE_ORG_ID: &str = "org-1";
    const FIXTURE_CONNECTION_ID: &str = "conn-1";
    const FIXTURE_PROVIDER_KEY: &str = "whatsapp";
    const FIXTURE_OPERATION: &str = "whatsapp.messages.send";
    const FIXTURE_CANONICAL_JSON: &str = "{\"org_id\":\"org-1\",\"connection_id\":\"conn-1\",\"provider_key\":\"whatsapp\",\"operation\":\"whatsapp.messages.send\",\"params\":{\"phoneNumberId\":\"phone-1\"},\"body\":{\"text\":{\"body\":\"Approved reply\"},\"to\":\"15550001\",\"type\":\"text\"}}";
    const FIXTURE_PAYLOAD_SHA256: &str =
        "99116abef35c712ba306b35113a4a27f0c6ff6d586ec15208de34ef388a8471b";

    fn fixture_params() -> Value {
        serde_json::json!({"phoneNumberId": "phone-1"})
    }

    fn fixture_body() -> Value {
        serde_json::json!({
            "to": "15550001",
            "type": "text",
            "text": {"body": "Approved reply"},
        })
    }

    #[test]
    fn payload_sha256_matches_the_go_fixed_vector() {
        let digest = payload_sha256(
            FIXTURE_ORG_ID,
            FIXTURE_CONNECTION_ID,
            FIXTURE_PROVIDER_KEY,
            FIXTURE_OPERATION,
            &fixture_params(),
            &fixture_body(),
        )
        .expect("payload_sha256 should succeed");
        assert_eq!(
            digest, FIXTURE_PAYLOAD_SHA256,
            "digest drifted from the Go-produced fixed vector"
        );
    }

    #[test]
    fn canonical_json_matches_the_go_fixed_vector_byte_for_byte() {
        let canonical = CanonicalPayload {
            org_id: FIXTURE_ORG_ID,
            connection_id: FIXTURE_CONNECTION_ID,
            provider_key: FIXTURE_PROVIDER_KEY,
            operation: FIXTURE_OPERATION,
            params: &fixture_params(),
            body: &fixture_body(),
        };
        let encoded = serde_json::to_string(&canonical).expect("serialize canonical payload");
        assert_eq!(
            encoded, FIXTURE_CANONICAL_JSON,
            "canonical JSON drifted from the Go-produced fixed vector"
        );
    }

    #[test]
    fn html_escape_matches_go_encoding_json_defaults() {
        let mut buf = "\"<b>a & b</b> \u{2028}\u{2029}\"".as_bytes().to_vec();
        html_escape_like_go(&mut buf);
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "\"\\u003cb\\u003ea \\u0026 b\\u003c/b\\u003e \\u2028\\u2029\""
        );
    }

    #[test]
    fn payload_sha256_escapes_html_sensitive_characters_like_go() {
        // Not covered by the fixture above (which contains none of these
        // characters) — this proves the escaping path independently.
        let body = serde_json::json!({"text": "<b>Tom & Jerry</b>"});
        let digest = payload_sha256(
            "org-1",
            "conn-1",
            "whatsapp",
            "whatsapp.messages.send",
            &Value::Null,
            &body,
        )
        .expect("payload_sha256 should succeed");
        // The digest must be over the HTML-escaped bytes, not the literal
        // ones: if escaping were skipped, this would equal
        // sha256({"...":"...","body":{"text":"<b>Tom & Jerry</b>"},"params":null,...}) instead.
        let mut unescaped = serde_json::to_vec(&CanonicalPayload {
            org_id: "org-1",
            connection_id: "conn-1",
            provider_key: "whatsapp",
            operation: "whatsapp.messages.send",
            params: &Value::Null,
            body: &body,
        })
        .unwrap();
        let unescaped_digest = to_lower_hex(&Sha256::digest(&unescaped));
        assert_ne!(
            digest, unescaped_digest,
            "escaping had no effect — this test's premise is broken"
        );
        html_escape_like_go(&mut unescaped);
        let reescaped_digest = to_lower_hex(&Sha256::digest(&unescaped));
        assert_eq!(digest, reescaped_digest);
    }

    #[test]
    fn sign_produces_a_verifiable_three_segment_jws() {
        let signer = Signer::new([7u8; 32], "model-execution-write-test")
            .expect("Signer::new should accept a valid key id");
        let token = signer
            .sign(Authorization {
                authorization_kind: AUTHORIZATION_HUMAN_APPROVED_AI_ACTION.to_owned(),
                authorization_id: "approval-1".to_owned(),
                approval_id: "approval-1".to_owned(),
                action_id: "approval-1".to_owned(),
                org_id: "org-1".to_owned(),
                connection_id: "conn-1".to_owned(),
                provider_key: "linkedin".to_owned(),
                operation: "linkedin.posts.create".to_owned(),
                actor_id: "user-1".to_owned(),
                payload_sha256: FIXTURE_PAYLOAD_SHA256.to_owned(),
                idempotency_key: "approval-1".to_owned(),
            })
            .expect("sign should succeed for a valid authorization");
        let segments: Vec<&str> = token.split('.').collect();
        assert_eq!(segments.len(), 3);

        let header_json = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(segments[0])
            .unwrap();
        let header: Value = serde_json::from_slice(&header_json).unwrap();
        assert_eq!(header["alg"], "EdDSA");
        assert_eq!(header["typ"], ATTESTATION_TYPE);
        assert_eq!(header["kid"], "model-execution-write-test");

        let claims_json = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(segments[1])
            .unwrap();
        let claims: Value = serde_json::from_slice(&claims_json).unwrap();
        assert_eq!(claims["iss"], ISSUER_MODEL_EXECUTION);
        assert_eq!(claims["aud"], AUDIENCE_INTEGRATION_CORE);
        assert_eq!(claims["presenter_service"], PRESENTER_MODEL_EXECUTION);
        assert_eq!(claims["approval_id"], "approval-1");

        let signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let verifying_key = signing_key.verifying_key();
        let signature_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(segments[2])
            .unwrap();
        let signature = ed25519_dalek::Signature::from_slice(&signature_bytes).unwrap();
        let signing_input = format!("{}.{}", segments[0], segments[1]);
        ed25519_dalek::Verifier::verify(&verifying_key, signing_input.as_bytes(), &signature)
            .expect("signature must verify against the corresponding public key");
    }

    #[test]
    fn sign_omits_approval_id_for_human_intent() {
        let signer = Signer::new([9u8; 32], "model-execution-write-test").unwrap();
        let token = signer
            .sign(Authorization {
                authorization_kind: AUTHORIZATION_HUMAN_INTENT.to_owned(),
                authorization_id: "intent-1".to_owned(),
                approval_id: String::new(),
                action_id: "intent-1".to_owned(),
                org_id: "org-1".to_owned(),
                connection_id: "conn-1".to_owned(),
                provider_key: "linkedin".to_owned(),
                operation: "linkedin.posts.create".to_owned(),
                actor_id: "user-1".to_owned(),
                payload_sha256: FIXTURE_PAYLOAD_SHA256.to_owned(),
                idempotency_key: "intent-1".to_owned(),
            })
            .expect("sign should succeed for a valid human_intent authorization");
        let segments: Vec<&str> = token.split('.').collect();
        let claims_json = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(segments[1])
            .unwrap();
        let claims: Value = serde_json::from_slice(&claims_json).unwrap();
        assert!(
            claims.get("approval_id").is_none(),
            "human_intent claims must omit approval_id entirely, not just leave it empty: {claims:?}"
        );
    }

    #[test]
    fn sign_rejects_approved_ai_action_without_matching_approval_id() {
        let signer = Signer::new([3u8; 32], "model-execution-write-test").unwrap();
        let result = signer.sign(Authorization {
            authorization_kind: AUTHORIZATION_HUMAN_APPROVED_AI_ACTION.to_owned(),
            authorization_id: "approval-1".to_owned(),
            approval_id: String::new(),
            action_id: "approval-1".to_owned(),
            org_id: "org-1".to_owned(),
            connection_id: "conn-1".to_owned(),
            provider_key: "linkedin".to_owned(),
            operation: "linkedin.posts.create".to_owned(),
            actor_id: "user-1".to_owned(),
            payload_sha256: FIXTURE_PAYLOAD_SHA256.to_owned(),
            idempotency_key: "approval-1".to_owned(),
        });
        assert!(
            result.is_err(),
            "must fail closed without a matching approval_id"
        );
    }
}
