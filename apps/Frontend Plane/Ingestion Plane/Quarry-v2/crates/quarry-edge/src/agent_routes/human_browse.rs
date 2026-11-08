//! Human-browse semantics (WS8.B, plan D8-1 to D8-5). A `human_browse` grant lets a
//! person drive a browser run on the open web, minus the broker's deny list. It is
//! dark by default (`QUARRY_EDGE__HUMAN_BROWSE`). Every decision here is a pure
//! function so it can be unit-tested without a browser; `lifecycle`, `admission`,
//! `step`, `tabs` and `control` call them at the points the plan names.

use quarry_browser::{BrokerNavPolicy, BrowserEgressPolicy};
use quarry_core::redaction::action::navigate_without_query;
use quarry_runtime::GrantValidation;

use super::StepInitiator;
use crate::run_limits::HumanReceiptMode;

/// The grant kind a person's open-web browsing run carries.
pub(super) const GRANT_KIND_HUMAN_BROWSE: &str = "human_browse";

/// The 403 reasons a `human_browse` start can return. These are wire strings; the
/// openapi document lists them.
pub(super) const REFUSED_DISABLED: &str = "human_browse_disabled";
pub(super) const REFUSED_SERVICE: &str = "human_browse_service_refused";
pub(super) const REFUSED_OWNER: &str = "human_browse_owner_mismatch";
pub(super) const REFUSED_ZDR: &str = "human_browse_zdr_posture";
pub(super) const REFUSED_POLICY: &str = "human_browse_policy_unknown";

/// The navigation authority of one run. `Allowlist` is the run's own exact host
/// list (`constraints.allowed_domains`); `OpenWeb` is any public http(s) host the
/// broker's denied list does not name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunNavPolicy {
    Allowlist,
    OpenWeb { denied: Vec<String> },
}

impl RunNavPolicy {
    /// The wire name stored on a checkpoint.
    pub fn mode_name(&self) -> &'static str {
        match self {
            Self::Allowlist => "allowlist",
            Self::OpenWeb { .. } => "open_web",
        }
    }

    pub fn is_open_web(&self) -> bool {
        matches!(self, Self::OpenWeb { .. })
    }

    /// Whether the run may navigate to `url`. An allow-list run uses its own host
    /// list; an open-web run uses the denied list and the always-local refusal.
    pub fn allows(&self, url: &str, allowed_domains: &[String]) -> bool {
        match self {
            Self::Allowlist => {
                BrowserEgressPolicy::from_allowed_domains(allowed_domains).allows_url(url)
            }
            Self::OpenWeb { denied } => BrowserEgressPolicy::open_web(denied).allows_url(url),
        }
    }
}

/// Gate a start that presents a `human_browse` grant. Checked in this order, each
/// refusal a 403 with its own reason: the switch; a person and not a service; the
/// grant's owner; the run is at least as protected as the grant; and the broker's
/// policy is open web with a list that parses. `run_zdr_active` is the run's posture
/// after the caller's claims are applied.
pub(super) fn admit_start(
    enabled: bool,
    caller_is_service: bool,
    caller_actor_id: &str,
    grant: &GrantValidation,
    run_zdr_active: bool,
) -> Result<RunNavPolicy, &'static str> {
    if !enabled {
        return Err(REFUSED_DISABLED);
    }
    if caller_is_service {
        return Err(REFUSED_SERVICE);
    }
    let owner = grant.owner_id.trim();
    if owner.is_empty() || owner != caller_actor_id {
        return Err(REFUSED_OWNER);
    }
    if grant.zdr && !run_zdr_active {
        return Err(REFUSED_ZDR);
    }
    match grant.nav_policy() {
        Ok(BrokerNavPolicy::OpenWeb { denied }) => Ok(RunNavPolicy::OpenWeb { denied }),
        // A human-browse grant never lends an allow-list: a person's run is open
        // web or nothing. An unknown or allow-list mode fails closed.
        Ok(BrokerNavPolicy::Allowlist { .. }) | Err(_) => Err(REFUSED_POLICY),
    }
}

/// Whether a stored checkpoint's policy mode matches the policy the run now holds.
/// A checkpoint with no mode is an allow-list run; an unknown stored mode never
/// matches, so it fails closed.
pub(super) fn resume_policy_matches(stored: Option<&str>, run: &RunNavPolicy) -> bool {
    match stored.unwrap_or("allowlist") {
        "allowlist" => !run.is_open_web(),
        "open_web" => run.is_open_web(),
        _ => false,
    }
}

/// Whether the broker's policy for a grant is the same authority the run holds:
/// an allow-list with the same exact host list, or open web with the same denied
/// list. Any other pairing is a mismatch.
pub(super) fn broker_matches_run(
    broker: &BrokerNavPolicy,
    run: &RunNavPolicy,
    run_domains: &[String],
) -> bool {
    match (broker, run) {
        (BrokerNavPolicy::Allowlist { domains }, RunNavPolicy::Allowlist) => domains == run_domains,
        (BrokerNavPolicy::OpenWeb { denied }, RunNavPolicy::OpenWeb { denied: run_denied }) => {
            denied == run_denied
        }
        _ => false,
    }
}

/// The egress boundary an open-web run is configured with. `web_egress` is what
/// the web policy resolves to for the run. When that resolution is "unchanged"
/// (an unrestricted policy), the open-web policy takes its place; a narrowing or
/// a deny-all from the web policy is kept as it is.
pub(super) fn open_web_egress(
    run: &RunNavPolicy,
    web_egress: BrowserEgressPolicy,
) -> BrowserEgressPolicy {
    match run {
        RunNavPolicy::OpenWeb { denied } if web_egress.is_unconstrained() => {
            // The web policy's own deny list (WS7.G) rides along with the grant's.
            BrowserEgressPolicy::open_web(denied).with_denied_domains(web_egress.denied_domains())
        }
        _ => web_egress,
    }
}

/// Whether a step counts against the run's step budget. The budget subject is the run's
/// class (plan R-8): a step on an agent-class run counts whoever drives it, so a person's
/// token holding a `run` grant, or a person's step on a run started with the `agent` hint,
/// cannot step past the agent's bounds. On any other run a person's own step is exempt (a
/// person's session is never capped) and an agent's or the system's step counts. A person's
/// step while the person holds the browser (`takeover`) is always exempt: the plan keeps the
/// initiator for takeover and receipts only, and the runtime clock is paused for it (WS6.B,
/// WS8.F), so taking over a run that hit its cap neither is refused nor spends what is left.
pub(super) fn counts_against_step_budget(
    initiator: StepInitiator,
    takeover: bool,
    budget: Option<&crate::budget::RunBudget>,
) -> bool {
    if initiator == StepInitiator::Human && takeover {
        return false;
    }
    initiator != StepInitiator::Human
        || budget.is_some_and(|budget| budget.class == crate::budget::RunClass::Agent)
}

/// Whether the takeover rule (7d78ab2e8) is what spared this step from the budget: a
/// person's step during a takeover on a run whose class is the agent's, which without the
/// takeover would count (R-8). The takeover counter reads this, so the metric measures
/// exactly the use the exemption lets through unmetered (TAKEOVER-BUDGET, X-12 shadow review).
pub(super) fn takeover_exempted_step(
    initiator: StepInitiator,
    takeover: bool,
    budget: Option<&crate::budget::RunBudget>,
) -> bool {
    takeover
        && counts_against_step_budget(initiator, false, budget)
        && !counts_against_step_budget(initiator, true, budget)
}

/// What a step writes to the receipt store (WS8.B). A ZDR run writes nothing, as
/// before. A person's step on a non-ZDR run is kept as the configured mode says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReceiptPlan {
    /// The full receipt, observation included (today's storage).
    Full,
    /// Metadata only: the action redacted, no observation.
    Metadata,
    /// No receipt.
    Skip,
}

pub(super) fn receipt_plan(
    initiator: StepInitiator,
    zdr_active: bool,
    mode: HumanReceiptMode,
) -> ReceiptPlan {
    if zdr_active {
        return ReceiptPlan::Skip;
    }
    if initiator != StepInitiator::Human {
        return ReceiptPlan::Full;
    }
    match mode {
        HumanReceiptMode::Metadata => ReceiptPlan::Metadata,
        HumanReceiptMode::Full => ReceiptPlan::Full,
        HumanReceiptMode::None => ReceiptPlan::Skip,
    }
}

/// The URL of a tab that has no page yet: a run's first tab is committed on it before its
/// first navigation (SC4 T4b), and stays on it when that navigation never commits.
pub(super) const BLANK_TAB_URL: &str = "about:blank";

/// Whether `url` is a tab with no page yet (T4b-1). Exactly `about:blank`: it loads
/// nothing, so a step that lands on it reached no host. It is an observation only, never a
/// navigation target: the edge's `RunNavPolicy::allows` still refuses it as one (the
/// browser's `guard_navigation_target` admits it, as it loads nothing). `about:srcdoc` (a frame's inline document, never a tab's URL) and every other
/// `about:` URL are not this case.
pub(super) fn is_blank_tab(url: &str) -> bool {
    url == BLANK_TAB_URL
}

/// The URL a checkpoint keeps. An open-web run keeps only scheme, host and path (a
/// query can carry a session token); an allow-list run keeps its URL as it was. A tab with
/// no page yet keeps `""`, as a run that never navigated does: there is nothing to recover
/// on a resume, and `about:blank` is never a recovery navigation (T4b-1).
pub(super) fn checkpoint_url(run: &RunNavPolicy, current_url: &str) -> String {
    if is_blank_tab(current_url) {
        return String::new();
    }
    match run {
        RunNavPolicy::OpenWeb { .. } => navigate_without_query(current_url),
        RunNavPolicy::Allowlist => current_url.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_runtime::GrantValidation;

    fn grant(json: serde_json::Value) -> GrantValidation {
        serde_json::from_value(json).expect("grant json")
    }

    fn human_grant(owner: &str, policy_mode: &str, denied: &[&str]) -> GrantValidation {
        grant(serde_json::json!({
            "grant_id": "grant_x",
            "active": true,
            "kind": "human_browse",
            "owner_id": owner,
            "policy_mode": policy_mode,
            "denied_domains": denied,
        }))
    }

    #[test]
    fn human_browse_refused_when_flag_off() {
        let g = human_grant("user_a", "open_web", &["bank.example"]);
        assert_eq!(admit_start(false, false, "user_a", &g, false), Err(REFUSED_DISABLED));
    }

    #[test]
    fn human_browse_service_principal_is_refused() {
        let g = human_grant("user_a", "open_web", &[]);
        assert_eq!(admit_start(true, true, "user_a", &g, false), Err(REFUSED_SERVICE));
    }

    #[test]
    fn human_browse_owner_mismatch_is_refused() {
        let g = human_grant("user_a", "open_web", &[]);
        assert_eq!(admit_start(true, false, "user_b", &g, false), Err(REFUSED_OWNER));
        let no_owner = human_grant("", "open_web", &[]);
        assert_eq!(admit_start(true, false, "", &no_owner, false), Err(REFUSED_OWNER));
    }

    #[test]
    fn human_browse_zdr_grant_needs_a_zdr_run() {
        let mut g = human_grant("user_a", "open_web", &[]);
        g.zdr = true;
        assert_eq!(admit_start(true, false, "user_a", &g, false), Err(REFUSED_ZDR));
        assert!(admit_start(true, false, "user_a", &g, true).is_ok());
    }

    #[test]
    fn human_browse_grant_sets_open_web_policy() {
        let g = human_grant("user_a", "open_web", &["bank.example"]);
        let run = admit_start(true, false, "user_a", &g, false).expect("admitted");
        assert_eq!(
            run,
            RunNavPolicy::OpenWeb {
                denied: vec!["bank.example".to_owned()]
            }
        );
        assert_eq!(run.mode_name(), "open_web");
    }

    #[test]
    fn human_browse_old_policy_mode_unknown_fails_closed() {
        let unknown = human_grant("user_a", "v0_future_mode", &[]);
        assert_eq!(
            admit_start(true, false, "user_a", &unknown, false),
            Err(REFUSED_POLICY)
        );
        // An allow-list mode on a human grant is refused too.
        let allow = human_grant("user_a", "allowlist", &[]);
        assert_eq!(
            admit_start(true, false, "user_a", &allow, false),
            Err(REFUSED_POLICY)
        );
    }

    fn run_budget(class: crate::budget::RunClass) -> crate::budget::RunBudget {
        crate::budget::RunBudget::start(
            &crate::budget::BudgetLimits::default(),
            class,
            &quarry_core::contracts::AgentConstraints {
                max_steps: 5,
                allowed_domains: Vec::new(),
                max_runtime_s: None,
                max_cost_usd: None,
            },
            None,
            std::time::Instant::now(),
        )
    }

    #[test]
    fn human_steps_do_not_consume_max_steps() {
        assert!(!counts_against_step_budget(StepInitiator::Human, false, None));
        let interactive = run_budget(crate::budget::RunClass::Interactive);
        assert!(!counts_against_step_budget(StepInitiator::Human, false, Some(&interactive)));
    }

    #[test]
    fn agent_steps_still_consume_max_steps() {
        assert!(counts_against_step_budget(StepInitiator::Agent, false, None));
        assert!(counts_against_step_budget(StepInitiator::System, false, None));
        let interactive = run_budget(crate::budget::RunClass::Interactive);
        assert!(counts_against_step_budget(StepInitiator::Agent, false, Some(&interactive)));
    }

    #[test]
    fn a_persons_step_on_an_agent_class_run_counts_because_the_run_class_is_the_budget_subject() {
        // R-8: the subject is the run class, not the step's initiator.
        let agent = run_budget(crate::budget::RunClass::Agent);
        assert!(counts_against_step_budget(StepInitiator::Human, false, Some(&agent)));
        assert!(counts_against_step_budget(StepInitiator::Agent, false, Some(&agent)));
    }

    #[test]
    fn a_persons_step_during_a_takeover_never_counts_even_on_an_agent_class_run() {
        // The plan keeps the initiator for takeover and receipts only (R-8, WS8.F).
        let agent = run_budget(crate::budget::RunClass::Agent);
        assert!(!counts_against_step_budget(StepInitiator::Human, true, Some(&agent)));
        assert!(!counts_against_step_budget(StepInitiator::Human, true, None));
    }

    #[test]
    fn the_takeover_counter_reads_only_a_step_the_takeover_rule_spared() {
        let agent = run_budget(crate::budget::RunClass::Agent);
        let interactive = run_budget(crate::budget::RunClass::Interactive);
        // A person's takeover step on an agent-class run: spared by the takeover rule.
        assert!(takeover_exempted_step(StepInitiator::Human, true, Some(&agent)));
        // The same person's step without the takeover counts (R-8): nothing to meter.
        assert!(!takeover_exempted_step(StepInitiator::Human, false, Some(&agent)));
        // On a person's own run (or with budgets off) a person's step is exempt anyway:
        // the takeover did not change the outcome, so it is not metered as takeover use.
        assert!(!takeover_exempted_step(StepInitiator::Human, true, Some(&interactive)));
        assert!(!takeover_exempted_step(StepInitiator::Human, true, None));
        // An agent's step is never spared by a takeover.
        assert!(!takeover_exempted_step(StepInitiator::Agent, true, Some(&agent)));
    }

    #[test]
    fn human_receipt_has_no_observation_and_redacted_text() {
        assert_eq!(
            receipt_plan(StepInitiator::Human, false, HumanReceiptMode::Metadata),
            ReceiptPlan::Metadata
        );
        assert_eq!(
            receipt_plan(StepInitiator::Human, false, HumanReceiptMode::None),
            ReceiptPlan::Skip
        );
        // An agent's step keeps the full receipt whatever the human mode is.
        assert_eq!(
            receipt_plan(StepInitiator::Agent, false, HumanReceiptMode::Metadata),
            ReceiptPlan::Full
        );
    }

    #[test]
    fn zdr_human_session_persists_nothing() {
        for mode in [HumanReceiptMode::Metadata, HumanReceiptMode::Full, HumanReceiptMode::None] {
            assert_eq!(receipt_plan(StepInitiator::Human, true, mode), ReceiptPlan::Skip);
        }
    }

    #[test]
    fn checkpoint_url_strips_query_for_open_web() {
        let open = RunNavPolicy::OpenWeb { denied: Vec::new() };
        assert_eq!(
            checkpoint_url(&open, "https://example.com/a?token=abc#x"),
            "https://example.com/a"
        );
        assert_eq!(
            checkpoint_url(&RunNavPolicy::Allowlist, "https://example.com/a?q=1"),
            "https://example.com/a?q=1"
        );
    }

    #[test]
    fn a_blank_tab_is_checkpointed_as_no_page_and_only_the_exact_url_counts() {
        let open = RunNavPolicy::OpenWeb { denied: Vec::new() };
        assert_eq!(checkpoint_url(&RunNavPolicy::Allowlist, "about:blank"), "");
        assert_eq!(checkpoint_url(&open, "about:blank"), "");
        assert!(is_blank_tab("about:blank"));
        for url in [
            "about:srcdoc",
            "about:blank#x",
            "about:blank?x=1",
            "ABOUT:BLANK",
            "about:config",
            "data:text/html,x",
            "javascript:void(0)",
            "",
        ] {
            assert!(!is_blank_tab(url), "{url} is not a tab with no page yet");
        }
        // It never becomes a navigation target: the run's host rule still refuses it.
        let domains = vec!["example.test".to_owned()];
        assert!(!RunNavPolicy::Allowlist.allows("about:blank", &domains));
        assert!(!open.allows("about:blank", &domains));
    }

    #[test]
    fn resume_requires_matching_policy_mode() {
        let open = RunNavPolicy::OpenWeb { denied: Vec::new() };
        assert!(resume_policy_matches(Some("open_web"), &open));
        assert!(!resume_policy_matches(Some("allowlist"), &open));
        assert!(resume_policy_matches(None, &RunNavPolicy::Allowlist));
        assert!(!resume_policy_matches(None, &open));
        // An unknown stored mode never resumes.
        assert!(!resume_policy_matches(Some("future"), &open));
        assert!(!resume_policy_matches(Some("future"), &RunNavPolicy::Allowlist));
    }

    #[test]
    fn validate_run_grant_open_web_requires_equal_denied_list() {
        let broker = BrokerNavPolicy::OpenWeb {
            denied: vec!["a.example".to_owned()],
        };
        let same = RunNavPolicy::OpenWeb {
            denied: vec!["a.example".to_owned()],
        };
        let wider = RunNavPolicy::OpenWeb {
            denied: vec!["a.example".to_owned(), "b.example".to_owned()],
        };
        assert!(broker_matches_run(&broker, &same, &[]));
        assert!(!broker_matches_run(&broker, &wider, &[]));
        assert!(!broker_matches_run(&broker, &RunNavPolicy::Allowlist, &[]));
    }

    #[test]
    fn human_browse_open_web_egress_replaces_unrestricted_only() {
        let open = RunNavPolicy::OpenWeb {
            denied: vec!["bank.example".to_owned()],
        };
        let unchanged = BrowserEgressPolicy::from_allowed_domains(&[]);
        let egress = open_web_egress(&open, unchanged);
        assert!(!egress.allows_url("https://bank.example/"));
        assert!(egress.allows_url("https://example.com/"));
        // The web policy's deny list is added to the grant's (WS7.G).
        let with_policy_deny = BrowserEgressPolicy::from_allowed_domains(&[])
            .with_denied_domains(&["ads.example".to_owned()]);
        let merged = open_web_egress(&open, with_policy_deny);
        assert!(!merged.allows_url("https://ads.example/"));
        assert!(!merged.allows_url("https://bank.example/"));
        assert!(merged.allows_url("https://example.com/"));
        // A narrowing from the web policy is kept.
        let narrowed = BrowserEgressPolicy::from_allowed_domains(&["only.example".to_owned()]);
        let kept = open_web_egress(&open, narrowed);
        assert!(!kept.allows_url("https://example.com/"));
        // D2-020: a deny-all from the web policy is kept too, never replaced by the open web.
        let denied_all = open_web_egress(&open, BrowserEgressPolicy::deny_all());
        assert!(!denied_all.allows_url("https://example.com/"));
        assert!(!denied_all.allows_url("https://only.example/"));
    }
}
