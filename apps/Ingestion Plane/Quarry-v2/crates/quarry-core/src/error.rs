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
            Self::Conflict => 409,
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

    pub fn unsupported_action(action: &str) -> Self {
        Self::new(
            ErrorCode::Unsupported,
            format!("unsupported action: {action}"),
        )
    }
}
