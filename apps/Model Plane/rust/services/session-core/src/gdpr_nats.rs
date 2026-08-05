//! NATS `JetStream` consumer for the cross-plane GDPR organization-erasure
//! fan-out (`verevon.gdpr.erasure.requested`).
//!
//! Mirrors `nats.rs`'s and `orchestration_nats.rs`'s established pattern for
//! this crate: the stream and this consumer's durable name/filter are
//! deployment-provisioned; runtime credentials can only bind/pull/ACK, never
//! create or mutate `JetStream` topology. See the two follow-ups below for
//! what still needs to happen outside this crate for that bind to succeed
//! and for the purge to be complete cross-plane.
//!
//! Decoding, subject-type gating (organization-only — see `gdpr.rs`'s
//! module docs for why per-user events must NOT trigger this purge), and the
//! actual hard-delete all live in [`crate::gdpr`]; this module is only the
//! NATS transport.
//!
//! This consumer connects over a dedicated shared-broker NATS session
//! (`NATS_SHARED_URL` / `NATS_SHARED_USER` / `NATS_SHARED_PASSWORD`, see
//! [`crate::nats_connection::connect_shared`]) pointed at
//! `control-shared-nats`, under the narrowly-scoped `session-core-gdpr`
//! identity — never session-core's own Model-Plane-local `NATS_URL`
//! connection (that broker does not host `AQENCIA_CONTROLPLANE`). This
//! mirrors documents-api-go's dual-client split: one client for its own
//! plane-local domain events, a second, separately-credentialed client only
//! for the cross-plane GDPR consumer.
//!
//! FOLLOW-UP (outside this crate — provisioning): the durable pull consumer
//! this module binds to ([`DURABLE_NAME`] on [`STREAM_NAME`], filtering
//! [`SUBJECT`]) does not exist yet. `audit-core`'s
//! `internal/provisioner/provisioner.go` (`ProvisionControlSharedRuntime`)
//! owns `AQENCIA_CONTROLPLANE`'s fixed consumers today — see
//! `gdprDocumentsConsumerConfig` there for the sibling Data-Plane consumer on
//! the same subject. A PULL consumer entry (no `DeliverSubject`, unlike that
//! push/queue one) for `session-core-gdpr-erasure-v1` needs to be added
//! there, the same way `session-core-tools` was added for `TOOLS_COMPLETIONS`,
//! before this binary can bind at startup. Until then, `run` below returns
//! an error from `get_consumer` and the supervised background task in
//! `main.rs` retries forever without making progress.
//!
//! FOLLOW-UP (outside this crate — cost-core): Model Plane cost/usage
//! records live in the sibling `cost-core` Go service's own Postgres
//! database (`apps/Model Plane/go/services/cost-core`), not reachable from
//! this crate. `cost-core` has no subscriber on this subject today; it needs
//! its own consumer to purge an erased org's cost/usage rows.

use async_nats::jetstream::{self, consumer::PullConsumer, AckKind};
use futures::StreamExt;
use metrics::{counter, histogram};
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

use crate::gdpr::{parse_erasure_event, purge_organization_data};
use crate::store::Pool;

const STREAM_NAME: &str = "AQENCIA_CONTROLPLANE";
const SUBJECT: &str = "verevon.gdpr.erasure.requested";
const DURABLE_NAME: &str = "session-core-gdpr-erasure-v1";

/// Run the GDPR erasure consumer until the connection is lost.
///
/// `nats_url` is the shared cross-plane broker (`control-shared-nats`), not
/// session-core's own Model-Plane-local `NATS_URL` — the caller in
/// `main.rs` passes `NATS_SHARED_URL` here, and connection uses the
/// dedicated `session-core-gdpr` shared-broker identity via
/// [`crate::nats_connection::connect_shared`].
///
/// # Errors
///
/// Returns an error if the initial NATS connection fails, the stream is
/// unavailable, or the pre-provisioned consumer is missing/misconfigured.
pub async fn run(pool: Pool, nats_url: String) -> anyhow::Result<()> {
    info!(%nats_url, subject = SUBJECT, "session-core GDPR erasure consumer connecting");

    let client = crate::nats_connection::connect_shared(&nats_url).await?;
    let js = jetstream::new(client);

    // Deployment provisioning owns this stream and its fixed consumer (see
    // the module-level FOLLOW-UP). Runtime credentials can bind/pull/ACK but
    // cannot mutate JetStream topology.
    let stream = js.get_stream(STREAM_NAME).await?;
    let consumer: PullConsumer = stream
        .get_consumer(DURABLE_NAME)
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    if consumer.cached_info().config.filter_subject != SUBJECT {
        return Err(anyhow::anyhow!(
            "pre-provisioned GDPR erasure consumer filter mismatch"
        ));
    }

    let mut messages = consumer.messages().await?;
    info!("session-core GDPR erasure consumer ready");

    while let Some(msg) = messages.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                warn!(error = %e, "GDPR erasure consumer receive error");
                counter!("mp_session_gdpr_erasure_total", "result" => "recv_error").increment(1);
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        };

        let started = Instant::now();
        match handle_message(&pool, &msg.payload).await {
            Ok(Outcome::Purged { org_id, rows }) => {
                histogram!("mp_session_gdpr_erasure_duration_seconds")
                    .record(started.elapsed().as_secs_f64());
                counter!("mp_session_gdpr_erasure_total", "result" => "purged").increment(1);
                info!(org_id = %org_id, rows, "session-core purged organization data");
                if let Err(e) = msg.ack().await {
                    warn!(error = %e, "ack failed after purge");
                }
            }
            Ok(Outcome::Skipped) => {
                counter!("mp_session_gdpr_erasure_total", "result" => "skipped").increment(1);
                if let Err(e) = msg.ack().await {
                    warn!(error = %e, "ack failed after skip");
                }
            }
            Err(e) => {
                histogram!("mp_session_gdpr_erasure_duration_seconds")
                    .record(started.elapsed().as_secs_f64());
                error!(error = %e, "failed to process GDPR erasure event");
                counter!("mp_session_gdpr_erasure_total", "result" => "error").increment(1);
                if let Err(ack_err) = msg.ack_with(AckKind::Nak(None)).await {
                    warn!(error = %ack_err, "nak failed");
                }
            }
        }
    }

    warn!("session-core GDPR erasure consumer message stream ended");
    Ok(())
}

enum Outcome {
    Purged { org_id: String, rows: u64 },
    Skipped,
}

async fn handle_message(pool: &Pool, payload: &[u8]) -> anyhow::Result<Outcome> {
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
