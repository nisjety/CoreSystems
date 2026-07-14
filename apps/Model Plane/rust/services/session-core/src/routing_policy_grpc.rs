//! gRPC handler for the `RoutingPolicy` service.
//!
//! Owns the durable store for the Velion intent layer's runtime policy (the
//! "model router"). The intent layer in inference-core resolves the
//! `velion-budget` / `velion-balance` / `velion-genius` modes to a concrete
//! model from request complexity + budget posture; this service makes that
//! policy editable at runtime instead of via compile-time constants.
//!
//! Storage is a JSONB singleton row (`routing_policy` table, `id = 1`). The
//! full policy travels as an opaque JSON string in `config_json` — the
//! canonical schema is owned by inference-core's `RoutingPolicy` struct.
//! session-core only validates the string parses as JSON before storing it;
//! it never interprets the contents. The BFF reads/writes through `SetPolicy`
//! and inference-core polls `GetPolicy`.

// tonic::Status is the unavoidable large Err for gRPC; boxing breaks the service-trait contract.
#![allow(clippy::result_large_err)]

use mp_contracts::model_plane::v1::{
    self as pb,
    routing_policy_server::{RoutingPolicy, RoutingPolicyServer},
};
use sqlx::PgPool;
use std::time::Instant;
use tonic::{Request, Response, Status};

fn record_metrics(method: &'static str, started: Instant, is_ok: bool) {
    let status = if is_ok { "ok" } else { "error" };
    metrics::counter!(
        "mp_session_routing_policy_grpc_requests_total",
        "method" => method,
        "status" => status,
    )
    .increment(1);
    metrics::histogram!(
        "mp_session_routing_policy_grpc_request_duration_seconds",
        "method" => method,
    )
    .record(started.elapsed().as_secs_f64());
}

/// Row shape returned by the singleton read/write queries. `updated_at` is
/// projected as Unix seconds (`extract(epoch ...)::bigint`) so it maps straight
/// onto the proto's `int64 updated_at` without timezone gymnastics.
#[derive(sqlx::FromRow)]
struct RoutingPolicyRow {
    config: serde_json::Value,
    version: i64,
    updated_by: String,
    updated_at: i64,
}

fn row_to_pb(row: RoutingPolicyRow) -> pb::RoutingPolicyMessage {
    pb::RoutingPolicyMessage {
        config_json: row.config.to_string(),
        version: row.version,
        updated_by: row.updated_by,
        updated_at: row.updated_at,
    }
}

/// Validate the wire `config_json` parses as JSON before it is stored as JSONB.
/// Mirrors `finetune_grpc::parse_hyperparameters`: an empty string is treated as
/// the empty object, and any malformed JSON is rejected with `invalid_argument`.
fn parse_config(raw: &str) -> Result<serde_json::Value, Status> {
    if raw.is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_str(raw)
        .map_err(|e| Status::invalid_argument(format!("config_json must be valid JSON: {e}")))
}

pub struct RoutingPolicyService {
    pool: PgPool,
}

impl RoutingPolicyService {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Convenience for `grpc.rs` so the wiring there mirrors how
    /// `FinetuneJobsService` and `OrchestrationGrpc` register their tonic
    /// servers.
    #[must_use]
    #[allow(dead_code)] // direct constructor retained for isolated service tests
    pub fn into_server(self) -> RoutingPolicyServer<Self> {
        RoutingPolicyServer::new(self)
    }
}

#[tonic::async_trait]
impl RoutingPolicy for RoutingPolicyService {
    async fn get_policy(
        &self,
        request: Request<pb::GetRoutingPolicyRequest>,
    ) -> Result<Response<pb::RoutingPolicyMessage>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::RoutingPolicyMessage>, Status> = async {
            // Read is platform-global (no tenant data), but still requires a
            // verified identity — never an anonymous fetch.
            let _caller = crate::auth::identity(&request)?;
            let row: RoutingPolicyRow = sqlx::query_as(
                "SELECT
                    config,
                    version,
                    updated_by,
                    extract(epoch from updated_at)::bigint AS updated_at
                 FROM routing_policy
                 WHERE id = 1",
            )
            .fetch_one(&self.pool)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

            Ok(Response::new(row_to_pb(row)))
        }
        .await;
        record_metrics("get_policy", started, result.is_ok());
        result
    }

    async fn set_policy(
        &self,
        request: Request<pb::SetRoutingPolicyRequest>,
    ) -> Result<Response<pb::RoutingPolicyMessage>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::RoutingPolicyMessage>, Status> = async {
            // The routing policy is platform-global: only an admin-scoped user
            // or a service principal may overwrite it. A regular member token
            // must not be able to change every org's model routing.
            let caller = crate::auth::identity(&request)?;
            if !caller.is_service() && !caller.has_scope("admin") {
                return Err(Status::permission_denied(
                    "changing the routing policy requires the admin scope",
                ));
            }
            let req = request.into_inner();
            let config = parse_config(&req.config_json)?;

            let row: RoutingPolicyRow = sqlx::query_as(
                "UPDATE routing_policy SET
                    config = $1::jsonb,
                    version = version + 1,
                    updated_by = $2,
                    updated_at = now()
                 WHERE id = 1
                 RETURNING
                    config,
                    version,
                    updated_by,
                    extract(epoch from updated_at)::bigint AS updated_at",
            )
            .bind(config)
            .bind(&req.updated_by)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

            Ok(Response::new(row_to_pb(row)))
        }
        .await;
        record_metrics("set_policy", started, result.is_ok());
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_config_empty_returns_empty_object() {
        let v = parse_config("").unwrap();
        assert_eq!(v, serde_json::json!({}));
    }

    #[test]
    fn parse_config_passes_through_valid_json() {
        let v = parse_config(r#"{"mode":"velion-balance"}"#).unwrap();
        assert_eq!(v, serde_json::json!({"mode": "velion-balance"}));
    }

    #[test]
    fn parse_config_rejects_invalid_json() {
        let err = parse_config("not json").unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("config_json must be valid JSON"));
    }

    #[test]
    fn row_to_pb_serializes_config_and_copies_scalars() {
        let row = RoutingPolicyRow {
            config: serde_json::json!({"mode": "velion-genius"}),
            version: 7,
            updated_by: "user-123".to_owned(),
            updated_at: 1_700_000_000,
        };
        let msg = row_to_pb(row);
        assert_eq!(msg.config_json, r#"{"mode":"velion-genius"}"#);
        assert_eq!(msg.version, 7);
        assert_eq!(msg.updated_by, "user-123");
        assert_eq!(msg.updated_at, 1_700_000_000);
    }
}
