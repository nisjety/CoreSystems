//! Browser-agent action/observation loop (MP-03).
//!
//! Implements the planner side of the browser-agent protocol defined in
//! `browser_agent.proto`. The loop:
//!   1. Initializes an `AgentPlan` with constraints (max_steps, max_runtime_s, allowed_domains).
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
            let elapsed = self.started_at.elapsed().as_secs() as i32;
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
        .find(|c: char| matches!(c, '/' | '?' | '#' | ':'))
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

/// Execute the full browser-agent loop synchronously (for integration with execution-core tool bridge).
pub fn run_browser_agent_loop(config: PlanConfig) -> (PlanStatus, Vec<BrowserObservation>, String) {
    let mut plan = AgentPlan::new(config);

    info!(plan_id = %plan.config.plan_id, "browser-agent loop started");

    let summary = loop {
        let last_obs = plan.observations.last().cloned();
        let result = plan_next_action(&mut plan, last_obs.as_ref());

        match result {
            PlanStepResult::Action(action) => {
                info!(
                    plan_id = %plan.config.plan_id,
                    step = plan.current_step,
                    action_type = action.action_type.as_str(),
                    "dispatching browser action"
                );
                // In production this would send the action to Quarry via gRPC/NATS
                // and wait for the observation response. For now we simulate a timeout
                // after dispatching since Quarry is not wired yet.
                plan.status = PlanStatus::Completed;
                break format!(
                    "browser-agent plan {} dispatched {} steps; awaiting Quarry wiring",
                    plan.config.plan_id, plan.current_step
                );
            }
            PlanStepResult::Completed(reason) => {
                info!(plan_id = %plan.config.plan_id, reason = %reason, "browser-agent loop completed");
                break reason;
            }
            PlanStepResult::WaitingApproval => {
                info!(plan_id = %plan.config.plan_id, "browser-agent paused for approval");
                break "paused for approval".to_owned();
            }
            PlanStepResult::Failed(error) => {
                warn!(plan_id = %plan.config.plan_id, error = %error, "browser-agent loop failed");
                break error;
            }
        }
    };

    let observations = plan.observations.clone();
    (plan.status, observations, summary)
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
        let mut lock = self.plans.lock().expect("mutex not poisoned");
        lock.insert(plan.config.plan_id.clone(), plan.clone());
        plan
    }

    pub fn get(&self, plan_id: &str) -> Option<AgentPlan> {
        let lock = self.plans.lock().expect("mutex not poisoned");
        lock.get(plan_id).cloned()
    }

    pub fn update(&self, plan: AgentPlan) {
        let mut lock = self.plans.lock().expect("mutex not poisoned");
        lock.insert(plan.config.plan_id.clone(), plan);
    }

    pub fn abort(&self, plan_id: &str) -> bool {
        let mut lock = self.plans.lock().expect("mutex not poisoned");
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
