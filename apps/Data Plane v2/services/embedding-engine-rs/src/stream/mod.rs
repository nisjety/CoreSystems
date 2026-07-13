use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_nats::jetstream::{self, consumer::PullConsumer, Context as JsContext};
use event_envelope_rs::{EventClaims, EventSigner, EventVerifier};
use futures::StreamExt;

use crate::batch::{self, BatchItem};
use crate::config::Config;
use crate::qdrant_writer;

// The knowledge stream uses interest retention because both embedding-engine
// and graph-index durably consume deletion events. Subjects still belong to
// exactly one stream; fan-out is implemented with distinct durable consumers.
const STREAM_NAME: &str = "DATAPLANE_KNOWLEDGE";
const SUBJECT_CREATED: &str = "dataplane.knowledge.units.created";
const SUBJECT_KNOWLEDGE_DELETED: &str = "dataplane.knowledge.units.deleted";
const CONSUMER_NAME: &str = "embedding-engine";
const DLQ_SUBJECT: &str = "dataplane.dlq.embedding-engine";

pub async fn setup_stream(js: &JsContext) -> anyhow::Result<()> {
    let config = knowledge_stream_config();
    let stream = js.get_or_create_stream(config).await?;
    if let Some(upgraded) = upgraded_knowledge_stream_config(&stream.cached_info().config) {
        js.update_stream(upgraded).await?;
    }
    tracing::info!("NATS stream ready: {STREAM_NAME}");
    Ok(())
}

fn knowledge_stream_config() -> jetstream::stream::Config {
    jetstream::stream::Config {
        name: STREAM_NAME.to_string(),
        subjects: vec![
            SUBJECT_CREATED.to_string(),
            SUBJECT_KNOWLEDGE_DELETED.to_string(),
        ],
        retention: jetstream::stream::RetentionPolicy::Interest,
        max_age: Duration::from_secs(7 * 24 * 3600),
        ..Default::default()
    }
}

fn upgraded_knowledge_stream_config(
    current: &jetstream::stream::Config,
) -> Option<jetstream::stream::Config> {
    let mut upgraded = current.clone();
    let mut changed = false;
    if upgraded.retention != jetstream::stream::RetentionPolicy::Interest {
        upgraded.retention = jetstream::stream::RetentionPolicy::Interest;
        changed = true;
    }
    for subject in [SUBJECT_CREATED, SUBJECT_KNOWLEDGE_DELETED] {
        if !upgraded.subjects.iter().any(|existing| existing == subject) {
            upgraded.subjects.push(subject.to_owned());
            changed = true;
        }
    }
    changed.then_some(upgraded)
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
    event_security: EventSecurity,
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
            let redelivery = msg.info().is_ok_and(|info| info.delivered > 1);
            let verified = match decode_event(
                event_security.verifier.as_deref(),
                &subject,
                &msg.payload,
                redelivery,
            ) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(err = %e, subject = %subject, "rejected unauthorized event");
                    let _ = msg.ack().await;
                    continue;
                }
            };
            let payload = verified.payload;
            let event_claims = verified.claims;

            if event_claims.zdr && subject == SUBJECT_CREATED {
                tracing::info!(org_id = %event_claims.org_id, "restrictive-ZDR embedding event dropped without durable writes");
                let _ = msg.ack().await;
                continue;
            }

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
                            "SELECT text FROM knowledge_units WHERE knowledge_id = $1 AND org_id = $2 AND document_id = $3",
                        )
                        .bind(&kid)
                        .bind(&event_claims.org_id)
                        .bind(&doc_id)
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

                    // ZDR source of truth: the owning document's classification.
                    // A `restricted` doc is Zero-Data-Retention content, so its
                    // chunks must not egress to a retaining embedding provider —
                    // the embed egress guard enforces it downstream. We read it
                    // here (the real column) rather than trusting the event
                    // payload, so a stale/forged event can't downgrade ZDR.
                    let zdr = if doc_id.is_empty() {
                        false
                    } else {
                        let row: Option<(Option<String>,)> = sqlx::query_as(
                            "SELECT zdr_classification FROM documents WHERE document_id = $1 AND org_id = $2",
                        )
                        .bind(&doc_id)
                        .bind(&event_claims.org_id)
                        .fetch_optional(&pool)
                        .await?;
                        event_claims.zdr || matches!(row, Some((Some(c),)) if c == "restricted")
                    };

                    buffer.push((
                        msg,
                        BatchItem {
                            knowledge_id: kid,
                            document_id: doc_id,
                            org_id,
                            chunk_index,
                            text,
                            zdr,
                            user_id: event_claims.user_id,
                        },
                    ));
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
                        &event_claims.org_id,
                        payload["document_id"].as_str().unwrap_or(""),
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
                    zdr: item.zdr,
                    user_id: item.user_id.clone(),
                })
                .collect();

            match batch::process_batch(
                &items,
                &provider,
                &qdrant,
                &pool,
                &config.qdrant_collection,
                &nats,
                event_security.signer.as_deref(),
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
                                "user_id": item.user_id,
                                "zdr": item.zdr,
                            });
                            let dlq_payload = encode_outbound_event(
                                event_security.signer.as_deref(),
                                DLQ_SUBJECT,
                                &item.org_id,
                                item.user_id.as_deref(),
                                item.zdr,
                                &dlq,
                            )?;
                            let _ = nats.publish(DLQ_SUBJECT, dlq_payload.into()).await;
                            let _ = msg.ack().await;
                        }
                    }
                }
            }
            buffer.clear();
        }
    }
}

pub struct EventSecurity {
    pub verifier: Option<Arc<EventVerifier>>,
    pub signer: Option<Arc<EventSigner>>,
}

struct DecodedEvent {
    claims: EventClaims,
    payload: serde_json::Value,
}

fn decode_event(
    verifier: Option<&EventVerifier>,
    subject: &str,
    bytes: &[u8],
    redelivery: bool,
) -> anyhow::Result<DecodedEvent> {
    if let Some(verifier) = verifier {
        return decode_verified_event(verifier, subject, bytes, redelivery);
    }
    let payload: serde_json::Value = serde_json::from_slice(bytes)?;
    Ok(DecodedEvent {
        claims: EventClaims {
            iss: "insecure-dev-legacy".into(),
            sub: "insecure-dev-legacy".into(),
            aud: "insecure-dev-legacy".into(),
            principal_type: "service".into(),
            org_id: payload["org_id"].as_str().unwrap_or_default().to_owned(),
            user_id: payload["user_id"].as_str().map(str::to_owned),
            scopes: vec![],
            zdr: payload["zdr"].as_bool().unwrap_or(false),
            event_type: subject.to_owned(),
            payload_sha256: String::new(),
            jti: String::new(),
            iat: 0,
            nbf: 0,
            exp: 0,
        },
        payload,
    })
}

fn decode_verified_event(
    verifier: &EventVerifier,
    subject: &str,
    bytes: &[u8],
    redelivery: bool,
) -> anyhow::Result<DecodedEvent> {
    let event = if redelivery {
        verifier.verify_redelivery(subject, bytes)?
    } else {
        verifier.verify(subject, bytes)?
    };
    Ok(DecodedEvent {
        claims: event.claims,
        payload: serde_json::from_slice(&event.payload)?,
    })
}

fn encode_outbound_event(
    signer: Option<&EventSigner>,
    subject: &str,
    org_id: &str,
    user_id: Option<&str>,
    zdr: bool,
    payload: &serde_json::Value,
) -> anyhow::Result<Vec<u8>> {
    let raw = serde_json::to_vec(payload)?;
    match signer {
        Some(signer) => Ok(signer.sign(subject, org_id, user_id, zdr, &raw)?),
        None => Ok(raw),
    }
}

#[cfg(test)]
mod signed_event_tests {
    use super::{
        decode_verified_event, upgraded_knowledge_stream_config, SUBJECT_CREATED,
        SUBJECT_KNOWLEDGE_DELETED,
    };
    use event_envelope_rs::{EventSigner, EventVerifier};
    use rsa::pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey};
    use rsa::{RsaPrivateKey, RsaPublicKey};

    #[test]
    fn embedding_rejects_unsigned_index_events_and_preserves_zdr() {
        let private = RsaPrivateKey::new(&mut rand::thread_rng(), 2048).expect("key");
        let public = RsaPublicKey::from(&private);
        let signer = EventSigner::from_rsa_pem(
            private
                .to_pkcs1_pem(Default::default())
                .expect("pem")
                .as_bytes(),
            "service:index-engine-rs",
            "index-events-v1",
            "dataplane-events",
            "events:index:publish",
        )
        .expect("signer");
        let verifier = EventVerifier::from_rsa_pem(
            public
                .to_pkcs1_pem(Default::default())
                .expect("pem")
                .as_bytes(),
            "service:index-engine-rs",
            "index-events-v1",
            "dataplane-events",
            "events:index:publish",
            10,
        )
        .expect("verifier");
        let raw = br#"{"knowledge_id":"kid-test","document_id":"doc-test","org_id":"org-test","zdr":true}"#;
        assert!(decode_verified_event(&verifier, SUBJECT_CREATED, raw, false).is_err());
        let envelope = signer
            .sign(SUBJECT_CREATED, "org-test", None, true, raw)
            .expect("sign");
        let event =
            decode_verified_event(&verifier, SUBJECT_CREATED, &envelope, false).expect("verified");
        assert!(event.claims.zdr);
        assert_eq!(event.payload["org_id"], "org-test");
    }

    #[test]
    fn existing_workqueue_stream_is_forward_safely_upgraded_to_interest() {
        let current = async_nats::jetstream::stream::Config {
            name: "DATAPLANE_KNOWLEDGE".into(),
            subjects: vec![SUBJECT_CREATED.into(), "operator.extra".into()],
            retention: async_nats::jetstream::stream::RetentionPolicy::WorkQueue,
            ..Default::default()
        };
        let upgraded = upgraded_knowledge_stream_config(&current).expect("migration required");
        assert_eq!(
            upgraded.retention,
            async_nats::jetstream::stream::RetentionPolicy::Interest
        );
        assert!(upgraded
            .subjects
            .iter()
            .any(|s| s == SUBJECT_KNOWLEDGE_DELETED));
        assert!(upgraded.subjects.iter().any(|s| s == "operator.extra"));
        assert!(upgraded_knowledge_stream_config(&upgraded).is_none());
    }
}
