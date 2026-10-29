use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use async_nats::jetstream::{self, consumer::PullConsumer, AckKind};
use event_envelope_rs::{EventClaims, EventVerifier};
use futures::StreamExt;
use serde_json::Value;

use crate::quickwit::quote_query_value;
use crate::rebuild::{self, RebuildContext};

pub const SUBJECT_KNOWLEDGE_CREATED: &str = "dataplane.knowledge.units.created";
pub const SUBJECT_DOCUMENT_INDEXED: &str = "dataplane.documents.indexed";
pub const SUBJECT_DOCUMENT_DELETED: &str = "dataplane.documents.deleted";
pub const SUBJECT_WIKI_PUBLISHED: &str = "dataplane.wiki.version.published";
pub const SUBJECT_SOURCE_OBJECT_CHANGED: &str = "dataplane.source_objects.changed";
pub const SUBJECT_SOURCE_OBJECT_DELETED: &str = "dataplane.source_objects.deleted";

/// Durable consumer name this service registers on every stream it reads.
/// Consumer names only need to be unique *within* a stream, so one name is
/// reused across streams.
const CONSUMER_NAME: &str = "quickwit-adapter";
/// Terminal-failure sink. Unlike the text consumers, this service holds no
/// event *signing* key (compose mounts four `.pub` verifiers and no `.pem`),
/// so DLQ envelopes are plain JSON and are an operator/diagnostic surface
/// only — never an input to a verified consumer. Provisioning a signing key
/// is tracked as a follow-up in
/// `docs/retrieval-quality-and-durability-plan-2026-08-05.md`.
const DLQ_SUBJECT: &str = "dataplane.dlq.quickwit-adapter";
const ACK_WAIT: Duration = Duration::from_secs(60);
const MAX_DELIVER: i64 = 5;
const FETCH_BATCH: usize = 64;
const FETCH_EXPIRES: Duration = Duration::from_secs(2);
const NAK_BACKOFF: Duration = Duration::from_secs(5);

/// The JetStream stream that carries each subscribed subject, grouped so one
/// durable consumer covers every subject this service needs from that stream.
///
/// Stream ownership stays with the producers (index-engine, embedding-engine,
/// documents-api, wiki-store); this service only ever *binds a consumer* to an
/// already-created stream. It never creates or reconfigures one.
fn consumer_plan() -> [(&'static str, &'static [&'static str]); 5] {
    [
        ("DATAPLANE_KNOWLEDGE", &[SUBJECT_KNOWLEDGE_CREATED]),
        ("DATAPLANE_GRAPH", &[SUBJECT_DOCUMENT_INDEXED]),
        ("DATAPLANE_DOCUMENTS", &[SUBJECT_DOCUMENT_DELETED]),
        ("DATAPLANE_WIKI", &[SUBJECT_WIKI_PUBLISHED]),
        (
            "DATAPLANE_SOURCE_OBJECTS",
            &[SUBJECT_SOURCE_OBJECT_CHANGED, SUBJECT_SOURCE_OBJECT_DELETED],
        ),
    ]
}

fn subscribed_subjects() -> [&'static str; 6] {
    [
        SUBJECT_KNOWLEDGE_CREATED,
        SUBJECT_DOCUMENT_INDEXED,
        SUBJECT_DOCUMENT_DELETED,
        SUBJECT_WIKI_PUBLISHED,
        SUBJECT_SOURCE_OBJECT_CHANGED,
        SUBJECT_SOURCE_OBJECT_DELETED,
    ]
}

pub struct EventSecurity {
    documents: Arc<EventVerifier>,
    index: Arc<EventVerifier>,
    embedding: Arc<EventVerifier>,
    wiki: Arc<EventVerifier>,
}

impl EventSecurity {
    pub fn new(
        documents: Arc<EventVerifier>,
        index: Arc<EventVerifier>,
        embedding: Arc<EventVerifier>,
        wiki: Arc<EventVerifier>,
    ) -> Self {
        Self {
            documents,
            index,
            embedding,
            wiki,
        }
    }

    fn verifier(&self, subject: &str) -> anyhow::Result<&EventVerifier> {
        match subject {
            SUBJECT_DOCUMENT_DELETED
            | SUBJECT_SOURCE_OBJECT_CHANGED
            | SUBJECT_SOURCE_OBJECT_DELETED => Ok(self.documents.as_ref()),
            SUBJECT_KNOWLEDGE_CREATED => Ok(self.index.as_ref()),
            SUBJECT_DOCUMENT_INDEXED => Ok(self.embedding.as_ref()),
            SUBJECT_WIKI_PUBLISHED => Ok(self.wiki.as_ref()),
            _ => anyhow::bail!("untrusted Quickwit event subject"),
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
            .context("decode verified Quickwit event payload")?,
    })
}

/// Starts the live FTS-sync consumers.
///
/// Each subscribed subject is served by a **durable JetStream pull consumer**
/// with explicit ack, bounded redelivery, and a terminal DLQ — so an event
/// published while this service is down is redelivered on restart instead of
/// being lost. The previous implementation used a plain core-NATS
/// `subscribe()`, which has no ack and no redelivery; anything published while
/// the adapter was restarting was dropped permanently, leaving the lexical
/// index silently behind the corpus.
///
/// Binding is per-stream and independent: if one stream refuses the consumer,
/// the others still upgrade. A refusal is expected while a stream still uses
/// `WorkQueue` retention, which permits only one consumer per subject — see
/// [`bind_durable`] for the operator remediation.
pub async fn spawn(
    nats: async_nats::Client,
    ctx: Arc<RebuildContext>,
    security: Arc<EventSecurity>,
) -> anyhow::Result<()> {
    let js = jetstream::new(nats.clone());
    // D17. Note this does NOT contradict `bind_durable`'s rule below that this
    // service never creates a stream: that rule is about the streams it
    // *consumes*, which producers own. This service is itself a *producer* on
    // `dataplane.dlq.quickwit-adapter`, and the DLQ stream's single shared
    // definition lives in `nats_connection::dlq`. Idempotent.
    nats_connection::ensure_or_warn(&js).await;

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
                        "Quickwit durable consumer online"
                    );
                    run_durable(consumer, ctx, security, nats, stream_name).await;
                });
            }
            Err(error) => {
                // Strictly-better-than-before fallback: keep the legacy
                // ephemeral subscriber for this stream's subjects so indexing
                // does not stop, but make the durability gap impossible to
                // miss. Once the stream is migrated the same code binds
                // durably with no further change.
                tracing::error!(
                    stream = stream_name,
                    ?subjects,
                    %error,
                    "Quickwit durable consumer REFUSED — falling back to lossy ephemeral \
                     subscription for these subjects. Events published while this service \
                     is down WILL BE LOST. Remediation: the stream must allow a second \
                     consumer (retention `interest`, as DATAPLANE_KNOWLEDGE already uses); \
                     `WorkQueue` retention permits only one consumer per subject and cannot \
                     be altered in place."
                );
                spawn_ephemeral_fallback(&nats, subjects, &ctx, &security).await?;
            }
        }
    }

    Ok(())
}

/// Binds this service's durable pull consumer to an existing stream.
///
/// Deliberately uses `get_stream` (not `get_or_create_stream`): stream
/// lifecycle belongs to the producing service, and creating a stream here with
/// guessed subjects/retention would silently diverge from the producer's
/// definition.
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

/// Legacy no-ack subscriber, retained only as the degraded path when a durable
/// consumer cannot be bound. Behaviourally identical to the pre-JetStream
/// implementation.
async fn spawn_ephemeral_fallback(
    nats: &async_nats::Client,
    subjects: &'static [&'static str],
    ctx: &Arc<RebuildContext>,
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
            tracing::warn!(subject, "Quickwit ephemeral (lossy) subscriber online");
            while let Some(msg) = sub.next().await {
                let decoded = decode_event(&security, msg.subject.as_str(), &msg.payload, false);
                let event = match decoded {
                    Ok(event) => event,
                    Err(error) => {
                        tracing::warn!(subject = %msg.subject, %error, "Quickwit rejected unauthorized live event");
                        continue;
                    }
                };
                if let Err(err) = handle_message(&ctx, msg.subject.as_str(), event).await {
                    tracing::warn!(subject = %msg.subject, error = %err, "Quickwit live update failed");
                }
            }
        });
    }
    Ok(())
}

/// Fetch/ack loop for one durable consumer.
///
/// Ack policy:
/// - decode rejection  → `ack` (terminal; a bad signature never becomes valid)
/// - handler success   → `ack`
/// - handler failure   → `nak` with backoff, until `MAX_DELIVER`, then DLQ + `ack`
///
/// A handler failure is never silently swallowed: it either retries or lands in
/// the DLQ.
async fn run_durable(
    consumer: PullConsumer,
    ctx: Arc<RebuildContext>,
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
                tracing::warn!(stream = stream_name, %error, "Quickwit consumer fetch failed");
                tokio::time::sleep(NAK_BACKOFF).await;
                continue;
            }
        };

        while let Some(msg_result) = messages.next().await {
            let msg = match msg_result {
                Ok(msg) => msg,
                Err(error) => {
                    tracing::warn!(stream = stream_name, %error, "Quickwit consumer message error");
                    continue;
                }
            };

            let subject = msg.subject.as_str().to_string();
            let delivered = msg.info().map(|info| info.delivered).unwrap_or(1);
            let redelivery = delivered > 1;

            let event = match decode_event(&security, &subject, &msg.payload, redelivery) {
                Ok(event) => event,
                Err(error) => {
                    // Unauthorized/undecodable is terminal — redelivering it
                    // would loop to max_deliver for no benefit.
                    tracing::warn!(subject = %subject, %error, "Quickwit rejected unauthorized live event");
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
                        "Quickwit live update failed; will redeliver"
                    );
                    let _ = msg.ack_with(AckKind::Nak(Some(NAK_BACKOFF))).await;
                }
                Err(error) => {
                    tracing::error!(
                        subject = %subject,
                        delivered,
                        %error,
                        "Quickwit live update failed permanently; routing to DLQ"
                    );
                    let dlq = serde_json::json!({
                        "original_subject": subject,
                        "stream": stream_name,
                        "error": error.to_string(),
                        "attempts": delivered,
                    });
                    if let Err(error) = nats.publish(DLQ_SUBJECT, dlq.to_string().into()).await {
                        tracing::error!(%error, "Quickwit DLQ publish failed");
                    }
                    let _ = msg.ack().await;
                }
            }
        }
    }
}

/// Starts the historical raw-JSON subscribers only for the caller's explicit,
/// isolated-development containment branch. Production must use [`spawn`].
pub async fn spawn_unverified_legacy(
    nats: async_nats::Client,
    ctx: Arc<RebuildContext>,
) -> anyhow::Result<()> {
    nats_connection::ensure_or_warn(&jetstream::new(nats.clone())).await;
    for subject in subscribed_subjects() {
        let mut sub = nats
            .subscribe(subject.to_string())
            .await
            .with_context(|| format!("subscribe {subject}"))?;
        let ctx = ctx.clone();
        tokio::spawn(async move {
            while let Some(msg) = sub.next().await {
                let payload: Value = match serde_json::from_slice(&msg.payload) {
                    Ok(payload) => payload,
                    Err(error) => {
                        tracing::warn!(subject = %msg.subject, %error, "legacy Quickwit event decode failed");
                        continue;
                    }
                };
                let org_id = payload
                    .get("org_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                if org_id.is_empty() {
                    tracing::warn!(subject = %msg.subject, "legacy Quickwit event missing tenant");
                    continue;
                }
                let event = DecodedEvent {
                    claims: EventClaims {
                        iss: "insecure-isolated-legacy".into(),
                        sub: "insecure-isolated-legacy".into(),
                        aud: "insecure-isolated-legacy".into(),
                        principal_type: "service".into(),
                        org_id,
                        user_id: payload
                            .get("user_id")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        scopes: Vec::new(),
                        zdr: payload.get("zdr").and_then(Value::as_bool).unwrap_or(false),
                        event_type: msg.subject.to_string(),
                        payload_sha256: String::new(),
                        jti: String::new(),
                        iat: 0,
                        nbf: 0,
                        exp: 0,
                    },
                    payload,
                };
                if let Err(error) = handle_message(&ctx, msg.subject.as_str(), event).await {
                    tracing::warn!(subject = %msg.subject, %error, "legacy Quickwit update failed");
                }
            }
        });
    }
    Ok(())
}

async fn handle_message(
    ctx: &Arc<RebuildContext>,
    subject: &str,
    event: DecodedEvent,
) -> anyhow::Result<()> {
    let org_id = event.claims.org_id.as_str();
    let value = event.payload;

    if event.claims.zdr
        && matches!(
            subject,
            SUBJECT_KNOWLEDGE_CREATED
                | SUBJECT_DOCUMENT_INDEXED
                | SUBJECT_WIKI_PUBLISHED
                | SUBJECT_SOURCE_OBJECT_CHANGED
        )
    {
        tracing::warn!(
            org_id,
            subject,
            "restrictive-ZDR Quickwit persistence event rejected"
        );
        return Ok(());
    }

    match subject {
        SUBJECT_KNOWLEDGE_CREATED => {
            if let Some(knowledge_id) = value.get("knowledge_id").and_then(Value::as_str) {
                let indexed =
                    rebuild::index_knowledge_unit_by_id(ctx, org_id, knowledge_id).await?;
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
                // Clear prior FTS entries for this document before re-indexing so a
                // re-index after a content update reflects exactly the current
                // knowledge units — no duplicate chunks and no orphans left behind
                // from the previous version. (No-op on first index.)
                ctx.quickwit
                    .delete_by_query(&tenant_id_query(org_id, "document_id", document_id))
                    .await?;
                let count =
                    rebuild::index_document_knowledge_units(ctx, org_id, document_id).await?;
                tracing::info!(
                    document_id,
                    count,
                    "re-indexed document knowledge units in Quickwit"
                );
            }
        }
        SUBJECT_DOCUMENT_DELETED => {
            if let Some(document_id) = value.get("document_id").and_then(Value::as_str) {
                ctx.quickwit
                    .delete_by_query(&tenant_id_query(org_id, "document_id", document_id))
                    .await?;
            }
        }
        SUBJECT_WIKI_PUBLISHED => {
            rebuild::index_wiki_event(ctx, org_id, &value).await?;
        }
        SUBJECT_SOURCE_OBJECT_CHANGED => {
            if let Some(source_object_id) = value.get("source_object_id").and_then(Value::as_str) {
                let indexed =
                    rebuild::index_source_object_by_id(ctx, org_id, source_object_id).await?;
                if !indexed {
                    tracing::debug!(source_object_id, "source object not found; skipping");
                }
            }
        }
        SUBJECT_SOURCE_OBJECT_DELETED => {
            if let Some(source_object_id) = value.get("source_object_id").and_then(Value::as_str) {
                ctx.quickwit
                    .delete_by_query(&tenant_id_query(
                        org_id,
                        "source_object_id",
                        source_object_id,
                    ))
                    .await?;
            }
        }
        _ => tracing::debug!(subject, "ignoring unknown subject"),
    }

    Ok(())
}

fn tenant_id_query(org_id: &str, field: &str, id: &str) -> String {
    format!(
        "org_id:{} AND {field}:{}",
        quote_query_value(org_id),
        quote_query_value(id)
    )
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

    #[test]
    fn destructive_rebuild_subject_is_not_subscribed() {
        assert!(!subscribed_subjects().contains(&"dataplane.search.rebuild.requested"));
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
        let wiki = producer(
            "service:wiki-store-go",
            "wiki-events-v1",
            "events:wiki:publish",
        );
        let security = EventSecurity::new(
            documents.verifier,
            index.verifier,
            embedding.verifier,
            wiki.verifier,
        );
        let raw = br#"{"org_id":"org-test","document_id":"doc-test","zdr":false}"#;

        assert!(decode_event(&security, SUBJECT_DOCUMENT_DELETED, raw, false).is_err());

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
        assert!(
            decode_event(&security, SUBJECT_DOCUMENT_DELETED, &wrong_producer, false,).is_err()
        );

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
        let wiki = producer(
            "service:wiki-store-go",
            "wiki-events-v1",
            "events:wiki:publish",
        );
        let security = EventSecurity::new(
            documents.verifier,
            index.verifier,
            embedding.verifier,
            wiki.verifier,
        );
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

        let event = decode_event(&security, SUBJECT_DOCUMENT_DELETED, &envelope, false)
            .expect("verified event");
        assert_eq!(event.claims.org_id, "org-test");
        assert_eq!(event.payload["document_id"], "doc-test");
        assert!(!event.claims.zdr);
    }
}
