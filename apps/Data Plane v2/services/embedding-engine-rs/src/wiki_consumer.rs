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

use std::time::Duration;

use anyhow::Context;
use async_nats::jetstream::{self, Context as JsContext};
use futures::StreamExt;
use serde::Deserialize;

use crate::provider::EmbeddingProvider;

pub const SUBJECT_WIKI_PUBLISHED: &str = "dataplane.wiki.version.published";
pub const WIKI_COLLECTION: &str = "wiki_block_embeddings";
pub const WIKI_STREAM: &str = "DATAPLANE_WIKI";
pub const WIKI_CONSUMER: &str = "embedding-engine-wiki";

#[derive(Debug, Deserialize)]
struct WikiPublishedEvent {
    page_id: String,
    version_id: String,
    org_id: String,
    workspace_id: String,
    title: String,
    path: String,
    content: String,
}

pub async fn spawn(
    js: JsContext,
    qdrant: qdrant_client::Qdrant,
    provider: EmbeddingProvider,
) -> anyhow::Result<()> {
    // Disjoint subject → its own stream (JetStream requires a subject belong
    // to exactly one stream). WorkQueue: a message is removed once acked.
    js.get_or_create_stream(jetstream::stream::Config {
        name: WIKI_STREAM.to_string(),
        subjects: vec![SUBJECT_WIKI_PUBLISHED.to_string()],
        retention: jetstream::stream::RetentionPolicy::WorkQueue,
        max_age: Duration::from_secs(7 * 24 * 3600),
        ..Default::default()
    })
    .await
    .context("create DATAPLANE_WIKI stream")?;

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
                let evt: WikiPublishedEvent = match serde_json::from_slice(&msg.payload) {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::warn!(error = %e, "invalid wiki.published payload; acking poison");
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

async fn handle(
    evt: &WikiPublishedEvent,
    qdrant: &qdrant_client::Qdrant,
    provider: &EmbeddingProvider,
) -> anyhow::Result<()> {
    use qdrant_client::qdrant::{PointStruct, UpsertPointsBuilder, Value as QdrantValue};

    // Wiki pages carry no `zdr_classification` (the ZDR doc path is the
    // `documents` table). No ZDR signal exists on this subject, so the egress
    // guard does not apply here.
    let vecs = provider
        .embed_batch(&evt.org_id, std::slice::from_ref(&evt.content), false)
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
