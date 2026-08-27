//! NATS `JetStream` consumer for the cross-plane GDPR organization-erasure
//! fan-out (`verevon.gdpr.erasure.requested`).
//!
//! Mirrors Model Plane's `session-core` `gdpr_nats.rs` pattern: the stream
//! and this consumer's durable name/filter are deployment-provisioned;
//! runtime credentials can only bind/pull/ACK, never create or mutate
//! `JetStream` topology.
//!
//! Decoding, subject-type gating, and the actual hard-delete all live in
//! [`crate::gdpr::event`] / [`crate::gdpr::purge`]; this module is only the
//! NATS transport.
//!
//! This consumer connects over a dedicated shared-broker NATS session
//! (`RETRIEVAL_ENGINE_GDPR_NATS_URL` / `RETRIEVAL_ENGINE_GDPR_NATS_USER` /
//! `RETRIEVAL_ENGINE_GDPR_NATS_PASSWORD`, see
//! [`nats_connection::connect_shared`]) pointed at `control-shared-nats`,
//! under the narrowly-scoped `retrieval-engine-gdpr` identity — NEVER this
//! service's own Data-Plane-local NATS connection (`DPV2_NATS_URL` /
//! `SHARED_NATS_URL` / `NATS_URL` / `NATS_LOCAL_URL` in `main.rs`): that
//! broker does not host `AQENCIA_CONTROLPLANE`, and `SHARED_NATS_URL` is
//! already claimed there for this service's own plane-local connection
//! fallback chain — reusing it (or its near-identical sibling name
//! `NATS_SHARED_URL`, which sibling services use for their own dedicated
//! GDPR identity) for this unrelated cross-plane identity would be exactly
//! the "reused an existing connection/identity" mistake the GDPR rollout's
//! verify pass already caught once elsewhere. Hence a fully distinct,
//! service-scoped env var trio here.
//!
//! Unlike `session-core`'s consumer, this one binds via
//! [`async_nats::jetstream::Context::get_consumer_from_stream`] rather than
//! `get_stream(..).get_consumer(..)`: the former issues only a
//! `CONSUMER.INFO.<stream>.<consumer>` request, so this identity needs
//! `CONSUMER.INFO`/`CONSUMER.MSG.NEXT`/`ACK` permissions only — NOT
//! `STREAM.INFO` (the permission gap that silently broke session-core's
//! consumer, per the GDPR rollout's fix pass).
//!
//! PROVISIONING: the durable pull consumer this module binds to
//! ([`DURABLE_NAME`] on [`STREAM_NAME`], filtering [`SUBJECT`]) is
//! provisioned by `audit-core`'s `internal/provisioner/provisioner.go`
//! (`ProvisionControlSharedRuntime`), and the `retrieval-engine-gdpr`
//! identity's permission block lives in `control-shared-nats.conf`. If
//! either is ever missing or out of sync, `run` below returns an error from
//! `get_consumer_from_stream` and the retry loop below keeps retrying
//! without making progress.

use async_nats::jetstream::{self, consumer::PullConsumer, AckKind};
use futures::StreamExt;
use metrics::{counter, gauge, histogram};
use sqlx::PgPool;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

use super::{parse_erasure_event, purge_organization_data};

pub const STREAM_NAME: &str = "AQENCIA_CONTROLPLANE";
pub const SUBJECT: &str = "verevon.gdpr.erasure.requested";
pub const DURABLE_NAME: &str = "retrieval-engine-gdpr-erasure-v1";

const INBOX_PREFIX: &str = "_INBOX.RETRIEVAL_ENGINE_GDPR";

/// Reconnect backoff between failed connect/bind attempts, so a
/// misconfigured or momentarily-unavailable shared broker doesn't hot-loop.
const RECONNECT_BACKOFF: Duration = Duration::from_secs(5);

/// Erasure-consumer readiness is tracked in the shared crate
/// (`nats_connection::erasure_health`) rather than here: the failure it guards
/// is identical in all five Rust services on this subject, and five copies of a
/// state machine is how they drift apart.
pub use nats_connection::erasure_health::{readiness, ErasureReadiness};
use nats_connection::erasure_health::{mark_connected, mark_enabled};

/// Run the GDPR erasure consumer until the process shuts down, reconnecting
/// with a fixed backoff on any connect/bind failure or stream-end. Never
/// returns under normal operation; the caller should `tokio::spawn` this and
/// treat a returned error as a supervised-task log line, not a fatal one —
/// the rest of the service must keep serving traffic if the shared broker is
/// unreachable.
///
/// Retrying forever is why this failure used to be invisible, so the loop now
/// also publishes its state: [`mark_connected`] drives `/readyz`, which starts
/// failing once the outage outlives the shared grace window.
pub async fn run_supervised(
    pool: PgPool,
    nats_url: String,
    nats_user: String,
    nats_password: String,
) {
    mark_enabled();
    loop {
        if let Err(e) = run_once(&pool, &nats_url, &nats_user, &nats_password).await {
            mark_connected(false);
            gauge!("dpv2_retrieval_gdpr_erasure_connected").set(0);
            // Escalate the wording once past the grace window: the plain retry
            // line is exactly what made this easy to scroll past.
            if let ErasureReadiness::Stalled { seconds_down } = readiness() {
                error!(
                    error = %e,
                    seconds_down,
                    "retrieval-engine GDPR erasure consumer STALLED past the readiness grace; \
                     org erasure is not being applied and /readyz is now failing"
                );
            } else {
                error!(error = %e, "retrieval-engine GDPR erasure consumer stopped; retrying");
            }
            counter!("dpv2_retrieval_gdpr_erasure_total", "result" => "connect_error").increment(1);
        }
        tokio::time::sleep(RECONNECT_BACKOFF).await;
    }
}

async fn run_once(
    pool: &PgPool,
    nats_url: &str,
    nats_user: &str,
    nats_password: &str,
) -> anyhow::Result<()> {
    info!(%nats_url, subject = SUBJECT, "retrieval-engine GDPR erasure consumer connecting");

    let client =
        nats_connection::connect_shared(nats_url, nats_user, nats_password, INBOX_PREFIX).await?;
    let js = jetstream::new(client);

    // Deployment provisioning owns this stream and its fixed consumer (see
    // the module-level FOLLOW-UP). Runtime credentials can bind/pull/ACK but
    // cannot mutate JetStream topology. `get_consumer_from_stream` avoids the
    // STREAM.INFO permission `get_stream(..).get_consumer(..)` would require
    // (see module docs).
    let consumer: PullConsumer = js
        .get_consumer_from_stream(DURABLE_NAME, STREAM_NAME)
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    if consumer.cached_info().config.filter_subject != SUBJECT {
        return Err(anyhow::anyhow!(
            "pre-provisioned GDPR erasure consumer filter mismatch"
        ));
    }

    let mut messages = consumer.messages().await?;
    mark_connected(true);
    gauge!("dpv2_retrieval_gdpr_erasure_connected").set(1);
    info!("retrieval-engine GDPR erasure consumer ready");

    while let Some(msg) = messages.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                warn!(error = %e, "GDPR erasure consumer receive error");
                counter!("dpv2_retrieval_gdpr_erasure_total", "result" => "recv_error")
                    .increment(1);
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        };

        let started = Instant::now();
        match handle_message(pool, &msg.payload).await {
            Ok(Outcome::Purged { org_id, rows }) => {
                histogram!("dpv2_retrieval_gdpr_erasure_duration_seconds")
                    .record(started.elapsed().as_secs_f64());
                counter!("dpv2_retrieval_gdpr_erasure_total", "result" => "purged").increment(1);
                info!(org_id = %org_id, rows, "retrieval-engine purged organization data");
                if let Err(e) = msg.ack().await {
                    warn!(error = %e, "ack failed after purge");
                }
            }
            Ok(Outcome::Skipped) => {
                counter!("dpv2_retrieval_gdpr_erasure_total", "result" => "skipped").increment(1);
                if let Err(e) = msg.ack().await {
                    warn!(error = %e, "ack failed after skip");
                }
            }
            Err(e) => {
                histogram!("dpv2_retrieval_gdpr_erasure_duration_seconds")
                    .record(started.elapsed().as_secs_f64());
                error!(error = %e, "failed to process GDPR erasure event");
                counter!("dpv2_retrieval_gdpr_erasure_total", "result" => "error").increment(1);
                if let Err(ack_err) = msg.ack_with(AckKind::Nak(None)).await {
                    warn!(error = %ack_err, "nak failed");
                }
            }
        }
    }

    warn!("retrieval-engine GDPR erasure consumer message stream ended");
    Ok(())
}

enum Outcome {
    Purged { org_id: String, rows: u64 },
    Skipped,
}

async fn handle_message(pool: &PgPool, payload: &[u8]) -> anyhow::Result<Outcome> {
    let Some(erasure) = parse_erasure_event(payload).map_err(|e| anyhow::anyhow!(e.to_string()))?
    else {
        return Ok(Outcome::Skipped);
    };

    let summary = purge_organization_data(pool, &erasure.org_id).await?;
    Ok(Outcome::Purged {
        org_id: erasure.org_id,
        rows: summary.total(),
    })
}
