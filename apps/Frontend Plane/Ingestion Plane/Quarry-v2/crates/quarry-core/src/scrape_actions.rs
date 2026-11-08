//! Scrape-time `actions[]` (WS13.2): the wire shape, the closed action set and
//! the rules that check a request's actions before any browser runs them.
//!
//! Types and validation only. No driver executes an action here (WS13.5).
//! Denied actions are refused by name at validation time, and the refusal is
//! the same whichever spelling the caller uses (the table is in the tests).
//! A `write` or `select` into a credential field is refused heuristically:
//! that is a guard for the common cases, not a guarantee (ADR-2, soft).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::capture::{CaptureLimits, PdfOptions, ScreenshotOptions};
use crate::scrape::FieldError;

/// Action types a request may name. Anything else is `unknown_action_type`
/// (or `action_denied` when it is in [`DENIED_ACTION_TYPES`]).
pub const SUPPORTED_ACTION_TYPES: [&str; 8] = [
    "wait",
    "click",
    "write",
    "press",
    "scroll",
    "select",
    "screenshot",
    "pdf",
];

/// Action types refused by name, in every spelling the table lists. Raw page
/// script, navigation, pointer-coordinate input, file transfer and dialogs
/// stay out of the agent-free contract; each is `action_denied`.
pub const DENIED_ACTION_TYPES: [&str; 10] = [
    "executeJavascript",
    "evaluate",
    "navigate",
    "back",
    "forward",
    "clickPoint",
    "mouseWheel",
    "upload",
    "download",
    "dialog",
];

/// Keys accepted by `press`: a named key from this list, or one printable character.
const NAMED_KEYS: [&str; 14] = [
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

/// Attribute tokens that mark a credential field in a selector. A token is a
/// run of letters and digits between separators, so `passage` does not match
/// `pass` and `#user_pass` does.
const CREDENTIAL_TOKENS: [&str; 8] = [
    "pass", "password", "pwd", "otp", "secret", "token", "cvc", "cvv",
];

/// What to do when an action fails: stop and report, or go on to the next one.
/// There is no retry: a replayed click or type can submit twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ActionOnError {
    #[default]
    Abort,
    Continue,
}

/// `scroll` to the top or bottom of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollTo {
    Top,
    Bottom,
}

/// `scroll` by a number of pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollDirection {
    Up,
    Down,
}

/// One action of the closed set. Unknown keys inside an action are refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum ScrapeAction {
    /// Wait for a time (`milliseconds`) or for a selector (`selector`, up to
    /// `timeout_ms`). Exactly one of the first two; the timeout counts in full
    /// toward the request's wait budget.
    Wait {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        milliseconds: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selector: Option<String>,
        #[serde(default, alias = "timeoutMs", skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u64>,
    },
    /// `selector` is optional on the wire so a missing one is a field error at
    /// `actions[i].selector`, not a decode error for the whole entry.
    Click {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selector: Option<String>,
    },
    /// Type `text` into the element. The text is never echoed in responses.
    Write {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selector: Option<String>,
        text: String,
    },
    Press {
        key: String,
    },
    /// Exactly one of `to` (page top or bottom), `selector` (into view) or
    /// `direction` with `pixels`.
    Scroll {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<ScrollTo>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selector: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direction: Option<ScrollDirection>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pixels: Option<u32>,
    },
    Select {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selector: Option<String>,
        value: String,
    },
    Screenshot(ScreenshotOptions),
    Pdf(PdfOptions),
}

impl ScrapeAction {
    /// The wire name of the action's type.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Wait { .. } => "wait",
            Self::Click { .. } => "click",
            Self::Write { .. } => "write",
            Self::Press { .. } => "press",
            Self::Scroll { .. } => "scroll",
            Self::Select { .. } => "select",
            Self::Screenshot(_) => "screenshot",
            Self::Pdf(_) => "pdf",
        }
    }
}

/// The caps on actions. Environment settings can lower them, never raise them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionLimits {
    pub max_actions: usize,
    pub max_total_wait_ms: u64,
    pub max_single_wait_ms: u64,
    /// A selector wait with no `timeout_ms` waits this long.
    pub default_wait_for_ms: u64,
    pub max_selector_bytes: usize,
    pub max_text_bytes: usize,
    pub max_total_text_bytes: usize,
    pub max_scroll_px: u32,
}

impl Default for ActionLimits {
    fn default() -> Self {
        Self {
            max_actions: 50,
            max_total_wait_ms: 60_000,
            max_single_wait_ms: 30_000,
            default_wait_for_ms: 5_000,
            max_selector_bytes: 512,
            max_text_bytes: 4096,
            max_total_text_bytes: 16_384,
            max_scroll_px: 20_000,
        }
    }
}

impl ActionLimits {
    /// Lower the action count and the total wait to the given values. A value
    /// above the built-in cap is ignored.
    pub fn lowered(max_actions: Option<usize>, max_total_wait_ms: Option<u64>) -> Self {
        let built_in = Self::default();
        Self {
            max_actions: max_actions.map_or(built_in.max_actions, |value| {
                value.min(built_in.max_actions)
            }),
            max_total_wait_ms: max_total_wait_ms.map_or(built_in.max_total_wait_ms, |value| {
                value.min(built_in.max_total_wait_ms)
            }),
            ..built_in
        }
    }
}

/// The actions of a request, checked. Holds the actions themselves; use
/// [`ValidatedActions::summary`] for what a response may echo.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedActions {
    pub actions: Vec<ScrapeAction>,
    pub on_error: ActionOnError,
    /// Sum of the wait budget: milliseconds, or the timeout (default if unset)
    /// of each selector wait, counted in full.
    pub total_wait_ms: u64,
    pub capture_count: usize,
    pub type_counts: BTreeMap<String, usize>,
}

impl ValidatedActions {
    /// What an effective-options echo may carry: counts, types, budget and the
    /// error policy. Never a text or a selector.
    pub fn summary(&self) -> ActionsSummary {
        ActionsSummary {
            count: self.actions.len(),
            types: self.type_counts.clone(),
            total_wait_ms: self.total_wait_ms,
            capture_count: self.capture_count,
            on_error: self.on_error,
        }
    }
}

/// The echo of a request's actions in an effective-options response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub struct ActionsSummary {
    pub count: usize,
    pub types: BTreeMap<String, usize>,
    pub total_wait_ms: u64,
    pub capture_count: usize,
    pub on_error: ActionOnError,
}

/// Outcome of one action, as metadata. Bytes of a capture travel in the page
/// captures, never here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Ok,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionOutput {
    /// `screenshot` or `pdf`.
    pub kind: String,
    pub content_type: String,
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionOutcome {
    pub index: usize,
    pub kind: String,
    pub status: ActionStatus,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ActionError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<ActionOutput>,
}

/// Whether a selector names a credential entry field. Heuristic: lower-cased,
/// spaces and quotes removed for the attribute tests; the other tests split the
/// selector into alphanumeric tokens and look for whole words.
pub fn is_credential_selector(selector: &str) -> bool {
    let lowered = selector.to_ascii_lowercase();
    let compact: String = lowered
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '"' && *c != '\'')
        .collect();
    if compact.contains("type=password") {
        return true;
    }
    if compact.contains("autocomplete")
        && (compact.contains("password") || compact.contains("one-time-code"))
    {
        return true;
    }
    lowered
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|token| CREDENTIAL_TOKENS.contains(&token))
}

fn is_denied(type_name: &str) -> bool {
    DENIED_ACTION_TYPES
        .iter()
        .any(|denied| denied.eq_ignore_ascii_case(type_name))
}

fn is_named_key(key: &str) -> bool {
    if NAMED_KEYS.contains(&key) {
        return true;
    }
    let mut chars = key.chars();
    matches!((chars.next(), chars.next()), (Some(c), None) if !c.is_control())
}

/// Running totals while the actions are checked.
#[derive(Default)]
struct Totals {
    wait_ms: u64,
    text_bytes: usize,
    captures: usize,
}

fn check_selector(selector: &str, path: &str, limits: &ActionLimits, errors: &mut Vec<FieldError>) {
    if selector.trim().is_empty() {
        errors.push(FieldError::new(
            format!("{path}.selector"),
            "invalid_action",
            "a selector is required",
        ));
    } else if selector.len() > limits.max_selector_bytes {
        errors.push(FieldError::new(
            format!("{path}.selector"),
            "selector_too_long",
            format!("a selector is at most {} bytes", limits.max_selector_bytes),
        ));
    }
}

fn check_text(text: &str, path: &str, limits: &ActionLimits, errors: &mut Vec<FieldError>) {
    if text.len() > limits.max_text_bytes {
        errors.push(FieldError::new(
            format!("{path}.text"),
            "text_too_long",
            format!("the text of one write is at most {} bytes", limits.max_text_bytes),
        ));
    }
}

/// Check one action; add its wait, text and capture counts to `totals`.
fn check_action(
    action: &ScrapeAction,
    path: &str,
    limits: &ActionLimits,
    totals: &mut Totals,
    errors: &mut Vec<FieldError>,
) {
    match action {
        ScrapeAction::Wait {
            milliseconds,
            selector,
            timeout_ms,
        } => {
            match (milliseconds, selector) {
                (Some(_), Some(_)) | (None, None) => errors.push(FieldError::new(
                    path,
                    "invalid_action",
                    "a wait takes either milliseconds or a selector",
                )),
                _ => {}
            }
            if timeout_ms.is_some() && selector.is_none() {
                errors.push(FieldError::new(
                    format!("{path}.timeout_ms"),
                    "invalid_action",
                    "timeoutMs applies to a selector wait only",
                ));
            }
            if let Some(selector) = selector {
                check_selector(selector, path, limits, errors);
            }
            let duration = match (milliseconds, timeout_ms) {
                (Some(ms), _) => *ms,
                (None, Some(timeout)) => *timeout,
                (None, None) => limits.default_wait_for_ms,
            };
            if duration > limits.max_single_wait_ms {
                errors.push(FieldError::new(
                    format!("{path}.milliseconds"),
                    "wait_budget_exceeded",
                    format!("one wait is at most {} ms", limits.max_single_wait_ms),
                ));
            }
            totals.wait_ms = totals.wait_ms.saturating_add(duration);
        }
        ScrapeAction::Click { selector } => {
            check_selector(selector.as_deref().unwrap_or(""), path, limits, errors)
        }
        ScrapeAction::Write { selector, text } => {
            let selector = selector.as_deref().unwrap_or("");
            check_selector(selector, path, limits, errors);
            if !selector.trim().is_empty() && is_credential_selector(selector) {
                errors.push(FieldError::new(
                    format!("{path}.selector"),
                    "credential_entry_denied",
                    "typing into a credential field is not allowed",
                ));
            }
            check_text(text, path, limits, errors);
            totals.text_bytes = totals.text_bytes.saturating_add(text.len());
        }
        ScrapeAction::Press { key } => {
            if !is_named_key(key) {
                errors.push(FieldError::new(
                    format!("{path}.key"),
                    "key_not_allowed",
                    "a key is a named key from the allowlist or one printable character",
                ));
            }
        }
        ScrapeAction::Scroll {
            to,
            selector,
            direction,
            pixels,
        } => {
            let chosen = usize::from(to.is_some())
                + usize::from(selector.is_some())
                + usize::from(direction.is_some());
            if chosen != 1 {
                errors.push(FieldError::new(
                    path,
                    "invalid_action",
                    "a scroll takes exactly one of to, selector or direction",
                ));
            }
            match (direction, pixels) {
                (Some(_), None) => errors.push(FieldError::new(
                    format!("{path}.pixels"),
                    "invalid_action",
                    "a directed scroll needs pixels",
                )),
                (Some(_), Some(value)) if *value == 0 || *value > limits.max_scroll_px => {
                    errors.push(FieldError::new(
                        format!("{path}.pixels"),
                        "invalid_action",
                        format!("pixels is 1 to {}", limits.max_scroll_px),
                    ))
                }
                (None, Some(_)) => errors.push(FieldError::new(
                    format!("{path}.pixels"),
                    "invalid_action",
                    "pixels applies to a directed scroll only",
                )),
                _ => {}
            }
            if let Some(selector) = selector {
                check_selector(selector, path, limits, errors);
            }
        }
        ScrapeAction::Select { selector, value } => {
            let selector = selector.as_deref().unwrap_or("");
            check_selector(selector, path, limits, errors);
            if !selector.trim().is_empty() && is_credential_selector(selector) {
                errors.push(FieldError::new(
                    format!("{path}.selector"),
                    "credential_entry_denied",
                    "choosing into a credential field is not allowed",
                ));
            }
            check_text(value, path, limits, errors);
        }
        ScrapeAction::Screenshot(options) => {
            totals.captures += 1;
            if let Err(error) = options.validate(path) {
                errors.push(error);
            }
        }
        ScrapeAction::Pdf(options) => {
            totals.captures += 1;
            if let Err(error) = options.validate(path) {
                errors.push(error);
            }
        }
    }
}

/// Check a request's `actions` in order. All field errors are collected, each
/// with its entry path (`actions[3].selector`). Returns the validated list or
/// the errors. Text and selectors never appear in an error message.
pub fn validate_actions(
    raw: Vec<Value>,
    on_error: ActionOnError,
    limits: &ActionLimits,
    captures: &CaptureLimits,
) -> Result<ValidatedActions, Vec<FieldError>> {
    if raw.len() > limits.max_actions {
        return Err(vec![FieldError::new(
            "actions",
            "too_many_actions",
            format!("at most {} actions per request", limits.max_actions),
        )]);
    }

    let mut errors = Vec::new();
    let mut actions = Vec::with_capacity(raw.len());
    let mut totals = Totals::default();
    let mut type_counts: BTreeMap<String, usize> = BTreeMap::new();

    for (index, value) in raw.into_iter().enumerate() {
        let path = format!("actions[{index}]");
        let Some(object) = value.as_object() else {
            errors.push(FieldError::new(
                &path,
                "invalid_action",
                "an action is an object with a type",
            ));
            continue;
        };
        let Some(type_name) = object.get("type").and_then(Value::as_str) else {
            errors.push(FieldError::new(
                format!("{path}.type"),
                "invalid_action",
                "an action needs a string type",
            ));
            continue;
        };
        if is_denied(type_name) {
            errors.push(FieldError::new(
                format!("{path}.type"),
                "action_denied",
                "this action type is not available in a scrape",
            ));
            continue;
        }
        if !SUPPORTED_ACTION_TYPES.contains(&type_name) {
            errors.push(FieldError::new(
                format!("{path}.type"),
                "unknown_action_type",
                format!(
                    "unknown action type; supported: {}",
                    SUPPORTED_ACTION_TYPES.join(", ")
                ),
            ));
            continue;
        }
        let action: ScrapeAction = match serde_json::from_value(value.clone()) {
            Ok(action) => action,
            Err(_) => {
                errors.push(FieldError::new(
                    &path,
                    "invalid_action",
                    "the action's fields do not match its type",
                ));
                continue;
            }
        };
        let before = errors.len();
        check_action(&action, &path, limits, &mut totals, &mut errors);
        if errors.len() == before {
            *type_counts
                .entry(action.type_name().to_owned())
                .or_insert(0) += 1;
            actions.push(action);
        }
    }

    if totals.wait_ms > limits.max_total_wait_ms {
        errors.push(FieldError::new(
            "actions",
            "wait_budget_exceeded",
            format!(
                "the waits add up to at most {} ms (a selector wait counts its timeout in full)",
                limits.max_total_wait_ms
            ),
        ));
    }
    if totals.text_bytes > limits.max_total_text_bytes {
        errors.push(FieldError::new(
            "actions",
            "text_too_long",
            format!(
                "the text of all writes is at most {} bytes",
                limits.max_total_text_bytes
            ),
        ));
    }
    if totals.captures > captures.max_captures {
        errors.push(FieldError::new(
            "actions",
            "too_many_captures",
            format!("at most {} captures per request", captures.max_captures),
        ));
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(ValidatedActions {
        actions,
        on_error,
        total_wait_ms: totals.wait_ms,
        capture_count: totals.captures,
        type_counts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn check(raw: Vec<Value>) -> Result<ValidatedActions, Vec<FieldError>> {
        validate_actions(
            raw,
            ActionOnError::Abort,
            &ActionLimits::default(),
            &CaptureLimits::default(),
        )
    }

    fn codes(errors: &[FieldError]) -> Vec<(String, String)> {
        errors
            .iter()
            .map(|error| (error.path.clone(), error.code.clone()))
            .collect()
    }

    #[test]
    fn fifty_actions_pass_fifty_one_fail_with_too_many_actions() {
        let fifty: Vec<Value> = (0..50).map(|_| json!({ "type": "press", "key": "Tab" })).collect();
        assert!(check(fifty).is_ok());
        let fifty_one: Vec<Value> =
            (0..51).map(|_| json!({ "type": "press", "key": "Tab" })).collect();
        let errors = check(fifty_one).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "too_many_actions");
        assert_eq!(errors[0].path, "actions");
    }

    #[test]
    fn wait_budget_counts_wait_for_timeouts_in_full() {
        // A selector wait with no timeout waits the default, 5 s, and counts in full.
        let twelve: Vec<Value> = (0..12)
            .map(|_| json!({ "type": "wait", "selector": "#ready" }))
            .collect();
        let ok = check(twelve).expect("12 x 5 s is 60 s, the budget");
        assert_eq!(ok.total_wait_ms, 60_000);
        let thirty_one: Vec<Value> = (0..31)
            .map(|_| json!({ "type": "wait", "selector": "#ready" }))
            .collect();
        let errors = check(thirty_one).unwrap_err();
        assert!(errors.iter().any(|error| error.code == "wait_budget_exceeded"));
        // An explicit timeout is what counts, not the default.
        let timed = check(vec![json!({ "type": "wait", "selector": "#x", "timeoutMs": 2000 })])
            .expect("a 2 s wait");
        assert_eq!(timed.total_wait_ms, 2000);
    }

    #[test]
    fn single_wait_over_30s_is_rejected() {
        let ok = check(vec![json!({ "type": "wait", "milliseconds": 30_000 })]);
        assert!(ok.is_ok());
        let errors = check(vec![json!({ "type": "wait", "milliseconds": 30_001 })]).unwrap_err();
        assert_eq!(errors[0].code, "wait_budget_exceeded");
        assert_eq!(errors[0].path, "actions[0].milliseconds");
        // A wait takes either a time or a selector, not both and not neither.
        let both = check(vec![json!({ "type": "wait", "milliseconds": 5, "selector": "#a" })]);
        assert_eq!(both.unwrap_err()[0].code, "invalid_action");
        let neither = check(vec![json!({ "type": "wait" })]);
        assert_eq!(neither.unwrap_err()[0].code, "invalid_action");
    }

    #[test]
    fn evaluate_navigate_and_back_are_action_denied() {
        // Every denied name, in the spelling the table lists, and in another case.
        let denied = [
            "executeJavascript",
            "evaluate",
            "navigate",
            "back",
            "forward",
            "clickPoint",
            "mouseWheel",
            "upload",
            "download",
            "dialog",
            "EVALUATE",
            "Navigate",
        ];
        for name in denied {
            let errors = check(vec![json!({ "type": name })]).unwrap_err();
            assert_eq!(errors[0].code, "action_denied", "{name}");
            assert_eq!(errors[0].path, "actions[0].type", "{name}");
        }
        assert_eq!(DENIED_ACTION_TYPES.len(), 10);
    }

    #[test]
    fn unknown_type_names_the_supported_set() {
        let errors = check(vec![json!({ "type": "scrape" })]).unwrap_err();
        assert_eq!(errors[0].code, "unknown_action_type");
        for name in SUPPORTED_ACTION_TYPES {
            assert!(errors[0].message.contains(name), "{name}");
        }
        assert_eq!(
            SUPPORTED_ACTION_TYPES.join(", "),
            "wait, click, write, press, scroll, select, screenshot, pdf"
        );
    }

    #[test]
    fn credential_selectors_are_refused() {
        let table = [
            "input[type=password]",
            "input[type=\"password\"]",
            "#pwd",
            "[autocomplete=one-time-code]",
            "input[name*=otp]",
            "#user_pass",
            "input[autocomplete=new-password]",
            "#cvv",
        ];
        for selector in table {
            assert!(is_credential_selector(selector), "{selector}");
            let errors = check(vec![json!({
                "type": "write", "selector": selector, "text": "x"
            })])
            .unwrap_err();
            assert_eq!(errors[0].code, "credential_entry_denied", "{selector}");
            assert_eq!(errors[0].path, "actions[0].selector");
        }
        // Negatives: ordinary fields are not refused.
        for selector in ["input[name=q]", "#search", "passage", "#email"] {
            assert!(!is_credential_selector(selector), "{selector}");
            assert!(check(vec![json!({
                "type": "write", "selector": selector, "text": "x"
            })])
            .is_ok());
        }
    }

    #[test]
    fn key_allowlist_accepts_enter_and_one_char_and_rejects_f12() {
        for key in ["Enter", "Tab", "ArrowDown", "PageUp", "a", "7"] {
            assert!(check(vec![json!({ "type": "press", "key": key })]).is_ok(), "{key}");
        }
        for key in ["F12", "Control", "ab", "", "\u{7}"] {
            let errors = check(vec![json!({ "type": "press", "key": key })]).unwrap_err();
            assert_eq!(errors[0].code, "key_not_allowed", "{key}");
        }
    }

    #[test]
    fn nine_captures_are_too_many() {
        let eight: Vec<Value> = (0..8).map(|_| json!({ "type": "screenshot" })).collect();
        let ok = check(eight).expect("eight captures");
        assert_eq!(ok.capture_count, 8);
        let nine: Vec<Value> = (0..9).map(|_| json!({ "type": "screenshot" })).collect();
        let errors = check(nine).unwrap_err();
        assert!(errors.iter().any(|error| error.code == "too_many_captures"));
    }

    #[test]
    fn entry_paths_in_errors_are_actions_index_field() {
        let errors = check(vec![
            json!({ "type": "press", "key": "Tab" }),
            json!({ "type": "press", "key": "Tab" }),
            json!({ "type": "press", "key": "Tab" }),
            json!({ "type": "write", "text": "hello" }),
        ])
        .unwrap_err();
        assert_eq!(
            codes(&errors),
            vec![("actions[3].selector".to_owned(), "invalid_action".to_owned())]
        );
        let scroll = check(vec![json!({ "type": "scroll", "to": "top", "pixels": 5 })])
            .unwrap_err();
        assert_eq!(scroll[0].path, "actions[0].pixels");
        let pdf = check(vec![json!({ "type": "pdf", "maxPages": 500 })]).unwrap_err();
        assert_eq!(pdf[0].path, "actions[0].maxPages");
        assert_eq!(pdf[0].code, "invalid_max_pages");
    }

    #[test]
    fn selector_and_text_limits_are_enforced() {
        let long = "a".repeat(513);
        let errors = check(vec![json!({ "type": "click", "selector": long })]).unwrap_err();
        assert_eq!(errors[0].code, "selector_too_long");
        let big_text = "b".repeat(4097);
        let errors = check(vec![json!({ "type": "write", "selector": "#q", "text": big_text })])
            .unwrap_err();
        assert_eq!(errors[0].code, "text_too_long");
        // Five writes of 3,400 bytes pass each cap but not the 16 KiB total.
        let chunk = "c".repeat(3400);
        let five: Vec<Value> = (0..5)
            .map(|_| json!({ "type": "write", "selector": "#q", "text": chunk }))
            .collect();
        let errors = check(five).unwrap_err();
        assert!(errors
            .iter()
            .any(|error| error.code == "text_too_long" && error.path == "actions"));
    }

    #[test]
    fn scroll_takes_exactly_one_form() {
        assert!(check(vec![json!({ "type": "scroll", "to": "bottom" })]).is_ok());
        assert!(check(vec![json!({ "type": "scroll", "selector": "#end" })]).is_ok());
        assert!(check(vec![json!({ "type": "scroll", "direction": "down", "pixels": 400 })]).is_ok());
        let none = check(vec![json!({ "type": "scroll" })]).unwrap_err();
        assert_eq!(none[0].code, "invalid_action");
        let directed_no_pixels = check(vec![json!({ "type": "scroll", "direction": "up" })])
            .unwrap_err();
        assert_eq!(directed_no_pixels[0].path, "actions[0].pixels");
        let too_far = check(vec![json!({ "type": "scroll", "direction": "up", "pixels": 20_001 })])
            .unwrap_err();
        assert_eq!(too_far[0].code, "invalid_action");
    }

    #[test]
    fn a_field_the_type_does_not_take_is_refused() {
        let errors = check(vec![json!({ "type": "click", "selector": "#a", "extra": 1 })])
            .unwrap_err();
        assert_eq!(errors[0].code, "invalid_action");
        assert_eq!(errors[0].path, "actions[0]");
    }

    #[test]
    fn summary_carries_counts_and_never_text_or_selectors() {
        let ok = check(vec![
            json!({ "type": "click", "selector": "#secret-button" }),
            json!({ "type": "write", "selector": "#field", "text": "TYPED-VALUE-9" }),
            json!({ "type": "wait", "milliseconds": 250 }),
        ])
        .expect("valid actions");
        let summary = ok.summary();
        assert_eq!(summary.count, 3);
        assert_eq!(summary.types.get("write"), Some(&1));
        assert_eq!(summary.total_wait_ms, 250);
        assert_eq!(summary.on_error, ActionOnError::Abort);
        let text = serde_json::to_string(&summary).expect("summary serializes");
        assert!(!text.contains("TYPED-VALUE-9"));
        assert!(!text.contains("#field"));
        assert!(!text.contains("#secret-button"));
    }

    #[test]
    fn action_limits_lower_but_never_raise() {
        let built_in = ActionLimits::default();
        assert_eq!(built_in.max_actions, 50);
        assert_eq!(built_in.max_total_wait_ms, 60_000);
        assert_eq!(ActionLimits::lowered(Some(3), None).max_actions, 3);
        assert_eq!(ActionLimits::lowered(Some(500), Some(1_000_000)), built_in);
    }

    #[test]
    fn outcomes_serialize_as_metadata() {
        let outcome = ActionOutcome {
            index: 2,
            kind: "screenshot".to_owned(),
            status: ActionStatus::Ok,
            duration_ms: 12,
            error: None,
            output: Some(ActionOutput {
                kind: "screenshot".to_owned(),
                content_type: "image/png".to_owned(),
                bytes: 4,
                width: Some(10),
                height: Some(20),
            }),
        };
        let value = serde_json::to_value(&outcome).expect("serializes");
        assert_eq!(value["status"], "ok");
        assert_eq!(value["output"]["content_type"], "image/png");
        assert!(value.get("error").is_none());
    }
}
