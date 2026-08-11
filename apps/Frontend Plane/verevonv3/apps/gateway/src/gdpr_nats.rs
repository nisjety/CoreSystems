//! `JetStream` pull consumer for the cross-plane GDPR erasure fan-out
//! (`verevon.gdpr.erasure.requested`), purging the Frontend Plane's own copy of
//! conversation content.
//!
//! # Why the Frontend Plane needs one at all
//!
//! The BFF keeps a per-(org, user) chat-history index and per-thread
//! transcripts in Dragonfly (see [`crate::domains::chat::history`]). That is a
//! complete second copy of conversations the Model Plane owns — and it sat
//! outside every erasure path: session-core deleted its messages, events and
//! threads and reported success while the same turns kept living here for the
//! rest of their 90-day TTL, readable through the transcript endpoint. The
//! self-service `DELETE /api/v1/privacy/erase` route purges its own caller
//! synchronously, but an ADMIN or org-scoped erasure never touches this
//! process. This consumer closes that: the obligation is to delete the data,
//! not to make it harder to reach.
//!
//! # Shape
//!
//! Mirrors Data Plane v2's `graph-index-rs`/`retrieval-engine-rs` and Model
//! Plane `session-core` consumers on the same subject — deliberately, so there
//! is one pattern to audit rather than five:
//!
//!   * Binds a DEPLOYMENT-PROVISIONED durable pull consumer via
//!     [`async_nats::jetstream::Context::get_consumer_from_stream`], never
//!     `get_stream(..).get_consumer(..)`. The latter issues a `STREAM.INFO`
//!     request first, so it would additionally require
//!     `$JS.API.STREAM.INFO.AQENCIA_CONTROLPLANE` — the exact permission gap
//!     that silently broke session-core's consumer once already. Runtime
//!     credentials here can bind, pull and ACK; they cannot create or mutate
//!     `JetStream` topology.
//!   * Connects to `control-shared-nats` (Control Plane), NOT the Frontend
//!     Plane's own `verevon-nats`. Both sit on `inter-plane-bus`, but only
//!     `control-shared-nats` hosts the `AQENCIA_CONTROLPLANE` stream that
//!     org-core and user-core publish erasure into. Pointing this at
//!     `verevon-nats` would connect, bind nothing, and retry forever — a
//!     consumer that looks healthy and receives nothing.
//!   * Under its OWN narrowly-scoped identity (`verevon-gateway-gdpr`) with its
//!     own env var trio and inbox prefix. Never reuses another service's
//!     shared-broker identity, and never the gateway's own runtime NATS
//!     connection.
//!
//! PROVISIONING: the durable consumer ([`DURABLE_NAME`] on [`STREAM_NAME`],
//! filtering [`SUBJECT`]) is created by audit-core's
//! `internal/provisioner/provisioner.go` (`orgErasureConsumerConfigs`), and the
//! identity's permission block lives in `apps/Control Plane/control-shared-nats.conf`.
//! If either is missing, [`run_once`] fails at `get_consumer_from_stream` and
//! [`run_supervised`] retries forever without making progress — which is why
//! both are part of this change rather than a follow-up.

use std::time::Duration;

use async_nats::jetstream::{self, consumer::PullConsumer, AckKind};
use futures_util::StreamExt;
use serde::Deserialize;

use crate::config::AppState;
use crate::domains::chat::history::{purge_org_history, purge_user_history, PurgeScope};

pub(crate) const STREAM_NAME: &str = "AQENCIA_CONTROLPLANE";
pub(crate) const SUBJECT: &str = "verevon.gdpr.erasure.requested";
pub(crate) const DURABLE_NAME: &str = "verevon-gateway-gdpr-erasure-v1";

const INBOX_PREFIX: &str = "_INBOX.VEREVON_GATEWAY_GDPR";

/// Backoff between failed connect/bind attempts, so a misconfigured or briefly
/// unavailable shared broker does not hot-loop.
const RECONNECT_BACKOFF: Duration = Duration::from_secs(5);

/// Env var trio naming this consumer's dedicated shared-broker credential.
const URL_ENV: &str = "VEREVON_GATEWAY_GDPR_NATS_URL";
const USER_ENV: &str = "VEREVON_GATEWAY_GDPR_NATS_USER";
const PASSWORD_ENV: &str = "VEREVON_GATEWAY_GDPR_NATS_PASSWORD";

/// Spawn the consumer if it is configured.
///
/// Returns `false` when the credential trio is absent, which is the normal
/// state for a local dev stack with no Control Plane running. It logs at WARN
/// rather than failing startup: an unreachable erasure bus must not stop the
/// gateway from serving traffic, and the ZDR write-gate plus the synchronous
/// self-service purge both keep working without it. It is a WARN and not a
/// DEBUG because a production deployment silently missing this is a compliance
/// gap, and the log line is the only place that shows.
pub(crate) fn spawn(state: AppState) -> bool {
    let (Ok(url), Ok(user), Ok(password)) = (
        std::env::var(URL_ENV),
        std::env::var(USER_ENV),
        std::env::var(PASSWORD_ENV),
    ) else {
        tracing::warn!(
            "GDPR erasure consumer not configured ({URL_ENV}/{USER_ENV}/{PASSWORD_ENV}); \
             org-scoped erasure will NOT purge the gateway's chat-history copy"
        );
        return false;
    };
    if url.trim().is_empty() || user.trim().is_empty() || password.trim().is_empty() {
        tracing::warn!("GDPR erasure consumer credentials are blank; consumer not started");
        return false;
    }
    tokio::spawn(run_supervised(state, url, user, password));
    true
}

/// Run until the process exits, reconnecting with a fixed backoff on any
/// connect/bind failure or stream end. Spawned as an independent task so a
/// shared-broker outage never takes down HTTP serving.
async fn run_supervised(state: AppState, url: String, user: String, password: String) {
    loop {
        if let Err(error) = run_once(&state, &url, &user, &password).await {
            tracing::error!(%error, "gateway GDPR erasure consumer stopped; retrying");
        }
        tokio::time::sleep(RECONNECT_BACKOFF).await;
    }
}

async fn run_once(state: &AppState, url: &str, user: &str, password: &str) -> anyhow::Result<()> {
    tracing::info!(%url, subject = SUBJECT, "gateway GDPR erasure consumer connecting");
    let client =
        async_nats::ConnectOptions::with_user_and_password(user.to_owned(), password.to_owned())
            .custom_inbox_prefix(INBOX_PREFIX)
            .connect(url)
            .await?;
    let js = jetstream::new(client);

    let consumer: PullConsumer = js
        .get_consumer_from_stream(DURABLE_NAME, STREAM_NAME)
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    if consumer.cached_info().config.filter_subject != SUBJECT {
        anyhow::bail!("pre-provisioned GDPR erasure consumer filter mismatch");
    }

    let mut messages = consumer.messages().await?;
    tracing::info!("gateway GDPR erasure consumer ready");

    while let Some(message) = messages.next().await {
        let message = match message {
            Ok(message) => message,
            Err(error) => {
                tracing::warn!(%error, "GDPR erasure consumer receive error");
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        };

        match handle_message(state, &message.payload).await {
            Outcome::Purged {
                scope,
                users,
                threads,
            } => {
                tracing::info!(
                    scope,
                    users,
                    threads,
                    "erasure: purged the gateway's chat-history copy"
                );
                if let Err(error) = message.ack().await {
                    tracing::warn!(%error, "ack failed after purge");
                }
            }
            // Both a well-formed event we do not own and an unparsable one are
            // ACKed, never NAKed. Redelivery cannot turn malformed JSON into
            // valid JSON, and NAK-ing poison would redeliver it until MaxDeliver
            // and bury a real event behind it — the same immediate-ack-on-poison
            // rule the sibling consumers on this subject follow.
            Outcome::Skipped => {
                if let Err(error) = message.ack().await {
                    tracing::warn!(%error, "ack failed after skip");
                }
            }
            Outcome::Poison { reason } => {
                tracing::error!(reason, "unprocessable GDPR erasure event; acking");
                if let Err(error) = message.ack().await {
                    tracing::warn!(%error, "ack failed after poison");
                }
            }
            // A purge that FAILED is different from one that was skipped: the
            // data is still there, so NAK and let JetStream redeliver.
            Outcome::Retry { reason } => {
                tracing::error!(reason, "GDPR erasure purge failed; naking for redelivery");
                if let Err(error) = message.ack_with(AckKind::Nak(None)).await {
                    tracing::warn!(%error, "nak failed");
                }
            }
        }
    }

    tracing::warn!("gateway GDPR erasure consumer message stream ended");
    Ok(())
}

enum Outcome {
    Purged {
        scope: &'static str,
        users: usize,
        threads: usize,
    },
    Skipped,
    Poison {
        reason: String,
    },
    #[allow(dead_code)] // Reserved for a purge path that can report failure.
    Retry {
        reason: String,
    },
}

/// The wire shape shared by org-core's and user-core's publishers.
///
/// Decoded LENIENTLY — unknown fields ignored — because one subject multiplexes
/// both producers. org-core publishes
/// `{subject_type:"organization", subject_id, org_id, requested_by, ts}`;
/// user-core publishes a richer per-org child event
/// `{event_id, operation_id, subject_type:"user"|"user_anonymize", subject_id,
/// org_id, requested_by, mode, ts}`. Fields this struct does not name are
/// expected multiplexing, not malformed input.
#[derive(Debug, Deserialize)]
struct ErasureEvent {
    #[serde(default)]
    subject_type: String,
    #[serde(default)]
    subject_id: String,
    #[serde(default)]
    org_id: String,
}

/// What this consumer should do with one event.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    /// Purge one subject's history in one org.
    PurgeUser { org_id: String, user_id: String },
    /// Purge every user's history in one org.
    PurgeOrg { org_id: String },
    /// A well-formed event this consumer does not own.
    Skip,
}

/// Classify an erasure event.
///
/// `user_anonymize` is treated exactly like `user`: anonymisation removes the
/// link between a person and their data, and a retained transcript is the
/// person's own words — leaving it behind under a scrubbed id would defeat the
/// operation entirely. There is nothing here to pseudonymise in place, so the
/// only faithful action is deletion.
fn classify(payload: &[u8]) -> Result<Action, String> {
    let event: ErasureEvent = serde_json::from_slice(payload)
        .map_err(|error| format!("decode erasure event: {error}"))?;
    let subject_type = event.subject_type.trim();
    let subject_id = event.subject_id.trim();
    let org_id = event.org_id.trim();

    match subject_type {
        "user" | "user_anonymize" => {
            // NO subject_id fallback for org_id here. The sibling consumers'
            // `if org_id == "" { org_id = subject_id }` is sound for THEIR case
            // — they only ever act on organization events, where org-core stamps
            // both fields from the same orgID. On a USER event `subject_id` is a
            // user id, so the same fallback would scope the purge to an "org"
            // that is really a person, and a user erasure missing its org would
            // look like a successful no-op instead of the poison it is.
            if subject_id.is_empty() || org_id.is_empty() {
                return Err("user erasure event requires subject_id and org_id".to_owned());
            }
            if subject_id.len() > 255 || org_id.len() > 255 {
                return Err("user erasure event scope is oversized".to_owned());
            }
            Ok(Action::PurgeUser {
                org_id: org_id.to_owned(),
                user_id: subject_id.to_owned(),
            })
        }
        "organization" => {
            // Here the fallback IS correct: org-core's PublishGDPRErasureFanout
            // stamps org_id and subject_id from the same orgID, so either
            // carries the scope. Matches every sibling consumer on this subject.
            let org_id = if org_id.is_empty() {
                subject_id
            } else {
                org_id
            };
            if org_id.is_empty() || org_id.len() > 255 {
                return Err("organization erasure event requires a bounded org_id".to_owned());
            }
            Ok(Action::PurgeOrg {
                org_id: org_id.to_owned(),
            })
        }
        // Any other subject_type is a deliberate no-op, never an error.
        _ => Ok(Action::Skip),
    }
}

async fn handle_message(state: &AppState, payload: &[u8]) -> Outcome {
    match classify(payload) {
        Ok(Action::PurgeUser { org_id, user_id }) => {
            let threads =
                purge_user_history(state, &org_id, &user_id, PurgeScope::Everything).await;
            Outcome::Purged {
                scope: "user",
                users: 1,
                threads,
            }
        }
        Ok(Action::PurgeOrg { org_id }) => {
            let (users, threads) = purge_org_history(state, &org_id).await;
            Outcome::Purged {
                scope: "organization",
                users,
                threads,
            }
        }
        Ok(Action::Skip) => Outcome::Skipped,
        Err(reason) => Outcome::Poison { reason },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// user-core's actual per-org child payload, verbatim from
    /// `gdpr_erasure_saga.go`'s `erasureFanoutPayload`.
    #[test]
    fn a_user_erasure_purges_that_user_in_that_org() {
        let payload = br#"{"event_id":"e1","operation_id":"op1","subject_type":"user",
            "subject_id":"user-9","org_id":"org-3","requested_by":"admin","mode":"hard",
            "ts":"2026-08-10T10:00:00Z"}"#;
        assert_eq!(
            classify(payload).unwrap(),
            Action::PurgeUser {
                org_id: "org-3".to_owned(),
                user_id: "user-9".to_owned()
            }
        );
    }

    /// Anonymisation severs the person↔data link. A retained transcript IS the
    /// person's own words, so it must go, not be re-keyed.
    #[test]
    fn anonymize_is_treated_as_a_full_purge_not_a_skip() {
        let payload =
            br#"{"subject_type":"user_anonymize","subject_id":"user-9","org_id":"org-3"}"#;
        assert_eq!(
            classify(payload).unwrap(),
            Action::PurgeUser {
                org_id: "org-3".to_owned(),
                user_id: "user-9".to_owned()
            }
        );
    }

    /// org-core's payload, verbatim from `PublishGDPRErasureFanout`.
    #[test]
    fn an_organization_erasure_purges_the_whole_org() {
        let payload = br#"{"subject_type":"organization","subject_id":"org-3","org_id":"org-3",
            "requested_by":"admin","ts":"2026-08-10T10:00:00Z"}"#;
        assert_eq!(
            classify(payload).unwrap(),
            Action::PurgeOrg {
                org_id: "org-3".to_owned()
            }
        );
    }

    /// On an ORGANIZATION event org-core stamps org_id and subject_id from the
    /// same orgID, so falling back to subject_id is safe — like every sibling
    /// consumer on this subject.
    #[test]
    fn org_id_falls_back_to_subject_id_for_organization_events_only() {
        let payload = br#"{"subject_type":"organization","subject_id":"org-7"}"#;
        assert_eq!(
            classify(payload).unwrap(),
            Action::PurgeOrg {
                org_id: "org-7".to_owned()
            }
        );
        // But NOT on a user event: there `subject_id` is a USER id, so the same
        // fallback would scope a purge to an "org" that is really a person, and
        // would turn a malformed event into a silent no-op instead of poison.
        assert!(classify(br#"{"subject_type":"user","subject_id":"user-7"}"#).is_err());
    }

    /// One subject carries several producers. An unowned subject_type is a
    /// deliberate no-op — never an error, and never a purge.
    #[test]
    fn an_unowned_subject_type_is_skipped_not_purged() {
        for payload in [
            &br#"{"subject_type":"team","subject_id":"team-1","org_id":"org-3"}"#[..],
            &br#"{"subject_type":"","subject_id":"x","org_id":"org-3"}"#[..],
            &br#"{"org_id":"org-3"}"#[..],
        ] {
            assert_eq!(classify(payload).unwrap(), Action::Skip);
        }
    }

    /// Poison is distinguishable from skip so the caller can ACK it rather than
    /// NAK it forever: no redelivery turns malformed input into valid input.
    #[test]
    fn malformed_or_unscoped_events_are_poison() {
        assert!(classify(b"not json").is_err());
        // Claims to be an org erasure but carries no scope.
        assert!(classify(br#"{"subject_type":"organization"}"#).is_err());
        // Claims a user erasure with no org to scope it to.
        assert!(classify(br#"{"subject_type":"user","subject_id":"u1"}"#).is_err());
        // Oversized scope.
        let huge = "a".repeat(256);
        let payload =
            format!(r#"{{"subject_type":"organization","org_id":"{huge}"}}"#).into_bytes();
        assert!(classify(&payload).is_err());
    }

    /// A user erasure must never be widened into an org-wide purge by a missing
    /// field — that would delete every colleague's history too.
    #[test]
    fn a_user_erasure_never_widens_into_an_org_purge() {
        let payload = br#"{"subject_type":"user","subject_id":"user-9","org_id":"org-3"}"#;
        match classify(payload).unwrap() {
            Action::PurgeUser { org_id, user_id } => {
                assert_eq!(org_id, "org-3");
                assert_eq!(user_id, "user-9");
            }
            other => panic!("a user erasure must stay user-scoped, got {other:?}"),
        }
    }
}
