//! gRPC handler for `RunLedgerService` (EP-14): what a run did and what its model calls were sent.
//!
//! Today it serves the request snapshot write (S6). The store, its checks and its sweep live in
//! [`crate::request_snapshots`]; this module authorizes the caller and applies the switch. The run timing
//! read (S5) and the request inspector read (S7) join it as their own handlers.

// tonic::Status is the unavoidable large Err for gRPC; boxing breaks the service-trait contract.
#![allow(clippy::result_large_err)]

use std::time::Instant;

use mp_contracts::model_plane::v1::{
    self as pb,
    run_ledger_service_server::{RunLedgerService, RunLedgerServiceServer},
};
use sqlx::PgPool;
use tonic::{Request, Response, Status};

fn record_metrics(method: &'static str, started: Instant, result: &Result<impl Sized, Status>) {
    let status = match result {
        Ok(_) => "ok",
        Err(error) if error.code() == tonic::Code::FailedPrecondition => "refused",
        Err(_) => "error",
    };
    metrics::counter!(
        "mp_session_run_ledger_grpc_requests_total",
        "method" => method,
        "status" => status,
    )
    .increment(1);
    metrics::histogram!(
        "mp_session_run_ledger_grpc_request_duration_seconds",
        "method" => method,
    )
    .record(started.elapsed().as_secs_f64());
}

pub struct RunLedgerGrpc {
    pool: PgPool,
    /// `SESSION_CORE_REQUEST_SNAPSHOTS_ENABLED`, read once at boot.
    snapshots_enabled: bool,
    /// `SESSION_CORE_REQUEST_SNAPSHOT_TTL_DAYS`, read once at boot and clamped.
    ttl_days: i32,
}

impl RunLedgerGrpc {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            snapshots_enabled: crate::request_snapshots::enabled(),
            ttl_days: crate::request_snapshots::ttl_days(),
        }
    }

    /// Convenience for `grpc.rs`, which registers it with the auth interceptor.
    #[must_use]
    #[allow(dead_code)] // direct constructor retained for isolated service tests
    pub fn into_server(self) -> RunLedgerServiceServer<Self> {
        RunLedgerServiceServer::new(self)
    }
}

/// With the switch off the write refuses, so a deployment that never turns it on stores nothing.
fn require_snapshots(enabled: bool) -> Result<(), Status> {
    if enabled {
        return Ok(());
    }
    Err(Status::failed_precondition(
        "request snapshots are not enabled (SESSION_CORE_REQUEST_SNAPSHOTS_ENABLED)",
    ))
}

#[tonic::async_trait]
impl RunLedgerService for RunLedgerGrpc {
    async fn record_request_snapshot(
        &self,
        request: Request<pb::RecordRequestSnapshotRequest>,
    ) -> Result<Response<pb::RecordRequestSnapshotResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = crate::auth::identity(&request)?;
            crate::request_snapshots::authorize_writer(&caller)?;
            require_snapshots(self.snapshots_enabled)?;
            let req = request.into_inner();
            crate::request_snapshots::record(&self.pool, caller.org_id(), self.ttl_days, &req)
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("record_request_snapshot", started, &result);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_off_refuses_the_write() {
        let error = require_snapshots(false).unwrap_err();
        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
        assert!(require_snapshots(true).is_ok());
    }
}
