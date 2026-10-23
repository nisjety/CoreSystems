//! Tab addressing and per-tab egress for a shared browser session (WS8.E).
//!
//! A session's tabs share one isolated browser context. `OpenTabOptions` says
//! how a new tab opens. The pure helpers here decide which tab an action
//! addresses, which egress policy governs a request and which observations
//! belong to a pinned tab. Keeping them free of CDP makes the rules testable
//! without Chromium. Nothing in this module is reachable by a caller yet: the
//! capability bits and trait defaults are the only surface (dark by design).

use crate::BrowserEgressPolicy;

/// Reason carried in `details.reason` when a pinned tab no longer exists.
/// Such an action fails with NOT_FOUND and never falls back to another tab.
pub const PINNED_TAB_CLOSED_REASON: &str = "pinned_tab_closed";

/// How a new tab opens.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenTabOptions {
    /// Initial navigation. `None` opens a blank tab.
    pub url: Option<String>,
    /// Whether the new tab becomes the session's active tab. `false` opens it
    /// in the background. A session that has no active tab yet activates the
    /// new tab anyway, so the session always has somewhere to act.
    pub activate: bool,
    /// Egress policy for this tab only. `None` leaves the tab on the session
    /// policy.
    pub egress_policy: Option<BrowserEgressPolicy>,
}

/// The tab an action on a session addresses. A pinned handle addresses its
/// pin and never the active tab, even when the pinned tab is gone: the caller
/// must report that rather than fall back.
pub fn addressed_tab_id<'a>(pin: Option<&'a str>, active: Option<&'a str>) -> Option<&'a str> {
    pin.or(active)
}

/// The egress policy for a request made from one tab: the tab's own policy
/// when it has one, otherwise the session policy, otherwise deny-all. A
/// missing policy never means unconstrained.
pub fn resolve_tab_egress_policy(
    tab: Option<&BrowserEgressPolicy>,
    session: Option<&BrowserEgressPolicy>,
) -> BrowserEgressPolicy {
    tab.or(session)
        .cloned()
        .unwrap_or_else(BrowserEgressPolicy::deny_all)
}

/// Whether an observation attributed to `tab_id` is visible through a handle
/// with `pin`. An unpinned handle sees everything. A pinned handle sees only
/// observations of its own tab: tab-less records (for example proxy-level
/// egress receipts) are not its tab's.
pub fn visible_through_pin(pin: Option<&str>, tab_id: Option<&str>) -> bool {
    match pin {
        None => true,
        Some(pin) => tab_id == Some(pin),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(domains: &[&str]) -> BrowserEgressPolicy {
        let domains: Vec<String> = domains.iter().map(|domain| (*domain).to_owned()).collect();
        BrowserEgressPolicy::from_allowed_domains(&domains)
    }

    // Key lookup order: the tab entry first, then the session entry, then deny-all.

    #[test]
    fn tab_policy_wins_over_session_policy() {
        let tab = policy(&["agent.example"]);
        let session = policy(&[]);
        let resolved = resolve_tab_egress_policy(Some(&tab), Some(&session));
        assert!(resolved.allows_url("https://agent.example/page"));
        assert!(!resolved.allows_url("https://third.example/page"));
    }

    #[test]
    fn session_policy_applies_when_the_tab_has_none() {
        let session = policy(&["shop.example"]);
        let resolved = resolve_tab_egress_policy(None, Some(&session));
        assert!(resolved.allows_url("https://shop.example/cart"));
        assert!(!resolved.allows_url("https://other.example/"));
    }

    #[test]
    fn missing_policies_are_deny_all_not_open() {
        let resolved = resolve_tab_egress_policy(None, None);
        assert!(!resolved.allows_url("https://any.example/"));
        // data: and blob: never open a connection, so they stay available.
        assert!(resolved.allows_url("data:text/plain,hello"));
    }

    // Pin resolution: a pin wins over the active tab and never falls back.

    #[test]
    fn pin_wins_over_the_active_tab() {
        assert_eq!(
            addressed_tab_id(Some("tab-2"), Some("tab-3")),
            Some("tab-2")
        );
    }

    #[test]
    fn unpinned_handle_addresses_the_active_tab() {
        assert_eq!(addressed_tab_id(None, Some("tab-3")), Some("tab-3"));
    }

    #[test]
    fn a_closed_pin_is_not_replaced_by_the_active_tab() {
        // The pin is still the answer; the caller finds it missing and reports
        // PINNED_TAB_CLOSED instead of acting on tab-3.
        assert_eq!(
            addressed_tab_id(Some("tab-2"), Some("tab-3")),
            Some("tab-2")
        );
        assert_eq!(PINNED_TAB_CLOSED_REASON, "pinned_tab_closed");
    }

    #[test]
    fn no_pin_and_no_active_tab_addresses_nothing() {
        assert_eq!(addressed_tab_id(None, None), None);
    }

    // Observation filters: a pinned handle sees its own tab only.

    #[test]
    fn unpinned_filter_keeps_every_observation() {
        assert!(visible_through_pin(None, Some("tab-1")));
        assert!(visible_through_pin(None, None));
    }

    #[test]
    fn pinned_filter_keeps_only_the_pinned_tab() {
        assert!(visible_through_pin(Some("tab-2"), Some("tab-2")));
        assert!(!visible_through_pin(Some("tab-2"), Some("tab-3")));
    }

    #[test]
    fn pinned_filter_drops_tabless_observations() {
        assert!(!visible_through_pin(Some("tab-2"), None));
    }

    #[test]
    fn open_tab_options_default_to_a_blank_background_tab() {
        let options = OpenTabOptions::default();
        assert!(options.url.is_none());
        assert!(!options.activate);
        assert!(options.egress_policy.is_none());
    }
}
