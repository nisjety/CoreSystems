use std::time::Duration;

use async_nats::jetstream::{self, consumer::PullConsumer, Context as JsContext};

use crate::builder::{self, DocumentEvent};
use crate::chunker::ChunkConfig;
use crate::config::Config;

const STREAM_NAME: &str = "DATAPLANE_DOCUMENTS";
const SUBJECT_CREATED: &str = "dataplane.documents.created";
const SUBJECT_UPDATED: &str = "dataplane.documents.updated";
const SUBJECT_DELETED: &str = "dataplane.documents.deleted";
const CONSUMER_NAME: &str = "index-engine";
const DLQ_SUBJECT: &str = "dataplane.dlq.index-engine";
// Published when a rebuild orphans prior chunks; embedding-engine purges the
// corresponding Qdrant points so stale content can't surface in retrieval.
const SUBJECT_KNOWLEDGE_DELETED: &str = "dataplane.knowledge.units.deleted";

pub async fn setup_stream(js: &JsContext) -> anyhow::Result<()> {
    let config = jetstream::stream::Config {
        name: STREAM_NAME.to_string(),
        subjects: vec![
            SUBJECT_CREATED.to_string(),
            SUBJECT_UPDATED.to_string(),
            SUBJECT_DELETED.to_string(),
        ],
        retention: jetstream::stream::RetentionPolicy::WorkQueue,
        max_age: Duration::from_secs(7 * 24 * 3600), // 7 days
        ..Default::default()
    };

    js.get_or_create_stream(config).await?;
    tracing::info!("NATS JetStream stream ready: {STREAM_NAME}");
    Ok(())
}

pub async fn create_consumer(js: &JsContext) -> anyhow::Result<PullConsumer> {
    let stream = js.get_stream(STREAM_NAME).await?;
    let consumer = stream
        .get_or_create_consumer(
            CONSUMER_NAME,
            jetstream::consumer::pull::Config {
                durable_name: Some(CONSUMER_NAME.to_string()),
                filter_subjects: vec![
                    SUBJECT_CREATED.to_string(),
                    SUBJECT_UPDATED.to_string(),
                    SUBJECT_DELETED.to_string(),
                ],
                ack_wait: Duration::from_secs(30),
                max_deliver: 5,
                ..Default::default()
            },
        )
        .await?;
    tracing::info!("NATS consumer ready: {CONSUMER_NAME}");
    Ok(consumer)
}

pub async fn run_consumer(
    consumer: PullConsumer,
    pool: sqlx::PgPool,
    config: Config,
    nats_client: async_nats::Client,
) -> anyhow::Result<()> {
    let chunk_config = ChunkConfig {
        chunk_size: config.chunk_size,
        chunk_overlap: config.chunk_overlap,
    };

    loop {
        let mut messages = consumer
            .fetch()
            .max_messages(config.batch_size)
            .expires(Duration::from_secs(2))
            .messages()
            .await?;

        while let Some(msg_result) = futures::StreamExt::next(&mut messages).await {
            let msg = match msg_result {
                Ok(m) => m,
                Err(e) => {
                    tracing::error!(err = %e, "failed to receive message");
                    continue;
                }
            };

            let subject = msg.subject.as_str().to_string();
            let payload: serde_json::Value = match serde_json::from_slice(&msg.payload) {
                Ok(v) => v,
                Err(e) => {
                    tracing::error!(err = %e, subject = %subject, "invalid message payload");
                    let _ = msg.ack().await;
                    continue;
                }
            };

            match subject.as_str() {
                SUBJECT_CREATED | SUBJECT_UPDATED => {
                    let event = DocumentEvent {
                        document_id: payload["document_id"].as_str().unwrap_or("").to_string(),
                        org_id: payload["org_id"].as_str().unwrap_or("").to_string(),
                        content: payload["content"].as_str().unwrap_or("").to_string(),
                        title: payload["title"].as_str().unwrap_or("").to_string(),
                        source: payload["source"].as_str().unwrap_or("").to_string(),
                        doc_type: payload["type"].as_str().unwrap_or("").to_string(),
                    };

                    match builder::process_document(&pool, &event, &chunk_config).await {
                        Ok(result) => {
                            for kid in &result.knowledge_ids {
                                let embed_event = serde_json::json!({
                                    "knowledge_id": kid,
                                    "document_id": event.document_id,
                                    "org_id": event.org_id,
                                    "text": "",
                                });
                                let _ = nats_client
                                    .publish(
                                        "dataplane.knowledge.units.created",
                                        serde_json::to_vec(&embed_event)?.into(),
                                    )
                                    .await;
                            }

                            // Purge vectors orphaned by a re-chunk (changed or
                            // removed chunks). Empty on first build, so this is a
                            // no-op for documents.created.
                            if !result.orphaned_knowledge_ids.is_empty() {
                                let del_event = serde_json::json!({
                                    "document_id": event.document_id,
                                    "org_id": event.org_id,
                                    "knowledge_ids": result.orphaned_knowledge_ids,
                                });
                                let _ = nats_client
                                    .publish(
                                        SUBJECT_KNOWLEDGE_DELETED,
                                        serde_json::to_vec(&del_event)?.into(),
                                    )
                                    .await;
                            }
                            let _ = msg.ack().await;
                        }
                        Err(e) => {
                            tracing::error!(
                                err = %e,
                                document_id = %event.document_id,
                                "failed to process document"
                            );
                            let num_delivered = msg.info().map(|i| i.delivered).unwrap_or(0) as u32;
                            if num_delivered >= config.max_delivery_attempts {
                                tracing::warn!(document_id = %event.document_id, "max retries reached, sending to DLQ");
                                let dlq = serde_json::json!({
                                    "original_subject": subject.as_str(),
                                    "document_id": event.document_id,
                                    "org_id": event.org_id,
                                    "error": e.to_string(),
                                    "attempts": num_delivered,
                                });
                                let _ = nats_client
                                    .publish(
                                        DLQ_SUBJECT,
                                        serde_json::to_vec(&dlq).unwrap_or_default().into(),
                                    )
                                    .await;
                                let _ = msg.ack().await;
                            }
                        }
                    }
                }
                SUBJECT_DELETED => {
                    let doc_id = payload["document_id"].as_str().unwrap_or("");
                    if let Err(e) = builder::handle_document_deleted(&pool, doc_id).await {
                        tracing::error!(err = %e, document_id = doc_id, "failed to handle delete");
                    }
                    let _ = msg.ack().await;
                }
                _ => {
                    tracing::warn!(subject = %subject, "unknown subject");
                    let _ = msg.ack().await;
                }
            }
        }
    }
}
