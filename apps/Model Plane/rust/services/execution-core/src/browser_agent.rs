//! Browser-agent action/observation loop (MP-03).
//!
//! Implements the planner side of the browser-agent protocol defined in
//! `browser_agent.proto`. The loop:
//!   1. Initializes an `AgentPlan` with constraints (`max_steps`, `max_runtime_s`, `allowed_domains`).
//!   2. Calls the planner to produce the next `BrowserAction`.
//!   3. Waits for a `BrowserObservation` from Quarry.
//!   4. Evaluates stop criteria and budget/step limits.
//!   5. Repeats until completion, failure, or abort.

use std::collections::{BTreeSet, HashMap};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tracing::{info, warn};

mod browser_risk;
pub mod admission;
pub mod failure;
pub mod metrics;
pub mod planner_view;
pub mod procedure;

use failure::{ActionFailure, BrowserFailureKind, FailureBudget, FailureVerdict, NoProgressDetector};

pub(crate) use browser_risk::{approval_poll_interval, approval_timeout_seconds};
use browser_risk::{classify_action_risk_with_focus, risk_reason};
pub use browser_risk::{ApprovalOutcome, RiskCategory, RiskyActionDetail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanStatus {
    Planning,
    Executing,
    WaitingApproval,
    Completed,
    Failed,
    /// User cancelled the run mid-loop (Phase 2 B5), or the loop was aborted
    /// for another non-planner reason. Distinct from `Failed` — an abort is
    /// user/operator-initiated, not an execution error.
    Aborted,
}

/// Why a browser plan stopped without completing its goal.
///
/// This stays structured through the tool bridge and runtime boundary. A plain
/// `PlanStatus::Aborted` is not sufficient: a user cancellation, a denied
/// approval, and an approval timeout must never be reclassified as a
/// successful tool result merely because they all stop the same loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserAbortReason {
    Cancelled,
    ApprovalDenied,
    ApprovalTimedOut,
    /// A future/operator abort that has no more specific semantic reason.
    /// Consumers must still treat this as non-successful.
    Other,
}

/// A hard execution budget reached by the browser agent.
///
/// Resource exhaustion is not a user-visible task completion. It is carried
/// separately from a planner stop criterion so dispatch can expose the exact
/// non-success terminal outcome without parsing a human-readable summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserResourceLimit {
    MaxSteps,
    Runtime,
    /// The planner's token budget (`PlanConfig::max_planner_tokens`).
    Tokens,
    /// The run's credential (delegated bearer) is about to expire
    /// (`PlanConfig::deadline_at`); no new planner call is started.
    Credential,
    /// Quarry answered 402 `BUDGET_EXHAUSTED` for this dimension.
    QuarryBudget(crate::quarry_agent::BudgetDimension),
    /// Quarry (or an upstream) answered 429. Backpressure, never retried here.
    QuarryBackpressure,
}

impl BrowserResourceLimit {
    pub(crate) const fn reason(self) -> &'static str {
        use crate::quarry_agent::BudgetDimension as D;
        match self {
            Self::MaxSteps => "max_steps exceeded",
            Self::Runtime => "max_runtime_s exceeded",
            Self::Tokens => "max_planner_tokens exceeded",
            Self::Credential => "run credential deadline reached",
            Self::QuarryBudget(D::Steps) => "quarry budget exhausted: steps",
            Self::QuarryBudget(D::Attempts) => "quarry budget exhausted: attempts",
            Self::QuarryBudget(D::Runtime) => "quarry budget exhausted: runtime",
            Self::QuarryBudget(D::Cost) => "quarry budget exhausted: cost",
            Self::QuarryBudget(D::OrgAllowance) => "quarry budget exhausted: org_allowance",
            Self::QuarryBudget(D::Other) => "quarry budget exhausted",
            Self::QuarryBackpressure => "quarry is shedding load (429); not retried",
        }
    }
}

/// The resource limit a failed Quarry call stands for, when the classifier
/// says it is one (402 `BUDGET_EXHAUSTED`, 429). `None` for every other error.
#[must_use]
pub fn resource_limit_for_quarry_error(
    error: &crate::quarry_agent::AgentClientError,
) -> Option<BrowserResourceLimit> {
    use crate::quarry_agent::{classify_quarry_error, quarry_budget_dimension, StepFailureClass};
    if classify_quarry_error(error) != StepFailureClass::ResourceLimit {
        return None;
    }
    Some(match quarry_budget_dimension(error) {
        Some(dimension) => BrowserResourceLimit::QuarryBudget(dimension),
        None => BrowserResourceLimit::QuarryBackpressure,
    })
}

/// What the planner spent in one loop, counts only (no content).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BrowserLoopUsage {
    pub planner_calls: u32,
    pub planner_input_tokens: u64,
    pub planner_output_tokens: u64,
}

impl BrowserLoopUsage {
    #[must_use]
    pub const fn planner_tokens(self) -> u64 {
        self.planner_input_tokens + self.planner_output_tokens
    }

    /// `planner_calls=N planner_input_tokens=N planner_output_tokens=N`.
    #[must_use]
    pub fn line(self) -> String {
        format!(
            "planner_calls={} planner_input_tokens={} planner_output_tokens={}",
            self.planner_calls, self.planner_input_tokens, self.planner_output_tokens
        )
    }
}

impl PlanStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Planning => "planning",
            Self::Executing => "executing",
            Self::WaitingApproval => "waiting_approval",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Aborted => "aborted",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Aborted)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionType {
    Goto,
    Click,
    Type,
    Extract,
    Observe,
    Scroll,
    Wait,
    /// Widened verbs (WS11.S4), proposed only with
    /// `QUARRY_BROWSER_AGENT_WIDENED_ACTIONS` on.
    Press,
    Select,
    Back,
    WaitFor,
}

impl ActionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Goto => "goto",
            Self::Click => "click",
            Self::Type => "type",
            Self::Extract => "extract",
            Self::Observe => "observe",
            Self::Scroll => "scroll",
            Self::Wait => "wait",
            Self::Press => "press",
            Self::Select => "select",
            Self::Back => "back",
            Self::WaitFor => "wait_for",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObservationStatus {
    Success,
    Failed,
    Timeout,
    Blocked,
}

impl ObservationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Timeout => "timeout",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone)]
pub struct BrowserAction {
    pub action_id: String,
    pub grant_id: String,
    pub action_type: ActionType,
    pub selector: String,
    pub value: String,
    pub url: String,
    pub max_wait_ms: i32,
    /// Opaque Quarry target binding from the most recent browser observation.
    /// When present for click/type, the Quarry wire client uses the native
    /// snapshot-ref action instead of replaying a CSS selector.
    pub target: Option<BrowserTargetRef>,
    /// The planner's rationale for choosing this action. Empty for the
    /// deterministic fallback (no LLM reason available). Surfaced on the
    /// run-event stream (Phase 2) so the UI can show model reasoning live.
    pub reason: String,
    /// The planner's self-reported risk classification (Phase 5), when the
    /// model provided one. `None` here does NOT mean "not risky" — the
    /// in-loop gate ORs this with a deterministic backstop
    /// (`classify_action_risk`) that runs regardless, so a model that omits
    /// or under-reports risk cannot silently bypass the HITL gate.
    pub risk_category: Option<RiskCategory>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserTargetRef {
    pub snapshot_id: String,
    pub generation: u32,
    pub ref_id: String,
    /// Child-frame identity from the Quarry snapshot. `None` means this is a
    /// top-level target; a child frame is emitted through the dedicated
    /// frame-ref action wire variant rather than downgraded to CSS.
    pub frame_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BrowserObservation {
    pub observation_id: String,
    pub action_id: String,
    pub grant_id: String,
    pub status: ObservationStatus,
    pub page_url: String,
    pub page_title: String,
    pub extracted_text: String,
    pub screenshot_ref: String,
    pub dom_snapshot_ref: String,
    /// Compact, public AX targets from Quarry's active snapshot. These are
    /// opaque evidence handles, never CDP backend node ids or selectors.
    pub snapshot_generation: Option<u32>,
    pub snapshot_targets: Vec<BrowserSnapshotTarget>,
    pub error_message: String,
    /// Quarry's access-state signal for this page. `None` when Quarry sent
    /// none (an older Quarry, or a page that is an ordinary page).
    pub challenge: Option<BrowserChallenge>,
    /// The redaction rule version Quarry stamped on this observation (WS12.S2),
    /// copied from the wire. `None` when Quarry sent no readable stamp, and on
    /// an observation built locally (a refusal). Nothing else of the stamp is
    /// kept.
    pub redaction_rule_version: Option<String>,
}

/// What Quarry says a page's access state is. Every field is a closed
/// vocabulary or a number, never page text, so it can reach the planner prompt
/// without carrying anything the page wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserChallenge {
    pub kind: ChallengeKind,
    pub strength: ChallengeStrength,
    /// Set on a strong signal only.
    pub block_reason: Option<BlockReason>,
    pub recovery: ChallengeRecovery,
    /// Quarry's confidence in thousandths (0 to 1000).
    pub confidence_per_mille: u16,
}

impl BrowserChallenge {
    /// Only a strong signal says a bot wall stands in front of the page.
    #[must_use]
    pub fn is_strong(&self) -> bool {
        self.strength == ChallengeStrength::Strong
    }
}

impl std::fmt::Display for BrowserChallenge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "kind={} strength={} block_reason={} recovery={} confidence_per_mille={}",
            self.kind.as_str(),
            self.strength.as_str(),
            self.block_reason.map_or("none", BlockReason::as_str),
            self.recovery.as_str(),
            self.confidence_per_mille,
        )
    }
}

/// Mirrors Quarry's `ChallengeKind` (`challenge-signal.schema.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeKind {
    Captcha,
    Waf,
    Login,
    Consent,
    RateLimit,
    AccessDenied,
    /// Informational: a paywall is not a bot verdict.
    Paywall,
    /// Informational: a regional or legal restriction is not a bot verdict.
    GeoBlocked,
    Unknown,
}

impl ChallengeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Captcha => "captcha",
            Self::Waf => "waf",
            Self::Login => "login",
            Self::Consent => "consent",
            Self::RateLimit => "rate_limit",
            Self::AccessDenied => "access_denied",
            Self::Paywall => "paywall",
            Self::GeoBlocked => "geo_blocked",
            Self::Unknown => "unknown",
        }
    }

    /// A value outside the closed list reads as `Unknown`, which Quarry's own
    /// list also has, so a newer Quarry never fails the observation.
    #[must_use]
    pub fn from_wire(value: &str) -> Self {
        match value {
            "captcha" => Self::Captcha,
            "waf" => Self::Waf,
            "login" => Self::Login,
            "consent" => Self::Consent,
            "rate_limit" => Self::RateLimit,
            "access_denied" => Self::AccessDenied,
            "paywall" => Self::Paywall,
            "geo_blocked" => Self::GeoBlocked,
            _ => Self::Unknown,
        }
    }
}

/// Mirrors Quarry's `ChallengeStrength`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeStrength {
    Strong,
    Weak,
    /// An ordinary access state (a login form, a consent banner, a paywall, a
    /// rate limit, a bare status), named so it is not mistaken for a bot wall.
    NonSignal,
}

impl ChallengeStrength {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Strong => "strong",
            Self::Weak => "weak",
            Self::NonSignal => "non_signal",
        }
    }

    /// A value outside the closed list reads as `Weak`: a hint worth a look,
    /// never a verdict. A real wall still ends in Quarry's own refusal, so
    /// guessing `Strong` here would only turn an unknown state into a stop.
    #[must_use]
    pub fn from_wire(value: &str) -> Self {
        match value {
            "strong" => Self::Strong,
            "non_signal" => Self::NonSignal,
            _ => Self::Weak,
        }
    }
}

/// Mirrors Quarry's `BlockReason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    CaptchaWidget,
    WafInterstitial,
    /// The same host challenged again after its one recovery was spent.
    ChallengeLoop,
    UnexpectedBotError,
}

impl BlockReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CaptchaWidget => "captcha_widget",
            Self::WafInterstitial => "waf_interstitial",
            Self::ChallengeLoop => "challenge_loop",
            Self::UnexpectedBotError => "unexpected_bot_error",
        }
    }

    /// `None` for a value outside the closed list: a reason is never invented.
    #[must_use]
    pub fn from_wire(value: &str) -> Option<Self> {
        match value {
            "captcha_widget" => Some(Self::CaptchaWidget),
            "waf_interstitial" => Some(Self::WafInterstitial),
            "challenge_loop" => Some(Self::ChallengeLoop),
            "unexpected_bot_error" => Some(Self::UnexpectedBotError),
            _ => None,
        }
    }
}

/// Mirrors Quarry's `ChallengeRecovery`: whether the one wait or reload the
/// planner gets on a strong signal is still there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeRecovery {
    Available,
    Exhausted,
    NotApplicable,
}

impl ChallengeRecovery {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Exhausted => "exhausted",
            Self::NotApplicable => "not_applicable",
        }
    }

    /// A value outside the closed list reads as `NotApplicable`, so the
    /// planner is never told a recovery exists that Quarry did not offer.
    #[must_use]
    pub fn from_wire(value: &str) -> Self {
        match value {
            "available" => Self::Available,
            "exhausted" => Self::Exhausted,
            _ => Self::NotApplicable,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BrowserSnapshotTarget {
    pub ref_id: String,
    pub role: Option<String>,
    pub name: Option<String>,
    pub text: Option<String>,
    pub frame_id: Option<String>,
}

/// Sink for surfacing browser-agent progress as orchestration events (B4).
///
/// The loop calls these per dispatched action and received observation so the
/// run-event stream (`/v1/runs/:id/events`) shows live browser progress. The
/// concrete implementation (in `browser_events.rs`) publishes to session-core's
/// orchestration broadcast via `RecordOrchestrationEvent`.
///
/// Implementations MUST be best-effort: a failure here must never surface to
/// the loop. Errors are swallowed and logged by the implementation, and the
/// loop ignores the returned unit regardless.
///
/// Kept free of proto types so the loop stays unit-testable without a gRPC
/// client; the implementation does the wire translation.
#[tonic::async_trait]
pub trait BrowserEventSink: Send + Sync {
    /// Emit that `action` was dispatched for the run/plan in `config`.
    async fn action_dispatched(&self, config: &PlanConfig, action: &BrowserAction);
    /// Emit that `observation` was received for the run/plan in `config`.
    async fn observation_received(&self, config: &PlanConfig, observation: &BrowserObservation);
    /// Emit that a user paused the run/plan in `config` (Phase 2 B5).
    async fn run_paused(&self, config: &PlanConfig);
    /// Emit that a user resumed the run/plan in `config` (Phase 2 B5).
    async fn run_resumed(&self, config: &PlanConfig);
    /// Request human approval for a risky browser action/condition (Phase 5)
    /// before it proceeds, and block until decided.
    ///
    /// Unlike the telemetry methods above, this is NOT best-effort: it is
    /// the actual gate. Implementations MUST reuse the EXISTING general HITL
    /// approval machinery (`OrchestrationCoreServiceClient::create_approval`
    /// / `get_approval`, the same durable `Approval` record and
    /// `RunPausedForApproval`/`ApprovalStateChanged` events the outer
    /// whole-tool `ask` gate uses) rather than a parallel approval system,
    /// and MUST fail CLOSED: any transport/session-core error, a missing
    /// run id, or the absence of a wired sink is `Denied`, never silently
    /// `Granted`. A caller that never wires a real sink (e.g. a unit test)
    /// gets the same fail-closed behavior via the default in
    /// `gate_risky_action`/`gate_persistent_cookie_use` when `sink` is
    /// `None` — see those functions.
    async fn require_approval(
        &self,
        config: &PlanConfig,
        detail: &RiskyActionDetail,
    ) -> ApprovalOutcome;
    /// Record on the run that its high-risk `ask` to start was satisfied by the
    /// user's broker-validated run grant rather than a durable approval, decided
    /// by `decided_by` (the user the run acts for). Best-effort metadata, never a
    /// gate; never called under ZDR. The default records nothing, so a sink that
    /// keeps no timeline need not implement it.
    async fn run_start_authorized(&self, _run_id: &str, _plan_id: &str, _decided_by: &str) {}
    /// WS8.G: a person took the agent tab of an attached run (`USER_TOOK_OVER`);
    /// the loop waits for the give-back. Ids only. The default records nothing.
    async fn paused_by_user(&self, _config: &PlanConfig, _tab_id: &str) {}
    /// WS8.G: the person gave the agent tab back; the loop goes on.
    async fn resumed_after_takeover(&self, _config: &PlanConfig, _tab_id: &str) {}
    /// WS8.G: an attached run shares the person's cookie jar (the session's
    /// own), recorded once per attachment. Ids only, never a URL or a title.
    async fn attach_shared_jar(
        &self,
        _config: &PlanConfig,
        _session_run_id: &str,
        _attach_id: &str,
        _tab_id: &str,
    ) {
    }
}

#[derive(Debug, Clone, Default)]
pub struct PlanConfig {
    pub plan_id: String,
    pub grant_id: String,
    pub run_id: String,
    pub org_id: String,
    pub system_prompt: String,
    pub max_steps: i32,
    pub max_runtime_s: i32,
    pub allowed_domains: Vec<String>,
    pub stop_criteria: String,
    pub require_approval: bool,
    /// Per-run cost ceiling (USD) forwarded to Quarry's `AgentConstraints`.
    pub max_cost_usd: Option<f64>,
    /// Zero-data-retention: when true, Quarry must not persist page content.
    pub zdr: bool,
    /// Persistent Quarry browser profile to reuse cookie/session state from
    /// (Phase 2). `None` acquires a fresh, isolated Quarry session — the same
    /// default a manual `create_session` gets without an explicit profile.
    pub profile_id: Option<String>,
    /// Where to navigate first (Phase 2). A freshly `start_run`'d Quarry
    /// lease has no page loaded — chromiumoxide fails any observe/click/etc.
    /// with "no current page; call `goto()` first" until something navigates.
    /// The manual `create_session` flow already issues an explicit navigate
    /// right after `start_run`; the AI loop needs the same first step, since
    /// it has no other source of an initial URL (`system_prompt` is free
    /// text, not a target). Typically the URL of the tab the user launched
    /// the run from. `None` keeps the pre-Phase-2 deterministic-`Observe`
    /// first step (e.g. existing unit tests, or a caller that genuinely has
    /// no starting point).
    pub start_url: Option<String>,
    /// Independent success condition for the Verified Outcome Foundation
    /// (verevon-roadmap.md §3b P1 item 3, see `crate::postcondition`).
    ///
    /// **Must not be the same string as `stop_criteria`.** `stop_criteria` is
    /// what makes the loop stop, so checking the final page for it again
    /// would test the agent against its own stop decision and always pass —
    /// a confirmation that proves nothing.
    /// `postcondition::judge_browser_procedure` detects that case and refuses
    /// to confirm, so a caller that sets them equal gets an honest
    /// "inconclusive", not a failure.
    ///
    /// Empty (the default) means the procedure can be **refuted but never
    /// confirmed** — a failed/blocked/timed-out final observation is still
    /// caught, but success cannot be independently established.
    ///
    /// Good values name something only the *completed* effect would put on
    /// the page and that the agent was not steering toward: an order number
    /// pattern, a confirmation-page heading, a receipt id.
    pub postcondition: String,
    /// Token budget for the LLM planner (input + output, all calls). `None`
    /// means no budget; it binds only when a planner is selected.
    pub max_planner_tokens: Option<u64>,
    /// The run's credential deadline. Once it passes, `check_limits` stops the
    /// run before any new planner call. `None` means no deadline.
    pub deadline_at: Option<Instant>,
    /// The org planner switch the gateway wrote (`on`, `off`, `unavailable`);
    /// read by the planner policy (WS11.S2). Any other value is dropped.
    pub planner_flag: Option<String>,
    /// Consecutive failed steps that end an LLM-planned run (0 = default 3).
    pub max_consecutive_failures: u32,
    /// Failed steps in total that end an LLM-planned run (0 = default 8).
    pub max_total_failures: u32,
    /// Which planner prompt procedure the run uses (`QUARRY_BROWSER_AGENT_PROCEDURE`,
    /// read per call by the tool bridge). The default is `legacy/0`, the
    /// paragraph the planner always sent.
    pub procedure: procedure::ProcedureId,
    /// Whether the planner may propose `press`, `select`, `back` and
    /// `wait_for` (`QUARRY_BROWSER_AGENT_WIDENED_ACTIONS`, off by default). Off,
    /// the verbs are absent from the schemas and the procedure text, and one the
    /// model sends anyway is read as an `observe`.
    pub widened_actions: bool,
    /// Tighten-only restrictions sent on `POST /v1/agent/runs` (WS7.L): plan
    /// mode, a Space restriction, an organisation ask. `None` or an empty ask
    /// sends nothing. Quarry can only be made stricter by it, never looser.
    pub policy: Option<crate::quarry_agent::WireRequestAsk>,
    /// Run trace context sent on start (WS10.S4): the Model run id, an agent
    /// label and the run origin. Labels only, never authority.
    pub trace: Option<crate::quarry_agent::WireRunTrace>,
    /// Correlation id sent as `x-request-id` on every Quarry call of the run.
    pub request_id: Option<String>,
    /// WS8.G attach mode: the agent works on its own tab of the person's
    /// Quarry run instead of a lease of its own. `None` keeps the leased run.
    /// Set only by the tool bridge, from a gateway-derived `attach` grant.
    pub attach: Option<AttachConfig>,
    /// How long an attached run waits for a person to give control of the
    /// agent tab back after `USER_TOOK_OVER` (0 = [`DEFAULT_TAKEOVER_WAIT_S`]).
    /// Never longer than what is left of the plan's runtime.
    pub takeover_wait_s: u32,
    /// How often a paused attached run checks for the give-back. `None` is
    /// [`TAKEOVER_POLL_INTERVAL`]; tests shorten it.
    pub takeover_poll: Option<std::time::Duration>,
    /// WS12.S4: refuse a Quarry observation that carries no redaction stamp
    /// (`EXECUTION_CORE_REQUIRE_QUARRY_REDACTION_STAMP`, default off). Set by
    /// the tool bridge from the process environment, never read in the loop.
    /// Off, a missing stamp is logged once per run and the step goes on.
    pub require_quarry_redaction_stamp: bool,
    /// WS8.J: the mark an attached run puts on its tab when it COMPLETES, just
    /// before the attachment is released (`deliverable` by default). Any other
    /// exit (failure, cancel, a limit) never marks. Only meaningful with
    /// `attach`; a leased run has no person's tab to hand over.
    pub tab_mark_on_complete: crate::quarry_agent::TabMark,
    /// E3.2-c: a rejection that carries text is a redirect for the planner
    /// instead of the end of the run (`QUARRY_BROWSER_AGENT_APPROVAL_REDIRECT`,
    /// default off). Set by the tool bridge from the process environment,
    /// never read in the loop. A rejection without text still aborts.
    pub approval_redirect: bool,
    /// E3.3-f: read the text a person sends to the live run at each step boundary
    /// (`QUARRY_BROWSER_AGENT_STEER`, default off). Set by the tool bridge from the
    /// process environment, never read in the loop. Ignored under ZDR.
    pub steer: bool,
}

/// E3.3-f: the switch for steering a live browser run. execution-core's pause and
/// resume handlers read it per call (they refuse a steer text while it is off), the
/// tool bridge copies it into [`PlanConfig::steer`]. Literal `1`, `true` or `on`.
pub const STEER_ENV: &str = "QUARRY_BROWSER_AGENT_STEER";

/// Whether `value` (an env value) turns steering on.
#[must_use]
pub fn steer_enabled_from(value: Option<&str>) -> bool {
    value.is_some_and(|value| matches!(value.trim(), "1" | "true" | "on"))
}

/// Whether steering is on in this process.
#[must_use]
pub fn steer_enabled() -> bool {
    steer_enabled_from(std::env::var(STEER_ENV).ok().as_deref())
}

/// E3.2-c: the longest redirect text the planner is shown, in characters.
pub const MAX_REDIRECT_CHARS: usize = 1000;
/// E3.2-c: redirects one run accepts. Past it a rejection aborts as before, so
/// a person who keeps rejecting cannot hold a run in a loop of replans.
pub const MAX_REDIRECTS_PER_RUN: u32 = 5;

/// E3.2-c: the person's rejection text, made safe to show the planner:
/// control characters become spaces, runs of whitespace collapse, the text is
/// trimmed and cut at [`MAX_REDIRECT_CHARS`] characters. `None` when nothing
/// is left (a rejection with no text stays a plain denial).
#[must_use]
pub fn bound_redirect_text(raw: &str) -> Option<String> {
    clean_person_text(raw, MAX_REDIRECT_CHARS)
}

/// E3.3-f: a steer text made safe to show the planner, cut at the queued-input
/// cap (`mp_contracts::queued_limits::MAX_QUEUED_CHARS`).
#[must_use]
pub fn bound_steer_text(raw: &str) -> Option<String> {
    clean_person_text(raw, mp_contracts::queued_limits::MAX_QUEUED_CHARS)
}

fn clean_person_text(raw: &str, max_chars: usize) -> Option<String> {
    let cleaned = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    Some(collapsed.chars().take(max_chars).collect())
}

/// E3.2-c: one rejected action and what the person said about it. Unlike an
/// [`ActionFailure`] it is not a failure: it is not counted against the failure
/// budget and its text is the person's, not a fixed per-kind sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionRedirect {
    pub step: i32,
    pub action_type: &'static str,
    /// Already bounded by [`bound_redirect_text`].
    pub text: String,
}

impl ActionRedirect {
    /// `step N <action> was not approved. The person said: "<text>"`
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "step {} {} was not approved. The person said: \"{}\"",
            self.step, self.action_type, self.text
        )
    }
}

/// The step failure when a stamp is required and absent.
pub const REDACTION_STAMP_MISSING_REASON: &str =
    "browser-agent step failed: quarry redaction stamp missing";

/// WS12.S4 guard: the failure reason when `observation` came from Quarry
/// without a redaction stamp and the plan requires one; otherwise `None`.
/// With the requirement off, the first missing stamp of a run is logged
/// (`warned` carries that across steps) and the step goes on.
fn redaction_stamp_failure(
    config: &PlanConfig,
    observation: &BrowserObservation,
    warned: &mut bool,
) -> Option<&'static str> {
    if observation.redaction_rule_version.is_some() {
        return None;
    }
    if config.require_quarry_redaction_stamp {
        warn!(
            plan_id = %config.plan_id,
            "quarry observation carries no redaction stamp; refusing it"
        );
        return Some(REDACTION_STAMP_MISSING_REASON);
    }
    if !*warned {
        *warned = true;
        warn!(
            plan_id = %config.plan_id,
            "quarry observation carries no redaction stamp (not required; logged once per run)"
        );
    }
    None
}

/// WS8.G: what an attached run attaches to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttachConfig {
    /// The person's Quarry run id (the attach grant's `resource_id`).
    pub session_run_id: String,
}

/// D5: a person who took the agent tab has ten minutes to give it back.
pub const DEFAULT_TAKEOVER_WAIT_S: u32 = 600;
/// How often a paused attached run checks whether the tab came back.
pub const TAKEOVER_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

#[derive(Debug, Clone)]
pub struct AgentPlan {
    pub config: PlanConfig,
    pub current_step: i32,
    pub status: PlanStatus,
    pub observations: Vec<BrowserObservation>,
    started_at: Instant,
    usage: BrowserLoopUsage,
    /// Tokens the most recent planner call used, the estimate for the next one.
    last_planner_tokens: u64,
    /// Recoverable step failures (only fed when a planner is selected).
    failures: FailureBudget,
    no_progress: NoProgressDetector,
    /// Page URLs observed so far (bounded); `back` may only return to the
    /// page before the current one.
    history: admission::UrlHistory,
    /// The field the last `type` or `select` acted on, cleared when the page
    /// URL changes. A `press Enter` is judged as a click on it.
    focus_hint: Option<BrowserSnapshotTarget>,
    /// Set after a rejected proposal: the same observation is planned over
    /// again, so it is not recorded or evaluated twice.
    replan: bool,
    /// Whether the Phase 5 run-level `PersistentCookieUse` gate has already
    /// run for this plan. Checked once, at the first `decide_next_action`
    /// call (after `start_run` has already succeeded) — never re-asked on
    /// later steps of the same run.
    cookie_use_gate_done: bool,
    /// E3.2-c: the person's redirect for the next planner call (shown once).
    redirect: Option<ActionRedirect>,
    /// E3.2-c: redirects accepted so far (capped by [`MAX_REDIRECTS_PER_RUN`]).
    redirects_used: u32,
    /// E3.2-c: set by `decide_next_action` for the step in hand: a redirect
    /// only makes sense when a planner will read it.
    planner_active: bool,
    /// E3.3-f: texts taken from the run's steer queue, waiting for the next
    /// planner call (shown once). At most `MAX_QUEUED_MESSAGES`.
    steer_pending: Vec<String>,
}

impl AgentPlan {
    pub fn new(config: PlanConfig) -> Self {
        let failures =
            FailureBudget::new(config.max_consecutive_failures, config.max_total_failures);
        Self {
            config,
            failures,
            no_progress: NoProgressDetector::default(),
            history: admission::UrlHistory::default(),
            focus_hint: None,
            replan: false,
            current_step: 0,
            status: PlanStatus::Planning,
            observations: Vec::new(),
            started_at: Instant::now(),
            usage: BrowserLoopUsage::default(),
            last_planner_tokens: 0,
            cookie_use_gate_done: false,
            redirect: None,
            redirects_used: 0,
            planner_active: false,
            steer_pending: Vec::new(),
        }
    }

    /// E3.2-c: take a person's redirect for `action` when the run may use one
    /// (flag on, a planner reading it, budget left, text left after bounding).
    /// `false` means the rejection stays a denial and the run aborts.
    fn accept_redirect(&mut self, action: &BrowserAction, text: &str) -> bool {
        if !self.config.approval_redirect
            || !self.planner_active
            || self.redirects_used >= MAX_REDIRECTS_PER_RUN
        {
            return false;
        }
        let Some(text) = bound_redirect_text(text) else {
            return false;
        };
        self.redirects_used += 1;
        self.redirect = Some(ActionRedirect {
            step: self.current_step,
            action_type: action.action_type.as_str(),
            text,
        });
        true
    }

    /// What the planner has spent so far.
    #[must_use]
    pub const fn usage(&self) -> BrowserLoopUsage {
        self.usage
    }

    /// Add one planner reply's actual usage to the meter.
    pub fn meter_planner_usage(&mut self, usage: crate::llm_planner::PlannerUsage) {
        self.usage.planner_calls = self.usage.planner_calls.saturating_add(1);
        self.usage.planner_input_tokens += u64::from(usage.input_tokens);
        self.usage.planner_output_tokens += u64::from(usage.output_tokens);
        self.last_planner_tokens = usage.total();
    }

    /// The pre-call token check: `Tokens` when the budget is spent, or when the
    /// next call (estimated as the size of the last one) would overshoot it.
    #[must_use]
    pub fn check_planner_budget(&self) -> Option<BrowserResourceLimit> {
        let max = self.config.max_planner_tokens?;
        let spent = self.usage.planner_tokens();
        if spent >= max || spent.saturating_add(self.last_planner_tokens) > max {
            return Some(BrowserResourceLimit::Tokens);
        }
        None
    }

    /// Remember the field a `type` or `select` acts on, so a following
    /// `press Enter` is judged as a click on it. Cleared when the page URL
    /// changes (see `plan_next_action`).
    fn note_dispatch(&mut self, action: &BrowserAction, last: Option<&BrowserObservation>) {
        if !matches!(action.action_type, ActionType::Type | ActionType::Select) {
            return;
        }
        self.focus_hint = action.target.as_ref().and_then(|bound| {
            last?
                .snapshot_targets
                .iter()
                .find(|target| target.ref_id == bound.ref_id)
                .cloned()
        });
    }

    pub fn check_limits(&self) -> Option<BrowserResourceLimit> {
        if self
            .config
            .deadline_at
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Some(BrowserResourceLimit::Credential);
        }
        if self.config.max_steps > 0 && self.current_step >= self.config.max_steps {
            return Some(BrowserResourceLimit::MaxSteps);
        }
        if self.config.max_runtime_s > 0 {
            let elapsed = i32::try_from(self.started_at.elapsed().as_secs()).unwrap_or(i32::MAX);
            if elapsed >= self.config.max_runtime_s {
                return Some(BrowserResourceLimit::Runtime);
            }
        }
        None
    }

    pub fn is_domain_allowed(&self, url: &str) -> bool {
        if self.config.allowed_domains.is_empty() {
            // A missing policy must never fall through to unrestricted browser
            // navigation. Broker-issued grants always carry a non-empty policy.
            return false;
        }
        let Some(host) = extract_host(url) else {
            return false;
        };
        self.config
            .allowed_domains
            .iter()
            .any(|d| host_matches(&host, d))
    }

    pub fn evaluate_stop_criteria(&self, observation: &BrowserObservation) -> bool {
        if self.config.stop_criteria.is_empty() {
            return false;
        }
        observation
            .extracted_text
            .contains(&self.config.stop_criteria)
            || observation.page_title.contains(&self.config.stop_criteria)
    }
}

/// Validate the immutable browser plan before any Quarry lease is acquired.
/// Browser grants are only safe when they carry an explicit canonical domain
/// policy; a caller cannot obtain an "allow all" plan by omitting the list.
pub(crate) fn validate_plan_config(config: &PlanConfig) -> Result<(), String> {
    if config.grant_id.trim().is_empty() {
        return Err("browser grant_id is required".to_owned());
    }
    let canonical = canonicalize_allowed_domains(&config.allowed_domains)?;
    if canonical != config.allowed_domains {
        return Err("browser allowed_domains must be canonical broker policy".to_owned());
    }
    if let Some(start_url) = config
        .start_url
        .as_deref()
        .filter(|url| !url.trim().is_empty())
    {
        let plan = AgentPlan::new(config.clone());
        if !plan.is_domain_allowed(start_url) {
            return Err("browser start_url is outside the broker domain policy".to_owned());
        }
    }
    // A restriction that cannot be sent as given fails the run; it is never
    // trimmed or dropped on the way to Quarry.
    if let Some(ask) = &config.policy {
        ask.validate()?;
    }
    if let Some(attach) = &config.attach {
        // An attached run acts on the person's own browser: it names their
        // run, never a persistent profile of its own, and an explicit domain
        // list (canonicalisation above already refused a malformed one).
        if !crate::quarry_agent::is_trace_token(&attach.session_run_id) {
            return Err("browser attach session id is malformed".to_owned());
        }
        if config.allowed_domains.is_empty() {
            return Err("browser attach requires explicit allowed_domains".to_owned());
        }
        if config.profile_id.is_some() {
            return Err("browser attach cannot use a persistent profile".to_owned());
        }
        if config.zdr {
            return Err("browser attach is refused for zero-data-retention runs".to_owned());
        }
    }
    Ok(())
}

/// Canonicalize a broker-supplied domain policy. This independently validates
/// the gRPC response at the execution boundary: a malformed or empty response
/// cannot turn into unrestricted navigation if a broker or proxy is miswired.
pub(crate) fn canonicalize_allowed_domains(domains: &[String]) -> Result<Vec<String>, String> {
    const MAX_ALLOWED_DOMAINS: usize = 32;
    if domains.is_empty() || domains.len() > MAX_ALLOWED_DOMAINS {
        return Err("browser allowed_domains must be a non-empty bounded policy".to_owned());
    }

    let mut canonical = BTreeSet::new();
    for raw in domains {
        let domain = raw.trim().to_ascii_lowercase();
        if !is_canonical_domain(&domain) {
            return Err("browser allowed_domains contains an invalid hostname".to_owned());
        }
        canonical.insert(domain);
    }
    if canonical.is_empty() {
        return Err("browser allowed_domains must be a non-empty bounded policy".to_owned());
    }
    Ok(canonical.into_iter().collect())
}

fn is_canonical_domain(domain: &str) -> bool {
    if domain.is_empty()
        || domain.len() > 253
        || domain.parse::<IpAddr>().is_ok()
        || !domain.contains('.')
        || domain.starts_with('.')
        || domain.ends_with('.')
    {
        return false;
    }

    domain.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    })
}

/// Result of a single planning step.
pub enum PlanStepResult {
    Action(BrowserAction),
    Completed(String),
    WaitingApproval,
    Failed(String),
    /// A proposal was not admitted (WS11.S4). The planner is asked again about
    /// the same observation; nothing was sent to Quarry.
    Rejected,
    /// A max-steps or runtime budget stopped the loop. This is distinct from
    /// the planner's successful `Completed` result.
    ResourceExhausted {
        reason: String,
        limit: BrowserResourceLimit,
    },
    /// The loop stopped on its own judgement (planner unavailable, repeated
    /// failures, no progress, the planner's `blocked` or `incomplete`). The
    /// reason starts with the kind's stable prefix.
    Stopped {
        reason: String,
        kind: BrowserFailureKind,
    },
    /// A terminal non-success state. The reason is structured so its meaning
    /// survives the async tool bridge/runtime handoff.
    Aborted {
        reason: String,
        abort_reason: BrowserAbortReason,
    },
}

/// Structured result from a browser-agent loop.
///
/// `summary` is operator/model context only; consumers must use
/// `abort_reason` or `resource_limit`, rather than parsing this text, to
/// determine lifecycle behavior.
#[derive(Debug, Clone)]
pub struct BrowserAgentLoopResult {
    pub status: PlanStatus,
    pub observations: Vec<BrowserObservation>,
    pub summary: String,
    pub abort_reason: Option<BrowserAbortReason>,
    pub resource_limit: Option<BrowserResourceLimit>,
    /// Planner spend for this loop (counts only).
    pub usage: BrowserLoopUsage,
    /// Why an LLM-planned run stopped on its own judgement, when it did. The
    /// `summary` carries the kind's stable prefix; this is the typed twin.
    pub failure_kind: Option<BrowserFailureKind>,
}

/// Extract the host portion of a URL without bringing in a full URL parser.
/// Handles `scheme://[user:pass@]host[:port][/path...]` and lowercases the
/// result. Returns `None` for inputs that lack a `://` separator or where
/// the host segment is empty.
fn extract_host(url: &str) -> Option<String> {
    let (scheme, after_scheme) = url.split_once("://")?;
    if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
        return None;
    }
    // Drop userinfo if present.
    let after_userinfo = match after_scheme.split_once('@') {
        Some((_, rest)) => rest,
        None => after_scheme,
    };
    // Host ends at the first `/`, `?`, `#`, or `:` (port separator).
    let host_end = after_userinfo
        .find(['/', '?', '#', ':'])
        .unwrap_or(after_userinfo.len());
    let host = &after_userinfo[..host_end];
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

/// Test whether `host` is allowed by an allowlist entry `pattern`.
///
/// Match rules:
///   * Exact match: `host == pattern` (after both lowercased).
///   * Subdomain match: `host` ends with `.{pattern}` so e.g. `pattern =
///     "example.com"` matches `api.example.com` but not
///     `evil-example.com`. The leading dot prevents the substring-attack
///     class that the previous `url.contains(pattern)` implementation was
///     vulnerable to.
fn host_matches(host: &str, pattern: &str) -> bool {
    let pat = pattern.trim().trim_start_matches('.').to_ascii_lowercase();
    if pat.is_empty() {
        return false;
    }
    if host == pat {
        return true;
    }
    host.len() > pat.len()
        && host.ends_with(&pat)
        && host.as_bytes()[host.len() - pat.len() - 1] == b'.'
}

/// Plan the next browser action given the current plan state and latest observation.
pub fn plan_next_action(
    plan: &mut AgentPlan,
    last_observation: Option<&BrowserObservation>,
) -> PlanStepResult {
    if plan.status.is_terminal() {
        return PlanStepResult::Failed("plan is in terminal state".to_owned());
    }

    if let Some(limit) = plan.check_limits() {
        plan.status = PlanStatus::Failed;
        return PlanStepResult::ResourceExhausted {
            reason: limit.reason().to_owned(),
            limit,
        };
    }

    let replanning = std::mem::take(&mut plan.replan);
    if let Some(obs) = last_observation.filter(|_| !replanning) {
        plan.observations.push(obs.clone());
        if plan.history.observe(&obs.page_url) {
            plan.focus_hint = None;
        }

        if obs.status == ObservationStatus::Failed {
            plan.status = PlanStatus::Failed;
            return PlanStepResult::Failed(obs.error_message.clone());
        }

        if obs.status == ObservationStatus::Blocked {
            plan.status = PlanStatus::Failed;
            return PlanStepResult::Failed(format!("action blocked: {}", obs.error_message));
        }

        if plan.evaluate_stop_criteria(obs) {
            plan.status = PlanStatus::Completed;
            return PlanStepResult::Completed("stop criteria met".to_owned());
        }
    }

    if plan.config.require_approval {
        plan.status = PlanStatus::WaitingApproval;
        return PlanStepResult::WaitingApproval;
    }

    plan.current_step += 1;
    plan.status = PlanStatus::Executing;

    let action_id = format!("act_{:04}", plan.current_step);

    if plan.current_step == 1 && plan.observations.is_empty() {
        // A fresh Quarry lease has no page loaded yet — navigate to the
        // configured starting point (typically the tab the run was launched
        // from) before anything else, or chromiumoxide fails every action
        // with "no current page". Falls back to the pre-Phase-2 deterministic
        // `Observe` when no `start_url` is configured.
        let start_url = plan
            .config
            .start_url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty());
        return PlanStepResult::Action(match start_url {
            Some(url) => BrowserAction {
                action_id,
                grant_id: plan.config.grant_id.clone(),
                action_type: ActionType::Goto,
                selector: String::new(),
                value: String::new(),
                url: url.to_owned(),
                max_wait_ms: 15_000,
                target: None,
                reason: "navigate to the run's starting page".to_owned(),
                risk_category: None,
            },
            None => BrowserAction {
                action_id,
                grant_id: plan.config.grant_id.clone(),
                action_type: ActionType::Observe,
                selector: String::new(),
                value: String::new(),
                url: String::new(),
                max_wait_ms: 5000,
                target: None,
                reason: String::new(),
                risk_category: None,
            },
        });
    }

    PlanStepResult::Action(BrowserAction {
        action_id,
        grant_id: plan.config.grant_id.clone(),
        action_type: ActionType::Observe,
        selector: String::new(),
        value: String::new(),
        url: String::new(),
        max_wait_ms: 5000,
        target: None,
        reason: String::new(),
        risk_category: None,
    })
}

/// Execute the full browser-agent loop, dispatching each action to Quarry's
/// `/v1/agent/*` endpoint and feeding the observation back into the planner.
///
/// `client` is `None` when the browser agent is unconfigured
/// (`QUARRY_BROWSER_AGENT_ENABLED`/`QUARRY_EDGE_URL` unset) — the loop then
/// fails fast rather than silently no-op'ing. The planner (`plan_next_action`)
/// is still deterministic (always `Observe`) until the LLM decider lands; the
/// dispatch path itself is real.
// Cohesive single-loop driver (lease → dispatch/observe → release); splitting
// it would obscure the linear dispatch/observe/cleanup flow.
#[allow(clippy::too_many_lines)]
pub async fn run_browser_agent_loop(
    config: PlanConfig,
    client: Option<&crate::quarry_agent::QuarryAgentClient>,
    planner: Option<&dyn crate::llm_planner::ActionPlanner>,
    sink: Option<&dyn BrowserEventSink>,
    state: Option<&crate::state::StateStore>,
) -> BrowserAgentLoopResult {
    let mut plan = AgentPlan::new(config);
    info!(plan_id = %plan.config.plan_id, "browser-agent loop started");

    if let Err(error) = validate_plan_config(&plan.config) {
        plan.status = PlanStatus::Failed;
        warn!(plan_id = %plan.config.plan_id, %error, "browser-agent policy rejected before Quarry launch");
        return BrowserAgentLoopResult {
            status: plan.status,
            observations: plan.observations,
            summary: error,
            abort_reason: None,
            resource_limit: None,
            usage: BrowserLoopUsage::default(),
            failure_kind: None,
        };
    }

    let Some(client) = client else {
        plan.status = PlanStatus::Failed;
        let summary =
            "browser agent unavailable: set QUARRY_BROWSER_AGENT_ENABLED=1 and QUARRY_EDGE_URL"
                .to_owned();
        warn!(plan_id = %plan.config.plan_id, "{summary}");
        return BrowserAgentLoopResult {
            status: plan.status,
            observations: plan.observations,
            summary,
            abort_reason: None,
            resource_limit: None,
            usage: BrowserLoopUsage::default(),
            failure_kind: None,
        };
    };

    let constraints = crate::quarry_agent::AgentConstraints {
        max_steps: u32::try_from(plan.config.max_steps).unwrap_or(0),
        allowed_domains: plan.config.allowed_domains.clone(),
        max_runtime_s: if plan.config.max_runtime_s > 0 {
            Some(u32::try_from(plan.config.max_runtime_s).unwrap_or(0))
        } else {
            None
        },
        max_cost_usd: plan.config.max_cost_usd,
    };
    let zdr = plan.config.zdr;

    // Attach mode (WS8.G): the agent gets its own tab of the person's run and
    // never acquires a lease (`start_run` is not called). The attach bounds are
    // the plan's, clamped to Quarry's (`attach.rs`), and always sent, so the
    // takeover wait can be held under the runtime Quarry will enforce.
    let launched = if let Some(attach) = plan.config.attach.clone() {
        let max_steps = match u32::try_from(plan.config.max_steps).unwrap_or(0) {
            0 => crate::quarry_agent::ATTACH_DEFAULT_STEPS,
            steps => steps.min(crate::quarry_agent::ATTACH_MAX_STEPS),
        };
        let runtime_s = match u64::try_from(plan.config.max_runtime_s).unwrap_or(0) {
            0 => crate::quarry_agent::ATTACH_DEFAULT_RUNTIME_S,
            seconds => seconds.min(crate::quarry_agent::ATTACH_MAX_RUNTIME_S),
        };
        client
            .attach_run(crate::quarry_agent::AttachParams {
                org_id: &plan.config.org_id,
                run_id: &attach.session_run_id,
                grant_id: &plan.config.grant_id,
                // The tab opens blank; the loop's first step navigates to the
                // start URL as an admitted agent step (one navigation, not two).
                start_url: None,
                max_steps: Some(max_steps),
                max_runtime_s: Some(runtime_s),
                request_id: plan.config.request_id.as_deref(),
            })
            .await
            .map(|data| QuarryRun::Attached {
                session_run_id: attach.session_run_id,
                attach: data,
                attached_at: Instant::now(),
                runtime_s,
            })
            .map_err(|e| (e, "attach"))
    } else {
        // Acquire a leased browser session for this run. `profile_id` threads
        // through so an AI run launched from a tab with a persistent profile
        // inherits its cookie state (Phase 2); `None` behaves exactly as before.
        client
            .start_run({
                let mut params = crate::quarry_agent::StartRunParams::new(
                    &plan.config.org_id,
                    &constraints,
                    zdr,
                    &plan.config.grant_id,
                );
                params.profile_id = plan.config.profile_id.clone();
                // Tighten-only restrictions and the trace label ride the start
                // request only when present; an empty ask sends no `policy` key.
                params.policy = plan
                    .config
                    .policy
                    .clone()
                    .filter(|ask| !ask.is_empty());
                params.trace = plan.config.trace.clone();
                params.request_id = plan.config.request_id.as_deref();
                params
            })
            .await
            .map(QuarryRun::Leased)
            .map_err(|e| (e, "start_run"))
    };
    let run = match launched {
        Ok(run) => run,
        Err((e, call)) => {
            plan.status = PlanStatus::Failed;
            warn!(plan_id = %plan.config.plan_id, error = %e, call, "browser-agent launch failed");
            // A 402 budget or a 429 at launch is a resource outcome, not a
            // failure of the run.
            let resource_limit = resource_limit_for_quarry_error(&e);
            return BrowserAgentLoopResult {
                status: plan.status,
                observations: plan.observations,
                summary: match resource_limit {
                    Some(limit) => limit.reason().to_owned(),
                    None => format!(
                        "browser-agent {call} failed: {}",
                        crate::quarry_agent::fixed_error_text(&e)
                    ),
                },
                abort_reason: None,
                resource_limit,
                usage: BrowserLoopUsage::default(),
                failure_kind: None,
            };
        }
    };
    // An attached run uses the person's own session, and so their cookie jar:
    // recorded once, as an audit event, in place of the persistent-profile
    // gate (an attached run never names a profile).
    if let (Some(sink), QuarryRun::Attached { session_run_id, attach, .. }) = (sink, &run) {
        sink.attach_shared_jar(&plan.config, session_run_id, &attach.attach_id, &attach.tab_id)
            .await;
    }

    let mut pending: Option<BrowserObservation> = None;
    let mut abort_reason = None;
    let mut resource_limit = None;
    let mut failure_kind = None;
    // WS12.S4: the missing-stamp warning is logged once per run.
    let mut stamp_warned = false;
    let summary = loop {
        let last = pending.take();
        let result = decide_next_action(&mut plan, last.as_ref(), planner, state, sink).await;
        match result {
            PlanStepResult::Action(action) => {
                // Enforce the domain allow-list at dispatch for navigations
                // (defense in depth alongside Quarry's server-side check).
                if matches!(action.action_type, ActionType::Goto)
                    && !plan.is_domain_allowed(&action.url)
                {
                    plan.status = PlanStatus::Failed;
                    break format!("navigation blocked by allow-list: {}", action.url);
                }
                info!(
                    plan_id = %plan.config.plan_id,
                    step = plan.current_step,
                    action_type = action.action_type.as_str(),
                    "dispatching browser action to quarry"
                );
                // Best-effort: surface the dispatched action on the run-event
                // stream before we block on Quarry. Never fails the loop.
                if let Some(sink) = sink {
                    sink.action_dispatched(&plan.config, &action).await;
                }
                let wire_action = crate::quarry_agent::action_to_wire(&action);
                match dispatch_step(
                    client,
                    &run,
                    &plan.config,
                    &constraints,
                    wire_action,
                    planner.is_some(),
                )
                .await
                {
                    Ok(wire_obs) => {
                        plan.failures.record_success();
                        let obs = crate::quarry_agent::observation_from_wire(
                            &wire_obs,
                            &action.action_id,
                            &plan.config.grant_id,
                        );
                        if let Some(reason) =
                            redaction_stamp_failure(&plan.config, &obs, &mut stamp_warned)
                        {
                            plan.status = PlanStatus::Failed;
                            break reason.to_owned();
                        }
                        // Best-effort: surface the observation we just received.
                        if let Some(sink) = sink {
                            sink.observation_received(&plan.config, &obs).await;
                        }
                        pending = Some(obs);
                    }
                    // A refused bot wall is an observation, not a failure to
                    // retry: the next planning step stops on its Blocked status.
                    Err(crate::quarry_agent::AgentClientError::Challenge(challenge)) => {
                        let page_url = if matches!(action.action_type, ActionType::Goto) {
                            action.url.clone()
                        } else {
                            plan.observations
                                .last()
                                .map(|o| o.page_url.clone())
                                .unwrap_or_default()
                        };
                        let obs = crate::quarry_agent::observation_from_challenge_refusal(
                            challenge,
                            &action.action_id,
                            &plan.config.grant_id,
                            &page_url,
                        );
                        if let Some(sink) = sink {
                            sink.observation_received(&plan.config, &obs).await;
                        }
                        pending = Some(obs);
                    }
                    // A policy refusal is final the same way: blocked with the
                    // fixed text, so the next planning step ends the run. The
                    // action never reaches the risk gate or an approval, and
                    // nothing is retried or re-read.
                    Err(crate::quarry_agent::AgentClientError::PolicyDenied(denial)) => {
                        warn!(
                            plan_id = %plan.config.plan_id,
                            reason = denial.reason.as_str(),
                            capability = denial.capability.as_deref().unwrap_or(""),
                            decision_source = denial.decision_source.as_deref().unwrap_or(""),
                            "quarry web policy refused the browser action"
                        );
                        let page_url = if matches!(action.action_type, ActionType::Goto) {
                            action.url.clone()
                        } else {
                            plan.observations
                                .last()
                                .map(|o| o.page_url.clone())
                                .unwrap_or_default()
                        };
                        let obs = crate::quarry_agent::observation_from_policy_denial(
                            &denial,
                            &action.action_id,
                            &plan.config.grant_id,
                            &page_url,
                        );
                        if let Some(sink) = sink {
                            sink.observation_received(&plan.config, &obs).await;
                        }
                        pending = Some(obs);
                    }
                    // WS8.G: a person took the agent tab. A state, not a fault:
                    // the run pauses until the tab comes back (checked once a
                    // second), and fails only when it does not within the wait.
                    // The refused action did not run and is not replayed; the
                    // planner goes on from the page as the person left it.
                    Err(e)
                        if matches!(run, QuarryRun::Attached { .. })
                            && crate::quarry_agent::is_user_took_over(&e) =>
                    {
                        let tab_id = run.attached_tab_id().unwrap_or_default().to_owned();
                        info!(plan_id = %plan.config.plan_id, step = plan.current_step, "a person took the agent tab; waiting for it to be given back");
                        if let Some(sink) = sink {
                            sink.paused_by_user(&plan.config, &tab_id).await;
                        }
                        let wait = takeover_wait(
                            &plan.config,
                            plan.started_at.elapsed(),
                            run.attach_time_left(),
                        );
                        let poll = plan.config.takeover_poll.unwrap_or(TAKEOVER_POLL_INTERVAL);
                        let outcome = {
                            let config = &plan.config;
                            let run = &run;
                            let constraints = &constraints;
                            await_give_back(
                                poll,
                                wait,
                                || {
                                    dispatch_step(
                                        client,
                                        run,
                                        config,
                                        constraints,
                                        crate::quarry_agent::AgentAction::GetContent,
                                        false,
                                    )
                                },
                                || run_cancelled(state, &config.run_id),
                            )
                            .await
                        };
                        match outcome {
                            GiveBack::Resumed(wire_obs) => {
                                info!(plan_id = %plan.config.plan_id, "the agent tab was given back; resuming");
                                if let Some(sink) = sink {
                                    sink.resumed_after_takeover(&plan.config, &tab_id).await;
                                }
                                let obs = crate::quarry_agent::observation_from_wire(
                                    &wire_obs,
                                    &action.action_id,
                                    &plan.config.grant_id,
                                );
                                if let Some(reason) =
                                    redaction_stamp_failure(&plan.config, &obs, &mut stamp_warned)
                                {
                                    plan.status = PlanStatus::Failed;
                                    break reason.to_owned();
                                }
                                if let Some(sink) = sink {
                                    sink.observation_received(&plan.config, &obs).await;
                                }
                                pending = Some(obs);
                            }
                            GiveBack::Cancelled => {
                                plan.status = PlanStatus::Aborted;
                                abort_reason = Some(BrowserAbortReason::Cancelled);
                                break "cancelled by user".to_owned();
                            }
                            GiveBack::TimedOut => {
                                plan.status = PlanStatus::Failed;
                                break took_over_timeout_reason(wait);
                            }
                            GiveBack::Failed(probe_error) => {
                                plan.status = PlanStatus::Failed;
                                if let Some(limit) = resource_limit_for_quarry_error(&probe_error) {
                                    resource_limit = Some(limit);
                                    break limit.reason().to_owned();
                                }
                                break format!(
                                    "browser-agent step failed: {}",
                                    crate::quarry_agent::fixed_error_text(&probe_error)
                                );
                            }
                        }
                    }
                    Err(e) => {
                        plan.status = PlanStatus::Failed;
                        // The classifier decides: a 402 budget or a 429 ends the
                        // run `resource_exhausted`, never `Failed`, never retried.
                        if let Some(limit) = resource_limit_for_quarry_error(&e) {
                            warn!(plan_id = %plan.config.plan_id, error = %e, "quarry resource limit");
                            resource_limit = Some(limit);
                            break limit.reason().to_owned();
                        }
                        // With a planner, a recoverable failure is recorded for
                        // it and the page is re-read, not the end of the run.
                        if planner.is_some() {
                            if let Some(kind) = crate::quarry_agent::recoverable_kind(&e) {
                                warn!(plan_id = %plan.config.plan_id, kind = kind.as_str(), "recoverable browser step failure");
                                let verdict = plan.failures.record(ActionFailure {
                                    step: plan.current_step,
                                    action_type: action.action_type.as_str(),
                                    kind: kind.as_str(),
                                    advice: kind.advice(),
                                });
                                if let FailureVerdict::Stop(reason) = verdict {
                                    failure_kind = Some(BrowserFailureKind::RepeatedFailure);
                                    break reason;
                                }
                                match dispatch_step(
                                    client,
                                    &run,
                                    &plan.config,
                                    &constraints,
                                    crate::quarry_agent::AgentAction::GetContent,
                                    true,
                                )
                                .await
                                {
                                    Ok(wire_obs) => {
                                        let obs = crate::quarry_agent::observation_from_wire(
                                            &wire_obs,
                                            &action.action_id,
                                            &plan.config.grant_id,
                                        );
                                        if let Some(reason) = redaction_stamp_failure(
                                            &plan.config,
                                            &obs,
                                            &mut stamp_warned,
                                        ) {
                                            plan.status = PlanStatus::Failed;
                                            break reason.to_owned();
                                        }
                                        if let Some(sink) = sink {
                                            sink.observation_received(&plan.config, &obs).await;
                                        }
                                        plan.status = PlanStatus::Executing;
                                        pending = Some(obs);
                                        continue;
                                    }
                                    Err(reread) => {
                                        if let Some(limit) = resource_limit_for_quarry_error(&reread) {
                                            resource_limit = Some(limit);
                                            break limit.reason().to_owned();
                                        }
                                        break format!(
                                            "browser-agent step failed: {}",
                                            crate::quarry_agent::fixed_error_text(&reread)
                                        );
                                    }
                                }
                            }
                        }
                        break format!(
                            "browser-agent step failed: {}",
                            crate::quarry_agent::fixed_error_text(&e)
                        );
                    }
                }
            }
            PlanStepResult::Completed(reason) => break reason,
            PlanStepResult::ResourceExhausted { reason, limit } => {
                resource_limit = Some(limit);
                break reason;
            }
            PlanStepResult::Aborted {
                reason,
                abort_reason: reason_kind,
            } => {
                abort_reason = Some(reason_kind);
                break reason;
            }
            PlanStepResult::Rejected => {
                // Plan over the same page again; it is already recorded.
                plan.replan = true;
                pending = last;
            }
            PlanStepResult::Stopped { reason, kind } => {
                failure_kind = Some(kind);
                break reason;
            }
            PlanStepResult::WaitingApproval => break "paused for approval".to_owned(),
            PlanStepResult::Failed(error) => break error,
        }
    };

    // Always release the leased session, or end the attachment: every exit
    // of the loop comes through here.
    match &run {
        QuarryRun::Leased(lease) => {
            if let Err(e) = client
                .close_run_with(
                    &lease.run_id,
                    &plan.config.org_id,
                    plan.config.request_id.as_deref(),
                )
                .await
            {
                warn!(plan_id = %plan.config.plan_id, error = %e, "browser-agent close_run failed");
            }
        }
        QuarryRun::Attached {
            session_run_id,
            attach,
            ..
        } => {
            // WS8.J: a run that completed marks its tab (`deliverable` by
            // default) so the release keeps it open for the person. Every other
            // exit marks nothing. A failed mark is logged and never blocks the
            // release.
            if plan.status == PlanStatus::Completed
                && plan.config.tab_mark_on_complete != crate::quarry_agent::TabMark::None
            {
                if let Err(e) = client
                    .mark_tab(crate::quarry_agent::MarkTabParams {
                        org_id: &plan.config.org_id,
                        run_id: session_run_id,
                        tab_id: &attach.tab_id,
                        grant_id: &plan.config.grant_id,
                        mark: plan.config.tab_mark_on_complete,
                        request_id: plan.config.request_id.as_deref(),
                    })
                    .await
                {
                    warn!(
                        plan_id = %plan.config.plan_id,
                        error = %e,
                        mark = plan.config.tab_mark_on_complete.as_str(),
                        "browser-agent mark_tab failed; releasing anyway"
                    );
                }
            }
            let disposition = attach_disposition(&plan.status);
            if let Err(e) = client
                .release_attach(
                    &plan.config.org_id,
                    session_run_id,
                    &attach.attach_id,
                    disposition,
                    plan.config.request_id.as_deref(),
                )
                .await
            {
                warn!(
                    plan_id = %plan.config.plan_id,
                    error = %e,
                    disposition = disposition.as_str(),
                    "browser-agent release_attach failed"
                );
            }
        }
    }

    info!(plan_id = %plan.config.plan_id, status = plan.status.as_str(), "browser-agent loop finished");
    let observations = plan.observations.clone();
    let usage = plan.usage();
    BrowserAgentLoopResult {
        status: plan.status,
        observations,
        summary,
        abort_reason,
        resource_limit,
        usage,
        failure_kind,
    }
}

/// The Quarry side of one loop: a lease of our own, or (WS8.G) an attachment
/// to the person's run.
enum QuarryRun {
    Leased(crate::quarry_agent::StartRunResponse),
    Attached {
        /// The person's Quarry run; every step and the release go to it.
        session_run_id: String,
        attach: crate::quarry_agent::AttachData,
        attached_at: Instant,
        /// The runtime bound sent with the attach; Quarry counts it from the
        /// attach, pauses included.
        runtime_s: u64,
    },
}

impl QuarryRun {
    fn attached_tab_id(&self) -> Option<&str> {
        match self {
            Self::Attached { attach, .. } => Some(attach.tab_id.as_str()),
            Self::Leased(_) => None,
        }
    }

    /// What is left of the attachment's runtime bound, `None` for a lease.
    fn attach_time_left(&self) -> Option<std::time::Duration> {
        match self {
            Self::Attached {
                attached_at,
                runtime_s,
                ..
            } => Some(
                std::time::Duration::from_secs(*runtime_s).saturating_sub(attached_at.elapsed()),
            ),
            Self::Leased(_) => None,
        }
    }
}

/// D6b: an attachment is released when the run completed and aborted on every
/// other exit (failure, cancel, a limit, an approval stop, a plan error).
#[must_use]
pub(crate) fn attach_disposition(status: &PlanStatus) -> crate::quarry_agent::AttachDisposition {
    if *status == PlanStatus::Completed {
        crate::quarry_agent::AttachDisposition::Release
    } else {
        crate::quarry_agent::AttachDisposition::Abort
    }
}

/// How long a paused attached run waits for the tab to come back (D5): the
/// configured wait (default 600 s), never longer than what is left of the
/// plan's runtime or of the attachment's own runtime bound.
#[must_use]
pub(crate) fn takeover_wait(
    config: &PlanConfig,
    run_elapsed: std::time::Duration,
    attach_left: Option<std::time::Duration>,
) -> std::time::Duration {
    let configured = match config.takeover_wait_s {
        0 => DEFAULT_TAKEOVER_WAIT_S,
        seconds => seconds,
    };
    let mut wait = std::time::Duration::from_secs(u64::from(configured));
    if let Ok(runtime) = u64::try_from(config.max_runtime_s) {
        if runtime > 0 {
            wait = wait.min(std::time::Duration::from_secs(runtime).saturating_sub(run_elapsed));
        }
    }
    if let Some(left) = attach_left {
        wait = wait.min(left);
    }
    wait
}

/// The run summary when the person kept the tab past the wait. Fixed text.
#[must_use]
pub(crate) fn took_over_timeout_reason(wait: std::time::Duration) -> String {
    format!(
        "the user did not give control back within {} s",
        wait.as_secs()
    )
}

/// How a takeover pause ended.
pub(crate) enum GiveBack {
    /// The tab came back; the probe's observation is the page as left.
    Resumed(crate::quarry_agent::WireObservation),
    /// The wait ran out with the tab still held.
    TimedOut,
    /// The run was cancelled while it waited.
    Cancelled,
    /// The probe failed for another reason (the tab or attachment is gone).
    Failed(crate::quarry_agent::AgentClientError),
}

/// Wait out a takeover: every `poll`, a passive probe step on the agent tab.
/// Quarry refuses any agent step on a tab a person holds with 409
/// `USER_TOOK_OVER` before admission (nothing runs, nothing is charged), so a
/// refused probe means "still held" and an answered one means "given back".
/// Quarry's tab list carries no holder, so the probe is the only signal. The
/// cancel gate is checked before and after each sleep.
pub(crate) async fn await_give_back<P, Fut, C>(
    poll: std::time::Duration,
    wait: std::time::Duration,
    mut probe: P,
    mut cancelled: C,
) -> GiveBack
where
    P: FnMut() -> Fut,
    Fut: std::future::Future<
        Output = Result<
            crate::quarry_agent::WireObservation,
            crate::quarry_agent::AgentClientError,
        >,
    >,
    C: FnMut() -> bool,
{
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        if cancelled() {
            return GiveBack::Cancelled;
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return GiveBack::TimedOut;
        }
        tokio::time::sleep(poll.min(deadline - now)).await;
        if cancelled() {
            return GiveBack::Cancelled;
        }
        match probe().await {
            Ok(observation) => return GiveBack::Resumed(observation),
            Err(error) if crate::quarry_agent::is_user_took_over(&error) => {}
            Err(error) => return GiveBack::Failed(error),
        }
    }
}

/// Whether the run was cancelled (the same `StateStore` gate the pause check
/// reads). False without a store or a run id.
fn run_cancelled(state: Option<&crate::state::StateStore>, run_id: &str) -> bool {
    state.is_some_and(|state| {
        !run_id.is_empty()
            && matches!(
                state.get_or_create(run_id).status,
                crate::state::RunStatus::Cancelled
            )
    })
}

/// One Quarry step. With `recover` (an LLM planner is selected) a 502
/// `DRIVER_FAILED` is replayed once, and a 429 that asks for at most 10 s is
/// waited out and replayed once; everything else, including a 504, is returned
/// as it came (a timed-out action is never replayed).
async fn dispatch_step(
    client: &crate::quarry_agent::QuarryAgentClient,
    run: &QuarryRun,
    config: &PlanConfig,
    constraints: &crate::quarry_agent::AgentConstraints,
    action: crate::quarry_agent::AgentAction,
    recover: bool,
) -> Result<crate::quarry_agent::WireObservation, crate::quarry_agent::AgentClientError> {
    let mut driver_retried = false;
    let mut waited = false;
    loop {
        let result = match run {
            QuarryRun::Leased(lease) => {
                let mut params = crate::quarry_agent::StepParams::new(
                    &lease.run_id,
                    &lease.lease_id,
                    &config.org_id,
                    action.clone(),
                    constraints,
                    config.zdr,
                    &config.grant_id,
                );
                params.request_id = config.request_id.as_deref();
                client.step(params).await
            }
            // A delegate step on the person's run: the attach grant, always
            // `initiated_by: "agent"`, the Model run id as the advisory label.
            QuarryRun::Attached { session_run_id, .. } => {
                client
                    .step_attached(crate::quarry_agent::AttachedStepParams {
                        org_id: &config.org_id,
                        run_id: session_run_id,
                        grant_id: &config.grant_id,
                        action: action.clone(),
                        ai_run_id: Some(config.run_id.as_str()).filter(|id| !id.is_empty()),
                        request_id: config.request_id.as_deref(),
                    })
                    .await
            }
        };
        let Err(error) = &result else {
            return result;
        };
        if !recover {
            return result;
        }
        if !driver_retried
            && crate::quarry_agent::recoverable_kind(error)
                == Some(crate::quarry_agent::RecoverableKind::DriverFailed)
        {
            driver_retried = true;
            continue;
        }
        if !waited {
            if let Some(wait) = crate::quarry_agent::backpressure_wait(error) {
                waited = true;
                tokio::time::sleep(if cfg!(test) {
                    std::time::Duration::from_millis(5)
                } else {
                    wait
                })
                .await;
                continue;
            }
        }
        return result;
    }
}

/// Polling interval while a run is paused (Phase 2 B5). Shortened under test
/// so pause/resume unit tests don't sleep in real wall-clock time.
fn pause_poll_interval() -> std::time::Duration {
    if cfg!(test) {
        std::time::Duration::from_millis(5)
    } else {
        std::time::Duration::from_millis(500)
    }
}

/// Decide the next step: first poll for a user-initiated cancel/pause
/// (Phase 2 B5, see [`wait_out_pause_or_cancel`]); then, once per run, gate
/// persistent-cookie-profile reuse (Phase 5, see
/// [`gate_persistent_cookie_use`]); then run the deterministic gate
/// (`plan_next_action` — terminal/limit/stop-criteria/approval checks + step
/// bookkeeping); then, when it yields an action and an LLM planner is
/// configured, let the model choose the real action from the latest
/// observation (falling back to the deterministic `Observe` action when the
/// planner is absent or errors); finally, gate the resulting action if it is
/// classified risky (Phase 5, see `classify_action_risk`) before returning
/// it for dispatch.
async fn decide_next_action(
    plan: &mut AgentPlan,
    last_observation: Option<&BrowserObservation>,
    planner: Option<&dyn crate::llm_planner::ActionPlanner>,
    state: Option<&crate::state::StateStore>,
    sink: Option<&dyn BrowserEventSink>,
) -> PlanStepResult {
    plan.planner_active = planner.is_some();
    if let Some(aborted) = wait_out_pause_or_cancel(plan, state, sink).await {
        return aborted;
    }

    if let Some(gated) = gate_persistent_cookie_use(plan, sink).await {
        return gated;
    }

    let gate = plan_next_action(plan, last_observation);
    let PlanStepResult::Action(candidate) = gate else {
        return gate;
    };
    // The first step of a run with a start URL is a navigation the run was
    // asked for, not a decision: no planner call, no tokens (F11).
    let deterministic_first_goto = candidate.action_type == ActionType::Goto
        && plan.current_step == 1
        && last_observation.is_none();
    let action = match planner {
        None => candidate,
        Some(_) if deterministic_first_goto => candidate,
        Some(planner) => {
            // The planner reads the hygiene view of the page, never the raw
            // observation; the risk gate and the bot-wall check below keep
            // reading the observation in full.
            // Pre-call token check: no planner call is started that the
            // budget cannot cover.
            if let Some(limit) = plan.check_planner_budget() {
                plan.status = PlanStatus::Failed;
                return PlanStepResult::ResourceExhausted {
                    reason: limit.reason().to_owned(),
                    limit,
                };
            }
            let view = last_observation.map(planner_view::PlannerView::from_observation);
            // E3.2-c: a person's redirect is shown for this one planner call
            // (its retries included) and then dropped.
            let redirect_line = plan.redirect.take().map(|redirect| redirect.line());
            // E3.3-f: what the person sent since the last call, shown once.
            let steer_lines = (!plan.steer_pending.is_empty()).then(|| {
                std::mem::take(&mut plan.steer_pending)
                    .iter()
                    .map(|text| format!("- \"{text}\""))
                    .collect::<Vec<_>>()
                    .join("\n")
            });
            // A transient or unreadable reply is retried once (the unreadable
            // one with a corrective line); the second failure stops the run.
            let mut attempts = 0u32;
            let reply = loop {
                let context = crate::llm_planner::PlannerContext {
                    config: &plan.config,
                    view: view.as_ref(),
                    failures: plan.failures.recent(),
                    retry_note: (attempts > 0).then_some(PARSE_RETRY_NOTE),
                    step: plan.current_step,
                    failed_total: plan.failures.total(),
                    redirect: redirect_line.as_deref(),
                    steer: steer_lines.as_deref(),
                };
                match planner.plan_next(&context).await {
                    Ok(reply) => break Ok(reply),
                    Err(e) => {
                        attempts += 1;
                        let retryable = matches!(
                            e.kind,
                            crate::llm_planner::PlannerErrorKind::Transient
                                | crate::llm_planner::PlannerErrorKind::Parse
                        );
                        if !retryable || attempts >= 2 {
                            break Err(e);
                        }
                        if e.kind == crate::llm_planner::PlannerErrorKind::Transient {
                            tokio::time::sleep(if cfg!(test) {
                                std::time::Duration::from_millis(5)
                            } else {
                                std::time::Duration::from_secs(1)
                            })
                            .await;
                        }
                    }
                }
            };
            if let Ok(reply) = &reply {
                plan.meter_planner_usage(reply.usage);
            }
            match reply {
                Ok(reply) => match reply.action.filter(|_| !reply.done) {
                    Some(action) => BrowserAction {
                        action_id: candidate.action_id,
                        grant_id: candidate.grant_id,
                        ..action
                    },
                    None => {
                        let payload = reply.done_payload.unwrap_or_default();
                        return finish_from_done(plan, last_observation, &payload);
                    }
                },
                // A selected planner that cannot answer ends the run; it never
                // degrades to a silent `Observe`.
                Err(e) => {
                    warn!(kind = e.kind.as_str(), error = %e, "llm planner failed; stopping the run");
                    plan.status = PlanStatus::Failed;
                    return PlanStepResult::Stopped {
                        reason: BrowserFailureKind::PlannerUnavailable
                            .reason(&format!("({})", e.kind.as_str())),
                        kind: BrowserFailureKind::PlannerUnavailable,
                    };
                }
            }
        }
    };

    // The widened verbs are checked here, after the planner and before the
    // risk gate. A rejection is a recoverable failure the planner is told about;
    // it counts as a step and toward the failure budget and never reaches Quarry.
    let action = if planner.is_some() {
        let proposed_name = action.action_type.as_str();
        let allowed = |url: &str| plan.is_domain_allowed(url);
        match admission::admit(
            action,
            plan.config.widened_actions,
            &plan.history,
            allowed,
        ) {
            Ok(admitted) => admitted,
            Err(rejection) => {
                warn!(
                    plan_id = %plan.config.plan_id,
                    kind = rejection.code(),
                    "browser action proposal rejected before Quarry"
                );
                let verdict = plan.failures.record(ActionFailure {
                    step: plan.current_step,
                    action_type: proposed_name,
                    kind: rejection.code(),
                    advice: rejection.advice(),
                });
                if let FailureVerdict::Stop(reason) = verdict {
                    plan.status = PlanStatus::Failed;
                    return PlanStepResult::Stopped {
                        reason,
                        kind: BrowserFailureKind::RepeatedFailure,
                    };
                }
                plan.status = PlanStatus::Executing;
                return PlanStepResult::Rejected;
            }
        }
    } else {
        action
    };

    if planner.is_some() && plan.no_progress.observe(&action, last_observation) {
        plan.status = PlanStatus::Failed;
        return PlanStepResult::Stopped {
            reason: BrowserFailureKind::NoProgress
                .reason("the same action was repeated on an unchanged page"),
            kind: BrowserFailureKind::NoProgress,
        };
    }

    let result = gate_risky_action(plan, last_observation, action, sink).await;
    if let PlanStepResult::Action(dispatched) = &result {
        plan.note_dispatch(dispatched, last_observation);
    }
    result
}

/// What a planner `done` means for the run (the honest `done`): `success`
/// completes with the answer as the summary; `blocked` and `incomplete` end the
/// run `Failed` with their stable prefixes. A `done` over a page that still
/// carries a strong challenge is a block whatever the planner called it.
fn finish_from_done(
    plan: &mut AgentPlan,
    last_observation: Option<&BrowserObservation>,
    payload: &crate::llm_planner::DonePayload,
) -> PlanStepResult {
    use crate::llm_planner::DoneOutcome;
    let stop = |plan: &mut AgentPlan, kind: BrowserFailureKind, detail: &str| {
        plan.status = PlanStatus::Failed;
        PlanStepResult::Stopped {
            reason: kind.reason(detail),
            kind,
        }
    };
    if let Some(reason) = blocked_by_bot_wall(last_observation) {
        plan.status = PlanStatus::Failed;
        return PlanStepResult::Stopped {
            reason,
            kind: BrowserFailureKind::Blocked,
        };
    }
    match payload.outcome {
        DoneOutcome::Success => {
            plan.status = PlanStatus::Completed;
            PlanStepResult::Completed(if payload.answer.is_empty() {
                "agent reported task complete".to_owned()
            } else {
                payload.answer.clone()
            })
        }
        DoneOutcome::Blocked => {
            let detail = if payload.answer.is_empty() {
                "the planner reported the task blocked"
            } else {
                payload.answer.as_str()
            };
            stop(plan, BrowserFailureKind::Blocked, detail)
        }
        DoneOutcome::Incomplete => {
            let detail = if payload.answer.is_empty() {
                "the planner reported the task incomplete"
            } else {
                payload.answer.as_str()
            };
            stop(plan, BrowserFailureKind::Incomplete, detail)
        }
    }
}

/// Appended to the planner request after an unreadable reply.
const PARSE_RETRY_NOTE: &str =
    "Your previous reply could not be read. Reply with ONLY one JSON object that matches the schema.";

/// The planner's rule for a strong challenge ends in `done` (the schema has no
/// separate blocked action). A `done` over a page that still carries a strong
/// signal is therefore the planner reporting the task blocked, not finished, so
/// the run fails instead of completing. Typed fields only: no page text.
fn blocked_by_bot_wall(last_observation: Option<&BrowserObservation>) -> Option<String> {
    let challenge = last_observation?
        .challenge
        .as_ref()
        .filter(|challenge| challenge.is_strong())?;
    Some(BrowserFailureKind::Blocked.reason(&format!(
        "the site refuses automation ({challenge})"
    )))
}

/// Phase 5 run-level gate: reusing a persistent, cookie-bearing Quarry
/// profile (`PlanConfig.profile_id.is_some() && !zdr`) is itself a listed
/// risk category ("persistent cookie use", plan capability #8) — gated once,
/// the first time `decide_next_action` runs for this plan, rather than once
/// per action. Returns `None` when there is nothing to gate (already gated,
/// ZDR, or no profile) so the caller falls through to normal planning.
async fn gate_persistent_cookie_use(
    plan: &mut AgentPlan,
    sink: Option<&dyn BrowserEventSink>,
) -> Option<PlanStepResult> {
    if plan.cookie_use_gate_done {
        return None;
    }
    plan.cookie_use_gate_done = true;
    if plan.config.zdr {
        return None;
    }
    let profile_id = plan.config.profile_id.clone()?;

    let detail = RiskyActionDetail {
        action_id: String::new(),
        action_type: "start_run".to_owned(),
        url: plan.config.start_url.clone().unwrap_or_default(),
        selector: String::new(),
        risk_category: RiskCategory::PersistentCookieUse,
        reason: format!(
            "run reuses persistent browser profile '{profile_id}' — cookies/session state carry over from prior sessions"
        ),
    };
    match request_approval(&plan.config, &detail, sink).await {
        // Approved — nothing to report; the caller falls through to normal
        // planning for this step.
        ApprovalOutcome::Granted => None,
        // A redirect is for a planned action; the run-level cookie gate has
        // none to replace, so it reads as the denial it is.
        ApprovalOutcome::Denied(reason) | ApprovalOutcome::Redirect(reason) => {
            plan.status = PlanStatus::Aborted;
            Some(PlanStepResult::Aborted {
                reason: format!("persistent cookie use denied: {reason}"),
                abort_reason: BrowserAbortReason::ApprovalDenied,
            })
        }
        ApprovalOutcome::TimedOut => {
            plan.status = PlanStatus::Aborted;
            Some(PlanStepResult::Aborted {
                reason: "persistent cookie use approval timed out".to_owned(),
                abort_reason: BrowserAbortReason::ApprovalTimedOut,
            })
        }
    }
}

/// Phase 5 per-action gate: classify `action` from deterministic browser
/// evidence with planner self-report as an additive fallback and, if risky,
/// request approval before it is returned for
/// dispatch. Approving proceeds with `action` unchanged. Denial or timeout
/// **aborts the run** rather than letting the planner silently try a
/// different action the human never saw — the safest, simplest behavior for
/// this stage (a human wanting the run to continue after a denial can start
/// a new one with a narrower goal).
async fn gate_risky_action(
    plan: &mut AgentPlan,
    last_observation: Option<&BrowserObservation>,
    action: BrowserAction,
    sink: Option<&dyn BrowserEventSink>,
) -> PlanStepResult {
    let focus = plan.focus_hint.clone();
    let Some(category) = classify_action_risk_with_focus(&action, last_observation, focus.as_ref())
    else {
        return PlanStepResult::Action(action);
    };

    let detail = RiskyActionDetail {
        action_id: action.action_id.clone(),
        action_type: action.action_type.as_str().to_owned(),
        url: action.url.clone(),
        selector: action.selector.clone(),
        risk_category: category,
        reason: risk_reason(category, &action),
    };
    match request_approval(&plan.config, &detail, sink).await {
        ApprovalOutcome::Granted => PlanStepResult::Action(action),
        // E3.2-c: the person rejected this action and said what to do instead.
        // The action is dropped (nothing reaches Quarry) and the planner plans
        // again over the same page; the next action meets every gate afresh,
        // so a redirect never stands in for an approval. When the flag is off,
        // no planner reads it, the budget is spent or no text is left, it is
        // the denial it always was.
        ApprovalOutcome::Redirect(text) if plan.accept_redirect(&action, &text) => {
            warn!(
                plan_id = %plan.config.plan_id,
                redirects = plan.redirects_used,
                "browser action rejected with a redirect; planning again"
            );
            plan.status = PlanStatus::Executing;
            PlanStepResult::Rejected
        }
        ApprovalOutcome::Denied(reason) | ApprovalOutcome::Redirect(reason) => {
            plan.status = PlanStatus::Aborted;
            PlanStepResult::Aborted {
                reason: format!("browser action denied: {reason}"),
                abort_reason: BrowserAbortReason::ApprovalDenied,
            }
        }
        ApprovalOutcome::TimedOut => {
            plan.status = PlanStatus::Aborted;
            PlanStepResult::Aborted {
                reason: "browser action approval timed out".to_owned(),
                abort_reason: BrowserAbortReason::ApprovalTimedOut,
            }
        }
    }
}

/// Route a gate request to the sink, failing CLOSED when none is wired. In
/// production `execute_step`'s gRPC handler always constructs a real sink
/// (`grpc.rs`); `None` is only reachable from a caller (e.g. a unit test)
/// that deliberately omits one, and a classified-risky action must never
/// silently proceed unattended just because nothing was there to ask.
async fn request_approval(
    config: &PlanConfig,
    detail: &RiskyActionDetail,
    sink: Option<&dyn BrowserEventSink>,
) -> ApprovalOutcome {
    match sink {
        Some(sink) => sink.require_approval(config, detail).await,
        None => ApprovalOutcome::Denied(
            "no approval sink configured to gate this risky browser action".to_owned(),
        ),
    }
}

/// Poll `state` for a user-initiated cancel or pause (Phase 2 B5) and block
/// accordingly. Returns `Some(PlanStepResult::Aborted { .. })` when the run was
/// cancelled (whether immediately or while paused); returns `None` once the
/// run is (or becomes) `Running`, so the caller proceeds to the planner gate.
///
/// No-op when `state` is absent or the plan has no `run_id` — mirrors the
/// existing best-effort shape of `BrowserEventSink`: a caller that never
/// wires a `StateStore` (e.g. existing tests, or a loop driven outside
/// `ExecuteStep`) sees unchanged behavior.
async fn wait_out_pause_or_cancel(
    plan: &mut AgentPlan,
    state: Option<&crate::state::StateStore>,
    sink: Option<&dyn BrowserEventSink>,
) -> Option<PlanStepResult> {
    let state = state?;
    if plan.config.run_id.is_empty() {
        return None;
    }

    let mut announced_paused = false;
    loop {
        match state.get_or_create(&plan.config.run_id).status {
            crate::state::RunStatus::Cancelled => {
                plan.status = PlanStatus::Aborted;
                return Some(PlanStepResult::Aborted {
                    reason: "cancelled by user".to_owned(),
                    abort_reason: BrowserAbortReason::Cancelled,
                });
            }
            crate::state::RunStatus::Paused => {
                if !announced_paused {
                    if let Some(sink) = sink {
                        sink.run_paused(&plan.config).await;
                    }
                    announced_paused = true;
                }
                tokio::time::sleep(pause_poll_interval()).await;
            }
            _ => {
                if announced_paused {
                    if let Some(sink) = sink {
                        sink.run_resumed(&plan.config).await;
                    }
                }
                take_steer_texts(plan, state);
                return None;
            }
        }
    }
}

/// E3.3-f: at a step boundary the run is neither cancelled nor paused: move what a
/// person queued for it into the plan, for the next planner call. Only when steering is
/// on for this plan, a planner will read it and the run is not ZDR; otherwise the queue
/// is left alone. It is text and nothing else: a pending approval is decided by the
/// approval gate, never by this, and the allowed domains are the plan's alone. At most
/// `MAX_QUEUED_MESSAGES` are kept (the newest win).
fn take_steer_texts(plan: &mut AgentPlan, state: &crate::state::StateStore) {
    if !plan.config.steer || !plan.planner_active || plan.config.zdr {
        return;
    }
    let taken = state.take_steer(&plan.config.run_id);
    if taken.is_empty() {
        return;
    }
    for text in taken {
        if let Some(text) = bound_steer_text(&text) {
            plan.steer_pending.push(text);
        }
    }
    let keep = mp_contracts::queued_limits::MAX_QUEUED_MESSAGES;
    if plan.steer_pending.len() > keep {
        let drop = plan.steer_pending.len() - keep;
        plan.steer_pending.drain(..drop);
    }
    tracing::debug!(
        plan_id = %plan.config.plan_id,
        messages = plan.steer_pending.len(),
        "steer text taken at the step boundary"
    );
}

/// In-memory plan store for tracking active browser-agent plans.
#[derive(Clone, Default)]
pub struct PlanStore {
    plans: Arc<Mutex<HashMap<String, AgentPlan>>>,
}

impl PlanStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create(&self, config: PlanConfig) -> AgentPlan {
        let plan = AgentPlan::new(config);
        let mut lock = self
            .plans
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        lock.insert(plan.config.plan_id.clone(), plan.clone());
        plan
    }

    pub fn get(&self, plan_id: &str) -> Option<AgentPlan> {
        let lock = self
            .plans
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        lock.get(plan_id).cloned()
    }

    pub fn update(&self, plan: AgentPlan) {
        let mut lock = self
            .plans
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        lock.insert(plan.config.plan_id.clone(), plan);
    }

    pub fn abort(&self, plan_id: &str) -> bool {
        let mut lock = self
            .plans
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(plan) = lock.get_mut(plan_id) {
            if !plan.status.is_terminal() {
                plan.status = PlanStatus::Aborted;
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> PlanConfig {
        PlanConfig {
            plan_id: "plan_001".to_owned(),
            grant_id: "grant_001".to_owned(),
            run_id: "run_001".to_owned(),
            org_id: "org_test".to_owned(),
            system_prompt: "Navigate and extract data".to_owned(),
            max_steps: 10,
            max_runtime_s: 60,
            allowed_domains: vec!["example.com".to_owned()],
            stop_criteria: String::new(),
            require_approval: false,
            max_cost_usd: None,
            zdr: false,
            profile_id: None,
            start_url: None,
            postcondition: String::new(),
            ..Default::default()
        }
    }

    #[test]
    fn new_plan_starts_in_planning_status() {
        let plan = AgentPlan::new(test_config());
        assert_eq!(plan.status, PlanStatus::Planning);
        assert_eq!(plan.current_step, 0);
    }

    #[test]
    fn first_action_is_observe() {
        let mut plan = AgentPlan::new(test_config());
        let result = plan_next_action(&mut plan, None);
        match result {
            PlanStepResult::Action(action) => {
                assert_eq!(action.action_type, ActionType::Observe);
                assert_eq!(plan.current_step, 1);
            }
            _ => panic!("expected Action"),
        }
    }

    #[test]
    fn first_action_navigates_when_a_start_url_is_configured() {
        // Phase 2, found live: a freshly `start_run`'d Quarry lease has no
        // page loaded, so an unconditional first `Observe` fails with
        // "no current page; call goto() first". `start_url` fixes this.
        let mut config = test_config();
        config.start_url = Some("https://example.com/landing".to_owned());
        let mut plan = AgentPlan::new(config);
        let result = plan_next_action(&mut plan, None);
        match result {
            PlanStepResult::Action(action) => {
                assert_eq!(action.action_type, ActionType::Goto);
                assert_eq!(action.url, "https://example.com/landing");
                assert_eq!(plan.current_step, 1);
            }
            _ => panic!("expected Action"),
        }
    }

    #[test]
    fn first_action_ignores_a_blank_start_url() {
        let mut config = test_config();
        config.start_url = Some("   ".to_owned());
        let mut plan = AgentPlan::new(config);
        let result = plan_next_action(&mut plan, None);
        match result {
            PlanStepResult::Action(action) => assert_eq!(action.action_type, ActionType::Observe),
            _ => panic!("expected Action"),
        }
    }

    #[test]
    fn max_steps_exhausts_the_plan_without_reporting_completion() {
        let mut config = test_config();
        config.max_steps = 1;
        let mut plan = AgentPlan::new(config);
        plan.current_step = 1;
        let result = plan_next_action(&mut plan, None);
        match result {
            PlanStepResult::ResourceExhausted { reason, limit } => {
                assert!(reason.contains("max_steps"));
                assert_eq!(limit, BrowserResourceLimit::MaxSteps);
            }
            _ => panic!("expected resource exhaustion"),
        }
        assert_eq!(plan.status, PlanStatus::Failed);
    }

    #[test]
    fn max_runtime_exhausts_the_plan_without_reporting_completion() {
        let mut config = test_config();
        config.max_runtime_s = 1;
        let mut plan = AgentPlan::new(config);
        plan.started_at = Instant::now()
            .checked_sub(std::time::Duration::from_secs(1))
            .expect("a one-second monotonic offset is representable in this test");

        let result = plan_next_action(&mut plan, None);
        match result {
            PlanStepResult::ResourceExhausted { reason, limit } => {
                assert!(reason.contains("max_runtime_s"));
                assert_eq!(limit, BrowserResourceLimit::Runtime);
            }
            _ => panic!("expected resource exhaustion"),
        }
        assert_eq!(plan.status, PlanStatus::Failed);
    }

    #[test]
    fn failed_observation_stops_plan() {
        let mut plan = AgentPlan::new(test_config());
        let obs = BrowserObservation {
            observation_id: "obs_1".to_owned(),
            action_id: "act_1".to_owned(),
            grant_id: "grant_001".to_owned(),
            status: ObservationStatus::Failed,
            page_url: String::new(),
            page_title: String::new(),
            extracted_text: String::new(),
            screenshot_ref: String::new(),
            dom_snapshot_ref: String::new(),
            snapshot_generation: None,
            snapshot_targets: vec![],
            error_message: "element not found".to_owned(),
            challenge: None,
            redaction_rule_version: None,
        };
        let result = plan_next_action(&mut plan, Some(&obs));
        match result {
            PlanStepResult::Failed(msg) => {
                assert!(msg.contains("element not found"));
            }
            _ => panic!("expected Failed"),
        }
    }

    #[test]
    fn redaction_stamp_guard_is_off_by_default_and_warns_once() {
        let unstamped = page_with_challenge(None);
        let stamped = BrowserObservation {
            redaction_rule_version: Some("qr-redact/4".to_owned()),
            ..page_with_challenge(None)
        };
        let off = test_config();
        assert!(!off.require_quarry_redaction_stamp, "default off");
        let mut warned = false;
        assert_eq!(redaction_stamp_failure(&off, &unstamped, &mut warned), None);
        assert!(warned, "the first missing stamp of a run is the one that is logged");
        assert_eq!(redaction_stamp_failure(&off, &stamped, &mut warned), None);

        let on = PlanConfig {
            require_quarry_redaction_stamp: true,
            ..test_config()
        };
        let mut warned = false;
        assert_eq!(
            redaction_stamp_failure(&on, &unstamped, &mut warned),
            Some(REDACTION_STAMP_MISSING_REASON)
        );
        assert_eq!(redaction_stamp_failure(&on, &stamped, &mut warned), None);
    }

    #[test]
    fn stop_criteria_completes_plan() {
        let mut config = test_config();
        config.stop_criteria = "success_marker".to_owned();
        let mut plan = AgentPlan::new(config);
        let obs = BrowserObservation {
            observation_id: "obs_1".to_owned(),
            action_id: "act_1".to_owned(),
            grant_id: "grant_001".to_owned(),
            status: ObservationStatus::Success,
            page_url: "https://example.com".to_owned(),
            page_title: "Test".to_owned(),
            extracted_text: "result: success_marker found".to_owned(),
            screenshot_ref: String::new(),
            dom_snapshot_ref: String::new(),
            snapshot_generation: None,
            snapshot_targets: vec![],
            error_message: String::new(),
            challenge: None,
            redaction_rule_version: None,
        };
        let result = plan_next_action(&mut plan, Some(&obs));
        match result {
            PlanStepResult::Completed(reason) => {
                assert!(reason.contains("stop criteria"));
            }
            _ => panic!("expected Completed"),
        }
    }

    #[test]
    fn approval_required_pauses_plan() {
        let mut config = test_config();
        config.require_approval = true;
        let mut plan = AgentPlan::new(config);
        let result = plan_next_action(&mut plan, None);
        assert!(matches!(result, PlanStepResult::WaitingApproval));
    }

    #[test]
    fn domain_allowlist_restricts_navigation() {
        let plan = AgentPlan::new(test_config());
        assert!(plan.is_domain_allowed("https://example.com/page"));
        assert!(!plan.is_domain_allowed("https://evil.com/page"));
    }

    #[test]
    fn empty_allowlist_blocks_every_navigation() {
        let mut config = test_config();
        config.allowed_domains = vec![];
        let plan = AgentPlan::new(config);
        assert!(!plan.is_domain_allowed("https://anything.com"));
    }

    #[test]
    fn plan_config_rejects_empty_or_untrusted_navigation_policy_before_launch() {
        let mut empty_policy = test_config();
        empty_policy.allowed_domains.clear();
        assert!(validate_plan_config(&empty_policy)
            .expect_err("an empty policy must not mean unrestricted browser access")
            .contains("non-empty"));

        let mut arbitrary_start = test_config();
        arbitrary_start.start_url = Some("https://evil.example/steal".to_owned());
        assert!(validate_plan_config(&arbitrary_start)
            .expect_err("a caller-selected URL outside the broker policy must fail")
            .contains("outside"));
    }

    #[test]
    fn canonical_domain_policy_rejects_urls_ips_and_wildcards() {
        for domains in [
            vec!["https://example.com".to_owned()],
            vec!["*.example.com".to_owned()],
            vec!["127.0.0.1".to_owned()],
            vec!["localhost".to_owned()],
        ] {
            assert!(
                canonicalize_allowed_domains(&domains).is_err(),
                "{domains:?}"
            );
        }
    }

    #[test]
    fn allowlist_rejects_lookalike_domains() {
        // Regression: the previous substring-match implementation
        // accepted attacker-controlled hosts that contained the allowlist
        // entry as a substring (e.g. "evil-example.com").
        let plan = AgentPlan::new(test_config());
        assert!(!plan.is_domain_allowed("https://evil-example.com/page"));
        assert!(!plan.is_domain_allowed("https://example.com.attacker.io/"));
        assert!(!plan.is_domain_allowed("https://exampleXcom/page"));
    }

    #[test]
    fn allowlist_accepts_legitimate_subdomains() {
        let plan = AgentPlan::new(test_config());
        assert!(plan.is_domain_allowed("https://api.example.com/v1/foo"));
        assert!(plan.is_domain_allowed("https://www.example.com/"));
    }

    #[test]
    fn allowlist_handles_userinfo_and_ports() {
        let plan = AgentPlan::new(test_config());
        assert!(plan.is_domain_allowed("https://user:pass@example.com:8443/page"));
        // The userinfo must not be confused for the host.
        assert!(!plan.is_domain_allowed("https://example.com@evil.io/"));
    }

    #[test]
    fn allowlist_rejects_urls_without_scheme() {
        let plan = AgentPlan::new(test_config());
        assert!(!plan.is_domain_allowed("example.com/page"));
        assert!(!plan.is_domain_allowed(""));
    }

    #[test]
    fn allowlist_match_is_case_insensitive() {
        let plan = AgentPlan::new(test_config());
        assert!(plan.is_domain_allowed("https://EXAMPLE.com/"));
        assert!(plan.is_domain_allowed("https://Example.COM/"));
    }

    #[test]
    fn plan_store_crud() {
        let store = PlanStore::new();
        let config = test_config();
        let _ = store.create(config);
        assert_eq!(store.get("plan_001").unwrap().status, PlanStatus::Planning);
        assert!(store.abort("plan_001"));
        assert_eq!(store.get("plan_001").unwrap().status, PlanStatus::Aborted);
        assert!(!store.abort("plan_001")); // already terminal
    }

    // ── Phase 2 B5: user-initiated pause/resume/cancel gate ────────────────

    #[derive(Default)]
    struct RecordingSink {
        paused: std::sync::atomic::AtomicUsize,
        resumed: std::sync::atomic::AtomicUsize,
    }

    #[tonic::async_trait]
    impl BrowserEventSink for RecordingSink {
        async fn action_dispatched(&self, _config: &PlanConfig, _action: &BrowserAction) {}
        async fn observation_received(
            &self,
            _config: &PlanConfig,
            _observation: &BrowserObservation,
        ) {
        }
        async fn run_paused(&self, _config: &PlanConfig) {
            self.paused
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        async fn run_resumed(&self, _config: &PlanConfig) {
            self.resumed
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        // Not exercised by the pause/resume/cancel suite below — every
        // scenario there uses non-risky actions, so this is never called.
        // Phase 5's own gate tests use a dedicated `ApprovingSink`/
        // `DecidingSink` further down instead of this fixture.
        async fn require_approval(
            &self,
            _config: &PlanConfig,
            _detail: &RiskyActionDetail,
        ) -> ApprovalOutcome {
            ApprovalOutcome::Granted
        }
    }

    #[tokio::test]
    async fn wait_out_pause_or_cancel_is_noop_without_a_state_store() {
        let mut plan = AgentPlan::new(test_config());
        let result = wait_out_pause_or_cancel(&mut plan, None, None).await;
        assert!(result.is_none());
        assert_eq!(plan.status, PlanStatus::Planning);
    }

    #[tokio::test]
    async fn wait_out_pause_or_cancel_is_noop_without_a_run_id() {
        let mut config = test_config();
        config.run_id = String::new();
        let mut plan = AgentPlan::new(config);
        let store = crate::state::StateStore::new();
        let _ = store.cancel("run_001", None); // a different (empty) run id — irrelevant
        let result = wait_out_pause_or_cancel(&mut plan, Some(&store), None).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn wait_out_pause_or_cancel_aborts_immediately_on_cancelled() {
        let store = crate::state::StateStore::new();
        let _ = store.cancel("run_001", Some("user_stop".to_owned()));
        let mut plan = AgentPlan::new(test_config());
        let result = wait_out_pause_or_cancel(&mut plan, Some(&store), None).await;
        match result {
            Some(PlanStepResult::Aborted {
                reason,
                abort_reason,
            }) => {
                assert_eq!(reason, "cancelled by user");
                assert_eq!(abort_reason, BrowserAbortReason::Cancelled);
            }
            other => panic!(
                "expected Aborted, got a different result: {}",
                other.is_some()
            ),
        }
        assert_eq!(plan.status, PlanStatus::Aborted);
    }

    #[tokio::test]
    async fn wait_out_pause_or_cancel_blocks_until_resumed_and_announces_once() {
        let store = crate::state::StateStore::new();
        let _ = store.pause("run_001");
        let sink = RecordingSink::default();

        let resumer_store = store.clone();
        let resumer = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            resumer_store.update(crate::state::RunSnapshot {
                run_id: "run_001".to_owned(),
                step_index: 0,
                status: crate::state::RunStatus::Running,
                last_error: None,
            });
        });

        let mut plan = AgentPlan::new(test_config());
        let result = wait_out_pause_or_cancel(&mut plan, Some(&store), Some(&sink)).await;
        resumer.await.expect("resumer task");

        assert!(result.is_none(), "run resumed — loop should proceed");
        assert_eq!(sink.paused.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(sink.resumed.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn wait_out_pause_or_cancel_aborts_while_paused_when_later_cancelled() {
        let store = crate::state::StateStore::new();
        let _ = store.pause("run_001");
        let sink = RecordingSink::default();

        let canceller_store = store.clone();
        let canceller = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            let _ = canceller_store.cancel("run_001", Some("user_stop".to_owned()));
        });

        let mut plan = AgentPlan::new(test_config());
        let result = wait_out_pause_or_cancel(&mut plan, Some(&store), Some(&sink)).await;
        canceller.await.expect("canceller task");

        assert!(matches!(
            result,
            Some(PlanStepResult::Aborted {
                abort_reason: BrowserAbortReason::Cancelled,
                ..
            })
        ));
        assert_eq!(plan.status, PlanStatus::Aborted);
        assert_eq!(sink.paused.load(std::sync::atomic::Ordering::SeqCst), 1);
        // Cancelled from Paused, never flipped through Running — no resume announced.
        assert_eq!(sink.resumed.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    // ── Phase 5: in-loop HITL approval gate (risk classification's own unit
    // tests live in browser_risk.rs, next to the classifier) ───────────────

    /// Test double for the Phase 5 approval gate: replays a scripted sequence
    /// of `ApprovalOutcome`s (one per `require_approval` call, FIFO) and
    /// records the `RiskyActionDetail` each call was made with, so tests can
    /// assert BOTH the loop's behavior and WHAT it asked approval for.
    #[derive(Default)]
    struct ScriptedApprovalSink {
        outcomes: std::sync::Mutex<std::collections::VecDeque<ApprovalOutcome>>,
        calls: std::sync::Mutex<Vec<RiskyActionDetail>>,
    }

    impl ScriptedApprovalSink {
        fn new(outcomes: Vec<ApprovalOutcome>) -> Self {
            Self {
                outcomes: std::sync::Mutex::new(outcomes.into()),
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn call_count(&self) -> usize {
            self.calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len()
        }
    }

    #[tonic::async_trait]
    impl BrowserEventSink for ScriptedApprovalSink {
        async fn action_dispatched(&self, _config: &PlanConfig, _action: &BrowserAction) {}
        async fn observation_received(
            &self,
            _config: &PlanConfig,
            _observation: &BrowserObservation,
        ) {
        }
        async fn run_paused(&self, _config: &PlanConfig) {}
        async fn run_resumed(&self, _config: &PlanConfig) {}
        async fn require_approval(
            &self,
            _config: &PlanConfig,
            detail: &RiskyActionDetail,
        ) -> ApprovalOutcome {
            self.calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(detail.clone());
            self.outcomes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pop_front()
                .unwrap_or_else(|| ApprovalOutcome::Denied("sink exhausted".to_owned()))
        }
    }

    #[tokio::test]
    async fn gate_persistent_cookie_use_skips_when_zdr() {
        let mut config = test_config();
        config.zdr = true;
        config.profile_id = Some("prof_1".to_owned());
        let mut plan = AgentPlan::new(config);
        let sink = ScriptedApprovalSink::new(vec![]);

        let result = gate_persistent_cookie_use(&mut plan, Some(&sink)).await;
        assert!(result.is_none());
        assert_eq!(sink.call_count(), 0);
    }

    #[tokio::test]
    async fn gate_persistent_cookie_use_skips_when_no_profile() {
        let mut plan = AgentPlan::new(test_config());
        let sink = ScriptedApprovalSink::new(vec![]);

        let result = gate_persistent_cookie_use(&mut plan, Some(&sink)).await;
        assert!(result.is_none());
        assert_eq!(sink.call_count(), 0);
    }

    #[tokio::test]
    async fn gate_persistent_cookie_use_asks_once_and_proceeds_on_grant() {
        let mut config = test_config();
        config.profile_id = Some("prof_1".to_owned());
        let mut plan = AgentPlan::new(config);
        let sink = ScriptedApprovalSink::new(vec![ApprovalOutcome::Granted]);

        let first = gate_persistent_cookie_use(&mut plan, Some(&sink)).await;
        assert!(
            first.is_none(),
            "granted — nothing to report, proceed normally"
        );
        assert_eq!(sink.call_count(), 1);

        // A second call on the same plan must NOT ask again.
        let second = gate_persistent_cookie_use(&mut plan, Some(&sink)).await;
        assert!(second.is_none());
        assert_eq!(sink.call_count(), 1, "gated exactly once per run");
    }

    #[tokio::test]
    async fn gate_persistent_cookie_use_aborts_the_run_on_denial() {
        let mut config = test_config();
        config.profile_id = Some("prof_1".to_owned());
        let mut plan = AgentPlan::new(config);
        let sink = ScriptedApprovalSink::new(vec![ApprovalOutcome::Denied("no".to_owned())]);

        let result = gate_persistent_cookie_use(&mut plan, Some(&sink)).await;
        assert!(matches!(
            result,
            Some(PlanStepResult::Aborted {
                abort_reason: BrowserAbortReason::ApprovalDenied,
                ..
            })
        ));
        assert_eq!(plan.status, PlanStatus::Aborted);
    }

    #[tokio::test]
    async fn decide_next_action_gates_a_risky_first_navigation_and_proceeds_on_grant() {
        let mut config = test_config();
        config.start_url = Some("https://example.com/login".to_owned());
        let mut plan = AgentPlan::new(config);
        let sink = ScriptedApprovalSink::new(vec![ApprovalOutcome::Granted]);

        let result = decide_next_action(&mut plan, None, None, None, Some(&sink)).await;
        match result {
            PlanStepResult::Action(action) => {
                assert_eq!(action.action_type, ActionType::Goto);
                assert_eq!(action.url, "https://example.com/login");
            }
            _ => panic!("expected Action"),
        }
        assert_eq!(sink.call_count(), 1);
        let calls = sink.calls.lock().unwrap();
        assert_eq!(calls[0].risk_category, RiskCategory::Login);
    }

    #[tokio::test]
    async fn decide_next_action_aborts_the_run_when_a_risky_action_is_denied() {
        let mut config = test_config();
        config.start_url = Some("https://example.com/login".to_owned());
        let mut plan = AgentPlan::new(config);
        let sink = ScriptedApprovalSink::new(vec![ApprovalOutcome::Denied("not now".to_owned())]);

        let result = decide_next_action(&mut plan, None, None, None, Some(&sink)).await;
        match result {
            PlanStepResult::Aborted {
                reason,
                abort_reason,
            } => {
                assert!(reason.contains("denied"));
                assert_eq!(abort_reason, BrowserAbortReason::ApprovalDenied);
            }
            _ => panic!("expected Aborted"),
        }
        assert_eq!(plan.status, PlanStatus::Aborted);
    }

    #[tokio::test]
    async fn gate_risky_action_records_deterministic_checkout_not_a_planner_login_label() {
        let mut plan = AgentPlan::new(test_config());
        let sink = ScriptedApprovalSink::new(vec![ApprovalOutcome::Granted]);
        let action = BrowserAction {
            action_id: "act_checkout".to_owned(),
            grant_id: "grant_1".to_owned(),
            action_type: ActionType::Click,
            selector: "button.continue".to_owned(),
            value: String::new(),
            url: "https://shop.example.com/checkout".to_owned(),
            max_wait_ms: 5_000,
            target: None,
            reason: "continue after sign-in".to_owned(),
            risk_category: Some(RiskCategory::Login),
        };

        assert!(matches!(
            gate_risky_action(&mut plan, None, action, Some(&sink)).await,
            PlanStepResult::Action(_)
        ));
        let calls = sink.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].risk_category, RiskCategory::Checkout);
    }

    #[tokio::test]
    async fn decide_next_action_aborts_the_run_when_approval_times_out() {
        let mut config = test_config();
        config.start_url = Some("https://example.com/checkout".to_owned());
        let mut plan = AgentPlan::new(config);
        let sink = ScriptedApprovalSink::new(vec![ApprovalOutcome::TimedOut]);

        let result = decide_next_action(&mut plan, None, None, None, Some(&sink)).await;
        match result {
            PlanStepResult::Aborted {
                reason,
                abort_reason,
            } => {
                assert!(reason.contains("timed out"));
                assert_eq!(abort_reason, BrowserAbortReason::ApprovalTimedOut);
            }
            _ => panic!("expected Aborted"),
        }
        assert_eq!(plan.status, PlanStatus::Aborted);
    }

    #[tokio::test]
    async fn decide_next_action_lets_non_risky_actions_through_without_asking() {
        let mut config = test_config();
        config.start_url = Some("https://example.com/about".to_owned());
        let mut plan = AgentPlan::new(config);
        // Would return "sink exhausted" (Denied) if ever called — proves the
        // non-risky path never reaches the gate at all.
        let sink = ScriptedApprovalSink::new(vec![]);

        let result = decide_next_action(&mut plan, None, None, None, Some(&sink)).await;
        assert!(matches!(result, PlanStepResult::Action(_)));
        assert_eq!(sink.call_count(), 0);
    }

    #[tokio::test]
    async fn decide_next_action_fails_closed_when_risky_and_no_sink_is_configured() {
        let mut config = test_config();
        config.start_url = Some("https://example.com/login".to_owned());
        let mut plan = AgentPlan::new(config);

        let result = decide_next_action(&mut plan, None, None, None, None).await;
        match result {
            PlanStepResult::Aborted {
                reason,
                abort_reason,
            } => {
                assert!(reason.contains("no approval sink"));
                assert_eq!(abort_reason, BrowserAbortReason::ApprovalDenied);
            }
            _ => panic!("expected Aborted (fail closed)"),
        }
        assert_eq!(plan.status, PlanStatus::Aborted);
    }

    fn strong_wall(recovery: ChallengeRecovery) -> BrowserChallenge {
        BrowserChallenge {
            kind: ChallengeKind::Waf,
            strength: ChallengeStrength::Strong,
            block_reason: Some(BlockReason::WafInterstitial),
            recovery,
            confidence_per_mille: 900,
        }
    }

    fn page_with_challenge(challenge: Option<BrowserChallenge>) -> BrowserObservation {
        BrowserObservation {
            observation_id: "obs_1".to_owned(),
            action_id: "act_0001".to_owned(),
            grant_id: "grant_001".to_owned(),
            status: ObservationStatus::Success,
            page_url: "https://example.com/".to_owned(),
            page_title: String::new(),
            extracted_text: String::new(),
            screenshot_ref: String::new(),
            dom_snapshot_ref: String::new(),
            snapshot_generation: None,
            snapshot_targets: vec![],
            error_message: String::new(),
            challenge,
            redaction_rule_version: None,
        }
    }

    #[test]
    fn done_over_a_strong_signal_is_blocked_and_over_anything_else_is_not() {
        let strong = page_with_challenge(Some(strong_wall(ChallengeRecovery::Exhausted)));
        let reason = blocked_by_bot_wall(Some(&strong)).expect("a strong signal blocks");
        assert!(reason.starts_with("blocked: the site refuses automation"), "{reason}");
        assert!(reason.contains("block_reason=waf_interstitial"), "{reason}");

        // A weak hint, an ordinary access state, no signal and no page at all
        // are all a normal `done`.
        for challenge in [
            Some(BrowserChallenge {
                strength: ChallengeStrength::Weak,
                ..strong_wall(ChallengeRecovery::NotApplicable)
            }),
            Some(BrowserChallenge {
                kind: ChallengeKind::Login,
                strength: ChallengeStrength::NonSignal,
                block_reason: None,
                ..strong_wall(ChallengeRecovery::NotApplicable)
            }),
            None,
        ] {
            assert_eq!(blocked_by_bot_wall(Some(&page_with_challenge(challenge))), None);
        }
        assert_eq!(blocked_by_bot_wall(None), None);
    }

    #[test]
    fn a_blocked_observation_with_a_challenge_stops_the_plan() {
        let mut plan = AgentPlan::new(test_config());
        let mut obs = page_with_challenge(Some(strong_wall(ChallengeRecovery::Exhausted)));
        obs.status = ObservationStatus::Blocked;
        obs.error_message = "the site refuses automation".to_owned();
        match plan_next_action(&mut plan, Some(&obs)) {
            PlanStepResult::Failed(reason) => assert!(reason.contains("action blocked"), "{reason}"),
            _ => panic!("expected Failed"),
        }
        assert_eq!(plan.status, PlanStatus::Failed);
    }

    #[tokio::test]
    async fn a_403_challenge_detected_ends_the_loop_blocked_without_a_retry() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/agent/runs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {"run_id": "run_abc", "lease_id": "lease_xyz"},
                "meta": {}, "error": null
            })))
            .mount(&server)
            .await;
        // Exactly one step: the refusal is not retried as a transient error.
        Mock::given(method("POST"))
            .and(path("/v1/agent/runs/run_abc/step"))
            .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
                "data": null, "meta": {},
                "error": {
                    "code": "CHALLENGE_DETECTED",
                    "message": "the site refuses automation",
                    "retryable": false,
                    "details": {"reason": "challenge_loop", "recovery": "exhausted",
                                "next": "user_takeover_or_other_source"}
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/v1/agent/runs/run_abc"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let mut config = test_config();
        config.start_url = Some("https://example.com/about".to_owned());
        let client = crate::quarry_agent::QuarryAgentClient::new(server.uri(), "tok").unwrap();
        let result = run_browser_agent_loop(config, Some(&client), None, None, None).await;

        assert_eq!(result.status, PlanStatus::Failed);
        assert!(result.summary.starts_with("action blocked:"), "{}", result.summary);
        assert!(!result.summary.contains("step failed"), "{}", result.summary);
        let last = result.observations.last().expect("the refusal is recorded");
        assert_eq!(last.status, ObservationStatus::Blocked);
        assert_eq!(last.page_url, "https://example.com/about");
        assert_eq!(
            last.challenge.as_ref().map(|c| (c.is_strong(), c.block_reason)),
            Some((true, Some(BlockReason::ChallengeLoop)))
        );
    }

    // ── WS11.S1: planner meter, credential deadline, typed Quarry limits ──

    /// A planner that always answers `observe` and reports a fixed usage.
    struct FixedUsagePlanner {
        tokens: u32,
        calls: std::sync::atomic::AtomicU32,
    }

    impl FixedUsagePlanner {
        fn new(tokens: u32) -> Self {
            Self {
                tokens,
                calls: std::sync::atomic::AtomicU32::new(0),
            }
        }

        fn calls(&self) -> u32 {
            self.calls.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[tonic::async_trait]
    impl crate::llm_planner::ActionPlanner for FixedUsagePlanner {
        async fn plan_next(
            &self,
            _ctx: &crate::llm_planner::PlannerContext<'_>,
        ) -> Result<crate::llm_planner::PlannerReply, crate::llm_planner::PlannerError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let action = BrowserAction {
                action_id: String::new(),
                grant_id: String::new(),
                action_type: ActionType::Observe,
                selector: String::new(),
                value: String::new(),
                url: String::new(),
                max_wait_ms: 5000,
                target: None,
                reason: String::new(),
                risk_category: None,
            };
            Ok(crate::llm_planner::PlannerReply::action(action).with_usage(
                crate::llm_planner::PlannerUsage {
                    input_tokens: self.tokens,
                    output_tokens: 0,
                },
            ))
        }
    }

    #[tokio::test]
    async fn meter_adds_actual_response_usage() {
        let mut config = test_config();
        config.max_planner_tokens = Some(10_000);
        let mut plan = AgentPlan::new(config);
        let planner = FixedUsagePlanner::new(300);
        let _ = decide_next_action(&mut plan, None, Some(&planner), None, None).await;
        let _ = decide_next_action(&mut plan, None, Some(&planner), None, None).await;
        let usage = plan.usage();
        assert_eq!(usage.planner_calls, 2);
        assert_eq!(usage.planner_input_tokens, 600);
        assert_eq!(usage.planner_output_tokens, 0);
        assert_eq!(usage.planner_tokens(), 600);
    }

    #[tokio::test]
    async fn meter_stops_before_overshoot() {
        let mut config = test_config();
        config.max_planner_tokens = Some(1000);
        let mut plan = AgentPlan::new(config);
        let planner = FixedUsagePlanner::new(400);
        for _ in 0..2 {
            assert!(matches!(
                decide_next_action(&mut plan, None, Some(&planner), None, None).await,
                PlanStepResult::Action(_)
            ));
        }
        // 800 spent, the last call cost 400: a third call could reach 1200.
        match decide_next_action(&mut plan, None, Some(&planner), None, None).await {
            PlanStepResult::ResourceExhausted { limit, .. } => {
                assert_eq!(limit, BrowserResourceLimit::Tokens);
            }
            _ => panic!("expected the token budget to stop the run"),
        }
        assert_eq!(planner.calls(), 2, "the stopped call was never made");
        assert_eq!(plan.status, PlanStatus::Failed);

        // No budget: the token check never stops it (two decisions, so the
        // no-progress detector, which needs three, stays out of it).
        let mut open = AgentPlan::new(test_config());
        for _ in 0..2 {
            assert!(matches!(
                decide_next_action(&mut open, None, Some(&planner), None, None).await,
                PlanStepResult::Action(_)
            ));
        }
    }

    #[tokio::test]
    async fn credential_deadline_stops_new_planner_calls() {
        let mut config = test_config();
        config.deadline_at = Instant::now().checked_sub(std::time::Duration::from_secs(1));
        let mut plan = AgentPlan::new(config);
        let planner = FixedUsagePlanner::new(10);
        match decide_next_action(&mut plan, None, Some(&planner), None, None).await {
            PlanStepResult::ResourceExhausted { limit, .. } => {
                assert_eq!(limit, BrowserResourceLimit::Credential);
            }
            _ => panic!("expected the credential deadline to stop the run"),
        }
        assert_eq!(planner.calls(), 0);

        // A deadline still ahead does not stop anything.
        let mut config = test_config();
        config.deadline_at = Some(Instant::now() + std::time::Duration::from_secs(60));
        let mut plan = AgentPlan::new(config);
        assert!(matches!(
            decide_next_action(&mut plan, None, Some(&planner), None, None).await,
            PlanStepResult::Action(_)
        ));
    }

    /// One step against a Quarry whose step answers `status` with `body`.
    async fn loop_against_step_error(
        status: u16,
        body: serde_json::Value,
    ) -> (BrowserAgentLoopResult, usize) {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/agent/runs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {"run_id": "run_abc", "lease_id": "lease_xyz"},
                "meta": {}, "error": null
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/agent/runs/run_abc/step"))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/v1/agent/runs/run_abc"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let mut config = test_config();
        config.start_url = Some("https://example.com/".to_owned());
        let client = crate::quarry_agent::QuarryAgentClient::new(server.uri(), "tok").unwrap();
        let result = run_browser_agent_loop(config, Some(&client), None, None, None).await;
        let step_calls = server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| r.url.path().ends_with("/step"))
            .count();
        (result, step_calls)
    }

    #[tokio::test]
    async fn quarry_402_maps_to_resource_limit_not_failed() {
        let (result, step_calls) = loop_against_step_error(
            402,
            serde_json::json!({
                "data": null, "meta": {},
                "error": {
                    "code": "BUDGET_EXHAUSTED", "message": "run budget exhausted",
                    "retryable": false,
                    "details": {"dimension": "cost", "limit": 1.0, "spent": 1.2,
                                "unit": "usd", "limit_source": "run_budget", "resumable": false}
                }
            }),
        )
        .await;
        assert_eq!(
            result.resource_limit,
            Some(BrowserResourceLimit::QuarryBudget(
                crate::quarry_agent::BudgetDimension::Cost
            ))
        );
        assert_eq!(result.summary, "quarry budget exhausted: cost");
        assert_eq!(step_calls, 1, "a 402 is never retried");
    }

    #[tokio::test]
    async fn quarry_429_is_backpressure_not_failed() {
        let (result, step_calls) = loop_against_step_error(
            429,
            serde_json::json!({
                "data": null, "meta": {},
                "error": {"code": "RATE_LIMITED", "message": "slow down", "retryable": true,
                          "details": {"reason": "capacity", "retry_after_seconds": 5}}
            }),
        )
        .await;
        assert_eq!(result.resource_limit, Some(BrowserResourceLimit::QuarryBackpressure));
        assert_eq!(step_calls, 1, "a 429 is not retried by the loop");

        // A 500 is still a plain failure.
        let (failed, _) = loop_against_step_error(
            500,
            serde_json::json!({"error": {"code": "INTERNAL", "message": "boom"}}),
        )
        .await;
        assert_eq!(failed.resource_limit, None);
        assert!(failed.summary.starts_with("browser-agent step failed"), "{}", failed.summary);
    }

    // -----------------------------------------------------------------------
    // WS8.G attach mode: the takeover wait, the disposition, the plan checks.
    // Virtual time (`start_paused`): the 1 s poll and the 600 s wait cost
    // nothing real.
    // -----------------------------------------------------------------------

    fn took_over() -> crate::quarry_agent::AgentClientError {
        let body = serde_json::json!({
            "data": null, "meta": {},
            "error": {"code": "USER_TOOK_OVER",
                      "message": "the person using the browser has taken control of this tab; this action was not run",
                      "retryable": false,
                      "details": {"scope": "tab", "tab_id": "tab_9", "tab_epoch": 2, "control_epoch": 5}},
        })
        .to_string();
        crate::quarry_agent::AgentClientError::Typed(
            crate::quarry_agent::parse_typed_error(409, &body).expect("envelope"),
        )
    }

    fn page(url: &str) -> crate::quarry_agent::WireObservation {
        crate::quarry_agent::WireObservation {
            url: url.to_owned(),
            ..Default::default()
        }
    }

    #[tokio::test(start_paused = true)]
    async fn user_took_over_pauses_then_resumes_after_give_back() {
        let probes = std::sync::atomic::AtomicU32::new(0);
        let started = tokio::time::Instant::now();
        let outcome = await_give_back(
            TAKEOVER_POLL_INTERVAL,
            std::time::Duration::from_secs(u64::from(DEFAULT_TAKEOVER_WAIT_S)),
            || {
                let n = probes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async move {
                    // Held for three checks, given back on the fourth.
                    if n < 3 {
                        Err(took_over())
                    } else {
                        Ok(page("https://example.com/after"))
                    }
                }
            },
            || false,
        )
        .await;
        let GiveBack::Resumed(observation) = outcome else {
            panic!("the run resumes once the tab is given back");
        };
        assert_eq!(observation.url, "https://example.com/after");
        assert_eq!(probes.load(std::sync::atomic::Ordering::SeqCst), 4);
        assert_eq!(started.elapsed(), std::time::Duration::from_secs(4), "one check a second");
    }

    #[tokio::test(start_paused = true)]
    async fn user_took_over_beyond_wait_fails_with_reason() {
        let probes = std::sync::atomic::AtomicU32::new(0);
        let started = tokio::time::Instant::now();
        let config = PlanConfig {
            max_runtime_s: 3600,
            ..test_config()
        };
        let wait = takeover_wait(&config, std::time::Duration::ZERO, None);
        assert_eq!(wait, std::time::Duration::from_secs(600), "D5 default");
        let outcome = await_give_back(
            TAKEOVER_POLL_INTERVAL,
            wait,
            || {
                probes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async { Err(took_over()) }
            },
            || false,
        )
        .await;
        assert!(matches!(outcome, GiveBack::TimedOut));
        assert_eq!(started.elapsed(), std::time::Duration::from_secs(600));
        assert_eq!(probes.load(std::sync::atomic::Ordering::SeqCst), 600);
        assert_eq!(
            took_over_timeout_reason(wait),
            "the user did not give control back within 600 s"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn takeover_wait_ends_on_cancel_and_on_another_refusal() {
        let checks = std::sync::atomic::AtomicU32::new(0);
        let outcome = await_give_back(
            TAKEOVER_POLL_INTERVAL,
            std::time::Duration::from_secs(600),
            || async { Err(took_over()) },
            || checks.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= 5,
        )
        .await;
        assert!(matches!(outcome, GiveBack::Cancelled));

        let closed = crate::quarry_agent::AgentClientError::Typed(
            crate::quarry_agent::parse_typed_error(
                404,
                r#"{"error":{"code":"NOT_FOUND","message":"pinned_tab_closed","retryable":false}}"#,
            )
            .unwrap(),
        );
        let mut closed = Some(closed);
        let outcome = await_give_back(
            TAKEOVER_POLL_INTERVAL,
            std::time::Duration::from_secs(600),
            || {
                let next = closed.take();
                async move { Err(next.unwrap_or_else(took_over)) }
            },
            || false,
        )
        .await;
        let GiveBack::Failed(error) = outcome else {
            panic!("a closed tab ends the wait");
        };
        assert_eq!(
            crate::quarry_agent::fixed_error_text(&error),
            "404 NOT_FOUND (pinned_tab_closed)"
        );
    }

    #[test]
    fn takeover_wait_is_capped_by_the_plan_and_the_attachment_runtime() {
        let secs = std::time::Duration::from_secs;
        let config = PlanConfig {
            max_runtime_s: 300,
            ..test_config()
        };
        // Never past the plan's runtime: 300 s total, 100 s spent.
        assert_eq!(takeover_wait(&config, secs(100), None), secs(200));
        // Nor past the attachment's own bound.
        assert_eq!(takeover_wait(&config, secs(0), Some(secs(90))), secs(90));
        // A configured wait below both wins.
        let short = PlanConfig {
            takeover_wait_s: 30,
            ..config.clone()
        };
        assert_eq!(takeover_wait(&short, secs(0), Some(secs(90))), secs(30));
        // Runtime spent: no wait at all.
        assert_eq!(takeover_wait(&config, secs(400), None), secs(0));
        // An unbounded plan keeps the D5 default.
        let unbounded = PlanConfig {
            max_runtime_s: 0,
            ..test_config()
        };
        assert_eq!(takeover_wait(&unbounded, secs(10_000), None), secs(600));
    }

    #[test]
    fn attach_is_released_only_on_completed_and_aborted_otherwise() {
        use crate::quarry_agent::AttachDisposition;
        assert_eq!(attach_disposition(&PlanStatus::Completed), AttachDisposition::Release);
        for status in [
            PlanStatus::Failed,
            PlanStatus::Aborted,
            PlanStatus::WaitingApproval,
            PlanStatus::Executing,
            PlanStatus::Planning,
        ] {
            assert_eq!(attach_disposition(&status), AttachDisposition::Abort, "{status:?}");
        }
    }

    #[test]
    fn attach_plans_need_a_session_domains_and_no_profile() {
        let attach = Some(AttachConfig {
            session_run_id: "run_user_1".to_owned(),
        });
        let ok = PlanConfig {
            attach: attach.clone(),
            ..test_config()
        };
        assert_eq!(validate_plan_config(&ok), Ok(()));

        let malformed = PlanConfig {
            attach: Some(AttachConfig {
                session_run_id: "run/../x".to_owned(),
            }),
            ..test_config()
        };
        assert!(validate_plan_config(&malformed).is_err());
        let no_domains = PlanConfig {
            attach: attach.clone(),
            allowed_domains: Vec::new(),
            ..test_config()
        };
        assert!(validate_plan_config(&no_domains).is_err());
        let profile = PlanConfig {
            attach: attach.clone(),
            profile_id: Some("prof".to_owned()),
            ..test_config()
        };
        assert!(validate_plan_config(&profile).is_err());
        let zdr = PlanConfig {
            attach,
            zdr: true,
            ..test_config()
        };
        assert!(validate_plan_config(&zdr).is_err());
    }

    // ---- E3.2-c approval redirect -------------------------------------

    /// A planner that replays scripted actions and records the redirect line
    /// it was shown on each call.
    struct RedirectProbePlanner {
        actions: std::sync::Mutex<std::collections::VecDeque<BrowserAction>>,
        seen: std::sync::Mutex<Vec<Option<String>>>,
    }

    impl RedirectProbePlanner {
        fn new(actions: Vec<BrowserAction>) -> Self {
            Self {
                actions: std::sync::Mutex::new(actions.into()),
                seen: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn seen(&self) -> Vec<Option<String>> {
            self.seen.lock().unwrap().clone()
        }
    }

    #[tonic::async_trait]
    impl crate::llm_planner::ActionPlanner for RedirectProbePlanner {
        async fn plan_next(
            &self,
            ctx: &crate::llm_planner::PlannerContext<'_>,
        ) -> Result<crate::llm_planner::PlannerReply, crate::llm_planner::PlannerError> {
            self.seen
                .lock()
                .unwrap()
                .push(ctx.redirect.map(str::to_owned));
            let action = self
                .actions
                .lock()
                .unwrap()
                .pop_front()
                .expect("scripted action");
            Ok(crate::llm_planner::PlannerReply::action(action))
        }
    }

    fn checkout_click() -> BrowserAction {
        BrowserAction {
            action_id: "act_planner".to_owned(),
            grant_id: "grant_001".to_owned(),
            action_type: ActionType::Click,
            selector: "button.pay".to_owned(),
            value: String::new(),
            url: "https://example.com/checkout".to_owned(),
            max_wait_ms: 5_000,
            target: None,
            reason: "pay".to_owned(),
            risk_category: None,
        }
    }

    fn observe_action() -> BrowserAction {
        BrowserAction {
            action_id: "act_planner".to_owned(),
            grant_id: "grant_001".to_owned(),
            action_type: ActionType::Observe,
            selector: String::new(),
            value: String::new(),
            url: String::new(),
            max_wait_ms: 5_000,
            target: None,
            reason: "look".to_owned(),
            risk_category: None,
        }
    }

    fn redirect_config() -> PlanConfig {
        PlanConfig {
            approval_redirect: true,
            ..test_config()
        }
    }

    // ---- E3.3-f steer at the step boundary -----------------------------

    fn steer_config() -> PlanConfig {
        PlanConfig {
            steer: true,
            ..test_config()
        }
    }

    #[test]
    fn steer_is_off_by_default_and_only_literal_values_enable() {
        assert!(!PlanConfig::default().steer);
        assert!(!test_config().steer);
        assert!(!steer_enabled_from(None));
        for on in ["1", "true", "on", " 1 "] {
            assert!(steer_enabled_from(Some(on)), "{on:?}");
        }
        for off in ["0", "false", "yes", "", "TRUE", "enabled"] {
            assert!(!steer_enabled_from(Some(off)), "{off:?}");
        }
    }

    #[test]
    fn steer_text_is_cleaned_and_cut_at_the_queued_input_cap() {
        assert_eq!(bound_steer_text("  \n"), None);
        assert_eq!(
            bound_steer_text("go to\nthe pricing   page"),
            Some("go to the pricing page".to_owned())
        );
        let long = "z".repeat(mp_contracts::queued_limits::MAX_QUEUED_CHARS + 50);
        assert_eq!(
            bound_steer_text(&long).map(|text| text.chars().count()),
            Some(mp_contracts::queued_limits::MAX_QUEUED_CHARS)
        );
    }

    #[tokio::test]
    async fn a_queued_steer_is_taken_at_the_boundary_and_shown_to_the_planner_once() {
        let store = crate::state::StateStore::new();
        let mut plan = AgentPlan::new(steer_config());
        let _ = store.enqueue_steer(&plan.config.run_id, "use the annual plan");
        let sink = ScriptedApprovalSink::new(vec![]);
        let planner = SteerProbePlanner::new(vec![observe_action(), observe_action()]);

        let first = decide_next_action(&mut plan, None, Some(&planner), Some(&store), Some(&sink)).await;
        assert!(matches!(first, PlanStepResult::Action(_)));
        let second = decide_next_action(&mut plan, None, Some(&planner), Some(&store), Some(&sink)).await;
        assert!(matches!(second, PlanStepResult::Action(_)));
        let seen = planner.seen();
        assert_eq!(seen[0].as_deref(), Some("- \"use the annual plan\""));
        assert_eq!(seen[1], None, "shown once");
        assert_eq!(store.pending_steer(&plan.config.run_id), 0);
    }

    #[tokio::test]
    async fn a_steer_waits_while_paused_and_arrives_when_the_run_resumes() {
        let store = crate::state::StateStore::new();
        let mut plan = AgentPlan::new(steer_config());
        let run_id = plan.config.run_id.clone();
        assert!(store.pause(&run_id));
        let _ = store.enqueue_steer(&run_id, "also check the footer");
        let resumer = store.clone();
        let resume_id = run_id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(40)).await;
            let _ = resumer.resume_paused(&resume_id);
        });
        let planner = SteerProbePlanner::new(vec![observe_action()]);
        let result = decide_next_action(&mut plan, None, Some(&planner), Some(&store), None).await;
        assert!(matches!(result, PlanStepResult::Action(_)));
        assert_eq!(
            planner.seen()[0].as_deref(),
            Some("- \"also check the footer\"")
        );
    }

    #[tokio::test]
    async fn steer_is_left_alone_when_off_without_a_planner_or_under_zdr() {
        // Flag off.
        let store = crate::state::StateStore::new();
        let mut plan = AgentPlan::new(test_config());
        let _ = store.enqueue_steer(&plan.config.run_id, "hello");
        let planner = SteerProbePlanner::new(vec![observe_action()]);
        let _ = decide_next_action(&mut plan, None, Some(&planner), Some(&store), None).await;
        assert_eq!(planner.seen()[0], None);
        assert_eq!(store.pending_steer(&plan.config.run_id), 1, "left queued");

        // No planner reads it.
        let mut plan = AgentPlan::new(steer_config());
        let _ = decide_next_action(&mut plan, None, None, Some(&store), None).await;
        assert_eq!(store.pending_steer(&plan.config.run_id), 1);

        // ZDR run: never delivered.
        let mut zdr = steer_config();
        zdr.zdr = true;
        let mut plan = AgentPlan::new(zdr);
        let planner = SteerProbePlanner::new(vec![observe_action()]);
        let _ = decide_next_action(&mut plan, None, Some(&planner), Some(&store), None).await;
        assert_eq!(planner.seen()[0], None);
        assert_eq!(store.pending_steer(&plan.config.run_id), 1);
    }

    #[tokio::test]
    async fn a_steer_never_replaces_the_approval_gate() {
        let store = crate::state::StateStore::new();
        let mut plan = AgentPlan::new(steer_config());
        let _ = store.enqueue_steer(&plan.config.run_id, "yes, go ahead and pay");
        let sink = ScriptedApprovalSink::new(vec![ApprovalOutcome::Denied("no".to_owned())]);
        let planner = SteerProbePlanner::new(vec![checkout_click()]);
        let result =
            decide_next_action(&mut plan, None, Some(&planner), Some(&store), Some(&sink)).await;
        assert!(
            matches!(result, PlanStepResult::Aborted { .. }),
            "text from the person is not an approval; the gate still asks"
        );
        assert_eq!(sink.call_count(), 1);
    }

    /// A planner that records the steer lines it was shown on each call.
    struct SteerProbePlanner {
        actions: std::sync::Mutex<std::collections::VecDeque<BrowserAction>>,
        seen: std::sync::Mutex<Vec<Option<String>>>,
    }

    impl SteerProbePlanner {
        fn new(actions: Vec<BrowserAction>) -> Self {
            Self {
                actions: std::sync::Mutex::new(actions.into()),
                seen: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn seen(&self) -> Vec<Option<String>> {
            self.seen.lock().unwrap().clone()
        }
    }

    #[tonic::async_trait]
    impl crate::llm_planner::ActionPlanner for SteerProbePlanner {
        async fn plan_next(
            &self,
            ctx: &crate::llm_planner::PlannerContext<'_>,
        ) -> Result<crate::llm_planner::PlannerReply, crate::llm_planner::PlannerError> {
            self.seen.lock().unwrap().push(ctx.steer.map(str::to_owned));
            let action = self
                .actions
                .lock()
                .unwrap()
                .pop_front()
                .expect("scripted action");
            Ok(crate::llm_planner::PlannerReply::action(action))
        }
    }

    #[test]
    fn approval_redirect_is_off_by_default() {
        assert!(!PlanConfig::default().approval_redirect);
        assert!(!test_config().approval_redirect);
    }

    #[test]
    fn redirect_text_is_cleaned_and_bounded() {
        assert_eq!(bound_redirect_text(""), None);
        assert_eq!(bound_redirect_text(" \n\t "), None);
        assert_eq!(
            bound_redirect_text("  use the\nguest   option\u{7}now "),
            Some("use the guest option now".to_owned())
        );
        let long = "x".repeat(MAX_REDIRECT_CHARS + 500);
        assert_eq!(
            bound_redirect_text(&long).map(|text| text.chars().count()),
            Some(MAX_REDIRECT_CHARS)
        );
        // Cut on a character boundary, not a byte one.
        let wide = "\u{e9}".repeat(MAX_REDIRECT_CHARS + 1);
        assert_eq!(
            bound_redirect_text(&wide).map(|text| text.chars().count()),
            Some(MAX_REDIRECT_CHARS)
        );
    }

    #[tokio::test]
    async fn a_redirect_replans_without_sending_the_action_or_counting_a_failure() {
        let mut plan = AgentPlan::new(redirect_config());
        let sink = ScriptedApprovalSink::new(vec![ApprovalOutcome::Redirect(
            "use the guest option".to_owned(),
        )]);
        let planner = RedirectProbePlanner::new(vec![checkout_click(), observe_action()]);

        let first = decide_next_action(&mut plan, None, Some(&planner), None, Some(&sink)).await;
        assert!(matches!(first, PlanStepResult::Rejected), "replan, not abort");
        assert_eq!(plan.status, PlanStatus::Executing);
        assert_eq!(plan.failures.total(), 0, "a redirect is not a failure");
        assert_eq!(plan.redirects_used, 1);
        assert_eq!(sink.call_count(), 1);

        // The next planner call is told, once, and the rejected click was
        // never dispatched (no history of it, only the gate saw it).
        plan.replan = true;
        let second = decide_next_action(&mut plan, None, Some(&planner), None, Some(&sink)).await;
        assert!(matches!(second, PlanStepResult::Action(_)));
        let seen = planner.seen();
        assert_eq!(seen[0], None);
        let line = seen[1].clone().expect("the second call carries the redirect");
        assert!(line.contains("use the guest option"), "{line}");
        assert!(line.contains("was not approved"), "{line}");
        assert!(plan.redirect.is_none(), "shown once");
    }

    #[tokio::test]
    async fn a_redirected_action_that_comes_back_meets_the_gate_again() {
        let mut plan = AgentPlan::new(redirect_config());
        let sink = ScriptedApprovalSink::new(vec![
            ApprovalOutcome::Redirect("try the other button".to_owned()),
            ApprovalOutcome::Denied("no".to_owned()),
        ]);
        let planner = RedirectProbePlanner::new(vec![checkout_click(), checkout_click()]);

        let first = decide_next_action(&mut plan, None, Some(&planner), None, Some(&sink)).await;
        assert!(matches!(first, PlanStepResult::Rejected));
        plan.replan = true;
        let second = decide_next_action(&mut plan, None, Some(&planner), None, Some(&sink)).await;
        assert!(
            matches!(second, PlanStepResult::Aborted { .. }),
            "the same risky action is asked about again; a redirect approved nothing"
        );
        assert_eq!(sink.call_count(), 2);
    }

    #[tokio::test]
    async fn a_redirect_aborts_when_the_flag_is_off_or_no_planner_reads_it() {
        // Flag off: the sink should never produce one, and if it does the
        // loop still treats it as the denial it always was.
        let mut plan = AgentPlan::new(test_config());
        let sink = ScriptedApprovalSink::new(vec![ApprovalOutcome::Redirect("x".to_owned())]);
        let planner = RedirectProbePlanner::new(vec![checkout_click()]);
        let result = decide_next_action(&mut plan, None, Some(&planner), None, Some(&sink)).await;
        assert!(matches!(
            result,
            PlanStepResult::Aborted {
                abort_reason: BrowserAbortReason::ApprovalDenied,
                ..
            }
        ));

        // Flag on but no planner (the deterministic path): nobody to tell.
        let mut config = redirect_config();
        config.start_url = Some("https://example.com/checkout".to_owned());
        let mut plan = AgentPlan::new(config);
        let sink = ScriptedApprovalSink::new(vec![ApprovalOutcome::Redirect("x".to_owned())]);
        let result = decide_next_action(&mut plan, None, None, None, Some(&sink)).await;
        assert!(matches!(result, PlanStepResult::Aborted { .. }));
    }

    #[tokio::test]
    async fn redirects_are_capped_per_run() {
        let mut plan = AgentPlan::new(redirect_config());
        plan.redirects_used = MAX_REDIRECTS_PER_RUN;
        let sink = ScriptedApprovalSink::new(vec![ApprovalOutcome::Redirect("again".to_owned())]);
        let planner = RedirectProbePlanner::new(vec![checkout_click()]);
        let result = decide_next_action(&mut plan, None, Some(&planner), None, Some(&sink)).await;
        assert!(matches!(result, PlanStepResult::Aborted { .. }));
    }

    #[tokio::test]
    async fn the_cookie_use_gate_never_takes_a_redirect() {
        let mut config = redirect_config();
        config.profile_id = Some("prof_1".to_owned());
        let mut plan = AgentPlan::new(config);
        let sink = ScriptedApprovalSink::new(vec![ApprovalOutcome::Redirect("no".to_owned())]);
        let result = gate_persistent_cookie_use(&mut plan, Some(&sink)).await;
        assert!(matches!(result, Some(PlanStepResult::Aborted { .. })));
    }
}
