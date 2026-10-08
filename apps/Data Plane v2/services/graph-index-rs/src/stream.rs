use std::sync::Arc;
use std::time::Duration;

use async_nats::jetstream::{self, consumer::PullConsumer, Context as JsContext};
use event_envelope_rs::{EnvelopeError, EventClaims, EventVerifier};
use futures::StreamExt;

use crate::pipeline::{self, DocumentReport, Live, Origin, Ports};
use crate::store::GraphStore;

// Each engine owns disjoint subjects in JetStream. graph-index owns only
// `dataplane.documents.indexed` (its trigger) — the other subjects belong to
// DATAPLANE_DOCUMENTS / DATAPLANE_KNOWLEDGE streams.
const STREAM_NAME: &str = "DATAPLANE_GRAPH";
pub(crate) const SUBJECT: &str = "dataplane.documents.indexed";
const CONSUMER_NAME: &str = "graph-index";
/// Where a chunk that was given up on is dead-lettered (the pipeline publishes it, in every
/// event-verifier mode).
pub(crate) const DLQ_SUBJECT: &str = "dataplane.dlq.graph-index";
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

/// What became of one message from the stream. The message is acknowledged in every case: what
/// was not done is in the database (a chunk's state), and the reconciler reads it from there.
#[derive(Debug)]
pub(crate) enum MessageResult {
    /// Not processed, and settled: unauthorized, expired, another tenant's, malformed, or a
    /// restrictive-ZDR event.
    Dropped,
    Processed {
        org_id: String,
        report: DocumentReport,
    },
    /// The pass failed before it could record what it did (a database error). The chunks keep
    /// the state they had, and the reconciler picks the document up from the database.
    Failed,
}

/// One message: verified, then the document through the pipeline. Everything that can be decided
/// without a database (authenticity, tenant, restrictive ZDR) is decided here; everything about
/// the document's chunks is the pipeline's.
///
/// Nothing here depends on whether a verifier is configured beyond how the envelope is read: a
/// chunk that is given up on is dead-lettered by the pipeline whatever the mode (the old code
/// published its dead letters only when no verifier was configured, so in production, where
/// events are signed, none was ever published).
pub(crate) async fn process_message<P: Ports>(
    ports: &P,
    verifier: Option<&EventVerifier>,
    bytes: &[u8],
    redelivery: bool,
) -> MessageResult {
    let verified = match decode_event(verifier, SUBJECT, bytes, redelivery) {
        Ok(v) => v,
        Err(e) => {
            // An EXPIRED envelope is not a forgery and must not be reported as one.
            // `EnvelopeError::Expired` is produced only after every authenticity check has
            // already passed (signature, key id, issuer, scope, payload digest, boundary
            // fields): the sole thing that failed is the clock window. Graph extraction is
            // INHERENTLY slow (one inference call per chunk), so a backlog of more than a couple
            // of documents always outlives the envelope TTL (120s, max 300s); observed: 12
            // documents announced, 2 extracted, 10 discarded, logged as "unauthorized", which
            // reads as an attack rather than latency. Dropping is still the behaviour (accepting
            // an expired envelope would weaken a real replay control), but the document is
            // named, and the reconciler extracts it from the database with no envelope at all.
            match e.downcast_ref::<EnvelopeError>() {
                Some(EnvelopeError::Expired(stale)) => {
                    let doc = serde_json::from_slice::<serde_json::Value>(&stale.payload)
                        .ok()
                        .and_then(|p| p["document_id"].as_str().map(str::to_owned))
                        .unwrap_or_else(|| "<unknown>".to_string());
                    tracing::error!(
                        document_id = %doc,
                        "graph event DROPPED: envelope authentic but expired \
                         (consumer is behind the envelope TTL). The reconciler extracts the \
                         document from the database; nothing needs re-announcing."
                    );
                }
                _ => {
                    tracing::warn!(err = %e, "rejected unauthorized graph event");
                }
            }
            return MessageResult::Dropped;
        }
    };
    let org_id = match event_org_id(&verified) {
        Ok(org_id) => org_id.to_owned(),
        Err(e) => {
            tracing::warn!(err = %e, "rejected graph event tenant mismatch");
            return MessageResult::Dropped;
        }
    };
    let payload = verified.payload;
    let claims = verified.claims;

    let doc_id = payload["document_id"].as_str().unwrap_or("").to_owned();
    if doc_id.is_empty() || org_id.is_empty() {
        return MessageResult::Dropped;
    }

    if claims.zdr {
        tracing::info!(
            org_id,
            "restrictive-ZDR graph event dropped without durable writes"
        );
        return MessageResult::Dropped;
    }

    tracing::info!(
        document_id = %doc_id,
        org_id,
        "processing document for graph extraction"
    );

    match pipeline::handle_document(ports, Origin::Stream, &org_id, &doc_id).await {
        Ok(report) => {
            tracing::info!(
                document_id = %doc_id,
                org_id,
                chunks = report.chunks,
                ready = report.ready,
                empty = report.empty,
                not_due = report.not_due,
                other = report.other,
                given_up = report.given_up,
                entities = report.entities,
                relationships = report.relationships,
                claims = report.claims,
                "graph extraction complete"
            );
            MessageResult::Processed { org_id, report }
        }
        Err(error) => {
            tracing::error!(
                %error,
                document_id = %doc_id,
                org_id,
                "graph extraction pass failed; the reconciler picks the document up from the database"
            );
            MessageResult::Failed
        }
    }
}

pub async fn run_consumer(
    consumer: PullConsumer,
    ports: Arc<Live>,
    event_verifier: Option<Arc<EventVerifier>>,
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
            let result = process_message(
                ports.as_ref(),
                event_verifier.as_deref(),
                &msg.payload,
                redelivery,
            )
            .await;

            // Ack BEFORE the community refresh: the refresh is a whole-org recompute (two bulk
            // queries + a replace tx), so running it inside the ack window would extend the
            // JetStream deadline under bulk ingest and risk redelivery. Post-ack keeps the
            // message settled; the per-org advisory lock in `replace_communities` serializes any
            // concurrent recompute so overlapping runs can't duplicate rows. The message is
            // acknowledged whatever came of it: a chunk that was not extracted has a state, and
            // the reconciler reads it.
            let _ = msg.ack().await;

            let MessageResult::Processed { org_id, report } = result else {
                continue;
            };

            // Refresh the org's derived communities when this document added relationships.
            // Best-effort: a failure is logged, never retried here (the message is already
            // acked), and detect_communities only replaces rows AFTER a successful listing: a
            // transient DB error cannot wipe existing communities.
            if report.relationships > 0 {
                if let Err(e) = crate::community::detect_communities(
                    ports.store.as_ref(),
                    &org_id,
                    community_min_size,
                )
                .await
                {
                    tracing::warn!(err = %e, org_id, "community refresh failed (non-fatal)");
                } else if let Err(e) =
                    crate::community::summarize_communities(ports.as_ref(), &org_id).await
                {
                    // P1-5: fill `graph_communities.summary` for communities that still lack
                    // one. Gated on `COMMUNITY_SUMMARY_ENABLED`, on the organization's policy,
                    // retention posture and budget (CR-T12), and capped per run, so a backlog
                    // drains over successive ingests instead of stalling one on many LLM calls.
                    // Only runs when detection succeeded: summarising against a half-updated
                    // community set would key summaries to memberships that never existed.
                    tracing::warn!(err = %e, org_id, "community summarisation failed (non-fatal)");
                }
            }

            // P1-4: flag claims this document contradicts. Post-ack for the same reason as the
            // community refresh. Best-effort and additive: a failure leaves existing flags
            // untouched, and the write only ever unions ids and sets `claim_status`, so a
            // partial run cannot erase prior findings.
            if !report.claim_ids.is_empty() {
                match ports
                    .store
                    .detect_claim_contradictions(&org_id, &report.claim_ids)
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

/// The consumer end to end through the real envelope verifier (design task CR-T12): a signed
/// event goes in, the pipeline runs, and what happens to a chunk that is given up on does not
/// depend on whether a verifier is configured.
#[cfg(test)]
mod consumer_tests {
    use super::{process_message, MessageResult, SUBJECT};
    use crate::pipeline::harness::{Control, Memory};
    use crate::pipeline::DeadLetter;
    use crate::state::ErrorClass;
    use crate::testing::{answer, capture_logs, fail, RICH_EXTRACTION};
    use event_envelope_rs::{EventSigner, EventVerifier};
    use rsa::pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey};
    use rsa::{RsaPrivateKey, RsaPublicKey};

    /// The producer the consumer pins: embedding-engine's key, as in production.
    fn keys() -> (EventSigner, EventVerifier) {
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
            10_000,
        )
        .expect("verifier");
        (signer, verifier)
    }

    fn indexed(signer: &EventSigner, org: &str, document: &str, zdr: bool) -> Vec<u8> {
        let payload = serde_json::json!({ "document_id": document, "org_id": org, "zdr": zdr });
        signer
            .sign(SUBJECT, org, None, zdr, payload.to_string().as_bytes())
            .expect("a signed event")
    }

    /// The defect: the dead letter was published only when NO verifier was configured, so in
    /// production, where events are signed, a chunk that failed for good was never dead-lettered.
    /// Here every event is signed and verified, and the sixth failure still dead-letters it,
    /// once.
    #[tokio::test]
    async fn a_chunk_that_is_given_up_on_is_dead_lettered_with_a_verifier_configured() {
        let (signer, verifier) = keys();
        let rig = Memory::new().await;
        let org = rig.org("dlq");
        let chunks = rig
            .add_document(
                &org,
                "poisoned",
                "org",
                "internal",
                &["a chunk nobody can extract"],
            )
            .await;
        rig.parts().inference.always(fail("provider_unavailable"));
        let (logs, _guard) = capture_logs();

        for attempt in 1..=7 {
            let envelope = indexed(&signer, &org, &format!("{org}-poisoned"), false);
            let result = process_message(&rig, Some(&verifier), &envelope, false).await;
            let MessageResult::Processed { report, .. } = result else {
                panic!("attempt {attempt}: a verified event is processed, got {result:?}");
            };
            assert_eq!(
                report.given_up,
                usize::from(attempt == 6),
                "attempt {attempt}"
            );
            rig.make_due(&org, &chunks[0]).await;
        }

        assert_eq!(
            rig.parts().inference.call_count(),
            6,
            "six attempts, then none"
        );
        assert_eq!(
            rig.letters(),
            vec![DeadLetter {
                org_id: org.clone(),
                document_id: format!("{org}-poisoned"),
                knowledge_id: chunks[0].clone(),
                error_class: ErrorClass::ProviderUnavailable,
                attempts: 6,
            }]
        );
        assert_eq!(logs.events_saying("graph extraction complete").len(), 7);
    }

    #[tokio::test]
    async fn a_verified_event_runs_the_document_through_the_pipeline() {
        let (signer, verifier) = keys();
        let rig = Memory::new().await;
        let org = rig.org("event");
        rig.add_document(&org, "d", "org", "internal", &["one", "two"])
            .await;
        rig.parts().inference.always(answer(RICH_EXTRACTION));

        let envelope = indexed(&signer, &org, &format!("{org}-d"), false);
        let MessageResult::Processed { org_id, report } =
            process_message(&rig, Some(&verifier), &envelope, false).await
        else {
            panic!("a verified event is processed");
        };
        assert_eq!(org_id, org);
        assert_eq!(
            (report.ready, report.relationships, report.claim_ids.len()),
            (2, 2, 2)
        );
    }

    /// Restrictive-ZDR events are dropped without a call and without a durable write: no
    /// state row is made for a document the event says must not be touched.
    #[tokio::test]
    async fn a_restrictive_zdr_event_is_dropped_with_no_call_and_no_state() {
        let (signer, verifier) = keys();
        let rig = Memory::new().await;
        let org = rig.org("zdr-event");
        let chunks = rig
            .add_document(&org, "d", "org", "internal", &["one"])
            .await;
        rig.parts().inference.always(answer(RICH_EXTRACTION));

        let envelope = indexed(&signer, &org, &format!("{org}-d"), true);
        assert!(matches!(
            process_message(&rig, Some(&verifier), &envelope, false).await,
            MessageResult::Dropped
        ));
        assert_eq!(rig.parts().inference.call_count(), 0);
        assert!(rig.row(&org, &chunks[0]).await.is_none());
    }

    /// An unsigned event is refused when a verifier is configured, and a replayed one is too:
    /// nothing about the state machine loosened the envelope's rules.
    #[tokio::test]
    async fn an_unsigned_or_replayed_event_is_dropped() {
        let (signer, verifier) = keys();
        let rig = Memory::new().await;
        let org = rig.org("forged");
        rig.add_document(&org, "d", "org", "internal", &["one"])
            .await;
        rig.parts().inference.always(answer(RICH_EXTRACTION));

        let raw = serde_json::json!({ "document_id": format!("{org}-d"), "org_id": org });
        assert!(matches!(
            process_message(&rig, Some(&verifier), raw.to_string().as_bytes(), false).await,
            MessageResult::Dropped
        ));
        let envelope = indexed(&signer, &org, &format!("{org}-d"), false);
        assert!(matches!(
            process_message(&rig, Some(&verifier), &envelope, false).await,
            MessageResult::Processed { .. }
        ));
        assert!(matches!(
            process_message(&rig, Some(&verifier), &envelope, false).await,
            MessageResult::Dropped
        ));
        assert_eq!(rig.parts().inference.call_count(), 1, "the first, once");
    }

    /// The text of the consumer has no path that depends on the verifier except how the envelope
    /// is read. A test over the text is crude, and exactly as strong as the regression it
    /// guards: someone gating the dead letter on it again.
    #[test]
    fn nothing_about_a_dead_letter_depends_on_the_verifier() {
        let source = include_str!("stream.rs");
        let production = source
            .split("mod signed_event_tests")
            .next()
            .expect("production code");
        assert!(
            !production.contains("event_verifier.is_none()")
                && !production.contains("verifier.is_none()"),
            "stream.rs gates something on the absence of a verifier"
        );
    }
}
