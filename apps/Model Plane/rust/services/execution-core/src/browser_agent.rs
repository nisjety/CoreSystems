//! Browser-agent action/observation loop (MP-03).
//!
//! Implements the planner side of the browser-agent protocol defined in
//! `browser_agent.proto`. The loop:
//!   1. Initializes an `AgentPlan` with constraints (`max_steps`, `max_runtime_s`, `allowed_domains`).
//!   2. Calls the planner to produce the next `BrowserAction`.
//!   3. Waits for a `BrowserObservation` from Quarry.
//!   4. Evaluates stop criteria and budget/step limits.
//!   5. Repeats until completion, failure, or abort.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tracing::{info, warn};

mod browser_risk;

pub(crate) use browser_risk::{approval_poll_interval, approval_timeout_seconds};
use browser_risk::{classify_action_risk, risk_reason};
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
    pub error_message: String,
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
}

#[derive(Debug, Clone)]
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
}

#[derive(Debug, Clone)]
pub struct AgentPlan {
    pub config: PlanConfig,
    pub current_step: i32,
    pub status: PlanStatus,
    pub observations: Vec<BrowserObservation>,
    started_at: Instant,
    /// Whether the Phase 5 run-level `PersistentCookieUse` gate has already
    /// run for this plan. Checked once, at the first `decide_next_action`
    /// call (after `start_run` has already succeeded) — never re-asked on
    /// later steps of the same run.
    cookie_use_gate_done: bool,
}

impl AgentPlan {
    pub fn new(config: PlanConfig) -> Self {
        Self {
            config,
            current_step: 0,
            status: PlanStatus::Planning,
            observations: Vec::new(),
            started_at: Instant::now(),
            cookie_use_gate_done: false,
        }
    }

    pub fn check_limits(&self) -> Option<&'static str> {
        if self.config.max_steps > 0 && self.current_step >= self.config.max_steps {
            return Some("max_steps exceeded");
        }
        if self.config.max_runtime_s > 0 {
            let elapsed = i32::try_from(self.started_at.elapsed().as_secs()).unwrap_or(i32::MAX);
            if elapsed >= self.config.max_runtime_s {
                return Some("max_runtime_s exceeded");
            }
        }
        None
    }

    pub fn is_domain_allowed(&self, url: &str) -> bool {
        if self.config.allowed_domains.is_empty() {
            return true;
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

/// Result of a single planning step.
pub enum PlanStepResult {
    Action(BrowserAction),
    Completed(String),
    WaitingApproval,
    Failed(String),
    /// The run was cancelled by the user mid-loop (Phase 2 B5).
    Aborted(String),
}

/// Extract the host portion of a URL without bringing in a full URL parser.
/// Handles `scheme://[user:pass@]host[:port][/path...]` and lowercases the
/// result. Returns `None` for inputs that lack a `://` separator or where
/// the host segment is empty.
fn extract_host(url: &str) -> Option<String> {
    let after_scheme = url.split_once("://")?.1;
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

    if let Some(reason) = plan.check_limits() {
        plan.status = PlanStatus::Completed;
        return PlanStepResult::Completed(reason.to_owned());
    }

    if let Some(obs) = last_observation {
        plan.observations.push(obs.clone());

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
    planner: Option<&crate::llm_planner::LlmPlanner>,
    sink: Option<&dyn BrowserEventSink>,
    state: Option<&crate::state::StateStore>,
) -> (PlanStatus, Vec<BrowserObservation>, String) {
    let mut plan = AgentPlan::new(config);
    info!(plan_id = %plan.config.plan_id, "browser-agent loop started");

    let Some(client) = client else {
        plan.status = PlanStatus::Failed;
        let summary =
            "browser agent unavailable: set QUARRY_BROWSER_AGENT_ENABLED=1 and QUARRY_EDGE_URL"
                .to_owned();
        warn!(plan_id = %plan.config.plan_id, "{summary}");
        return (plan.status, plan.observations, summary);
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

    // Acquire a leased browser session for this run. `profile_id` threads
    // through so an AI run launched from a tab with a persistent profile
    // inherits its cookie state (Phase 2); `None` behaves exactly as before.
    let run = match client
        .start_run(
            &plan.config.org_id,
            &constraints,
            zdr,
            plan.config.profile_id.clone(),
        )
        .await
    {
        Ok(r) => r,
        Err(e) => {
            plan.status = PlanStatus::Failed;
            warn!(plan_id = %plan.config.plan_id, error = %e, "browser-agent start_run failed");
            return (
                plan.status,
                plan.observations,
                format!("browser-agent start_run failed: {e}"),
            );
        }
    };

    let mut pending: Option<BrowserObservation> = None;
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
                match client
                    .step(
                        &run.run_id,
                        &run.lease_id,
                        &plan.config.org_id,
                        wire_action,
                        &constraints,
                        zdr,
                    )
                    .await
                {
                    Ok(wire_obs) => {
                        let obs = crate::quarry_agent::observation_from_wire(
                            &wire_obs,
                            &action.action_id,
                            &plan.config.grant_id,
                        );
                        // Best-effort: surface the observation we just received.
                        if let Some(sink) = sink {
                            sink.observation_received(&plan.config, &obs).await;
                        }
                        pending = Some(obs);
                    }
                    Err(e) => {
                        plan.status = PlanStatus::Failed;
                        break format!("browser-agent step failed: {e}");
                    }
                }
            }
            // `Completed`/`Aborted` share a body today (both simply end the
            // loop with their reason string) but are intentionally distinct
            // variants — `plan.status` already diverged (Completed/Aborted)
            // before reaching here, and callers may branch on it later.
            PlanStepResult::Completed(reason) | PlanStepResult::Aborted(reason) => break reason,
            PlanStepResult::WaitingApproval => break "paused for approval".to_owned(),
            PlanStepResult::Failed(error) => break error,
        }
    };

    // Always release the leased session.
    if let Err(e) = client.close_run(&run.run_id, &plan.config.org_id).await {
        warn!(plan_id = %plan.config.plan_id, error = %e, "browser-agent close_run failed");
    }

    info!(plan_id = %plan.config.plan_id, status = plan.status.as_str(), "browser-agent loop finished");
    let observations = plan.observations.clone();
    (plan.status, observations, summary)
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
/// classified risky (Phase 5, see [`classify_action_risk`]) before returning
/// it for dispatch.
async fn decide_next_action(
    plan: &mut AgentPlan,
    last_observation: Option<&BrowserObservation>,
    planner: Option<&crate::llm_planner::LlmPlanner>,
    state: Option<&crate::state::StateStore>,
    sink: Option<&dyn BrowserEventSink>,
) -> PlanStepResult {
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
    let action = match planner {
        None => candidate,
        Some(planner) => match planner.next_action(&plan.config, last_observation).await {
            Ok(Some(action)) => BrowserAction {
                action_id: candidate.action_id,
                grant_id: candidate.grant_id,
                ..action
            },
            Ok(None) => {
                plan.status = PlanStatus::Completed;
                return PlanStepResult::Completed("agent reported task complete".to_owned());
            }
            Err(e) => {
                warn!(error = %e, "llm planner failed; falling back to deterministic observe");
                candidate
            }
        },
    };

    gate_risky_action(plan, last_observation, action, sink).await
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
        ApprovalOutcome::Denied(reason) => {
            plan.status = PlanStatus::Aborted;
            Some(PlanStepResult::Aborted(format!(
                "persistent cookie use denied: {reason}"
            )))
        }
        ApprovalOutcome::TimedOut => {
            plan.status = PlanStatus::Aborted;
            Some(PlanStepResult::Aborted(
                "persistent cookie use approval timed out".to_owned(),
            ))
        }
    }
}

/// Phase 5 per-action gate: classify `action` (self-report OR deterministic
/// backstop) and, if risky, request approval before it is returned for
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
    let Some(category) = classify_action_risk(&action, last_observation) else {
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
        ApprovalOutcome::Denied(reason) => {
            plan.status = PlanStatus::Aborted;
            PlanStepResult::Aborted(format!("browser action denied: {reason}"))
        }
        ApprovalOutcome::TimedOut => {
            plan.status = PlanStatus::Aborted;
            PlanStepResult::Aborted("browser action approval timed out".to_owned())
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
/// accordingly. Returns `Some(PlanStepResult::Aborted(..))` when the run was
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
                return Some(PlanStepResult::Aborted("cancelled by user".to_owned()));
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
                return None;
            }
        }
    }
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
    fn max_steps_stops_loop() {
        let mut config = test_config();
        config.max_steps = 1;
        let mut plan = AgentPlan::new(config);
        plan.current_step = 1;
        let result = plan_next_action(&mut plan, None);
        match result {
            PlanStepResult::Completed(reason) => {
                assert!(reason.contains("max_steps"));
            }
            _ => panic!("expected Completed"),
        }
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
            error_message: "element not found".to_owned(),
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
            error_message: String::new(),
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
    fn empty_allowlist_permits_all() {
        let mut config = test_config();
        config.allowed_domains = vec![];
        let plan = AgentPlan::new(config);
        assert!(plan.is_domain_allowed("https://anything.com"));
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
            Some(PlanStepResult::Aborted(reason)) => assert_eq!(reason, "cancelled by user"),
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

        assert!(matches!(result, Some(PlanStepResult::Aborted(_))));
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
        assert!(matches!(result, Some(PlanStepResult::Aborted(_))));
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
            PlanStepResult::Aborted(reason) => assert!(reason.contains("denied")),
            _ => panic!("expected Aborted"),
        }
        assert_eq!(plan.status, PlanStatus::Aborted);
    }

    #[tokio::test]
    async fn decide_next_action_aborts_the_run_when_approval_times_out() {
        let mut config = test_config();
        config.start_url = Some("https://example.com/checkout".to_owned());
        let mut plan = AgentPlan::new(config);
        let sink = ScriptedApprovalSink::new(vec![ApprovalOutcome::TimedOut]);

        let result = decide_next_action(&mut plan, None, None, None, Some(&sink)).await;
        match result {
            PlanStepResult::Aborted(reason) => assert!(reason.contains("timed out")),
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
            PlanStepResult::Aborted(reason) => assert!(reason.contains("no approval sink")),
            _ => panic!("expected Aborted (fail closed)"),
        }
        assert_eq!(plan.status, PlanStatus::Aborted);
    }
}
