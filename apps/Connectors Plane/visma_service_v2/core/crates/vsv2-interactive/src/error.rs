use thiserror::Error;

pub type Result<T> = std::result::Result<T, InteractiveError>;

#[derive(Debug, Error)]
pub enum InteractiveError {
    #[error("write denied: {0}")]
    Denied(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("unknown write operation: {0}")]
    UnknownOperation(String),

    #[error("confirm token does not match this plan")]
    TokenInvalid,

    #[error("plan expired: created {age_secs}s ago, limit {ttl_secs}s")]
    PlanExpired { age_secs: i64, ttl_secs: i64 },

    #[error("plan {0} was already executed (or an attempt was recorded); build a new plan")]
    AlreadyExecuted(String),

    #[error("audit store error: {0}")]
    Audit(String),

    #[error("outcome unknown: {0}. Verify the record in Visma before planning the change again")]
    OutcomeUnknown(String),

    #[error("upstream refused the write with status {status}: {body}")]
    Upstream { status: u16, body: String },

    /// The request never left: no credential, no scope, rate-limited, or a header that cannot be sent. The write
    /// certainly did not happen, unlike [`OutcomeUnknown`](Self::OutcomeUnknown).
    #[error("not sent: {0}")]
    NotSent(String),

    /// A precondition read failed (nothing was written).
    #[error("could not read the record (status {status}): {body}")]
    ReadFailed { status: u16, body: String },

    #[error(transparent)]
    Common(#[from] vsv2_common::Error),
}
