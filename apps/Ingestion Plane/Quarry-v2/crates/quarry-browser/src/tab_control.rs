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
    /// policy. A tab policy only narrows: the tab's requests answer to the
    /// session policy and this one both (see [`TabEgressPolicy`]).
    pub egress_policy: Option<BrowserEgressPolicy>,
}

/// The tab an action on a session addresses. A pinned handle addresses its
/// pin and never the active tab, even when the pinned tab is gone: the caller
/// must report that rather than fall back.
pub fn addressed_tab_id<'a>(pin: Option<&'a str>, active: Option<&'a str>) -> Option<&'a str> {
    pin.or(active)
}

/// The egress policy that governs a request made from one tab: the meet of the
/// session policy and the tab's own policy (D2-013). A tab can narrow what the
/// session allows, never widen it: a request is allowed only when every policy
/// that applies allows it. Evaluating both at the request, instead of building one
/// merged policy, keeps the meet exact across the allow-list and open-web modes and
/// their deny lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabEgressPolicy {
    session: BrowserEgressPolicy,
    tab: Option<BrowserEgressPolicy>,
}

impl TabEgressPolicy {
    /// Whether a request from the tab may reach `raw`.
    pub fn allows_url(&self, raw: &str) -> bool {
        self.session.allows_url(raw)
            && self.tab.as_ref().map_or(true, |tab| tab.allows_url(raw))
    }
}

/// The egress policy for a request made from one tab: the session policy met with
/// the tab's own policy when it has one. A session with no policy is deny-all, so a
/// tab policy alone is never authority, and a missing policy never means
/// unconstrained.
pub fn resolve_tab_egress_policy(
    tab: Option<&BrowserEgressPolicy>,
    session: Option<&BrowserEgressPolicy>,
) -> TabEgressPolicy {
    TabEgressPolicy {
        session: session
            .cloned()
            .unwrap_or_else(BrowserEgressPolicy::deny_all),
        tab: tab.cloned(),
    }
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

    // The effective policy is the meet of the session policy and the tab policy.

    #[test]
    fn a_tab_policy_narrows_an_unrestricted_session() {
        let tab = policy(&["agent.example"]);
        let session = policy(&[]);
        let resolved = resolve_tab_egress_policy(Some(&tab), Some(&session));
        assert!(resolved.allows_url("https://agent.example/page"));
        assert!(!resolved.allows_url("https://third.example/page"));
    }

    #[test]
    fn a_tab_policy_wider_than_the_session_cannot_widen_it() {
        // D2-013: the tab names a host the session does not allow, and an
        // unrestricted tab sits under a restricted session.
        let session = policy(&["shop.example"]);
        let wider = policy(&["shop.example", "other.example"]);
        let resolved = resolve_tab_egress_policy(Some(&wider), Some(&session));
        assert!(resolved.allows_url("https://shop.example/cart"));
        assert!(!resolved.allows_url("https://other.example/"));
        let unrestricted_tab = policy(&[]);
        let resolved = resolve_tab_egress_policy(Some(&unrestricted_tab), Some(&session));
        assert!(resolved.allows_url("https://shop.example/cart"));
        assert!(!resolved.allows_url("https://other.example/"));
    }

    #[test]
    fn the_effective_policy_is_the_intersection_of_two_allow_lists() {
        let session = policy(&["a.example", "b.example"]);
        let tab = policy(&["b.example", "c.example"]);
        let resolved = resolve_tab_egress_policy(Some(&tab), Some(&session));
        assert!(resolved.allows_url("https://b.example/"));
        assert!(resolved.allows_url("https://www.b.example/"));
        assert!(!resolved.allows_url("https://a.example/"));
        assert!(!resolved.allows_url("https://c.example/"));
        // Disjoint lists allow nothing, instead of falling back to either side.
        let disjoint = policy(&["z.example"]);
        let resolved = resolve_tab_egress_policy(Some(&disjoint), Some(&session));
        assert!(!resolved.allows_url("https://a.example/"));
        assert!(!resolved.allows_url("https://z.example/"));
    }

    #[test]
    fn the_session_deny_list_and_open_web_floor_hold_on_a_wider_tab() {
        let open_session = BrowserEgressPolicy::open_web(&["bank.example".to_owned()]);
        // An unrestricted tab policy does not lift the session's denied list or
        // the open-web refusal of private hosts.
        let tab = policy(&[]);
        let resolved = resolve_tab_egress_policy(Some(&tab), Some(&open_session));
        assert!(resolved.allows_url("https://news.example/"));
        assert!(!resolved.allows_url("https://bank.example/"));
        assert!(!resolved.allows_url("http://127.0.0.1/"));
        // And the tab's own deny list narrows an unrestricted session.
        let denying_tab = policy(&[]).with_denied_domains(&["ads.example".to_owned()]);
        let resolved = resolve_tab_egress_policy(Some(&denying_tab), Some(&policy(&[])));
        assert!(!resolved.allows_url("https://ads.example/"));
        assert!(resolved.allows_url("https://news.example/"));
    }

    #[test]
    fn a_tab_policy_without_a_session_policy_is_not_authority() {
        let tab = policy(&["agent.example"]);
        let resolved = resolve_tab_egress_policy(Some(&tab), None);
        assert!(!resolved.allows_url("https://agent.example/"));
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
