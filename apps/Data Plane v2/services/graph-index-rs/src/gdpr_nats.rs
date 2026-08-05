//! `JetStream` pull consumer for the cross-plane GDPR organization-erasure
//! fan-out (`verevon.gdpr.erasure.requested`).
//!
//! Mirrors Data Plane v2 sibling `retrieval-engine-rs`'s `gdpr::consumer`
//! pattern (itself modeled on Model Plane `session-core`'s `gdpr_nats.rs`):
//! the stream and this consumer's durable name/filter are
//! deployment-provisioned; runtime credentials can only bind/pull/ACK, never
//! create or mutate `JetStream` topology.
//!
//! Decoding and subject-type gating live in [`crate::gdpr`]; the actual
//! hard-delete lives in [`crate::store::GraphStore::purge_organization_data`];
//! this module is only the NATS transport.
//!
//! This consumer connects over a dedicated shared-broker NATS session
//! (`GRAPH_INDEX_GDPR_NATS_URL` / `GRAPH_INDEX_GDPR_NATS_USER` /
//! `GRAPH_INDEX_GDPR_NATS_PASSWORD`, via the shared
//! [`nats_connection::connect_shared`] this crate already depends on),
//! pointed at `control-shared-nats`, under the narrowly-scoped
//! `graph-index-gdpr` identity — NEVER this service's own Data-Plane-local
//! NATS connection (`Config::nats_url` / `nats_connection::connect`, used by
//! `stream.rs`): that broker does not host `AQENCIA_CONTROLPLANE`, and reusing
//! that connection/identity (or any other service's shared-broker identity)
//! would be exactly the "reused an existing connection/identity" mistake the
//! GDPR rollout's verify pass already caught once elsewhere. Hence a fully
//! distinct, service-scoped env var trio here — matching
//! `retrieval-engine-gdpr`'s and sibling `*_GDPR_NATS_*` identities' naming,
//! not the generic `NATS_SHARED_URL`/`SHARED_NATS_URL` some other services
//! use for the same purpose.
//!
//! Unlike Model Plane `session-core`'s consumer (which calls
//! `Context::get_stream(..).get_consumer(..)`), this one binds via
//! [`async_nats::jetstream::Context::get_consumer_from_stream`]: the former
//! issues a `STREAM.INFO` request before the `CONSUMER.INFO` lookup, so that
//! identity needs `$JS.API.STREAM.INFO.<stream>` permission too — the
//! permission gap that silently broke session-core's consumer, per the GDPR
//! rollout's fix pass. `get_consumer_from_stream` issues only
//! `CONSUMER.INFO.<stream>.<consumer>`, so the `graph-index-gdpr` identity
//! needs `CONSUMER.INFO` / `CONSUMER.MSG.NEXT` / `ACK` permissions only — NOT
//! `STREAM.INFO`. This mirrors `retrieval-engine-rs`'s `gdpr::consumer`,
//! which adopted the same fix on the same async-nats 0.49.1 client.
//!
//! PROVISIONING: the durable pull consumer this module binds to
//! ([`DURABLE_NAME`] on [`STREAM_NAME`], filtering [`SUBJECT`]) is
//! provisioned by `audit-core`'s `internal/provisioner/provisioner.go`
//! (`ProvisionControlSharedRuntime`, see `graphIndexGDPRErasureConsumerConfig`
//! there), and the `graph-index-gdpr` identity's permission block lives in
//! `control-shared-nats.conf`. If either is ever missing or out of sync,
//! `run_once` below returns an error from `get_consumer_from_stream` and
//! `run_supervised` retries forever without making progress.
//!
//! FOLLOW-UP (outside this crate — sibling Data Plane v2 services): this
//! closes only graph-index-rs's slice of Data Plane v2 org erasure.
//! `embedding-engine-rs` / `index-engine-rs` (`knowledge_units`),
//! `wiki-store-go`, `data-quality-go` / `data-orchestrator-go`, and
//! `quickwit-adapter-rs` each still need their own consumer on this subject
//! (`retrieval-engine-rs` and `documents-api-go` are already done) — see
//! `documents-api-go`'s `internal/gdpr/org_purge.go` module doc for the full
//! list.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_nats::jetstream::{self, consumer::PullConsumer, AckKind};
use futures::StreamExt;

use crate::gdpr::parse_erasure_event;
use crate::store::GraphStore;

pub const STREAM_NAME: &str = "AQENCIA_CONTROLPLANE";
pub const SUBJECT: &str = "verevon.gdpr.erasure.requested";
pub const DURABLE_NAME: &str = "graph-index-gdpr-erasure-v1";

const INBOX_PREFIX: &str = "_INBOX.GRAPH_INDEX_GDPR";

/// Reconnect backoff between failed connect/bind attempts, so a
/// misconfigured or momentarily-unavailable shared broker doesn't hot-loop.
const RECONNECT_BACKOFF: Duration = Duration::from_secs(5);

/// Run the GDPR erasure consumer until the process shuts down, reconnecting
/// with a fixed backoff on any connect/bind failure or stream-end. Never
/// returns under normal operation. `main.rs` spawns this as an independent
/// task (not raced inside its `tokio::select!`) so a shared-broker outage
/// never takes down HTTP/gRPC/the graph-extraction consumers.
pub async fn run_supervised(
    store: Arc<GraphStore>,
    nats_url: String,
    nats_user: String,
    nats_password: String,
) {
    loop {
        if let Err(e) = run_once(&store, &nats_url, &nats_user, &nats_password).await {
            tracing::error!(err = %e, "graph-index GDPR erasure consumer stopped; retrying");
        }
        tokio::time::sleep(RECONNECT_BACKOFF).await;
    }
}

async fn run_once(
    store: &Arc<GraphStore>,
    nats_url: &str,
    nats_user: &str,
    nats_password: &str,
) -> anyhow::Result<()> {
    tracing::info!(
        %nats_url,
        subject = SUBJECT,
        "graph-index GDPR erasure consumer connecting"
    );

    let client =
        nats_connection::connect_shared(nats_url, nats_user, nats_password, INBOX_PREFIX).await?;
    let js = jetstream::new(client);

    // Deployment provisioning owns this stream and its fixed consumer (see
    // the module-level FOLLOW-UP). This identity can bind/pull/ACK but
    // cannot create or mutate JetStream topology. `get_consumer_from_stream`
    // avoids the STREAM.INFO permission `get_stream(..).get_consumer(..)`
    // would require (see module docs).
    let consumer: PullConsumer = js
        .get_consumer_from_stream(DURABLE_NAME, STREAM_NAME)
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    if consumer.cached_info().config.filter_subject != SUBJECT {
        anyhow::bail!("pre-provisioned GDPR erasure consumer filter mismatch");
    }

    let mut messages = consumer.messages().await?;
    tracing::info!("graph-index GDPR erasure consumer ready");

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
        match handle_message(store, &msg.payload).await {
            Ok(Outcome::Purged { org_id, rows }) => {
                tracing::info!(
                    org_id = %org_id,
                    rows,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "graph-index purged organization graph data"
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

    tracing::warn!("graph-index GDPR erasure consumer message stream ended");
    Ok(())
}

enum Outcome {
    Purged { org_id: String, rows: u64 },
    Skipped,
}

async fn handle_message(store: &GraphStore, payload: &[u8]) -> anyhow::Result<Outcome> {
    let Some(erasure) = parse_erasure_event(payload).map_err(|e| anyhow::anyhow!(e.to_string()))?
    else {
        return Ok(Outcome::Skipped);
    };

    let summary = store.purge_organization_data(&erasure.org_id).await?;
    Ok(Outcome::Purged {
        org_id: erasure.org_id,
        rows: summary.total(),
    })
}
