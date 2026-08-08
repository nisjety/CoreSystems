//! `JetStream` pull consumer for the cross-plane GDPR organization-erasure
//! fan-out (`verevon.gdpr.erasure.requested`).
//!
//! Mirrors Data Plane v2 siblings `graph-index-rs`'s and
//! `retrieval-engine-rs`'s `gdpr_nats`/`gdpr::consumer` pattern (itself
//! modeled on Model Plane `session-core`'s `gdpr_nats.rs`): the stream and
//! this consumer's durable name/filter are deployment-provisioned; runtime
//! credentials can only bind/pull/ACK, never create or mutate `JetStream`
//! topology.
//!
//! Decoding and subject-type gating live in [`crate::gdpr`]; the actual
//! Qdrant purge lives in [`crate::gdpr::purge_organization_data`]; this
//! module is only the NATS transport.
//!
//! This consumer connects over a dedicated shared-broker NATS session
//! (`EMBEDDING_ENGINE_GDPR_NATS_URL` / `EMBEDDING_ENGINE_GDPR_NATS_USER` /
//! `EMBEDDING_ENGINE_GDPR_NATS_PASSWORD`, via the shared
//! [`nats_connection::connect_shared`] this crate already depends on),
//! pointed at `control-shared-nats`, under the narrowly-scoped
//! `embedding-engine-gdpr` identity — NEVER this service's own
//! Data-Plane-local NATS connection (`Config::nats_url` /
//! `nats_connection::connect`, used by `stream.rs`/`wiki_consumer.rs`/
//! `image_consumer.rs`): that broker does not host `AQENCIA_CONTROLPLANE`,
//! and reusing that connection/identity (or any other service's
//! shared-broker identity) would be exactly the "reused an existing
//! connection/identity" mistake this GDPR rollout's verify pass already
//! caught once elsewhere. Hence a fully distinct, service-scoped env var
//! trio here — matching `graph-index-gdpr`'s and `retrieval-engine-gdpr`'s
//! naming convention, not the generic `NATS_SHARED_URL` some earlier
//! services in this rollout used for the same purpose.
//!
//! Unlike Model Plane `session-core`'s consumer (which calls
//! `Context::get_stream(..).get_consumer(..)`), this one binds via
//! [`async_nats::jetstream::Context::get_consumer_from_stream`]: the former
//! issues a `STREAM.INFO` request before the `CONSUMER.INFO` lookup, so that
//! identity needs `$JS.API.STREAM.INFO.<stream>` permission too — the
//! permission gap that silently broke session-core's consumer, per the GDPR
//! rollout's fix pass. `get_consumer_from_stream` issues only
//! `CONSUMER.INFO.<stream>.<consumer>`, so the `embedding-engine-gdpr`
//! identity needs `CONSUMER.INFO` / `CONSUMER.MSG.NEXT` / `ACK` permissions
//! only — NOT `STREAM.INFO`. This crate depends on the same async-nats
//! 0.49.1 client as `graph-index-rs`/`retrieval-engine-rs`/`index-engine-rs`
//! (workspace-pinned in `Data Plane v2/Cargo.toml`), so the same
//! `get_consumer_from_stream` behavior applies here.
//!
//! PROVISIONING: the durable pull consumer this module binds to
//! ([`DURABLE_NAME`] on [`STREAM_NAME`], filtering [`SUBJECT`]) is
//! provisioned by `audit-core`'s `internal/provisioner/provisioner.go`
//! (`ProvisionControlSharedRuntime`, see `embeddingEngineOrgErasureConsumerConfig`
//! there), and the `embedding-engine-gdpr` identity's permission block lives
//! in `control-shared-nats.conf`. If either is ever missing or out of sync,
//! `run_once` below returns an error from `get_consumer_from_stream` and
//! `run_supervised` retries forever without making progress.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_nats::jetstream::{self, consumer::PullConsumer, AckKind};
use futures::StreamExt;
use qdrant_client::Qdrant;

use crate::cas_store::CasStore;
use crate::gdpr::{parse_erasure_event, purge_organization_data, PurgeCollections};

pub const STREAM_NAME: &str = "AQENCIA_CONTROLPLANE";
pub const SUBJECT: &str = "verevon.gdpr.erasure.requested";
pub const DURABLE_NAME: &str = "embedding-engine-org-erasure";

const INBOX_PREFIX: &str = "_INBOX.EMBEDDING_ENGINE_GDPR";

/// Reconnect backoff between failed connect/bind attempts, so a
/// misconfigured or momentarily-unavailable shared broker doesn't hot-loop.
const RECONNECT_BACKOFF: Duration = Duration::from_secs(5);

/// Run the GDPR erasure consumer until the process shuts down, reconnecting
/// with a fixed backoff on any connect/bind failure or stream-end. Never
/// returns under normal operation. `main.rs` spawns this as an independent
/// supervised task (not raced inside its `tokio::select!`) so a
/// shared-broker outage never takes down the admin HTTP server or the
/// embedding/wiki/page-image consumers.
pub async fn run_supervised(
    qdrant: Qdrant,
    collections: Arc<PurgeCollections>,
    cas: Option<Arc<CasStore>>,
    nats_url: String,
    nats_user: String,
    nats_password: String,
) {
    loop {
        if let Err(e) = run_once(
            &qdrant,
            &collections,
            cas.as_deref(),
            &nats_url,
            &nats_user,
            &nats_password,
        )
        .await
        {
            tracing::error!(err = %e, "embedding-engine GDPR erasure consumer stopped; retrying");
        }
        tokio::time::sleep(RECONNECT_BACKOFF).await;
    }
}

async fn run_once(
    qdrant: &Qdrant,
    collections: &PurgeCollections,
    cas: Option<&CasStore>,
    nats_url: &str,
    nats_user: &str,
    nats_password: &str,
) -> anyhow::Result<()> {
    tracing::info!(
        %nats_url,
        subject = SUBJECT,
        "embedding-engine GDPR erasure consumer connecting"
    );

    let client =
        nats_connection::connect_shared(nats_url, nats_user, nats_password, INBOX_PREFIX).await?;
    let js = jetstream::new(client);

    // Deployment provisioning owns this stream and its fixed consumer (see
    // the module-level PROVISIONING note). This identity can bind/pull/ACK
    // but cannot create or mutate JetStream topology.
    // `get_consumer_from_stream` avoids the STREAM.INFO permission
    // `get_stream(..).get_consumer(..)` would require (see module docs).
    let consumer: PullConsumer = js
        .get_consumer_from_stream(DURABLE_NAME, STREAM_NAME)
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    if consumer.cached_info().config.filter_subject != SUBJECT {
        anyhow::bail!("pre-provisioned GDPR erasure consumer filter mismatch");
    }

    let mut messages = consumer.messages().await?;
    tracing::info!("embedding-engine GDPR erasure consumer ready");

    while let Some(msg) = messages.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(err = %e, "GDPR erasure consumer receive error");
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        };

        let started = Instant::now();
        match handle_message(qdrant, collections, cas, &msg.payload).await {
            Ok(Outcome::Purged { org_id, summary }) => {
                tracing::info!(
                    org_id = %org_id,
                    points = summary.total(),
                    cas_objects = summary.cas_objects,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "embedding-engine purged organization vectors and CAS objects"
                );
                if let Err(e) = msg.ack().await {
                    tracing::warn!(err = %e, "ack failed after purge");
                }
            }
            Ok(Outcome::Skipped) => {
                if let Err(e) = msg.ack().await {
                    tracing::warn!(err = %e, "ack failed after skip");
                }
            }
            Err(e) => {
                tracing::error!(err = %e, "failed to process GDPR erasure event");
                if let Err(ack_err) = msg.ack_with(AckKind::Nak(None)).await {
                    tracing::warn!(err = %ack_err, "nak failed");
                }
            }
        }
    }

    tracing::warn!("embedding-engine GDPR erasure consumer message stream ended");
    Ok(())
}

enum Outcome {
    Purged {
        org_id: String,
        summary: crate::gdpr::PurgeSummary,
    },
    Skipped,
}

async fn handle_message(
    qdrant: &Qdrant,
    collections: &PurgeCollections,
    cas: Option<&CasStore>,
    payload: &[u8],
) -> anyhow::Result<Outcome> {
    let Some(erasure) = parse_erasure_event(payload).map_err(|e| anyhow::anyhow!(e.to_string()))?
    else {
        return Ok(Outcome::Skipped);
    };

    let summary = purge_organization_data(qdrant, collections, cas, &erasure.org_id).await?;
    Ok(Outcome::Purged {
        org_id: erasure.org_id,
        summary,
    })
}
