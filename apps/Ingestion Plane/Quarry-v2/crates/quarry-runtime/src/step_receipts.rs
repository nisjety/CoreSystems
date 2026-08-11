//! Step receipts — append-only audit trail for agent actions.
//!
//! Every `AgentAction` executed by `AgentLoop` produces a receipt that's
//! either:
//! - Emitted as an `action.completed` / `action.failed` event (existing path)
//! - Persisted via [`StepReceiptStore`] (this module) for replay, audit, and
//!   debugging
//!
//! Stagehand-style "step receipts" are the contract that lets reviewers and
//! oversight tools answer "what exactly did the agent do?" without replaying
//! the whole loop. Quarry collects them; Model Plane reasons about them; App
//! Shell renders them.
//!
//! Persistence is pluggable — `InMemoryStore` for dev/test, `S3Store` for
//! production (artifact-bucket flavor), or a control-plane HTTP store for
//! organization-scoped storage. All stores are append-only; never mutate or
//! delete a receipt. To override a step, append a corrective receipt with
//! `correction_of: <prior_id>`.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use quarry_core::contracts::{AgentAction, AgentConstraints, BrowserObservation};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::ids::kinds::RunKind;
use quarry_core::lease::BrowserViewport;
use quarry_core::zdr::ZdrMode;

/// Durable continuation descriptor for a governed browser run. It deliberately
/// contains no browser process/session handle: after a crash, Quarry reacquires
/// a lease using the persisted profile and resumes only after the caller
/// presents a still-valid BrowserBroker grant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunCheckpoint {
    pub org_id: String,
    /// Signed user or service actor that started the browser run. Empty legacy
    /// checkpoints are deliberately not sufficient for user-scoped reads.
    #[serde(default)]
    pub actor_id: String,
    pub run_id: String,
    /// Lease identifiers are process-local, but retaining the identifier lets
    /// an owner projection distinguish the historical lease from a resumed one.
    #[serde(default)]
    pub lease_id: String,
    pub profile_id: String,
    pub step: u32,
    pub current_url: String,
    #[serde(default)]
    pub page_hash: String,
    pub constraints: AgentConstraints,
    #[serde(default)]
    pub persist_profile: bool,
    #[serde(default)]
    pub profile_scope: BrowserProfileScope,
    #[serde(default)]
    pub viewport: Option<BrowserViewport>,
    pub zdr: ZdrMode,
    #[serde(default)]
    pub grant_id: Option<String>,
    #[serde(default)]
    pub status: AgentRunStatus,
    /// The current browser-control mode. This belongs to the browser owner so
    /// a gateway restart cannot reset a human takeover back to agent control.
    #[serde(default)]
    pub control_mode: BrowserControlMode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStatus {
    #[default]
    Active,
    Closed,
}

/// The authority currently allowed to drive a browser run. This is an owner
/// state transition, not an `AgentAction`: Model Plane may propose actions,
/// but only Quarry accepts or rejects a control hand-off.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BrowserControlMode {
    #[default]
    AgentControl,
    HumanTakeover,
}

/// The retention and sharing boundary of the browser profile attached to a
/// run. This is owner state, rather than a gateway presentation hint: it is
/// checkpointed so a resumed run cannot silently change how its profile is
/// described to a different scope.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BrowserProfileScope {
    #[default]
    Ephemeral,
    UserPrivate,
    OrgShared,
    RunScoped,
}

/// A compact, append-only audit record for browser-session state transitions.
/// It intentionally excludes page bodies, screenshots, raw DevTools payloads,
/// credentials, and CDP traffic. Detailed action proof remains in
/// `StepReceipt`; timeline consumers combine the two owner-owned streams.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserTimelineEvent {
    pub event_id: String,
    pub org_id: String,
    pub actor_id: String,
    pub run_id: String,
    pub occurred_at: DateTime<Utc>,
    pub event: BrowserTimelineEventKind,
}

impl BrowserTimelineEvent {
    pub fn new(
        org_id: impl Into<String>,
        actor_id: impl Into<String>,
        run_id: impl Into<String>,
        event: BrowserTimelineEventKind,
    ) -> Self {
        Self {
            event_id: format!("bevt_{}", ulid::Ulid::new()),
            org_id: org_id.into(),
            actor_id: actor_id.into(),
            run_id: run_id.into(),
            occurred_at: Utc::now(),
            event,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserTimelineEventKind {
    Control {
        mode: BrowserControlMode,
        initiated_by: BrowserTimelineInitiator,
    },
    Tab {
        operation: BrowserTabOperation,
        tab_id: Option<String>,
        active_tab_id: Option<String>,
    },
    Devtools {
        event_count: u32,
        last_sequence: u64,
    },
    Lifecycle {
        state: BrowserTimelineLifecycle,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserTimelineInitiator {
    Human,
    Agent,
    System,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserTabOperation {
    Opened,
    Selected,
    Closed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserTimelineLifecycle {
    Started,
    Closed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepReceipt {
    /// Verified tenant that owns this receipt. Optional for backwards
    /// compatibility with receipts written before tenant binding existed;
    /// production writers must always populate it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<String>,
    /// Signed initiating user or delegated service actor. Legacy rows without
    /// this binding are deliberately excluded from user-scoped history reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    pub receipt_id: String,
    pub run_id: String,
    pub step: u32,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub action: AgentAction,
    pub outcome: StepOutcome,
    /// Snapshot of the post-action observation, if any. Stored so reviewers
    /// can see the world state the agent reacted to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation: Option<BrowserObservation>,
    /// When the agent retried after an error, points back at the original.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correction_of: Option<String>,
    /// Cost spent by this step in micro-USD (so receipts can be summed
    /// without floating-point drift).
    pub cost_micro_usd: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum StepOutcome {
    Completed { latency_ms: u64 },
    Failed { error_code: String, message: String },
    Skipped { reason: String },
}

#[async_trait]
pub trait StepReceiptStore: Send + Sync {
    /// Append a receipt. MUST be idempotent on `receipt_id`.
    async fn append(&self, receipt: StepReceipt) -> QuarryResult<()>;

    /// Tenant-aware append used by production paths. Existing stores retain
    /// their old behaviour by default; durable stores must enforce the org.
    async fn append_for_org(&self, org_id: &str, mut receipt: StepReceipt) -> QuarryResult<()> {
        receipt.org_id = Some(org_id.to_owned());
        self.append(receipt).await
    }

    /// Fetch all receipts for a run, ordered by `step` ascending.
    async fn list(&self, run_id: &RunKind) -> QuarryResult<Vec<StepReceipt>>;

    /// Tenant-aware listing. The default delegates for in-memory/dev stores;
    /// durable implementations override this to enforce the SQL predicate.
    async fn list_for_org(&self, org_id: &str, run_id: &RunKind) -> QuarryResult<Vec<StepReceipt>> {
        let receipts = self.list(run_id).await?;
        if receipts.iter().any(|receipt| {
            receipt
                .org_id
                .as_deref()
                .is_some_and(|owner| owner != org_id)
        }) {
            return Err(QuarryError::new(
                ErrorCode::Forbidden,
                "receipt stream belongs to another org",
            ));
        }
        Ok(receipts)
    }

    /// User/service-actor-aware receipt listing. Do not fall back to
    /// tenant-only history: records written before actor binding cannot be
    /// safely attributed and must remain unavailable to actor-scoped callers.
    async fn list_for_actor(
        &self,
        org_id: &str,
        actor_id: &str,
        run_id: &RunKind,
    ) -> QuarryResult<Vec<StepReceipt>> {
        if actor_id.trim().is_empty() {
            return Err(QuarryError::new(
                ErrorCode::Forbidden,
                "actor_id is required",
            ));
        }
        let receipts = self.list_for_org(org_id, run_id).await?;
        Ok(receipts
            .into_iter()
            .filter(|receipt| receipt.actor_id.as_deref() == Some(actor_id))
            .collect())
    }

    /// Append a privacy-bounded browser owner event. Implementations must be
    /// idempotent on `event_id`; callers skip this entirely for ZDR runs.
    async fn append_browser_timeline_event(
        &self,
        _event: BrowserTimelineEvent,
    ) -> QuarryResult<()> {
        Err(QuarryError::new(
            ErrorCode::Unsupported,
            "browser timeline persistence is not configured",
        ))
    }

    /// List the browser-owner events for one exact tenant, actor, and run.
    async fn list_browser_timeline_events(
        &self,
        _org_id: &str,
        _actor_id: &str,
        _run_id: &RunKind,
    ) -> QuarryResult<Vec<BrowserTimelineEvent>> {
        Err(QuarryError::new(
            ErrorCode::Unsupported,
            "browser timeline persistence is not configured",
        ))
    }

    /// Fetch a single receipt by ID.
    async fn get(&self, receipt_id: &str) -> QuarryResult<Option<StepReceipt>>;

    /// Save the latest resumable browser-run descriptor. Dev stores may keep
    /// this in memory; durable stores must make it tenant-scoped and
    /// idempotent. ZDR callers are expected to skip this method entirely.
    async fn save_run_checkpoint(&self, _checkpoint: AgentRunCheckpoint) -> QuarryResult<()> {
        Ok(())
    }

    async fn load_run_checkpoint(
        &self,
        _org_id: &str,
        _run_id: &RunKind,
    ) -> QuarryResult<Option<AgentRunCheckpoint>> {
        Ok(None)
    }

    async fn close_run_checkpoint(&self, _org_id: &str, _run_id: &RunKind) -> QuarryResult<()> {
        Ok(())
    }
}

/// In-process append-only store for tests and single-node dev. Receipts are
/// keyed by `run_id` so `list()` is a single hash lookup.
#[derive(Default, Clone)]
pub struct InMemoryStepReceiptStore {
    by_run: Arc<RwLock<HashMap<String, Vec<StepReceipt>>>>,
    by_id: Arc<RwLock<HashMap<String, StepReceipt>>>,
    checkpoints: Arc<RwLock<HashMap<(String, String), AgentRunCheckpoint>>>,
    timeline_events: Arc<RwLock<HashMap<(String, String, String), Vec<BrowserTimelineEvent>>>>,
}

impl InMemoryStepReceiptStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn count(&self) -> usize {
        self.by_id.read().await.len()
    }
}

#[async_trait]
impl StepReceiptStore for InMemoryStepReceiptStore {
    async fn append(&self, receipt: StepReceipt) -> QuarryResult<()> {
        let id = receipt.receipt_id.clone();
        let run = receipt.run_id.clone();

        // Idempotent: skip if we've seen this receipt_id before.
        let mut by_id = self.by_id.write().await;
        if by_id.contains_key(&id) {
            return Ok(());
        }
        by_id.insert(id.clone(), receipt.clone());
        drop(by_id);

        let mut by_run = self.by_run.write().await;
        by_run.entry(run).or_default().push(receipt);
        Ok(())
    }

    async fn list(&self, run_id: &RunKind) -> QuarryResult<Vec<StepReceipt>> {
        let by_run = self.by_run.read().await;
        let mut receipts = by_run.get(&run_id.to_string()).cloned().unwrap_or_default();
        receipts.sort_by_key(|r| r.step);
        Ok(receipts)
    }

    async fn get(&self, receipt_id: &str) -> QuarryResult<Option<StepReceipt>> {
        Ok(self.by_id.read().await.get(receipt_id).cloned())
    }

    async fn append_browser_timeline_event(&self, event: BrowserTimelineEvent) -> QuarryResult<()> {
        if event.org_id.trim().is_empty() || event.actor_id.trim().is_empty() {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "browser timeline event requires org_id and actor_id",
            ));
        }
        let key = (
            event.org_id.clone(),
            event.actor_id.clone(),
            event.run_id.clone(),
        );
        let mut events = self.timeline_events.write().await;
        let stream = events.entry(key).or_default();
        if stream
            .iter()
            .any(|current| current.event_id == event.event_id)
        {
            return Ok(());
        }
        stream.push(event);
        Ok(())
    }

    async fn list_browser_timeline_events(
        &self,
        org_id: &str,
        actor_id: &str,
        run_id: &RunKind,
    ) -> QuarryResult<Vec<BrowserTimelineEvent>> {
        let mut events = self
            .timeline_events
            .read()
            .await
            .get(&(org_id.to_owned(), actor_id.to_owned(), run_id.to_string()))
            .cloned()
            .unwrap_or_default();
        events.sort_by(|left, right| {
            left.occurred_at
                .cmp(&right.occurred_at)
                .then_with(|| left.event_id.cmp(&right.event_id))
        });
        Ok(events)
    }

    async fn save_run_checkpoint(&self, checkpoint: AgentRunCheckpoint) -> QuarryResult<()> {
        self.checkpoints.write().await.insert(
            (checkpoint.org_id.clone(), checkpoint.run_id.clone()),
            checkpoint,
        );
        Ok(())
    }

    async fn load_run_checkpoint(
        &self,
        org_id: &str,
        run_id: &RunKind,
    ) -> QuarryResult<Option<AgentRunCheckpoint>> {
        Ok(self
            .checkpoints
            .read()
            .await
            .get(&(org_id.to_owned(), run_id.to_string()))
            .cloned())
    }

    async fn close_run_checkpoint(&self, org_id: &str, run_id: &RunKind) -> QuarryResult<()> {
        if let Some(checkpoint) = self
            .checkpoints
            .write()
            .await
            .get_mut(&(org_id.to_owned(), run_id.to_string()))
        {
            checkpoint.status = AgentRunStatus::Closed;
        }
        Ok(())
    }
}

/// Builder helper — agent loops use this to construct receipts inline without
/// hand-rolling timestamps.
pub struct ReceiptBuilder {
    org_id: Option<String>,
    actor_id: Option<String>,
    run_id: String,
    step: u32,
    started_at: DateTime<Utc>,
    correction_of: Option<String>,
}

impl ReceiptBuilder {
    pub fn start(run_id: impl Into<String>, step: u32) -> Self {
        Self {
            org_id: None,
            actor_id: None,
            run_id: run_id.into(),
            step,
            started_at: Utc::now(),
            correction_of: None,
        }
    }

    pub fn org_id(mut self, org_id: impl Into<String>) -> Self {
        self.org_id = Some(org_id.into());
        self
    }

    pub fn actor_id(mut self, actor_id: impl Into<String>) -> Self {
        self.actor_id = Some(actor_id.into());
        self
    }

    pub fn correction_of(mut self, prior_receipt_id: impl Into<String>) -> Self {
        self.correction_of = Some(prior_receipt_id.into());
        self
    }

    pub fn complete(
        self,
        action: AgentAction,
        observation: Option<BrowserObservation>,
        cost_usd: f64,
    ) -> StepReceipt {
        let finished_at = Utc::now();
        let latency_ms = (finished_at - self.started_at).num_milliseconds().max(0) as u64;
        StepReceipt {
            org_id: self.org_id,
            actor_id: self.actor_id,
            receipt_id: format!("rcpt_{}", ulid::Ulid::new()),
            run_id: self.run_id,
            step: self.step,
            started_at: self.started_at,
            finished_at,
            action,
            outcome: StepOutcome::Completed { latency_ms },
            observation,
            correction_of: self.correction_of,
            cost_micro_usd: (cost_usd * 1_000_000.0).max(0.0) as u64,
        }
    }

    pub fn fail(self, action: AgentAction, err: &QuarryError) -> StepReceipt {
        let finished_at = Utc::now();
        StepReceipt {
            org_id: self.org_id,
            actor_id: self.actor_id,
            receipt_id: format!("rcpt_{}", ulid::Ulid::new()),
            run_id: self.run_id,
            step: self.step,
            started_at: self.started_at,
            finished_at,
            action,
            outcome: StepOutcome::Failed {
                error_code: format!("{:?}", err.code).to_uppercase(),
                message: err.message.clone(),
            },
            observation: None,
            correction_of: self.correction_of,
            cost_micro_usd: 0,
        }
    }

    pub fn skip(self, action: AgentAction, reason: impl Into<String>) -> StepReceipt {
        let finished_at = Utc::now();
        StepReceipt {
            org_id: self.org_id,
            actor_id: self.actor_id,
            receipt_id: format!("rcpt_{}", ulid::Ulid::new()),
            run_id: self.run_id,
            step: self.step,
            started_at: self.started_at,
            finished_at,
            action,
            outcome: StepOutcome::Skipped {
                reason: reason.into(),
            },
            observation: None,
            correction_of: self.correction_of,
            cost_micro_usd: 0,
        }
    }
}

/// Helper to verify a sequence of receipts is well-ordered (step counts
/// strictly increasing) and contains no duplicate receipt_ids.
pub fn validate_sequence(receipts: &[StepReceipt]) -> QuarryResult<()> {
    let mut prev_step: i64 = -1;
    let mut seen = std::collections::HashSet::new();
    for r in receipts {
        if !seen.insert(r.receipt_id.clone()) {
            return Err(QuarryError::new(
                ErrorCode::Conflict,
                format!("duplicate receipt_id: {}", r.receipt_id),
            ));
        }
        if (r.step as i64) <= prev_step {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                format!(
                    "receipts not strictly increasing: step {} after {}",
                    r.step, prev_step
                ),
            ));
        }
        prev_step = r.step as i64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::contracts::{AgentAction, AgentConstraints};
    use quarry_core::ids::Id;

    fn sample_action() -> AgentAction {
        AgentAction::Navigate {
            url: "https://example.com".into(),
        }
    }

    #[tokio::test]
    async fn append_and_list_returns_ordered_by_step() {
        let store = InMemoryStepReceiptStore::new();
        let run_id: RunKind = Id::new();

        let r2 =
            ReceiptBuilder::start(run_id.to_string(), 2).complete(sample_action(), None, 0.001);
        let r1 =
            ReceiptBuilder::start(run_id.to_string(), 1).complete(sample_action(), None, 0.001);
        let r3 =
            ReceiptBuilder::start(run_id.to_string(), 3).complete(sample_action(), None, 0.001);

        store.append(r2.clone()).await.unwrap();
        store.append(r1.clone()).await.unwrap();
        store.append(r3.clone()).await.unwrap();

        let listed = store.list(&run_id).await.unwrap();
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].step, 1);
        assert_eq!(listed[1].step, 2);
        assert_eq!(listed[2].step, 3);
    }

    #[tokio::test]
    async fn in_memory_checkpoint_is_tenant_bound_and_closes() {
        let store = InMemoryStepReceiptStore::new();
        let run_id: RunKind = Id::new();
        let checkpoint = AgentRunCheckpoint {
            org_id: "org_a".into(),
            actor_id: "user_a".into(),
            run_id: run_id.to_string(),
            lease_id: "lease_01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            profile_id: "prof_01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            step: 2,
            current_url: "https://example.com".into(),
            page_hash: "blake3:abc".into(),
            constraints: AgentConstraints {
                max_steps: 5,
                allowed_domains: vec!["example.com".into()],
                max_runtime_s: Some(60),
                max_cost_usd: None,
            },
            persist_profile: true,
            profile_scope: BrowserProfileScope::UserPrivate,
            viewport: None,
            zdr: ZdrMode::Off,
            grant_id: Some("grant_1".into()),
            status: AgentRunStatus::Active,
            control_mode: BrowserControlMode::AgentControl,
        };
        store.save_run_checkpoint(checkpoint).await.unwrap();
        assert!(store
            .load_run_checkpoint("org_b", &run_id)
            .await
            .unwrap()
            .is_none());
        let loaded = store
            .load_run_checkpoint("org_a", &run_id)
            .await
            .unwrap()
            .expect("checkpoint should be present");
        assert_eq!(loaded.step, 2);
        assert_eq!(loaded.actor_id, "user_a");
        assert_eq!(loaded.lease_id, "lease_01ARZ3NDEKTSV4RRFFQ69G5FAV");
        store.close_run_checkpoint("org_a", &run_id).await.unwrap();
        assert_eq!(
            store
                .load_run_checkpoint("org_a", &run_id)
                .await
                .unwrap()
                .unwrap()
                .status,
            AgentRunStatus::Closed
        );
    }

    #[tokio::test]
    async fn append_is_idempotent_on_receipt_id() {
        let store = InMemoryStepReceiptStore::new();
        let run_id: RunKind = Id::new();
        let r = ReceiptBuilder::start(run_id.to_string(), 1).complete(sample_action(), None, 0.0);
        store.append(r.clone()).await.unwrap();
        store.append(r.clone()).await.unwrap();
        store.append(r).await.unwrap();
        assert_eq!(store.count().await, 1);
    }

    #[tokio::test]
    async fn list_unknown_run_returns_empty() {
        let store = InMemoryStepReceiptStore::new();
        let run_id: RunKind = Id::new();
        let listed = store.list(&run_id).await.unwrap();
        assert!(listed.is_empty());
    }

    #[tokio::test]
    async fn get_returns_some_when_present() {
        let store = InMemoryStepReceiptStore::new();
        let run_id: RunKind = Id::new();
        let r = ReceiptBuilder::start(run_id.to_string(), 1).complete(sample_action(), None, 0.0);
        let id = r.receipt_id.clone();
        store.append(r).await.unwrap();
        let got = store.get(&id).await.unwrap();
        assert!(got.is_some());
    }

    #[tokio::test]
    async fn fail_receipt_records_error_outcome() {
        let err = QuarryError::new(ErrorCode::Timeout, "slow page");
        let receipt = ReceiptBuilder::start("run_x", 1).fail(sample_action(), &err);
        match &receipt.outcome {
            StepOutcome::Failed {
                error_code,
                message,
            } => {
                assert_eq!(error_code, "TIMEOUT");
                assert_eq!(message, "slow page");
            }
            _ => panic!("expected Failed outcome"),
        }
    }

    #[tokio::test]
    async fn correction_of_links_back_to_prior_receipt() {
        let prior = ReceiptBuilder::start("run_x", 1).complete(sample_action(), None, 0.0);
        let corrective = ReceiptBuilder::start("run_x", 2)
            .correction_of(prior.receipt_id.clone())
            .complete(sample_action(), None, 0.0);
        assert_eq!(
            corrective.correction_of.as_deref(),
            Some(prior.receipt_id.as_str())
        );
    }

    #[test]
    fn validate_sequence_rejects_out_of_order() {
        let r1 = ReceiptBuilder::start("run_x", 2).complete(sample_action(), None, 0.0);
        let r2 = ReceiptBuilder::start("run_x", 1).complete(sample_action(), None, 0.0);
        let err = validate_sequence(&[r1, r2]).unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }

    #[test]
    fn validate_sequence_rejects_duplicate_id() {
        let r = ReceiptBuilder::start("run_x", 1).complete(sample_action(), None, 0.0);
        let r_dup = StepReceipt {
            org_id: None,
            actor_id: None,
            receipt_id: r.receipt_id.clone(),
            ..r.clone()
        };
        let err = validate_sequence(&[r, r_dup]).unwrap_err();
        assert_eq!(err.code, ErrorCode::Conflict);
    }

    #[test]
    fn cost_micro_usd_rounds_correctly() {
        let receipt = ReceiptBuilder::start("run_x", 1).complete(sample_action(), None, 0.012345);
        assert_eq!(receipt.cost_micro_usd, 12_345);
    }

    #[tokio::test]
    async fn actor_scoped_receipts_never_fall_back_to_tenant_only_history() {
        let store = InMemoryStepReceiptStore::new();
        let run_id: RunKind = Id::new();
        let receipt = ReceiptBuilder::start(run_id.to_string(), 1)
            .org_id("org_a")
            .actor_id("user_a")
            .complete(sample_action(), None, 0.0);
        store.append(receipt).await.unwrap();
        let legacy_receipt = ReceiptBuilder::start(run_id.to_string(), 2)
            .org_id("org_a")
            .complete(sample_action(), None, 0.0);
        store.append(legacy_receipt).await.unwrap();

        assert_eq!(
            store
                .list_for_actor("org_a", "user_a", &run_id)
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(store
            .list_for_actor("org_a", "user_b", &run_id)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn browser_timeline_is_exactly_tenant_and_actor_scoped() {
        let store = InMemoryStepReceiptStore::new();
        let run_id: RunKind = Id::new();
        store
            .append_browser_timeline_event(BrowserTimelineEvent::new(
                "org_a",
                "user_a",
                run_id.to_string(),
                BrowserTimelineEventKind::Control {
                    mode: BrowserControlMode::HumanTakeover,
                    initiated_by: BrowserTimelineInitiator::Human,
                },
            ))
            .await
            .unwrap();

        assert_eq!(
            store
                .list_browser_timeline_events("org_a", "user_a", &run_id)
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(store
            .list_browser_timeline_events("org_a", "user_b", &run_id)
            .await
            .unwrap()
            .is_empty());
    }
}
