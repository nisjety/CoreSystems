//! Recoverable-failure bookkeeping for the LLM-planned browser loop (WS11.S3).
//!
//! A step that fails in a way the planner can work around (a stale target, an
//! unsupported action, one slow action) does not end the run. It is recorded as
//! an [`ActionFailure`] in a small ring the planner reads, counted against a
//! [`FailureBudget`], and a [`NoProgressDetector`] ends a run that keeps doing
//! the same thing to the same page. Nothing here carries page text or Quarry's
//! error message: a failure is a closed `kind` plus the action that caused it.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::{BrowserAction, BrowserObservation};

/// How many recent failures the planner is shown.
pub const FAILURE_RING_CAP: usize = 10;
/// Consecutive failed steps that end a run (`..._MAX_CONSECUTIVE_FAILURES`).
pub const DEFAULT_MAX_CONSECUTIVE_FAILURES: u32 = 3;
/// Failed steps in total that end a run (`..._MAX_TOTAL_FAILURES`).
pub const DEFAULT_MAX_TOTAL_FAILURES: u32 = 8;
/// Identical (action, page) pairs in a row that end a run.
pub const NO_PROGRESS_LIMIT: u32 = 3;

/// Why an LLM-planned run stopped on its own judgement. The loop's summary
/// starts with [`BrowserFailureKind::prefix`], so a caller (and a dashboard)
/// can tell the kinds apart without parsing prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserFailureKind {
    /// The planner could not answer (credential, route, unreadable twice).
    PlannerUnavailable,
    /// Too many failed actions in a row or in total.
    RepeatedFailure,
    /// The same action kept hitting the same page.
    NoProgress,
    /// A bot wall or refusal stops the task (the planner's `blocked`, or a
    /// `done` over a strong challenge).
    Blocked,
    /// The planner has no useful move left (its `incomplete`).
    Incomplete,
}

impl BrowserFailureKind {
    /// The stable label (metrics, logs).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PlannerUnavailable => "planner_unavailable",
            Self::RepeatedFailure => "repeated_failure",
            Self::NoProgress => "no_progress",
            Self::Blocked => "blocked",
            Self::Incomplete => "incomplete",
        }
    }

    /// The stable start of the run's failure text.
    #[must_use]
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::PlannerUnavailable => "browser planner unavailable",
            Self::RepeatedFailure => "repeated_failure",
            Self::NoProgress => "no_progress",
            Self::Blocked => "blocked",
            Self::Incomplete => "incomplete",
        }
    }

    /// `<prefix>: <detail>`; the planner-unavailable text keeps its older shape
    /// (`browser planner unavailable (<kind>)`), so its detail joins with a space.
    #[must_use]
    pub fn reason(self, detail: &str) -> String {
        match self {
            Self::PlannerUnavailable => format!("{} {detail}", self.prefix()),
            _ => format!("{}: {detail}", self.prefix()),
        }
    }
}

/// One failed action, as the planner reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionFailure {
    /// The loop step the action was planned at.
    pub step: i32,
    /// `goto`, `click`, ... (closed vocabulary).
    pub action_type: &'static str,
    /// Closed failure kind, e.g. `stale_target`, `action_timeout`.
    pub kind: &'static str,
    /// What the planner should do about it, fixed text per kind.
    pub advice: &'static str,
}

impl ActionFailure {
    /// `step N <action> failed: <kind>. <advice>`
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "step {} {} failed: {}. {}",
            self.step, self.action_type, self.kind, self.advice
        )
    }
}

/// The counters' verdict after a failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureVerdict {
    Continue,
    /// The run must stop; the text is a stable, content-free reason.
    Stop(String),
}

/// The ring of recent failures plus the consecutive and total counters.
#[derive(Debug, Clone)]
pub struct FailureBudget {
    ring: Vec<ActionFailure>,
    consecutive: u32,
    total: u32,
    max_consecutive: u32,
    max_total: u32,
}

impl Default for FailureBudget {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

impl FailureBudget {
    /// A zero limit takes the default (3 consecutive, 8 total).
    #[must_use]
    pub fn new(max_consecutive: u32, max_total: u32) -> Self {
        Self {
            ring: Vec::new(),
            consecutive: 0,
            total: 0,
            max_consecutive: if max_consecutive == 0 {
                DEFAULT_MAX_CONSECUTIVE_FAILURES
            } else {
                max_consecutive
            },
            max_total: if max_total == 0 {
                DEFAULT_MAX_TOTAL_FAILURES
            } else {
                max_total
            },
        }
    }

    /// Record a failed step.
    pub fn record(&mut self, failure: ActionFailure) -> FailureVerdict {
        if self.ring.len() == FAILURE_RING_CAP {
            self.ring.remove(0);
        }
        self.ring.push(failure);
        self.consecutive += 1;
        self.total += 1;
        if self.consecutive >= self.max_consecutive {
            return FailureVerdict::Stop(BrowserFailureKind::RepeatedFailure.reason(&format!(
                "stopped after {} consecutive failed actions",
                self.consecutive
            )));
        }
        if self.total >= self.max_total {
            return FailureVerdict::Stop(BrowserFailureKind::RepeatedFailure.reason(&format!(
                "stopped after {} failed actions in total",
                self.total
            )));
        }
        FailureVerdict::Continue
    }

    /// A step that worked resets the consecutive counter (the total stays).
    pub fn record_success(&mut self) {
        self.consecutive = 0;
    }

    /// The recent failures, oldest first.
    #[must_use]
    pub fn recent(&self) -> &[ActionFailure] {
        &self.ring
    }

    #[must_use]
    pub const fn total(&self) -> u32 {
        self.total
    }
}

/// Ends a run that repeats the same action against an unchanged page.
#[derive(Debug, Clone, Default)]
pub struct NoProgressDetector {
    last: Option<u64>,
    repeats: u32,
}

impl NoProgressDetector {
    /// Feed the action the planner chose and the page it saw. Returns `true`
    /// when the same pair has now come [`NO_PROGRESS_LIMIT`] times in a row.
    pub fn observe(&mut self, action: &BrowserAction, page: Option<&BrowserObservation>) -> bool {
        let fingerprint = fingerprint(action, page);
        if self.last == Some(fingerprint) {
            self.repeats += 1;
        } else {
            self.last = Some(fingerprint);
            self.repeats = 1;
        }
        self.repeats >= NO_PROGRESS_LIMIT
    }
}

/// A hash of what the action does and what the page looked like. Never logged
/// or sent anywhere; it only has to tell "same" from "different".
fn fingerprint(action: &BrowserAction, page: Option<&BrowserObservation>) -> u64 {
    let mut hasher = DefaultHasher::new();
    action.action_type.as_str().hash(&mut hasher);
    action.selector.hash(&mut hasher);
    action.value.hash(&mut hasher);
    action.url.hash(&mut hasher);
    if let Some(target) = &action.target {
        target.ref_id.hash(&mut hasher);
        target.snapshot_id.hash(&mut hasher);
    }
    if let Some(page) = page {
        page.page_url.hash(&mut hasher);
        page.extracted_text.hash(&mut hasher);
        page.snapshot_targets.len().hash(&mut hasher);
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_kind_prefixes_are_stable() {
        let all = [
            (BrowserFailureKind::PlannerUnavailable, "planner_unavailable", "browser planner unavailable (parse)"),
            (BrowserFailureKind::RepeatedFailure, "repeated_failure", "repeated_failure: x"),
            (BrowserFailureKind::NoProgress, "no_progress", "no_progress: x"),
            (BrowserFailureKind::Blocked, "blocked", "blocked: x"),
            (BrowserFailureKind::Incomplete, "incomplete", "incomplete: x"),
        ];
        for (kind, label, text) in all {
            assert_eq!(kind.as_str(), label);
            let detail = if kind == BrowserFailureKind::PlannerUnavailable { "(parse)" } else { "x" };
            assert_eq!(kind.reason(detail), text);
        }
    }
    use crate::browser_agent::{ActionType, ObservationStatus};

    fn failure(step: i32) -> ActionFailure {
        ActionFailure {
            step,
            action_type: "click",
            kind: "stale_target",
            advice: "the page changed; pick a target from the fresh observation",
        }
    }

    fn action(action_type: ActionType, url: &str) -> BrowserAction {
        BrowserAction {
            action_id: String::new(),
            grant_id: String::new(),
            action_type,
            selector: String::new(),
            value: String::new(),
            url: url.to_owned(),
            max_wait_ms: 5000,
            target: None,
            reason: String::new(),
            risk_category: None,
        }
    }

    fn page(url: &str, text: &str) -> BrowserObservation {
        BrowserObservation {
            observation_id: "o".to_owned(),
            action_id: "a".to_owned(),
            grant_id: "g".to_owned(),
            status: ObservationStatus::Success,
            page_url: url.to_owned(),
            page_title: String::new(),
            extracted_text: text.to_owned(),
            screenshot_ref: String::new(),
            dom_snapshot_ref: String::new(),
            snapshot_generation: None,
            snapshot_targets: Vec::new(),
            error_message: String::new(),
            challenge: None,
            redaction_rule_version: None,
        }
    }

    #[test]
    fn failure_budget_stops_on_the_third_consecutive_failure() {
        let mut budget = FailureBudget::default();
        assert_eq!(budget.record(failure(1)), FailureVerdict::Continue);
        assert_eq!(budget.record(failure(2)), FailureVerdict::Continue);
        assert_eq!(
            budget.record(failure(3)),
            FailureVerdict::Stop(
                "repeated_failure: stopped after 3 consecutive failed actions".to_owned()
            )
        );
    }

    #[test]
    fn failure_budget_success_resets_consecutive_but_total_still_stops() {
        let mut budget = FailureBudget::default();
        for step in 0..7 {
            assert_eq!(budget.record(failure(step)), FailureVerdict::Continue, "{step}");
            budget.record_success();
        }
        assert_eq!(
            budget.record(failure(8)),
            FailureVerdict::Stop(
                "repeated_failure: stopped after 8 failed actions in total".to_owned()
            )
        );
        assert_eq!(budget.total(), 8);
    }

    #[test]
    fn failure_budget_limits_come_from_the_config_and_zero_means_default() {
        let mut one = FailureBudget::new(1, 0);
        assert!(matches!(one.record(failure(1)), FailureVerdict::Stop(_)));
        let mut wide = FailureBudget::new(5, 20);
        for step in 0..4 {
            assert_eq!(wide.record(failure(step)), FailureVerdict::Continue);
        }
    }

    #[test]
    fn failure_ring_keeps_the_ten_most_recent() {
        let mut budget = FailureBudget::new(100, 100);
        for step in 1..=12 {
            budget.record(failure(step));
        }
        let steps: Vec<i32> = budget.recent().iter().map(|f| f.step).collect();
        assert_eq!(steps, (3..=12).collect::<Vec<_>>());
    }

    #[test]
    fn failure_line_is_fixed_text_without_page_content() {
        assert_eq!(
            failure(4).line(),
            "step 4 click failed: stale_target. the page changed; pick a target from the fresh observation"
        );
    }

    #[test]
    fn no_progress_detector_stops_on_the_third_identical_pair() {
        let mut detector = NoProgressDetector::default();
        let click = action(ActionType::Click, "");
        let same = page("https://example.com/", "unchanged");
        assert!(!detector.observe(&click, Some(&same)));
        assert!(!detector.observe(&click, Some(&same)));
        assert!(detector.observe(&click, Some(&same)));
    }

    #[test]
    fn no_progress_detector_resets_when_the_page_or_the_action_changes() {
        let mut detector = NoProgressDetector::default();
        let click = action(ActionType::Click, "");
        assert!(!detector.observe(&click, Some(&page("https://example.com/", "a"))));
        assert!(!detector.observe(&click, Some(&page("https://example.com/", "a"))));
        // The page moved on: not stuck.
        assert!(!detector.observe(&click, Some(&page("https://example.com/", "b"))));
        assert!(!detector.observe(&click, Some(&page("https://example.com/", "b"))));
        // A different action on the same page: not stuck either.
        let goto = action(ActionType::Goto, "https://example.com/x");
        assert!(!detector.observe(&goto, Some(&page("https://example.com/", "b"))));
        assert!(!detector.observe(&goto, Some(&page("https://example.com/", "b"))));
        // No page at all counts as a page.
        assert!(!detector.observe(&goto, None));
    }
}
