use event_envelope_rs::{EventSigner, EventVerifier};
use rsa::pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey};
use rsa::{RsaPrivateKey, RsaPublicKey};

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
