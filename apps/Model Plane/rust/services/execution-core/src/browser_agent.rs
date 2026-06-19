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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanStatus {
    Planning,
    Executing,
    WaitingApproval,
    Completed,
    Failed,
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
}

#[derive(Debug, Clone)]
pub struct AgentPlan {
    pub config: PlanConfig,
    pub current_step: i32,
    pub status: PlanStatus,
    pub observations: Vec<BrowserObservation>,
    started_at: Instant,
}

impl AgentPlan {
    pub fn new(config: PlanConfig) -> Self {
        Self {
            config,
            current_step: 0,
            status: PlanStatus::Planning,
            observations: Vec::new(),
            started_at: Instant::now(),
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
        return PlanStepResult::Action(BrowserAction {
            action_id,
            grant_id: plan.config.grant_id.clone(),
            action_type: ActionType::Observe,
            selector: String::new(),
            value: String::new(),
            url: String::new(),
            max_wait_ms: 5000,
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
pub async fn run_browser_agent_loop(
    config: PlanConfig,
    client: Option<&crate::quarry_agent::QuarryAgentClient>,
    planner: Option<&crate::llm_planner::LlmPlanner>,
    sink: Option<&dyn BrowserEventSink>,
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

    // Acquire a leased browser session for this run.
    let run = match client
        .start_run(&plan.config.org_id, &constraints, zdr, None)
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
        let result = decide_next_action(&mut plan, last.as_ref(), planner).await;
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
            PlanStepResult::Completed(reason) => break reason,
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

/// Decide the next step: run the deterministic gate (`plan_next_action` —
/// terminal/limit/stop-criteria/approval checks + step bookkeeping), then, when
/// it yields an action and an LLM planner is configured, let the model choose
/// the real action from the latest observation. Falls back to the deterministic
/// `Observe` action when the planner is absent or errors.
async fn decide_next_action(
    plan: &mut AgentPlan,
    last_observation: Option<&BrowserObservation>,
    planner: Option<&crate::llm_planner::LlmPlanner>,
) -> PlanStepResult {
    let gate = plan_next_action(plan, last_observation);
    let PlanStepResult::Action(candidate) = gate else {
        return gate;
    };
    let Some(planner) = planner else {
        return PlanStepResult::Action(candidate);
    };
    match planner.next_action(&plan.config, last_observation).await {
        Ok(Some(action)) => PlanStepResult::Action(BrowserAction {
            action_id: candidate.action_id,
            grant_id: candidate.grant_id,
            ..action
        }),
        Ok(None) => {
            plan.status = PlanStatus::Completed;
            PlanStepResult::Completed("agent reported task complete".to_owned())
        }
        Err(e) => {
            warn!(error = %e, "llm planner failed; falling back to deterministic observe");
            PlanStepResult::Action(candidate)
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
}
