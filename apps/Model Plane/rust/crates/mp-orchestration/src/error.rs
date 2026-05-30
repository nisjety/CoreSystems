//! Orchestration domain errors. Every illegal state transition or invariant
//! violation surfaces through this single `OrchestrationError` type so callers
//! in any service get consistent error semantics.

use thiserror::Error;

/// Result alias for orchestration operations.
pub type OrchestrationResult<T> = Result<T, OrchestrationError>;

/// All domain errors the orchestration records can emit.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum OrchestrationError {
    /// State transition is not permitted from the current state.
    #[error("illegal transition from {from} to {to} on {kind}")]
    IllegalTransition {
        /// Record kind, e.g. "plan", "todo", "approval".
        kind: &'static str,
        /// Source state as rendered by its `Display`/`as_str`.
        from: String,
        /// Target state.
        to: String,
    },

    /// Required field was missing at construction time.
    #[error("missing required field {field} on {kind}")]
    MissingField {
        /// Record kind.
        kind: &'static str,
        /// Field name.
        field: &'static str,
    },

    /// Invariant violated at validation time.
    #[error("{kind} invariant violated: {detail}")]
    InvariantViolation {
        /// Record kind.
        kind: &'static str,
        /// Human-readable explanation.
        detail: String,
    },

    /// A referenced record (by id) was not found in the lookup scope.
    #[error("{kind} not found: {id}")]
    NotFound {
        /// Record kind.
        kind: &'static str,
        /// Identifier that was looked up.
        id: String,
    },
}

impl OrchestrationError {
    /// Canonical error code for HTTP / gRPC mapping. Stable across languages.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::IllegalTransition { .. } => "ILLEGAL_TRANSITION",
            Self::MissingField { .. } => "MISSING_FIELD",
            Self::InvariantViolation { .. } => "INVARIANT_VIOLATION",
            Self::NotFound { .. } => "NOT_FOUND",
        }
    }
}
