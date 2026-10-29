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
use quarry_core::redaction::action::{
    record_policy as process_record_policy, redact_observation_durable, redacted_for_human_receipt,
    redacted_for_record, RecordBranch, RecordPolicy,
};
use quarry_core::redaction::{
    self, redact_text_with, FieldKind, Mode, Profile, Tally, RULE_VERSION,
};
use quarry_core::zdr::ZdrMode;

use crate::action_cost::CostSource;

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
    /// How many times `control_mode` has really changed. A resumed run
    /// continues this count so an epoch never repeats for one run. 0 on a
    /// checkpoint written before epochs existed.
    #[serde(default)]
    pub control_epoch: u64,
    /// What the run has spent against its budget. Absent on a checkpoint
    /// written before budgets existed, or with enforcement off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<BudgetCheckpoint>,
    /// The navigation policy mode of the run's grant (WS8.B): `allowlist` or
    /// `open_web`. Absent on a checkpoint written before human browsing existed
    /// (read as `allowlist`). A resume whose grant disagrees is refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_mode: Option<String>,
    /// The step key the run hands out next: one past the highest key it reserved, for a
    /// completed, failed or refused-landing step alike (`step` counts completed steps
    /// only, and stays what the step cap and the projection read). A resume starts its
    /// keys at the highest of this, `step` and one past the run's last receipt, so no step
    /// key repeats in a run. 0 on a checkpoint written before it existed (D2).
    #[serde(default)]
    pub next_step: u32,
}

/// The budget ledger of a run, stored with its checkpoint so a resume restores
/// what was spent instead of starting from zero.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetCheckpoint {
    /// `interactive` or `agent`.
    #[serde(default)]
    pub class: String,
    /// Completed steps.
    #[serde(default)]
    pub steps: u32,
    /// Attempts, completed or failed.
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub spent_micro_usd: u64,
    /// Time under agent control, in milliseconds.
    #[serde(default)]
    pub active_ms: u64,
    /// WS6.C-e: the run segment this checkpoint was written in. 0 for the first start; a
    /// resume is one more. Usage events are keyed by it, so a resumed run never reuses an
    /// event id. Absent (0) on a checkpoint written before segments existed.
    #[serde(default, skip_serializing_if = "is_first_segment")]
    pub segment: u32,
    /// The dimension that was reached, once one was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exhausted: Option<String>,
    /// The grant budget claim (WS6.E) the run's bounds were tightened by, so a
    /// resume keeps it. Absent on a checkpoint written before claims existed
    /// or for a run no claim applied to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim: Option<crate::grant_validator::GrantBudget>,
}

/// serde skip rule: the first segment is the default and is not written.
fn is_first_segment(segment: &u32) -> bool {
    *segment == 0
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
        /// The control epoch this transition produced.
        #[serde(default, skip_serializing_if = "is_zero")]
        control_epoch: u64,
    },
    Tab {
        operation: BrowserTabOperation,
        tab_id: Option<String>,
        active_tab_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        initiated_by: Option<BrowserTimelineInitiator>,
        /// Which agent opened, selected or closed the tab, when `initiated_by` is
        /// `agent`. Absent for a person and on events written before agents were
        /// recorded.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent: Option<AgentIdentity>,
        #[serde(default, skip_serializing_if = "is_zero")]
        control_epoch: u64,
    },
    Devtools {
        event_count: u32,
        last_sequence: u64,
    },
    /// WS8.F: who holds a tab after a change. `tab_epoch` is the tab's epoch after it.
    TabControl {
        tab_id: String,
        holder: BrowserTabHolder,
        initiated_by: BrowserTimelineInitiator,
        #[serde(default, skip_serializing_if = "is_zero")]
        tab_epoch: u64,
    },
    /// WS8.F: an agent attachment began or ended.
    Attach {
        state: BrowserAttachState,
        attach_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tab_id: Option<String>,
    },
    Lifecycle {
        state: BrowserTimelineLifecycle,
        /// Who ended the run when its owner did not: `system` for the sweeper's
        /// expiry. Absent for `started`, for an owner's close and on events written
        /// before this field existed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        initiated_by: Option<BrowserTimelineInitiator>,
    },
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserTimelineInitiator {
    Human,
    Agent,
    System,
}

/// The timeline kinds this build understands. A row written by a later release
/// with another kind is skipped rather than failing the whole read (WS8.B ships
/// this decode one release before WS8.F writes new kinds: rollback safety).
pub const KNOWN_TIMELINE_KINDS: &[&str] = &["control", "tab", "devtools", "lifecycle"];

/// Decode one stored timeline event body. `Ok(None)` for a kind this build does
/// not know; an error only for a known kind whose body is malformed.
pub fn decode_timeline_event_body(
    body: serde_json::Value,
) -> QuarryResult<Option<BrowserTimelineEventKind>> {
    let known = body
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|kind| KNOWN_TIMELINE_KINDS.contains(&kind));
    if !known {
        return Ok(None);
    }
    serde_json::from_value(body).map(Some).map_err(|error| {
        QuarryError::new(
            ErrorCode::Internal,
            format!("decode browser timeline event: {error}"),
        )
    })
}

/// How an [`AgentIdentity`] was established.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentIdentityBasis {
    /// Verified: the call carried a service principal's own signed token, and
    /// `principal_id` is that token's service id.
    ServicePrincipal,
    /// Not verified: the call carried a person's session token with an `agent`
    /// hint (the gateway sends one for an action the model proposed through the
    /// person's own session). Which agent it was is only what the caller said.
    UserSession,
}

/// Which agent was behind an operation an agent initiated. Ids and classes only:
/// never a URL, a title or free text, so it is safe to store under ZDR rules that
/// allow ids.
///
/// `principal_id` is the only verified part, and only for
/// `AgentIdentityBasis::ServicePrincipal`. `ai_run_id` and `label` are what the
/// caller supplied (identifier-shaped, bounded): advisory, never used for an
/// authorization decision.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentIdentity {
    pub basis: AgentIdentityBasis,
    /// The signed service id of the calling principal; present only for
    /// `ServicePrincipal`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principal_id: Option<String>,
    /// The AI run the caller says it is acting for. Advisory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_run_id: Option<String>,
    /// A short identifier-shaped name the caller gave its agent. Advisory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl AgentIdentity {
    /// True when `principal_id` was established from a verified token.
    #[must_use]
    pub fn is_verified(&self) -> bool {
        self.basis == AgentIdentityBasis::ServicePrincipal && self.principal_id.is_some()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserTabOperation {
    Opened,
    Selected,
    Closed,
}

/// Who holds a browser tab (WS8.F). The wire values are `human` and `agent`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserTabHolder {
    Human,
    Agent,
}

/// Where an agent attachment stands (WS8.F). `attached` is the only live state.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserAttachState {
    Attached,
    Released,
    Aborted,
    Revoked,
    IdleReleased,
    SessionEnded,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserTimelineLifecycle {
    Started,
    Closed,
    /// Released by the run sweeper rather than closed by its owner.
    Expired,
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
    /// Who drove this step: a person, an agent or the system. Absent on a
    /// receipt written before initiators were recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initiated_by: Option<BrowserTimelineInitiator>,
    /// The control epoch the step was admitted under. Absent on older receipts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_epoch: Option<u64>,
    /// Which agent drove the step, when `initiated_by` is `agent`. Absent for a
    /// person and on receipts written before agents were recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentIdentity>,
    /// Where `cost_micro_usd` came from (`flat_table`, `provider_billed`). Absent
    /// on a receipt with no recorded source (written before WS6.B, or a skip).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_source: Option<CostSource>,
    /// The version of the secret-redaction rule set the stored action, observation
    /// and failure text were masked under (WS12.S3). Absent on a receipt written
    /// before it existed (the stored text may be raw; every read masks it) and on
    /// one written while masking was not enforcing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redaction_rule_version: Option<String>,
    /// The browser tab a step addressed, when the step ran on an attached run or was a
    /// tab operation (WS8.F). Absent for a run that has only its first tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    /// The attach a delegate step ran under (WS8.F). Absent for a person's step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach_id: Option<String>,
}

impl StepReceipt {
    /// Mask this receipt for a reader, under the process-wide redaction mode and
    /// receipt text policy (WS12.S3). Stored evidence is never rewritten: a legacy
    /// row that holds raw text is returned masked.
    ///
    /// Idempotent. A receipt stamped with the current [`RULE_VERSION`] is returned
    /// as it is. A receipt with no stamp may hold raw typed text and an unknown
    /// control kind, so its ref-addressed typing actions are masked as secrets; a
    /// receipt stamped with an older version was masked by control kind when it was
    /// written, so it only passes the `Durable` text rules again.
    pub fn redact_for_read(&mut self) {
        self.redact_for_read_with(redaction::mode(), process_record_policy());
    }

    /// [`Self::redact_for_read`] under an explicit mode and policy.
    pub fn redact_for_read_with(&mut self, mode: Mode, policy: RecordPolicy) {
        if self.redaction_rule_version.as_deref() == Some(RULE_VERSION) {
            return;
        }
        // A key press carries no control kind of its own (D2-065): on a row that was not
        // masked under the current rules, its character is masked as a secret.
        let typed = if self.redaction_rule_version.is_none()
            || matches!(self.action, AgentAction::Press { .. })
        {
            None
        } else {
            Some(FieldKind::Unknown)
        };
        let mut tally = Tally::new();
        self.action = redacted_for_record(
            RecordBranch::Typed,
            &self.action,
            typed,
            policy,
            mode,
            &mut tally,
        );
        self.observation = self
            .observation
            .take()
            .and_then(|observation| redact_observation_durable(&observation, mode, &mut tally));
        mask_outcome_text(&mut self.outcome, mode, &mut tally);
        if mode == Mode::Enforce {
            self.redaction_rule_version = Some(RULE_VERSION.to_owned());
        }
    }
}

/// The free text of an outcome (a failure message, a skip reason) under the
/// `Durable` text rules.
fn mask_outcome_text(outcome: &mut StepOutcome, mode: Mode, tally: &mut Tally) {
    match outcome {
        StepOutcome::Failed { message, .. } => {
            *message = redact_text_with(mode, message, Profile::Durable, tally).into_owned();
        }
        StepOutcome::Skipped { reason } => {
            *reason = redact_text_with(mode, reason, Profile::Durable, tally).into_owned();
        }
        StepOutcome::Completed { .. } => {}
    }
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

type TimelineEventStream = Vec<BrowserTimelineEvent>;
type TimelineEventIndex = HashMap<(String, String, String), TimelineEventStream>;

/// In-process append-only store for tests and single-node dev. Receipts are
/// keyed by `run_id` so `list()` is a single hash lookup.
#[derive(Default, Clone)]
pub struct InMemoryStepReceiptStore {
    by_run: Arc<RwLock<HashMap<String, Vec<StepReceipt>>>>,
    by_id: Arc<RwLock<HashMap<String, StepReceipt>>>,
    checkpoints: Arc<RwLock<HashMap<(String, String), AgentRunCheckpoint>>>,
    timeline_events: Arc<RwLock<TimelineEventIndex>>,
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
    initiated_by: Option<BrowserTimelineInitiator>,
    control_epoch: Option<u64>,
    agent: Option<AgentIdentity>,
    cost_source: Option<CostSource>,
    tab_id: Option<String>,
    attach_id: Option<String>,
    /// The flat cost of a failed attempt (the budget charges it; WS6.B).
    failed_cost_micro_usd: u64,
    /// The kind of the control a typing action went to, read from the snapshot
    /// before the action ran (WS12.S3). `None` when it is not known.
    typed_field: Option<FieldKind>,
    record_policy: RecordPolicy,
    redaction_mode: Mode,
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
            initiated_by: None,
            control_epoch: None,
            agent: None,
            cost_source: None,
            tab_id: None,
            attach_id: None,
            failed_cost_micro_usd: 0,
            typed_field: None,
            record_policy: process_record_policy(),
            redaction_mode: redaction::mode(),
        }
    }

    /// The kind of the control a typing action went to (`typed_field_kind` on the
    /// snapshot, read before the action ran). Call before `complete`, `fail` or
    /// `skip`. Without it a ref-addressed typing action is masked as a secret.
    pub fn typed_field(mut self, kind: Option<FieldKind>) -> Self {
        self.typed_field = kind;
        self
    }

    /// Which text this receipt keeps (default: the process-wide policy).
    pub fn record_policy(mut self, policy: RecordPolicy) -> Self {
        self.record_policy = policy;
        self
    }

    /// The redaction mode this receipt is built under (default: the process-wide mode).
    pub fn redaction_mode(mut self, mode: Mode) -> Self {
        self.redaction_mode = mode;
        self
    }

    /// The action as this builder's receipt may carry it.
    fn masked_action(&self, action: &AgentAction, tally: &mut Tally) -> AgentAction {
        redacted_for_record(
            RecordBranch::Typed,
            action,
            self.typed_field,
            self.record_policy,
            self.redaction_mode,
            tally,
        )
    }

    /// The stamp: set only when the rules were enforcing, so a receipt written with
    /// masking off or in shadow is masked again on every read.
    fn rule_version_stamp(&self) -> Option<String> {
        (self.redaction_mode == Mode::Enforce).then(|| RULE_VERSION.to_owned())
    }

    /// Where the cost of a completed step came from. Call before `complete`.
    pub fn cost_source(mut self, source: CostSource) -> Self {
        self.cost_source = Some(source);
        self
    }

    /// The flat cost of a failed attempt, in USD. Call before `fail`: a failed attempt
    /// is charged against the budget, so its receipt carries the same cost.
    pub fn failed_cost(mut self, cost_usd: f64) -> Self {
        self.failed_cost_micro_usd = (cost_usd * 1_000_000.0).max(0.0) as u64;
        self
    }

    /// Record which agent drove the step (call after `initiator`).
    pub fn agent(mut self, agent: Option<AgentIdentity>) -> Self {
        self.agent = agent;
        self
    }

    /// Record the tab a step addressed and the attach it ran under (WS8.F).
    pub fn tab(mut self, tab_id: Option<String>, attach_id: Option<String>) -> Self {
        self.tab_id = tab_id;
        self.attach_id = attach_id;
        self
    }

    /// Record who drove the step and the control epoch it was admitted under.
    pub fn initiator(mut self, initiated_by: BrowserTimelineInitiator, control_epoch: u64) -> Self {
        self.initiated_by = Some(initiated_by);
        self.control_epoch = Some(control_epoch);
        self
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

    /// A completed step's receipt. The action is masked by the control kind and the
    /// text policy, the observation passes the `Durable` rules over every string
    /// leaf, and the receipt carries the rule version it was masked under.
    pub fn complete(
        self,
        action: AgentAction,
        observation: Option<BrowserObservation>,
        cost_usd: f64,
    ) -> StepReceipt {
        let mut tally = Tally::new();
        let action = self.masked_action(&action, &mut tally);
        let observation = observation.and_then(|observation| {
            redact_observation_durable(&observation, self.redaction_mode, &mut tally)
        });
        let stamp = self.rule_version_stamp();
        self.finish_completed(action, observation, cost_usd, stamp)
    }

    fn finish_completed(
        self,
        action: AgentAction,
        observation: Option<BrowserObservation>,
        cost_usd: f64,
        redaction_rule_version: Option<String>,
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
            cost_source: self.cost_source,
            initiated_by: self.initiated_by,
            control_epoch: self.control_epoch,
            agent: self.agent,
            redaction_rule_version,
            tab_id: self.tab_id,
            attach_id: self.attach_id,
        }
    }

    /// [`Self::complete`], or for a metadata-only plan [`Self::complete_metadata_only`].
    pub fn complete_maybe_metadata(
        self,
        metadata_only: bool,
        action: AgentAction,
        observation: Option<BrowserObservation>,
        cost_usd: f64,
    ) -> StepReceipt {
        if metadata_only {
            self.complete_metadata_only(action, cost_usd)
        } else {
            self.complete(action, observation, cost_usd)
        }
    }

    /// [`Self::fail`], with the action redacted on a metadata-only plan. A failure
    /// never stores an observation.
    pub fn fail_maybe_metadata(
        self,
        metadata_only: bool,
        action: AgentAction,
        err: &QuarryError,
    ) -> StepReceipt {
        if metadata_only {
            let action = redacted_for_human_receipt(&action);
            self.finish_failed(action, err, Some(RULE_VERSION.to_owned()))
        } else {
            self.fail(action, err)
        }
    }

    /// A human step's receipt on a non-ZDR run (WS8.B): metadata only. The action
    /// is redacted (typed text, selected values and key presses become
    /// `[redacted]`; a navigation keeps scheme, host and path) and no observation
    /// is stored.
    pub fn complete_metadata_only(self, action: AgentAction, cost_usd: f64) -> StepReceipt {
        self.finish_completed(
            redacted_for_human_receipt(&action),
            None,
            cost_usd,
            Some(RULE_VERSION.to_owned()),
        )
    }

    /// A failed attempt's receipt. The action is masked from the pre-action control
    /// kind exactly as a completed step's is, and the failure message passes the
    /// `Durable` text rules: an error can quote the text that was typed.
    pub fn fail(self, action: AgentAction, err: &QuarryError) -> StepReceipt {
        let mut tally = Tally::new();
        let action = self.masked_action(&action, &mut tally);
        let stamp = self.rule_version_stamp();
        self.finish_failed(action, err, stamp)
    }

    fn finish_failed(
        self,
        action: AgentAction,
        err: &QuarryError,
        redaction_rule_version: Option<String>,
    ) -> StepReceipt {
        let finished_at = Utc::now();
        let message = redact_text_with(
            self.redaction_mode,
            &err.message,
            Profile::Durable,
            &mut Tally::new(),
        )
        .into_owned();
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
                // The wire name (SECURITY_BLOCKED), not the Debug rendering of
                // the Rust identifier (SECURITYBLOCKED). Rows already stored keep
                // their old spelling.
                error_code: err.code.wire_name().to_owned(),
                message,
            },
            observation: None,
            correction_of: self.correction_of,
            cost_micro_usd: self.failed_cost_micro_usd,
            cost_source: self.cost_source,
            initiated_by: self.initiated_by,
            control_epoch: self.control_epoch,
            agent: self.agent,
            redaction_rule_version,
            tab_id: self.tab_id,
            attach_id: self.attach_id,
        }
    }

    pub fn skip(self, action: AgentAction, reason: impl Into<String>) -> StepReceipt {
        let mut tally = Tally::new();
        let action = self.masked_action(&action, &mut tally);
        let redaction_rule_version = self.rule_version_stamp();
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
            cost_source: None,
            initiated_by: self.initiated_by,
            control_epoch: self.control_epoch,
            agent: self.agent,
            redaction_rule_version,
            tab_id: self.tab_id,
            attach_id: self.attach_id,
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
            control_epoch: 0,
            budget: None,
            policy_mode: None,
            next_step: 3,
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
        assert_eq!(loaded.next_step, 3, "the step key high-water mark is kept");
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

    /// D2: a checkpoint written before `next_step` existed loads with 0, so a resume falls
    /// back to `step` and the run's receipts for its first key.
    #[test]
    fn legacy_checkpoint_without_next_step_reads_zero() {
        let legacy = serde_json::json!({
            "org_id": "org_a",
            "run_id": "run_a",
            "profile_id": "prof_a",
            "step": 1,
            "current_url": "",
            "constraints": { "max_steps": 5, "allowed_domains": ["example.com"] },
            "zdr": "off",
        });
        let checkpoint: AgentRunCheckpoint =
            serde_json::from_value(legacy).expect("a checkpoint without next_step loads");
        assert_eq!((checkpoint.step, checkpoint.next_step), (1, 0));
        let written = serde_json::to_value(AgentRunCheckpoint {
            next_step: 4,
            ..checkpoint
        })
        .unwrap();
        assert_eq!(written["next_step"], 4, "the field is always written");
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
    fn receipt_roundtrips_initiator_and_epoch() {
        let receipt = ReceiptBuilder::start("run_x", 1)
            .org_id("org_a")
            .actor_id("user_a")
            .initiator(BrowserTimelineInitiator::Agent, 3)
            .complete(sample_action(), None, 0.0);
        assert_eq!(receipt.initiated_by, Some(BrowserTimelineInitiator::Agent));
        assert_eq!(receipt.control_epoch, Some(3));
        let json = serde_json::to_value(&receipt).unwrap();
        assert_eq!(json["initiated_by"], "agent");
        assert_eq!(json["control_epoch"], 3);
        let back: StepReceipt = serde_json::from_value(json).unwrap();
        assert_eq!(back.initiated_by, Some(BrowserTimelineInitiator::Agent));
        assert_eq!(back.control_epoch, Some(3));
    }

    #[test]
    fn legacy_receipt_without_new_fields_deserialises() {
        // A receipt written before initiators were recorded has neither key, and a
        // receipt without them does not write them either.
        let receipt = ReceiptBuilder::start("run_x", 1).complete(sample_action(), None, 0.0);
        let mut json = serde_json::to_value(&receipt).unwrap();
        assert!(json.get("initiated_by").is_none());
        assert!(json.get("control_epoch").is_none());
        json.as_object_mut().unwrap().remove("initiated_by");
        let back: StepReceipt = serde_json::from_value(json).unwrap();
        assert_eq!(back.initiated_by, None);
        assert_eq!(back.control_epoch, None);
    }

    fn service_agent() -> AgentIdentity {
        AgentIdentity {
            basis: AgentIdentityBasis::ServicePrincipal,
            principal_id: Some("service:execution-core".into()),
            ai_run_id: Some("run_01J".into()),
            label: None,
        }
    }

    #[test]
    fn receipt_roundtrips_the_agent_identity_and_omits_it_for_a_person() {
        let receipt = ReceiptBuilder::start("run_x", 1)
            .initiator(BrowserTimelineInitiator::Agent, 0)
            .agent(Some(service_agent()))
            .complete(sample_action(), None, 0.0);
        let json = serde_json::to_value(&receipt).unwrap();
        assert_eq!(json["agent"]["basis"], "service_principal");
        assert_eq!(json["agent"]["principalId"], "service:execution-core");
        assert_eq!(json["agent"]["aiRunId"], "run_01J");
        assert!(json["agent"].get("label").is_none(), "{json}");
        let back: StepReceipt = serde_json::from_value(json).unwrap();
        assert_eq!(back.agent, Some(service_agent()));
        assert!(back.agent.as_ref().unwrap().is_verified());

        let person = ReceiptBuilder::start("run_x", 2)
            .initiator(BrowserTimelineInitiator::Human, 0)
            .complete(sample_action(), None, 0.0);
        assert!(serde_json::to_value(&person).unwrap().get("agent").is_none());
    }

    #[test]
    fn a_user_session_identity_is_never_verified() {
        let identity = AgentIdentity {
            basis: AgentIdentityBasis::UserSession,
            principal_id: Some("service:forged".into()),
            ai_run_id: None,
            label: Some("planner".into()),
        };
        assert!(!identity.is_verified());
    }

    #[test]
    fn tab_event_roundtrips_the_agent_and_a_legacy_event_has_none() {
        let kind = BrowserTimelineEventKind::Tab {
            operation: BrowserTabOperation::Opened,
            tab_id: Some("tab-2".into()),
            active_tab_id: Some("tab-2".into()),
            initiated_by: Some(BrowserTimelineInitiator::Agent),
            agent: Some(service_agent()),
            control_epoch: 1,
        };
        let json = serde_json::to_value(&kind).unwrap();
        assert_eq!(json["kind"], "tab");
        assert_eq!(json["initiated_by"], "agent");
        assert_eq!(json["agent"]["principalId"], "service:execution-core");
        assert_eq!(serde_json::from_value::<BrowserTimelineEventKind>(json).unwrap(), kind);

        // An event stored before agents were recorded has no key and reads as none.
        let legacy = serde_json::json!({
            "kind": "tab", "operation": "closed", "tab_id": "tab-1",
            "active_tab_id": null, "initiated_by": "agent", "control_epoch": 2
        });
        match serde_json::from_value::<BrowserTimelineEventKind>(legacy).unwrap() {
            BrowserTimelineEventKind::Tab { agent, initiated_by, .. } => {
                assert_eq!(agent, None);
                assert_eq!(initiated_by, Some(BrowserTimelineInitiator::Agent));
            }
            other => panic!("not a tab event: {other:?}"),
        }
    }

    /// A lifecycle event stored before the expiry carried an initiator has no
    /// `initiated_by`; it reads as `None`, and a `None` is not written back out.
    #[test]
    fn legacy_lifecycle_event_without_an_initiator_deserialises_and_an_expiry_roundtrips() {
        let legacy: BrowserTimelineEventKind =
            serde_json::from_str(r#"{"kind":"lifecycle","state":"expired"}"#).unwrap();
        assert_eq!(
            legacy,
            BrowserTimelineEventKind::Lifecycle {
                state: BrowserTimelineLifecycle::Expired,
                initiated_by: None,
            }
        );
        let started = serde_json::to_value(BrowserTimelineEventKind::Lifecycle {
            state: BrowserTimelineLifecycle::Started,
            initiated_by: None,
        })
        .unwrap();
        assert!(started.get("initiated_by").is_none(), "{started}");

        let expired = BrowserTimelineEventKind::Lifecycle {
            state: BrowserTimelineLifecycle::Expired,
            initiated_by: Some(BrowserTimelineInitiator::System),
        };
        let wire = serde_json::to_value(&expired).unwrap();
        assert_eq!(wire["initiated_by"], "system");
        assert_eq!(serde_json::from_value::<BrowserTimelineEventKind>(wire).unwrap(), expired);
    }

    #[test]
    fn legacy_checkpoint_without_control_epoch_defaults_to_zero() {
        let checkpoint = serde_json::json!({
            "org_id": "org_a",
            "run_id": "run_x",
            "profile_id": "prof_x",
            "step": 1,
            "current_url": "https://example.com",
            "constraints": { "max_steps": 5, "allowed_domains": [] },
            "zdr": "off"
        });
        let loaded: AgentRunCheckpoint = serde_json::from_value(checkpoint).unwrap();
        assert_eq!(loaded.control_epoch, 0);
        assert_eq!(loaded.control_mode, BrowserControlMode::AgentControl);
    }

    #[test]
    fn failed_receipt_uses_the_wire_name_of_the_code() {
        let err = QuarryError::new(ErrorCode::SecurityBlocked, "denied");
        let receipt = ReceiptBuilder::start("run_x", 1).fail(sample_action(), &err);
        match receipt.outcome {
            StepOutcome::Failed { error_code, .. } => assert_eq!(error_code, "SECURITY_BLOCKED"),
            _ => panic!("expected Failed outcome"),
        }
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

    #[test]
    fn failure_receipt_carries_the_flat_cost_it_was_charged() {
        let err = QuarryError::new(ErrorCode::Internal, "driver failed");
        let failed = ReceiptBuilder::start("run_x", 1)
            .failed_cost(0.0001)
            .cost_source(CostSource::FlatTable)
            .fail(sample_action(), &err);
        assert_eq!(failed.cost_micro_usd, 100);
        assert_eq!(failed.cost_source, Some(CostSource::FlatTable));
        // A skip is never priced.
        let skipped = ReceiptBuilder::start("run_x", 2).skip(sample_action(), "user_took_over");
        assert_eq!(skipped.cost_micro_usd, 0);
        assert_eq!(skipped.cost_source, None);
    }

    #[test]
    fn receipt_roundtrips_cost_source_and_serialises_it_as_the_column_value() {
        let receipt = ReceiptBuilder::start("run_x", 1)
            .cost_source(CostSource::ProviderBilled)
            .complete(sample_action(), None, 0.002);
        let value = serde_json::to_value(&receipt).unwrap();
        assert_eq!(value["cost_source"], "provider_billed");
        let back: StepReceipt = serde_json::from_value(value).unwrap();
        assert_eq!(back.cost_source, Some(CostSource::ProviderBilled));
        assert_eq!(back.cost_micro_usd, 2_000);
    }

    #[test]
    fn legacy_receipt_without_cost_source_deserialises_to_none() {
        let receipt = ReceiptBuilder::start("run_x", 1).complete(sample_action(), None, 0.001);
        let mut value = serde_json::to_value(&receipt).unwrap();
        // Written before WS6.B: the key is absent, not null.
        value.as_object_mut().unwrap().remove("cost_source");
        let back: StepReceipt = serde_json::from_value(value).unwrap();
        assert_eq!(back.cost_source, None);
        assert_eq!(back.cost_micro_usd, 1_000);
    }

    #[test]
    fn legacy_checkpoint_without_budget_fields_deserialises() {
        let run_id: RunKind = Id::new();
        let checkpoint = AgentRunCheckpoint {
            policy_mode: None,
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
            persist_profile: false,
            profile_scope: BrowserProfileScope::Ephemeral,
            viewport: None,
            zdr: ZdrMode::Off,
            grant_id: None,
            status: AgentRunStatus::Active,
            control_mode: BrowserControlMode::AgentControl,
            control_epoch: 0,
            next_step: 2,
            budget: Some(BudgetCheckpoint {
                class: "agent".into(),
                steps: 2,
                attempts: 2,
                spent_micro_usd: 2_000,
                active_ms: 900,
                segment: 0,
                exhausted: None,
                claim: None,
            }),
        };
        let mut value = serde_json::to_value(&checkpoint).unwrap();
        // A checkpoint written before budgets existed has no `budget` key at all.
        value.as_object_mut().unwrap().remove("budget");
        let back: AgentRunCheckpoint = serde_json::from_value(value).unwrap();
        assert!(back.budget.is_none());
        assert_eq!(back.step, 2);
        // A partial ledger (fields absent) reads with defaults, not an error.
        let partial: BudgetCheckpoint = serde_json::from_value(serde_json::json!({"steps": 3})).unwrap();
        assert_eq!((partial.steps, partial.attempts, partial.spent_micro_usd), (3, 0, 0));
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
                    control_epoch: 1,
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

#[cfg(test)]
mod human_receipt_tests {
    use super::*;

    #[test]
    fn timeline_decode_skips_unknown_kind() {
        let future = serde_json::json!({"kind": "from_a_later_release", "x": 1});
        assert!(decode_timeline_event_body(future).unwrap().is_none());
        let known = serde_json::json!({"kind": "lifecycle", "state": "closed"});
        assert!(matches!(
            decode_timeline_event_body(known).unwrap(),
            Some(BrowserTimelineEventKind::Lifecycle { .. })
        ));
        // A known kind with a malformed body is still an error, not a silent skip.
        let broken = serde_json::json!({"kind": "lifecycle"});
        assert!(decode_timeline_event_body(broken).is_err());
    }

    #[test]
    fn human_receipt_has_no_observation_and_redacted_text() {
        let receipt = ReceiptBuilder::start("run_x", 1)
            .initiator(BrowserTimelineInitiator::Human, 0)
            .complete_metadata_only(
                AgentAction::Type {
                    selector: "#pw".to_owned(),
                    text: "hunter2".to_owned(),
                },
                0.0,
            );
        assert!(receipt.observation.is_none());
        assert!(matches!(
            receipt.action,
            AgentAction::Type { ref text, .. } if text == "[redacted]"
        ));
        assert_eq!(receipt.initiated_by, Some(BrowserTimelineInitiator::Human));
    }

    #[test]
    fn human_receipt_navigate_drops_query() {
        let receipt = ReceiptBuilder::start("run_x", 1).initiator(BrowserTimelineInitiator::Human, 0).complete_metadata_only(
            AgentAction::Navigate {
                url: "https://example.com/a?session=abc".to_owned(),
            },
            0.0,
        );
        assert!(matches!(
            receipt.action,
            AgentAction::Navigate { ref url } if url == "https://example.com/a"
        ));
    }
}

#[cfg(test)]
mod redaction_tests {
    use super::*;
    use quarry_core::contracts::{ActionOutcome, BrowserTelemetry};
    use quarry_core::ids::Id;

    // Assembled at run time so no secret-shaped literal exists in the repository.
    fn jwt() -> String {
        format!(
            "{}.{}.{}",
            ["eyJ", "hbGciOiJIUzI1NiJ9"].concat(),
            ["eyJ", "zdWIiOiIxMjM0NTY3ODkwIn0"].concat(),
            ["SflKxwRJSM", "eKKF2QT4fwpMeJf36POk6yJV_adQssw5c"].concat()
        )
    }

    fn password() -> String {
        ["S3cret", "P@ss-0451"].concat()
    }

    fn type_ref(text: &str) -> AgentAction {
        AgentAction::TypeRef {
            snapshot_id: "snap_x".to_owned(),
            generation: 1,
            ref_id: "@e1".to_owned(),
            text: text.to_owned(),
        }
    }

    fn observation() -> BrowserObservation {
        BrowserObservation {
            run_id: Id::new(),
            step: 1,
            url: format!("https://example.test/cb?access_token={}&page=2", jwt()),
            title: Some(format!("Welcome {}", jwt())),
            snapshot: None,
            dom_summary: None,
            screenshot_artifact_id: None,
            visual_observation_artifact_id: None,
            evidence_delta_artifact_id: None,
            console_summary: vec![],
            network_summary: vec![],
            egress_receipts: vec![],
            dialogs: vec![],
            policy_denials: vec![],
            action_outcome: ActionOutcome::verified("typed"),
            observation_delta: None,
            challenge: None,
            extraction_profile: None,
            extraction_result: None,
            proof_bundle: None,
            target_resolution: None,
            telemetry: BrowserTelemetry::default(),
            redaction: None,
            observed_at: Utc::now(),
        }
    }

    fn text_of(action: &AgentAction) -> &str {
        match action {
            AgentAction::TypeRef { text, .. } | AgentAction::Type { text, .. } => text,
            other => panic!("not a typing action: {other:?}"),
        }
    }

    #[test]
    fn complete_masks_action_and_observation() {
        let receipt = ReceiptBuilder::start("run_x", 1)
            .redaction_mode(Mode::Enforce)
            .record_policy(RecordPolicy::SensitiveOnly)
            .typed_field(Some(FieldKind::Password))
            .complete(type_ref(&password()), Some(observation()), 0.0);
        assert_eq!(text_of(&receipt.action), "[redacted:password]");
        let stored = serde_json::to_string(&receipt).unwrap();
        assert!(!stored.contains(&password()));
        assert!(!stored.contains("eyJ"));
        let masked = receipt.observation.as_ref().expect("observation is kept");
        assert!(masked.url.contains("example.test/cb"));
        assert!(masked.url.contains("page=2"));
        assert!(masked.title.as_deref().unwrap().starts_with("Welcome "));
    }

    #[test]
    fn agent_receipts_keep_benign_text_under_the_sensitive_policy() {
        let receipt = ReceiptBuilder::start("run_x", 1)
            .redaction_mode(Mode::Enforce)
            .record_policy(RecordPolicy::SensitiveOnly)
            .typed_field(Some(FieldKind::Unknown))
            .complete(type_ref("wireless headphones"), None, 0.0);
        assert_eq!(text_of(&receipt.action), "wireless headphones");
        let strict = ReceiptBuilder::start("run_x", 1)
            .redaction_mode(Mode::Enforce)
            .record_policy(RecordPolicy::AllTyped)
            .typed_field(Some(FieldKind::Unknown))
            .complete(type_ref("wireless headphones"), None, 0.0);
        assert_eq!(text_of(&strict.action), "[redacted:secret]");
    }

    #[test]
    fn fail_receipt_masks_typed_text_from_the_pre_action_snapshot() {
        let error = QuarryError::new(
            ErrorCode::BadRequest,
            format!("the page rejected {}", jwt()),
        );
        let receipt = ReceiptBuilder::start("run_x", 1)
            .redaction_mode(Mode::Enforce)
            .record_policy(RecordPolicy::SensitiveOnly)
            .typed_field(Some(FieldKind::Password))
            .fail(type_ref(&password()), &error);
        assert_eq!(text_of(&receipt.action), "[redacted:password]");
        let StepOutcome::Failed { message, .. } = &receipt.outcome else {
            panic!("not a failure");
        };
        assert!(message.starts_with("the page rejected "));
        assert!(!message.contains("eyJ"));
        assert_eq!(receipt.redaction_rule_version.as_deref(), Some(RULE_VERSION));
    }

    #[test]
    fn a_skipped_typing_action_is_masked_too() {
        let receipt = ReceiptBuilder::start("run_x", 1)
            .redaction_mode(Mode::Enforce)
            .typed_field(Some(FieldKind::Password))
            .skip(type_ref(&password()), "user_took_over");
        assert_eq!(text_of(&receipt.action), "[redacted:password]");
        assert_eq!(receipt.redaction_rule_version.as_deref(), Some(RULE_VERSION));
    }

    fn key_of(action: &AgentAction) -> &str {
        match action {
            AgentAction::Press { key } => key,
            other => panic!("not a key press: {other:?}"),
        }
    }

    #[test]
    fn a_password_typed_key_by_key_is_masked_in_every_receipt() {
        // D2-065: the run last focused a password control, then pressed each key.
        for (outcome, key) in password().chars().map(|c| ("complete", c.to_string())).chain([("fail", "x".to_owned()), ("skip", "y".to_owned())]) {
            let builder = ReceiptBuilder::start("run_x", 1)
                .redaction_mode(Mode::Enforce)
                .record_policy(RecordPolicy::SensitiveOnly)
                .typed_field(Some(FieldKind::Password));
            let action = AgentAction::Press { key: key.clone() };
            let receipt = match outcome {
                "complete" => builder.complete(action, None, 0.0),
                "fail" => builder.fail(action, &QuarryError::new(ErrorCode::BadRequest, "no")),
                _ => builder.skip(action, "user_took_over"),
            };
            assert_eq!(key_of(&receipt.action), "[redacted:password]", "{outcome} {key:?}");
            assert!(!serde_json::to_string(&receipt).unwrap().contains(&format!("\"key\":\"{key}\"")));
        }
    }

    #[test]
    fn a_key_press_with_an_unknown_focus_is_masked_and_a_named_key_is_kept() {
        let unknown = ReceiptBuilder::start("run_x", 1)
            .redaction_mode(Mode::Enforce)
            .record_policy(RecordPolicy::SensitiveOnly)
            .complete(AgentAction::Press { key: "p".to_owned() }, None, 0.0);
        assert_eq!(key_of(&unknown.action), "[redacted:secret]");
        let enter = ReceiptBuilder::start("run_x", 1)
            .redaction_mode(Mode::Enforce)
            .record_policy(RecordPolicy::SensitiveOnly)
            .complete(AgentAction::Press { key: "Enter".to_owned() }, None, 0.0);
        assert_eq!(key_of(&enter.action), "Enter");
        // An ordinary control keeps its characters under `sensitive`, not under `all`.
        let search = ReceiptBuilder::start("run_x", 1)
            .redaction_mode(Mode::Enforce)
            .record_policy(RecordPolicy::SensitiveOnly)
            .typed_field(Some(FieldKind::Unknown))
            .complete(AgentAction::Press { key: "s".to_owned() }, None, 0.0);
        assert_eq!(key_of(&search.action), "s");
        let strict = ReceiptBuilder::start("run_x", 1)
            .redaction_mode(Mode::Enforce)
            .record_policy(RecordPolicy::AllTyped)
            .typed_field(Some(FieldKind::Unknown))
            .complete(AgentAction::Press { key: "s".to_owned() }, None, 0.0);
        assert_eq!(key_of(&strict.action), "[redacted:secret]");
    }

    #[test]
    fn a_legacy_key_press_is_masked_on_read() {
        for stamp in [None, Some("qr-redact/3".to_owned())] {
            let mut receipt = ReceiptBuilder::start("run_x", 1)
                .redaction_mode(Mode::Off)
                .complete(AgentAction::Press { key: "k".to_owned() }, None, 0.0);
            receipt.redaction_rule_version = stamp.clone();
            receipt.redact_for_read_with(Mode::Enforce, RecordPolicy::SensitiveOnly);
            assert_eq!(key_of(&receipt.action), "[redacted:secret]", "stamp {stamp:?}");
        }
    }

    #[test]
    fn receipt_stamp_is_set() {
        let enforcing = ReceiptBuilder::start("run_x", 1)
            .redaction_mode(Mode::Enforce)
            .complete(type_ref("x"), None, 0.0);
        assert_eq!(enforcing.redaction_rule_version.as_deref(), Some(RULE_VERSION));
        // Masking that did not run is not claimed.
        for mode in [Mode::Off, Mode::Shadow] {
            let receipt = ReceiptBuilder::start("run_x", 1)
                .redaction_mode(mode)
                .complete(type_ref("x"), None, 0.0);
            assert_eq!(receipt.redaction_rule_version, None);
        }
        // A person's metadata-only receipt is final as written.
        let human = ReceiptBuilder::start("run_x", 1)
            .initiator(BrowserTimelineInitiator::Human, 0)
            .complete_metadata_only(type_ref(&password()), 0.0);
        assert_eq!(human.redaction_rule_version.as_deref(), Some(RULE_VERSION));
        assert_eq!(text_of(&human.action), "[redacted]");
    }

    #[test]
    fn the_stamp_is_serialised_and_a_legacy_receipt_without_it_still_reads() {
        let stamped = ReceiptBuilder::start("run_x", 1)
            .redaction_mode(Mode::Enforce)
            .complete(type_ref("x"), None, 0.0);
        let mut json = serde_json::to_value(&stamped).unwrap();
        assert_eq!(json["redaction_rule_version"], RULE_VERSION);
        json.as_object_mut().unwrap().remove("redaction_rule_version");
        let legacy: StepReceipt = serde_json::from_value(json).unwrap();
        assert_eq!(legacy.redaction_rule_version, None);
    }

    #[test]
    fn legacy_receipt_is_masked_on_read_and_idempotent() {
        // Written before the stamp existed: raw typed text, raw page text.
        let mut receipt = ReceiptBuilder::start("run_x", 1)
            .redaction_mode(Mode::Off)
            .complete(type_ref(&password()), Some(observation()), 0.0);
        assert_eq!(receipt.redaction_rule_version, None);
        assert_eq!(text_of(&receipt.action), password());

        receipt.redact_for_read_with(Mode::Enforce, RecordPolicy::SensitiveOnly);
        // The control kind is unknown, so the typed text is masked as a secret.
        assert_eq!(text_of(&receipt.action), "[redacted:secret]");
        let stored = serde_json::to_string(&receipt).unwrap();
        assert!(!stored.contains(&password()));
        assert!(!stored.contains("eyJ"));
        assert_eq!(receipt.redaction_rule_version.as_deref(), Some(RULE_VERSION));

        let once = serde_json::to_value(&receipt).unwrap();
        receipt.redact_for_read_with(Mode::Enforce, RecordPolicy::SensitiveOnly);
        assert_eq!(serde_json::to_value(&receipt).unwrap(), once);
    }

    #[test]
    fn a_receipt_stamped_with_an_older_version_keeps_its_benign_text_on_read() {
        let mut receipt = ReceiptBuilder::start("run_x", 1)
            .redaction_mode(Mode::Enforce)
            .typed_field(Some(FieldKind::Unknown))
            .complete(type_ref("wireless headphones"), None, 0.0);
        receipt.redaction_rule_version = Some("qr-redact/1".to_owned());
        receipt.redact_for_read_with(Mode::Enforce, RecordPolicy::SensitiveOnly);
        assert_eq!(text_of(&receipt.action), "wireless headphones");
        assert_eq!(receipt.redaction_rule_version.as_deref(), Some(RULE_VERSION));
    }

    #[test]
    fn the_observation_pass_masks_every_leaf_and_counts() {
        let raw = observation();
        let mut tally = Tally::new();
        let masked = redact_observation_durable(&raw, Mode::Enforce, &mut tally)
            .expect("an observation of this shape rebuilds");
        let json = serde_json::to_string(&masked).unwrap();
        assert!(!json.contains("eyJ"));
        assert!(!tally.is_empty());
    }
}
