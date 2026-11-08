//! `EventLog` gRPC service: the read path of the run trace (WS10.S6).
//!
//! `QueryEvents` is a facade over `run_trace_events` (migration 0086), the table
//! `trace_nats.rs` fills from Quarry's `mp.v1.trace.run.*` events. It is the only RPC served in v1:
//! `AppendEvent` and `SubscribeEvents` are `UNIMPLEMENTED` (the trace has one writer, the NATS
//! consumer, and the SPA polls with a cursor).
//!
//! Rules of the read:
//!
//! * `filter.run_id` is required, and `filter.org_id` must be the caller's org. There is no
//!   cross-run query.
//! * The caller must be able to read the run's thread, exactly as `ReplayThread` requires
//!   (`authorize_thread_owner` with `OwnerIntent::Read`), and must not have lost the Space the run
//!   lives in. Anyone else gets `PERMISSION_DENIED`; an unknown run, or a run of another org, is
//!   `NOT_FOUND`. The model-gateway route turns both into a 404.
//! * Order is the server-assigned `ingest_seq`, ascending (descending with `reverse`). The cursor
//!   is `mp_eventlog::Cursor { offset: ingest_seq }` of the last row returned. It is returned
//!   whenever the page has rows, `has_more` or not, so a poller of a live run can resume from the
//!   end (the proto comment says "empty when exhausted"; `has_more` is the exhaustion signal).
//! * `limit` is clamped to 1..=500; 0 means unset and reads 100.
//! * Identity-column commit-order skew (a row with a lower `ingest_seq` committing after one with
//!   a higher) is accepted: there is one consumer and one row per statement, and a reader that
//!   wants certainty re-reads the last page.

// tonic::Status is the unavoidable large Err for gRPC.
#![allow(clippy::result_large_err)]

pub(crate) use mp_contracts::model_plane::v1::event_log_server::EventLogServer;
use mp_contracts::model_plane::v1::{self as pb, event_log_server::EventLog};
use mp_eventlog::{Cursor, CursorPayload, EventFilter as DomainFilter, EventTypeMask};
use sqlx::PgPool;
use tonic::{Request, Response, Status};

use crate::auth::{authorize_operation, identity, OwnerIntent};
use crate::trace_nats::TRACE_EVENT_TYPES;

/// The default page when the request names no limit.
pub const DEFAULT_LIMIT: u32 = 100;
/// The largest page.
pub const MAX_LIMIT: u32 = 500;
/// Type URL of a trace event's payload (JSON bytes in the `Any`).
pub const TRACE_PAYLOAD_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.BrowserTraceEvent";

/// Clamp a requested page size.
#[must_use]
pub fn clamp_limit(requested: u32) -> u32 {
    if requested == 0 {
        DEFAULT_LIMIT
    } else {
        requested.clamp(1, MAX_LIMIT)
    }
}

/// The `EventLog` service.
pub struct EventLogService {
    pool: PgPool,
}

impl EventLogService {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct TraceRow {
    ingest_seq: i64,
    event_id: String,
    event_type: String,
    schema_version: i32,
    occurred_at: chrono::DateTime<chrono::Utc>,
    producer: String,
    producer_seq: i64,
    org_id: String,
    user_id: String,
    resource_ref: String,
    correlation_id: String,
    causation_id: String,
    request_id: String,
    agent_id: String,
    agent_role: String,
    agent_parent_id: String,
    origin: String,
    run_id: String,
    payload: serde_json::Value,
}

const SELECT_ROWS: &str =
    "SELECT ingest_seq, event_id, event_type, schema_version, occurred_at, producer, \
     producer_seq, org_id, user_id, resource_ref, correlation_id, causation_id, request_id, \
     agent_id, agent_role, agent_parent_id, origin, run_id, payload \
     FROM run_trace_events \
     WHERE org_id = $1 AND run_id = $2 \
       AND ($4::text[] IS NULL OR event_type = ANY($4)) \
       AND ($5 = '' OR correlation_id = $5) \
       AND ($6 = '' OR resource_ref = $6) ";

fn origin_to_proto(origin: &str) -> i32 {
    match origin {
        "USER" => pb::RunOrigin::User as i32,
        "AGENT" => pb::RunOrigin::Agent as i32,
        "SCHEDULE" => pb::RunOrigin::Schedule as i32,
        "WEBHOOK" => pb::RunOrigin::Webhook as i32,
        "API" => pb::RunOrigin::Api as i32,
        _ => pb::RunOrigin::Unspecified as i32,
    }
}

fn row_to_proto(row: TraceRow) -> pb::Event {
    let TraceRow {
        ingest_seq: _,
        event_id,
        event_type,
        schema_version,
        occurred_at,
        producer,
        producer_seq,
        org_id,
        user_id,
        resource_ref,
        correlation_id,
        causation_id,
        request_id,
        agent_id,
        agent_role,
        agent_parent_id,
        origin,
        run_id,
        payload,
    } = row;
    let agent = (!agent_id.is_empty() || !agent_role.is_empty() || !agent_parent_id.is_empty())
        .then_some(pb::AgentRef {
            id: agent_id,
            role: agent_role,
            parent_id: agent_parent_id,
        });
    pb::Event {
        event_id,
        event_type: crate::grpc::event_type_to_i32(&event_type),
        schema_version: u32::try_from(schema_version).unwrap_or(0),
        ts: Some(prost_types::Timestamp {
            seconds: occurred_at.timestamp(),
            nanos: i32::try_from(occurred_at.timestamp_subsec_nanos()).unwrap_or(0),
        }),
        producer,
        correlation_id,
        causation_id,
        org_id,
        user_id,
        resource_ref,
        payload: Some(prost_types::Any {
            type_url: TRACE_PAYLOAD_TYPE_URL.to_owned(),
            value: serde_json::to_vec(&payload).unwrap_or_default(),
        }),
        // Only non-ZDR runs have rows.
        zdr: false,
        producer_seq: u64::try_from(producer_seq).unwrap_or(0),
        agent,
        origin: origin_to_proto(&origin),
        run_id,
        request_id,
        ..Default::default()
    }
}

/// The trace event type names a request's numeric types select. `None` = no type filter. A type
/// that is not a trace type selects nothing; the list can come back empty, which is "no rows".
fn selected_type_names(mask: &EventTypeMask) -> Option<Vec<String>> {
    if mask.is_any() {
        return None;
    }
    Some(
        TRACE_EVENT_TYPES
            .iter()
            .filter(|name| {
                u32::try_from(crate::grpc::event_type_to_i32(name)).is_ok_and(|n| mask.matches(n))
            })
            .map(|name| (*name).to_owned())
            .collect(),
    )
}

#[tonic::async_trait]
impl EventLog for EventLogService {
    async fn append_event(
        &self,
        _request: Request<pb::AppendEventRequest>,
    ) -> Result<Response<pb::AppendEventResponse>, Status> {
        Err(Status::unimplemented(
            "EventLog.AppendEvent is not served: the run trace is written by its NATS consumer",
        ))
    }

    async fn query_events(
        &self,
        request: Request<pb::QueryEventsRequest>,
    ) -> Result<Response<pb::QueryEventsResponse>, Status> {
        let caller = identity(&request)?;
        authorize_operation(&caller, "session:read")?;
        let req = request.into_inner();

        let wire = req
            .filter
            .ok_or_else(|| Status::invalid_argument("filter is required"))?;
        let filter = DomainFilter {
            org_id: wire.org_id,
            run_id: wire.run_id,
            event_types: wire
                .event_types
                .iter()
                .filter_map(|t| u32::try_from(*t).ok())
                .collect(),
            correlation_id: wire.correlation_id,
            resource_ref: wire.resource_ref,
        };
        filter
            .validate()
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        // The trace has no cross-run reading.
        if filter.run_id.is_empty() {
            return Err(Status::invalid_argument("filter.run_id is required"));
        }
        caller.authorize_org(&filter.org_id)?;

        let run: Option<(String, String)> =
            sqlx::query_as("SELECT org_id, thread_id FROM runs WHERE id = $1")
                .bind(&filter.run_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| Status::internal(error.to_string()))?;
        // A run of another org is as absent as an unknown one.
        let (run_org, thread_id) = run
            .filter(|(org, _)| *org == filter.org_id)
            .ok_or_else(|| Status::not_found("run not found"))?;
        debug_assert_eq!(run_org, filter.org_id);
        crate::grpc::authorize_thread_owner(&self.pool, &caller, &thread_id, OwnerIntent::Read)
            .await?;
        // Also the run-level gate (org, owner, and a Space membership that was revoked).
        crate::run_service_grpc::authorize_run_owner(
            &self.pool,
            &caller,
            &filter.run_id,
            OwnerIntent::Read,
        )
        .await?;

        let limit = clamp_limit(req.limit);
        let offset = match req.cursor.as_ref().map(|c| c.value.as_str()) {
            None | Some("") => None,
            Some(encoded) => {
                let cursor = Cursor::decode(encoded)
                    .map_err(|_| Status::invalid_argument("cursor is not valid"))?;
                Some(
                    i64::try_from(cursor.payload().offset)
                        .map_err(|_| Status::invalid_argument("cursor is not valid"))?,
                )
            }
        };

        let types = selected_type_names(&filter.event_types);
        if types.as_ref().is_some_and(Vec::is_empty) {
            return Ok(Response::new(pb::QueryEventsResponse::default()));
        }

        let sql = if req.reverse {
            format!("{SELECT_ROWS} AND ingest_seq < $3 ORDER BY ingest_seq DESC LIMIT $7")
        } else {
            format!("{SELECT_ROWS} AND ingest_seq > $3 ORDER BY ingest_seq ASC LIMIT $7")
        };
        let bound = offset.unwrap_or(if req.reverse { i64::MAX } else { 0 });
        let mut rows: Vec<TraceRow> = sqlx::query_as(&sql)
            .bind(&filter.org_id)
            .bind(&filter.run_id)
            .bind(bound)
            .bind(types)
            .bind(&filter.correlation_id)
            .bind(&filter.resource_ref)
            .bind(i64::from(limit) + 1)
            .fetch_all(&self.pool)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;

        let has_more = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        let next_cursor = match rows.last() {
            Some(last) => {
                let encoded = Cursor::new(CursorPayload {
                    offset: u64::try_from(last.ingest_seq).unwrap_or(0),
                })
                .encode()
                .map_err(|error| Status::internal(error.to_string()))?;
                Some(pb::Cursor { value: encoded })
            }
            None => None,
        };
        Ok(Response::new(pb::QueryEventsResponse {
            events: rows.into_iter().map(row_to_proto).collect(),
            next_cursor,
            has_more,
        }))
    }

    type SubscribeEventsStream = tokio_stream::wrappers::ReceiverStream<Result<pb::Event, Status>>;

    async fn subscribe_events(
        &self,
        _request: Request<pb::SubscribeEventsRequest>,
    ) -> Result<Response<Self::SubscribeEventsStream>, Status> {
        Err(Status::unimplemented(
            "EventLog.SubscribeEvents is not served: poll QueryEvents with a cursor",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::VerifiedIdentity;
    use crate::loop_checkpoint::test_support::{scratch_pool, seed_run};
    use crate::trace_nats::{persist, validate_trace_envelope, TraceEvent};
    use serde_json::{json, Value};

    const GOLDEN: &str = include_str!("../tests/fixtures/run-trace-envelope.golden.json");

    fn event_for(run_id: &str, org: &str, seq: u64, event_type: &str) -> TraceEvent {
        let mut value: Value = serde_json::from_str(GOLDEN).expect("golden");
        value["run_id"] = json!(run_id);
        value["org_id"] = json!(org);
        value["producer_seq"] = json!(seq);
        value["event_type"] = json!(event_type);
        value["event_id"] = json!(format!("ev-{run_id}-{seq}"));
        value["resource_ref"] = json!(format!("run/{run_id}"));
        validate_trace_envelope(&serde_json::to_vec(&value).expect("json")).expect("valid")
    }

    fn request(
        caller: VerifiedIdentity,
        org: &str,
        run: &str,
        cursor: &str,
        limit: u32,
    ) -> Request<pb::QueryEventsRequest> {
        let mut request = Request::new(pb::QueryEventsRequest {
            filter: Some(pb::EventFilter {
                org_id: org.to_owned(),
                run_id: run.to_owned(),
                ..Default::default()
            }),
            cursor: (!cursor.is_empty()).then(|| pb::Cursor {
                value: cursor.to_owned(),
            }),
            limit,
            reverse: false,
        });
        request.extensions_mut().insert(caller);
        request
    }

    #[test]
    fn the_limit_is_clamped_to_one_through_five_hundred() {
        assert_eq!(clamp_limit(0), DEFAULT_LIMIT);
        assert_eq!(clamp_limit(1), 1);
        assert_eq!(clamp_limit(37), 37);
        assert_eq!(clamp_limit(500), 500);
        assert_eq!(clamp_limit(501), 500);
        assert_eq!(clamp_limit(u32::MAX), 500);
    }

    #[test]
    fn a_type_filter_selects_only_trace_types() {
        let any = EventTypeMask::any();
        assert_eq!(selected_type_names(&any), None);
        let step_and_egress = EventTypeMask::from_iter([154, 155, 101]);
        assert_eq!(
            selected_type_names(&step_and_egress),
            Some(vec![
                "BROWSER_STEP_RECEIPT_RECORDED".to_owned(),
                "BROWSER_EGRESS_DECIDED".to_owned()
            ])
        );
        // 101 (STEP_COMPLETED) is not a trace type: selecting only it selects nothing.
        assert_eq!(
            selected_type_names(&EventTypeMask::from_iter([101])),
            Some(Vec::new())
        );
    }

    #[test]
    fn append_and_subscribe_are_not_served() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        rt.block_on(async {
            let pool = sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgresql://session-core@127.0.0.1:1/session_core")
                .expect("lazy pool");
            let service = EventLogService::new(pool);
            let appended = service
                .append_event(Request::new(pb::AppendEventRequest::default()))
                .await
                .expect_err("append");
            assert_eq!(appended.code(), tonic::Code::Unimplemented);
            let subscribed = service
                .subscribe_events(Request::new(pb::SubscribeEventsRequest::default()))
                .await
                .err()
                .expect("subscribe");
            assert_eq!(subscribed.code(), tonic::Code::Unimplemented);
        });
    }

    #[test]
    fn the_eventlog_service_is_in_the_health_names() {
        assert!(crate::grpc::HEALTH_SERVICE_NAMES.contains(&"model_plane.v1.EventLog"));
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn trace_query_pages_by_ingest_seq_without_skips_or_repeats() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, user) = seed_run(&pool, "persistent").await;
        // Persist out of producer order: the read order is the ingest order.
        for seq in [1_u64, 2, 3, 5, 4, 6, 7, 8, 9, 10, 11, 12] {
            let kind = if seq % 2 == 0 {
                "BROWSER_STEP_RECEIPT_RECORDED"
            } else {
                "BROWSER_EGRESS_DECIDED"
            };
            persist(&pool, &event_for(&run, &org, seq, kind))
                .await
                .expect("persist");
        }
        let service = EventLogService::new(pool.clone());
        let owner = VerifiedIdentity::user_for_test(&org, &user);

        let mut seen = Vec::new();
        let mut cursor = String::new();
        let mut pages = 0;
        loop {
            let page = service
                .query_events(request(owner.clone(), &org, &run, &cursor, 5))
                .await
                .expect("page")
                .into_inner();
            pages += 1;
            seen.extend(page.events.iter().map(|e| e.producer_seq));
            for event in &page.events {
                assert_eq!(event.run_id, run);
                assert_eq!(event.org_id, org);
                assert_eq!(event.producer, "quarry-edge");
                assert_eq!(event.origin, pb::RunOrigin::Agent as i32);
                assert_eq!(
                    event.agent.as_ref().map(|a| a.id.as_str()),
                    Some("general-v1")
                );
                assert!(!event.zdr);
                let payload = event.payload.as_ref().expect("payload");
                assert_eq!(payload.type_url, TRACE_PAYLOAD_TYPE_URL);
                let body: Value = serde_json::from_slice(&payload.value).expect("json payload");
                assert_eq!(body["outcome"], "ok");
            }
            cursor = page.next_cursor.map(|c| c.value).unwrap_or_default();
            if !page.has_more {
                break;
            }
            assert!(!cursor.is_empty(), "a page with more has a cursor");
            assert!(pages < 10, "paging must terminate");
        }
        // Ingest order: 5 landed before 4.
        assert_eq!(seen, vec![1, 2, 3, 5, 4, 6, 7, 8, 9, 10, 11, 12]);
        assert_eq!(pages, 3, "12 rows at 5 per page");

        // The last cursor is a resume point: nothing new, then one new row.
        let empty = service
            .query_events(request(owner.clone(), &org, &run, &cursor, 5))
            .await
            .expect("tail")
            .into_inner();
        assert!(empty.events.is_empty() && !empty.has_more);
        persist(&pool, &event_for(&run, &org, 13, "BROWSER_SESSION_CLOSED"))
            .await
            .expect("13");
        let tail = service
            .query_events(request(owner.clone(), &org, &run, &cursor, 5))
            .await
            .expect("tail")
            .into_inner();
        assert_eq!(
            tail.events
                .iter()
                .map(|e| e.producer_seq)
                .collect::<Vec<_>>(),
            vec![13]
        );

        // A type filter and reverse order.
        let mut by_type = request(owner.clone(), &org, &run, "", 500);
        by_type
            .get_mut()
            .filter
            .as_mut()
            .expect("filter")
            .event_types = vec![155];
        by_type.get_mut().reverse = true;
        let reversed = service
            .query_events(by_type)
            .await
            .expect("typed")
            .into_inner();
        let seqs: Vec<u64> = reversed.events.iter().map(|e| e.producer_seq).collect();
        assert_eq!(seqs, vec![11, 9, 7, 5, 3, 1]);

        // A forged cursor is refused, not read as zero.
        let forged = service
            .query_events(request(owner, &org, &run, "!!!not-a-cursor", 5))
            .await
            .expect_err("forged");
        assert_eq!(forged.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn trace_query_requires_a_run_id() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, user) = seed_run(&pool, "persistent").await;
        persist(&pool, &event_for(&run, &org, 1, "BROWSER_SESSION_OPENED"))
            .await
            .expect("persist");
        let service = EventLogService::new(pool);
        let owner = VerifiedIdentity::user_for_test(&org, &user);
        // No run id: no cross-run read, even for the org's own member.
        let none = service
            .query_events(request(owner.clone(), &org, "", "", 10))
            .await
            .expect_err("run id required");
        assert_eq!(none.code(), tonic::Code::InvalidArgument);
        // No filter at all, and no org.
        let mut bare = Request::new(pb::QueryEventsRequest::default());
        bare.extensions_mut().insert(owner.clone());
        assert_eq!(
            service.query_events(bare).await.expect_err("filter").code(),
            tonic::Code::InvalidArgument
        );
        assert_eq!(
            service
                .query_events(request(owner.clone(), "", &run, "", 10))
                .await
                .expect_err("org")
                .code(),
            tonic::Code::InvalidArgument
        );
        // An unknown run is not found; a run read under another org is not found either.
        assert_eq!(
            service
                .query_events(request(owner, &org, "no-such-run", "", 10))
                .await
                .expect_err("unknown")
                .code(),
            tonic::Code::NotFound
        );
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn trace_query_denies_a_non_owner() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, user) = seed_run(&pool, "persistent").await;
        persist(&pool, &event_for(&run, &org, 1, "BROWSER_SESSION_OPENED"))
            .await
            .expect("persist");
        let service = EventLogService::new(pool);

        // Another member of the same org.
        let colleague = VerifiedIdentity::user_for_test(&org, "someone-else");
        assert_eq!(
            service
                .query_events(request(colleague, &org, &run, "", 10))
                .await
                .expect_err("colleague")
                .code(),
            tonic::Code::PermissionDenied
        );
        // A user of another org, naming the right org in the filter, and naming their own.
        let outsider = VerifiedIdentity::user_for_test("other-org", &user);
        assert_eq!(
            service
                .query_events(request(outsider.clone(), &org, &run, "", 10))
                .await
                .expect_err("outsider")
                .code(),
            tonic::Code::PermissionDenied
        );
        assert_eq!(
            service
                .query_events(request(outsider, "other-org", &run, "", 10))
                .await
                .expect_err("outsider own org")
                .code(),
            tonic::Code::NotFound
        );
        // The owner reads.
        let owner = VerifiedIdentity::user_for_test(&org, &user);
        let page = service
            .query_events(request(owner, &org, &run, "", 10))
            .await
            .expect("owner")
            .into_inner();
        assert_eq!(page.events.len(), 1);
    }
}
