//! The connected accounts a Do run was scoped to, recorded on the run record (WS23-S4).
//!
//! `RunAgentRequest.connector_scope` carries the connections the person selected for the
//! turn. It gates every connector read of the run, but until now it was never kept: nothing on
//! the run said which accounts the run could read. [`record`] stamps the selected ids on the
//! run record through `RecordRunContext`, best-effort, as
//! `runs.metadata.run_context.connector_scope`.
//!
//! * Off unless `CONNECTOR_SCOPE_RECORD_ENABLED` is the literal `true`: off, nothing is
//!   recorded and the run is byte for byte what it was.
//! * Ids only. No token, account name or connector content is on the stamp, and the log
//!   line carries the run id alone.
//! * A zero-retention or waived run is never recorded, and a run with no selection records
//!   nothing: the ids reveal which accounts a person used, and session-core refuses the write
//!   for such a credential anyway.
//! * A lost stamp costs the durable record of the scope, never the run, and is counted.
//!   The ids go with the run, the thread and the organisation through the existing erasure
//!   cascade (they live in the run's own metadata).

use std::sync::OnceLock;
use std::time::Duration;

use mp_contracts::model_plane::v1::{self as pb, session_core_client::SessionCoreClient};
use tonic::transport::Channel;

/// Environment switch for recording the connector scope on the run record.
pub const SWITCH_ENV: &str = "CONNECTOR_SCOPE_RECORD_ENABLED";

/// Counter bumped when the scope could not be stamped on the run record.
const RECORD_FAILED_METRIC: &str = "connector_scope_record_failed_total";

/// Bound on the best-effort run-record stamp, so a slow session-core cannot hold a run's start.
const RECORD_TIMEOUT: Duration = Duration::from_secs(2);

/// Only the literal `true` enables the switch, as every switch of this service reads it.
#[must_use]
pub fn switch_on(raw: Option<&str>) -> bool {
    raw == Some("true")
}

/// Whether the deployment records the connector scope, read once per process so a run cannot
/// see it change.
#[must_use]
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| switch_on(std::env::var(SWITCH_ENV).ok().as_deref()))
}

/// Whether a run's scope is recorded: the switch is on, the run selected at least one
/// connection, and the run keeps state (not zero-retention, not waived).
#[must_use]
pub fn applies(on: bool, zdr: bool, waived: bool, connector_ids: &[String]) -> bool {
    on && !zdr && !waived && connector_ids.iter().any(|id| !id.trim().is_empty())
}

/// Stamp the connections a Do run was scoped to on the run record, best-effort. The status
/// holds the selected ids and nothing else, so session-core merges it beside the parts and pins
/// already recorded.
///
/// The caller has already checked [`applies`]; this checks nothing about retention.
pub async fn record(
    session_channel: &Channel,
    bearer: &str,
    org_id: &str,
    run_id: &str,
    connector_ids: &[String],
) {
    let request = crate::runtime_loop::authenticated_session_request(
        pb::RecordRunContextRequest {
            run_id: run_id.to_owned(),
            org_id: org_id.to_owned(),
            status: Some(pb::RunContextStatus {
                connector_ids: connector_ids.to_vec(),
                ..Default::default()
            }),
        },
        bearer,
    );
    let Ok(request) = request else {
        tracing::warn!(%run_id, "connector scope: session credential is not forwardable; not recorded");
        metrics::counter!(RECORD_FAILED_METRIC).increment(1);
        return;
    };
    let mut client = SessionCoreClient::new(session_channel.clone());
    let call = client.record_run_context(request);
    match tokio::time::timeout(RECORD_TIMEOUT, call).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            tracing::warn!(code = ?error.code(), %run_id, "connector scope: not recorded (best-effort)");
            metrics::counter!(RECORD_FAILED_METRIC).increment(1);
        }
        Err(_elapsed) => {
            tracing::warn!(%run_id, "connector scope: recording timed out (best-effort)");
            metrics::counter!(RECORD_FAILED_METRIC).increment(1);
        }
    }
}
