//! Phase 5 — browser-action risk classification.
//!
//! Pure, unit-testable logic split out of `browser_agent.rs` to keep that
//! module's size in check: the types and the deterministic classifier here
//! have no dependency on `AgentPlan`/loop state — they only read
//! `BrowserAction`/`BrowserObservation`. The loop-integration glue that DOES
//! need `AgentPlan` (`gate_risky_action`, `gate_persistent_cookie_use`,
//! `request_approval`) stays in `browser_agent.rs` alongside `decide_next_action`.

use super::{extract_host, ActionType, BrowserAction, BrowserObservation};

/// Category of elevated-risk browser action or run-level condition requiring
/// human approval before it proceeds (Phase 5 — HITL gates, plan capability
/// #8: "login, checkout, posting forms, destructive actions, downloads,
/// uploads, cross-domain navigation, persistent cookie use").
///
/// Deliberately does NOT cover `downloads`/`uploads`: no `ActionType`
/// variant (or Quarry-side manual-browsing capability) represents a file
/// transfer today — plan target capability #10 ("File upload/download
/// handling") is not yet built anywhere in the stack. Classifying risk for
/// an action that cannot occur would be a fabricated classifier, not a real
/// gate; this is an explicit, honest scope cut for this stage, not an
/// oversight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskCategory {
    /// The action interacts with a login/authentication form.
    Login,
    /// The action interacts with a checkout/payment flow.
    Checkout,
    /// The action submits/posts a form (comment, message, publish, …).
    PostingForm,
    /// The action looks destructive/irreversible (delete, remove, cancel,
    /// unsubscribe, deactivate, …).
    Destructive,
    /// A navigation whose target host differs from the last observed page's
    /// host. Distinct from — and additive to — the unconditional
    /// `allowed_domains` allow-list enforced at dispatch: a domain can be on
    /// the allow-list and still represent a meaningful hop worth a human's
    /// attention (e.g. moving from a shopping site to a third-party payment
    /// processor it redirects to).
    CrossDomainNavigation,
    /// The run reuses a persistent, cookie-bearing Quarry browser profile
    /// (not ZDR, not ephemeral). Gated once per run rather than per action.
    PersistentCookieUse,
}

impl RiskCategory {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Checkout => "checkout",
            Self::PostingForm => "posting_form",
            Self::Destructive => "destructive",
            Self::CrossDomainNavigation => "cross_domain_navigation",
            Self::PersistentCookieUse => "persistent_cookie_use",
        }
    }

    /// Parse a planner- or caller-supplied risk-category string (Phase 5's
    /// `ACTION_SCHEMA.risk_category`). Unknown, empty, or `"none"` values are
    /// not an error — they just mean "no self-reported risk"; the
    /// deterministic backstop (`classify_action_risk`) still runs regardless.
    #[must_use]
    pub fn from_wire(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "login" => Some(Self::Login),
            "checkout" => Some(Self::Checkout),
            "posting_form" | "post" | "form_submit" | "posting form" => Some(Self::PostingForm),
            "destructive" => Some(Self::Destructive),
            "cross_domain" | "cross_domain_navigation" | "cross-domain" => {
                Some(Self::CrossDomainNavigation)
            }
            "persistent_cookie" | "persistent_cookie_use" | "persistent cookie" => {
                Some(Self::PersistentCookieUse)
            }
            _ => None,
        }
    }
}

/// Detail describing WHICH browser action (or run-level condition) triggered
/// the Phase 5 HITL approval gate, and WHY — carried onto the
/// `BrowserActionApprovalRequired`/`BrowserActionDecided` orchestration
/// events so the run timeline shows a specific reason instead of a generic
/// "risky tool" label (contrast the outer, whole-tool `is_risky_tool` gate).
#[derive(Debug, Clone)]
pub struct RiskyActionDetail {
    /// Empty for a run-level gate (e.g. `PersistentCookieUse`) not tied to
    /// one specific dispatched action.
    pub action_id: String,
    /// Action type slug, or a synthetic slug (e.g. `"start_run"`) for a
    /// run-level gate.
    pub action_type: String,
    pub url: String,
    pub selector: String,
    pub risk_category: RiskCategory,
    pub reason: String,
}

/// Outcome of the Phase 5 in-loop HITL approval gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalOutcome {
    /// A human granted the approval — proceed with the gated action/run.
    Granted,
    /// A human denied it, or the gate could not be reached at all (fail
    /// closed — see `BrowserEventSink::require_approval`'s doc).
    Denied(String),
    /// No decision arrived within the wait budget.
    TimedOut,
}

// Keyword/URL-path backstop marker lists for categories with no structural
// signal in `BrowserAction` (mirrors `permission::is_risky_tool`'s
// keyword-list pattern). Checked in a fixed, most-specific-first order (see
// `classify_action_risk` below) so an action touching more than one marker
// set (e.g. a checkout page's own login gate) still gets a single, sensible
// classification.
const CHECKOUT_MARKERS: &[&str] = &[
    "/checkout",
    "/cart",
    "/payment",
    "/billing",
    "place-order",
    "placeorder",
    "buy-now",
    "buynow",
    "pay-now",
    "paynow",
];
const LOGIN_MARKERS: &[&str] = &["/login", "/signin", "/sign-in", "password", "passwd", "pwd"];
const DESTRUCTIVE_MARKERS: &[&str] = &[
    "delete",
    "remove",
    "unsubscribe",
    "cancel-order",
    "cancelorder",
    "deactivate",
    "close-account",
    "closeaccount",
];
const POSTING_MARKERS: &[&str] = &["submit", "post-comment", "publish", "send-message"];

/// Deterministic risk classification with the planner's self-report as an
/// additive fallback. Concrete browser evidence wins whenever both are
/// present, so a planner cannot relabel a checkout or destructive action to
/// make the human approval prompt less informative. Never classifies
/// `downloads`/`uploads` — see [`RiskCategory`]'s doc for why.
pub(super) fn classify_action_risk(
    action: &BrowserAction,
    last_observation: Option<&BrowserObservation>,
) -> Option<RiskCategory> {
    let deterministic = classify_deterministic_risk(action, last_observation);
    deterministic.or(action.risk_category)
}

fn classify_deterministic_risk(
    action: &BrowserAction,
    last_observation: Option<&BrowserObservation>,
) -> Option<RiskCategory> {
    // Cross-domain navigation: compare the target host of a `Goto` against
    // the host of the last page actually observed — NOT the allow-list
    // (`AgentPlan::is_domain_allowed`), which is a separate, unconditional
    // dispatch-time check that stays unchanged. A page can legitimately
    // redirect across allow-listed domains (e.g. a shop to its payment
    // processor); that hop is still worth a human's attention. The very
    // first navigation of a run (no prior observation yet) is never flagged
    // here — there is no "prior domain" to have moved away from.
    if action.action_type == ActionType::Goto {
        if let (Some(target_host), Some(prior_host)) = (
            extract_host(&action.url),
            last_observation.and_then(|o| extract_host(&o.page_url)),
        ) {
            if target_host != prior_host {
                return Some(RiskCategory::CrossDomainNavigation);
            }
        }
    }

    let haystack = format!(
        "{} {} {}",
        action.url.to_ascii_lowercase(),
        action.selector.to_ascii_lowercase(),
        action.value.to_ascii_lowercase(),
    );

    if CHECKOUT_MARKERS.iter().any(|m| haystack.contains(m)) {
        return Some(RiskCategory::Checkout);
    }
    if LOGIN_MARKERS.iter().any(|m| haystack.contains(m)) {
        return Some(RiskCategory::Login);
    }
    if DESTRUCTIVE_MARKERS.iter().any(|m| haystack.contains(m)) {
        return Some(RiskCategory::Destructive);
    }
    if matches!(action.action_type, ActionType::Click)
        && POSTING_MARKERS.iter().any(|m| haystack.contains(m))
    {
        return Some(RiskCategory::PostingForm);
    }

    None
}

/// Human-readable "why" for a per-action gate, combining the classification
/// with the model's own rationale (`action.reason`) when present.
pub(super) fn risk_reason(category: RiskCategory, action: &BrowserAction) -> String {
    let base = match category {
        RiskCategory::CrossDomainNavigation => {
            format!("navigation to a new domain: {}", action.url)
        }
        RiskCategory::Login => {
            "action appears to interact with a login/authentication form".to_owned()
        }
        RiskCategory::Checkout => {
            "action appears to interact with a checkout/payment flow".to_owned()
        }
        RiskCategory::PostingForm => "action appears to submit/post a form".to_owned(),
        RiskCategory::Destructive => {
            "action appears destructive (delete/remove/cancel/unsubscribe/deactivate)".to_owned()
        }
        RiskCategory::PersistentCookieUse => {
            "run reuses a persistent cookie-bearing profile".to_owned()
        }
    };
    if action.reason.is_empty() {
        base
    } else {
        format!("{base} (model: {})", action.reason)
    }
}

/// Polling interval while waiting on a Phase 5 HITL approval decision.
/// Shortened under test, mirrors `super::pause_poll_interval`.
pub(crate) fn approval_poll_interval() -> std::time::Duration {
    if cfg!(test) {
        std::time::Duration::from_millis(5)
    } else {
        std::time::Duration::from_millis(500)
    }
}

/// How long (seconds) to wait for a human decision before a risky browser
/// action's approval gate gives up and treats it as timed out (fail closed —
/// never `Granted` by default). Overridable via
/// `QUARRY_BROWSER_AGENT_APPROVAL_TIMEOUT_S` for ops; short under test so the
/// unit suite never blocks on real wall-clock time.
pub(crate) fn approval_timeout_seconds() -> u32 {
    if cfg!(test) {
        return 1;
    }
    std::env::var("QUARRY_BROWSER_AGENT_APPROVAL_TIMEOUT_S")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(1800)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser_agent::ObservationStatus;

    fn action(action_type: ActionType, url: &str, selector: &str, value: &str) -> BrowserAction {
        BrowserAction {
            action_id: "act_1".to_owned(),
            grant_id: "grant_1".to_owned(),
            action_type,
            selector: selector.to_owned(),
            value: value.to_owned(),
            url: url.to_owned(),
            max_wait_ms: 5000,
            reason: String::new(),
            risk_category: None,
        }
    }

    fn observation_at(url: &str) -> BrowserObservation {
        BrowserObservation {
            observation_id: "obs_1".to_owned(),
            action_id: "act_0".to_owned(),
            grant_id: "grant_1".to_owned(),
            status: ObservationStatus::Success,
            page_url: url.to_owned(),
            page_title: String::new(),
            extracted_text: String::new(),
            screenshot_ref: String::new(),
            dom_snapshot_ref: String::new(),
            error_message: String::new(),
        }
    }

    #[test]
    fn classify_planner_self_report_covers_a_risk_without_deterministic_evidence() {
        let mut a = action(
            ActionType::Click,
            "https://example.com/help",
            "button.info",
            "",
        );
        a.risk_category = Some(RiskCategory::Destructive);
        assert_eq!(
            classify_action_risk(&a, None),
            Some(RiskCategory::Destructive)
        );
    }

    #[test]
    fn classify_deterministic_checkout_is_not_masked_by_a_planner_login_label() {
        let mut a = action(
            ActionType::Click,
            "https://shop.example.com/checkout",
            "button.continue",
            "",
        );
        a.risk_category = Some(RiskCategory::Login);

        assert_eq!(classify_action_risk(&a, None), Some(RiskCategory::Checkout));
    }

    #[test]
    fn classify_detects_checkout_by_url() {
        let a = action(
            ActionType::Goto,
            "https://shop.example.com/checkout",
            "",
            "",
        );
        assert_eq!(classify_action_risk(&a, None), Some(RiskCategory::Checkout));
    }

    #[test]
    fn classify_detects_login_by_selector() {
        let a = action(
            ActionType::Type,
            "https://example.com/account",
            "input[type=password]",
            "hunter2",
        );
        assert_eq!(classify_action_risk(&a, None), Some(RiskCategory::Login));
    }

    #[test]
    fn classify_detects_destructive_by_selector() {
        let a = action(
            ActionType::Click,
            "https://example.com/settings",
            "button.delete-account",
            "",
        );
        assert_eq!(
            classify_action_risk(&a, None),
            Some(RiskCategory::Destructive)
        );
    }

    #[test]
    fn classify_detects_posting_form_on_click_only() {
        let click = action(
            ActionType::Click,
            "https://example.com/comments",
            "button[type=submit]",
            "",
        );
        assert_eq!(
            classify_action_risk(&click, None),
            Some(RiskCategory::PostingForm)
        );

        // Same marker on a non-click action type doesn't count (no submit
        // affordance actually being invoked).
        let scroll = action(
            ActionType::Scroll,
            "https://example.com/comments",
            "button[type=submit]",
            "",
        );
        assert_eq!(classify_action_risk(&scroll, None), None);
    }

    #[test]
    fn classify_detects_cross_domain_navigation_relative_to_last_observation() {
        let goto = action(
            ActionType::Goto,
            "https://alt-domain.example/landing",
            "",
            "",
        );
        let last = observation_at("https://shop.example.com/cart");
        assert_eq!(
            classify_action_risk(&goto, Some(&last)),
            Some(RiskCategory::CrossDomainNavigation)
        );
    }

    #[test]
    fn classify_does_not_flag_same_domain_navigation() {
        let goto = action(ActionType::Goto, "https://shop.example.com/about", "", "");
        let last = observation_at("https://shop.example.com/cart");
        assert_eq!(classify_action_risk(&goto, Some(&last)), None);
    }

    #[test]
    fn classify_does_not_flag_the_first_navigation_of_a_run() {
        // No prior observation yet — nothing to have moved "away" from.
        let goto = action(ActionType::Goto, "https://example.com/", "", "");
        assert_eq!(classify_action_risk(&goto, None), None);
    }

    #[test]
    fn classify_returns_none_for_a_benign_action() {
        let observe = action(ActionType::Observe, "", "", "");
        assert_eq!(classify_action_risk(&observe, None), None);
    }
}
