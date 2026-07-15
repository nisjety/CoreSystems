use async_nats::Client as NatsClient;
use event_envelope_rs::EventVerifier;
use futures::StreamExt;
use serde::Deserialize;
use std::sync::Arc;

use super::CacheLayer;

const SUBJECT_DOC_CREATED: &str = "dataplane.documents.created";
const SUBJECT_DOC_DELETED: &str = "dataplane.documents.deleted";
const SUBJECT_DOC_UPDATED: &str = "dataplane.documents.updated";

#[derive(Deserialize)]
struct DocEvent {
    org_id: String,
}

/// Spawns a background task that listens for document mutation events on NATS
/// and invalidates the retrieval-result cache for the affected org_id.
///
/// Embedding cache (keyed by query text) is intentionally NOT invalidated — it
/// is independent of document state.
pub fn spawn_invalidator(nats: NatsClient, cache: CacheLayer, verifier: Arc<EventVerifier>) {
    tokio::spawn(async move {
        let subjects = [
            SUBJECT_DOC_CREATED,
            SUBJECT_DOC_DELETED,
            SUBJECT_DOC_UPDATED,
        ];
        for subject in subjects {
            let cache = cache.clone();
            let nats = nats.clone();
            let verifier = verifier.clone();
            tokio::spawn(async move {
                let mut sub = match nats.subscribe(subject).await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(?e, subject, "cache invalidator subscribe failed");
                        return;
                    }
                };
                tracing::info!(subject, "cache invalidator subscribed");

                while let Some(msg) = sub.next().await {
                    let evt = match decode_doc_event(&verifier, subject, &msg.payload, false) {
                        Ok(e) => e,
                        Err(e) => {
                            tracing::warn!(?e, "cache invalidator decode failed");
                            continue;
                        }
                    };
                    cache.invalidate_org_retrieval(&evt.org_id).await;
                }
            });
        }
    });
}

fn decode_doc_event(
    verifier: &EventVerifier,
    subject: &str,
    bytes: &[u8],
    redelivery: bool,
) -> anyhow::Result<DocEvent> {
    let event = if redelivery {
        verifier.verify_redelivery(subject, bytes)?
    } else {
        verifier.verify(subject, bytes)?
    };
    let payload: DocEvent = serde_json::from_slice(&event.payload)?;
    anyhow::ensure!(
        payload.org_id == event.claims.org_id,
        "cache invalidation tenant mismatch"
    );
    Ok(payload)
}

#[cfg(test)]
mod signed_event_tests {
    use super::*;
    use event_envelope_rs::{EventSigner, EventVerifier};
    use rsa::{
        pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey},
        rand_core::OsRng,
        RsaPrivateKey, RsaPublicKey,
    };

    fn contract() -> (EventSigner, EventVerifier) {
        let private = RsaPrivateKey::new(&mut OsRng, 2048).expect("test RSA key");
        let public = RsaPublicKey::from(&private);
        let private_pem = private
            .to_pkcs1_pem(Default::default())
            .expect("private PEM");
        let public_pem = public.to_pkcs1_pem(Default::default()).expect("public PEM");
        (
            EventSigner::from_rsa_pem(
                private_pem.as_bytes(),
                "service:documents-api-go",
                "documents-events-v1",
                "dataplane-events",
                "events:documents:publish",
            )
            .expect("signer"),
            EventVerifier::from_rsa_pem(
                public_pem.as_bytes(),
                "service:documents-api-go",
                "documents-events-v1",
                "dataplane-events",
                "events:documents:publish",
                32,
            )
            .expect("verifier"),
        )
    }

    #[test]
    fn invalidator_rejects_raw_and_decodes_only_verified_tenant() {
        let (signer, verifier) = contract();
        let raw = br#"{"org_id":"org-test","document_id":"doc-test","zdr":false}"#;
        assert!(decode_doc_event(&verifier, SUBJECT_DOC_UPDATED, raw, false).is_err());

        let envelope = signer
            .sign(
                SUBJECT_DOC_UPDATED,
                "org-test",
                Some("user-test"),
                false,
                br#"{"org_id":"org-test","user_id":"user-test","document_id":"doc-test","zdr":false}"#,
            )
            .expect("signed event");
        let event = decode_doc_event(&verifier, SUBJECT_DOC_UPDATED, &envelope, false)
            .expect("verified event");
        assert_eq!(event.org_id, "org-test");

        assert!(signer
            .sign(
                SUBJECT_DOC_UPDATED,
                "org-test",
                None,
                false,
                br#"{"org_id":"other-org","document_id":"doc-test","zdr":false}"#,
            )
            .is_err());
    }
}
