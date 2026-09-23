//! HMAC delegation signing for conversation-core's Verevon read lane.
//!
//! conversation-core does not accept the shared internal API key that
//! insight-core and social-core take — it refuses it explicitly, and has a test
//! saying so. Every route there is bound to a named service principal whose
//! requests are HMAC-signed over a canonical description of the request, so a
//! captured header set cannot be replayed against a different method, path,
//! organization, or body.
//!
//! This module signs as `model-gateway`, whose principal on the other side
//! reaches exactly two org-scoped GETs (`/internal/v1/verevon/conversations`
//! and `.../{id}`). It is deliberately not the `verevon-gateway` principal: that
//! one carries every write in the conversation API, and a model-driven turn
//! must not be one route registration away from sending mail.
//!
//! The canonical form is conversation-core's `delegation.Canonical` v2, field
//! for field. The two must agree exactly — a mismatch is a 401 with no
//! diagnostic, since the verifier cannot say which field differed without
//! becoming an oracle.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, SecondsFormat, Utc};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};

/// The principal name conversation-core knows this service by.
pub const SERVICE_ID: &str = "model-gateway";

/// conversation-core's delegation audience. A signature is bound to it, so a
/// token shared with another core could not be replayed here.
const AUDIENCE: &str = "conversation-core";

static NONCE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// One signed request's headers, in the order conversation-core reads them.
pub struct DelegationHeaders {
    pub service_id: &'static str,
    pub user_id: String,
    pub org_id: String,
    pub timestamp: String,
    pub nonce: String,
    pub body_sha256: String,
    pub signature: String,
}

/// A nonce conversation-core's replay cache will accept: `[A-Za-z0-9_-]{16,128}`.
///
/// Uniqueness comes from time, process and a counter together — time alone
/// repeats across two requests in the same nanosecond bucket, and a counter
/// alone repeats after a restart, which is exactly when a replay window opens.
fn nonce(now: DateTime<Utc>) -> String {
    let counter = NONCE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let seed = format!(
        "{}:{}:{counter}",
        now.timestamp_nanos_opt().unwrap_or_default(),
        std::process::id()
    );
    URL_SAFE_NO_PAD.encode(Sha256::digest(seed.as_bytes()))
}

/// conversation-core `delegation.Canonical`, v2.
///
/// Every field is part of the signature, including the empty ones: the role is
/// empty for this principal (the Model Plane has no verified role to assert),
/// and it still occupies its line, because both sides join a fixed-length list.
fn canonical(
    timestamp: &str,
    nonce: &str,
    method: &str,
    uri: &str,
    user_id: &str,
    org_id: &str,
    role: &str,
    body_sha256: &str,
) -> String {
    [
        "v2",
        SERVICE_ID,
        AUDIENCE,
        timestamp,
        nonce,
        method,
        uri,
        user_id,
        org_id,
        role,
        body_sha256,
    ]
    .join("\n")
}

/// Sign one GET of `uri` (path plus query, exactly as it goes on the wire).
///
/// `uri` must be the request-URI conversation-core will see. Signing a path
/// while sending a different query string is the failure this signature exists
/// to catch, so callers build the query once and pass the joined result.
pub fn sign_get(
    service_token: &str,
    uri: &str,
    user_id: &str,
    org_id: &str,
    now: DateTime<Utc>,
) -> DelegationHeaders {
    // A GET has no body, and conversation-core still hashes one: the digest of
    // the empty byte string, not an empty header.
    let body_sha256 = URL_SAFE_NO_PAD.encode(Sha256::digest([]));
    let timestamp = now.to_rfc3339_opts(SecondsFormat::Secs, false);
    let nonce = nonce(now);
    let user_id = user_id.trim().to_owned();
    let org_id = org_id.trim().to_owned();
    let canonical = canonical(
        &timestamp,
        &nonce,
        "GET",
        uri,
        &user_id,
        &org_id,
        "",
        &body_sha256,
    );

    let mut mac = <Hmac<Sha256>>::new_from_slice(service_token.as_bytes())
        .expect("HMAC accepts a key of any length");
    mac.update(canonical.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());

    DelegationHeaders {
        service_id: SERVICE_ID,
        user_id,
        org_id,
        timestamp,
        nonce,
        body_sha256,
        signature,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "model-gateway-test-secret-at-least-32b";

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("valid timestamp")
    }

    /// The canonical string is the contract with conversation-core's Go
    /// verifier. Pinning it here means a refactor that reorders or drops a
    /// field fails in CI rather than as an undiagnosable 401 in production.
    #[test]
    fn canonical_matches_conversation_core_field_order() {
        let built = canonical(
            "2026-09-15T00:00:00+00:00",
            "nonce-value",
            "GET",
            "/internal/v1/verevon/conversations?limit=10",
            "user-1",
            "org-1",
            "",
            "digest",
        );
        assert_eq!(
            built,
            concat!(
                "v2\n",
                "model-gateway\n",
                "conversation-core\n",
                "2026-09-15T00:00:00+00:00\n",
                "nonce-value\n",
                "GET\n",
                "/internal/v1/verevon/conversations?limit=10\n",
                "user-1\n",
                "org-1\n",
                "\n",
                "digest",
            )
        );
    }

    #[test]
    fn body_digest_is_the_hash_of_an_empty_body_not_an_empty_string() {
        let headers = sign_get(TOKEN, "/internal/v1/verevon/conversations", "u", "o", at(0));
        // Go: base64.RawURLEncoding.EncodeToString(sha256.Sum256(nil)).
        assert_eq!(
            headers.body_sha256,
            "47DEQpj8HBSa-_TImW-5JCeuQeRkm5NMpJWZG3hSuFU"
        );
    }

    /// A signature that did not change with the URI would let a captured header
    /// set be replayed against another conversation.
    #[test]
    fn signature_binds_the_request_uri() {
        let one = sign_get(TOKEN, "/internal/v1/verevon/conversations/a", "u", "o", at(0));
        let two = sign_get(TOKEN, "/internal/v1/verevon/conversations/b", "u", "o", at(0));
        assert_ne!(one.signature, two.signature);
    }

    /// The same, for the tenant: org-1's signature must not verify for org-2.
    #[test]
    fn signature_binds_the_organization() {
        let one = sign_get(TOKEN, "/internal/v1/verevon/conversations", "u", "org-1", at(0));
        let two = sign_get(TOKEN, "/internal/v1/verevon/conversations", "u", "org-2", at(0));
        assert_ne!(one.signature, two.signature);
    }

    #[test]
    fn nonce_is_unique_per_call_and_matches_the_accepted_pattern() {
        let one = sign_get(TOKEN, "/x", "u", "o", at(0));
        let two = sign_get(TOKEN, "/x", "u", "o", at(0));
        assert_ne!(one.nonce, two.nonce);
        for nonce in [&one.nonce, &two.nonce] {
            assert!(
                (16..=128).contains(&nonce.len())
                    && nonce
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
                "nonce {nonce} would be rejected by conversation-core"
            );
        }
    }

    /// Seconds precision, offset spelled out: `to_rfc3339_opts(.., false)`
    /// renders `+00:00`, which Go's `time.RFC3339` parses. `Z` would parse too,
    /// but the signed string must match the header byte for byte, so the one
    /// that matters is that both come from this single rendering.
    #[test]
    fn timestamp_is_rfc3339_seconds() {
        let headers = sign_get(TOKEN, "/x", "u", "o", at(1_757_894_400));
        assert_eq!(headers.timestamp, "2025-09-15T00:00:00+00:00");
    }
}
