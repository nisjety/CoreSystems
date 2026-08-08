//! §16.3.8 — wiki publish subscriber (DURABLE JetStream).
//!
//! wiki-store-go emits `dataplane.wiki.version.published` (core NATS) on every
//! published version (CreatePage / CreateVersion). We own a durable JetStream
//! stream (`DATAPLANE_WIKI`) that captures + persists that subject, and pull
//! from it with a durable consumer so wiki embeds survive restarts and RETRY
//! on transient failure (was best-effort core-NATS — silently lossy if the
//! subscriber was offline or the embed failed).
//!
//! For each event we:
//!   1. Embed `content` via the existing EmbeddingProvider.
//!   2. Upsert one point into the `wiki_block_embeddings` Qdrant collection
//!      keyed by `version_id`, with payload {org_id, page_id, workspace_id,
//!      title, path}.
//!
//! Success → ack. Transient failure → no ack → JetStream redelivers (up to
//! max_deliver). Poison payload → ack (don't redeliver forever).

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use async_nats::jetstream::{self, Context as JsContext};
use event_envelope_rs::EventVerifier;
use futures::StreamExt;
use serde::Deserialize;

use crate::provider::EmbeddingProvider;

pub const SUBJECT_WIKI_PUBLISHED: &str = "dataplane.wiki.version.published";
pub const WIKI_COLLECTION: &str = "wiki_block_embeddings";
pub const WIKI_STREAM: &str = "DATAPLANE_WIKI";
pub const WIKI_CONSUMER: &str = "embedding-engine-wiki";
const WIKI_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

/// The desired stream configuration. Split out from [`ensure_wiki_stream`] so
/// the retention invariant can be asserted in a unit test without a live
/// broker — see `wiki_retention_is_interest_not_workqueue`.
fn wiki_stream_config() -> jetstream::stream::Config {
    jetstream::stream::Config {
        name: WIKI_STREAM.to_string(),
        subjects: vec![SUBJECT_WIKI_PUBLISHED.to_string()],
        // `Interest`, not `WorkQueue`: the published wiki version fans out to
        // BOTH this embedding consumer and quickwit-adapter's lexical index.
        // WorkQueue permits exactly one consumer per subject, which silently
        // refuses the second reader and forces it onto a lossy core-NATS
        // subscription. `Interest` still removes a message once every
        // registered durable consumer has acked it.
        retention: jetstream::stream::RetentionPolicy::Interest,
        max_age: WIKI_MAX_AGE,
        ..Default::default()
    }
}

/// Create or converge `DATAPLANE_WIKI`. Idempotent and safe to call on every
/// boot.
///
/// `get_or_create_stream` alone is not enough: if the stream already exists —
/// with ANY retention, including a wrong one — it is returned as-is and the
/// desired config passed in is silently discarded. That is exactly how this
/// stream spent a day on `WorkQueue` retention after an unrelated migration
/// recreated it: nothing here ever looked at what actually came back, so
/// nothing ever noticed or complained, and quickwit-adapter's second reader
/// was quietly refused and fell back to a lossy subscription the whole time.
///
/// Retention is immutable after creation (attempting `update_stream` with a
/// changed retention field fails the WHOLE call — see `nats_connection::dlq`,
/// which hit the same constraint first), so a retention mismatch is reported
/// loudly via `tracing::error!` rather than "fixed": the only real fix is an
/// operator deleting and recreating the stream while it is empty, which quite
/// deliberately makes it visible instead of silent.
///
/// Errors here still propagate: unlike the DLQ (ancillary, best-effort), the
/// wiki embedding path genuinely cannot work without this stream, so a broker
/// that cannot be reached at all should still fail `spawn`.
async fn ensure_wiki_stream(js: &JsContext) -> anyhow::Result<()> {
    let desired = wiki_stream_config();
    let stream = js
        .get_or_create_stream(desired)
        .await
        .context("create DATAPLANE_WIKI stream")?;
    let current = stream.cached_info().config.clone();

    if current.retention != jetstream::stream::RetentionPolicy::Interest {
        tracing::error!(
            stream = WIKI_STREAM,
            retention = ?current.retention,
            "DATAPLANE_WIKI exists with non-Interest retention; a second durable \
             consumer (quickwit-adapter) will be refused and fall back to a lossy \
             subscription. Retention is immutable — delete and recreate the stream \
             (only safe while it holds zero messages) to repair."
        );
    }

    if let Some(upgraded) = upgraded_wiki_config(&current) {
        js.update_stream(upgraded)
            .await
            .context("converge DATAPLANE_WIKI stream limits")?;
        tracing::info!(stream = WIKI_STREAM, "wiki stream limits converged");
    }

    Ok(())
}

/// Which of the *mutable* settings need converging on an already-existing
/// stream. Retention is deliberately absent: it cannot be updated in place,
/// and attempting it makes `update_stream` fail the whole call.
fn upgraded_wiki_config(current: &jetstream::stream::Config) -> Option<jetstream::stream::Config> {
    let mut upgraded = current.clone();
    let mut changed = false;

    if !upgraded
        .subjects
        .iter()
        .any(|subject| subject == SUBJECT_WIKI_PUBLISHED)
    {
        upgraded.subjects.push(SUBJECT_WIKI_PUBLISHED.to_string());
        changed = true;
    }
    // Only ever *lengthen* retention here. An operator who deliberately
    // widened it on a live broker should not have that undone by the next
    // deploy.
    if upgraded.max_age < WIKI_MAX_AGE {
        upgraded.max_age = WIKI_MAX_AGE;
        changed = true;
    }

    changed.then_some(upgraded)
}

#[derive(Debug, Deserialize)]
struct WikiPublishedEvent {
    page_id: String,
    version_id: String,
    org_id: String,
    workspace_id: String,
    title: String,
    path: String,
    content: String,
    #[serde(default)]
    user_id: Option<String>,
    zdr: bool,
}

pub async fn spawn(
    js: JsContext,
    qdrant: qdrant_client::Qdrant,
    provider: EmbeddingProvider,
    verifier: Arc<EventVerifier>,
) -> anyhow::Result<()> {
    // Disjoint subject → its own stream (JetStream requires a subject belong
    // to exactly one stream).
    ensure_wiki_stream(&js).await?;

    let stream = js
        .get_stream(WIKI_STREAM)
        .await
        .context("get wiki stream")?;
    let consumer = stream
        .get_or_create_consumer(
            WIKI_CONSUMER,
            jetstream::consumer::pull::Config {
                durable_name: Some(WIKI_CONSUMER.to_string()),
                filter_subjects: vec![SUBJECT_WIKI_PUBLISHED.to_string()],
                ack_wait: Duration::from_secs(60),
                max_deliver: 5,
                ..Default::default()
            },
        )
        .await
        .context("create wiki durable consumer")?;
    tracing::info!(
        subject = SUBJECT_WIKI_PUBLISHED,
        stream = WIKI_STREAM,
        "wiki durable subscriber online"
    );

    tokio::spawn(async move {
        loop {
            let mut messages = match consumer.messages().await {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!(error = %e, "wiki consumer stream open failed; retrying");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };
            while let Some(item) = messages.next().await {
                let msg = match item {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!(error = %e, "wiki message recv error");
                        continue;
                    }
                };
                let redelivery = msg.info().is_ok_and(|info| info.delivered > 1);
                let evt = match decode_verified_wiki_event(&verifier, &msg.payload, redelivery) {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::warn!(error = %e, "unverified wiki.published event; acking poison");
                        let _ = msg.ack().await;
                        continue;
                    }
                };
                match handle(&evt, &qdrant, &provider).await {
                    Ok(()) => {
                        let _ = msg.ack().await;
                    }
                    Err(e) => {
                        // No ack → JetStream redelivers (up to max_deliver).
                        tracing::warn!(
                            error = %e,
                            version_id = %evt.version_id,
                            "wiki embed failed; will redeliver"
                        );
                    }
                }
            }
        }
    });
    Ok(())
}

fn decode_verified_wiki_event(
    verifier: &EventVerifier,
    envelope: &[u8],
    redelivery: bool,
) -> anyhow::Result<WikiPublishedEvent> {
    let verified = if redelivery {
        verifier.verify_redelivery(SUBJECT_WIKI_PUBLISHED, envelope)?
    } else {
        verifier.verify(SUBJECT_WIKI_PUBLISHED, envelope)?
    };
    let event: WikiPublishedEvent = serde_json::from_slice(&verified.payload)?;
    anyhow::ensure!(
        !event.page_id.trim().is_empty()
            && !event.version_id.trim().is_empty()
            && !event.org_id.trim().is_empty()
            && !event.content.trim().is_empty(),
        "wiki event required fields are empty"
    );
    anyhow::ensure!(event.org_id == verified.claims.org_id, "tenant mismatch");
    anyhow::ensure!(
        event.user_id.as_deref() == verified.claims.user_id.as_deref(),
        "user mismatch"
    );
    anyhow::ensure!(
        event.zdr == verified.claims.zdr && !event.zdr,
        "invalid ZDR posture"
    );
    Ok(event)
}

async fn handle(
    evt: &WikiPublishedEvent,
    qdrant: &qdrant_client::Qdrant,
    provider: &EmbeddingProvider,
) -> anyhow::Result<()> {
    use qdrant_client::qdrant::{PointStruct, UpsertPointsBuilder, Value as QdrantValue};

    let vecs = provider
        .embed_batch(&evt.org_id, std::slice::from_ref(&evt.content), evt.zdr)
        .await
        .context("embed wiki content")?;
    let vec = vecs.into_iter().next().context("empty embed result")?;
    let mut payload = std::collections::HashMap::new();
    payload.insert(
        "page_id".to_string(),
        QdrantValue::from(evt.page_id.clone()),
    );
    payload.insert(
        "version_id".to_string(),
        QdrantValue::from(evt.version_id.clone()),
    );
    payload.insert("org_id".to_string(), QdrantValue::from(evt.org_id.clone()));
    payload.insert(
        "workspace_id".to_string(),
        QdrantValue::from(evt.workspace_id.clone()),
    );
    payload.insert("title".to_string(), QdrantValue::from(evt.title.clone()));
    payload.insert("path".to_string(), QdrantValue::from(evt.path.clone()));
    // Fields the retrieval-engine's vector_search reads so a wiki point can be
    // a first-class hybrid candidate (text body + source tag + a stable key /
    // document id). Without `text` the candidate would have an empty body.
    payload.insert("text".to_string(), QdrantValue::from(evt.content.clone()));
    payload.insert("source_type".to_string(), QdrantValue::from("wiki"));
    payload.insert(
        "knowledge_id".to_string(),
        QdrantValue::from(evt.version_id.clone()),
    );
    payload.insert(
        "document_id".to_string(),
        QdrantValue::from(evt.page_id.clone()),
    );

    let point = PointStruct::new(evt.version_id.clone(), vec, payload);
    qdrant
        .upsert_points(UpsertPointsBuilder::new(WIKI_COLLECTION, vec![point]).wait(true))
        .await
        .context("qdrant upsert wiki point")?;
    Ok(())
}

#[cfg(test)]
mod stream_config_tests {
    use super::*;

    #[test]
    fn wiki_retention_is_interest_not_workqueue() {
        // The single most important assertion in this module. `WorkQueue`
        // permits exactly one consumer per subject, which is what silently
        // refused quickwit-adapter's reader for a full day and forced it onto
        // a lossy fallback.
        assert_eq!(
            wiki_stream_config().retention,
            jetstream::stream::RetentionPolicy::Interest
        );
        assert_ne!(
            wiki_stream_config().retention,
            jetstream::stream::RetentionPolicy::WorkQueue
        );
    }

    #[test]
    fn upgraded_config_never_touches_retention() {
        // A stream that already exists with the wrong (immutable) retention
        // must not have that field carried into an `update_stream` call —
        // doing so fails the whole call. Construct exactly that drifted state
        // and prove the returned config, if any, still reports the ORIGINAL
        // retention untouched.
        let drifted = jetstream::stream::Config {
            retention: jetstream::stream::RetentionPolicy::WorkQueue,
            max_age: Duration::from_secs(1), // also short, so a mutable fix is expected
            ..wiki_stream_config()
        };

        let upgraded = upgraded_wiki_config(&drifted).expect("max_age should converge");
        assert_eq!(
            upgraded.retention,
            jetstream::stream::RetentionPolicy::WorkQueue,
            "upgrade path must never carry a retention change into update_stream"
        );
        assert_eq!(upgraded.max_age, WIKI_MAX_AGE);
    }

    #[test]
    fn matching_config_needs_no_update() {
        assert!(upgraded_wiki_config(&wiki_stream_config()).is_none());
    }

    #[test]
    fn wider_operator_set_max_age_is_not_shortened() {
        let widened = jetstream::stream::Config {
            max_age: WIKI_MAX_AGE * 4,
            ..wiki_stream_config()
        };
        assert!(upgraded_wiki_config(&widened).is_none());
    }

    #[test]
    fn missing_subject_is_restored() {
        let stripped = jetstream::stream::Config {
            subjects: vec![],
            ..wiki_stream_config()
        };
        let upgraded = upgraded_wiki_config(&stripped).expect("subject should be restored");
        assert_eq!(upgraded.subjects, vec![SUBJECT_WIKI_PUBLISHED.to_string()]);
    }
}

#[cfg(test)]
mod security_tests {
    use super::*;
    use event_envelope_rs::{EventSigner, EventVerifier};
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
                "service:wiki-store-go",
                "wiki-events-v1",
                "dataplane-events",
                "events:wiki:publish",
            )
            .expect("signer"),
            EventVerifier::from_rsa_pem(
                public_pem.as_bytes(),
                "service:wiki-store-go",
                "wiki-events-v1",
                "dataplane-events",
                "events:wiki:publish",
                10,
            )
            .expect("verifier"),
        )
    }

    #[test]
    fn signed_wiki_decoder_pins_tenant_user_and_non_zdr_posture() {
        let (signer, verifier) = contract();
        let payload = br#"{"page_id":"page-test","version_id":"version-test","org_id":"org-test","workspace_id":"workspace-test","title":"title","path":"/path","content":"content","user_id":"user-test","zdr":false}"#;
        let envelope = signer
            .sign(
                SUBJECT_WIKI_PUBLISHED,
                "org-test",
                Some("user-test"),
                false,
                payload,
            )
            .expect("sign");
        let event = decode_verified_wiki_event(&verifier, &envelope, false).expect("verify");
        assert_eq!(event.org_id, "org-test");
        assert_eq!(event.user_id.as_deref(), Some("user-test"));
        assert!(!event.zdr);
    }

    #[test]
    fn signed_wiki_decoder_rejects_plain_json_tampering_and_replay() {
        let (signer, verifier) = contract();
        let payload = br#"{"page_id":"page-test","version_id":"version-test","org_id":"org-test","workspace_id":"workspace-test","title":"title","path":"/path","content":"content","zdr":false}"#;
        assert!(decode_verified_wiki_event(&verifier, payload, false).is_err());
        let envelope = signer
            .sign(SUBJECT_WIKI_PUBLISHED, "org-test", None, false, payload)
            .expect("sign");
        assert!(decode_verified_wiki_event(&verifier, &envelope, false).is_ok());
        assert!(decode_verified_wiki_event(&verifier, &envelope, false).is_err());
        assert!(decode_verified_wiki_event(&verifier, &envelope, true).is_ok());

        let mut tampered: serde_json::Value = serde_json::from_slice(&envelope).expect("json");
        tampered["data"] = serde_json::Value::String("e30".into());
        assert!(decode_verified_wiki_event(
            &verifier,
            &serde_json::to_vec(&tampered).expect("json"),
            false,
        )
        .is_err());
    }
}
