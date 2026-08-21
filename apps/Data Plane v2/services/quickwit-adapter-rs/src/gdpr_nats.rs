//! NATS `JetStream` consumer for the cross-plane GDPR organization-erasure
//! fan-out (`verevon.gdpr.erasure.requested`).
//!
//! Mirrors Model Plane session-core's `gdpr_nats.rs` pattern (same fan-out
//! contract, same shared-broker isolation rules): the stream and this
//! consumer's durable name/filter are deployment-provisioned; runtime
//! credentials can only bind/pull/ACK, never create or mutate `JetStream`
//! topology.
//!
//! Decoding, subject-type gating (organization-only — see `gdpr.rs`'s
//! module docs for why per-user events must NOT trigger this purge), and the
//! actual hard-delete all live in [`crate::gdpr`]; this module is only the
//! NATS transport.
//!
//! This consumer connects over a dedicated shared-broker NATS session
//! (`NATS_SHARED_URL` / `NATS_SHARED_USER` / `NATS_SHARED_PASSWORD`) pointed
//! at `control-shared-nats`, under the narrowly-scoped `quickwit-adapter-gdpr`
//! identity — never this crate's own Data-Plane-local `NATS_URL` connection
//! (that broker, reached via `nats-connection-rs`'s `DATAPLANE_NATS_TOKEN`
//! auth, does not host `AQENCIA_CONTROLPLANE`, and its token-only auth model
//! is not the shared broker's username/password scheme anyway). This mirrors
//! documents-api-go's and session-core's dual-client split: one client for
//! this crate's own plane-local domain events (`stream.rs`), a second,
//! separately-credentialed client only for the cross-plane GDPR consumer.
//!
//! PROVISIONING: the durable pull consumer this module binds to
//! ([`DURABLE_NAME`] on [`STREAM_NAME`], filtering [`SUBJECT`]) is
//! provisioned by `audit-core`'s `internal/provisioner/provisioner.go`
//! (`ProvisionControlSharedRuntime`, see `sessionGDPRErasureConsumerConfig`
//! there for the sibling Model-Plane consumer on the same subject), and the
//! `quickwit-adapter-gdpr` identity's permission block lives in
//! `control-shared-nats.conf`. If either is ever missing or out of sync,
//! `run` below returns an error from `get_consumer` and the caller's retry
//! loop in `main.rs` retries forever without making progress.

use async_nats::jetstream::{self, consumer::PullConsumer, AckKind};
use futures::StreamExt;
use sqlx::PgPool;
use tracing::{error, info, warn};

use crate::gdpr::{parse_erasure_event, purge_organization_data};

const STREAM_NAME: &str = "AQENCIA_CONTROLPLANE";
const SUBJECT: &str = "verevon.gdpr.erasure.requested";
const DURABLE_NAME: &str = "quickwit-adapter-gdpr-erasure-v1";

/// Inbox prefix for the dedicated shared-broker connection used by the GDPR
/// erasure consumer. Kept distinct from any plane-local inbox this crate may
/// use in the future, because the two connections are separate NATS
/// sessions under separate credentials against separate brokers.
const SHARED_INBOX_PREFIX: &str = "_INBOX.QUICKWIT_ADAPTER_GDPR";

/// Run the GDPR erasure consumer until the connection is lost.
///
/// `nats_url` is the shared cross-plane broker (`control-shared-nats`), not
/// this crate's own Data-Plane-local `NATS_URL` — the caller in `main.rs`
/// passes `NATS_SHARED_URL` here, and connection uses the dedicated
/// `quickwit-adapter-gdpr` shared-broker identity via [`connect_shared`].
///
/// # Errors
///
/// Returns an error if the initial NATS connection fails, the stream is
/// unavailable, or the pre-provisioned consumer is missing/misconfigured.
pub async fn run(
    pool: PgPool,
    nats_url: String,
    quickwit: crate::quickwit::QuickwitClient,
) -> anyhow::Result<()> {
    info!(%nats_url, subject = SUBJECT, "quickwit-adapter GDPR erasure consumer connecting");

    let client = connect_shared(&nats_url).await?;
    let js = jetstream::new(client);

    // Deployment provisioning owns this stream and its fixed consumer (see
    // the module-level FOLLOW-UP). Runtime credentials can bind/pull/ACK but
    // cannot mutate JetStream topology. `get_stream` (rather than
    // constructing a `Stream` handle locally) is required by this crate's
    // async-nats 0.49.1 API to look up the consumer below, and itself needs
    // `$JS.API.STREAM.INFO.AQENCIA_CONTROLPLANE` publish permission on the
    // `quickwit-adapter-gdpr` identity — see the report for this build.
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
    info!("quickwit-adapter GDPR erasure consumer ready");

    while let Some(msg) = messages.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                warn!(error = %e, "GDPR erasure consumer receive error");
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                continue;
            }
        };

        match handle_message(&pool, &quickwit, &msg.payload).await {
            Ok(Outcome::Purged {
                org_id,
                rows,
                index_pruned,
            }) => {
                if index_pruned {
                    info!(
                        org_id = %org_id,
                        rows,
                        index_pruned,
                        "quickwit-adapter purged organization data and submitted an index delete task"
                    );
                } else {
                    // Not info: the org's documents are still searchable.
                    error!(
                        org_id = %org_id,
                        rows,
                        "quickwit-adapter purged organization bookkeeping WITHOUT pruning the \
                         index; the org's documents remain searchable"
                    );
                }
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

    warn!("quickwit-adapter GDPR erasure consumer message stream ended");
    Ok(())
}

/// Connect to the shared cross-plane broker (`control-shared-nats`) under
/// the narrowly-scoped `quickwit-adapter-gdpr` identity used only by the
/// GDPR erasure consumer. Username/password only, no token fallback: a
/// missing/misconfigured shared identity fails the connection attempt (and
/// is retried by the caller's supervised background loop) rather than
/// silently degrading to an unauthenticated session on a broker this crate
/// does not own.
async fn connect_shared(url: &str) -> Result<async_nats::Client, async_nats::ConnectError> {
    let user = std::env::var("NATS_SHARED_USER").unwrap_or_default();
    let password = std::env::var("NATS_SHARED_PASSWORD").unwrap_or_default();
    async_nats::ConnectOptions::with_user_and_password(user, password)
        .custom_inbox_prefix(SHARED_INBOX_PREFIX)
        .connect(url)
        .await
}

enum Outcome {
    Purged {
        org_id: String,
        rows: u64,
        /// Carried so the completion log records whether the searchable copy was
        /// pruned, not just how many bookkeeping rows went. Without it an
        /// operator auditing an erasure cannot tell from logs whether the org's
        /// documents were actually removed from the index.
        index_pruned: bool,
    },
    Skipped,
}

async fn handle_message(
    pool: &PgPool,
    quickwit: &crate::quickwit::QuickwitClient,
    payload: &[u8],
) -> anyhow::Result<Outcome> {
    let Some(erasure) = parse_erasure_event(payload).map_err(|e| anyhow::anyhow!(e.to_string()))?
    else {
        return Ok(Outcome::Skipped);
    };

    let summary = purge_organization_data(pool, &erasure.org_id, Some(quickwit)).await?;
    Ok(Outcome::Purged {
        org_id: erasure.org_id,
        rows: summary.total(),
        index_pruned: summary.index_delete_task_submitted,
    })
}
