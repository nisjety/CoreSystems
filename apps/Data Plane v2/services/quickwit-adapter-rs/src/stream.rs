use std::sync::Arc;

use anyhow::Context;
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
pub const SUBJECT_SEARCH_REBUILD_REQUESTED: &str = "dataplane.search.rebuild.requested";

pub async fn spawn(nats: async_nats::Client, ctx: Arc<RebuildContext>) -> anyhow::Result<()> {
    for subject in [
        SUBJECT_KNOWLEDGE_CREATED,
        SUBJECT_DOCUMENT_INDEXED,
        SUBJECT_DOCUMENT_DELETED,
        SUBJECT_WIKI_PUBLISHED,
        SUBJECT_SOURCE_OBJECT_CHANGED,
        SUBJECT_SOURCE_OBJECT_DELETED,
        SUBJECT_SEARCH_REBUILD_REQUESTED,
    ] {
        let mut sub = nats
            .subscribe(subject.to_string())
            .await
            .with_context(|| format!("subscribe {subject}"))?;
        let ctx = ctx.clone();
        tokio::spawn(async move {
            tracing::info!(subject, "Quickwit live subscriber online");
            while let Some(msg) = sub.next().await {
                if let Err(err) = handle_message(&ctx, msg.subject.as_str(), &msg.payload).await {
                    tracing::warn!(subject = %msg.subject, error = %err, "Quickwit live update failed");
                }
            }
        });
    }

    Ok(())
}

async fn handle_message(
    ctx: &Arc<RebuildContext>,
    subject: &str,
    payload: &[u8],
) -> anyhow::Result<()> {
    let value: Value = serde_json::from_slice(payload).context("decode NATS payload")?;

    match subject {
        SUBJECT_KNOWLEDGE_CREATED => {
            if let Some(knowledge_id) = value.get("knowledge_id").and_then(Value::as_str) {
                let indexed = rebuild::index_knowledge_unit_by_id(ctx, knowledge_id).await?;
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
                    .delete_by_query(&format!("document_id:{}", quote_query_value(document_id)))
                    .await?;
                let count = rebuild::index_document_knowledge_units(ctx, document_id).await?;
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
                    .delete_by_query(&format!("document_id:{}", quote_query_value(document_id)))
                    .await?;
            }
        }
        SUBJECT_WIKI_PUBLISHED => {
            rebuild::index_wiki_event(ctx, &value).await?;
        }
        SUBJECT_SOURCE_OBJECT_CHANGED => {
            if let Some(source_object_id) = value.get("source_object_id").and_then(Value::as_str) {
                let indexed = rebuild::index_source_object_by_id(ctx, source_object_id).await?;
                if !indexed {
                    tracing::debug!(source_object_id, "source object not found; skipping");
                }
            }
        }
        SUBJECT_SOURCE_OBJECT_DELETED => {
            if let Some(source_object_id) = value.get("source_object_id").and_then(Value::as_str) {
                ctx.quickwit
                    .delete_by_query(&format!(
                        "source_object_id:{}",
                        quote_query_value(source_object_id)
                    ))
                    .await?;
            }
        }
        SUBJECT_SEARCH_REBUILD_REQUESTED => {
            let org_id = value
                .get("org_id")
                .and_then(Value::as_str)
                .filter(|org| !org.is_empty())
                .map(ToString::to_string);
            let clear = value.get("clear").and_then(Value::as_bool).unwrap_or(false);
            let ctx = ctx.clone();
            tokio::spawn(async move {
                if let Err(err) = rebuild::rebuild_all(ctx, org_id, clear).await {
                    tracing::error!(error = %err, "event-driven Quickwit rebuild failed");
                }
            });
        }
        _ => tracing::debug!(subject, "ignoring unknown subject"),
    }

    Ok(())
}
