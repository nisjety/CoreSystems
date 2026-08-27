use std::sync::Arc;
use std::time::Duration;

use async_nats::jetstream::{self, consumer::PullConsumer, Context as JsContext};
use event_envelope_rs::{EnvelopeError, EventClaims, EventVerifier};
use futures::StreamExt;

use crate::extractor::GraphExtractor;
use crate::neo4j::Neo4jClient;
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
    let event = DecodedEvent {
        claims: event.claims,
        payload,
    };
    event_org_id(&event)?;
    Ok(event)
}

pub async fn setup_stream(js: &JsContext) -> anyhow::Result<()> {
    let config = jetstream::stream::Config {
        name: STREAM_NAME.to_string(),
        subjects: vec![SUBJECT.to_string()],
        // `Interest`, not `WorkQueue`: `dataplane.documents.indexed` fans out
        // to BOTH graph-index (extraction) and quickwit-adapter (lexical
        // index). WorkQueue allows exactly one consumer per subject, so the
        // second reader was silently refused and fell back to a lossy
        // core-NATS subscription. Matches DATAPLANE_KNOWLEDGE above.
        retention: jetstream::stream::RetentionPolicy::Interest,
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
    // Optional Neo4j read-model mirror. `None` disables the mirror; the
    // canonical Postgres graph is written regardless.
    neo4j: Option<Arc<Neo4jClient>>,
    // Minimum member count for a derived community (config `community_min_size`).
    community_min_size: usize,
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
                        // An EXPIRED envelope is not a forgery and must not be
                        // reported as one. `EnvelopeError::Expired` is produced
                        // only after every authenticity check has already passed
                        // (signature, key id, issuer, scope, payload digest,
                        // boundary fields) — the sole thing that failed is the
                        // clock window. Same distinction embedding-engine draws
                        // in its `SUBJECT_CREATED` handler.
                        //
                        // This matters here more than anywhere else in the plane,
                        // because graph extraction is INHERENTLY slow: one
                        // inference call per chunk, tens of seconds per document.
                        // A backlog of more than a couple of documents therefore
                        // always outlives the envelope TTL (120s, max 300s), and
                        // every document behind the head of the queue was being
                        // dropped and acked — silently, logged as
                        // "unauthorized", which reads as an attack rather than
                        // latency. Observed: 12 documents announced, 2 extracted,
                        // 10 discarded this way with the graph left empty.
                        //
                        // Dropping is still the behaviour (accepting an expired
                        // envelope would weaken a real replay control, and this
                        // service has no reconciler to hand it to), but it is now
                        // an explicit, actionable signal naming the document to
                        // re-announce. The durable fix is a reconciler for this
                        // subject, or extraction fanned out behind a fresh
                        // envelope per chunk.
                        match e.downcast_ref::<EnvelopeError>() {
                            Some(EnvelopeError::Expired(stale)) => {
                                let doc =
                                    serde_json::from_slice::<serde_json::Value>(&stale.payload)
                                        .ok()
                                        .and_then(|p| p["document_id"].as_str().map(str::to_owned))
                                        .unwrap_or_else(|| "<unknown>".to_string());
                                tracing::error!(
                                    document_id = %doc,
                                    "graph event DROPPED: envelope authentic but expired \
                                     (consumer is behind the envelope TTL). Extraction did \
                                     NOT run for this document; re-announce it to retry."
                                );
                            }
                            _ => {
                                tracing::warn!(err = %e, "rejected unauthorized graph event");
                            }
                        }
                        let _ = msg.ack().await;
                        continue;
                    }
                };
            let org_id = match event_org_id(&verified) {
                Ok(org_id) => org_id.to_owned(),
                Err(e) => {
                    tracing::warn!(err = %e, "rejected graph event tenant mismatch");
                    let _ = msg.ack().await;
                    continue;
                }
            };
            let payload = verified.payload;
            let claims = verified.claims;

            let doc_id = payload["document_id"].as_str().unwrap_or("");

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
            // `unwrap_or_default()` also collapses a DB error into "no chunks",
            // so the two cases are logged apart — an empty result is a routine
            // policy skip, a query failure is not.
            let chunks = match store.load_org_visible_chunks(&org_id, doc_id).await {
                Ok(chunks) => chunks,
                Err(e) => {
                    tracing::error!(
                        err = %e,
                        document_id = doc_id,
                        org_id = %org_id,
                        "loading org-visible chunks failed; treating as no chunks"
                    );
                    Vec::new()
                }
            };

            // Say so when a document is skipped by the visibility policy rather
            // than extracted.
            //
            // Without this the run logged "graph extraction complete,
            // entities=0" and acked — indistinguishable from extraction running
            // and finding nothing, and the `all_failed` DLQ guard below is
            // deliberately gated on `!chunks.is_empty()` so it stays quiet too.
            // A whole 190-document corpus produced an empty graph this way, with
            // every document reporting success in ~3ms: the corpus was ingested
            // `visibility='private'` and this path only reads `'org'`.
            //
            // The filter itself is correct and must stay — graph entities are
            // org-shared, so extracting a private document would leak its
            // content org-wide through the graph arm. Only the silence was
            // wrong.
            if chunks.is_empty() {
                tracing::info!(
                    document_id = doc_id,
                    org_id = %org_id,
                    "graph extraction skipped: no org-visible live chunks \
                     (documents must be visibility='org'; private/shared and \
                     deleted documents are excluded by design)"
                );
            }

            let mut total_entities = 0usize;
            let mut total_rels = 0usize;
            let mut total_claims = 0usize;
            let mut all_claim_ids: Vec<String> = Vec::new();

            for (kid, text) in &chunks {
                match extractor.extract(text, &org_id, claims.zdr).await {
                    Ok(result) => match store.persist_extraction(&org_id, kid, &result).await {
                        Ok(persisted) => {
                            if let Err(e) = store
                                .persist_text_unit_mappings(
                                    &org_id,
                                    kid,
                                    &persisted.entity_ids,
                                    &persisted.rel_ids,
                                    &persisted.claim_ids,
                                )
                                .await
                            {
                                tracing::error!(err = %e, knowledge_id = kid, "persist text_unit mappings failed");
                            }
                            // Mirror to the Neo4j read-model. Best-effort and
                            // non-fatal — Postgres is canonical, the read-model
                            // is rebuildable. This runs ONLY on org-visible,
                            // non-restrictive-ZDR content: restrictive events
                            // are dropped above (claims.zdr) and non-visible
                            // chunks yield an empty PersistedExtraction, so the
                            // mirror inherits both gates.
                            if let Some(neo4j) = neo4j.as_ref() {
                                if let Err(e) = neo4j
                                    .merge_extraction(
                                        &org_id,
                                        &persisted.mirror_entities,
                                        &persisted.mirror_relationships,
                                    )
                                    .await
                                {
                                    tracing::warn!(err = %e, knowledge_id = kid, "neo4j mirror write failed (non-fatal; postgres canonical)");
                                }
                            }
                            total_entities += persisted.entity_ids.len();
                            total_rels += persisted.rel_ids.len();
                            total_claims += persisted.claim_ids.len();
                            // Collected for the post-ack contradiction sweep:
                            // detection needs the whole document's claims, and
                            // the text_unit mappings above must already exist
                            // for the visibility join to resolve.
                            all_claim_ids.extend(persisted.claim_ids.iter().cloned());
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

            // Ack BEFORE the community refresh: the refresh is a whole-org
            // recompute (two bulk queries + a replace tx), so running it inside
            // the ack window would extend the JetStream deadline under bulk
            // ingest and risk redelivery. Post-ack keeps the message settled;
            // the per-org advisory lock in `replace_communities` serializes any
            // concurrent recompute so overlapping runs can't duplicate rows.
            let _ = msg.ack().await;

            // Refresh the org's derived communities when this document added
            // relationships. Best-effort: a failure is logged, never retried
            // here (the message is already acked), and detect_communities only
            // replaces rows AFTER a successful listing — a transient DB error
            // cannot wipe existing communities.
            if total_rels > 0 {
                if let Err(e) =
                    crate::community::detect_communities(&store, &org_id, community_min_size).await
                {
                    tracing::warn!(err = %e, org_id, "community refresh failed (non-fatal)");
                } else if let Err(e) =
                    crate::community::summarize_communities(&store, &extractor, &org_id).await
                {
                    // P1-5: fill `graph_communities.summary` for communities that
                    // still lack one. Gated on `COMMUNITY_SUMMARY_ENABLED` and
                    // capped per run, so a backlog drains over successive
                    // ingests instead of stalling one on many LLM calls. Only
                    // runs when detection succeeded — summarising against a
                    // half-updated community set would key summaries to
                    // memberships that never existed.
                    tracing::warn!(err = %e, org_id, "community summarisation failed (non-fatal)");
                }
            }

            // P1-4: flag claims this document contradicts. Post-ack for the same
            // reason as the community refresh — keeping it inside the ack window
            // would extend the JetStream deadline under bulk ingest and risk
            // redelivery. Best-effort and additive: a failure leaves existing
            // flags untouched, and the write only ever unions ids and sets
            // `claim_status`, so a partial run cannot erase prior findings.
            if !all_claim_ids.is_empty() {
                match store
                    .detect_claim_contradictions(&org_id, &all_claim_ids)
                    .await
                {
                    Ok(0) => {}
                    Ok(pairs) => {
                        tracing::info!(org_id, pairs, "claim contradictions recorded")
                    }
                    Err(e) => {
                        tracing::warn!(err = %e, org_id, "contradiction detection failed (non-fatal)")
                    }
                }
            }
        }
    }
}

struct DecodedEvent {
    claims: EventClaims,
    payload: serde_json::Value,
}

/// Return the tenant identity from the verified event claims after checking
/// that the payload cannot override it. Signed envelopes currently enforce
/// this invariant too, but keeping the check at the graph consumer boundary
/// protects the downstream path when legacy/test decoding is enabled and
/// makes the claim-to-tenant pin explicit.
fn event_org_id(event: &DecodedEvent) -> anyhow::Result<&str> {
    let claims_org = event.claims.org_id.as_str();
    anyhow::ensure!(
        !claims_org.is_empty() && claims_org == claims_org.trim(),
        "graph event claims missing canonical org_id"
    );

    let payload_org = event.payload["org_id"].as_str().unwrap_or("");
    anyhow::ensure!(
        !payload_org.is_empty() && payload_org == payload_org.trim(),
        "graph event payload missing canonical org_id"
    );
    anyhow::ensure!(
        payload_org == claims_org,
        "graph event payload org_id does not match verified claims"
    );
    Ok(claims_org)
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
    let event = DecodedEvent {
        claims: event.claims,
        payload: serde_json::from_slice(&event.payload)?,
    };
    event_org_id(&event)?;
    Ok(event)
}

#[cfg(test)]
mod signed_event_tests {
    use super::{
        decode_cleanup_event, decode_verified_event, event_org_id, SUBJECT,
        SUBJECT_KNOWLEDGE_DELETED,
    };
    use event_envelope_rs::{EventSigner, EventVerifier};
    use rsa::pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey};
    use rsa::{RsaPrivateKey, RsaPublicKey};

    #[test]
    fn graph_rejects_verified_claim_payload_tenant_mismatch() {
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
        let envelope = signer
            .sign(
                SUBJECT,
                "org-a",
                None,
                false,
                br#"{"document_id":"doc-a","org_id":"org-a"}"#,
            )
            .expect("signed event");
        let mut event =
            decode_verified_event(&verifier, SUBJECT, &envelope, false).expect("verified event");
        event.payload["org_id"] = serde_json::Value::String("org-b".into());

        assert!(event_org_id(&event).is_err());
    }

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
