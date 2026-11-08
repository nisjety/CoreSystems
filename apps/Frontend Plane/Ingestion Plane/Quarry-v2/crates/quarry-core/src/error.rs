//! Canonical error envelope. Mirrors CONTRACTS §2 error path.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub type QuarryResult<T> = Result<T, QuarryError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    BadRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    RateLimited,
    Timeout,
    SecurityBlocked,
    /// The browser API returned but Quarry could not verify the requested
    /// business postcondition. Callers must not treat this as success.
    ActionUnknown,
    /// A challenge/access state requires a policy or human escalation.
    ChallengeDetected,
    /// The process is alive but its required durable/runtime dependencies are
    /// not compatible with the requested operation.
    RuntimeNotReady,
    /// A resumable run checkpoint is stale, missing, or no longer valid.
    CheckpointLost,
    /// A selector/target repair is required before an action can continue.
    TargetRepairRequired,
    DriverFailed,
    UpstreamBlocked,
    Unsupported,
    /// The fetched body is a kind of content Quarry has no extractor for
    /// (an image, an archive, a spreadsheet). A client condition, not a server
    /// fault: never retried. Details carry the detected kind.
    UnsupportedMedia,
    /// An agent-class run reached a step, attempt, runtime or cost bound.
    /// Details name the dimension. Not retryable: the budget does not refill
    /// by waiting, and a resumed run keeps what it spent.
    BudgetExhausted,
    /// The person using the browser has taken it over, so an action proposed by
    /// an agent is refused until control is given back. A state, not a fault:
    /// never retried by the caller. Details carry the control mode and epoch.
    UserTookOver,
    Internal,
}

impl ErrorCode {
    pub fn retryable(self) -> bool {
        matches!(
            self,
            Self::Timeout | Self::RateLimited | Self::UpstreamBlocked | Self::Internal
        )
    }

    pub fn http_status(self) -> u16 {
        match self {
            Self::BadRequest => 400,
            Self::Unauthorized => 401,
            Self::Forbidden | Self::SecurityBlocked | Self::ChallengeDetected => 403,
            Self::ActionUnknown | Self::CheckpointLost | Self::TargetRepairRequired => 409,
            Self::NotFound => 404,
            Self::Conflict | Self::UserTookOver => 409,
            Self::RateLimited => 429,
            Self::Timeout => 504,
            Self::DriverFailed | Self::UpstreamBlocked => 502,
            Self::Unsupported => 501,
            Self::UnsupportedMedia => 415,
            Self::BudgetExhausted => 402,
            Self::RuntimeNotReady => 503,
            Self::Internal => 500,
        }
    }

    /// The code as it travels on the wire (`SCREAMING_SNAKE_CASE`), as a static
    /// string. New receipts store this instead of the `Debug` rendering, which
    /// is a Rust identifier and not a contract.
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::BadRequest => "BAD_REQUEST",
            Self::Unauthorized => "UNAUTHORIZED",
            Self::Forbidden => "FORBIDDEN",
            Self::NotFound => "NOT_FOUND",
            Self::Conflict => "CONFLICT",
            Self::RateLimited => "RATE_LIMITED",
            Self::Timeout => "TIMEOUT",
            Self::SecurityBlocked => "SECURITY_BLOCKED",
            Self::ActionUnknown => "ACTION_UNKNOWN",
            Self::ChallengeDetected => "CHALLENGE_DETECTED",
            Self::RuntimeNotReady => "RUNTIME_NOT_READY",
            Self::CheckpointLost => "CHECKPOINT_LOST",
            Self::TargetRepairRequired => "TARGET_REPAIR_REQUIRED",
            Self::DriverFailed => "DRIVER_FAILED",
            Self::UpstreamBlocked => "UPSTREAM_BLOCKED",
            Self::Unsupported => "UNSUPPORTED",
            Self::UnsupportedMedia => "UNSUPPORTED_MEDIA",
            Self::BudgetExhausted => "BUDGET_EXHAUSTED",
            Self::UserTookOver => "USER_TOOK_OVER",
            Self::Internal => "INTERNAL",
        }
    }
}

#[derive(Debug, Clone, Error, Serialize, Deserialize)]
#[error("{code:?}: {message}")]
pub struct QuarryError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    pub retryable: bool,
}

impl QuarryError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
            retryable: code.retryable(),
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    /// WS12.S2: the error as it may leave the process. The message and every
    /// string in `details` pass the Live secret-redaction rules, so a driver
    /// or parser message that echoes page-derived text cannot carry a token or a
    /// card number to the caller. The code, the status and the retryability are
    /// unchanged, and so is the envelope shape. A no-op under `Mode::Off`.
    #[must_use]
    pub fn redacted(mut self) -> Self {
        use crate::redaction::{redact_json, redact_text, Profile, Tally};
        let mut tally = Tally::new();
        self.message = redact_text(&self.message, Profile::Live, &mut tally).into_owned();
        if let Some(details) = self.details.as_mut() {
            redact_json(details, Profile::Live, &mut tally);
        }
        self
    }

    /// Raw page script execution (`AgentAction::Evaluate`) is refused on every
    /// agent entry point: it runs in the page's main world and can express any
    /// action (submit, delete, read a sensitive field) without passing the typed
    /// action policy, the risk gates or a one-shot approval. The message and
    /// details never carry the script.
    pub fn raw_script_denied() -> Self {
        Self::new(
            ErrorCode::Forbidden,
            "raw page script execution (evaluate) is not permitted; use typed browser actions",
        )
        .with_details(serde_json::json!({
            "reason": "raw_script_denied",
            "denied_action": "evaluate",
        }))
    }

    /// A page reload (`AgentAction::Reload`) proposed by an agent (WS9.B). Reload is a
    /// person's action: it re-submits a form post and drops page state, which a model
    /// must not do on its own. Quarry decides who the agent is from the verified token
    /// (a service principal, or a person's token labelled `agent`), so the refusal holds
    /// on every entry point whatever a gateway in front did or did not check (D2-012).
    /// Nothing is consumed. Not retryable by the same caller.
    pub fn agent_reload_denied() -> Self {
        Self::new(
            ErrorCode::Forbidden,
            "reload is available to people, not to agents; use navigate",
        )
        .with_details(serde_json::json!({
            "reason": "agent_reload_denied",
            "denied_action": "reload",
        }))
    }

    /// The run met a strong challenge on a host, spent the one recovery it is
    /// allowed there, and asked for more. 403, not retryable by the same
    /// caller: the next step is a person (user take-over) or another source.
    /// `host` is left out of the details when it is empty, which is how a Zero
    /// Data Retention run avoids echoing the host back into a stored receipt.
    /// The message never names the host or the URL.
    pub fn challenge_detected(host: &str, reason: &str) -> Self {
        let mut details = serde_json::json!({
            "reason": reason,
            "recovery": "exhausted",
            "next": "user_takeover_or_other_source",
        });
        if !host.is_empty() {
            details["host"] = serde_json::Value::String(host.to_owned());
        }
        Self::new(
            ErrorCode::ChallengeDetected,
            "this host keeps presenting a challenge after the one allowed recovery; hand the run to a person or use another source",
        )
        .with_details(details)
    }

    /// A scrape fetched a page that is itself a bot wall (a CAPTCHA widget, a
    /// WAF interstitial, two independent block markers). 403, not retryable by the
    /// same caller. A scrape is one shot: it has no recovery to spend and no
    /// browser to hand to a person, so `recovery` is `not_applicable` and `next`
    /// is `other_source`. `block_reason` is one of `captcha_widget`,
    /// `waf_interstitial`, `unexpected_bot_error` (never `challenge_loop`, which
    /// only an agent run can reach) and is also given as `reason`, the key the
    /// agent path's `CHALLENGE_DETECTED` uses. Neither the message nor the details
    /// carry the host, the URL or any page text.
    pub fn challenge_detected_on_fetch(block_reason: &str) -> Self {
        Self::new(
            ErrorCode::ChallengeDetected,
            "the page is a bot challenge that Quarry does not get past; use another source",
        )
        .with_details(serde_json::json!({
            "reason": block_reason,
            "block_reason": block_reason,
            "recovery": "not_applicable",
            "next": "other_source",
        }))
    }

    /// The `details.reason` of a `TARGET_REPAIR_REQUIRED`, when it is one of
    /// the strings Quarry itself raises (the closed list in
    /// [`target_repair_reason`] or a legacy in-page string). Anything else, and
    /// any other error code, is `None`: a reason is never free text.
    pub fn target_repair_reason(&self) -> Option<&str> {
        if self.code != ErrorCode::TargetRepairRequired {
            return None;
        }
        self.details
            .as_ref()?
            .get("reason")?
            .as_str()
            .filter(|reason| target_repair_reason::is_known(reason))
    }

    /// A `DRIVER_FAILED` that carries one of the fetch sub-codes of
    /// [`driver_failed_reason`] (E3.8.1). `details.reason` is the sub-code and
    /// `retryable` is the sub-code's own flag, not the code's (false). A string
    /// outside the closed list is not a sub-code: the error is a plain
    /// `DRIVER_FAILED` with no reason, so free text never reaches a caller.
    pub fn driver_failed(reason: &str, message: impl Into<String>) -> Self {
        let Some(reason) = driver_failed_reason::canonical(reason) else {
            return Self::new(ErrorCode::DriverFailed, message);
        };
        let mut error = Self::new(ErrorCode::DriverFailed, message)
            .with_details(serde_json::json!({ "reason": reason }));
        error.retryable = driver_failed_reason::retryable(reason);
        error
    }

    /// The fetch sub-code of a `DRIVER_FAILED`, when `details.reason` is one of
    /// the closed list. Another code, another reason and free text are `None`.
    pub fn driver_failed_reason(&self) -> Option<&'static str> {
        if self.code != ErrorCode::DriverFailed {
            return None;
        }
        let raw = self.details.as_ref()?.get("reason")?.as_str()?;
        driver_failed_reason::canonical(raw)
    }

    pub fn unsupported_action(action: &str) -> Self {
        Self::new(
            ErrorCode::Unsupported,
            format!("unsupported action: {action}"),
        )
    }

    /// The text of this error that may go into a log line (WS12.S6). The
    /// message goes through the WS12 Durable redaction rules, so a secret, a
    /// token or a URL query value that a transport error or an excerpt carried
    /// does not reach `tracing`. `details` are left out: a log line carries the
    /// `error_class` field (`code.wire_name()`) instead. `Display` is unchanged
    /// and still shows the message as the caller gets it.
    pub fn log_message(&self) -> String {
        self.log_message_under(crate::redaction::mode())
    }

    fn log_message_under(&self, mode: crate::redaction::Mode) -> String {
        let mut tally = crate::redaction::Tally::new();
        crate::redaction::redact_text_with(
            mode,
            &self.message,
            crate::redaction::Profile::Durable,
            &mut tally,
        )
        .into_owned()
    }
}

/// The closed vocabulary of `details.reason` on a `TARGET_REPAIR_REQUIRED`
/// that the trusted-input actionability pipeline raises (WS3.K, behind
/// `QUARRY_BROWSER_TRUSTED_INPUT`). The strings are wire contract: a planner
/// keys its recovery sentence on them, so they never change meaning and a new
/// one is an additive, documented change (docs/ERROR_REASONS.md).
///
/// `target_covered` is the only reason with a second detail,
/// `covered_by_tag`: the tag name (`div`, `dialog`, `my-overlay`) of the
/// element that received the hit test, never its text, class, id or URL.
pub mod target_repair_reason {
    /// The element is gone from the document (a stale ref).
    pub const TARGET_NOT_ATTACHED: &str = "target_not_attached";
    /// `display: none`, `visibility: hidden` or no layout box at all.
    pub const TARGET_NOT_VISIBLE: &str = "target_not_visible";
    /// Disabled; for typing also read-only or not an editable control.
    pub const TARGET_DISABLED: &str = "target_disabled";
    /// Another element receives the pointer at the click point.
    pub const TARGET_COVERED: &str = "target_covered";
    /// The box kept moving for the whole stability window.
    pub const TARGET_UNSTABLE: &str = "target_unstable";
    /// The element is laid out but its box has no area.
    pub const TARGET_HAS_NO_BOX: &str = "target_has_no_box";
    /// The box is outside the viewport even after scrolling it into view.
    pub const TARGET_UNREACHABLE_AFTER_SCROLL: &str = "target_unreachable_after_scroll";

    /// Every reason the actionability pipeline can raise, in documentation order.
    pub const ALL: [&str; 7] = [
        TARGET_NOT_ATTACHED,
        TARGET_NOT_VISIBLE,
        TARGET_DISABLED,
        TARGET_COVERED,
        TARGET_UNSTABLE,
        TARGET_HAS_NO_BOX,
        TARGET_UNREACHABLE_AFTER_SCROLL,
    ];

    /// The reasons the synthetic (in-page script) path has always raised. They
    /// stay valid for one release after trusted input is GA and are not part of
    /// the closed list above.
    pub const LEGACY: [&str; 8] = [
        "native_target_not_connected",
        "target_not_unique_or_changed",
        "target_not_typeable",
        "value_setter_unavailable",
        "target_not_select",
        "select_value_unavailable",
        "unsupported_native_operation",
        "unsupported_verified_operation",
    ];

    /// The closed-list reason a string stands for: itself when it is in
    /// [`ALL`], or the reason a legacy string is an alias of
    /// (`native_target_not_connected` is `target_not_attached`).
    pub fn canonical(raw: &str) -> Option<&'static str> {
        if let Some(known) = ALL.iter().find(|known| **known == raw) {
            return Some(known);
        }
        match raw {
            "native_target_not_connected" => Some(TARGET_NOT_ATTACHED),
            _ => None,
        }
    }

    /// Whether `raw` is a reason Quarry itself raises (closed list or legacy).
    pub fn is_known(raw: &str) -> bool {
        ALL.contains(&raw) || LEGACY.contains(&raw)
    }
}

/// The closed vocabulary of `details.reason` on a `DRIVER_FAILED` that the fetch
/// waterfall raises (E3.8.1). No new `ErrorCode` (R-10): the code stays
/// `DRIVER_FAILED` (502) and the reason says why. Each reason carries its own
/// `retryable` flag. The strings are wire contract: a new one is an additive,
/// documented change (docs/ERROR_REASONS.md) and mirrors into the Go errs package.
pub mod driver_failed_reason {
    /// The host name did not resolve. A resolver fault can clear on a repeat; a
    /// name that does not exist fails the same way, so one or two retries at most.
    pub const DNS: &str = "dns";
    /// The TLS handshake or the certificate check failed. A repeat of the same
    /// call cannot change that; another TLS profile is a fallback tier's job.
    pub const TLS: &str = "tls";
    /// No TCP connection could be made (refused, reset, unreachable).
    pub const CONNECT: &str = "connect";
    /// Every tier of the chain was tried and none served the page, and the tiers
    /// did not agree on one cause. Each tier's error is in `fallback_attempts`.
    pub const ALL_DRIVERS_FAILED: &str = "all_drivers_failed";

    /// Every sub-code, in documentation order.
    pub const ALL: [&str; 4] = [DNS, TLS, CONNECT, ALL_DRIVERS_FAILED];

    /// The sub-code a string stands for, when it is one of [`ALL`].
    pub fn canonical(raw: &str) -> Option<&'static str> {
        ALL.iter().find(|known| **known == raw).copied()
    }

    /// Whether a repeat of the same call can succeed. `dns` and `connect` can
    /// clear on their own; `tls` and `all_drivers_failed` do not clear on a
    /// straight repeat. A string outside the list is not retryable.
    pub fn retryable(reason: &str) -> bool {
        canonical(reason).is_some_and(|known| known == DNS || known == CONNECT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reason_strings_are_closed_and_stable() {
        // Wire contract: these exact strings, in this order.
        assert_eq!(
            target_repair_reason::ALL,
            [
                "target_not_attached",
                "target_not_visible",
                "target_disabled",
                "target_covered",
                "target_unstable",
                "target_has_no_box",
                "target_unreachable_after_scroll",
            ]
        );
        for reason in target_repair_reason::ALL
            .iter()
            .chain(target_repair_reason::LEGACY.iter())
        {
            assert!(
                reason.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
                "{reason} is not snake_case"
            );
            assert!(target_repair_reason::is_known(reason));
        }
        let mut every: Vec<&str> = target_repair_reason::ALL
            .iter()
            .chain(target_repair_reason::LEGACY.iter())
            .copied()
            .collect();
        every.sort_unstable();
        let before = every.len();
        every.dedup();
        assert_eq!(before, every.len(), "a reason is listed twice");
        // The legacy alias resolves to the closed list; an unknown string does not.
        assert_eq!(
            target_repair_reason::canonical("native_target_not_connected"),
            Some("target_not_attached")
        );
        assert_eq!(
            target_repair_reason::canonical("target_covered"),
            Some("target_covered")
        );
        assert_eq!(target_repair_reason::canonical("target_not_typeable"), None);
        assert_eq!(target_repair_reason::canonical("anything else"), None);
        assert!(!target_repair_reason::is_known("anything else"));
    }

    #[test]
    fn target_repair_reason_reads_only_a_known_reason_of_a_repair_error() {
        let covered = QuarryError::new(ErrorCode::TargetRepairRequired, "covered").with_details(
            serde_json::json!({ "reason": "target_covered", "covered_by_tag": "div" }),
        );
        assert_eq!(covered.target_repair_reason(), Some("target_covered"));
        let legacy = QuarryError::new(ErrorCode::TargetRepairRequired, "gone").with_details(
            serde_json::json!({ "ok": false, "reason": "native_target_not_connected" }),
        );
        assert_eq!(
            legacy.target_repair_reason(),
            Some("native_target_not_connected")
        );
        // Another code never reports one, and a free-text reason is not echoed.
        let other = QuarryError::new(ErrorCode::DriverFailed, "x")
            .with_details(serde_json::json!({ "reason": "target_covered" }));
        assert_eq!(other.target_repair_reason(), None);
        let free = QuarryError::new(ErrorCode::TargetRepairRequired, "x")
            .with_details(serde_json::json!({ "reason": "Buy now at https://example.test" }));
        assert_eq!(free.target_repair_reason(), None);
        let none = QuarryError::new(ErrorCode::TargetRepairRequired, "x");
        assert_eq!(none.target_repair_reason(), None);
        // The wire code and status do not move.
        assert_eq!(ErrorCode::TargetRepairRequired.http_status(), 409);
        assert_eq!(
            ErrorCode::TargetRepairRequired.wire_name(),
            "TARGET_REPAIR_REQUIRED"
        );
    }
}

#[cfg(test)]
mod log_message_tests {
    use super::*;
    use crate::redaction::Mode;

    #[test]
    fn log_message_masks_a_secret_and_keeps_details_out() {
        let err = QuarryError::new(
            ErrorCode::DriverFailed,
            "login failed: password=hunter2 on the form",
        )
        .with_details(serde_json::json!({ "url": "https://example.test/private/path" }));
        // The log text: the secret is masked, the details never appear.
        let logged = err.log_message_under(Mode::Enforce);
        assert!(
            !logged.contains("hunter2"),
            "secret reached the log: {logged}"
        );
        assert!(
            !logged.contains("private/path"),
            "details reached the log: {logged}"
        );
        assert!(
            logged.contains("login failed"),
            "the message text is kept: {logged}"
        );
        // Display is untouched: it still carries the message the caller gets.
        assert!(err.to_string().contains("hunter2"));
    }

    #[test]
    fn log_message_under_off_returns_the_message_as_is() {
        let err = QuarryError::new(ErrorCode::Internal, "plain text, nothing to mask");
        assert_eq!(
            err.log_message_under(Mode::Off),
            "plain text, nothing to mask"
        );
    }
}

#[cfg(test)]
mod driver_failed_reason_tests {
    use super::*;

    #[test]
    fn driver_failed_sub_codes_are_closed_and_stable() {
        // Wire contract: these exact strings, in this order.
        assert_eq!(
            driver_failed_reason::ALL,
            ["dns", "tls", "connect", "all_drivers_failed"]
        );
        for reason in driver_failed_reason::ALL {
            assert_eq!(driver_failed_reason::canonical(reason), Some(reason));
        }
        // Closed and case-sensitive: an unknown or upper-case string is not one.
        assert_eq!(driver_failed_reason::canonical("DNS"), None);
        assert_eq!(driver_failed_reason::canonical("action_failed"), None);
        assert_eq!(driver_failed_reason::canonical("anything else"), None);
    }

    #[test]
    fn each_sub_code_carries_its_own_retryable_flag() {
        assert!(driver_failed_reason::retryable("dns"));
        assert!(driver_failed_reason::retryable("connect"));
        assert!(!driver_failed_reason::retryable("tls"));
        assert!(!driver_failed_reason::retryable("all_drivers_failed"));
        // Not a sub-code, so not retryable either.
        assert!(!driver_failed_reason::retryable("anything else"));
        // The code itself does not move: no new code, still 502, not retryable by code.
        assert!(!ErrorCode::DriverFailed.retryable());
        assert_eq!(ErrorCode::DriverFailed.http_status(), 502);
        assert_eq!(ErrorCode::DriverFailed.wire_name(), "DRIVER_FAILED");
    }

    #[test]
    fn driver_failed_constructor_sets_the_sub_code_and_its_own_flag() {
        let dns = QuarryError::driver_failed(driver_failed_reason::DNS, "lookup failed");
        assert_eq!(dns.code, ErrorCode::DriverFailed);
        assert!(dns.retryable, "dns carries its own flag, not the code's");
        assert_eq!(dns.details.as_ref().expect("details")["reason"], "dns");
        assert_eq!(dns.driver_failed_reason(), Some("dns"));

        let tls = QuarryError::driver_failed(driver_failed_reason::TLS, "handshake failed");
        assert!(!tls.retryable);
        assert_eq!(tls.driver_failed_reason(), Some("tls"));

        // Free text is not a sub-code: no details, not retryable, nothing echoed.
        let free = QuarryError::driver_failed("https://example.test/private", "x");
        assert!(free.details.is_none());
        assert!(!free.retryable);
        assert_eq!(free.driver_failed_reason(), None);
    }

    #[test]
    fn driver_failed_reason_reads_only_a_sub_code_of_a_driver_failed() {
        let other_code = QuarryError::new(ErrorCode::Timeout, "x")
            .with_details(serde_json::json!({ "reason": "dns" }));
        assert_eq!(other_code.driver_failed_reason(), None);
        let typed = QuarryError::new(ErrorCode::DriverFailed, "x")
            .with_details(serde_json::json!({ "reason": "action_failed" }));
        assert_eq!(typed.driver_failed_reason(), None);
        let none = QuarryError::new(ErrorCode::DriverFailed, "x");
        assert_eq!(none.driver_failed_reason(), None);
    }
}
