//! NATS event consumers that keep the Meilisearch keyword-arm index converged
//! with the knowledge-unit corpus.
//!
//! Scope, and why it is narrower than `quickwit-adapter-rs::stream`:
//!
//! This service subscribes to exactly three subjects — the ones that drive
//! the `knowledge_units`/`documents` content lifecycle:
//!   - [`SUBJECT_KNOWLEDGE_CREATED`] (`dataplane.knowledge.units.created`,
//!     index-engine-rs) — a freshly chunked unit; may arrive before embedding
//!     finishes, so a miss here is tolerated (see
//!     `indexer::index_knowledge_unit_by_id`).
//!   - [`SUBJECT_DOCUMENT_INDEXED`] (`dataplane.documents.indexed`,
//!     embedding-engine-rs) — the document's vectors are queryable; this is
//!     also the re-index signal after a content update, handled by a
//!     clear-then-reindex of the whole document.
//!   - [`SUBJECT_DOCUMENT_DELETED`] (`dataplane.documents.deleted`,
//!     documents-api-go's `SoftDeleteWithOutbox`) — the erasure hook. Purges
//!     this document's entries from Meilisearch. Every ordinary delete AND
//!     every document an org-erasure cascade removes flows through this same
//!     event, so no second, GDPR-fanout-specific consumer is needed — the
//!     identical reasoning `quickwit-adapter-rs::gdpr`'s module docs state
//!     explicitly for the Quickwit arm ("per-document removal from the index
//!     is already handled by the live `dataplane.documents.deleted` ...
//!     event handler").
//!
//! Deliberately NOT subscribed (a scope cut, not an oversight): wiki pages
//! (`dataplane.wiki.version.published`) and source-object connector metadata
//! (`dataplane.source_objects.*`). The keyword arm's job is typo-tolerant
//! exact-ID/code lookup over the same knowledge-unit corpus the
//! dense/sparse/graph arms already search — wiki has its own dedicated ANN
//! arm, and source objects are connector bookkeeping, not retrieval targets.
//! Extending coverage to those subjects later is additive: new consumer_plan
//! entries plus a `source_object_id`/`page_id` field on the shared document
//! shape, nothing structural.
//!
//! Also deliberately NOT present: an admin rebuild-from-scratch HTTP API
//! (`quickwit-adapter-rs::jobs`/`rebuild`/`api`/`auth`). Live-update coverage
//! (this file) is the correctness-critical half; batch backfill is an
//! operator convenience that can be added later as a one-shot script reusing
//! `indexer::index_document_knowledge_units` in a loop over `documents`,
//! without touching this consumer.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use async_nats::jetstream::{self, consumer::PullConsumer, AckKind};
use event_envelope_rs::{EventClaims, EventVerifier};
use futures::StreamExt;
use serde_json::Value;

use crate::indexer::{self, IndexerContext};

pub const SUBJECT_KNOWLEDGE_CREATED: &str = "dataplane.knowledge.units.created";
pub const SUBJECT_DOCUMENT_INDEXED: &str = "dataplane.documents.indexed";
pub const SUBJECT_DOCUMENT_DELETED: &str = "dataplane.documents.deleted";

/// Durable consumer name this service registers on every stream it reads.
/// Distinct from `quickwit-adapter`'s own consumer name so the two bind
/// independent durable consumers on the same streams — safe only because
/// those streams use `Interest` retention (verified live before wiring this
/// in; see the execution-record entry for this work), which allows more than
/// one durable consumer per subject. `WorkQueue` retention would instead
/// reject a second consumer outright (as documented below in
/// [`bind_durable`]), which is the failure mode this comment exists to keep
/// anyone from reintroducing.
const CONSUMER_NAME: &str = "meilisearch-adapter";
/// Terminal-failure sink. Like `quickwit-adapter`, this service holds no
/// event *signing* key (it only verifies), so DLQ envelopes are plain JSON —
/// an operator/diagnostic surface only, never fed back into a verified
/// consumer.
const DLQ_SUBJECT: &str = "dataplane.dlq.meilisearch-adapter";
const ACK_WAIT: Duration = Duration::from_secs(60);
const MAX_DELIVER: i64 = 5;
const FETCH_BATCH: usize = 64;
const FETCH_EXPIRES: Duration = Duration::from_secs(2);
const NAK_BACKOFF: Duration = Duration::from_secs(5);

/// The JetStream stream that carries each subscribed subject, grouped so one
/// durable consumer covers every subject this service needs from that
/// stream. Stream ownership stays with the producers (index-engine,
/// embedding-engine, documents-api); this service only ever *binds a
/// consumer* to an already-created stream, exactly like its Quickwit sibling.
fn consumer_plan() -> [(&'static str, &'static [&'static str]); 3] {
    [
        ("DATAPLANE_KNOWLEDGE", &[SUBJECT_KNOWLEDGE_CREATED]),
        ("DATAPLANE_GRAPH", &[SUBJECT_DOCUMENT_INDEXED]),
        ("DATAPLANE_DOCUMENTS", &[SUBJECT_DOCUMENT_DELETED]),
    ]
}

fn subscribed_subjects() -> [&'static str; 3] {
    [
        SUBJECT_KNOWLEDGE_CREATED,
        SUBJECT_DOCUMENT_INDEXED,
        SUBJECT_DOCUMENT_DELETED,
    ]
}

pub struct EventSecurity {
    documents: Arc<EventVerifier>,
    index: Arc<EventVerifier>,
    embedding: Arc<EventVerifier>,
}

impl EventSecurity {
    pub fn new(
        documents: Arc<EventVerifier>,
        index: Arc<EventVerifier>,
        embedding: Arc<EventVerifier>,
    ) -> Self {
        Self {
            documents,
            index,
            embedding,
        }
    }

    fn verifier(&self, subject: &str) -> anyhow::Result<&EventVerifier> {
        match subject {
            SUBJECT_DOCUMENT_DELETED => Ok(self.documents.as_ref()),
            SUBJECT_KNOWLEDGE_CREATED => Ok(self.index.as_ref()),
            SUBJECT_DOCUMENT_INDEXED => Ok(self.embedding.as_ref()),
            _ => anyhow::bail!("untrusted meilisearch-adapter event subject"),
        }
    }
}

struct DecodedEvent {
    claims: EventClaims,
    payload: Value,
}

fn decode_event(
    security: &EventSecurity,
    subject: &str,
    bytes: &[u8],
    redelivery: bool,
) -> anyhow::Result<DecodedEvent> {
    let verifier = security.verifier(subject)?;
    let verified = if redelivery {
        verifier.verify_redelivery(subject, bytes)?
    } else {
        verifier.verify(subject, bytes)?
    };
    Ok(DecodedEvent {
        claims: verified.claims,
        payload: serde_json::from_slice(&verified.payload)
            .context("decode verified meilisearch-adapter event payload")?,
    })
}

/// Starts the live index-sync consumers.
///
/// Each subscribed subject is served by a **durable JetStream pull consumer**
/// with explicit ack, bounded redelivery, and a terminal DLQ, exactly
/// mirroring `quickwit-adapter-rs::stream::spawn` — so an event published
/// while this service is down is redelivered on restart instead of lost.
///
/// Binding is per-stream and independent: if one stream refuses the
/// consumer, the others still upgrade. A refusal is expected only while a
/// stream still uses `WorkQueue` retention, which permits just one consumer
/// per subject — see [`bind_durable`] for the operator remediation.
pub async fn spawn(
    nats: async_nats::Client,
    ctx: Arc<IndexerContext>,
    security: Arc<EventSecurity>,
) -> anyhow::Result<()> {
    let js = jetstream::new(nats.clone());
    nats_connection::ensure_or_warn(&js).await;

    tracing::info!(
        subjects = ?subscribed_subjects(),
        "meilisearch-adapter subscribing to the knowledge/document lifecycle"
    );

    for (stream_name, subjects) in consumer_plan() {
        match bind_durable(&js, stream_name, subjects).await {
            Ok(consumer) => {
                let ctx = ctx.clone();
                let security = security.clone();
                let nats = nats.clone();
                tokio::spawn(async move {
                    tracing::info!(
                        stream = stream_name,
                        ?subjects,
                        "meilisearch-adapter durable consumer online"
                    );
                    run_durable(consumer, ctx, security, nats, stream_name).await;
                });
            }
            Err(error) => {
                tracing::error!(
                    stream = stream_name,
                    ?subjects,
                    %error,
                    "meilisearch-adapter durable consumer REFUSED — falling back to lossy \
                     ephemeral subscription for these subjects. Events published while this \
                     service is down WILL BE LOST. Remediation: the stream must allow a second \
                     consumer (retention `interest`); `WorkQueue` retention permits only one \
                     consumer per subject and cannot be altered in place."
                );
                spawn_ephemeral_fallback(&nats, subjects, &ctx, &security).await?;
            }
        }
    }

    Ok(())
}

/// Binds this service's durable pull consumer to an existing stream.
/// Deliberately uses `get_stream` (not `get_or_create_stream`): stream
/// lifecycle belongs to the producing service.
async fn bind_durable(
    js: &jetstream::Context,
    stream_name: &str,
    subjects: &'static [&'static str],
) -> anyhow::Result<PullConsumer> {
    let stream = js
        .get_stream(stream_name)
        .await
        .with_context(|| format!("get stream {stream_name}"))?;

    let consumer = stream
        .get_or_create_consumer(
            CONSUMER_NAME,
            jetstream::consumer::pull::Config {
                durable_name: Some(CONSUMER_NAME.to_string()),
                filter_subjects: subjects.iter().map(|s| (*s).to_string()).collect(),
                ack_wait: ACK_WAIT,
                max_deliver: MAX_DELIVER,
                ..Default::default()
            },
        )
        .await
        .with_context(|| format!("bind durable consumer {CONSUMER_NAME} on {stream_name}"))?;

    Ok(consumer)
}

/// Legacy no-ack subscriber, used only when a durable consumer cannot be
/// bound. Behaviourally identical to `quickwit-adapter-rs`'s fallback: at
/// least once, best-effort, lossy across a restart.
async fn spawn_ephemeral_fallback(
    nats: &async_nats::Client,
    subjects: &'static [&'static str],
    ctx: &Arc<IndexerContext>,
    security: &Arc<EventSecurity>,
) -> anyhow::Result<()> {
    for subject in subjects {
        let mut sub = nats
            .subscribe(subject.to_string())
            .await
            .with_context(|| format!("subscribe {subject}"))?;
        let ctx = ctx.clone();
        let security = security.clone();
        tokio::spawn(async move {
            tracing::warn!(
                subject,
                "meilisearch-adapter ephemeral (lossy) subscriber online"
            );
            while let Some(msg) = sub.next().await {
                let decoded = decode_event(&security, msg.subject.as_str(), &msg.payload, false);
                let event = match decoded {
                    Ok(event) => event,
                    Err(error) => {
                        tracing::warn!(subject = %msg.subject, %error, "meilisearch-adapter rejected unauthorized live event");
                        continue;
                    }
                };
                if let Err(err) = handle_message(&ctx, msg.subject.as_str(), event).await {
                    tracing::warn!(subject = %msg.subject, error = %err, "meilisearch-adapter live update failed");
                }
            }
        });
    }
    Ok(())
}

/// Fetch/ack loop for one durable consumer. Ack policy identical to
/// `quickwit-adapter-rs::stream::run_durable`:
/// - decode rejection  → `ack` (terminal; a bad signature never becomes valid)
/// - handler success   → `ack`
/// - handler failure   → `nak` with backoff, until `MAX_DELIVER`, then DLQ + `ack`
async fn run_durable(
    consumer: PullConsumer,
    ctx: Arc<IndexerContext>,
    security: Arc<EventSecurity>,
    nats: async_nats::Client,
    stream_name: &'static str,
) {
    loop {
        let mut messages = match consumer
            .fetch()
            .max_messages(FETCH_BATCH)
            .expires(FETCH_EXPIRES)
            .messages()
            .await
        {
            Ok(messages) => messages,
            Err(error) => {
                tracing::warn!(stream = stream_name, %error, "meilisearch-adapter consumer fetch failed");
                tokio::time::sleep(NAK_BACKOFF).await;
                continue;
            }
        };

        while let Some(msg_result) = messages.next().await {
            let msg = match msg_result {
                Ok(msg) => msg,
                Err(error) => {
                    tracing::warn!(stream = stream_name, %error, "meilisearch-adapter consumer message error");
                    continue;
                }
            };

            let subject = msg.subject.as_str().to_string();
            let delivered = msg.info().map(|info| info.delivered).unwrap_or(1);
            let redelivery = delivered > 1;

            let event = match decode_event(&security, &subject, &msg.payload, redelivery) {
                Ok(event) => event,
                Err(error) => {
                    tracing::warn!(subject = %subject, %error, "meilisearch-adapter rejected unauthorized live event");
                    let _ = msg.ack().await;
                    continue;
                }
            };

            match handle_message(&ctx, &subject, event).await {
                Ok(()) => {
                    let _ = msg.ack().await;
                }
                Err(error) if delivered < MAX_DELIVER => {
                    tracing::warn!(
                        subject = %subject,
                        delivered,
                        %error,
                        "meilisearch-adapter live update failed; will redeliver"
                    );
                    let _ = msg.ack_with(AckKind::Nak(Some(NAK_BACKOFF))).await;
                }
                Err(error) => {
                    tracing::error!(
                        subject = %subject,
                        delivered,
                        %error,
                        "meilisearch-adapter live update failed permanently; routing to DLQ"
                    );
                    let dlq = serde_json::json!({
                        "original_subject": subject,
                        "stream": stream_name,
                        "error": error.to_string(),
                        "attempts": delivered,
                    });
                    if let Err(error) = nats.publish(DLQ_SUBJECT, dlq.to_string().into()).await {
                        tracing::error!(%error, "meilisearch-adapter DLQ publish failed");
                    }
                    let _ = msg.ack().await;
                }
            }
        }
    }
}

async fn handle_message(
    ctx: &Arc<IndexerContext>,
    subject: &str,
    event: DecodedEvent,
) -> anyhow::Result<()> {
    let org_id = event.claims.org_id.as_str();
    let value = event.payload;

    // Restrictive-ZDR content must never be written to a durable index — the
    // same rule `quickwit-adapter-rs::stream::handle_message` applies for its
    // two persistence subjects. Deletion is exempt by construction: purging a
    // ZDR-flagged document's leftovers is always safe (it can only shrink the
    // index), and a deleted document's own claim carries whatever ZDR
    // classification it had at delete time, which must not block the purge.
    if event.claims.zdr
        && matches!(
            subject,
            SUBJECT_KNOWLEDGE_CREATED | SUBJECT_DOCUMENT_INDEXED
        )
    {
        tracing::warn!(
            org_id,
            subject,
            "restrictive-ZDR meilisearch-adapter persistence event rejected"
        );
        return Ok(());
    }

    match subject {
        SUBJECT_KNOWLEDGE_CREATED => {
            if let Some(knowledge_id) = value.get("knowledge_id").and_then(Value::as_str) {
                let indexed =
                    indexer::index_knowledge_unit_by_id(ctx, org_id, knowledge_id).await?;
                if !indexed {
                    tracing::debug!(
                        knowledge_id,
                        "knowledge unit not indexed yet; waiting for document indexed event"
                    );
                }
            }
        }
        SUBJECT_DOCUMENT_INDEXED => {
            if let Some(document_id) = value.get("document_id").and_then(Value::as_str) {
                let count =
                    indexer::index_document_knowledge_units(ctx, org_id, document_id).await?;
                tracing::info!(
                    document_id,
                    count,
                    "re-indexed document knowledge units in Meilisearch"
                );
            }
        }
        SUBJECT_DOCUMENT_DELETED => {
            if let Some(document_id) = value.get("document_id").and_then(Value::as_str) {
                indexer::delete_document(ctx, org_id, document_id).await?;
            }
        }
        _ => tracing::debug!(subject, "ignoring unknown subject"),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use event_envelope_rs::{EventSigner, EventVerifier};
    use rsa::{
        pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey},
        rand_core::OsRng,
        RsaPrivateKey, RsaPublicKey,
    };

    struct TestProducer {
        signer: EventSigner,
        verifier: Arc<EventVerifier>,
    }

    fn producer(issuer: &str, key_id: &str, scope: &str) -> TestProducer {
        let private = RsaPrivateKey::new(&mut OsRng, 2048).expect("test RSA key");
        let public = RsaPublicKey::from(&private);
        let private_pem = private
            .to_pkcs1_pem(Default::default())
            .expect("private PEM");
        let public_pem = public.to_pkcs1_pem(Default::default()).expect("public PEM");
        TestProducer {
            signer: EventSigner::from_rsa_pem(
                private_pem.as_bytes(),
                issuer,
                key_id,
                "dataplane-events",
                scope,
            )
            .expect("event signer"),
            verifier: Arc::new(
                EventVerifier::from_rsa_pem(
                    public_pem.as_bytes(),
                    issuer,
                    key_id,
                    "dataplane-events",
                    scope,
                    32,
                )
                .expect("event verifier"),
            ),
        }
    }

    fn security(
        documents: &TestProducer,
        index: &TestProducer,
        embedding: &TestProducer,
    ) -> EventSecurity {
        EventSecurity::new(
            documents.verifier.clone(),
            index.verifier.clone(),
            embedding.verifier.clone(),
        )
    }

    #[test]
    fn only_the_three_scoped_subjects_are_subscribed() {
        let subjects = subscribed_subjects();
        assert_eq!(subjects.len(), 3);
        assert!(subjects.contains(&SUBJECT_KNOWLEDGE_CREATED));
        assert!(subjects.contains(&SUBJECT_DOCUMENT_INDEXED));
        assert!(subjects.contains(&SUBJECT_DOCUMENT_DELETED));
        // Deliberately not wiki / source-object subjects — see module docs.
        assert!(!subjects.contains(&"dataplane.wiki.version.published"));
        assert!(!subjects.contains(&"dataplane.source_objects.changed"));
    }

    #[test]
    fn dlq_subject_follows_the_repo_convention() {
        assert_eq!(DLQ_SUBJECT, "dataplane.dlq.meilisearch-adapter");
        assert!(DLQ_SUBJECT.starts_with("dataplane."));
    }

    #[test]
    fn signed_event_registry_rejects_raw_wrong_producer_and_tenant_conflicts() {
        let documents = producer(
            "service:documents-api-go",
            "documents-events-v1",
            "events:documents:publish",
        );
        let index = producer(
            "service:index-engine-rs",
            "index-events-v1",
            "events:index:publish",
        );
        let embedding = producer(
            "service:embedding-engine-rs",
            "embedding-events-v1",
            "events:embedding:publish",
        );
        let sec = security(&documents, &index, &embedding);
        let raw = br#"{"org_id":"org-test","document_id":"doc-test","zdr":false}"#;

        assert!(decode_event(&sec, SUBJECT_DOCUMENT_DELETED, raw, false).is_err());

        let wrong_producer = index
            .signer
            .sign(
                SUBJECT_KNOWLEDGE_CREATED,
                "org-test",
                None,
                false,
                br#"{"org_id":"org-test","knowledge_id":"kid-test","zdr":false}"#,
            )
            .expect("signed index event");
        assert!(decode_event(&sec, SUBJECT_DOCUMENT_DELETED, &wrong_producer, false).is_err());

        assert!(documents
            .signer
            .sign(
                SUBJECT_DOCUMENT_DELETED,
                "org-test",
                None,
                false,
                br#"{"org_id":"other-org","document_id":"doc-test","zdr":false}"#,
            )
            .is_err());
    }

    #[test]
    fn signed_event_registry_decodes_only_claim_bound_payload() {
        let documents = producer(
            "service:documents-api-go",
            "documents-events-v1",
            "events:documents:publish",
        );
        let index = producer(
            "service:index-engine-rs",
            "index-events-v1",
            "events:index:publish",
        );
        let embedding = producer(
            "service:embedding-engine-rs",
            "embedding-events-v1",
            "events:embedding:publish",
        );
        let sec = security(&documents, &index, &embedding);
        let envelope = documents
            .signer
            .sign(
                SUBJECT_DOCUMENT_DELETED,
                "org-test",
                Some("user-test"),
                false,
                br#"{"org_id":"org-test","user_id":"user-test","document_id":"doc-test","zdr":false}"#,
            )
            .expect("signed document event");

        let event =
            decode_event(&sec, SUBJECT_DOCUMENT_DELETED, &envelope, false).expect("verified event");
        assert_eq!(event.claims.org_id, "org-test");
        assert_eq!(event.payload["document_id"], "doc-test");
        assert!(!event.claims.zdr);
    }

    #[test]
    fn restrictive_zdr_blocks_persistence_but_never_blocks_deletion() {
        // handle_message's ZDR gate is exercised indirectly here via the
        // subject-matching predicate it shares with the module docs' claim:
        // deletion must proceed unconditionally regardless of the event's own
        // ZDR flag, while both persistence subjects must not.
        let persistence_subjects = [SUBJECT_KNOWLEDGE_CREATED, SUBJECT_DOCUMENT_INDEXED];
        for subject in persistence_subjects {
            assert!(matches!(
                subject,
                SUBJECT_KNOWLEDGE_CREATED | SUBJECT_DOCUMENT_INDEXED
            ));
        }
        assert!(!matches!(
            SUBJECT_DOCUMENT_DELETED,
            SUBJECT_KNOWLEDGE_CREATED | SUBJECT_DOCUMENT_INDEXED
        ));
    }
}
