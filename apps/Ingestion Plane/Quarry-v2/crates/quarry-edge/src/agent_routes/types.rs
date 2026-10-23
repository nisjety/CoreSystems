use super::*;

/// A live agent run: the leased browser session plus the per-run observation
/// context (carries `step`/current_url across `/step` calls) and budget.
pub struct RunEntry {
    pub run_id: RunKind,
    pub org_id: String,
    /// Signed user/service actor that started this run. The browser edge
    /// scopes live-session reads and mutations to this actor as well as
    /// the tenant; an org membership alone is not browser-run authority.
    pub actor_id: String,
    pub session: BrowserSession,
    pub ctx: ObservationContext,
    pub lease: BrowserLease,
    pub constraints: AgentConstraints,
    pub zdr: ZdrMode,
    pub grant_id: Option<String>,
    pub control_mode: BrowserControlMode,
    /// The control epoch as of the last change this entry saw. The authority is
    /// `RunLive`; this copy is what a checkpoint and a projection are built from.
    pub control_epoch: u64,
    pub profile_scope: BrowserProfileScope,
    pub execution_tier: BrowserExecutionTier,
    /// Current live observation. It is intentionally process-local for a
    /// ZDR run, while non-ZDR proof remains in immutable receipts.
    pub last_observation: Option<BrowserObservation>,
    /// A sensitive approval grant can authorize one irreversible action
    /// only. It is deliberately process-local: a resumed run must obtain
    /// fresh authority instead of replaying a prior approval.
    pub used_sensitive_approval_grants: HashSet<String>,
    /// Steps, attempts, active time and estimated cost against the run's
    /// bounds (`crate::budget`). `None` when enforcement is `off`.
    pub budget: Option<crate::budget::RunBudget>,
    /// Set when the run is being closed, so an action that was waiting for the
    /// lock is refused instead of running on a session that is going away.
    pub closed: bool,
    /// The control epoch for which a refused agent action already left its one
    /// `skipped` receipt (one per takeover, not one per refused call).
    pub last_rejected_epoch: Option<u64>,
    /// Who opened each tab a call through the tab routes opened (tab id to opener).
    /// Process-local, ids and classes only, bounded by the live tabs and, for the
    /// full records, by a cap that degrades to a class-only `unattributed` entry
    /// instead of dropping the tab (see `tabs::remember_tab_opener`).
    /// A tab the browser opened itself (the first one, a popup) has no entry.
    pub tab_origins: HashMap<String, TabOrigin>,
    /// The navigation authority this run holds (WS8.B): its own allow-list, or open
    /// web minus the broker's denied list for a person's `human_browse` run.
    pub nav_policy: super::human_browse::RunNavPolicy,
}

/// Who opened a tab: the initiator class and, for an agent, which one. The owner
/// projection carries it on the tab as `openedBy`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabOrigin {
    pub initiated_by: BrowserTimelineInitiator,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentIdentity>,
    /// True when the run already held the most full records it keeps, so only the
    /// class was kept and the agent's ids were dropped. The tab did have an opener
    /// through the tab routes; which agent it was is not recorded.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub unattributed: bool,
}

/// run_id → handle. Outer std-Mutex guards the map (held only for the O(1)
/// get/insert/remove, never across an await); the handle's tokio-Mutex
/// serializes steps within a single run across await points, and its `RunLive`
/// is readable and changeable without that lock.
pub type AgentRuns = Arc<RunRegistry>;

/// A registry with no caps and no sweeper (tests and tools).
#[must_use]
pub fn new_runs() -> AgentRuns {
    Arc::new(RunRegistry::default())
}

/// The registry the edge runs with: capped and swept per `limits`.
#[must_use]
pub fn new_runs_with(limits: crate::run_limits::RunLimits) -> AgentRuns {
    Arc::new(RunRegistry::new(limits))
}

#[derive(Debug, Deserialize)]
pub struct StartRunBody {
    // quarry-core `AgentConstraints` has no `Default` impl, so supply one
    // via a serde default fn (unbounded budget when the caller omits it).
    #[serde(default = "default_constraints")]
    pub constraints: AgentConstraints,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub persist_profile: bool,
    /// The effective scope is provided by the authenticated BFF after it
    /// validates the requested profile. It remains owner state so reads
    /// and resume paths never depend on a gateway-local session cache.
    #[serde(default)]
    pub profile_scope: BrowserProfileScope,
    #[serde(default)]
    pub viewport: Option<BrowserViewport>,
    #[serde(default)]
    pub zdr: bool,
    /// BrowserBroker's signed capability. Quarry revalidates it before
    /// every action, so an approval cannot be replayed after expiry.
    #[serde(default)]
    pub grant_id: Option<String>,
    /// Resume a previously checkpointed run after an edge restart. The
    /// checkpoint's tenant, constraints, profile, and current step remain
    /// authoritative; a new/renewed grant may be supplied explicitly.
    #[serde(default)]
    pub resume_run_id: Option<String>,
    /// Who is driving this run: `"agent"` marks it as a model-driven run.
    /// Tighten-only: it can make a run an agent run (budgeted), never the
    /// reverse; a service principal is an agent run whatever it sends.
    #[serde(default, alias = "initiatedBy")]
    pub initiated_by: Option<String>,
    /// Optional requirements for the selected execution driver. Quarry
    /// fails closed instead of silently beginning a run with a driver that
    /// cannot supply a capability the caller needs.
    #[serde(default)]
    pub driver_requirements: BrowserDriverRequirements,
    /// `ephemeral_evidence` is an opt-in experimental contract for public,
    /// disposable evidence acquisition. It is never an alias for a normal
    /// authenticated browser run.
    #[serde(default)]
    pub execution_tier: BrowserExecutionTier,
    /// WS7.D: the caller's web-policy ask. Tighten-only: it can disable a capability, lower a
    /// ceiling or narrow the hosts, and never loosens the operator baseline or the ZDR floor.
    /// Resolved once at start. A resumed run does not carry it (see the W2-U29 depth notes).
    #[serde(default)]
    pub policy: Option<quarry_core::web_policy::RequestAsk>,
}

/// Browser execution contract selected at run creation. The renderer
/// implementation is intentionally separate from the contract: the Lite
/// experiment currently transparently falls back to Chromium while a
/// lightweight renderer earns capability evidence on its own corpus.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserExecutionTier {
    #[default]
    Chromium,
    EphemeralEvidence,
}

impl BrowserExecutionTier {
    pub(super) const fn is_ephemeral_evidence(self) -> bool {
        matches!(self, Self::EphemeralEvidence)
    }

    pub(super) const fn engine_label(self) -> &'static str {
        match self {
            Self::Chromium => "chromium",
            // Do not imply a Boa/Blitz renderer exists merely because its
            // contract is exposed. This run is a constrained Chromium
            // fallback until a Lite engine supplies independent evidence.
            Self::EphemeralEvidence => "chromium_fallback",
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct BrowserDriverRequirements {
    #[serde(default)]
    pub persistent_profile: bool,
    #[serde(default)]
    pub devtools_trace: bool,
    #[serde(default)]
    pub downloads_to_artifacts: bool,
    #[serde(default)]
    pub uploads_from_artifacts: bool,
    #[serde(default)]
    pub full_visual_fidelity: bool,
    #[serde(default)]
    pub isolated_egress: bool,
    #[serde(default)]
    pub security_evidence: bool,
    #[serde(default)]
    pub atomic_target_actions: bool,
}

pub(super) fn default_constraints() -> AgentConstraints {
    AgentConstraints {
        max_steps: 0,
        allowed_domains: Vec::new(),
        max_runtime_s: None,
        max_cost_usd: None,
    }
}

#[derive(Debug, Serialize)]
pub struct StartRunData {
    pub run_id: String,
    pub lease_id: String,
    pub profile_id: String,
    /// What the selected driver can actually supply for this lease. This
    /// is evidence-backed routing metadata, not a promise inferred from a
    /// provider name.
    pub driver_capabilities: BrowserDriverCapabilities,
    pub execution_tier: BrowserExecutionTier,
    /// Actual engine used for this run. `chromium_fallback` is explicit so
    /// callers never infer Lite throughput/fidelity from a request alone.
    pub execution_engine: &'static str,
    /// The bounds this run runs under, after defaults and ceilings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constraints: Option<crate::budget::EffectiveConstraints>,
    /// Requested values that were replaced, and by what.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub clamped: Vec<crate::budget::ClampView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSessionProjectionData {
    pub run_id: String,
    pub lease_id: String,
    pub profile_id: String,
    pub profile_storage: &'static str,
    pub profile_scope: BrowserProfileScope,
    pub viewport: Option<BrowserViewport>,
    pub current_url: String,
    pub step: u32,
    pub status: AgentRunStatus,
    pub live: bool,
    pub zdr: bool,
    pub control_mode: BrowserControlMode,
    pub control_epoch: u64,
    pub execution_tier: BrowserExecutionTier,
    pub tabs: Vec<BrowserSessionTab>,
    pub last_observation: Option<BrowserObservation>,
    /// The BrowserBroker session grant the run was started with. Internal:
    /// the projection is owner-gated, and the Verevon gateway reads it to
    /// derive an AI run's grant but never forwards it to the SPA (WS1.B).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSessionTab {
    pub tab_id: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub active: bool,
    /// Who opened the tab, for a tab opened through the tab routes. Absent for a
    /// tab the browser opened itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opened_by: Option<TabOrigin>,
}

#[derive(Debug, Deserialize)]
pub struct StepBody {
    pub action: AgentAction,
    /// Who initiated the action. Can only tighten: `agent` on a person's token
    /// makes this call an agent's; a service principal is an agent regardless.
    /// `initiatedBy` is accepted as well.
    #[serde(default, alias = "initiatedBy")]
    pub initiated_by: Option<InitiatorHint>,
    /// The AI run the caller says it acts for. Advisory (see `AdvisoryAgent`).
    #[serde(default, alias = "aiRunId")]
    pub ai_run_id: Option<String>,
    /// A short label the caller gave its agent. Advisory (see `AdvisoryAgent`).
    #[serde(default, alias = "agentLabel")]
    pub agent_label: Option<String>,
    #[serde(default)]
    pub instruction: Option<String>,
    #[serde(default)]
    pub extraction_profile: Option<ExtractionProfile>,
}

impl StepBody {
    #[must_use]
    pub fn advisory_agent(&self) -> AdvisoryAgent {
        AdvisoryAgent::new(self.ai_run_id.as_deref(), self.agent_label.as_deref())
    }
}

#[derive(Debug, Deserialize)]
pub struct BrowserSessionControlBody {
    pub mode: BrowserControlMode,
    /// Compare-and-set: refuse with 409 when the run's control epoch is not this.
    #[serde(default)]
    pub expected_epoch: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct BrowserTimelineQuery {
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserTimelineData {
    pub items: Vec<BrowserTimelineItem>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserTimelineItem {
    pub id: String,
    pub occurred_at: String,
    #[serde(flatten)]
    pub detail: BrowserTimelineItemDetail,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserTimelineItemDetail {
    Action {
        action: &'static str,
        outcome: &'static str,
        error_code: Option<String>,
        #[serde(rename = "initiatedBy", skip_serializing_if = "Option::is_none")]
        initiated_by: Option<BrowserTimelineInitiator>,
        #[serde(skip_serializing_if = "Option::is_none")]
        agent: Option<AgentIdentity>,
        #[serde(rename = "controlEpoch", skip_serializing_if = "Option::is_none")]
        control_epoch: Option<u64>,
    },
    Control {
        mode: BrowserControlMode,
        #[serde(rename = "initiatedBy")]
        initiated_by: BrowserTimelineInitiator,
        #[serde(rename = "controlEpoch")]
        control_epoch: u64,
    },
    Tab {
        operation: BrowserTabOperation,
        tab_id: Option<String>,
        active_tab_id: Option<String>,
        #[serde(rename = "initiatedBy", skip_serializing_if = "Option::is_none")]
        initiated_by: Option<BrowserTimelineInitiator>,
        #[serde(skip_serializing_if = "Option::is_none")]
        agent: Option<AgentIdentity>,
        #[serde(rename = "controlEpoch")]
        control_epoch: u64,
    },
    Devtools {
        event_count: u32,
        last_sequence: u64,
    },
    Lifecycle {
        state: BrowserTimelineLifecycle,
        #[serde(rename = "initiatedBy", skip_serializing_if = "Option::is_none")]
        initiated_by: Option<BrowserTimelineInitiator>,
    },
}

#[derive(Debug, Deserialize)]
pub struct CompileProcedureBody {
    /// Stable caller-owned identifier for the compiled candidate. The
    /// procedure is still only a deterministic replay candidate; it is
    /// never silently promoted to an autonomous action policy.
    #[serde(default)]
    pub procedure_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReplayCheckBody {
    pub procedure: BrowserProcedure,
    #[serde(default)]
    pub actions: Vec<AgentAction>,
}

#[derive(Debug, Deserialize)]
pub struct ProcedureImpactBody {
    pub procedure: BrowserProcedure,
    #[serde(default)]
    pub changed_urls: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveFrameData {
    pub mime_type: String,
    pub data_base64: String,
    pub zdr: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveFrameStreamData {
    pub sequence: u64,
    pub mime_type: String,
    pub data_base64: String,
    pub zdr: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserTabsData {
    pub tabs: Vec<BrowserTab>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserTabData {
    pub tab: BrowserTab,
    pub tabs: Vec<BrowserTab>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevtoolsQuery {
    #[serde(default)]
    pub after_sequence: Option<u64>,
    #[serde(default)]
    pub limit: Option<usize>,
    /// Comma-separated console levels (WS9.C): `debug`, `info`, `log`, `warn`,
    /// `warning`, `error`, `verbose`; at most five. Absent means every level.
    #[serde(default)]
    pub levels: Option<String>,
    /// Case-insensitive substring over `text`, `url` and `name` only; never `payload`.
    #[serde(default)]
    pub filter: Option<String>,
    /// `console`, `network` or `lifecycle`.
    #[serde(default)]
    pub category: Option<String>,
    /// A tab id as the driver reports it (`[A-Za-z0-9_-]{1,64}`).
    #[serde(default)]
    pub tab_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevtoolsEventsData {
    pub events: Vec<BrowserDevtoolsEvent>,
    /// The sequence of the last event this read examined (WS9.C). Pass it back
    /// as `afterSequence` to continue past the scanned window, even when no
    /// event matched the filter. Equals the request cursor when nothing was scanned.
    pub next_sequence: u64,
    pub zdr: bool,
}

/// Bounded, redacted decisions made by the browser egress boundary. This
/// contains neither resolved IPs nor request secrets; it is safe to read
/// during a live ZDR session but is never a substitute for the immutable
/// step receipt retained under the run's ZDR policy.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EgressReceiptsData {
    pub receipts: Vec<BrowserEgressReceipt>,
    pub zdr: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTabBody {
    #[serde(default)]
    pub url: Option<String>,
    /// `initiatedBy` and `initiated_by` are both accepted (the wire spelling is
    /// camelCase; `/step` has always taken snake_case).
    #[serde(default, alias = "initiated_by")]
    pub initiated_by: Option<InitiatorHint>,
    /// The AI run the caller says it acts for. Advisory (see `AdvisoryAgent`).
    #[serde(default, alias = "ai_run_id")]
    pub ai_run_id: Option<String>,
    /// A short label the caller gave its agent. Advisory (see `AdvisoryAgent`).
    #[serde(default, alias = "agent_label")]
    pub agent_label: Option<String>,
}

impl NewTabBody {
    #[must_use]
    pub fn advisory_agent(&self) -> AdvisoryAgent {
        AdvisoryAgent::new(self.ai_run_id.as_deref(), self.agent_label.as_deref())
    }
}

/// The query of a tab select or close (neither has a body): who initiated the call.
/// Same rule as `/step`: the hint can only tighten. `agent` on a person's token makes
/// the call an agent's, so it is refused during a take-over; a service principal is an
/// agent whatever it sends. A value that is not `human` or `agent` is a 400, never
/// read as the person's.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabCallQuery {
    #[serde(default, alias = "initiated_by")]
    pub initiated_by: Option<InitiatorHint>,
    /// The AI run the caller says it acts for. Advisory (see `AdvisoryAgent`).
    #[serde(default, alias = "ai_run_id")]
    pub ai_run_id: Option<String>,
    /// A short label the caller gave its agent. Advisory (see `AdvisoryAgent`).
    #[serde(default, alias = "agent_label")]
    pub agent_label: Option<String>,
}

impl TabCallQuery {
    #[must_use]
    pub fn advisory_agent(&self) -> AdvisoryAgent {
        AdvisoryAgent::new(self.ai_run_id.as_deref(), self.agent_label.as_deref())
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BrowserWsClientMessage {
    Action {
        action: AgentAction,
        #[serde(default)]
        instruction: Option<String>,
        #[serde(default, alias = "initiated_by", alias = "initiatedBy")]
        actor: Option<InitiatorHint>,
    },
    Ping,
}

pub(super) type ApiErr = (StatusCode, Json<Envelope<()>>);

pub(super) fn status_err(status: StatusCode, request_id: &str, msg: &str) -> ApiErr {
    (
        status,
        Json(Envelope::<()>::err(
            request_id,
            QuarryError::new(ErrorCode::BadRequest, msg),
        )),
    )
}

/// The 404 for a run id this edge no longer holds.
///
/// A run whose browser the sweeper (or a restart) released keeps an active
/// checkpoint unless it ran under ZDR. When the caller owns such a checkpoint
/// the 404 says so and names the id to pass as `resume_run_id`, so a client
/// can tell "resume this" from "this never existed". Anyone else, and every ZDR
/// run, gets the same plain 404 as before: the hint must not reveal that
/// somebody else's run exists.
pub(super) async fn run_gone_err(
    state: &AppState,
    claims: &crate::auth::Claims,
    request_id: &str,
    run_id: &str,
) -> ApiErr {
    if !run_is_resumable(state, claims, run_id).await {
        return status_err(StatusCode::NOT_FOUND, request_id, "agent run not found");
    }
    run_not_live_err(request_id, run_id, true)
}

/// Whether the caller owns an active, non-ZDR checkpoint for `run_id`, i.e. may
/// start a run with it as `resume_run_id`. An explicitly closed run's checkpoint is
/// `closed`, so it is not resumable.
async fn run_is_resumable(state: &AppState, claims: &crate::auth::Claims, run_id: &str) -> bool {
    match run_id.parse::<RunKind>() {
        Ok(parsed) => matches!(
            state.receipts.load_run_checkpoint(&claims.org_id, &parsed).await,
            Ok(Some(checkpoint))
                if checkpoint.status == AgentRunStatus::Active
                    && !checkpoint.zdr.is_active()
                    && caller_owns_browser_run(&checkpoint.org_id, &checkpoint.actor_id, claims)
        ),
        Err(_) => false,
    }
}

/// The 404 `run_not_live`, with the resume hint when the run can be resumed.
fn run_not_live_err(request_id: &str, run_id: &str, resumable: bool) -> ApiErr {
    let details = if resumable {
        serde_json::json!({
            "reason": "run_not_live",
            "resumable": true,
            "resume_run_id": run_id,
        })
    } else {
        serde_json::json!({ "reason": "run_not_live" })
    };
    (
        StatusCode::NOT_FOUND,
        Json(Envelope::<()>::err(
            request_id,
            QuarryError::new(ErrorCode::NotFound, "agent run is not live").with_details(details),
        )),
    )
}

/// The 404 for a call that was queued on the run's lock when the run was closed
/// under it (the sweeper released it, or its owner closed it). The caller owns the
/// run (checked before this), so the answer is `run_not_live` as for a run already
/// gone; it carries the resume hint only when the checkpoint is still active: an
/// expiry leaves it so, an explicit close does not.
pub(super) async fn run_closed_err(
    state: &AppState,
    claims: &crate::auth::Claims,
    request_id: &str,
    run_id: &str,
) -> ApiErr {
    run_not_live_err(
        request_id,
        run_id,
        run_is_resumable(state, claims, run_id).await,
    )
}

pub(super) fn driver_err(request_id: &str, err: QuarryError) -> ApiErr {
    // WS12.S2: a driver or target error can echo page text; it leaves the process masked.
    let err = err.redacted();
    (
        StatusCode::from_u16(err.code.http_status())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        Json(Envelope::<()>::err(request_id, err)),
    )
}

/// What a drain did to the live runs.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct DrainReport {
    /// Sessions released.
    pub released: usize,
    /// Releases that failed or timed out (the browser dies with the process).
    pub failed: usize,
    /// Runs an action was still running on when the per-run deadline passed; they are
    /// left to the exit. (A request that merely holds a reference to a run does not
    /// count: its session is released.)
    pub in_use: usize,
}
