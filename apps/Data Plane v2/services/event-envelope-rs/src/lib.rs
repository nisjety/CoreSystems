//! Signed, producer-scoped envelopes for Data Plane asynchronous events.
//!
//! The payload is encoded exactly once and its SHA-256 digest is covered by an
//! RS256 JWS. Consumers verify the JWS before decoding domain data, then pin
//! tenant, optional user, event subject, scope and ZDR posture to the claims.

use std::collections::HashMap;
use std::sync::Mutex;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Utc;
use jsonwebtoken::{decode, decode_header, encode, Algorithm, DecodingKey, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const TOKEN_TTL_SECONDS: i64 = 120;
const MAX_TOKEN_TTL_SECONDS: i64 = 300;
const CLOCK_SKEW_SECONDS: i64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventClaims {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    pub principal_type: String,
    pub org_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    pub scopes: Vec<String>,
    pub zdr: bool,
    pub event_type: String,
    pub payload_sha256: String,
    pub jti: String,
    pub iat: i64,
    pub nbf: i64,
    pub exp: i64,
}

#[derive(Debug, Clone)]
pub struct VerifiedEvent {
    pub claims: EventClaims,
    pub payload: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireEnvelope {
    authorization: String,
    data: String,
}

#[derive(Debug, thiserror::Error)]
pub enum EnvelopeError {
    #[error("invalid signed event configuration")]
    InvalidConfiguration,
    #[error("invalid signed event envelope")]
    InvalidEnvelope,
    #[error("signed event replay rejected")]
    Replay,
    #[error("signed event replay cache unavailable")]
    ReplayCacheUnavailable,
}

pub struct EventSigner {
    key: EncodingKey,
    issuer: String,
    key_id: String,
    audience: String,
    scope: String,
}

impl EventSigner {
    pub fn from_rsa_pem(
        pem: &[u8],
        issuer: &str,
        key_id: &str,
        audience: &str,
        scope: &str,
    ) -> Result<Self, EnvelopeError> {
        validate_contract(issuer, key_id, audience, scope)?;
        let key =
            EncodingKey::from_rsa_pem(pem).map_err(|_| EnvelopeError::InvalidConfiguration)?;
        Ok(Self {
            key,
            issuer: issuer.to_owned(),
            key_id: key_id.to_owned(),
            audience: audience.to_owned(),
            scope: scope.to_owned(),
        })
    }

    pub fn sign(
        &self,
        event_type: &str,
        org_id: &str,
        user_id: Option<&str>,
        zdr: bool,
        payload: &[u8],
    ) -> Result<Vec<u8>, EnvelopeError> {
        if !subject_allowed_for_scope(&self.scope, event_type)
            || (self.scope == "events:wiki:publish" && zdr)
        {
            return Err(EnvelopeError::InvalidEnvelope);
        }
        let payload_json = validate_boundary_fields(event_type, org_id, user_id, zdr, payload)?;
        validate_payload_claims(&payload_json, org_id, user_id, zdr)?;

        let now = Utc::now().timestamp();
        let claims = EventClaims {
            iss: self.issuer.clone(),
            sub: self.issuer.clone(),
            aud: self.audience.clone(),
            principal_type: "service".to_owned(),
            org_id: org_id.to_owned(),
            user_id: user_id.map(str::to_owned),
            scopes: vec![self.scope.clone()],
            zdr,
            event_type: event_type.to_owned(),
            payload_sha256: payload_digest(payload),
            jti: Uuid::new_v4().to_string(),
            iat: now,
            nbf: now,
            exp: now + TOKEN_TTL_SECONDS,
        };
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(self.key_id.clone());
        let token =
            encode(&header, &claims, &self.key).map_err(|_| EnvelopeError::InvalidEnvelope)?;
        serde_json::to_vec(&WireEnvelope {
            authorization: format!("Bearer {token}"),
            data: URL_SAFE_NO_PAD.encode(payload),
        })
        .map_err(|_| EnvelopeError::InvalidEnvelope)
    }
}

pub struct EventVerifier {
    key: DecodingKey,
    expected_issuer: String,
    expected_key_id: String,
    audience: String,
    required_scope: String,
    replay_capacity: usize,
    replay: Mutex<HashMap<String, i64>>,
}

impl EventVerifier {
    pub fn from_rsa_pem(
        pem: &[u8],
        expected_issuer: &str,
        expected_key_id: &str,
        audience: &str,
        required_scope: &str,
        replay_capacity: usize,
    ) -> Result<Self, EnvelopeError> {
        validate_contract(expected_issuer, expected_key_id, audience, required_scope)?;
        if replay_capacity == 0 {
            return Err(EnvelopeError::InvalidConfiguration);
        }
        let key =
            DecodingKey::from_rsa_pem(pem).map_err(|_| EnvelopeError::InvalidConfiguration)?;
        Ok(Self {
            key,
            expected_issuer: expected_issuer.to_owned(),
            expected_key_id: expected_key_id.to_owned(),
            audience: audience.to_owned(),
            required_scope: required_scope.to_owned(),
            replay_capacity,
            replay: Mutex::new(HashMap::new()),
        })
    }

    pub fn verify(&self, subject: &str, envelope: &[u8]) -> Result<VerifiedEvent, EnvelopeError> {
        self.verify_with_replay_policy(subject, envelope, false)
    }

    /// Verifies an authoritative JetStream redelivery while still rejecting a
    /// duplicate first delivery. Callers must derive this choice from broker
    /// delivery metadata, never from an event-controlled field.
    pub fn verify_redelivery(
        &self,
        subject: &str,
        envelope: &[u8],
    ) -> Result<VerifiedEvent, EnvelopeError> {
        self.verify_with_replay_policy(subject, envelope, true)
    }

    fn verify_with_replay_policy(
        &self,
        subject: &str,
        envelope: &[u8],
        allow_known_replay: bool,
    ) -> Result<VerifiedEvent, EnvelopeError> {
        if !subject_allowed_for_scope(&self.required_scope, subject) {
            return Err(EnvelopeError::InvalidEnvelope);
        }
        let wire: WireEnvelope =
            serde_json::from_slice(envelope).map_err(|_| EnvelopeError::InvalidEnvelope)?;
        let token = wire
            .authorization
            .strip_prefix("Bearer ")
            .filter(|value| !value.is_empty())
            .ok_or(EnvelopeError::InvalidEnvelope)?;
        let header = decode_header(token).map_err(|_| EnvelopeError::InvalidEnvelope)?;
        if header.alg != Algorithm::RS256
            || header.kid.as_deref() != Some(self.expected_key_id.as_str())
        {
            return Err(EnvelopeError::InvalidEnvelope);
        }

        let mut validation = jsonwebtoken::Validation::new(Algorithm::RS256);
        validation.set_audience(&[self.audience.as_str()]);
        validation.set_issuer(&[self.expected_issuer.as_str()]);
        validation.leeway = CLOCK_SKEW_SECONDS as u64;
        validation.validate_nbf = true;
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub", "nbf", "iat", "jti"]);
        let claims = decode::<EventClaims>(token, &self.key, &validation)
            .map_err(|_| EnvelopeError::InvalidEnvelope)?
            .claims;

        let payload = URL_SAFE_NO_PAD
            .decode(wire.data)
            .map_err(|_| EnvelopeError::InvalidEnvelope)?;
        let payload_json = validate_boundary_fields(
            subject,
            &claims.org_id,
            claims.user_id.as_deref(),
            claims.zdr,
            &payload,
        )?;
        validate_payload_claims(
            &payload_json,
            &claims.org_id,
            claims.user_id.as_deref(),
            claims.zdr,
        )?;
        let now = Utc::now().timestamp();
        if claims.iss != self.expected_issuer
            || claims.sub != self.expected_issuer
            || claims.principal_type != "service"
            || claims.event_type != subject
            || !claims
                .scopes
                .iter()
                .any(|scope| scope == &self.required_scope)
            || claims.scopes.iter().any(|scope| scope.trim().is_empty())
            || claims.payload_sha256 != payload_digest(&payload)
            || claims.jti.trim().is_empty()
            || claims.iat > now + CLOCK_SKEW_SECONDS
            || claims.nbf < claims.iat
            || claims.exp <= claims.nbf
            || claims.exp - claims.iat > MAX_TOKEN_TTL_SECONDS
            || (self.required_scope == "events:wiki:publish" && claims.zdr)
        {
            return Err(EnvelopeError::InvalidEnvelope);
        }

        self.consume_replay_id(&claims.jti, claims.exp, now, allow_known_replay)?;
        Ok(VerifiedEvent { claims, payload })
    }

    fn consume_replay_id(
        &self,
        jti: &str,
        exp: i64,
        now: i64,
        allow_known_replay: bool,
    ) -> Result<(), EnvelopeError> {
        let mut replay = self
            .replay
            .lock()
            .map_err(|_| EnvelopeError::ReplayCacheUnavailable)?;
        replay.retain(|_, expires_at| *expires_at + CLOCK_SKEW_SECONDS >= now);
        if replay.contains_key(jti) {
            if allow_known_replay {
                return Ok(());
            }
            return Err(EnvelopeError::Replay);
        }
        if replay.len() >= self.replay_capacity {
            return Err(EnvelopeError::ReplayCacheUnavailable);
        }
        replay.insert(jti.to_owned(), exp);
        Ok(())
    }
}

fn validate_contract(
    issuer: &str,
    key_id: &str,
    audience: &str,
    scope: &str,
) -> Result<(), EnvelopeError> {
    if issuer
        .strip_prefix("service:")
        .is_none_or(|service| service.is_empty())
        || [key_id, audience, scope]
            .iter()
            .any(|value| value.trim().is_empty())
    {
        return Err(EnvelopeError::InvalidConfiguration);
    }
    Ok(())
}

fn validate_boundary_fields(
    event_type: &str,
    org_id: &str,
    user_id: Option<&str>,
    _zdr: bool,
    payload: &[u8],
) -> Result<Value, EnvelopeError> {
    if event_type.trim().is_empty()
        || org_id.trim().is_empty()
        || user_id.is_some_and(|user| user.trim().is_empty())
        || payload.is_empty()
    {
        return Err(EnvelopeError::InvalidEnvelope);
    }
    let value: Value =
        serde_json::from_slice(payload).map_err(|_| EnvelopeError::InvalidEnvelope)?;
    if !value.is_object() {
        return Err(EnvelopeError::InvalidEnvelope);
    }
    Ok(value)
}

fn validate_payload_claims(
    payload: &Value,
    org_id: &str,
    user_id: Option<&str>,
    zdr: bool,
) -> Result<(), EnvelopeError> {
    if payload.get("org_id").and_then(Value::as_str) != Some(org_id) {
        return Err(EnvelopeError::InvalidEnvelope);
    }
    if let Some(payload_user) = payload.get("user_id") {
        if payload_user.as_str() != user_id {
            return Err(EnvelopeError::InvalidEnvelope);
        }
    }
    if let Some(payload_zdr) = payload.get("zdr") {
        if payload_zdr.as_bool() != Some(zdr) {
            return Err(EnvelopeError::InvalidEnvelope);
        }
    }
    Ok(())
}

fn payload_digest(payload: &[u8]) -> String {
    let digest = Sha256::digest(payload);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn subject_allowed_for_scope(scope: &str, subject: &str) -> bool {
    match scope {
        "events:documents:publish" => matches!(
            subject,
            "dataplane.documents.created"
                | "dataplane.documents.updated"
                | "dataplane.documents.deleted"
                | "dataplane.source_objects.changed"
                | "dataplane.source_objects.deleted"
        ),
        "events:index:publish" => matches!(
            subject,
            "dataplane.knowledge.units.created"
                | "dataplane.knowledge.units.deleted"
                | "dataplane.dlq.index-engine"
        ),
        "events:embedding:publish" => matches!(
            subject,
            "dataplane.documents.indexed"
                | "dataplane.cost.ledger"
                | "dataplane.dlq.embedding-engine"
        ),
        "events:wiki:publish" => subject == "dataplane.wiki.version.published",
        _ => false,
    }
}
