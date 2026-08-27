use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Utc;
use event_envelope_rs::{EnvelopeError, EventSigner, EventVerifier};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use rsa::pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey};
use rsa::{RsaPrivateKey, RsaPublicKey};
use serde::Serialize;
use sha2::{Digest, Sha256};

fn keys() -> (String, String) {
    let private = RsaPrivateKey::new(&mut rand::thread_rng(), 2048).expect("test RSA key");
    let public = RsaPublicKey::from(&private);
    (
        private
            .to_pkcs1_pem(Default::default())
            .expect("private pem")
            .to_string(),
        public
            .to_pkcs1_pem(Default::default())
            .expect("public pem")
            .to_string(),
    )
}

fn contract() -> (EventSigner, EventVerifier) {
    let (private, public) = keys();
    (
        EventSigner::from_rsa_pem(
            private.as_bytes(),
            "service:documents-api-go",
            "documents-events-v1",
            "dataplane-events",
            "events:documents:publish",
        )
        .expect("signer"),
        EventVerifier::from_rsa_pem(
            public.as_bytes(),
            "service:documents-api-go",
            "documents-events-v1",
            "dataplane-events",
            "events:documents:publish",
            100,
        )
        .expect("verifier"),
    )
}

#[derive(Serialize)]
struct GoCompatibleClaims<'a> {
    iss: &'a str,
    sub: &'a str,
    aud: Vec<&'a str>,
    principal_type: &'a str,
    org_id: &'a str,
    user_id: &'a str,
    scopes: Vec<&'a str>,
    zdr: bool,
    event_type: &'a str,
    payload_sha256: String,
    jti: &'a str,
    iat: i64,
    nbf: i64,
    exp: i64,
}

fn go_compatible_envelope(private: &str, audiences: Vec<&str>) -> Vec<u8> {
    let payload = br#"{"document_id":"doc-go","org_id":"org-go","user_id":"user-go","zdr":false}"#;
    let now = Utc::now().timestamp();
    let claims = GoCompatibleClaims {
        iss: "service:documents-api-go",
        sub: "service:documents-api-go",
        aud: audiences,
        principal_type: "service",
        org_id: "org-go",
        user_id: "user-go",
        scopes: vec!["events:documents:publish"],
        zdr: false,
        event_type: "dataplane.documents.created",
        payload_sha256: Sha256::digest(payload)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        jti: "go-compatibility-fixture",
        iat: now,
        nbf: now,
        exp: now + 120,
    };
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("documents-events-v1".to_owned());
    let token = encode(
        &header,
        &claims,
        &EncodingKey::from_rsa_pem(private.as_bytes()).expect("encoding key"),
    )
    .expect("token");
    serde_json::to_vec(&serde_json::json!({
        "authorization": format!("Bearer {token}"),
        "data": URL_SAFE_NO_PAD.encode(payload),
    }))
    .expect("envelope")
}

#[test]
fn accepts_go_single_audience_array_but_rejects_ambiguous_arrays() {
    let (private, public) = keys();
    let verifier = || {
        EventVerifier::from_rsa_pem(
            public.as_bytes(),
            "service:documents-api-go",
            "documents-events-v1",
            "dataplane-events",
            "events:documents:publish",
            10,
        )
        .expect("verifier")
    };

    let compatible = go_compatible_envelope(&private, vec!["dataplane-events"]);
    assert!(verifier()
        .verify("dataplane.documents.created", &compatible)
        .is_ok());

    let ambiguous = go_compatible_envelope(&private, vec!["dataplane-events", "other"]);
    assert!(verifier()
        .verify("dataplane.documents.created", &ambiguous)
        .is_err());

    let empty = go_compatible_envelope(&private, vec![""]);
    assert!(verifier()
        .verify("dataplane.documents.created", &empty)
        .is_err());
}

#[test]
fn verifies_and_returns_claim_bound_payload() {
    let (signer, verifier) = contract();
    let payload = br#"{"document_id":"doc-test","org_id":"org-test","user_id":"user-test"}"#;
    let envelope = signer
        .sign(
            "dataplane.documents.created",
            "org-test",
            Some("user-test"),
            true,
            payload,
        )
        .expect("sign");

    let verified = verifier
        .verify("dataplane.documents.created", &envelope)
        .expect("verify");
    assert_eq!(verified.payload, payload);
    assert_eq!(verified.claims.org_id, "org-test");
    assert_eq!(verified.claims.user_id.as_deref(), Some("user-test"));
    assert!(verified.claims.zdr);
}

#[test]
fn rejects_tampering_wrong_subject_tenant_user_and_replay() {
    let (signer, verifier) = contract();
    let payload = br#"{"document_id":"doc-test","org_id":"org-test","user_id":"user-test"}"#;
    let envelope = signer
        .sign(
            "dataplane.documents.created",
            "org-test",
            Some("user-test"),
            false,
            payload,
        )
        .expect("sign");

    assert!(verifier
        .verify("dataplane.documents.updated", &envelope)
        .is_err());
    assert!(verifier
        .verify("dataplane.documents.created", &envelope)
        .is_ok());
    assert!(verifier
        .verify("dataplane.documents.created", &envelope)
        .is_err());
    assert!(verifier
        .verify_redelivery("dataplane.documents.created", &envelope)
        .is_ok());

    let mut tampered: serde_json::Value = serde_json::from_slice(&envelope).expect("json");
    tampered["data"] = serde_json::Value::String("e30".to_string());
    assert!(verifier
        .verify(
            "dataplane.documents.created",
            &serde_json::to_vec(&tampered).expect("json")
        )
        .is_err());

    for conflicting in [
        br#"{"org_id":"other-org","user_id":"user-test"}"#.as_slice(),
        br#"{"org_id":"org-test","user_id":"other-user"}"#.as_slice(),
    ] {
        let signed = signer.sign(
            "dataplane.documents.created",
            "org-test",
            Some("user-test"),
            false,
            conflicting,
        );
        assert!(
            signed.is_err(),
            "producer must not sign claim-conflicting data"
        );
    }
}

#[test]
fn rejects_wrong_key_issuer_scope_and_ambiguous_identity() {
    let (private, public) = keys();
    let (_, other_public) = keys();
    let signer = EventSigner::from_rsa_pem(
        private.as_bytes(),
        "service:index-engine-rs",
        "index-events-v1",
        "dataplane-events",
        "events:index:publish",
    )
    .expect("signer");
    let envelope = signer
        .sign(
            "dataplane.knowledge.units.created",
            "org-test",
            None,
            false,
            br#"{"org_id":"org-test"}"#,
        )
        .expect("sign");

    for verifier in [
        EventVerifier::from_rsa_pem(
            other_public.as_bytes(),
            "service:index-engine-rs",
            "index-events-v1",
            "dataplane-events",
            "events:index:publish",
            10,
        ),
        EventVerifier::from_rsa_pem(
            public.as_bytes(),
            "service:other",
            "index-events-v1",
            "dataplane-events",
            "events:index:publish",
            10,
        ),
        EventVerifier::from_rsa_pem(
            public.as_bytes(),
            "service:index-engine-rs",
            "index-events-v1",
            "dataplane-events",
            "events:admin:publish",
            10,
        ),
    ] {
        assert!(verifier
            .expect("verifier config")
            .verify("dataplane.knowledge.units.created", &envelope)
            .is_err());
    }

    assert!(EventSigner::from_rsa_pem(
        private.as_bytes(),
        "documents-api-go",
        "index-events-v1",
        "dataplane-events",
        "events:index:publish",
    )
    .is_err());

    assert!(signer
        .sign(
            "dataplane.search.rebuild.requested",
            "org-test",
            None,
            false,
            br#"{"org_id":"org-test"}"#,
        )
        .is_err());
}

#[test]
fn wiki_scope_is_producer_specific_and_requires_non_zdr_payload() {
    let (private, public) = keys();
    let signer = EventSigner::from_rsa_pem(
        private.as_bytes(),
        "service:wiki-store-go",
        "wiki-events-v1",
        "dataplane-events",
        "events:wiki:publish",
    )
    .expect("wiki signer");
    let verifier = EventVerifier::from_rsa_pem(
        public.as_bytes(),
        "service:wiki-store-go",
        "wiki-events-v1",
        "dataplane-events",
        "events:wiki:publish",
        10,
    )
    .expect("wiki verifier");
    let payload = br#"{"page_id":"page-test","version_id":"version-test","org_id":"org-test","user_id":"user-test","zdr":false}"#;
    let envelope = signer
        .sign(
            "dataplane.wiki.version.published",
            "org-test",
            Some("user-test"),
            false,
            payload,
        )
        .expect("sign wiki event");
    let verified = verifier
        .verify("dataplane.wiki.version.published", &envelope)
        .expect("verify wiki event");
    assert_eq!(verified.claims.org_id, "org-test");
    assert!(!verified.claims.zdr);

    assert!(signer
        .sign(
            "dataplane.documents.created",
            "org-test",
            Some("user-test"),
            false,
            payload,
        )
        .is_err());
    assert!(signer
        .sign(
            "dataplane.wiki.version.published",
            "org-test",
            Some("user-test"),
            true,
            br#"{"org_id":"org-test","user_id":"user-test","zdr":true}"#,
        )
        .is_err());
}

#[test]
fn retrieval_cost_scope_is_producer_specific_and_claim_bound() {
    let (private, public) = keys();
    let signer = EventSigner::from_rsa_pem(
        private.as_bytes(),
        "service:retrieval-engine-rs",
        "retrieval-events-v1",
        "dataplane-events",
        "events:retrieval:publish",
    )
    .expect("retrieval signer");
    let verifier = EventVerifier::from_rsa_pem(
        public.as_bytes(),
        "service:retrieval-engine-rs",
        "retrieval-events-v1",
        "dataplane-events",
        "events:retrieval:publish",
        10,
    )
    .expect("retrieval verifier");
    let payload = br#"{"event_type":"rerank","model":"test","count":2,"estimated_tokens":64,"org_id":"org-test","user_id":"user-test","zdr":false,"idempotency_key":"cost-test"}"#;
    let envelope = signer
        .sign(
            "dataplane.cost.ledger",
            "org-test",
            Some("user-test"),
            false,
            payload,
        )
        .expect("signed retrieval cost event");
    let verified = verifier
        .verify("dataplane.cost.ledger", &envelope)
        .expect("verified retrieval cost event");
    assert_eq!(verified.claims.org_id, "org-test");
    assert!(!verified.claims.zdr);

    assert!(signer
        .sign(
            "dataplane.documents.indexed",
            "org-test",
            Some("user-test"),
            false,
            payload,
        )
        .is_err());
}

// ── Expired-vs-forged: the distinction that stops silent data loss ──────────
//
// A durable queue (max_age 7 days) carries envelopes that live 120s, so a
// consumer backlog longer than two minutes expires messages in place. Treating
// that as a forgery discarded 1,044 of 1,164 chunks on a single real ingest.
// `EnvelopeError::Expired` exists so a consumer can re-drive those; it must
// therefore be reachable ONLY for an otherwise-perfect envelope.

/// Build an envelope whose `exp` is already in the past but which is otherwise
/// completely valid — correct key, issuer, scope, audience, payload digest.
fn expired_envelope_jti(private: &str, age_seconds: i64, jti: &str) -> Vec<u8> {
    let payload = br#"{"document_id":"doc-go","org_id":"org-go","user_id":"user-go","zdr":false}"#;
    let now = Utc::now().timestamp();
    let issued = now - age_seconds;
    let claims = GoCompatibleClaims {
        iss: "service:documents-api-go",
        sub: "service:documents-api-go",
        aud: vec!["dataplane-events"],
        principal_type: "service",
        org_id: "org-go",
        user_id: "user-go",
        scopes: vec!["events:documents:publish"],
        zdr: false,
        event_type: "dataplane.documents.created",
        payload_sha256: Sha256::digest(payload)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        jti,
        iat: issued,
        nbf: issued,
        // 120s lifetime, same as TOKEN_TTL_SECONDS, so `exp - iat` stays inside
        // MAX_TOKEN_TTL_SECONDS and only the expiry itself is at fault.
        exp: issued + 120,
    };
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("documents-events-v1".to_owned());
    let token = encode(
        &header,
        &claims,
        &EncodingKey::from_rsa_pem(private.as_bytes()).expect("encoding key"),
    )
    .expect("token");
    serde_json::to_vec(&serde_json::json!({
        "authorization": format!("Bearer {token}"),
        "data": URL_SAFE_NO_PAD.encode(payload),
    }))
    .expect("envelope")
}

fn expired_envelope(private: &str, age_seconds: i64) -> Vec<u8> {
    expired_envelope_jti(
        private,
        age_seconds,
        &format!("expired-fixture-{age_seconds}"),
    )
}

#[test]
fn an_expired_but_authentic_envelope_is_recoverable_not_discarded() {
    let (private, public) = keys();
    let verifier = EventVerifier::from_rsa_pem(
        public.as_bytes(),
        "service:documents-api-go",
        "documents-events-v1",
        "dataplane-events",
        "events:documents:publish",
        100,
    )
    .expect("verifier");

    // 10 minutes stale: well past both the 120s TTL and the 30s skew.
    let envelope = expired_envelope(&private, 600);
    match verifier.verify("dataplane.documents.created", &envelope) {
        Err(EnvelopeError::Expired(stale)) => {
            // The claims and payload must be usable, because identifying the
            // stranded work is the entire point of this variant.
            assert_eq!(stale.claims.org_id, "org-go");
            assert_eq!(stale.claims.event_type, "dataplane.documents.created");
            let payload: serde_json::Value =
                serde_json::from_slice(&stale.payload).expect("payload is JSON");
            assert_eq!(payload["document_id"], "doc-go");
        }
        other => panic!("expected Expired, got {other:?}"),
    }
}

/// The security-critical half. Every way of being wrong OTHER than expiry must
/// still yield `InvalidEnvelope`, so no forgery can reach the recovery path and
/// have its (attacker-chosen) payload trusted.
#[test]
fn a_forged_envelope_never_reaches_the_expired_recovery_path() {
    let (private, public) = keys();
    let (other_private, _) = keys(); // a different signing key entirely
    let verifier = || {
        EventVerifier::from_rsa_pem(
            public.as_bytes(),
            "service:documents-api-go",
            "documents-events-v1",
            "dataplane-events",
            "events:documents:publish",
            100,
        )
        .expect("verifier")
    };

    // 1. Expired AND signed by the wrong key: must be InvalidEnvelope, not
    //    Expired — otherwise an attacker forges "expired" events at will.
    let wrong_key = expired_envelope(&other_private, 600);
    assert!(matches!(
        verifier().verify("dataplane.documents.created", &wrong_key),
        Err(EnvelopeError::InvalidEnvelope)
    ));

    // 2. Expired but with a tampered payload (digest no longer matches).
    let mut tampered: serde_json::Value =
        serde_json::from_slice(&expired_envelope(&private, 600)).expect("json");
    tampered["data"] = serde_json::json!(URL_SAFE_NO_PAD.encode(
        br#"{"document_id":"ATTACKER","org_id":"org-go","user_id":"user-go","zdr":false}"#
    ));
    assert!(matches!(
        verifier().verify(
            "dataplane.documents.created",
            &serde_json::to_vec(&tampered).expect("bytes")
        ),
        Err(EnvelopeError::InvalidEnvelope)
    ));

    // 3. Expired but for a subject outside the signer's scope.
    assert!(matches!(
        verifier().verify(
            "dataplane.wiki.pages.created",
            &expired_envelope(&private, 600)
        ),
        Err(EnvelopeError::InvalidEnvelope)
    ));

    // 4. Garbage.
    assert!(matches!(
        verifier().verify("dataplane.documents.created", b"not-an-envelope"),
        Err(EnvelopeError::InvalidEnvelope)
    ));
}

/// A fresh envelope must still verify normally, and an envelope inside the
/// clock-skew window must NOT be treated as expired.
#[test]
fn a_fresh_envelope_is_unaffected_and_skew_is_still_tolerated() {
    let (private, public) = keys();
    let verifier = EventVerifier::from_rsa_pem(
        public.as_bytes(),
        "service:documents-api-go",
        "documents-events-v1",
        "dataplane-events",
        "events:documents:publish",
        100,
    )
    .expect("verifier");

    // Fresh: accepted. Distinct `jti` per envelope — replay protection is
    // per-id, so reusing one would make the second verify fail as a replay and
    // hide what this test is actually checking.
    let fresh = expired_envelope_jti(&private, 0, "skew-fresh");
    assert!(verifier
        .verify("dataplane.documents.created", &fresh)
        .is_ok());

    // 130s old: exp passed 10s ago, inside the 30s skew -> still accepted, so
    // the new explicit check reproduces the library's leeway rather than
    // tightening it.
    let barely = expired_envelope_jti(&private, 130, "skew-barely");
    assert!(
        verifier
            .verify("dataplane.documents.created", &barely)
            .is_ok(),
        "clock skew must still be tolerated"
    );
}
