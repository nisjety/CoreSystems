//! Wave 10f — trajectory recording + Atropos-format export.
//!
//! In-memory ring-buffer of completed-run trajectories keyed by
//! `trajectory_id`. Each `RecordTrajectory` call also publishes the
//! trajectory onto `agents.trajectory.recorded` so external pipelines
//! (a separate Atropos exporter, an analytics aggregator) can stream
//! them without coupling to the gateway.
//!
//! Why in-memory instead of postgres (v2 used a Postgres table):
//!   - Trajectories are large; persisting every run blocks the hot
//!     path even when wrapped in `ensure_future`.
//!   - The gateway is meant to stay stateless. Long-term retention
//!     belongs to an analytics service that subscribes to the NATS
//!     subject. This module is a thin "tail of the river" — a
//!     bounded look-back for debugging + on-demand export.
//!
//! Size budget: at most `MAX_TRAJECTORIES_PER_ORG` per org. Older
//! entries are evicted FIFO. Operators wanting durable history should
//! consume the NATS subject + ship to S3/MinIO.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use dashmap::DashMap;
use mp_events::publisher::EventPublisher;
use mp_ids::new_ulid;
use tokio::sync::RwLock;
use tonic::Status;
use tracing::warn;

use mp_contracts::model_plane::v1::{
    ExportTrajectoriesRequest, ExportTrajectoriesResponse, ListTrajectoriesRequest,
    ListTrajectoriesResponse, RecordTrajectoryRequest, RecordTrajectoryResponse, Trajectory,
};

/// Max trajectories retained per org. Tuned to keep total memory
/// under ~50 MB even with verbose actions.
const MAX_TRAJECTORIES_PER_ORG: usize = 200;

/// Maximum bytes a single export will inline. Beyond this we refuse
/// rather than allocate an unbounded gRPC response; callers must page.
const MAX_EXPORT_BYTES: usize = 10 * 1024 * 1024;

const DEFAULT_LIST_LIMIT: i32 = 100;
const MAX_LIST_LIMIT: i32 = 500;

/// Per-org ring buffer of trajectory IDs in insertion order, plus a
/// global ID → trajectory map. Two indexes so listing is cheap and
/// FIFO eviction is O(1).
#[derive(Clone, Default, Debug)]
pub struct TrajectoryStore {
    by_id: Arc<DashMap<String, Trajectory>>,
    per_org: Arc<RwLock<DashMap<String, VecDeque<String>>>>,
}

impl TrajectoryStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Store the trajectory, evicting the oldest per-org entry when
    /// at capacity. Returns the assigned `trajectory_id`.
    async fn store(&self, mut t: Trajectory) -> String {
        if t.trajectory_id.is_empty() {
            t.trajectory_id = new_ulid();
        }
        if t.created_at_unix == 0 {
            t.created_at_unix = now_unix();
        }

        let id = t.trajectory_id.clone();
        let org = t.org_id.clone();
        self.by_id.insert(id.clone(), t);

        // Update per-org ring; evict the oldest when full.
        let org_map = self.per_org.write().await;
        let mut q = org_map.entry(org).or_insert_with(VecDeque::new);
        q.push_back(id.clone());
        if q.len() > MAX_TRAJECTORIES_PER_ORG {
            if let Some(evict_id) = q.pop_front() {
                drop(q); // release the entry guard before mutating by_id
                self.by_id.remove(&evict_id);
            }
        }
        id
    }

    async fn list(
        &self,
        org_id: &str,
        outcome_filter: &str,
        pattern_filter: &str,
        since_unix: i64,
        limit: usize,
    ) -> (Vec<Trajectory>, i32) {
        let org_map = self.per_org.read().await;
        let ids = match org_map.get(org_id) {
            Some(q) => q.value().iter().cloned().collect::<Vec<_>>(),
            None => return (Vec::new(), 0),
        };
        drop(org_map);

        let mut out: Vec<Trajectory> = ids
            .iter()
            .filter_map(|id| self.by_id.get(id).map(|t| t.value().clone()))
            .filter(|t| outcome_filter.is_empty() || t.outcome == outcome_filter)
            .filter(|t| pattern_filter.is_empty() || t.task_pattern == pattern_filter)
            .filter(|t| since_unix == 0 || t.created_at_unix >= since_unix)
            .collect();

        // Newest-first.
        out.sort_by(|a, b| b.created_at_unix.cmp(&a.created_at_unix));
        let total = i32::try_from(out.len()).unwrap_or(i32::MAX);
        out.truncate(limit);
        (out, total)
    }

    async fn export(
        &self,
        org_id: &str,
        since_unix: i64,
        until_unix: i64,
    ) -> (Vec<Trajectory>, i32) {
        let until = if until_unix == 0 {
            i64::MAX
        } else {
            until_unix
        };
        let org_map = self.per_org.read().await;
        let ids = match org_map.get(org_id) {
            Some(q) => q.value().iter().cloned().collect::<Vec<_>>(),
            None => return (Vec::new(), 0),
        };
        drop(org_map);

        let mut out: Vec<Trajectory> = ids
            .iter()
            .filter_map(|id| self.by_id.get(id).map(|t| t.value().clone()))
            .filter(|t| {
                (since_unix == 0 || t.created_at_unix >= since_unix) && t.created_at_unix < until
            })
            .collect();
        out.sort_by(|a, b| a.created_at_unix.cmp(&b.created_at_unix));
        let total = i32::try_from(out.len()).unwrap_or(i32::MAX);
        (out, total)
    }
}

/// Records an agent trajectory in the store and best-effort publishes an event.
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.trajectory` is absent or its `org_id`
/// or `run_id` is empty. Event-publish failures are reported via the response's
/// `published` flag, not as an `Err`.
pub async fn handle_record_trajectory<P: EventPublisher>(
    store: &TrajectoryStore,
    publisher: &P,
    req: RecordTrajectoryRequest,
) -> Result<RecordTrajectoryResponse, Status> {
    let trajectory = req
        .trajectory
        .ok_or_else(|| Status::invalid_argument("trajectory is required"))?;
    if trajectory.org_id.is_empty() {
        return Err(Status::invalid_argument("trajectory.org_id is required"));
    }
    if trajectory.run_id.is_empty() {
        return Err(Status::invalid_argument("trajectory.run_id is required"));
    }

    let trajectory_id = store.store(trajectory.clone()).await;
    let mut stored = trajectory;
    stored.trajectory_id = trajectory_id.clone();

    // Best-effort publish for external aggregation.
    let envelope = mp_events::envelope::Envelope {
        event_id: new_ulid(),
        event_type: "TRAJECTORY_RECORDED".to_string(),
        schema_version: 1,
        ts: Utc::now(),
        producer: "model-gateway".to_string(),
        correlation_id: req.request_id.clone(),
        causation_id: String::new(),
        idempotency_key: trajectory_id.clone(),
        org_id: stored.org_id.clone(),
        user_id: "agent".to_string(),
        resource_ref: format!("trajectory/{trajectory_id}"),
        payload: trajectory_to_payload(&stored),
        zdr: false,
    };
    let was_published = match publisher
        .publish("agents.trajectory.recorded", &envelope)
        .await
    {
        Ok(()) => true,
        Err(e) => {
            warn!(error = %e, "trajectory publish failed (in-memory store still updated)");
            false
        }
    };

    Ok(RecordTrajectoryResponse {
        request_id: req.request_id,
        trajectory_id,
        published: was_published,
    })
}

/// Lists recorded trajectories for an org with optional filters.
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.org_id` is empty.
pub async fn handle_list_trajectories(
    store: &TrajectoryStore,
    req: ListTrajectoriesRequest,
) -> Result<ListTrajectoriesResponse, Status> {
    if req.org_id.is_empty() {
        return Err(Status::invalid_argument("org_id is required"));
    }
    let limit = usize::try_from(if req.limit <= 0 {
        DEFAULT_LIST_LIMIT
    } else {
        req.limit.min(MAX_LIST_LIMIT)
    })
    .unwrap_or(0);

    let (trajectories, total) = store
        .list(
            &req.org_id,
            &req.outcome_filter,
            &req.pattern_filter,
            req.since_unix,
            limit,
        )
        .await;

    Ok(ListTrajectoriesResponse {
        request_id: req.request_id,
        trajectories,
        total,
    })
}

/// Exports trajectories for an org (e.g. for offline learning).
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.org_id` is empty.
pub async fn handle_export_trajectories(
    store: &TrajectoryStore,
    req: ExportTrajectoriesRequest,
) -> Result<ExportTrajectoriesResponse, Status> {
    if req.org_id.is_empty() {
        return Err(Status::invalid_argument("org_id is required"));
    }
    let (trajectories, _) = store
        .export(&req.org_id, req.since_unix, req.until_unix)
        .await;

    // JSONL encoding — one line per trajectory.
    let mut buf = Vec::with_capacity(trajectories.len() * 512);
    for t in &trajectories {
        let line = serde_json::to_vec(&trajectory_to_payload(t))
            .map_err(|e| Status::internal(format!("jsonl encode: {e}")))?;
        if buf.len() + line.len() + 1 > MAX_EXPORT_BYTES {
            return Err(Status::resource_exhausted(format!(
                "export exceeds {MAX_EXPORT_BYTES} bytes; narrow the time window"
            )));
        }
        buf.extend_from_slice(&line);
        buf.push(b'\n');
    }

    let count = i32::try_from(trajectories.len()).unwrap_or(i32::MAX);
    Ok(ExportTrajectoriesResponse {
        request_id: req.request_id,
        jsonl: buf,
        count,
    })
}

/// JSON payload matching v2's Atropos export shape. Kept here rather
/// than deriving Serialize on the proto types so the wire format is
/// stable across proto evolution.
fn trajectory_to_payload(t: &Trajectory) -> serde_json::Value {
    serde_json::json!({
        "id": t.trajectory_id,
        "org_id": t.org_id,
        "run_id": t.run_id,
        "task_pattern": t.task_pattern,
        "goal": t.goal,
        "planned_actions": t.planned_actions.iter().map(action_to_payload).collect::<Vec<_>>(),
        "executed_actions": t.executed_actions.iter().map(action_to_payload).collect::<Vec<_>>(),
        "outcome": t.outcome,
        "duration_sec": t.duration_sec,
        "skills_used": t.skills_used,
        "cost_usd": t.cost_usd,
        "model": t.model,
        "created_at_unix": t.created_at_unix,
    })
}

fn action_to_payload(a: &mp_contracts::model_plane::v1::TrajectoryAction) -> serde_json::Value {
    serde_json::json!({
        "name": a.name,
        "status": a.status,
        "input_json": a.input_json,
        "output_json": a.output_json,
        "duration_ms": a.duration_ms,
    })
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_trajectory(org: &str, run: &str, outcome: &str) -> Trajectory {
        Trajectory {
            trajectory_id: String::new(),
            org_id: org.into(),
            run_id: run.into(),
            task_pattern: "test".into(),
            goal: "do thing".into(),
            planned_actions: vec![],
            executed_actions: vec![],
            outcome: outcome.into(),
            duration_sec: 1.0,
            skills_used: vec!["bash".into()],
            cost_usd: 0.01,
            model: "claude-sonnet-4-20250514".into(),
            created_at_unix: 0,
        }
    }

    #[tokio::test]
    async fn store_assigns_id_and_timestamp() {
        let s = TrajectoryStore::new();
        let id = s.store(make_trajectory("o", "r", "success")).await;
        assert!(!id.is_empty());
        let stored = s.by_id.get(&id).unwrap();
        assert!(stored.created_at_unix > 0);
    }

    #[tokio::test]
    async fn ring_buffer_evicts_oldest() {
        let s = TrajectoryStore::new();
        let mut first_id = String::new();
        for i in 0..MAX_TRAJECTORIES_PER_ORG + 5 {
            let mut t = make_trajectory("o", &format!("r{i}"), "success");
            t.trajectory_id = format!("t{i:04}");
            let id = s.store(t).await;
            if i == 0 {
                first_id = id;
            }
        }
        // First inserted should be evicted.
        assert!(s.by_id.get(&first_id).is_none());
        // Newest should be present.
        let last_id = format!("t{:04}", MAX_TRAJECTORIES_PER_ORG + 4);
        assert!(s.by_id.get(&last_id).is_some());
    }

    #[tokio::test]
    async fn list_filters_outcome_and_pattern() {
        let s = TrajectoryStore::new();
        s.store(make_trajectory("o", "r1", "success")).await;
        s.store(make_trajectory("o", "r2", "failure")).await;
        let mut diff = make_trajectory("o", "r3", "success");
        diff.task_pattern = "other".into();
        s.store(diff).await;

        let (list, total) = s.list("o", "success", "", 0, 10).await;
        assert_eq!(list.len(), 2);
        assert_eq!(total, 2);

        let (list2, _) = s.list("o", "success", "test", 0, 10).await;
        assert_eq!(list2.len(), 1);
    }

    #[tokio::test]
    async fn list_returns_empty_for_unknown_org() {
        let s = TrajectoryStore::new();
        let (list, total) = s.list("missing", "", "", 0, 10).await;
        assert!(list.is_empty());
        assert_eq!(total, 0);
    }

    #[tokio::test]
    async fn export_includes_time_bounded_records() {
        let s = TrajectoryStore::new();
        let mut t1 = make_trajectory("o", "r1", "success");
        t1.created_at_unix = 100;
        s.store(t1).await;
        let mut t2 = make_trajectory("o", "r2", "success");
        t2.created_at_unix = 200;
        s.store(t2).await;

        let (exp, _) = s.export("o", 150, 0).await;
        assert_eq!(exp.len(), 1);
        assert_eq!(exp[0].run_id, "r2");
    }
}
