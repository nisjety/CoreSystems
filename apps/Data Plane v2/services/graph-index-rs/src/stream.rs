use std::sync::Arc;
use std::time::Duration;

use async_nats::jetstream::{self, consumer::PullConsumer, Context as JsContext};
use event_envelope_rs::{EventClaims, EventVerifier};
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
const KNOWLEDGE_STREAM_NAME: &str = "DATAPLANE_KNOWLEDGE";
const SUBJECT_KNOWLEDGE_DELETED: &str = "dataplane.knowledge.units.deleted";
const CLEANUP_CONSUMER_NAME: &str = "graph-index-cleanup";

pub async fn setup_cleanup_stream(js: &JsContext) -> anyhow::Result<()> {
    let desired = jetstream::stream::Config {
        name: KNOWLEDGE_STREAM_NAME.to_owned(),
        subjects: vec![
            "dataplane.knowledge.units.created".to_owned(),
            SUBJECT_KNOWLEDGE_DELETED.to_owned(),
        ],
        retention: jetstream::stream::RetentionPolicy::Interest,
        max_age: Duration::from_secs(7 * 24 * 3600),
        ..Default::default()
    };
    let stream = js.get_or_create_stream(desired).await?;
    if let Some(upgraded) = upgraded_cleanup_stream_config(&stream.cached_info().config) {
        js.update_stream(upgraded).await?;
    }
    Ok(())
}

fn upgraded_cleanup_stream_config(
    current: &jetstream::stream::Config,
) -> Option<jetstream::stream::Config> {
    let mut upgraded = current.clone();
    let mut changed = false;
    if upgraded.retention != jetstream::stream::RetentionPolicy::Interest {
        upgraded.retention = jetstream::stream::RetentionPolicy::Interest;
        changed = true;
    }
    for subject in [
        "dataplane.knowledge.units.created",
        SUBJECT_KNOWLEDGE_DELETED,
    ] {
        if !upgraded.subjects.iter().any(|existing| existing == subject) {
            upgraded.subjects.push(subject.to_owned());
            changed = true;
        }
    }
    changed.then_some(upgraded)
}

pub async fn create_cleanup_consumer(js: &JsContext) -> anyhow::Result<PullConsumer> {
    let stream = js.get_stream(KNOWLEDGE_STREAM_NAME).await?;
    Ok(stream
        .get_or_create_consumer(
            CLEANUP_CONSUMER_NAME,
            jetstream::consumer::pull::Config {
                durable_name: Some(CLEANUP_CONSUMER_NAME.to_owned()),
                filter_subject: SUBJECT_KNOWLEDGE_DELETED.to_owned(),
                ack_wait: Duration::from_secs(60),
                max_deliver: 5,
                ..Default::default()
            },
        )
        .await?)
}

/// Durably consumes signed index deletion events. A database failure leaves
/// the message unacked for redelivery; the tenant-scoped delete is idempotent.
pub async fn run_cleanup_consumer(
    consumer: PullConsumer,
    store: Arc<GraphStore>,
    verifier: Option<Arc<EventVerifier>>,
) -> anyhow::Result<()> {
    loop {
        let mut messages = consumer
            .fetch()
            .max_messages(32)
            .expires(Duration::from_secs(2))
            .messages()
            .await?;
        while let Some(message) = messages.next().await {
            let msg = match message {
                Ok(msg) => msg,
                Err(error) => {
                    tracing::warn!(%error, "graph cleanup receive failed");
                    continue;
                }
            };
            let redelivery = msg.info().is_ok_and(|info| info.delivered > 1);
            let cleanup = match verifier.as_deref() {
                Some(verifier) => decode_cleanup_event(verifier, &msg.payload, redelivery),
                None => serde_json::from_slice(&msg.payload)
                    .map(|payload| DecodedEvent {
                        claims: insecure_legacy_claims(SUBJECT_KNOWLEDGE_DELETED, &payload),
                        payload,
                    })
                    .map_err(anyhow::Error::from),
            };
            let cleanup = match cleanup {
                Ok(event) => event,
                Err(e) => {
                    tracing::warn!(err = %e, "rejected unauthorized knowledge.units.deleted payload");
                    let _ = msg.ack().await;
                    continue;
                }
            };
            if cleanup.claims.zdr {
                tracing::warn!(
                    "restrictive-ZDR cleanup event rejected from durable graph consumer"
                );
                let _ = msg.ack().await;
                continue;
            }
            let org_id = cleanup.claims.org_id.as_str();
            let kids: Vec<String> = cleanup.payload["knowledge_ids"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            if org_id.is_empty() || kids.is_empty() {
                let _ = msg.ack().await;
                continue;
            }
            match store.delete_text_unit_mappings(org_id, &kids).await {
                Ok(removed) => {
                    tracing::info!(removed, org_id, "purged orphaned graph mappings");
                    let _ = msg.ack().await;
                }
                Err(e) => {
                    tracing::error!(err = %e, "purge orphaned graph mappings failed");
                }
            }
        }
    }
}

fn decode_cleanup_event(
    verifier: &EventVerifier,
    bytes: &[u8],
    redelivery: bool,
) -> anyhow::Result<DecodedEvent> {
    let event = if redelivery {
        verifier.verify_redelivery(SUBJECT_KNOWLEDGE_DELETED, bytes)?
    } else {
        verifier.verify(SUBJECT_KNOWLEDGE_DELETED, bytes)?
    };
    let payload: serde_json::Value = serde_json::from_slice(&event.payload)?;
    if payload["document_id"].as_str().unwrap_or("").is_empty()
        || payload["knowledge_ids"].as_array().is_none_or(|ids| {
            ids.is_empty() || ids.iter().any(|id| id.as_str().is_none_or(str::is_empty))
        })
    {
        anyhow::bail!("invalid tenant-bound graph cleanup payload");
    }
    Ok(DecodedEvent {
        claims: event.claims,
        payload,
    })
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
    nats: async_nats::Client,
    event_verifier: Option<Arc<EventVerifier>>,
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

            let redelivery = msg.info().is_ok_and(|info| info.delivered > 1);
            let verified =
                match decode_event(event_verifier.as_deref(), SUBJECT, &msg.payload, redelivery) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!(err = %e, "rejected unauthorized graph event");
                        let _ = msg.ack().await;
                        continue;
                    }
                };
            let payload = verified.payload;
            let claims = verified.claims;

            let doc_id = payload["document_id"].as_str().unwrap_or("");
            let org_id = payload["org_id"].as_str().unwrap_or("");

            if doc_id.is_empty() || org_id.is_empty() {
                let _ = msg.ack().await;
                continue;
            }

            if claims.zdr {
                tracing::info!(
                    org_id,
                    "restrictive-ZDR graph event dropped without durable writes"
                );
                let _ = msg.ack().await;
                continue;
            }

            tracing::info!(
                document_id = doc_id,
                org_id,
                "processing document for graph extraction"
            );

            // Only organization-visible, live documents may enter graph
            // extraction. Filtering after the model call is too late: it would
            // disclose private/shared content to the extractor even when the
            // resulting graph rows were discarded.
            let chunks = store
                .load_org_visible_chunks(org_id, doc_id)
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
                    if event_verifier.is_none() {
                        let _ = nats
                            .publish(
                                DLQ_SUBJECT,
                                serde_json::to_vec(&dlq).unwrap_or_default().into(),
                            )
                            .await;
                    }
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
        claims: insecure_legacy_claims(subject, &payload),
        payload,
    })
}

fn insecure_legacy_claims(subject: &str, payload: &serde_json::Value) -> EventClaims {
    EventClaims {
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
    }
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

#[cfg(test)]
mod signed_event_tests {
    use super::{decode_cleanup_event, decode_verified_event, SUBJECT, SUBJECT_KNOWLEDGE_DELETED};
    use event_envelope_rs::{EventSigner, EventVerifier};
    use rsa::pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey};
    use rsa::{RsaPrivateKey, RsaPublicKey};

    #[test]
    fn graph_rejects_unsigned_embedding_events_and_preserves_tenant_zdr() {
        let private = RsaPrivateKey::new(&mut rand::thread_rng(), 2048).expect("key");
        let public = RsaPublicKey::from(&private);
        let signer = EventSigner::from_rsa_pem(
            private
                .to_pkcs1_pem(Default::default())
                .expect("pem")
                .as_bytes(),
            "service:embedding-engine-rs",
            "embedding-events-v1",
            "dataplane-events",
            "events:embedding:publish",
        )
        .expect("signer");
        let verifier = EventVerifier::from_rsa_pem(
            public
                .to_pkcs1_pem(Default::default())
                .expect("pem")
                .as_bytes(),
            "service:embedding-engine-rs",
            "embedding-events-v1",
            "dataplane-events",
            "events:embedding:publish",
            10,
        )
        .expect("verifier");
        let raw = br#"{"document_id":"doc-test","org_id":"org-test","zdr":true}"#;
        assert!(decode_verified_event(&verifier, SUBJECT, raw, false).is_err());
        let envelope = signer
            .sign(SUBJECT, "org-test", None, true, raw)
            .expect("sign");
        let event = decode_verified_event(&verifier, SUBJECT, &envelope, false).expect("verified");
        assert_eq!(event.claims.org_id, "org-test");
        assert!(event.claims.zdr);
    }

    #[test]
    fn graph_cleanup_requires_signed_index_scope_and_rejects_replay() {
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
        let raw =
            br#"{"document_id":"doc-a","org_id":"org-a","knowledge_ids":["kid-a"],"zdr":false}"#;
        assert!(decode_cleanup_event(&verifier, raw, false).is_err());
        let envelope = signer
            .sign(SUBJECT_KNOWLEDGE_DELETED, "org-a", None, false, raw)
            .expect("sign");
        let mut tampered: serde_json::Value = serde_json::from_slice(&envelope).expect("wire json");
        tampered["data"] = serde_json::Value::String("e30".to_owned());
        assert!(decode_cleanup_event(
            &verifier,
            &serde_json::to_vec(&tampered).expect("wire json"),
            false,
        )
        .is_err());
        let event = decode_cleanup_event(&verifier, &envelope, false).expect("verified cleanup");
        assert_eq!(event.claims.org_id, "org-a");
        assert_eq!(event.payload["document_id"], "doc-a");
        assert!(decode_cleanup_event(&verifier, &envelope, false).is_err());
        assert!(decode_cleanup_event(&verifier, &envelope, true).is_ok());

        let cross_tenant = signer.sign(SUBJECT_KNOWLEDGE_DELETED, "org-b", None, false, raw);
        assert!(cross_tenant.is_err());
    }
}
