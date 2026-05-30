use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_nats::jetstream::{self, consumer::PullConsumer, Context as JsContext};
use futures::StreamExt;

use crate::batch::{self, BatchItem};
use crate::config::Config;
use crate::qdrant_writer;

// Each engine owns its own JetStream with disjoint subjects — JetStream
// requires that a subject belongs to exactly one stream. `documents.deleted`
// is owned by the DATAPLANE_DOCUMENTS stream (index-engine); we observe it
// via core NATS subscribe in the consumer loop instead of a JS subscription.
const STREAM_NAME: &str = "DATAPLANE_KNOWLEDGE";
const SUBJECT_CREATED: &str = "dataplane.knowledge.units.created";
const SUBJECT_KNOWLEDGE_DELETED: &str = "dataplane.knowledge.units.deleted";
const SUBJECT_DOC_DELETED: &str = "dataplane.documents.deleted";
const CONSUMER_NAME: &str = "embedding-engine";
const DLQ_SUBJECT: &str = "dataplane.dlq.embedding-engine";

pub async fn setup_stream(js: &JsContext) -> anyhow::Result<()> {
    let config = jetstream::stream::Config {
        name: STREAM_NAME.to_string(),
        subjects: vec![
            SUBJECT_CREATED.to_string(),
            SUBJECT_KNOWLEDGE_DELETED.to_string(),
        ],
        retention: jetstream::stream::RetentionPolicy::WorkQueue,
        max_age: Duration::from_secs(7 * 24 * 3600),
        ..Default::default()
    };
    js.get_or_create_stream(config).await?;
    tracing::info!("NATS stream ready: {STREAM_NAME}");
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
                    SUBJECT_KNOWLEDGE_DELETED.to_string(),
                ],
                ack_wait: Duration::from_secs(60),
                max_deliver: 5,
                ..Default::default()
            },
        )
        .await?;
    Ok(consumer)
}

pub async fn run_consumer(
    consumer: PullConsumer,
    pool: sqlx::PgPool,
    qdrant: qdrant_client::Qdrant,
    provider: crate::provider::EmbeddingProvider,
    config: Config,
    nats: async_nats::Client,
) -> anyhow::Result<()> {
    let mut buffer: Vec<(async_nats::jetstream::message::Message, BatchItem)> = Vec::new();

    // §16.2.3 — drain on shutdown. A separate task watches SIGTERM/Ctrl-C
    // and flips this flag; the consumer loop checks it after every fetch
    // batch so in-flight work either commits or NACKs cleanly before exit,
    // instead of being mid-process when Tokio cancels the future.
    let shutdown = Arc::new(AtomicBool::new(false));
    {
        let shutdown = shutdown.clone();
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                let mut term =
                    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    {
                        Ok(s) => s,
                        Err(_) => return,
                    };
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            #[cfg(not(unix))]
            {
                let _ = tokio::signal::ctrl_c().await;
            }
            tracing::info!("embedding consumer shutdown signal received; will drain on next loop");
            shutdown.store(true, Ordering::SeqCst);
        });
    }

    loop {
        if shutdown.load(Ordering::SeqCst) && buffer.is_empty() {
            tracing::info!("embedding consumer drained, exiting cleanly");
            return Ok(());
        }

        let mut messages = consumer
            .fetch()
            .max_messages(config.batch_size)
            .expires(Duration::from_secs(2))
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

            let subject = msg.subject.as_str().to_string();
            let payload: serde_json::Value = match serde_json::from_slice(&msg.payload) {
                Ok(v) => v,
                Err(e) => {
                    tracing::error!(err = %e, "invalid payload");
                    let _ = msg.ack().await;
                    continue;
                }
            };

            match subject.as_str() {
                SUBJECT_CREATED => {
                    let kid = payload["knowledge_id"].as_str().unwrap_or("").to_string();
                    let doc_id = payload["document_id"].as_str().unwrap_or("").to_string();
                    let org_id = payload["org_id"].as_str().unwrap_or("").to_string();

                    // Fetch text from DB if not in message
                    let text = if let Some(t) = payload["text"].as_str().filter(|t| !t.is_empty()) {
                        t.to_string()
                    } else {
                        let row: Option<(String,)> = sqlx::query_as(
                            "SELECT text FROM knowledge_units WHERE knowledge_id = $1",
                        )
                        .bind(&kid)
                        .fetch_optional(&pool)
                        .await?;
                        match row {
                            Some((t,)) => t,
                            None => {
                                tracing::warn!(knowledge_id = %kid, "chunk not found, skipping");
                                let _ = msg.ack().await;
                                continue;
                            }
                        }
                    };

                    let chunk_index = payload["chunk_index"].as_i64().unwrap_or(0) as i32;

                    buffer.push((
                        msg,
                        BatchItem {
                            knowledge_id: kid,
                            document_id: doc_id,
                            org_id,
                            chunk_index,
                            text,
                        },
                    ));
                }
                SUBJECT_DOC_DELETED => {
                    let doc_id = payload["document_id"].as_str().unwrap_or("");
                    if let Err(e) = qdrant_writer::delete_vectors_by_document(
                        &qdrant,
                        &config.qdrant_collection,
                        doc_id,
                    )
                    .await
                    {
                        tracing::error!(err = %e, document_id = doc_id, "qdrant delete failed");
                    }
                    let _ = msg.ack().await;
                }
                SUBJECT_KNOWLEDGE_DELETED => {
                    // Orphaned chunks from a content re-chunk (index-engine). Purge
                    // their vectors so superseded content can't surface in search.
                    let ids: Vec<String> = payload["knowledge_ids"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    if let Err(e) = qdrant_writer::delete_vectors_by_ids(
                        &qdrant,
                        &config.qdrant_collection,
                        &ids,
                    )
                    .await
                    {
                        tracing::error!(err = %e, "qdrant delete by ids failed");
                    }
                    let _ = msg.ack().await;
                }
                _ => {
                    let _ = msg.ack().await;
                }
            }
        }

        // Process accumulated batch
        if !buffer.is_empty() {
            let items: Vec<BatchItem> = buffer
                .iter()
                .map(|(_, item)| BatchItem {
                    knowledge_id: item.knowledge_id.clone(),
                    document_id: item.document_id.clone(),
                    org_id: item.org_id.clone(),
                    chunk_index: item.chunk_index,
                    text: item.text.clone(),
                })
                .collect();

            match batch::process_batch(
                &items,
                &provider,
                &qdrant,
                &pool,
                &config.qdrant_collection,
                &nats,
            )
            .await
            {
                Ok(()) => {
                    for (msg, _) in &buffer {
                        let _ = msg.ack().await;
                    }
                }
                Err(e) => {
                    tracing::error!(err = %e, batch_size = buffer.len(), "batch processing failed");
                    for (msg, item) in &buffer {
                        let num_delivered = msg.info().map(|i| i.delivered).unwrap_or(0) as u32;
                        if num_delivered >= config.max_delivery_attempts {
                            tracing::warn!(knowledge_id = %item.knowledge_id, "max retries reached, sending to DLQ");
                            let dlq = serde_json::json!({
                                "original_subject": SUBJECT_CREATED,
                                "knowledge_id": item.knowledge_id,
                                "document_id": item.document_id,
                                "org_id": item.org_id,
                                "error": e.to_string(),
                                "attempts": num_delivered,
                            });
                            let _ = nats
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
            buffer.clear();
        }
    }
}
