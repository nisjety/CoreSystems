//! Admission of the widened browser verbs (WS11.S4).
//!
//! With `QUARRY_BROWSER_AGENT_WIDENED_ACTIONS` on, the planner may propose
//! `press`, `select`, `back` and `wait_for`. Every one of them is a proposal:
//! this module checks it after the planner replies and before the risk gate, and
//! a rejection is a recoverable failure the planner is told about in fixed
//! words. A rejected action never reaches Quarry.
//!
//! The checks only tighten. Quarry still validates the grant, the domains, the
//! refs and the budgets; until it validates `press` keys and `back` destinations
//! itself, the checks here (and the domain fence with its post-action 403) are
//! what stand in front of those two.

use std::collections::VecDeque;

use super::{ActionType, BrowserAction};

/// The only keys `press` may send: no chords (`+`), no printable characters.
pub const KEY_ALLOWLIST: &[&str] = &[
    "Enter",
    "Tab",
    "Escape",
    "Backspace",
    "Delete",
    "Space",
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
    "PageUp",
    "PageDown",
    "Home",
    "End",
];

/// The longest option value `select` may send.
pub const MAX_SELECT_VALUE_BYTES: usize = 256;
/// `wait_for` is clamped into this range (milliseconds).
pub const MIN_WAIT_MS: i32 = 250;
pub const MAX_WAIT_MS: i32 = 10_000;
/// How many observed page URLs `back` can look at.
pub const URL_HISTORY_CAP: usize = 20;

/// `ms` clamped into [`MIN_WAIT_MS`]..=[`MAX_WAIT_MS`].
#[must_use]
pub fn clamp_wait(ms: i32) -> i32 {
    ms.clamp(MIN_WAIT_MS, MAX_WAIT_MS)
}

/// Whether the verb exists only behind the widened-actions flag.
#[must_use]
pub fn is_widened(action_type: &ActionType) -> bool {
    matches!(
        action_type,
        ActionType::Press | ActionType::Select | ActionType::Back | ActionType::WaitFor
    )
}

/// The page URLs the run observed, newest last, bounded. `back` may only go to
/// the page before the current one.
#[derive(Debug, Clone, Default)]
pub struct UrlHistory {
    urls: VecDeque<String>,
}

impl UrlHistory {
    /// Record an observed page URL. The same URL again is no new page; the URL
    /// of the page before the current one means the run went back, so the
    /// current page is dropped. Returns `true` when the URL differs from the
    /// current page.
    pub fn observe(&mut self, url: &str) -> bool {
        let url = url.trim();
        if url.is_empty() {
            return false;
        }
        if self.urls.back().is_some_and(|last| last == url) {
            return false;
        }
        let went_back = self.urls.len() >= 2 && self.urls[self.urls.len() - 2] == url;
        if went_back {
            self.urls.pop_back();
        } else {
            if self.urls.len() == URL_HISTORY_CAP {
                self.urls.pop_front();
            }
            self.urls.push_back(url.to_owned());
        }
        true
    }

    /// The page `back` would return to.
    #[must_use]
    pub fn previous(&self) -> Option<&str> {
        let len = self.urls.len();
        (len >= 2).then(|| self.urls[len - 2].as_str())
    }
}

/// Why a proposed action was not admitted. A closed list; the planner is told
/// the code and the fixed advice, nothing the model or the page wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    KeyNotAllowed,
    TargetRequired,
    ValueRequired,
    ValueTooLong,
    NoPreviousPage,
    PreviousPageNotAllowed,
}

impl Rejection {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::KeyNotAllowed => "key_not_allowed",
            Self::TargetRequired => "target_required",
            Self::ValueRequired => "value_required",
            Self::ValueTooLong => "value_too_long",
            Self::NoPreviousPage => "no_previous_page",
            Self::PreviousPageNotAllowed => "previous_page_not_allowed",
        }
    }

    #[must_use]
    pub const fn advice(self) -> &'static str {
        match self {
            Self::KeyNotAllowed => {
                "press sends one named key only (Enter, Tab, Escape, Backspace, Delete, Space, an arrow key, PageUp, PageDown, Home, End); no chords, no text"
            }
            Self::TargetRequired => {
                "this action needs snapshot_id, generation and ref_id from the latest observation"
            }
            Self::ValueRequired => "select needs the option to choose in value",
            Self::ValueTooLong => "the option value is too long",
            Self::NoPreviousPage => "there is no previous page to go back to",
            Self::PreviousPageNotAllowed => {
                "the previous page is outside the allowed sites; use another approach"
            }
        }
    }
}

/// Check one proposal. `widened` is the flag: with it off, a widened verb is
/// read as an `observe` (what an unknown verb always meant), so the flag's
/// default changes nothing. A `wait_for` is clamped, not rejected.
pub fn admit(
    mut action: BrowserAction,
    widened: bool,
    history: &UrlHistory,
    is_domain_allowed: impl Fn(&str) -> bool,
) -> Result<BrowserAction, Rejection> {
    if !widened && is_widened(&action.action_type) {
        action.action_type = ActionType::Observe;
        action.target = None;
        action.value.clear();
        return Ok(action);
    }
    match action.action_type {
        ActionType::Press => {
            let key = action.value.trim();
            if !KEY_ALLOWLIST.contains(&key) {
                return Err(Rejection::KeyNotAllowed);
            }
            action.value = key.to_owned();
            Ok(action)
        }
        ActionType::Select => {
            if action.target.is_none() {
                return Err(Rejection::TargetRequired);
            }
            if action.value.trim().is_empty() {
                return Err(Rejection::ValueRequired);
            }
            if action.value.len() > MAX_SELECT_VALUE_BYTES {
                return Err(Rejection::ValueTooLong);
            }
            Ok(action)
        }
        ActionType::Back => match history.previous() {
            None => Err(Rejection::NoPreviousPage),
            Some(previous) if is_domain_allowed(previous) => Ok(action),
            Some(_) => Err(Rejection::PreviousPageNotAllowed),
        },
        ActionType::WaitFor => {
            if action.target.is_none() {
                return Err(Rejection::TargetRequired);
            }
            action.max_wait_ms = clamp_wait(action.max_wait_ms);
            Ok(action)
        }
        _ => Ok(action),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser_agent::BrowserTargetRef;

    fn action(action_type: ActionType) -> BrowserAction {
        BrowserAction {
            action_id: String::new(),
            grant_id: String::new(),
            action_type,
            selector: String::new(),
            value: String::new(),
            url: String::new(),
            max_wait_ms: 5000,
            target: None,
            reason: String::new(),
            risk_category: None,
        }
    }

    fn bound(mut action: BrowserAction, frame: Option<&str>) -> BrowserAction {
        action.target = Some(BrowserTargetRef {
            snapshot_id: "snap-1".to_owned(),
            generation: 1,
            ref_id: "@e1".to_owned(),
            frame_id: frame.map(str::to_owned),
        });
        action
    }

    fn press(key: &str) -> BrowserAction {
        BrowserAction {
            value: key.to_owned(),
            ..action(ActionType::Press)
        }
    }

    fn allow_example(url: &str) -> bool {
        url.starts_with("https://example.com/")
    }

    fn admit_on(action: BrowserAction, history: &UrlHistory) -> Result<BrowserAction, Rejection> {
        admit(action, true, history, allow_example)
    }

    #[test]
    fn press_allowlist_accepts_named_keys_only() {
        let history = UrlHistory::default();
        for key in KEY_ALLOWLIST {
            let admitted = admit_on(press(key), &history).unwrap_or_else(|e| panic!("{key}: {e:?}"));
            assert_eq!(admitted.value, *key);
        }
        assert_eq!(KEY_ALLOWLIST.len(), 14);
        // Whitespace around a name is tolerated; a different case is not a key.
        assert_eq!(admit_on(press(" Enter "), &history).unwrap().value, "Enter");
        for key in ["enter", "ENTER", "Return", "F5", "Insert", ""] {
            assert_eq!(admit_on(press(key), &history).unwrap_err(), Rejection::KeyNotAllowed, "{key:?}");
        }
    }

    #[test]
    fn press_rejects_modifier_chords_and_printable_characters() {
        let history = UrlHistory::default();
        for key in [
            "Control+A", "Ctrl+C", "Meta+V", "Alt+F4", "Shift+Tab", "Control+Shift+I", "a", "A", "1",
            " ", "/", "Enter+", "+", "Enter Enter", "Enter\n",
        ] {
            let result = admit_on(press(key), &history);
            // "Enter\n" trims to a valid key; everything else is refused.
            if key == "Enter\n" {
                assert!(result.is_ok());
            } else {
                assert_eq!(result.unwrap_err(), Rejection::KeyNotAllowed, "{key:?}");
            }
        }
    }

    #[test]
    fn select_requires_a_bound_ref_and_never_emits_the_legacy_css_variant() {
        let history = UrlHistory::default();
        let mut css = action(ActionType::Select);
        css.selector = "select#shipping".to_owned();
        css.value = "express".to_owned();
        assert_eq!(admit_on(css, &history).unwrap_err(), Rejection::TargetRequired);

        let mut no_value = bound(action(ActionType::Select), None);
        no_value.value = "  ".to_owned();
        assert_eq!(admit_on(no_value, &history).unwrap_err(), Rejection::ValueRequired);

        let mut long = bound(action(ActionType::Select), None);
        long.value = "x".repeat(MAX_SELECT_VALUE_BYTES + 1);
        assert_eq!(admit_on(long, &history).unwrap_err(), Rejection::ValueTooLong);

        let mut ok = bound(action(ActionType::Select), None);
        ok.value = "x".repeat(MAX_SELECT_VALUE_BYTES);
        assert!(admit_on(ok, &history).is_ok());
    }

    #[test]
    fn back_rejected_without_a_previous_allowed_url() {
        let mut history = UrlHistory::default();
        assert_eq!(
            admit_on(action(ActionType::Back), &history).unwrap_err(),
            Rejection::NoPreviousPage
        );
        history.observe("https://example.com/a");
        assert_eq!(
            admit_on(action(ActionType::Back), &history).unwrap_err(),
            Rejection::NoPreviousPage,
            "one page has nothing before it"
        );
        history.observe("https://example.com/b");
        // Allowed by example.com only; the previous page is elsewhere.
        let mut elsewhere = UrlHistory::default();
        elsewhere.observe("https://other.example/a");
        elsewhere.observe("https://example.com/b");
        assert_eq!(
            admit_on(action(ActionType::Back), &elsewhere).unwrap_err(),
            Rejection::PreviousPageNotAllowed
        );
    }

    #[test]
    fn back_allowed_when_previous_url_is_allowed() {
        let mut history = UrlHistory::default();
        history.observe("https://example.com/a");
        history.observe("https://example.com/b");
        assert_eq!(history.previous(), Some("https://example.com/a"));
        assert!(admit_on(action(ActionType::Back), &history).is_ok());
        // After going back, the page before it is the new previous (none here).
        history.observe("https://example.com/a");
        assert_eq!(history.previous(), None);
    }

    #[test]
    fn url_history_is_bounded_and_ignores_repeats() {
        let mut history = UrlHistory::default();
        assert!(history.observe("https://example.com/0"));
        assert!(!history.observe("https://example.com/0"), "same page again");
        assert!(!history.observe("  "), "blank is no page");
        for n in 1..=(URL_HISTORY_CAP + 5) {
            history.observe(&format!("https://example.com/{n}"));
        }
        assert_eq!(history.urls.len(), URL_HISTORY_CAP);
        assert_eq!(
            history.previous(),
            Some(format!("https://example.com/{}", URL_HISTORY_CAP + 4).as_str())
        );
    }

    #[test]
    fn wait_for_clamps_timeout() {
        let history = UrlHistory::default();
        for (given, expected) in [(0, 250), (-5, 250), (249, 250), (250, 250), (4000, 4000), (10_000, 10_000), (60_000, 10_000)] {
            let mut wait = bound(action(ActionType::WaitFor), None);
            wait.max_wait_ms = given;
            assert_eq!(admit_on(wait, &history).unwrap().max_wait_ms, expected, "{given}");
        }
        assert_eq!(clamp_wait(i32::MAX), MAX_WAIT_MS);
    }

    #[test]
    fn wait_for_requires_a_bound_ref() {
        let history = UrlHistory::default();
        assert_eq!(
            admit_on(action(ActionType::WaitFor), &history).unwrap_err(),
            Rejection::TargetRequired
        );
        assert!(admit_on(bound(action(ActionType::WaitFor), Some("frame-2")), &history).is_ok());
    }

    #[test]
    fn with_the_flag_off_a_widened_verb_is_read_as_observe() {
        let history = UrlHistory::default();
        for verb in [ActionType::Press, ActionType::Select, ActionType::Back, ActionType::WaitFor] {
            assert!(is_widened(&verb));
            let mut proposal = bound(action(verb), None);
            proposal.value = "Control+A".to_owned();
            let admitted = admit(proposal, false, &history, allow_example).expect("never rejected");
            assert_eq!(admitted.action_type, ActionType::Observe);
            assert!(admitted.target.is_none());
            assert!(admitted.value.is_empty());
        }
        // The seven original verbs pass through untouched either way.
        let click = bound(action(ActionType::Click), None);
        assert_eq!(admit(click, false, &history, allow_example).unwrap().action_type, ActionType::Click);
    }

    #[test]
    fn rejection_vocabulary_is_fixed_and_carries_no_model_text() {
        let all = [
            Rejection::KeyNotAllowed,
            Rejection::TargetRequired,
            Rejection::ValueRequired,
            Rejection::ValueTooLong,
            Rejection::NoPreviousPage,
            Rejection::PreviousPageNotAllowed,
        ];
        for rejection in all {
            assert!(rejection.code().bytes().all(|b| b.is_ascii_lowercase() || b == b'_'));
            assert!(!rejection.advice().is_empty());
        }
        let codes: std::collections::BTreeSet<_> = all.iter().map(|r| r.code()).collect();
        assert_eq!(codes.len(), all.len());
    }
}
