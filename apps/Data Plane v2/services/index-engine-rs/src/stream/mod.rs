use std::time::Duration;

use async_nats::jetstream::{self, consumer::PullConsumer, Context as JsContext};
use event_envelope_rs::{EventClaims, EventSigner, EventVerifier};

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
    event_verifier: Option<std::sync::Arc<EventVerifier>>,
    event_signer: Option<std::sync::Arc<EventSigner>>,
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
            let redelivery = msg.info().is_ok_and(|info| info.delivered > 1);
            let verified = match decode_event(
                event_verifier.as_deref(),
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

            if event_claims.zdr && matches!(subject.as_str(), SUBJECT_CREATED | SUBJECT_UPDATED) {
                tracing::info!(org_id = %event_claims.org_id, "restrictive-ZDR indexing event dropped without durable writes");
                let _ = msg.ack().await;
                continue;
            }

            match subject.as_str() {
                SUBJECT_CREATED | SUBJECT_UPDATED => {
                    let event = DocumentEvent {
                        document_id: payload["document_id"].as_str().unwrap_or("").to_string(),
                        org_id: payload["org_id"].as_str().unwrap_or("").to_string(),
                        title: payload["title"].as_str().unwrap_or("").to_string(),
                        source: payload["source"].as_str().unwrap_or("").to_string(),
                        doc_type: payload["type"].as_str().unwrap_or("").to_string(),
                        user_id: event_claims.user_id.clone(),
                        idempotency_key: event_claims.jti.clone(),
                        zdr: event_claims.zdr,
                    };

                    match builder::process_document(&pool, &event, &chunk_config).await {
                        Ok(result) => {
                            let mut publish_failed = false;
                            for kid in &result.knowledge_ids {
                                let embed_event = serde_json::json!({
                                    "knowledge_id": kid,
                                    "document_id": event.document_id,
                                    "org_id": event.org_id,
                                    "text": "",
                                    "user_id": event_claims.user_id,
                                    "zdr": event_claims.zdr,
                                });
                                let embed_payload = encode_downstream_event(
                                    event_signer.as_deref(),
                                    "dataplane.knowledge.units.created",
                                    &event_claims,
                                    &embed_event,
                                )?;
                                if let Err(error) = nats_client
                                    .publish(
                                        "dataplane.knowledge.units.created",
                                        embed_payload.into(),
                                    )
                                    .await
                                {
                                    tracing::error!(%error, knowledge_id = kid, "signed knowledge event publish failed");
                                    publish_failed = true;
                                    break;
                                }
                            }
                            if publish_failed {
                                continue;
                            }
                            if let Err(error) = nats_client.flush().await {
                                tracing::error!(%error, document_id = %event.document_id, "signed knowledge event flush failed");
                                continue;
                            }

                            // Orphan deletion intent was committed atomically by
                            // process_document and is published by the leased outbox worker.
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
                                    "user_id": event_claims.user_id,
                                    "zdr": event_claims.zdr,
                                });
                                let dlq_payload = encode_downstream_event(
                                    event_signer.as_deref(),
                                    DLQ_SUBJECT,
                                    &event_claims,
                                    &dlq,
                                )?;
                                let _ = nats_client.publish(DLQ_SUBJECT, dlq_payload.into()).await;
                                let _ = msg.ack().await;
                            }
                        }
                    }
                }
                SUBJECT_DELETED => {
                    let doc_id = payload["document_id"].as_str().unwrap_or("");
                    if event_signer.is_none() {
                        tracing::warn!(
                            document_id = doc_id,
                            "unsigned legacy document deletion disabled fail-closed"
                        );
                        let _ = msg.ack().await;
                        continue;
                    }
                    match crate::outbox::delete_and_enqueue(
                        &pool,
                        &event_claims.org_id,
                        doc_id,
                        event_claims.user_id.as_deref(),
                        &event_claims.jti,
                        event_claims.zdr,
                    )
                    .await
                    {
                        Ok(count) => {
                            tracing::info!(
                                document_id = doc_id,
                                count,
                                "knowledge deletion and signed outbox intent committed"
                            );
                        }
                        Err(e) => {
                            tracing::error!(err = %e, document_id = doc_id, "failed to commit deletion outbox");
                            continue;
                        }
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
    let org_id = payload["org_id"].as_str().unwrap_or_default().to_owned();
    Ok(DecodedEvent {
        claims: EventClaims {
            iss: "insecure-dev-legacy".to_owned(),
            sub: "insecure-dev-legacy".to_owned(),
            aud: "insecure-dev-legacy".to_owned(),
            principal_type: "service".to_owned(),
            org_id,
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

fn encode_downstream_event(
    signer: Option<&EventSigner>,
    subject: &str,
    upstream: &EventClaims,
    payload: &serde_json::Value,
) -> anyhow::Result<Vec<u8>> {
    let raw = serde_json::to_vec(payload)?;
    match signer {
        Some(signer) => Ok(signer.sign(
            subject,
            &upstream.org_id,
            upstream.user_id.as_deref(),
            upstream.zdr,
            &raw,
        )?),
        None => Ok(raw),
    }
}

#[cfg(test)]
mod signed_event_tests {
    use super::{decode_verified_event, SUBJECT_CREATED};
    use crate::outbox::{intent_payload, DeletionIntent};
    use event_envelope_rs::{EventSigner, EventVerifier};
    use rsa::pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey};
    use rsa::{RsaPrivateKey, RsaPublicKey};

    #[test]
    fn index_accepts_only_signed_document_events_and_preserves_zdr() {
        let private = RsaPrivateKey::new(&mut rand::thread_rng(), 2048).expect("key");
        let public = RsaPublicKey::from(&private);
        let signer = EventSigner::from_rsa_pem(
            private
                .to_pkcs1_pem(Default::default())
                .expect("pem")
                .as_bytes(),
            "service:documents-api-go",
            "documents-events-v1",
            "dataplane-events",
            "events:documents:publish",
        )
        .expect("signer");
        let verifier = EventVerifier::from_rsa_pem(
            public
                .to_pkcs1_pem(Default::default())
                .expect("pem")
                .as_bytes(),
            "service:documents-api-go",
            "documents-events-v1",
            "dataplane-events",
            "events:documents:publish",
            10,
        )
        .expect("verifier");
        let raw =
            br#"{"document_id":"doc-test","org_id":"org-test","user_id":"user-test","zdr":true}"#;
        assert!(decode_verified_event(&verifier, SUBJECT_CREATED, raw, false).is_err());
        let envelope = signer
            .sign(SUBJECT_CREATED, "org-test", Some("user-test"), true, raw)
            .expect("signed");
        let event =
            decode_verified_event(&verifier, SUBJECT_CREATED, &envelope, false).expect("verified");
        assert_eq!(event.claims.org_id, "org-test");
        assert!(event.claims.zdr);
        assert_eq!(event.payload["document_id"], "doc-test");
    }

    #[test]
    fn delete_fanout_payload_is_tenant_document_and_chunk_bound() {
        let payload = intent_payload(&DeletionIntent {
            outbox_id: 1,
            org_id: "org-a".into(),
            document_id: "doc-a".into(),
            knowledge_ids: vec!["kid-a".into(), "kid-b".into()],
            user_id: Some("user-a".into()),
            idempotency_key: "delete-event-1".into(),
            attempts: 1,
        })
        .expect("valid deletion payload");
        assert_eq!(payload["org_id"], "org-a");
        assert_eq!(payload["document_id"], "doc-a");
        assert_eq!(payload["knowledge_ids"].as_array().unwrap().len(), 2);
        assert_eq!(payload["zdr"], false);
    }
}
