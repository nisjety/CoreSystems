use std::sync::Arc;
use std::time::Duration;

use async_nats::jetstream::{self, consumer::PullConsumer, Context as JsContext};
use futures::StreamExt;

use crate::extractor::GraphExtractor;
use crate::store::GraphStore;

// Each engine owns disjoint subjects in JetStream. graph-index owns only
// `dataplane.documents.indexed` (its trigger) — the other subjects belong to
// DATAPLANE_DOCUMENTS / DATAPLANE_KNOWLEDGE streams.
const STREAM_NAME: &str = "DATAPLANE_GRAPH";
const SUBJECT: &str = "dataplane.documents.indexed";
const CONSUMER_NAME: &str = "graph-index";
const DLQ_SUBJECT: &str = "dataplane.dlq.graph-index";
// Owned by the DATAPLANE_KNOWLEDGE stream; observed here via core NATS so we
// don't claim the subject for a second JetStream stream.
const SUBJECT_KNOWLEDGE_DELETED: &str = "dataplane.knowledge.units.deleted";

/// Subscribes (core NATS) to knowledge.units.deleted and purges the graph
/// mappings for chunks orphaned by a content re-chunk, keeping superseded
/// content out of graph retrieval. Best-effort: a missed message leaves
/// dangling mappings that the next update or a GC pass clears.
pub async fn spawn_orphan_cleanup(
    nats: async_nats::Client,
    store: Arc<GraphStore>,
) -> anyhow::Result<()> {
    let mut sub = nats
        .subscribe(SUBJECT_KNOWLEDGE_DELETED.to_string())
        .await?;
    tokio::spawn(async move {
        tracing::info!(
            subject = SUBJECT_KNOWLEDGE_DELETED,
            "graph orphan-cleanup subscriber online"
        );
        while let Some(msg) = sub.next().await {
            let payload: serde_json::Value = match serde_json::from_slice(&msg.payload) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(err = %e, "invalid knowledge.units.deleted payload");
                    continue;
                }
            };
            let org_id = payload["org_id"].as_str().unwrap_or("");
            let kids: Vec<String> = payload["knowledge_ids"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            if org_id.is_empty() || kids.is_empty() {
                continue;
            }
            match store.delete_text_unit_mappings(org_id, &kids).await {
                Ok(removed) => {
                    tracing::info!(removed, org_id, "purged orphaned graph mappings")
                }
                Err(e) => tracing::error!(err = %e, "purge orphaned graph mappings failed"),
            }
        }
    });
    Ok(())
}

pub async fn setup_stream(js: &JsContext) -> anyhow::Result<()> {
    let config = jetstream::stream::Config {
        name: STREAM_NAME.to_string(),
        subjects: vec![SUBJECT.to_string()],
        retention: jetstream::stream::RetentionPolicy::WorkQueue,
        max_age: Duration::from_secs(7 * 24 * 3600),
        ..Default::default()
    };
    js.get_or_create_stream(config).await?;
    Ok(())
}

pub async fn create_consumer(js: &JsContext) -> anyhow::Result<PullConsumer> {
    let stream = js.get_stream(STREAM_NAME).await?;
    let consumer = stream
        .get_or_create_consumer(
            CONSUMER_NAME,
            jetstream::consumer::pull::Config {
                durable_name: Some(CONSUMER_NAME.to_string()),
                filter_subjects: vec![SUBJECT.to_string()],
                ack_wait: Duration::from_secs(120),
                max_deliver: 3,
                ..Default::default()
            },
        )
        .await?;
    Ok(consumer)
}

pub async fn run_consumer(
    consumer: PullConsumer,
    store: Arc<GraphStore>,
    extractor: Arc<GraphExtractor>,
    pool: sqlx::PgPool,
    nats: async_nats::Client,
) -> anyhow::Result<()> {
    loop {
        let mut messages = consumer
            .fetch()
            .max_messages(1)
            .expires(Duration::from_secs(5))
            .messages()
            .await?;

        while let Some(msg_result) = messages.next().await {
            let msg = match msg_result {
                Ok(m) => m,
                Err(e) => {
                    tracing::error!(err = %e, "receive error");
                    continue;
                }
            };

            let payload: serde_json::Value = match serde_json::from_slice(&msg.payload) {
                Ok(v) => v,
                Err(e) => {
                    tracing::error!(err = %e, "invalid payload");
                    let _ = msg.ack().await;
                    continue;
                }
            };

            let doc_id = payload["document_id"].as_str().unwrap_or("");
            let org_id = payload["org_id"].as_str().unwrap_or("");

            if doc_id.is_empty() || org_id.is_empty() {
                let _ = msg.ack().await;
                continue;
            }

            tracing::info!(
                document_id = doc_id,
                org_id,
                "processing document for graph extraction"
            );

            let chunks: Vec<(String, String)> = sqlx::query_as(
                "SELECT knowledge_id, text FROM knowledge_units WHERE document_id = $1 AND org_id = $2"
            )
            .bind(doc_id)
            .bind(org_id)
            .fetch_all(&pool)
            .await
            .unwrap_or_default();

            let mut total_entities = 0usize;
            let mut total_rels = 0usize;
            let mut total_claims = 0usize;

            for (kid, text) in &chunks {
                match extractor.extract(text, org_id).await {
                    Ok(result) => match store.persist_extraction(org_id, kid, &result).await {
                        Ok((eids, rids, cids)) => {
                            if let Err(e) = store
                                .persist_text_unit_mappings(org_id, kid, &eids, &rids, &cids)
                                .await
                            {
                                tracing::error!(err = %e, knowledge_id = kid, "persist text_unit mappings failed");
                            }
                            total_entities += eids.len();
                            total_rels += rids.len();
                            total_claims += cids.len();
                        }
                        Err(e) => {
                            tracing::error!(err = %e, knowledge_id = kid, "persist extraction failed")
                        }
                    },
                    Err(e) => tracing::error!(err = %e, knowledge_id = kid, "extraction failed"),
                }
            }

            let all_failed =
                total_entities == 0 && total_rels == 0 && total_claims == 0 && !chunks.is_empty();
            if all_failed {
                let num_delivered = msg.info().map(|i| i.delivered).unwrap_or(0) as u32;
                if num_delivered >= 3 {
                    tracing::warn!(
                        document_id = doc_id,
                        "all extractions failed, sending to DLQ"
                    );
                    let dlq = serde_json::json!({
                        "original_subject": SUBJECT,
                        "document_id": doc_id,
                        "org_id": org_id,
                        "error": "all chunk extractions failed",
                        "attempts": num_delivered,
                    });
                    let _ = nats
                        .publish(
                            DLQ_SUBJECT,
                            serde_json::to_vec(&dlq).unwrap_or_default().into(),
                        )
                        .await;
                }
            }

            tracing::info!(
                document_id = doc_id,
                entities = total_entities,
                relationships = total_rels,
                claims = total_claims,
                "graph extraction complete"
            );

            let _ = msg.ack().await;
        }
    }
}
