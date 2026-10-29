//! Per-document erasure — the visual arm's missing half of GDPR Art. 17.
//!
//! `documents-api-go`'s soft-delete already publishes a signed
//! `dataplane.documents.deleted` on the `DATAPLANE_DOCUMENTS` stream (owned by
//! `index-engine-rs`, which creates it — this module only *binds a consumer*,
//! mirroring `quickwit-adapter-rs`'s `bind_durable` convention for a stream it
//! doesn't own). Before this module existed, nothing in Data Plane v2 reacted
//! to that event for the visual arm at all: `image_consumer.rs` has a correct
//! handler for `dataplane.page_images.deleted` (purges Qdrant vectors) — but
//! nothing has ever published that event, so the handler has never once run.
//! The page-image Qdrant vectors AND the underlying MinIO CAS binaries (raw +
//! rendered PNGs, potentially containing scanned PII) for a deleted document
//! stayed live indefinitely.
//!
//! This module closes that gap directly off the event that already fires:
//! verify `dataplane.documents.deleted`, then purge both the page-image
//! vectors (`qdrant_writer::delete_vectors_by_document`, the same call
//! `image_consumer.rs`'s dead handler would have made) and the CAS objects
//! (`cas_store::CasStore::delete_by_doc`) for that one document. It does NOT
//! touch the main text-chunk Qdrant collection — that arm's own erasure path
//! (via `dataplane.knowledge.units.deleted`) is unrelated and already wired
//! elsewhere in this crate.
//!
//! Deliberately independent of `stream::run_consumer` (the signed text-mutation
//! pipeline): erasure must keep working even when a deployment runs with that
//! pipeline disabled (e.g. the unsigned-legacy-dev gate), and a failure here
//! must never be able to stall document indexing.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use async_nats::jetstream::{self, Context as JsContext};
use event_envelope_rs::EventVerifier;
use futures::StreamExt;
use qdrant_client::Qdrant;
use serde::Deserialize;

use crate::cas_store::CasStore;

pub const SUBJECT_DOCUMENT_DELETED: &str = "dataplane.documents.deleted";
/// Owned and created by `index-engine-rs`; this module only binds to it.
const DOCUMENTS_STREAM: &str = "DATAPLANE_DOCUMENTS";
const CONSUMER_NAME: &str = "embedding-engine-document-erasure";
const DLQ_SUBJECT: &str = "dataplane.dlq.embedding-engine-document-erasure";
const ACK_WAIT: Duration = Duration::from_secs(60);
const MAX_DELIVER: i64 = 5;

#[derive(Debug, Deserialize)]
struct DocumentDeletedEvent {
    document_id: String,
    org_id: String,
}

/// Bind the durable consumer and spawn the handling loop. Returns an error
/// only if the bind itself fails (broker unreachable, stream genuinely
/// missing) — matching `wiki_consumer::spawn`'s contract, since erasure
/// genuinely cannot work without this stream and a silent no-op would be
/// worse than a startup failure that's actually visible.
pub async fn spawn(
    js: JsContext,
    qdrant: Qdrant,
    visual_collection: String,
    cas: Option<Arc<CasStore>>,
    nats: async_nats::Client,
    verifier: Arc<EventVerifier>,
) -> anyhow::Result<()> {
    let stream = js
        .get_stream(DOCUMENTS_STREAM)
        .await
        .with_context(|| format!("get stream {DOCUMENTS_STREAM}"))?;
    let consumer = stream
        .get_or_create_consumer(
            CONSUMER_NAME,
            jetstream::consumer::pull::Config {
                durable_name: Some(CONSUMER_NAME.to_string()),
                filter_subjects: vec![SUBJECT_DOCUMENT_DELETED.to_string()],
                ack_wait: ACK_WAIT,
                max_deliver: MAX_DELIVER,
                ..Default::default()
            },
        )
        .await
        .with_context(|| format!("bind durable consumer {CONSUMER_NAME} on {DOCUMENTS_STREAM}"))?;

    tracing::info!(
        subject = SUBJECT_DOCUMENT_DELETED,
        stream = DOCUMENTS_STREAM,
        cas_enabled = cas.is_some(),
        "document erasure subscriber online"
    );

    tokio::spawn(async move {
        loop {
            let mut messages = match consumer.messages().await {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!(error = %e, "document erasure consumer stream open failed; retrying");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };
            while let Some(item) = messages.next().await {
                let msg = match item {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!(error = %e, "document erasure message recv error");
                        continue;
                    }
                };
                let redelivery = msg.info().is_ok_and(|info| info.delivered > 1);
                let evt = match decode_verified(&verifier, &msg.payload, redelivery) {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::warn!(error = %e, "unverified documents.deleted event; acking poison");
                        let _ = msg.ack().await;
                        continue;
                    }
                };

                let result = purge(&qdrant, &visual_collection, cas.as_deref(), &evt).await;
                match result {
                    Ok(()) => {
                        let _ = msg.ack().await;
                    }
                    Err(e) => {
                        let delivered = msg.info().map(|info| info.delivered).unwrap_or(1);
                        if delivered < MAX_DELIVER {
                            tracing::warn!(
                                error = %e,
                                document_id = %evt.document_id,
                                delivered,
                                "document erasure failed; will redeliver"
                            );
                        } else {
                            tracing::error!(
                                error = %e,
                                document_id = %evt.document_id,
                                delivered,
                                "document erasure failed permanently; routing to DLQ"
                            );
                            let dlq = serde_json::json!({
                                "original_subject": SUBJECT_DOCUMENT_DELETED,
                                "stream": DOCUMENTS_STREAM,
                                "document_id": evt.document_id,
                                "org_id": evt.org_id,
                                "error": e.to_string(),
                                "attempts": delivered,
                            });
                            if let Err(error) =
                                nats.publish(DLQ_SUBJECT, dlq.to_string().into()).await
                            {
                                tracing::error!(%error, "document erasure DLQ publish failed");
                            }
                            let _ = msg.ack().await;
                        }
                    }
                }
            }
        }
    });

    Ok(())
}

fn decode_verified(
    verifier: &EventVerifier,
    envelope: &[u8],
    redelivery: bool,
) -> anyhow::Result<DocumentDeletedEvent> {
    let verified = if redelivery {
        verifier.verify_redelivery(SUBJECT_DOCUMENT_DELETED, envelope)?
    } else {
        verifier.verify(SUBJECT_DOCUMENT_DELETED, envelope)?
    };
    let event: DocumentDeletedEvent = serde_json::from_slice(&verified.payload)?;
    anyhow::ensure!(
        !event.document_id.trim().is_empty() && !event.org_id.trim().is_empty(),
        "documents.deleted event missing document_id/org_id"
    );
    anyhow::ensure!(event.org_id == verified.claims.org_id, "tenant mismatch");
    Ok(event)
}

/// Purge the page-image Qdrant vectors and (when configured) the CAS objects
/// for one deleted document. Both steps run regardless of the document's
/// former ZDR/visibility posture — erasure is unconditional; a document that
/// is gone is gone from every store this crate independently writes to.
async fn purge(
    qdrant: &Qdrant,
    visual_collection: &str,
    cas: Option<&CasStore>,
    evt: &DocumentDeletedEvent,
) -> anyhow::Result<()> {
    crate::qdrant_writer::delete_vectors_by_document(qdrant, visual_collection, &evt.document_id)
        .await
        .context("delete page-image vectors")?;
    if let Some(store) = cas {
        store
            .delete_by_doc(&evt.org_id, &evt.document_id)
            .await
            .context("delete page-image CAS objects")?;
    }
    Ok(())
}

#[cfg(test)]
mod security_tests {
    use super::*;
    use event_envelope_rs::EventSigner;
    use rsa::pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey};
    use rsa::{RsaPrivateKey, RsaPublicKey};

    fn contract() -> (EventSigner, EventVerifier) {
        let private = RsaPrivateKey::new(&mut rand::thread_rng(), 2048).expect("test key");
        let public = RsaPublicKey::from(&private);
        let private_pem = private
            .to_pkcs1_pem(Default::default())
            .expect("private pem");
        let public_pem = public.to_pkcs1_pem(Default::default()).expect("public pem");
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
                10,
            )
            .expect("verifier"),
        )
    }

    #[test]
    fn decodes_a_genuinely_signed_deletion() {
        let (signer, verifier) = contract();
        let payload = br#"{"document_id":"doc-1","org_id":"org-1","visibility":"org","zdr":false}"#;
        let envelope = signer
            .sign(SUBJECT_DOCUMENT_DELETED, "org-1", None, false, payload)
            .expect("sign");
        let event = decode_verified(&verifier, &envelope, false).expect("verify");
        assert_eq!(event.document_id, "doc-1");
        assert_eq!(event.org_id, "org-1");
    }

    #[test]
    fn rejects_unsigned_plain_json() {
        let (_signer, verifier) = contract();
        let payload = br#"{"document_id":"doc-1","org_id":"org-1"}"#;
        assert!(decode_verified(&verifier, payload, false).is_err());
    }

    // No test exercises `decode_verified`'s `event.org_id == verified.claims.org_id`
    // check via a genuinely signed envelope: `EventSigner::sign`'s own
    // `validate_payload_claims` already refuses to sign a payload whose
    // `org_id` disagrees with its `org_id` argument (confirmed directly —
    // attempting it here returned `Err(InvalidEnvelope)` before a signature
    // ever existed to verify). The check stays as defense-in-depth against a
    // different producer/signer implementation ever weakening that guarantee,
    // matching the identical check already in
    // `retrieval-engine-rs/src/cache/invalidator.rs` — but a live test would
    // require hand-assembling a wire-format envelope outside `EventSigner`,
    // which is more fragile and misleading than valuable here.

    #[test]
    fn rejects_replay_without_redelivery_flag() {
        let (signer, verifier) = contract();
        let payload = br#"{"document_id":"doc-1","org_id":"org-1","visibility":"org","zdr":false}"#;
        let envelope = signer
            .sign(SUBJECT_DOCUMENT_DELETED, "org-1", None, false, payload)
            .expect("sign");
        assert!(decode_verified(&verifier, &envelope, false).is_ok());
        assert!(decode_verified(&verifier, &envelope, false).is_err());
        assert!(decode_verified(&verifier, &envelope, true).is_ok());
    }
}
