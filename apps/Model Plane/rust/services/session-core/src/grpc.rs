//! gRPC server implementing `SessionCore` on :9091.

use chrono::Utc;
use mp_contracts::dataplane::graph_v1::{
    self as graph_pb, graph_service_client::GraphServiceClient,
};
use mp_contracts::dataplane::knowledge_v2::{
    self as know_pb, knowledge_service_client::KnowledgeServiceClient,
};
use mp_contracts::dataplane::retrieval_v2::{
    self as ret_pb, retrieval_service_client::RetrievalServiceClient,
};
use mp_contracts::model_plane::v1::{
    self as pb,
    session_core_server::{SessionCore, SessionCoreServer},
};
use mp_events::idempotency::derive_idempotency_hash;
use mp_ids::new_ulid;
use sqlx::PgPool;
use std::time::Instant;
use tonic::transport::Channel;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

const RUN_STARTED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.RunStarted";
const STEP_COMPLETED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.StepCompleted";
const RUN_TERMINAL_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.RunTerminal";
const CHECKPOINT_SAVED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.CheckpointSaved";
const THREAD_CREATED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.ThreadCreated";
const MESSAGE_APPENDED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.MessageAppended";
const CONTEXT_MESSAGE_LIMIT: i64 = 20;
const CONTEXT_MEMORY_LIMIT: i64 = 64;

fn nanos_to_i32(nanos: u32) -> i32 {
    i32::try_from(nanos).unwrap_or(i32::MAX)
}

fn record_metrics(method: &'static str, started: Instant, is_ok: bool) {
    let status = if is_ok { "ok" } else { "error" };
    metrics::counter!("mp_session_grpc_requests_total", "method" => method, "status" => status)
        .increment(1);
    metrics::histogram!("mp_session_grpc_request_duration_seconds", "method" => method)
        .record(started.elapsed().as_secs_f64());
}

#[derive(Default)]
struct TopicBuckets {
    policy: Vec<String>,
    workspace: Vec<String>,
    agent: Vec<String>,
    user: Vec<String>,
    episodic: Vec<String>,
    skill_index: Vec<String>,
    skill_expansion: Vec<String>,
    retrieval: Vec<String>,
}

fn bucket_memory_segments(rows: Vec<(String, String)>) -> TopicBuckets {
    let mut buckets = TopicBuckets::default();

    for (topic, content) in rows {
        if content.is_empty() {
            continue;
        }

        match topic.as_str() {
            "POLICY" => buckets.policy.push(content),
            "WORKSPACE" => buckets.workspace.push(content),
            "AGENT" => buckets.agent.push(content),
            "USER" => buckets.user.push(content),
            "MEMORY" => buckets.episodic.push(content),
            "SKILL_INDEX" => buckets.skill_index.push(content),
            "SKILL_EXPANSION" => buckets.skill_expansion.push(content),
            "RETRIEVAL" => buckets.retrieval.push(content),
            _ => buckets.episodic.push(format!("{topic}: {content}")),
        }
    }

    buckets
}

pub struct SessionService {
    pool: PgPool,
    retrieval_client: Option<RetrievalServiceClient<Channel>>,
    graph_client: Option<GraphServiceClient<Channel>>,
    knowledge_client: Option<KnowledgeServiceClient<Channel>>,
}

/// Insert the `THREAD_CREATED` event row for a freshly created thread, within
/// the caller's open transaction.
async fn insert_thread_created_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    thread_id: &str,
    req: &pb::CreateThreadRequest,
    now: chrono::DateTime<Utc>,
) -> Result<(), Status> {
    let thread_resource = format!("thread:{thread_id}");
    let thread_idem = derive_idempotency_hash(
        "session-core",
        "THREAD_CREATED",
        &thread_resource,
        &format!("{thread_id}:created"),
    );
    sqlx::query(
        "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
         VALUES ($1, 'THREAD_CREATED', $2, $3, $4, $5, $6, $7, '', $8, $9, $10, 'session-core', 1)",
    )
    .bind(new_ulid())
    .bind(thread_id)
    .bind(serde_json::json!({
        "thread_id": thread_id,
        "session_key": &req.session_key,
    }))
    .bind(now)
    .bind(&req.org_id)
    .bind(&req.user_id)
    .bind(thread_id)
    .bind(&thread_idem)
    .bind(&thread_resource)
    .bind(THREAD_CREATED_TYPE_URL)
    .execute(&mut **tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;
    Ok(())
}

/// Core of `SessionCore::create_thread`, factored out to keep the trait method
/// small. Inserts the thread row and its `THREAD_CREATED` event in one tx.
async fn create_thread_inner(
    pool: &PgPool,
    req: pb::CreateThreadRequest,
) -> Result<Response<pb::CreateThreadResponse>, Status> {
    let thread_id = new_ulid();
    let now = Utc::now();

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    sqlx::query(
        "INSERT INTO threads (id, session_key, org_id, user_id, created_at) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(&thread_id)
    .bind(&req.session_key)
    .bind(&req.org_id)
    .bind(&req.user_id)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;

    insert_thread_created_event(&mut tx, &thread_id, &req, now).await?;

    tx.commit()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    info!(thread_id = %thread_id, "thread created");

    Ok(Response::new(pb::CreateThreadResponse {
        thread_id,
        created_at: Some(prost_types::Timestamp {
            seconds: now.timestamp(),
            nanos: nanos_to_i32(now.timestamp_subsec_nanos()),
        }),
    }))
}

/// Core of `SessionCore::append_message`, factored out to keep the trait method
/// small. Inserts the message row and its `MESSAGE_APPENDED` event in one tx.
async fn append_message_inner(
    pool: &PgPool,
    req: pb::AppendMessageRequest,
) -> Result<Response<pb::AppendMessageResponse>, Status> {
    let msg_id = new_ulid();
    let now = Utc::now();

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    let row: (i64,) = sqlx::query_as(
        "INSERT INTO messages (id, thread_id, role, content, created_at)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING sequence",
    )
    .bind(&msg_id)
    .bind(&req.thread_id)
    .bind(&req.role)
    .bind(&req.content)
    .bind(now)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;

    let sequence =
        u64::try_from(row.0).map_err(|_| Status::internal("message sequence out of range"))?;

    let msg_resource = format!("message:{}", &msg_id);
    let msg_idem = derive_idempotency_hash(
        "session-core",
        "MESSAGE_APPENDED",
        &msg_resource,
        &format!("{}:{}", &req.thread_id, sequence),
    );
    sqlx::query(
        "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
         SELECT $1, 'MESSAGE_APPENDED', $2, $3, $4, t.org_id, t.user_id, $5, '', $6, $7, $8, 'session-core', 1
         FROM threads t WHERE t.id = $2",
    )
    .bind(new_ulid())
    .bind(&req.thread_id)
    .bind(serde_json::json!({
        "thread_id": &req.thread_id,
        "message_id": &msg_id,
        "sequence": sequence,
        "role": &req.role,
    }))
    .bind(now)
    .bind(&req.thread_id)
    .bind(&msg_idem)
    .bind(&msg_resource)
    .bind(MESSAGE_APPENDED_TYPE_URL)
    .execute(&mut *tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    Ok(Response::new(pb::AppendMessageResponse { sequence }))
}

/// Core of `SessionCore::start_run`, factored out to keep the trait method
/// small. Inserts the run row and its `RUN_STARTED` event in one tx.
async fn start_run_inner(
    pool: &PgPool,
    req: pb::StartRunRequest,
) -> Result<Response<pb::StartRunResponse>, Status> {
    let run_id = new_ulid();
    let now = Utc::now();

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    sqlx::query(
        "INSERT INTO runs (id, thread_id, parent_run_id, agent_id, goal, mode, org_id, user_id, status, created_at, updated_at)
         VALUES ($1, $2, NULLIF($3, ''), $4, $5, $6, $7, $8, 'queued', $9, $9)",
    )
    .bind(&run_id)
    .bind(&req.thread_id)
    .bind(&req.parent_run_id)
    .bind(&req.agent_id)
    .bind(&req.goal)
    .bind(&req.mode)
    .bind(&req.org_id)
    .bind(&req.user_id)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;

    let run_started_resource = format!("run:{}", &run_id);
    let run_started_idem = derive_idempotency_hash(
        "session-core",
        "RUN_STARTED",
        &run_started_resource,
        &format!("{}:started", &run_id),
    );
    sqlx::query(
        "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
         VALUES ($1, 'RUN_STARTED', $2, $3, $4, $5, $6, $7, '', $8, $9, $10, 'session-core', 1)",
    )
    .bind(new_ulid())
    .bind(&run_id)
    .bind(serde_json::json!({
        "goal": &req.goal,
        "mode": &req.mode,
        "agent_id": &req.agent_id,
    }))
    .bind(now)
    .bind(&req.org_id)
    .bind(&req.user_id)
    .bind(&run_id)
    .bind(&run_started_idem)
    .bind(&run_started_resource)
    .bind(RUN_STARTED_TYPE_URL)
    .execute(&mut *tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    info!(run_id = %run_id, "run started");

    Ok(Response::new(pb::StartRunResponse {
        run_id,
        created_at: Some(prost_types::Timestamp {
            seconds: now.timestamp(),
            nanos: nanos_to_i32(now.timestamp_subsec_nanos()),
        }),
    }))
}

/// Record a terminal run event (`RUN_COMPLETED` / `RUN_FAILED`) and flip the
/// run's status, within the caller's open transaction. `causation_event_id` is
/// the `STEP_COMPLETED` event that drove the terminal transition.
async fn record_run_terminal(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    req: &pb::CompleteStepRequest,
    causation_event_id: &str,
) -> Result<(), Status> {
    let (terminal_event_type, run_status) = if req.status == "failed" {
        ("RUN_FAILED", "failed")
    } else {
        ("RUN_COMPLETED", "completed")
    };

    let terminal_resource = format!("run:{}", &req.run_id);
    let terminal_idem = derive_idempotency_hash(
        "session-core",
        terminal_event_type,
        &terminal_resource,
        &format!("{}:terminal", &req.run_id),
    );
    sqlx::query(
        "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
         SELECT $1, $2, $3, $4, now(), r.org_id, r.user_id, $3, $5, $8, $6, $7, 'session-core', 1
         FROM runs r WHERE r.id = $3",
    )
    .bind(new_ulid())
    .bind(terminal_event_type)
    .bind(&req.run_id)
    .bind(serde_json::json!({ "error": &req.error }))
    .bind(causation_event_id)
    .bind(&terminal_resource)
    .bind(RUN_TERMINAL_TYPE_URL)
    .bind(&terminal_idem)
    .execute(&mut **tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;

    sqlx::query("UPDATE runs SET status = $1, ended_at = now(), updated_at = now() WHERE id = $2")
        .bind(run_status)
        .bind(&req.run_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
    Ok(())
}

/// Core of `SessionCore::complete_step`, factored out to keep the trait method
/// small. Appends the `STEP_COMPLETED` event and, on a terminal status, the
/// run-terminal event + status flip, all in one tx.
async fn complete_step_inner(
    pool: &PgPool,
    req: pb::CompleteStepRequest,
) -> Result<Response<pb::CompleteStepResponse>, Status> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    // Append step event; trigger assigns step_ordinal.
    let event_id = new_ulid();
    let step_resource = format!("run:{}:step:{}", &req.run_id, &req.step_id);
    let step_idem = derive_idempotency_hash(
        "session-core",
        "STEP_COMPLETED",
        &step_resource,
        &format!("{}:{}", &req.run_id, &req.step_id),
    );
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
          SELECT $1, 'STEP_COMPLETED', $2, $3, now(), r.org_id, r.user_id, $2, '', $4, $5, $6, 'session-core', 1
         FROM runs r WHERE r.id = $2
         RETURNING step_ordinal",
    )
    .bind(&event_id)
    .bind(&req.run_id)
    .bind(serde_json::json!({
        "step_id": req.step_id,
        "status": req.status,
        "output": req.output,
        "error": req.error,
    }))
    .bind(&step_idem)
    .bind(&step_resource)
    .bind(STEP_COMPLETED_TYPE_URL)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;

    if req.status == "completed" || req.status == "failed" {
        record_run_terminal(&mut tx, &req, &event_id).await?;
    }

    tx.commit()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    let step_index =
        u32::try_from(row.0).map_err(|_| Status::internal("step index out of range"))?;

    Ok(Response::new(pb::CompleteStepResponse { step_index }))
}

/// One event row in the exact column order selected by `replay_thread_task`'s
/// query (see that function's `SELECT` for the field-by-field mapping).
type ReplayEventRow = (
    String,
    String,
    serde_json::Value,
    chrono::NaiveDateTime,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    i32,
);

/// Map a replay event row to its proto `Event`.
fn replay_event_row_to_proto(row: ReplayEventRow) -> pb::Event {
    let (
        event_id,
        event_type,
        payload,
        ts,
        org_id,
        user_id,
        correlation_id,
        causation_id,
        type_url,
        idempotency_key,
        resource_ref,
        producer,
        schema_version,
    ) = row;
    pb::Event {
        event_id,
        event_type: event_type_to_i32(&event_type),
        schema_version: u32::try_from(schema_version).unwrap_or(0),
        ts: Some(prost_types::Timestamp {
            seconds: ts.and_utc().timestamp(),
            nanos: nanos_to_i32(ts.and_utc().timestamp_subsec_nanos()),
        }),
        producer,
        correlation_id,
        causation_id,
        idempotency_key,
        org_id,
        user_id,
        resource_ref,
        payload: Some(prost_types::Any {
            type_url,
            value: serde_json::to_vec(&payload).unwrap_or_default(),
        }),
        zdr: false,
    }
}

/// Spawned worker for `SessionCore::replay_thread`: replays a thread's events
/// (ordered by `(ts, id)` for determinism) onto the response stream.
async fn replay_thread_task(
    pool: PgPool,
    req: pb::ReplayThreadRequest,
    tx: tokio::sync::mpsc::Sender<Result<pb::Event, Status>>,
) {
    let rows = sqlx::query_as::<_, ReplayEventRow>(
        "SELECT e.id, e.event_type, e.payload, e.ts, e.org_id, e.user_id, e.correlation_id, e.causation_id, e.type_url, e.idempotency_key, e.resource_ref, e.producer, e.schema_version
         FROM events e
         JOIN runs r ON e.run_id = r.id
         WHERE r.thread_id = $1
         AND ($2 = '' OR e.id > $2)
         ORDER BY e.ts ASC, e.id ASC
         LIMIT CASE WHEN $3 = 0 THEN 10000 ELSE $3 END",
    )
    .bind(&req.thread_id)
    .bind(&req.after_event_id)
    .bind(i64::from(req.limit))
    .fetch_all(&pool)
    .await;

    match rows {
        Ok(events) => {
            for row in events {
                if tx.send(Ok(replay_event_row_to_proto(row))).await.is_err() {
                    break;
                }
            }
        }
        Err(e) => {
            let _ = tx.send(Err(Status::internal(e.to_string()))).await;
        }
    }
}

/// Recent non-empty thread messages (oldest-first) for context assembly.
async fn load_thread_messages(
    pool: &PgPool,
    thread_id: &str,
) -> Result<Vec<(String, String)>, Status> {
    let mut msgs = sqlx::query_as::<_, (String, String)>(
        "SELECT role, content
         FROM (
             SELECT role, content, sequence
             FROM messages
             WHERE thread_id = $1
             ORDER BY sequence DESC
             LIMIT $2
         ) recent
         ORDER BY sequence ASC",
    )
    .bind(thread_id)
    .bind(CONTEXT_MESSAGE_LIMIT)
    .fetch_all(pool)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;
    msgs.retain(|(_, content)| !content.is_empty());
    Ok(msgs)
}

/// Core of `SessionCore::get_context_assembly`. Loads run metadata, recent
/// messages, and memory buckets; fans out to retrieval/knowledge/graph; then
/// assembles the budgeted context segments.
async fn get_context_assembly_inner(
    svc: &SessionService,
    req: pb::GetContextAssemblyRequest,
) -> Result<Response<pb::GetContextAssemblyResponse>, Status> {
    let thread_id = req.thread_id.clone();
    let run_id = req.run_id.clone();

    let run_meta: Option<(String, String, String)> = if run_id.is_empty() {
        None
    } else {
        sqlx::query_as::<_, (String, String, String)>(
            "SELECT agent_id, user_id, goal FROM runs WHERE id = $1",
        )
        .bind(&run_id)
        .fetch_optional(&svc.pool)
        .await
        .map_err(|e| Status::internal(e.to_string()))?
    };

    let (run_agent, user_id, prompt_goal) = match run_meta {
        Some((a, u, g)) => (Some(a), Some(u), Some(g)),
        None => (None, None, None),
    };

    let agent_id = if req.agent_id.is_empty() {
        run_agent
    } else {
        Some(req.agent_id.clone())
    };

    let thread_messages = load_thread_messages(&svc.pool, &thread_id).await?;

    let memory_rows = sqlx::query_as::<_, (String, String)>(
        "SELECT topic, content
         FROM memory_index
         WHERE thread_id = $1
         ORDER BY updated_at DESC
         LIMIT $2",
    )
    .bind(&thread_id)
    .bind(CONTEXT_MEMORY_LIMIT)
    .fetch_all(&svc.pool)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;

    let memory = bucket_memory_segments(memory_rows);

    let evidence = svc
        .fetch_retrieval_segments(
            &thread_id,
            &thread_messages,
            req.max_tokens,
            &memory.retrieval,
        )
        .await;

    // Persist live Data Plane retrieval into memory_index so the next call's
    // `bucket_memory_segments` path picks it up even when Data Plane is
    // unreachable. Best-effort; never blocking.
    if evidence.source == RetrievalSource::DataPlane {
        svc.persist_retrieval_evidence(&thread_id, &evidence.segments)
            .await;
    }
    if evidence.low_confidence {
        info!(thread_id, "retrieval returned low_confidence");
    }

    let knowledge_segments = svc
        .fetch_knowledge_segments(&thread_id, &evidence.document_ids)
        .await;
    let graph_segments = svc.fetch_graph_segments(&thread_id).await;

    let inputs = AssemblyInputs {
        policy_id: req.policy_id,
        workspace_id: req.workspace_id,
        agent_id,
        user_id,
        prompt_goal,
        thread_messages,
        policy_segments: memory.policy,
        workspace_segments: memory.workspace,
        agent_segments: memory.agent,
        user_segments: memory.user,
        episodic_segments: memory.episodic,
        skill_index_segments: memory.skill_index,
        skill_expansion_segments: memory.skill_expansion,
        retrieval_segments: evidence.segments,
        knowledge_segments,
        graph_segments,
        run_id,
        max_tokens: req.max_tokens,
    };

    let (segments, estimated_tokens) = assemble_segments(&inputs);

    Ok(Response::new(pb::GetContextAssemblyResponse {
        segments,
        estimated_tokens,
    }))
}

#[tonic::async_trait]
impl SessionCore for SessionService {
    async fn create_thread(
        &self,
        request: Request<pb::CreateThreadRequest>,
    ) -> Result<Response<pb::CreateThreadResponse>, Status> {
        let started = Instant::now();
        let result = create_thread_inner(&self.pool, request.into_inner()).await;
        record_metrics("create_thread", started, result.is_ok());
        result
    }

    async fn append_message(
        &self,
        request: Request<pb::AppendMessageRequest>,
    ) -> Result<Response<pb::AppendMessageResponse>, Status> {
        let started = Instant::now();
        let result = append_message_inner(&self.pool, request.into_inner()).await;
        record_metrics("append_message", started, result.is_ok());
        result
    }

    async fn start_run(
        &self,
        request: Request<pb::StartRunRequest>,
    ) -> Result<Response<pb::StartRunResponse>, Status> {
        let started = Instant::now();
        let result = start_run_inner(&self.pool, request.into_inner()).await;
        record_metrics("start_run", started, result.is_ok());
        result
    }

    async fn complete_step(
        &self,
        request: Request<pb::CompleteStepRequest>,
    ) -> Result<Response<pb::CompleteStepResponse>, Status> {
        let started = Instant::now();
        let result = complete_step_inner(&self.pool, request.into_inner()).await;
        record_metrics("complete_step", started, result.is_ok());
        result
    }

    async fn save_checkpoint(
        &self,
        request: Request<pb::SaveCheckpointRequest>,
    ) -> Result<Response<pb::SaveCheckpointResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::SaveCheckpointResponse>, Status> = async {
            let req = request.into_inner();
            let now = Utc::now();

            let mut tx = self
                .pool
                .begin()
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            let row: (i64,) = sqlx::query_as(
                "INSERT INTO checkpoints (id, run_id, state, created_at)
                 VALUES ($1, $2, $3, $4)
                 RETURNING ordinal",
            )
            .bind(&req.checkpoint_id)
            .bind(&req.run_id)
            .bind(&req.state[..])
            .bind(now)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

            let checkpoint_resource = format!("run:{}:checkpoint:{}", &req.run_id, &req.checkpoint_id);
            let checkpoint_idem = derive_idempotency_hash(
                "session-core",
                "CHECKPOINT_SAVED",
                &checkpoint_resource,
                &format!("{}:{}", &req.run_id, &req.checkpoint_id),
            );
            sqlx::query(
                "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
                 SELECT $1, 'CHECKPOINT_SAVED', $2, $3, $4, r.org_id, r.user_id, $2, '', $5, $6, $7, 'session-core', 1
                 FROM runs r WHERE r.id = $2",
            )
            .bind(new_ulid())
            .bind(&req.run_id)
            .bind(serde_json::json!({
                "checkpoint_id": &req.checkpoint_id,
                "size": req.state.len(),
            }))
            .bind(now)
            .bind(&checkpoint_idem)
            .bind(&checkpoint_resource)
            .bind(CHECKPOINT_SAVED_TYPE_URL)
            .execute(&mut *tx)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

            tx.commit()
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            let checkpoint_index = u32::try_from(row.0)
                .map_err(|_| Status::internal("checkpoint index out of range"))?;

            Ok(Response::new(pb::SaveCheckpointResponse {
                checkpoint_index,
                saved_at: Some(prost_types::Timestamp {
                    seconds: now.timestamp(),
                    nanos: nanos_to_i32(now.timestamp_subsec_nanos()),
                }),
            }))
        }
        .await;
        record_metrics("save_checkpoint", started, result.is_ok());
        result
    }

    type ReplayThreadStream = tokio_stream::wrappers::ReceiverStream<Result<pb::Event, Status>>;

    async fn replay_thread(
        &self,
        request: Request<pb::ReplayThreadRequest>,
    ) -> Result<Response<Self::ReplayThreadStream>, Status> {
        let started = Instant::now();
        let req = request.into_inner();
        let pool = self.pool.clone();

        let (tx, rx) = tokio::sync::mpsc::channel(64);
        tokio::spawn(replay_thread_task(pool, req, tx));

        let resp = Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            rx,
        )));
        record_metrics("replay_thread", started, true);
        resp
    }

    async fn compact_now(
        &self,
        request: Request<pb::CompactNowRequest>,
    ) -> Result<Response<pb::CompactNowResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::CompactNowResponse>, Status> = async {
            let req = request.into_inner();
            let n = crate::compaction::compact_once(&self.pool)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            let summary = if req.toon {
                let payload = serde_json::json!({
                    "compacted": n,
                    "service": "session-core",
                    "ts": Utc::now().to_rfc3339(),
                });
                mp_toon::encode(&payload)
            } else {
                format!("compaction complete: {n} checkpoint(s) inserted")
            };
            info!(compacted = n, toon = req.toon, "on-demand compaction");
            Ok(Response::new(pb::CompactNowResponse {
                compacted_count: n,
                summary,
            }))
        }
        .await;
        record_metrics("compact_now", started, result.is_ok());
        result
    }

    // G7 closed learning loop: persist a skill body into agent_skills. The
    // capability-core learning loop calls this with origin="background_review";
    // a human skill editor uses origin="user". Provenance is enforced in the
    // DB: a background_review upsert never overwrites a user-authored skill
    // (the `WHERE` guard on the conflict), surfaced as `skipped_protected`
    // rather than an error so the loop yields gracefully to the human.
    async fn upsert_agent_skill(
        &self,
        request: Request<pb::UpsertAgentSkillRequest>,
    ) -> Result<Response<pb::UpsertAgentSkillResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::UpsertAgentSkillResponse>, Status> = async {
            let req = request.into_inner();
            if req.org_id.is_empty() || req.name.is_empty() {
                return Err(Status::invalid_argument("org_id and name are required"));
            }
            let origin = match req.origin.as_str() {
                "" | "background_review" => "background_review",
                "user" => "user",
                other => {
                    return Err(Status::invalid_argument(format!("invalid origin: {other}")));
                }
            };
            let id = new_ulid();
            // None ⟺ the conflict guard rejected the write (target is a
            // protected user skill); a fresh insert or permitted update always
            // returns a row.
            let row: Option<(String, bool)> = sqlx::query_as(
                "INSERT INTO agent_skills
                    (id, org_id, name, description, content, trigger_keywords,
                     trigger_file_patterns, tool_restrictions, enabled, origin,
                     created_at, updated_at)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10, now(), now())
                 ON CONFLICT (org_id, name) DO UPDATE SET
                     description = EXCLUDED.description,
                     content = EXCLUDED.content,
                     trigger_keywords = EXCLUDED.trigger_keywords,
                     trigger_file_patterns = EXCLUDED.trigger_file_patterns,
                     tool_restrictions = EXCLUDED.tool_restrictions,
                     enabled = EXCLUDED.enabled,
                     updated_at = now()
                 WHERE agent_skills.origin <> 'user' OR EXCLUDED.origin = 'user'
                 RETURNING id, (xmax = 0) AS created",
            )
            .bind(&id)
            .bind(&req.org_id)
            .bind(&req.name)
            .bind(&req.description)
            .bind(&req.content)
            .bind(serde_json::json!(req.trigger_keywords))
            .bind(serde_json::json!(req.trigger_file_patterns))
            .bind(serde_json::json!(req.tool_restrictions))
            .bind(req.enabled)
            .bind(origin)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| {
                warn!(error = %e, "upsert_agent_skill failed");
                Status::internal(e.to_string())
            })?;

            let resp = match row {
                Some((id, created)) => pb::UpsertAgentSkillResponse {
                    id,
                    created,
                    skipped_protected: false,
                },
                None => pb::UpsertAgentSkillResponse {
                    id: String::new(),
                    created: false,
                    skipped_protected: true,
                },
            };
            Ok(Response::new(resp))
        }
        .await;
        record_metrics("upsert_agent_skill", started, result.is_ok());
        result
    }

    // ROADMAP P3 run modes: durably set a run's mode on the runs row. The
    // gateway's in-memory plan-mode cache write-throughs here so plan mode
    // survives restart (auditability/resume — GOAL.md §7).
    async fn set_run_mode(
        &self,
        request: Request<pb::SetRunModeRequest>,
    ) -> Result<Response<pb::SetRunModeResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::SetRunModeResponse>, Status> = async {
            let req = request.into_inner();
            if req.run_id.is_empty() || req.org_id.is_empty() {
                return Err(Status::invalid_argument("run_id and org_id are required"));
            }
            let mode = match req.mode.as_str() {
                "execute" | "plan" | "reactive" | "research" => req.mode.as_str(),
                other => {
                    return Err(Status::invalid_argument(format!(
                        "invalid run mode: {other}"
                    )));
                }
            };
            // Org-scoped UPDATE: a run is only mutable by its owning org, so a
            // caller can never flip the mode of another org's run (per-org
            // isolation invariant). A missing row ⟺ wrong org OR unknown run;
            // both surface as not_found without leaking which.
            let row: Option<(String, String)> = sqlx::query_as(
                "UPDATE runs SET mode = $2, updated_at = now()
                 WHERE id = $1 AND org_id = $3 RETURNING id, mode",
            )
            .bind(&req.run_id)
            .bind(mode)
            .bind(&req.org_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| {
                warn!(error = %e, "set_run_mode failed");
                Status::internal(e.to_string())
            })?;
            let (run_id, mode) =
                row.ok_or_else(|| Status::not_found(format!("run {} not found", req.run_id)))?;
            Ok(Response::new(pb::SetRunModeResponse { run_id, mode }))
        }
        .await;
        record_metrics("set_run_mode", started, result.is_ok());
        result
    }

    // G7 read path (the loop's "last mile"): list an org's agent skills so the
    // gateway's MatchSkills cache can surface LEARNED skills, not just disk
    // ones. Org-scoped — only this org's rows. The jsonb array columns are
    // NOT NULL DEFAULT '[]', so they decode cleanly into Vec<String>.
    async fn list_agent_skills(
        &self,
        request: Request<pb::ListAgentSkillsRequest>,
    ) -> Result<Response<pb::ListAgentSkillsResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::ListAgentSkillsResponse>, Status> = async {
            let req = request.into_inner();
            if req.org_id.is_empty() {
                return Err(Status::invalid_argument("org_id is required"));
            }
            type Row = (
                String,
                String,
                String,
                String,
                sqlx::types::Json<Vec<String>>,
                sqlx::types::Json<Vec<String>>,
                sqlx::types::Json<Vec<String>>,
                bool,
                String,
            );
            let rows: Vec<Row> = sqlx::query_as(
                "SELECT id, name, description, content, trigger_keywords,
                        trigger_file_patterns, tool_restrictions, enabled, origin
                 FROM agent_skills
                 WHERE org_id = $1 AND (NOT $2 OR enabled)
                 ORDER BY name",
            )
            .bind(&req.org_id)
            .bind(req.enabled_only)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                warn!(error = %e, "list_agent_skills failed");
                Status::internal(e.to_string())
            })?;
            let skills = rows
                .into_iter()
                .map(
                    |(id, name, description, content, kw, fp, tr, enabled, origin)| {
                        pb::AgentSkill {
                            id,
                            name,
                            description,
                            content,
                            trigger_keywords: kw.0,
                            trigger_file_patterns: fp.0,
                            tool_restrictions: tr.0,
                            enabled,
                            origin,
                        }
                    },
                )
                .collect();
            Ok(Response::new(pb::ListAgentSkillsResponse { skills }))
        }
        .await;
        record_metrics("list_agent_skills", started, result.is_ok());
        result
    }

    // G7 transcript source: list a thread's conversation in order. Org-scoped
    // via a JOIN on threads.org_id, so a caller can only read its own org's
    // conversation (the messages table has no org_id of its own).
    async fn list_conversation(
        &self,
        request: Request<pb::ListConversationRequest>,
    ) -> Result<Response<pb::ListConversationResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::ListConversationResponse>, Status> = async {
            let req = request.into_inner();
            if req.org_id.is_empty() || req.thread_id.is_empty() {
                return Err(Status::invalid_argument(
                    "org_id and thread_id are required",
                ));
            }
            let rows: Vec<(String, String)> = sqlx::query_as(
                "SELECT m.role, m.content
                 FROM messages m
                 JOIN threads t ON t.id = m.thread_id
                 WHERE m.thread_id = $1 AND t.org_id = $2
                 ORDER BY m.sequence",
            )
            .bind(&req.thread_id)
            .bind(&req.org_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                warn!(error = %e, "list_thread_messages failed");
                Status::internal(e.to_string())
            })?;
            let messages = rows
                .into_iter()
                .map(|(role, content)| pb::SessionMessage { role, content })
                .collect();
            Ok(Response::new(pb::ListConversationResponse { messages }))
        }
        .await;
        record_metrics("list_conversation", started, result.is_ok());
        result
    }

    async fn get_context_assembly(
        &self,
        request: Request<pb::GetContextAssemblyRequest>,
    ) -> Result<Response<pb::GetContextAssemblyResponse>, Status> {
        let started = Instant::now();
        let result = get_context_assembly_inner(self, request.into_inner()).await;
        record_metrics("get_context_assembly", started, result.is_ok());
        result
    }
}

const RETRIEVAL_BUDGET_FRACTION: u32 = 4;
const RETRIEVAL_DEFAULT_TOP_K: i32 = 10;
const GRAPH_CONTRADICTIONS_LIMIT: i32 = 5;
const KNOWLEDGE_DOC_LIMIT: usize = 3;
const KNOWLEDGE_UNITS_PER_DOC_LIMIT: usize = 4;
const PERSIST_RETRIEVAL_LIMIT: usize = 5;

/// Outcome of a retrieval pass — text segments ready for the assembler
/// plus the structured metadata callers need to enrich context further
/// (knowledge units), persist the evidence (`memory_index`), and emit
/// learning-loop events (`mp.v1.retrieval.{used,low_confidence}`).
pub(crate) struct RetrievalEvidence {
    pub segments: Vec<String>,
    /// Distinct `document_ids` returned by the retrieval call, ordered by
    /// rank. Capped at top-K so downstream knowledge enrichment doesn't
    /// blow out latency.
    pub document_ids: Vec<String>,
    /// True iff the retrieval call returned `low_confidence: true` —
    /// signals the corpus has a gap and may want a wiki proposal.
    pub low_confidence: bool,
    /// Source of the segments. Distinguishes "live Data Plane" from
    /// "fell back to `memory_index`" for telemetry and tests.
    pub source: RetrievalSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetrievalSource {
    /// Returned by `RetrievalService.Retrieve`.
    DataPlane,
    /// Returned from `memory_index` rows persisted by an earlier call —
    /// happens when the Data Plane client is unconfigured, the call
    /// failed, or the thread's `org_id` is missing.
    LocalFallback,
}

/// Map a Data Plane `RetrieveResponse` into `RetrievalEvidence`, preferring the
/// `context_pack` facts, then raw candidate text, falling back to `local_fallback`.
fn map_retrieve_response(
    inner: ret_pb::RetrieveResponse,
    local_fallback: &[String],
) -> RetrievalEvidence {
    let low_confidence = inner.low_confidence;
    let mut document_ids: Vec<String> = inner
        .candidates
        .iter()
        .map(|c| c.document_id.clone())
        .filter(|d| !d.is_empty())
        .collect();
    document_ids.dedup();

    let segments = if let Some(pack) = inner.context_pack {
        let facts: Vec<String> = pack
            .facts
            .into_iter()
            .map(|f| format!("[{}] ({}): {}", f.source_title, f.source_type, f.text))
            .collect();
        if facts.is_empty() {
            local_fallback.to_vec()
        } else {
            facts
        }
    } else if inner.candidates.is_empty() {
        local_fallback.to_vec()
    } else {
        inner.candidates.into_iter().map(|c| c.text).collect()
    };

    if segments.is_empty() {
        return RetrievalEvidence {
            segments: local_fallback.to_vec(),
            document_ids: Vec::new(),
            low_confidence: false,
            source: RetrievalSource::LocalFallback,
        };
    }

    RetrievalEvidence {
        segments,
        document_ids,
        low_confidence,
        source: RetrievalSource::DataPlane,
    }
}

impl SessionService {
    async fn fetch_retrieval_segments(
        &self,
        thread_id: &str,
        thread_messages: &[(String, String)],
        max_tokens: u32,
        local_fallback: &[String],
    ) -> RetrievalEvidence {
        let local = || RetrievalEvidence {
            segments: local_fallback.to_vec(),
            document_ids: Vec::new(),
            low_confidence: false,
            source: RetrievalSource::LocalFallback,
        };

        let mut client = match &self.retrieval_client {
            Some(c) => c.clone(),
            None => return local(),
        };

        let query = thread_messages
            .iter()
            .rev()
            .find(|(role, _)| role == "user")
            .map(|(_, content)| content.as_str());

        let query = match query {
            Some(q) if !q.is_empty() => q,
            _ => return local(),
        };

        let org_id: Option<String> =
            match sqlx::query_scalar("SELECT org_id FROM threads WHERE id = $1")
                .bind(thread_id)
                .fetch_optional(&self.pool)
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    warn!(error = %e, thread_id, "failed to look up org_id for retrieval");
                    return local();
                }
            };

        let org_id = match org_id {
            Some(id) if !id.is_empty() => id,
            _ => return local(),
        };

        let budget = if max_tokens > 0 {
            Some(i32::try_from(max_tokens / RETRIEVAL_BUDGET_FRACTION).unwrap_or(i32::MAX))
        } else {
            None
        };

        let retrieve_req = ret_pb::RetrieveRequest {
            org_id,
            query: query.to_owned(),
            filters: None,
            top_k: RETRIEVAL_DEFAULT_TOP_K,
            user_id: None,
            role: None,
            query_expansion: None,
            reranker_model: None,
            top_k_before_rerank: None,
            zdr_mode: None,
            context_budget_tokens: budget,
            context_format: Some("toon".to_owned()),
            agent_id: None,
        };

        match client.retrieve(retrieve_req).await {
            Ok(resp) => map_retrieve_response(resp.into_inner(), local_fallback),
            Err(e) => {
                warn!(error = %e, "retrieval service call failed, using local fallback");
                local()
            }
        }
    }

    /// Fetch knowledge-unit context for the top-N retrieval `doc_ids`.
    /// `KnowledgeService.GetKnowledgeUnits` returns chunks adjacent to
    /// the retrieved candidates — useful when the candidate text alone
    /// is too narrow (e.g. it's the answer span but the model needs the
    /// surrounding paragraph to interpret it). Calls are dispatched
    /// concurrently so latency is `~max(per_call)` not `sum`.
    async fn fetch_knowledge_segments(
        &self,
        thread_id: &str,
        document_ids: &[String],
    ) -> Vec<String> {
        let client = match &self.knowledge_client {
            Some(c) => c.clone(),
            None => return Vec::new(),
        };
        if document_ids.is_empty() {
            return Vec::new();
        }
        let org_id: Option<String> =
            match sqlx::query_scalar("SELECT org_id FROM threads WHERE id = $1")
                .bind(thread_id)
                .fetch_optional(&self.pool)
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    warn!(error = %e, thread_id, "failed to look up org_id for knowledge");
                    return Vec::new();
                }
            };
        let org_id = match org_id {
            Some(id) if !id.is_empty() => id,
            _ => return Vec::new(),
        };

        let take = document_ids.len().min(KNOWLEDGE_DOC_LIMIT);
        let futures = document_ids[..take].iter().map(|doc_id| {
            let mut c = client.clone();
            let req = know_pb::GetKnowledgeUnitsRequest {
                document_id: doc_id.clone(),
                org_id: org_id.clone(),
            };
            async move { c.get_knowledge_units(req).await }
        });
        let results = futures::future::join_all(futures).await;

        let mut out = Vec::new();
        for resp in results {
            let Ok(resp) = resp else { continue };
            let units = resp.into_inner().units;
            for u in units.into_iter().take(KNOWLEDGE_UNITS_PER_DOC_LIMIT) {
                if u.text.is_empty() {
                    continue;
                }
                out.push(format!(
                    "[doc:{} chunk:{}] {}",
                    u.document_id, u.chunk_index, u.text
                ));
            }
        }
        out
    }

    /// Persist the top retrieval segments to `memory_index` with topic
    /// `RETRIEVAL` so subsequent context-assembly calls on this thread
    /// can fall back to them when Data Plane is unreachable, and so the
    /// model "remembers" what it saw across calls instead of refetching.
    /// Bounded by `PERSIST_RETRIEVAL_LIMIT` to keep `memory_index` lean.
    /// Failures are logged and ignored — persistence is best-effort.
    async fn persist_retrieval_evidence(&self, thread_id: &str, segments: &[String]) {
        if segments.is_empty() {
            return;
        }
        let org_id: Option<String> =
            match sqlx::query_scalar("SELECT org_id FROM threads WHERE id = $1")
                .bind(thread_id)
                .fetch_optional(&self.pool)
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    warn!(error = %e, thread_id, "skip retrieval persist: org_id lookup failed");
                    return;
                }
            };
        let Some(org_id) = org_id.filter(|s| !s.is_empty()) else {
            return;
        };

        let take = segments.len().min(PERSIST_RETRIEVAL_LIMIT);
        for content in &segments[..take] {
            if content.is_empty() {
                continue;
            }
            let id = new_ulid();
            if let Err(e) = sqlx::query(
                "INSERT INTO memory_index (id, thread_id, topic, content, org_id) \
                 VALUES ($1, $2, 'RETRIEVAL', $3, $4)",
            )
            .bind(&id)
            .bind(thread_id)
            .bind(content)
            .bind(&org_id)
            .execute(&self.pool)
            .await
            {
                warn!(error = %e, "failed to persist retrieval evidence");
                return;
            }
        }
    }

    /// Fetch graph-aware context segments. Currently surfaces active contradictions
    /// for the thread's org so the model is aware of conflicting claims when reasoning
    /// over retrieval evidence. Returns an empty vec if the graph service is unconfigured
    /// or any call fails — graph context is opportunistic, never blocking.
    async fn fetch_graph_segments(&self, thread_id: &str) -> Vec<String> {
        let mut client = match &self.graph_client {
            Some(c) => c.clone(),
            None => return Vec::new(),
        };

        let org_id: Option<String> =
            match sqlx::query_scalar("SELECT org_id FROM threads WHERE id = $1")
                .bind(thread_id)
                .fetch_optional(&self.pool)
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    warn!(error = %e, thread_id, "failed to look up org_id for graph context");
                    return Vec::new();
                }
            };

        let org_id = match org_id {
            Some(id) if !id.is_empty() => id,
            _ => return Vec::new(),
        };

        let req = graph_pb::GetContradictionsRequest {
            org_id,
            entity_id: None,
            limit: GRAPH_CONTRADICTIONS_LIMIT,
            offset: 0,
        };

        match client.get_contradictions(req).await {
            Ok(resp) => resp
                .into_inner()
                .contradictions
                .into_iter()
                .map(|c| {
                    let prov = if c.provenance.is_empty() {
                        String::new()
                    } else {
                        format!(" [src: {}]", c.provenance)
                    };
                    format!("contradiction ({}): {}{}", c.status, c.text, prov)
                })
                .collect(),
            Err(e) => {
                warn!(error = %e, "graph contradictions call failed");
                Vec::new()
            }
        }
    }
}

pub(crate) struct AssemblyInputs {
    pub policy_id: String,
    pub workspace_id: String,
    pub agent_id: Option<String>,
    pub user_id: Option<String>,
    pub prompt_goal: Option<String>,
    pub thread_messages: Vec<(String, String)>,
    pub policy_segments: Vec<String>,
    pub workspace_segments: Vec<String>,
    pub agent_segments: Vec<String>,
    pub user_segments: Vec<String>,
    pub episodic_segments: Vec<String>,
    pub skill_index_segments: Vec<String>,
    pub skill_expansion_segments: Vec<String>,
    pub retrieval_segments: Vec<String>,
    pub knowledge_segments: Vec<String>,
    pub graph_segments: Vec<String>,
    pub run_id: String,
    pub max_tokens: u32,
}

fn push_all(
    kind: &str,
    items: &[String],
    budget: u32,
    segs: &mut Vec<pb::ContextSegment>,
    total: &mut u32,
) -> bool {
    for item in items {
        if !try_push(kind, item.clone(), budget, segs, total) {
            return false;
        }
    }
    true
}

/// Push one assembly tier: prefer explicit `segments`; otherwise fall back to a
/// single id-based segment (`{kind}:{id}`) when `fallback_id` is non-empty.
/// Returns `false` when the budget was exhausted and assembly should stop.
fn push_tier(
    kind: &str,
    segments: &[String],
    fallback_id: Option<&str>,
    budget: u32,
    segs: &mut Vec<pb::ContextSegment>,
    total: &mut u32,
) -> bool {
    if !segments.is_empty() {
        return push_all(kind, segments, budget, segs, total);
    }
    match fallback_id {
        Some(id) if !id.is_empty() => try_push(kind, format!("{kind}:{id}"), budget, segs, total),
        _ => true,
    }
}

fn try_push(
    kind: &str,
    content: String,
    budget: u32,
    segs: &mut Vec<pb::ContextSegment>,
    total: &mut u32,
) -> bool {
    let len = u32::try_from(content.len()).unwrap_or(u32::MAX);
    let est = len.saturating_div(4);
    if budget != 0 && total.saturating_add(est) > budget {
        return false;
    }
    *total = total.saturating_add(est);
    segs.push(pb::ContextSegment {
        kind: kind.to_owned(),
        content,
        estimated_tokens: est,
    });
    true
}

pub(crate) fn assemble_segments(inputs: &AssemblyInputs) -> (Vec<pb::ContextSegment>, u32) {
    let mut segments: Vec<pb::ContextSegment> = Vec::new();
    let mut total: u32 = 0;
    let budget = inputs.max_tokens;

    if !push_tier(
        "policy",
        &inputs.policy_segments,
        Some(&inputs.policy_id),
        budget,
        &mut segments,
        &mut total,
    ) {
        return (segments, total);
    }
    if !push_tier(
        "workspace",
        &inputs.workspace_segments,
        Some(&inputs.workspace_id),
        budget,
        &mut segments,
        &mut total,
    ) {
        return (segments, total);
    }
    if !push_tier(
        "agent",
        &inputs.agent_segments,
        inputs.agent_id.as_deref(),
        budget,
        &mut segments,
        &mut total,
    ) {
        return (segments, total);
    }
    if !push_tier(
        "user",
        &inputs.user_segments,
        inputs.user_id.as_deref(),
        budget,
        &mut segments,
        &mut total,
    ) {
        return (segments, total);
    }

    for (role, content) in &inputs.thread_messages {
        if !try_push(
            "thread",
            format!("{role}: {content}"),
            budget,
            &mut segments,
            &mut total,
        ) {
            return (segments, total);
        }
    }

    let tail_tiers: [(&str, &[String]); 6] = [
        ("episodic", &inputs.episodic_segments),
        ("skill_index", &inputs.skill_index_segments),
        ("skill_expansion", &inputs.skill_expansion_segments),
        ("retrieval", &inputs.retrieval_segments),
        ("knowledge", &inputs.knowledge_segments),
        ("graph", &inputs.graph_segments),
    ];
    for (kind, tier) in tail_tiers {
        if !push_all(kind, tier, budget, &mut segments, &mut total) {
            return (segments, total);
        }
    }

    let prompt = match &inputs.prompt_goal {
        Some(g) if !g.is_empty() => g.clone(),
        _ => format!("run:{}", inputs.run_id),
    };
    let _ = try_push("prompt", prompt, budget, &mut segments, &mut total);

    (segments, total)
}

fn event_type_to_i32(s: &str) -> i32 {
    match s {
        "SESSION_START" => 1,
        "SESSION_END" => 2,
        "RUN_STARTED" => 90,
        "RUN_COMPLETED" => 91,
        "RUN_FAILED" => 92,
        "STEP_COMPLETED" => 101,
        "CHECKPOINT_SAVED" => 110,
        "THREAD_CREATED" => 120,
        "MESSAGE_APPENDED" => 121,
        _ => 0,
    }
}

/// Start the gRPC server on :9091.
///
/// # Errors
///
/// Returns an error if the server fails to bind.
pub async fn serve(
    pool: PgPool,
    events_tx: tokio::sync::broadcast::Sender<mp_contracts::model_plane::v1::OrchestrationEvent>,
) -> anyhow::Result<()> {
    let addr = "0.0.0.0:9091".parse()?;
    info!("gRPC listening on :9091");

    let retrieval_addr = std::env::var("DATAPLANE_RETRIEVAL_ADDR")
        .unwrap_or_else(|_| "http://localhost:50052".into());

    let retrieval_client = match Channel::from_shared(retrieval_addr.clone()) {
        Ok(ep) => {
            let ch = ep.connect_lazy();
            info!(addr = %retrieval_addr, "retrieval client configured (lazy connect)");
            Some(RetrievalServiceClient::new(ch))
        }
        Err(e) => {
            warn!(error = %e, "invalid DATAPLANE_RETRIEVAL_ADDR, skipping retrieval client");
            None
        }
    };

    let graph_addr =
        std::env::var("DATAPLANE_GRAPH_ADDR").unwrap_or_else(|_| "http://localhost:50053".into());

    let graph_client = match Channel::from_shared(graph_addr.clone()) {
        Ok(ep) => {
            let ch = ep.connect_lazy();
            info!(addr = %graph_addr, "graph client configured (lazy connect)");
            Some(GraphServiceClient::new(ch))
        }
        Err(e) => {
            warn!(error = %e, "invalid DATAPLANE_GRAPH_ADDR, skipping graph client");
            None
        }
    };

    // Knowledge service typically lives on the same port as retrieval
    // (Data Plane retrieval-engine-rs serves both); separate env var so
    // operators can override if they're split.
    let knowledge_addr =
        std::env::var("DATAPLANE_KNOWLEDGE_ADDR").unwrap_or_else(|_| retrieval_addr.clone());

    let knowledge_client = match Channel::from_shared(knowledge_addr.clone()) {
        Ok(ep) => {
            let ch = ep.connect_lazy();
            info!(addr = %knowledge_addr, "knowledge client configured (lazy connect)");
            Some(KnowledgeServiceClient::new(ch))
        }
        Err(e) => {
            warn!(error = %e, "invalid DATAPLANE_KNOWLEDGE_ADDR, skipping knowledge client");
            None
        }
    };

    let orchestration =
        crate::orchestration_grpc::OrchestrationGrpc::new_from_env(pool.clone(), events_tx)
            .await
            .into_server();

    // Wave 7 — fine-tuning job state machine. Owns the `finetune_jobs` table
    // in this same Postgres. Provider HTTP (Azure OpenAI) lives in the gateway.
    let finetune = crate::finetune_grpc::FinetuneJobsService::new(pool.clone()).into_server();

    tonic::transport::Server::builder()
        .add_service(SessionCoreServer::new(SessionService {
            pool,
            retrieval_client,
            graph_client,
            knowledge_client,
        }))
        .add_service(orchestration)
        .add_service(finetune)
        .serve(addr)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{assemble_segments, AssemblyInputs};

    fn base() -> AssemblyInputs {
        AssemblyInputs {
            policy_id: String::new(),
            workspace_id: String::new(),
            agent_id: None,
            user_id: None,
            prompt_goal: None,
            thread_messages: vec![],
            policy_segments: vec![],
            workspace_segments: vec![],
            agent_segments: vec![],
            user_segments: vec![],
            episodic_segments: vec![],
            skill_index_segments: vec![],
            skill_expansion_segments: vec![],
            retrieval_segments: vec![],
            knowledge_segments: vec![],
            graph_segments: vec![],
            run_id: String::new(),
            max_tokens: 0,
        }
    }

    #[test]
    fn ordering_canonical() {
        let mut i = base();
        i.policy_id = "p1".to_owned();
        i.workspace_id = "w1".to_owned();
        i.agent_id = Some("a1".to_owned());
        i.user_id = Some("u1".to_owned());
        i.thread_messages = vec![("user".to_owned(), "hi".to_owned())];
        i.episodic_segments = vec!["memo".to_owned()];
        i.prompt_goal = Some("goal".to_owned());
        let (segs, _) = assemble_segments(&i);
        let kinds: Vec<&str> = segs.iter().map(|s| s.kind.as_str()).collect();
        assert_eq!(
            kinds,
            vec![
                "policy",
                "workspace",
                "agent",
                "user",
                "thread",
                "episodic",
                "prompt"
            ]
        );
    }

    #[test]
    fn budget_truncates() {
        let mut i = base();
        i.max_tokens = 3;
        i.policy_id = "short".to_owned();
        i.workspace_id = "this-is-a-very-long-workspace-id-that-exceeds-budget".to_owned();
        i.prompt_goal = Some("goal".to_owned());
        let (segs, total) = assemble_segments(&i);
        assert!(total <= 3);
        assert!(segs.iter().all(|s| s.kind != "workspace"));
    }

    #[test]
    fn empty_ids_skip() {
        let (segs, _) = assemble_segments(&base());
        let kinds: Vec<&str> = segs.iter().map(|s| s.kind.as_str()).collect();
        assert_eq!(kinds, vec!["prompt"]);
    }

    #[test]
    fn prompt_fallback() {
        let mut i = base();
        i.run_id = "r1".to_owned();
        let (segs, _) = assemble_segments(&i);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].kind, "prompt");
        assert_eq!(segs[0].content, "run:r1");
    }

    #[test]
    fn materialized_context_sources_take_priority_over_identifier_fallbacks() {
        let mut i = base();
        i.policy_id = "policy-id".to_owned();
        i.workspace_id = "workspace-id".to_owned();
        i.agent_id = Some("agent-id".to_owned());
        i.user_id = Some("user-id".to_owned());
        i.policy_segments = vec!["policy body".to_owned()];
        i.workspace_segments = vec!["workspace body".to_owned()];
        i.agent_segments = vec!["agent body".to_owned()];
        i.user_segments = vec!["user body".to_owned()];
        i.episodic_segments = vec!["memory body".to_owned()];
        i.skill_index_segments = vec!["skill index body".to_owned()];
        i.skill_expansion_segments = vec!["skill expansion body".to_owned()];
        i.retrieval_segments = vec!["retrieval body".to_owned()];
        i.prompt_goal = Some("prompt body".to_owned());

        let (segs, _) = assemble_segments(&i);
        let contents: Vec<(&str, &str)> = segs
            .iter()
            .map(|seg| (seg.kind.as_str(), seg.content.as_str()))
            .collect();

        assert_eq!(
            contents,
            vec![
                ("policy", "policy body"),
                ("workspace", "workspace body"),
                ("agent", "agent body"),
                ("user", "user body"),
                ("episodic", "memory body"),
                ("skill_index", "skill index body"),
                ("skill_expansion", "skill expansion body"),
                ("retrieval", "retrieval body"),
                ("prompt", "prompt body"),
            ]
        );
    }

    #[test]
    fn full_context_order_includes_all_plan_segments() {
        let mut i = base();
        i.policy_segments = vec!["policy".to_owned()];
        i.workspace_segments = vec!["workspace".to_owned()];
        i.agent_segments = vec!["agent".to_owned()];
        i.user_segments = vec!["user".to_owned()];
        i.thread_messages = vec![("user".to_owned(), "thread".to_owned())];
        i.episodic_segments = vec!["episodic".to_owned()];
        i.skill_index_segments = vec!["skill-index".to_owned()];
        i.skill_expansion_segments = vec!["skill-expansion".to_owned()];
        i.retrieval_segments = vec!["retrieval".to_owned()];
        i.knowledge_segments = vec!["knowledge".to_owned()];
        i.graph_segments = vec!["graph".to_owned()];
        i.prompt_goal = Some("prompt".to_owned());

        let (segs, _) = assemble_segments(&i);
        let kinds: Vec<&str> = segs.iter().map(|s| s.kind.as_str()).collect();
        assert_eq!(
            kinds,
            vec![
                "policy",
                "workspace",
                "agent",
                "user",
                "thread",
                "episodic",
                "skill_index",
                "skill_expansion",
                "retrieval",
                "knowledge",
                "graph",
                "prompt",
            ]
        );
    }

    #[test]
    fn knowledge_segments_appear_after_retrieval() {
        // Regression for the context-retention pass: knowledge units fetched
        // from `KnowledgeService.GetKnowledgeUnits` must render between the
        // retrieval evidence (raw candidates) and the graph segment
        // (contradictions), giving the model: evidence → adjacent
        // chunks for that evidence → graph-level constraints.
        let mut i = base();
        i.retrieval_segments = vec!["doc1 hit".to_owned()];
        i.knowledge_segments = vec!["[doc:a chunk:0] surrounding".to_owned()];
        i.graph_segments = vec!["contradiction (active): X".to_owned()];

        let (segs, _) = assemble_segments(&i);
        let kinds: Vec<&str> = segs.iter().map(|s| s.kind.as_str()).collect();
        let pos = |k| kinds.iter().position(|x| *x == k).unwrap();
        assert!(pos("retrieval") < pos("knowledge"));
        assert!(pos("knowledge") < pos("graph"));
    }

    #[test]
    fn empty_knowledge_segments_skip_cleanly() {
        // No knowledge_client configured (or no doc_ids in retrieval) →
        // empty vec. Assembly must not insert a placeholder "knowledge"
        // segment in that case.
        let mut i = base();
        i.retrieval_segments = vec!["evidence".to_owned()];
        i.knowledge_segments = vec![];

        let (segs, _) = assemble_segments(&i);
        let kinds: Vec<&str> = segs.iter().map(|s| s.kind.as_str()).collect();
        assert!(!kinds.contains(&"knowledge"));
        assert!(kinds.contains(&"retrieval"));
    }

    // ---------------------------------------------------------------------
    // PR-8: Context assembly SLO — latency + token-budget compliance.
    // ---------------------------------------------------------------------
    //
    // Wires `mp-slo::harness::context_assembly` against the real
    // `assemble_segments` function with a realistic fixture (hundreds of
    // memory segments + long thread history).
    //
    // Two invariants enforced:
    //   1. Latency p95 < `context_assembly_latency` SLO target (default 2s).
    //   2. Every assembly result's token total ≤ requested `max_tokens`
    //      (operator-visible budget compliance).

    fn large_fixture() -> AssemblyInputs {
        let mut i = base();
        i.policy_id = "policy-slo".to_owned();
        i.workspace_id = "workspace-slo".to_owned();
        i.agent_id = Some("agent-slo".to_owned());
        i.user_id = Some("user-slo".to_owned());
        i.run_id = "run-slo".to_owned();
        i.max_tokens = 8_000;
        i.prompt_goal = Some("synthesize recent work and propose next steps".repeat(4));

        // 32 turns of thread history, each ~300 chars.
        i.thread_messages = (0..32)
            .map(|n| {
                let role = if n % 2 == 0 { "user" } else { "assistant" };
                (
                    role.to_owned(),
                    format!("[turn {n}] {}", "lorem ipsum ".repeat(25)),
                )
            })
            .collect();

        // Dense memory sprinkled across kinds.
        let seg = |prefix: &str, n: usize| -> Vec<String> {
            (0..n)
                .map(|i| format!("{prefix}-{i:03}: {}", "data ".repeat(20)))
                .collect()
        };
        i.policy_segments = seg("policy", 8);
        i.workspace_segments = seg("workspace", 16);
        i.agent_segments = seg("agent", 16);
        i.user_segments = seg("user", 16);
        i.episodic_segments = seg("episodic", 32);
        i.skill_index_segments = seg("skill-idx", 16);
        i.skill_expansion_segments = seg("skill-exp", 16);
        i.retrieval_segments = seg("retrieval", 32);
        i
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn context_assembly_meets_slo() {
        use mp_slo::{defaults, harness};
        type BoxedErr = Box<dyn std::error::Error + Send + Sync>;

        let iterations = 50usize;
        let fixture = large_fixture();
        let budget = fixture.max_tokens;

        let (recorder, tokens) = harness::context_assembly(iterations, |_| {
            let inputs = large_fixture();
            async move {
                let (_segs, total) = assemble_segments(&inputs);
                Ok::<usize, BoxedErr>(usize::try_from(total).unwrap_or(0))
            }
        })
        .await
        .expect("context_assembly harness ran");

        // Latency SLO.
        let sorted = recorder.sorted_durations();
        let verdict = defaults::context_assembly_latency().evaluate(&sorted);
        assert!(!verdict.breached, "{}", verdict.summary());
        assert_eq!(recorder.samples().len(), iterations);

        // Token-budget compliance. Every invocation's observed token count
        // must be ≤ requested max_tokens.
        for (idx, &t) in tokens.iter().enumerate() {
            assert!(
                u32::try_from(t).unwrap_or(u32::MAX) <= budget,
                "iteration {idx}: estimated_tokens {t} exceeds budget {budget}"
            );
        }

        // Sanity: the assembler actually produced something under this budget.
        let max_observed = tokens.iter().copied().max().unwrap_or(0);
        assert!(
            max_observed > 0,
            "expected non-empty assembly; fixture may be wrong"
        );
    }

    // P3 SetRunMode — durable run.mode, invalid-mode rejection, and per-org
    // isolation (a foreign org's UPDATE matches no row → not_found, no mutation).
    async fn assert_set_run_mode_isolation(
        svc: &super::SessionService,
        pool: &sqlx::PgPool,
        run_id: &str,
        org: &str,
        sfx: u32,
    ) {
        use mp_contracts::model_plane::v1 as pb;
        use mp_contracts::model_plane::v1::session_core_server::SessionCore;
        use tonic::Request;

        let resp = svc
            .set_run_mode(Request::new(pb::SetRunModeRequest {
                run_id: run_id.to_owned(),
                mode: "plan".into(),
                org_id: org.to_owned(),
            }))
            .await
            .expect("set_run_mode")
            .into_inner();
        assert_eq!(resp.mode, "plan");
        assert!(svc
            .set_run_mode(Request::new(pb::SetRunModeRequest {
                run_id: run_id.to_owned(),
                mode: "bogus".into(),
                org_id: org.to_owned(),
            }))
            .await
            .is_err());

        let cross = svc
            .set_run_mode(Request::new(pb::SetRunModeRequest {
                run_id: run_id.to_owned(),
                mode: "execute".into(),
                org_id: format!("attacker-{sfx}"),
            }))
            .await;
        assert_eq!(
            cross.unwrap_err().code(),
            tonic::Code::NotFound,
            "cross-org set_run_mode must be rejected (per-org isolation)"
        );
        let persisted: (String,) = sqlx::query_as("SELECT mode FROM runs WHERE id = $1")
            .bind(run_id)
            .fetch_one(pool)
            .await
            .expect("read mode");
        assert_eq!(
            persisted.0, "plan",
            "cross-org attempt must not mutate the run"
        );
    }

    // G7 — fresh background_review insert succeeds; a background_review overwrite
    // of an existing user skill must be skipped (protected).
    async fn assert_upsert_skill_guards(svc: &super::SessionService, org: &str) {
        use mp_contracts::model_plane::v1 as pb;
        use mp_contracts::model_plane::v1::session_core_server::SessionCore;
        use tonic::Request;

        let skill = |name: &str, content: &str, origin: &str| pb::UpsertAgentSkillRequest {
            org_id: org.to_owned(),
            name: name.into(),
            description: "d".into(),
            content: content.into(),
            trigger_keywords: vec![],
            trigger_file_patterns: vec![],
            tool_restrictions: vec![],
            enabled: true,
            origin: origin.into(),
        };

        let r = svc
            .upsert_agent_skill(Request::new(skill("Cache", "c1", "background_review")))
            .await
            .expect("upsert")
            .into_inner();
        assert!(r.created && !r.skipped_protected);

        svc.upsert_agent_skill(Request::new(skill("Runbook", "human", "user")))
            .await
            .expect("user skill");
        let blocked = svc
            .upsert_agent_skill(Request::new(skill(
                "Runbook",
                "MACHINE",
                "background_review",
            )))
            .await
            .expect("guarded upsert")
            .into_inner();
        assert!(
            blocked.skipped_protected,
            "background_review must not overwrite a user skill"
        );
    }

    // Handler-level integration test against a REAL Postgres. Exercises the
    // actual gRPC handler code (validation + SQL + mapping), not just raw SQL.
    // #[ignore]d so plain `cargo test` (no DB) skips it; run explicitly with a
    // DB:  DATABASE_URL=… cargo test -p session-core --bin session-core
    //        set_run_mode_and_upsert_skill_against_real_pg -- --ignored
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
    async fn set_run_mode_and_upsert_skill_against_real_pg() {
        use super::SessionService;

        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");

        let sfx = std::process::id();
        let (thread_id, run_id, org) =
            (format!("t-{sfx}"), format!("r-{sfx}"), format!("org-{sfx}"));
        sqlx::query("INSERT INTO threads (id,session_key,org_id,user_id) VALUES ($1,$1,$2,'u1')")
            .bind(&thread_id)
            .bind(&org)
            .execute(&pool)
            .await
            .expect("seed thread");
        sqlx::query(
            "INSERT INTO runs (id,thread_id,goal,org_id,user_id) VALUES ($1,$2,'g',$3,'u1')",
        )
        .bind(&run_id)
        .bind(&thread_id)
        .bind(&org)
        .execute(&pool)
        .await
        .expect("seed run");

        let svc = SessionService {
            pool: pool.clone(),
            retrieval_client: None,
            graph_client: None,
            knowledge_client: None,
        };

        assert_set_run_mode_isolation(&svc, &pool, &run_id, &org, sfx).await;
        assert_upsert_skill_guards(&svc, &org).await;

        // G7 read path: list_agent_skills returns this org's skills (Cache +
        // Runbook), proving the learned-skill last mile is readable.
        let listed = svc
            .list_agent_skills(Request::new(pb::ListAgentSkillsRequest {
                org_id: org.clone(),
                enabled_only: true,
            }))
            .await
            .expect("list_agent_skills")
            .into_inner();
        let names: std::collections::BTreeSet<&str> =
            listed.skills.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains("Cache") && names.contains("Runbook"),
            "list must return the upserted skills, got {names:?}"
        );
        // Per-org isolation: another org sees none of them.
        let other = svc
            .list_agent_skills(Request::new(pb::ListAgentSkillsRequest {
                org_id: format!("other-{sfx}"),
                enabled_only: true,
            }))
            .await
            .expect("list other org")
            .into_inner();
        assert!(
            other.skills.is_empty(),
            "another org must not see this org's skills"
        );

        // G7 transcript source: list_thread_messages returns the conversation
        // in sequence order, org-scoped via the owning thread.
        for (i, (role, content)) in [("user", "hello"), ("assistant", "hi there")]
            .iter()
            .enumerate()
        {
            sqlx::query("INSERT INTO messages (id, thread_id, role, content) VALUES ($1,$2,$3,$4)")
                .bind(format!("m-{sfx}-{i}"))
                .bind(&thread_id)
                .bind(*role)
                .bind(*content)
                .execute(&pool)
                .await
                .expect("seed message");
        }
        let convo = svc
            .list_conversation(Request::new(pb::ListConversationRequest {
                org_id: org.clone(),
                thread_id: thread_id.clone(),
            }))
            .await
            .expect("list_thread_messages")
            .into_inner();
        assert_eq!(convo.messages.len(), 2, "expected 2 messages");
        assert_eq!(convo.messages[0].role, "user");
        assert_eq!(convo.messages[0].content, "hello");
        assert_eq!(convo.messages[1].role, "assistant", "ordered by sequence");
        // Per-org isolation: another org cannot read this thread's conversation.
        let cross_convo = svc
            .list_conversation(Request::new(pb::ListConversationRequest {
                org_id: format!("attacker-{sfx}"),
                thread_id: thread_id.clone(),
            }))
            .await
            .expect("list other-org convo")
            .into_inner();
        assert!(
            cross_convo.messages.is_empty(),
            "another org must not read this thread's messages"
        );

        // cleanup (messages first — FK to threads)
        sqlx::query("DELETE FROM messages WHERE thread_id=$1")
            .bind(&thread_id)
            .execute(&pool)
            .await
            .ok();
        for q in [
            "DELETE FROM agent_skills WHERE org_id=$1",
            "DELETE FROM approvals WHERE org_id=$1",
            "DELETE FROM runs WHERE org_id=$1",
            "DELETE FROM threads WHERE org_id=$1",
        ] {
            sqlx::query(q).bind(&org).execute(&pool).await.ok();
        }
    }

    // True multi-service round-trip: a real SessionCoreClient calls a real
    // session-core gRPC server over TCP, persisting to real Postgres — the
    // EXACT client→wire→handler→DB path the gateway's plan-mode write-through
    // uses (gateway calls session_client.set_run_mode the same way). Verifies
    // the deployed transport + handler + durability together.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
    async fn grpc_set_run_mode_round_trip_over_the_wire() {
        use super::SessionService;
        use mp_contracts::model_plane::v1 as pb;
        use mp_contracts::model_plane::v1::session_core_client::SessionCoreClient;
        use mp_contracts::model_plane::v1::session_core_server::SessionCoreServer;
        use std::time::Duration;
        use tonic::transport::Server;

        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");

        let sfx = std::process::id();
        let (thread_id, run_id, org) = (
            format!("e2e-t-{sfx}"),
            format!("e2e-r-{sfx}"),
            format!("e2e-org-{sfx}"),
        );
        sqlx::query("INSERT INTO threads (id,session_key,org_id,user_id) VALUES ($1,$1,$2,'u1')")
            .bind(&thread_id)
            .bind(&org)
            .execute(&pool)
            .await
            .expect("seed thread");
        sqlx::query(
            "INSERT INTO runs (id,thread_id,goal,org_id,user_id) VALUES ($1,$2,'g',$3,'u1')",
        )
        .bind(&run_id)
        .bind(&thread_id)
        .bind(&org)
        .execute(&pool)
        .await
        .expect("seed run");

        let svc = SessionService {
            pool: pool.clone(),
            retrieval_client: None,
            graph_client: None,
            knowledge_client: None,
        };
        let addr: std::net::SocketAddr = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        tokio::spawn(async move {
            Server::builder()
                .add_service(SessionCoreServer::new(svc))
                .serve(addr)
                .await
                .ok();
        });

        // Connect with retry while the server binds.
        let mut client = None;
        for _ in 0..30 {
            if let Ok(c) = SessionCoreClient::connect(format!("http://{addr}")).await {
                client = Some(c);
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let mut client = client.expect("connect to session-core over the wire");

        // Over-the-wire call (the gateway's write-through mechanism).
        let resp = client
            .set_run_mode(pb::SetRunModeRequest {
                run_id: run_id.clone(),
                mode: "plan".into(),
                org_id: org.clone(),
            })
            .await
            .expect("set_run_mode rpc over wire")
            .into_inner();
        assert_eq!(resp.mode, "plan");

        // And it actually persisted in Postgres.
        let (mode,): (String,) = sqlx::query_as("SELECT mode FROM runs WHERE id = $1")
            .bind(&run_id)
            .fetch_one(&pool)
            .await
            .expect("read back run mode");
        assert_eq!(
            mode, "plan",
            "run mode must be durable after the wire round-trip"
        );

        for q in [
            "DELETE FROM runs WHERE org_id=$1",
            "DELETE FROM threads WHERE org_id=$1",
        ] {
            sqlx::query(q).bind(&org).execute(&pool).await.ok();
        }
    }
}
