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

    pub fn unsupported_action(action: &str) -> Self {
        Self::new(
            ErrorCode::Unsupported,
            format!("unsupported action: {action}"),
        )
    }
}
