//! NATS bridge: consumes `mp.v1.orchestration.>` subjects and broadcasts the
//! decoded `OrchestrationEvent` payloads to in-process gRPC subscribers.
//!
//! Wire format: each NATS message body is JSON-encoded
//! `mp_orchestration::OrchestrationEvent` (tagged enum with `kind` field).
//! It is converted to the proto envelope via the existing
//! `impl From<OrchestrationEvent> for ProtoOrchestrationEvent` shim.
//!
//! Subjects: see `mp_events::subjects::ORCHESTRATION_*` constants.
//! Stream:   `MP_ORCHESTRATION_EVENTS` (idempotent get-or-create on startup).
//! Consumer: ephemeral pull consumer, fan-out only — gRPC subscribers re-read
//!           durable state from Postgres if they need replay.

use std::time::Duration;

use async_nats::jetstream::{self, consumer::PullConsumer};
use futures::StreamExt;
use mp_contracts::model_plane::v1 as proto;
use mp_events::subjects::ORCHESTRATION_WILDCARD;
use mp_orchestration::OrchestrationEvent;
use tokio::sync::broadcast;
use tracing::{error, info, warn};

const STREAM_NAME: &str = "MP_ORCHESTRATION_EVENTS";

/// Run the NATS -> broadcast bridge until the connection is lost.
///
/// # Errors
///
/// Returns an error if the initial NATS connection or stream creation fails.
pub async fn run(
    nats_url: String,
    events_tx: broadcast::Sender<proto::OrchestrationEvent>,
) -> anyhow::Result<()> {
    info!(%nats_url, subject = ORCHESTRATION_WILDCARD, "orchestration NATS bridge connecting");

    let client = crate::nats_connection::connect(&nats_url).await?;
    let js = jetstream::new(client);

    let stream = js.get_stream(STREAM_NAME).await?;
    let consumer: PullConsumer = stream
        .get_consumer("session-core-orchestration")
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    if consumer.cached_info().config.filter_subject != ORCHESTRATION_WILDCARD {
        return Err(anyhow::anyhow!(
            "pre-provisioned orchestration consumer filter mismatch"
        ));
    }

    let mut messages = consumer.messages().await?;
    info!("orchestration NATS bridge ready");

    while let Some(msg) = messages.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                warn!(error = %e, "orchestration NATS receive error");
                metrics::counter!(
                    "mp_session_orchestration_bridge_total",
                    "result" => "recv_error",
                )
                .increment(1);
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        };

        match handle(&msg.payload, &events_tx) {
            Ok(()) => {
                metrics::counter!(
                    "mp_session_orchestration_bridge_total",
                    "result" => "ok",
                )
                .increment(1);
            }
            Err(BridgeError::Decode(e)) => {
                warn!(error = %e, subject = %msg.subject, "orchestration event decode failed");
                metrics::counter!(
                    "mp_session_orchestration_bridge_total",
                    "result" => "decode_error",
                )
                .increment(1);
            }
            Err(BridgeError::NoSubscribers) => {
                metrics::counter!(
                    "mp_session_orchestration_bridge_total",
                    "result" => "no_subscribers",
                )
                .increment(1);
            }
        }
    }

    error!("orchestration NATS message stream ended");
    Ok(())
}

#[derive(Debug, thiserror::Error)]
enum BridgeError {
    #[error("decode error: {0}")]
    Decode(String),
    #[error("no subscribers")]
    NoSubscribers,
}

fn handle(
    payload: &[u8],
    events_tx: &broadcast::Sender<proto::OrchestrationEvent>,
) -> Result<(), BridgeError> {
    let domain: OrchestrationEvent =
        serde_json::from_slice(payload).map_err(|e| BridgeError::Decode(e.to_string()))?;
    let pb: proto::OrchestrationEvent = domain.into();
    events_tx
        .send(pb)
        .map(|_| ())
        .map_err(|_| BridgeError::NoSubscribers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use mp_contracts::model_plane::v1::orchestration_event;
    use mp_orchestration::{ApprovalKind, ApprovalState, PlanState, SubagentRole, TodoState};

    fn fixed_at() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).expect("valid epoch")
    }

    #[tokio::test]
    async fn handle_plan_transitioned_round_trip() {
        let (tx, mut rx) = broadcast::channel(8);
        let body = serde_json::to_vec(&OrchestrationEvent::PlanTransitioned {
            plan_id: "plan_1".into(),
            run_id: "run_1".into(),
            from: PlanState::Draft,
            to: PlanState::Proposed,
            at: fixed_at(),
        })
        .unwrap();

        handle(&body, &tx).expect("forwarded");

        let ev = rx.recv().await.expect("event received");
        match ev.event.as_ref().unwrap() {
            orchestration_event::Event::PlanTransitioned(p) => {
                assert_eq!(p.plan_id, "plan_1");
                assert_eq!(p.run_id, "run_1");
                assert_eq!(p.from, proto::PlanState::Draft as i32);
                assert_eq!(p.to, proto::PlanState::Proposed as i32);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[tokio::test]
    async fn handle_todo_transitioned_round_trip() {
        let (tx, mut rx) = broadcast::channel(8);
        let body = serde_json::to_vec(&OrchestrationEvent::TodoTransitioned {
            todo_id: "todo_1".into(),
            thread_id: "thread_1".into(),
            from: TodoState::Pending,
            to: TodoState::InProgress,
            at: fixed_at(),
        })
        .unwrap();

        handle(&body, &tx).expect("forwarded");
        let ev = rx.recv().await.expect("event received");
        match ev.event.as_ref().unwrap() {
            orchestration_event::Event::TodoTransitioned(p) => {
                assert_eq!(p.todo_id, "todo_1");
                assert_eq!(p.thread_id, "thread_1");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[tokio::test]
    async fn handle_approval_state_changed_round_trip() {
        let (tx, mut rx) = broadcast::channel(8);
        let body = serde_json::to_vec(&OrchestrationEvent::ApprovalStateChanged {
            approval_id: "appr_1".into(),
            run_id: "run_1".into(),
            approval_kind: ApprovalKind::ToolCall,
            to: ApprovalState::Granted,
            decided_by: "alice".into(),
            at: fixed_at(),
        })
        .unwrap();

        handle(&body, &tx).expect("forwarded");
        let ev = rx.recv().await.expect("event received");
        match ev.event.as_ref().unwrap() {
            orchestration_event::Event::ApprovalStateChanged(p) => {
                assert_eq!(p.approval_id, "appr_1");
                assert_eq!(p.decided_by, "alice");
                assert_eq!(p.to, proto::ApprovalState::Granted as i32);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[tokio::test]
    async fn handle_subagent_attached_round_trip() {
        let (tx, mut rx) = broadcast::channel(8);
        let body = serde_json::to_vec(&OrchestrationEvent::SubagentAttached {
            parent_run_id: "run_p".into(),
            child_run_id: "run_c".into(),
            role: SubagentRole::Coder,
            at: fixed_at(),
        })
        .unwrap();

        handle(&body, &tx).expect("forwarded");
        let ev = rx.recv().await.expect("event received");
        match ev.event.as_ref().unwrap() {
            orchestration_event::Event::SubagentAttached(p) => {
                assert_eq!(p.parent_run_id, "run_p");
                assert_eq!(p.child_run_id, "run_c");
                assert_eq!(p.role, proto::SubagentRole::Coder as i32);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn handle_invalid_json_returns_decode_error() {
        let (tx, _rx) = broadcast::channel(8);
        let err = handle(b"not-json", &tx).unwrap_err();
        assert!(matches!(err, BridgeError::Decode(_)));
    }

    #[test]
    fn handle_no_subscribers_returns_no_subscribers_error() {
        let (tx, rx) = broadcast::channel(8);
        drop(rx);
        let body = serde_json::to_vec(&OrchestrationEvent::SubagentStopped {
            child_run_id: "run_c".into(),
            status: "completed".into(),
            at: fixed_at(),
        })
        .unwrap();
        let err = handle(&body, &tx).unwrap_err();
        assert!(matches!(err, BridgeError::NoSubscribers));
    }
}
