//! §16.3.8 — wiki publish subscriber.
//!
//! wiki-store-go emits `dataplane.wiki.version.published` on every published
//! version (CreatePage / CreateVersion). We subscribe to it as a core-NATS
//! subscription (not JetStream — the wiki publisher publishes core, and
//! embedding for wiki is a best-effort enrichment, not a durable contract).
//!
//! For each event we:
//!   1. Embed `content` via the existing EmbeddingProvider.
//!   2. Upsert one point into the `wiki_block_embeddings` Qdrant collection
//!      keyed by `version_id`, with payload {org_id, page_id, workspace_id,
//!      title, path}.
//!
//! Failure (embed timeout, Qdrant down) is logged and skipped — the next
//! version_published event for that page will overwrite the point.

use anyhow::Context;
use futures::StreamExt;
use serde::Deserialize;

use crate::provider::EmbeddingProvider;

pub const SUBJECT_WIKI_PUBLISHED: &str = "dataplane.wiki.version.published";
pub const WIKI_COLLECTION: &str = "wiki_block_embeddings";

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
    nats: async_nats::Client,
    qdrant: qdrant_client::Qdrant,
    provider: EmbeddingProvider,
) -> anyhow::Result<()> {
    let mut sub = nats
        .subscribe(SUBJECT_WIKI_PUBLISHED.to_string())
        .await
        .context("subscribe wiki.published")?;
    tracing::info!(subject = SUBJECT_WIKI_PUBLISHED, "wiki subscriber online");

    tokio::spawn(async move {
        while let Some(msg) = sub.next().await {
            let evt: WikiPublishedEvent = match serde_json::from_slice(&msg.payload) {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!(error = %e, "invalid wiki.published payload");
                    continue;
                }
            };
            if let Err(e) = handle(&evt, &qdrant, &provider).await {
                tracing::warn!(
                    error = %e,
                    version_id = %evt.version_id,
                    "wiki embed failed (will retry on next publish)"
                );
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

    let vecs = provider
        .embed_batch(&evt.org_id, &[evt.content.clone()])
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

    let point = PointStruct::new(evt.version_id.clone(), vec, payload);
    qdrant
        .upsert_points(UpsertPointsBuilder::new(WIKI_COLLECTION, vec![point]).wait(true))
        .await
        .context("qdrant upsert wiki point")?;
    Ok(())
}
