//! Typed errors for eventlog domain operations.

use thiserror::Error;

/// Result alias for eventlog operations.
pub type EventLogResult<T> = Result<T, EventLogError>;

/// All failure modes a caller might see when building filters, cursors, or
/// idempotency keys. DB / RPC errors are *out of scope* — those surface
/// through the calling service's own error types.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EventLogError {
    /// Required field missing at construction time.
    #[error("eventlog: missing required field {field}")]
    MissingField {
        /// Field name.
        field: &'static str,
    },

    /// Field present but invalid (wrong shape, out-of-range, etc).
    #[error("eventlog: invalid {field}: {detail}")]
    InvalidField {
        /// Field name.
        field: &'static str,
        /// Human-readable detail.
        detail: String,
    },

    /// Cursor decode failed (corrupt, forged, wrong version).
    #[error("eventlog: cursor decode failed: {0}")]
    CursorDecode(String),
}

impl EventLogError {
    /// Stable error code for cross-language RPC mapping.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::MissingField { .. } => "MISSING_FIELD",
            Self::InvalidField { .. } => "INVALID_FIELD",
            Self::CursorDecode(_) => "CURSOR_DECODE",
        }
    }
}
