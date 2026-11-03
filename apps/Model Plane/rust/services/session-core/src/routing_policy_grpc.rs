//! gRPC handler for the `RoutingPolicy` service.
//!
//! Owns the durable store for the Verevon intent layer's runtime policy (the
//! "model router"). The intent layer in inference-core resolves the
//! `verevon-budget` / `verevon-balance` / `verevon-genius` modes to a concrete
//! model from request complexity + budget posture; this service makes that
//! policy editable at runtime instead of via compile-time constants.
//!
//! Storage is a JSONB singleton row (`routing_policy` table, `id = 1`). The
//! full policy travels as an opaque JSON string in `config_json` — the
//! canonical schema is owned by inference-core's `RoutingPolicy` struct.
//! session-core only validates the string parses as JSON before storing it;
//! it never interprets the contents. The BFF reads/writes through `SetPolicy`
//! and inference-core polls `GetPolicy`.
//!
//! A change can also go through a candidate (EP-14 S10). `ProposeCandidate`
//! stores a proposed policy beside the live row, `RecordCandidateEvaluation`
//! records the review, and `PromoteCandidate` writes the live row in the same
//! transaction that marks the candidate promoted. Promotion writes only while
//! the live version is still the one the candidate was proposed against, so a
//! candidate that lost a race is superseded instead of applied.

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

/// The switch for the candidate RPCs, read once at boot. Only the literal `true` enables it.
const CANDIDATES_ENV: &str = "SESSION_CORE_ROUTING_CANDIDATES_ENABLED";

/// The page a candidate list returns when the caller names no limit, and the ceiling for any limit.
const DEFAULT_LIST_LIMIT: i32 = 50;
const MAX_LIST_LIMIT: i32 = 200;

/// The singleton's columns, in the order `RoutingPolicyRow` reads them.
const POLICY_COLUMNS: &str = "config, version, updated_by,
    extract(epoch from updated_at)::bigint AS updated_at, candidate_id";

/// A candidate's columns, in the order `CandidateRow` reads them.
const CANDIDATE_COLUMNS: &str = "id, config, base_version, proposed_by,
    extract(epoch from created_at)::bigint AS proposed_at, state, diff, live_eval,
    waiver_reason, decided_by";

/// Row shape returned by the singleton read/write queries. `updated_at` is
/// projected as Unix seconds (`extract(epoch ...)::bigint`) so it maps straight
/// onto the proto's `int64 updated_at` without timezone gymnastics.
#[derive(sqlx::FromRow)]
struct RoutingPolicyRow {
    config: serde_json::Value,
    version: i64,
    updated_by: String,
    updated_at: i64,
    candidate_id: String,
}

fn row_to_pb(row: RoutingPolicyRow) -> pb::RoutingPolicyMessage {
    pb::RoutingPolicyMessage {
        config_json: row.config.to_string(),
        version: row.version,
        updated_by: row.updated_by,
        updated_at: row.updated_at,
        candidate_id: row.candidate_id,
    }
}

/// Row shape of a candidate. `proposed_at` is projected as Unix seconds, as `updated_at` is.
#[derive(sqlx::FromRow)]
struct CandidateRow {
    id: String,
    config: serde_json::Value,
    base_version: i64,
    proposed_by: String,
    proposed_at: i64,
    state: String,
    diff: serde_json::Value,
    live_eval: serde_json::Value,
    waiver_reason: String,
    decided_by: String,
}

fn candidate_to_pb(row: CandidateRow) -> pb::PolicyCandidate {
    pb::PolicyCandidate {
        id: row.id,
        config_json: row.config.to_string(),
        base_version: row.base_version,
        proposed_by: row.proposed_by,
        proposed_at: row.proposed_at,
        state: row.state,
        diff_json: row.diff.to_string(),
        live_eval_json: row.live_eval.to_string(),
        waiver_reason: row.waiver_reason,
        decided_by: row.decided_by,
    }
}

fn db_error(error: sqlx::Error) -> Status {
    Status::internal(error.to_string())
}

/// Validate a wire JSON field parses as JSON before it is stored as JSONB.
/// Mirrors `finetune_grpc::parse_hyperparameters`: an empty string is treated as
/// the empty object, and any malformed JSON is rejected with `invalid_argument`.
fn parse_json(field: &str, raw: &str) -> Result<serde_json::Value, Status> {
    if raw.is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_str(raw)
        .map_err(|e| Status::invalid_argument(format!("{field} must be valid JSON: {e}")))
}

fn parse_config(raw: &str) -> Result<serde_json::Value, Status> {
    parse_json("config_json", raw)
}

/// Changing the live policy, and every candidate RPC, is platform-global: only an admin-scoped user or a
/// service principal may do it. A regular member token must not be able to change every org's routing.
fn require_policy_admin(caller: &crate::auth::VerifiedIdentity) -> Result<(), Status> {
    if caller.is_service() || caller.has_scope("admin") {
        return Ok(());
    }
    Err(Status::permission_denied(
        "changing the routing policy requires the admin scope",
    ))
}

/// The strict reading of `SESSION_CORE_ROUTING_CANDIDATES_ENABLED`: only the literal `true` enables it.
fn candidates_enabled_from(raw: Option<&str>) -> bool {
    raw.is_some_and(|value| value.trim() == "true")
}

/// With the switch off the candidate RPCs refuse, so a deployment that never turns it on keeps exactly the
/// direct write it had.
fn require_candidates(enabled: bool) -> Result<(), Status> {
    if enabled {
        return Ok(());
    }
    Err(Status::failed_precondition(
        "routing policy candidates are not enabled (SESSION_CORE_ROUTING_CANDIDATES_ENABLED)",
    ))
}

/// Why a candidate cannot be promoted yet, or `None` when it can. Promotion needs an evaluated candidate (an
/// accepted diff) and either a passing live evaluation or a written waiver reason.
fn promotion_refusal(
    state: &str,
    live_eval: &serde_json::Value,
    waiver_reason: &str,
) -> Option<String> {
    if state != "evaluated" {
        return Some(format!(
            "candidate is {state}; only an evaluated candidate can be promoted"
        ));
    }
    let passed = live_eval.get("passed").and_then(serde_json::Value::as_bool) == Some(true);
    if !passed && waiver_reason.trim().is_empty() {
        return Some("promotion needs a passing live evaluation or a waiver reason".to_owned());
    }
    None
}

/// The refusal for a candidate that the live policy has moved past.
fn stale_candidate(candidate: &CandidateRow) -> Status {
    Status::aborted(format!(
        "candidate {} was proposed against version {}, which is no longer live; it is superseded",
        candidate.id, candidate.base_version
    ))
}

/// The page size a candidate list returns: the default for 0, the ceiling above it, and a refusal for a
/// negative limit.
fn list_limit(requested: i32) -> Result<i32, Status> {
    match requested {
        0 => Ok(DEFAULT_LIST_LIMIT),
        n if n < 0 => Err(Status::invalid_argument("limit must not be negative")),
        n => Ok(n.min(MAX_LIST_LIMIT)),
    }
}

pub struct RoutingPolicyService {
    pool: PgPool,
    candidates_enabled: bool,
}

impl RoutingPolicyService {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            candidates_enabled: candidates_enabled_from(
                std::env::var(CANDIDATES_ENV).ok().as_deref(),
            ),
        }
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
            let row: RoutingPolicyRow = sqlx::query_as(&format!(
                "SELECT {POLICY_COLUMNS} FROM routing_policy WHERE id = 1"
            ))
            .fetch_one(&self.pool)
            .await
            .map_err(db_error)?;

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
            let caller = crate::auth::identity(&request)?;
            require_policy_admin(&caller)?;
            let req = request.into_inner();
            let config = parse_config(&req.config_json)?;
            if req.expected_version < 0 {
                return Err(Status::invalid_argument(
                    "expected_version must not be negative",
                ));
            }

            let mut tx = self.pool.begin().await.map_err(db_error)?;
            // An expected_version of 0 matches any live version: the write that predates the field.
            let row: Option<RoutingPolicyRow> = sqlx::query_as(&format!(
                "UPDATE routing_policy SET
                    config = $1::jsonb,
                    version = version + 1,
                    updated_by = $2,
                    updated_at = now(),
                    candidate_id = ''
                 WHERE id = 1 AND ($3::bigint = 0 OR version = $3::bigint)
                 RETURNING {POLICY_COLUMNS}"
            ))
            .bind(config)
            .bind(&req.updated_by)
            .bind(req.expected_version)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?;
            let Some(row) = row else {
                return Err(Status::aborted(format!(
                    "routing policy changed since version {} was read; reload it and retry",
                    req.expected_version
                )));
            };
            // A direct write moves the live version past every pending candidate's base, so none of them can
            // promote now. They are superseded in the same transaction as the write.
            sqlx::query(
                "UPDATE routing_policy_candidates SET state = 'superseded', decided_at = now()
                 WHERE state IN ('proposed', 'evaluated')",
            )
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
            tx.commit().await.map_err(db_error)?;
            Ok(Response::new(row_to_pb(row)))
        }
        .await;
        record_metrics("set_policy", started, result.is_ok());
        result
    }

    async fn propose_candidate(
        &self,
        request: Request<pb::ProposeCandidateRequest>,
    ) -> Result<Response<pb::ProposeCandidateResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::ProposeCandidateResponse>, Status> = async {
            let caller = crate::auth::identity(&request)?;
            require_policy_admin(&caller)?;
            require_candidates(self.candidates_enabled)?;
            let req = request.into_inner();
            let config = parse_config(&req.config_json)?;
            let diff = parse_json("diff_json", &req.diff_json)?;
            // The base is the live version, read by the same statement that inserts the candidate.
            let row: CandidateRow = sqlx::query_as(&format!(
                "INSERT INTO routing_policy_candidates (id, config, base_version, proposed_by, state, diff)
                 SELECT $1, $2::jsonb, version, $3, 'proposed', $4::jsonb
                 FROM routing_policy WHERE id = 1
                 RETURNING {CANDIDATE_COLUMNS}"
            ))
            .bind(format!("cand_{}", mp_ids::new_ulid()))
            .bind(config)
            .bind(&req.proposed_by)
            .bind(diff)
            .fetch_one(&self.pool)
            .await
            .map_err(db_error)?;
            Ok(Response::new(pb::ProposeCandidateResponse {
                candidate: Some(candidate_to_pb(row)),
            }))
        }
        .await;
        record_metrics("propose_candidate", started, result.is_ok());
        result
    }

    async fn record_candidate_evaluation(
        &self,
        request: Request<pb::RecordCandidateEvaluationRequest>,
    ) -> Result<Response<pb::RecordCandidateEvaluationResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::RecordCandidateEvaluationResponse>, Status> = async {
            let caller = crate::auth::identity(&request)?;
            require_policy_admin(&caller)?;
            require_candidates(self.candidates_enabled)?;
            let req = request.into_inner();
            let live_eval = parse_json("live_eval_json", &req.live_eval_json)?;
            let mut tx = self.pool.begin().await.map_err(db_error)?;
            let state: Option<String> = sqlx::query_scalar(
                "SELECT state FROM routing_policy_candidates WHERE id = $1 FOR UPDATE",
            )
            .bind(&req.candidate_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?;
            let state =
                state.ok_or_else(|| Status::not_found("routing policy candidate not found"))?;
            if state != "proposed" {
                return Err(Status::failed_precondition(format!(
                    "candidate is {state}; only a proposed candidate takes a review"
                )));
            }
            let next_state = if req.diff_accepted {
                "evaluated"
            } else {
                "rejected"
            };
            let row: CandidateRow = sqlx::query_as(&format!(
                "UPDATE routing_policy_candidates SET
                    state = $2,
                    live_eval = $3::jsonb,
                    waiver_reason = $4,
                    decided_by = $5,
                    decided_at = now()
                 WHERE id = $1
                 RETURNING {CANDIDATE_COLUMNS}"
            ))
            .bind(&req.candidate_id)
            .bind(next_state)
            .bind(live_eval)
            .bind(&req.waiver_reason)
            .bind(&req.decided_by)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
            tx.commit().await.map_err(db_error)?;
            Ok(Response::new(pb::RecordCandidateEvaluationResponse {
                candidate: Some(candidate_to_pb(row)),
            }))
        }
        .await;
        record_metrics("record_candidate_evaluation", started, result.is_ok());
        result
    }

    async fn promote_candidate(
        &self,
        request: Request<pb::PromoteCandidateRequest>,
    ) -> Result<Response<pb::PromoteCandidateResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::PromoteCandidateResponse>, Status> = async {
            let caller = crate::auth::identity(&request)?;
            require_policy_admin(&caller)?;
            require_candidates(self.candidates_enabled)?;
            let req = request.into_inner();
            let mut tx = self.pool.begin().await.map_err(db_error)?;
            // The live row is locked before the candidate, the order set_policy takes its locks in, so a promotion
            // and a direct write cannot each wait on the other.
            let live_version: i64 =
                sqlx::query_scalar("SELECT version FROM routing_policy WHERE id = 1 FOR UPDATE")
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(db_error)?;
            let candidate: Option<CandidateRow> = sqlx::query_as(&format!(
                "SELECT {CANDIDATE_COLUMNS} FROM routing_policy_candidates WHERE id = $1 FOR UPDATE"
            ))
            .bind(&req.candidate_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?;
            let candidate =
                candidate.ok_or_else(|| Status::not_found("routing policy candidate not found"))?;
            if candidate.state == "superseded" {
                return Err(stale_candidate(&candidate));
            }
            if let Some(refusal) = promotion_refusal(
                &candidate.state,
                &candidate.live_eval,
                &candidate.waiver_reason,
            ) {
                return Err(Status::failed_precondition(refusal));
            }
            if candidate.base_version != live_version {
                // A direct write landed after this candidate was proposed, so it can never apply. The supersession
                // is committed, and the call answers ABORTED so the caller proposes again.
                sqlx::query(
                    "UPDATE routing_policy_candidates SET state = 'superseded', decided_at = now()
                     WHERE id = $1",
                )
                .bind(&candidate.id)
                .execute(&mut *tx)
                .await
                .map_err(db_error)?;
                tx.commit().await.map_err(db_error)?;
                return Err(stale_candidate(&candidate));
            }
            let row: RoutingPolicyRow = sqlx::query_as(&format!(
                "UPDATE routing_policy SET
                    config = $1::jsonb,
                    version = version + 1,
                    updated_by = $2,
                    updated_at = now(),
                    candidate_id = $3
                 WHERE id = 1
                 RETURNING {POLICY_COLUMNS}"
            ))
            .bind(candidate.config.clone())
            .bind(&req.decided_by)
            .bind(&candidate.id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
            sqlx::query(
                "UPDATE routing_policy_candidates SET state = 'promoted', decided_by = $2, decided_at = now()
                 WHERE id = $1",
            )
            .bind(&candidate.id)
            .bind(&req.decided_by)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
            // Every other pending candidate was proposed against an older version, and this write moved past it.
            sqlx::query(
                "UPDATE routing_policy_candidates SET state = 'superseded', decided_at = now()
                 WHERE state IN ('proposed', 'evaluated') AND id <> $1",
            )
            .bind(&candidate.id)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
            tx.commit().await.map_err(db_error)?;
            Ok(Response::new(pb::PromoteCandidateResponse {
                policy: Some(row_to_pb(row)),
            }))
        }
        .await;
        record_metrics("promote_candidate", started, result.is_ok());
        result
    }

    async fn list_candidates(
        &self,
        request: Request<pb::ListCandidatesRequest>,
    ) -> Result<Response<pb::ListCandidatesResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::ListCandidatesResponse>, Status> = async {
            let caller = crate::auth::identity(&request)?;
            require_policy_admin(&caller)?;
            require_candidates(self.candidates_enabled)?;
            let limit = list_limit(request.get_ref().limit)?;
            let rows: Vec<CandidateRow> = sqlx::query_as(&format!(
                "SELECT {CANDIDATE_COLUMNS} FROM routing_policy_candidates
                 ORDER BY created_at DESC, id DESC
                 LIMIT $1"
            ))
            .bind(i64::from(limit))
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
            Ok(Response::new(pb::ListCandidatesResponse {
                candidates: rows.into_iter().map(candidate_to_pb).collect(),
            }))
        }
        .await;
        record_metrics("list_candidates", started, result.is_ok());
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
        let v = parse_config(r#"{"mode":"verevon-balance"}"#).unwrap();
        assert_eq!(v, serde_json::json!({"mode": "verevon-balance"}));
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
            config: serde_json::json!({"mode": "verevon-genius"}),
            version: 7,
            updated_by: "user-123".to_owned(),
            updated_at: 1_700_000_000,
            candidate_id: "cand_01".to_owned(),
        };
        let msg = row_to_pb(row);
        assert_eq!(msg.config_json, r#"{"mode":"verevon-genius"}"#);
        assert_eq!(msg.version, 7);
        assert_eq!(msg.updated_by, "user-123");
        assert_eq!(msg.updated_at, 1_700_000_000);
        assert_eq!(msg.candidate_id, "cand_01");
    }

    #[test]
    fn candidates_switch_reads_only_the_literal_true() {
        assert!(candidates_enabled_from(Some("true")));
        assert!(candidates_enabled_from(Some(" true ")));
        for raw in [
            None,
            Some(""),
            Some("1"),
            Some("TRUE"),
            Some("yes"),
            Some("false"),
        ] {
            assert!(!candidates_enabled_from(raw), "{raw:?}");
        }
    }

    #[test]
    fn candidate_rpcs_refuse_while_the_switch_is_off() {
        let err = require_candidates(false).unwrap_err();
        assert_eq!(err.code(), tonic::Code::FailedPrecondition);
        assert!(require_candidates(true).is_ok());
    }

    #[test]
    fn candidate_rpcs_need_the_admin_scope_or_a_service() {
        let service = crate::auth::VerifiedIdentity::service_for_test("platform", &[], false);
        assert!(require_policy_admin(&service).is_ok());
        let member = crate::auth::VerifiedIdentity::user_for_test("org-1", "u1");
        let err = require_policy_admin(&member).unwrap_err();
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }

    #[test]
    fn promotion_needs_an_evaluated_candidate() {
        let passing = serde_json::json!({"passed": true});
        let refused = promotion_refusal("proposed", &passing, "").expect("a proposal is refused");
        assert!(refused.contains("proposed"), "{refused}");
        assert!(promotion_refusal("evaluated", &passing, "").is_none());
    }

    #[test]
    fn promotion_needs_a_passing_evaluation_or_a_waiver() {
        let failing = serde_json::json!({"passed": false});
        assert!(promotion_refusal("evaluated", &serde_json::json!({}), "").is_some());
        assert!(promotion_refusal("evaluated", &failing, "   ").is_some());
        // A waiver stands in for the live evaluation, and a written reason is what makes it a waiver.
        assert!(promotion_refusal("evaluated", &failing, "reviewed by ops").is_none());
    }

    #[test]
    fn list_limit_defaults_caps_and_refuses_negatives() {
        assert_eq!(list_limit(0).unwrap(), DEFAULT_LIST_LIMIT);
        assert_eq!(list_limit(10).unwrap(), 10);
        assert_eq!(list_limit(10_000).unwrap(), MAX_LIST_LIMIT);
        assert_eq!(
            list_limit(-1).unwrap_err().code(),
            tonic::Code::InvalidArgument
        );
    }

    #[test]
    fn candidate_to_pb_copies_the_review_trail() {
        let row = CandidateRow {
            id: "cand_01".to_owned(),
            config: serde_json::json!({"mode": "verevon-balance"}),
            base_version: 3,
            proposed_by: "admin-1".to_owned(),
            proposed_at: 1_700_000_100,
            state: "evaluated".to_owned(),
            diff: serde_json::json!({"changed": 2}),
            live_eval: serde_json::json!({"passed": true}),
            waiver_reason: String::new(),
            decided_by: "reviewer-1".to_owned(),
        };
        let msg = candidate_to_pb(row);
        assert_eq!(msg.id, "cand_01");
        assert_eq!(msg.base_version, 3);
        assert_eq!(msg.state, "evaluated");
        assert_eq!(msg.diff_json, r#"{"changed":2}"#);
        assert_eq!(msg.live_eval_json, r#"{"passed":true}"#);
        assert_eq!(msg.decided_by, "reviewer-1");
    }
}

/// Database tests for the expected-version check and the candidate flow. They need a disposable Postgres:
///
///   DATABASE_URL=... cargo test -p session-core --bin session-core promote_ -- --ignored --test-threads=1
#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::auth::VerifiedIdentity;

    async fn open_service(candidates_enabled: bool) -> RoutingPolicyService {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
        let pool = PgPool::connect(&url).await.expect("connect postgres");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");
        RoutingPolicyService {
            pool,
            candidates_enabled,
        }
    }

    fn admin() -> VerifiedIdentity {
        VerifiedIdentity::service_for_test("platform", &[], false)
    }

    fn as_caller<T>(message: T, caller: VerifiedIdentity) -> Request<T> {
        let mut request = Request::new(message);
        request.extensions_mut().insert(caller);
        request
    }

    fn set(config: &serde_json::Value, expected_version: i64) -> pb::SetRoutingPolicyRequest {
        pb::SetRoutingPolicyRequest {
            config_json: config.to_string(),
            updated_by: "test-admin".to_owned(),
            expected_version,
        }
    }

    fn propose(config: &serde_json::Value) -> pb::ProposeCandidateRequest {
        pb::ProposeCandidateRequest {
            config_json: config.to_string(),
            proposed_by: "test-admin".to_owned(),
            diff_json: r#"{"changed":0}"#.to_owned(),
        }
    }

    fn review(
        id: &str,
        accepted: bool,
        live_eval: &str,
        waiver: &str,
    ) -> pb::RecordCandidateEvaluationRequest {
        pb::RecordCandidateEvaluationRequest {
            candidate_id: id.to_owned(),
            diff_accepted: accepted,
            live_eval_json: live_eval.to_owned(),
            waiver_reason: waiver.to_owned(),
            decided_by: "test-reviewer".to_owned(),
        }
    }

    async fn live_version(service: &RoutingPolicyService) -> i64 {
        service
            .get_policy(as_caller(pb::GetRoutingPolicyRequest {}, admin()))
            .await
            .expect("get")
            .into_inner()
            .version
    }

    async fn proposed_id(service: &RoutingPolicyService, config: &serde_json::Value) -> String {
        service
            .propose_candidate(as_caller(propose(config), admin()))
            .await
            .expect("propose")
            .into_inner()
            .candidate
            .expect("candidate")
            .id
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn set_policy_expected_version_conflict() {
        let service = open_service(true).await;
        let config = serde_json::json!({"enabled": true});
        // Two admins read the same version. The first write lands, and the second, still holding the old
        // version, is refused instead of overwriting it.
        let read = live_version(&service).await;
        let first = service
            .set_policy(as_caller(set(&config, read), admin()))
            .await
            .expect("first write lands")
            .into_inner();
        assert_eq!(first.version, read + 1);
        let stale = service
            .set_policy(as_caller(set(&config, read), admin()))
            .await
            .expect_err("stale write refused");
        assert_eq!(stale.code(), tonic::Code::Aborted);
        // expected_version 0 keeps today's unchecked write.
        let unchecked = service
            .set_policy(as_caller(set(&config, 0), admin()))
            .await
            .expect("unchecked write");
        assert_eq!(unchecked.into_inner().version, read + 2);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn promote_requires_evaluation() {
        let service = open_service(true).await;
        let config = serde_json::json!({"enabled": true, "budget_cap_usd": 40.0});
        let id = proposed_id(&service, &config).await;
        let early = service
            .promote_candidate(as_caller(
                pb::PromoteCandidateRequest {
                    candidate_id: id.clone(),
                    decided_by: "test-admin".to_owned(),
                },
                admin(),
            ))
            .await
            .expect_err("a proposed candidate is not promoted");
        assert_eq!(early.code(), tonic::Code::FailedPrecondition);
        // Accepted, but no passing live evaluation and no waiver: still not promotable.
        service
            .record_candidate_evaluation(as_caller(review(&id, true, "{}", ""), admin()))
            .await
            .expect("review");
        let unevaluated = service
            .promote_candidate(as_caller(
                pb::PromoteCandidateRequest {
                    candidate_id: id,
                    decided_by: "test-admin".to_owned(),
                },
                admin(),
            ))
            .await
            .expect_err("no live evidence is refused");
        assert_eq!(unevaluated.code(), tonic::Code::FailedPrecondition);
        assert!(unevaluated.message().contains("passing live evaluation"));
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn promote_with_waiver_reason() {
        let service = open_service(true).await;
        let config = serde_json::json!({"enabled": true, "budget_cap_usd": 41.0});
        let read = live_version(&service).await;
        let id = proposed_id(&service, &config).await;
        service
            .record_candidate_evaluation(as_caller(
                review(
                    &id,
                    true,
                    r#"{"passed":false}"#,
                    "live eval skipped: test org outage",
                ),
                admin(),
            ))
            .await
            .expect("review with a waiver");
        let promoted = service
            .promote_candidate(as_caller(
                pb::PromoteCandidateRequest {
                    candidate_id: id.clone(),
                    decided_by: "test-admin".to_owned(),
                },
                admin(),
            ))
            .await
            .expect("waived candidate promotes")
            .into_inner()
            .policy
            .expect("the live policy");
        assert_eq!(promoted.version, read + 1);
        assert_eq!(promoted.candidate_id, id);
        let stored: serde_json::Value =
            serde_json::from_str(&promoted.config_json).expect("config json");
        assert_eq!(stored, config);
        let state: String =
            sqlx::query_scalar("SELECT state FROM routing_policy_candidates WHERE id = $1")
                .bind(&id)
                .fetch_one(&service.pool)
                .await
                .expect("state");
        assert_eq!(state, "promoted");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn promote_supersedes_on_newer_live_version() {
        let service = open_service(true).await;
        let config = serde_json::json!({"enabled": true, "budget_cap_usd": 42.0});
        let id = proposed_id(&service, &config).await;
        service
            .record_candidate_evaluation(as_caller(
                review(&id, true, r#"{"passed":true}"#, ""),
                admin(),
            ))
            .await
            .expect("review");
        // A direct write lands first, so the candidate's base is no longer live.
        service
            .set_policy(as_caller(
                set(&serde_json::json!({"enabled": false}), 0),
                admin(),
            ))
            .await
            .expect("direct write");
        let refused = service
            .promote_candidate(as_caller(
                pb::PromoteCandidateRequest {
                    candidate_id: id.clone(),
                    decided_by: "test-admin".to_owned(),
                },
                admin(),
            ))
            .await
            .expect_err("a stale candidate is aborted");
        assert_eq!(refused.code(), tonic::Code::Aborted);
        let state: String =
            sqlx::query_scalar("SELECT state FROM routing_policy_candidates WHERE id = $1")
                .bind(&id)
                .fetch_one(&service.pool)
                .await
                .expect("state");
        assert_eq!(state, "superseded");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn candidate_rpcs_need_admin_scope() {
        let service = open_service(true).await;
        let member = VerifiedIdentity::user_for_test("org-1", "member");
        let denied = service
            .propose_candidate(as_caller(
                propose(&serde_json::json!({"enabled": true})),
                member.clone(),
            ))
            .await
            .expect_err("a member cannot propose");
        assert_eq!(denied.code(), tonic::Code::PermissionDenied);
        let listed = service
            .list_candidates(as_caller(pb::ListCandidatesRequest { limit: 5 }, member))
            .await
            .expect_err("a member cannot list candidates");
        assert_eq!(listed.code(), tonic::Code::PermissionDenied);
        // With the switch off, even an admin is told the RPCs are not enabled.
        let off = open_service(false).await;
        let refused = off
            .propose_candidate(as_caller(
                propose(&serde_json::json!({"enabled": true})),
                admin(),
            ))
            .await
            .expect_err("the switch gates the RPCs");
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn promote_keeps_a_candidate_role_table() {
        // EP-13 S4 put a role table inside the RoutingPolicy document. A candidate carries it unchanged: the
        // candidate store treats the config as opaque JSON, so the roles survive propose, review and promote.
        let service = open_service(true).await;
        let config = serde_json::json!({
            "enabled": true,
            "roles": {
                "title": {"candidates": ["gpt-5-nano"], "effort": "minimal"},
                "research_plan": {"candidates": ["claude-opus-4-8"]}
            }
        });
        let id = proposed_id(&service, &config).await;
        service
            .record_candidate_evaluation(as_caller(
                review(&id, true, r#"{"passed":true}"#, ""),
                admin(),
            ))
            .await
            .expect("review");
        let promoted = service
            .promote_candidate(as_caller(
                pb::PromoteCandidateRequest {
                    candidate_id: id,
                    decided_by: "test-admin".to_owned(),
                },
                admin(),
            ))
            .await
            .expect("a candidate with roles promotes")
            .into_inner()
            .policy
            .expect("the live policy");
        let stored: serde_json::Value =
            serde_json::from_str(&promoted.config_json).expect("config json");
        assert_eq!(stored, config);
    }
}
