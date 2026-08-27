//! NATS `JetStream` consumer for the cross-plane GDPR organization-erasure
//! fan-out (`verevon.gdpr.erasure.requested`) on the shared cross-plane
//! broker (`control-shared-nats`, stream `AQENCIA_CONTROLPLANE`).
//!
//! Decoding, subject-type gating (organization-only — see `gdpr.rs`'s
//! module docs for why per-user events must NOT trigger this purge), and the
//! actual hard-delete all live in [`crate::gdpr`]; this module is only the
//! NATS transport.
//!
//! This consumer connects over a **dedicated** shared-broker NATS session
//! (`NATS_SHARED_URL` / `NATS_SHARED_USER` / `NATS_SHARED_PASSWORD`, see
//! [`connect_shared`]) under the narrowly-scoped `index-engine-gdpr`
//! identity — never index-engine-rs's own Data-Plane-local
//! `NATS_URL`/`DATAPLANE_NATS_TOKEN` connection to `data-nats` via
//! `nats_connection::connect` (that broker does not host
//! `AQENCIA_CONTROLPLANE`), and never any other service's shared-broker
//! identity.
//!
//! This binds to the pre-provisioned consumer via
//! [`async_nats::jetstream::Context::get_consumer_from_stream`], which
//! resolves the consumer directly from `CONSUMER.INFO.<stream>.<consumer>`
//! **without** first calling `Context::get_stream`
//! (`STREAM.INFO.<stream>`) — the crate's own docs note this saves a
//! server round trip when binding to a single consumer. This identity
//! therefore does not need `$JS.API.STREAM.INFO.AQENCIA_CONTROLPLANE`
//! permission at all, deliberately avoiding the `get_stream`-before-
//! `get_consumer` pattern that required an extra permission grant for
//! Model Plane session-core's `gdpr_nats.rs` (`apps/Model Plane/rust/
//! services/session-core/src/gdpr_nats.rs`, which calls `js.get_stream(..)`
//! before `stream.get_consumer(..)` and consequently needs
//! `$JS.API.STREAM.INFO.AQENCIA_CONTROLPLANE` granted to `session-core-gdpr`
//! in `control-shared-nats.conf`). Runtime credentials here can only
//! bind/pull/ACK a pre-existing consumer — never create or mutate JetStream
//! topology (no `CONSUMER.CREATE`/`STREAM.CREATE` rights).
//!
//! PROVISIONING: the durable **pull** consumer this module binds to
//! ([`DURABLE_NAME`] on [`STREAM_NAME`], filtering [`SUBJECT`]) is
//! provisioned by `audit-core`'s `internal/provisioner/provisioner.go`
//! (`ProvisionControlSharedRuntime`), and the `index-engine-gdpr` identity's
//! permission block lives in `control-shared-nats.conf`. If either is ever
//! missing or out of sync, `run` below returns an error and the caller's
//! supervised background task retries forever without making progress.

use async_nats::jetstream::{self, consumer::PullConsumer, AckKind};
use futures::StreamExt;
use sqlx::PgPool;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

use crate::gdpr::{parse_erasure_event, purge_organization_data};

const STREAM_NAME: &str = "AQENCIA_CONTROLPLANE";
const SUBJECT: &str = "verevon.gdpr.erasure.requested";
const DURABLE_NAME: &str = "index-engine-org-erasure";
const INBOX_PREFIX: &str = "_INBOX.INDEX_ENGINE_GDPR";

/// Run the GDPR erasure consumer until the connection is lost.
///
/// `nats_url` is the shared cross-plane broker (`control-shared-nats`), not
/// index-engine-rs's own Data-Plane-local `NATS_URL` — the caller passes
/// the dedicated `NATS_SHARED_URL` here, and connection uses the dedicated
/// `index-engine-gdpr` shared-broker identity via [`connect_shared`].
///
/// # Errors
///
/// Returns an error if the initial NATS connection fails, or the
/// pre-provisioned consumer is missing/misconfigured.
pub async fn run(pool: PgPool, nats_url: String) -> anyhow::Result<()> {
    info!(%nats_url, subject = SUBJECT, "index-engine GDPR erasure consumer connecting");

    let client = connect_shared(&nats_url).await?;
    let js = jetstream::new(client);

    // Deployment provisioning owns this stream and its fixed consumer (see
    // the module-level FOLLOW-UP). Runtime credentials can bind/pull/ACK but
    // cannot mutate JetStream topology. Resolved straight from Context (no
    // get_stream call) so this identity needs no STREAM.INFO permission.
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
    nats_connection::erasure_health::mark_connected(true);
    info!("index-engine GDPR erasure consumer ready");

    while let Some(msg) = messages.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                warn!(error = %e, "GDPR erasure consumer receive error");
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        };

        let started = Instant::now();
        match handle_message(&pool, &msg.payload).await {
            Ok(Outcome::Purged { org_id, rows }) => {
                info!(
                    org_id = %org_id,
                    rows,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "index-engine purged organization data"
                );
                if let Err(e) = msg.ack().await {
                    warn!(error = %e, "ack failed after purge");
                }
            }
            Ok(Outcome::Skipped) => {
                if let Err(e) = msg.ack().await {
                    warn!(error = %e, "ack failed after skip");
                }
            }
            Err(e) => {
                error!(error = %e, "failed to process GDPR erasure event");
                if let Err(ack_err) = msg.ack_with(AckKind::Nak(None)).await {
                    warn!(error = %ack_err, "nak failed");
                }
            }
        }
    }

    warn!("index-engine GDPR erasure consumer message stream ended");
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

/// Connect to the shared cross-plane broker (`control-shared-nats`) under
/// the narrowly-scoped `index-engine-gdpr` identity used only by this
/// consumer.
///
/// Deliberately separate from `nats_connection::connect` (the
/// `nats-connection-rs` shared crate used by `main.rs` for index-engine's
/// own Data-Plane-local `data-nats` broker under a bearer token) — that
/// connection is a different NATS session, under different credentials,
/// against a different broker that does not host `AQENCIA_CONTROLPLANE`.
/// `NATS_SHARED_USER`/`NATS_SHARED_PASSWORD` are read directly with no
/// fallback, so a missing/misconfigured shared identity fails the
/// connection attempt (retried by the caller's supervised background loop)
/// rather than silently connecting unauthenticated to a broker this crate
/// does not own.
async fn connect_shared(url: &str) -> Result<async_nats::Client, async_nats::ConnectError> {
    let user = std::env::var("NATS_SHARED_USER").unwrap_or_default();
    let password = std::env::var("NATS_SHARED_PASSWORD").unwrap_or_default();
    async_nats::ConnectOptions::with_user_and_password(user, password)
        .custom_inbox_prefix(INBOX_PREFIX)
        .connect(url)
        .await
}
