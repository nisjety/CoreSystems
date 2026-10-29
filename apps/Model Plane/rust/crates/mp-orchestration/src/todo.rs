//! Todo records. Owned by `session-core`.
//!
//! Todos are durable work items the agent or user creates within a thread.
//! Unlike plans, todos are free-form and may span runs.
//!
//! State machine:
//! ```text
//!   pending ──start──▶ in_progress ──complete──▶ completed
//!      │                    │
//!      ├──block──▶ blocked  │
//!      ├──cancel──▶ cancelled
//!      │                    └──block──▶ blocked
//!      └──assign──▶ pending (just reassign)
//!
//!   blocked ──unblock──▶ pending | in_progress (whichever preceded)
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{OrchestrationError, OrchestrationResult};

/// Todo lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoState {
    /// Created; waiting to be started.
    Pending,
    /// Currently being worked on.
    InProgress,
    /// Paused / blocked on a dependency.
    Blocked,
    /// Finished successfully.
    Completed,
    /// Cancelled before completion.
    Cancelled,
}

impl TodoState {
    /// Canonical wire string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }

    /// True when no further transitions are allowed.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }
}

/// Priority bands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum TodoPriority {
    /// Low priority.
    Low,
    /// Normal priority (default).
    #[default]
    Normal,
    /// High priority.
    High,
    /// Urgent; should be addressed immediately.
    Urgent,
}

/// Durable work-item record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    /// Todo ULID (prefix `todo_`).
    pub id: String,
    /// Thread this todo lives in.
    pub thread_id: String,
    /// Creating run (optional).
    #[serde(default)]
    pub run_id: String,
    /// Assignee (user id or agent id).
    pub assignee: String,
    /// Title.
    pub title: String,
    /// Optional long description.
    #[serde(default)]
    pub description: String,
    /// Current state.
    pub state: TodoState,
    /// Priority.
    pub priority: TodoPriority,
    /// IDs of todos that must complete first.
    #[serde(default)]
    pub blocked_by: Vec<String>,
    /// Arbitrary metadata.
    #[serde(default)]
    pub metadata: serde_json::Value,
    /// Created at.
    pub created_at: DateTime<Utc>,
    /// Last updated.
    pub updated_at: DateTime<Utc>,
    /// Completed at (populated in terminal states).
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
}

impl Todo {
    const KIND: &'static str = "todo";

    /// Construct a new todo in `Pending` state.
    ///
    /// # Errors
    /// - `MissingField("id" | "thread_id" | "title" | "assignee")`
    pub fn new(
        id: impl Into<String>,
        thread_id: impl Into<String>,
        assignee: impl Into<String>,
        title: impl Into<String>,
        now: DateTime<Utc>,
    ) -> OrchestrationResult<Self> {
        let id = id.into();
        if id.is_empty() {
            return Err(OrchestrationError::MissingField {
                kind: Self::KIND,
                field: "id",
            });
        }
        let thread_id = thread_id.into();
        if thread_id.is_empty() {
            return Err(OrchestrationError::MissingField {
                kind: Self::KIND,
                field: "thread_id",
            });
        }
        let assignee = assignee.into();
        if assignee.is_empty() {
            return Err(OrchestrationError::MissingField {
                kind: Self::KIND,
                field: "assignee",
            });
        }
        let title = title.into();
        if title.is_empty() {
            return Err(OrchestrationError::MissingField {
                kind: Self::KIND,
                field: "title",
            });
        }
        Ok(Self {
            id,
            thread_id,
            run_id: String::new(),
            assignee,
            title,
            description: String::new(),
            state: TodoState::Pending,
            priority: TodoPriority::Normal,
            blocked_by: Vec::new(),
            metadata: serde_json::Value::Null,
            created_at: now,
            updated_at: now,
            completed_at: None,
        })
    }

    /// Attempt a state transition.
    ///
    /// # Errors
    /// - `IllegalTransition` when `(self.state, next)` is not in the allowed set.
    pub fn transition(&mut self, next: TodoState, now: DateTime<Utc>) -> OrchestrationResult<()> {
        if !Self::is_allowed(self.state, next) {
            return Err(OrchestrationError::IllegalTransition {
                kind: Self::KIND,
                from: self.state.as_str().to_owned(),
                to: next.as_str().to_owned(),
            });
        }
        self.state = next;
        self.updated_at = now;
        if next.is_terminal() {
            self.completed_at = Some(now);
        }
        Ok(())
    }

    /// Reassign the todo (allowed in any non-terminal state).
    ///
    /// # Errors
    /// - `InvariantViolation` when terminal.
    /// - `MissingField` when `new_assignee` is empty.
    pub fn reassign(
        &mut self,
        new_assignee: impl Into<String>,
        now: DateTime<Utc>,
    ) -> OrchestrationResult<()> {
        if self.state.is_terminal() {
            return Err(OrchestrationError::InvariantViolation {
                kind: Self::KIND,
                detail: format!("cannot reassign in terminal state {}", self.state.as_str()),
            });
        }
        let a = new_assignee.into();
        if a.is_empty() {
            return Err(OrchestrationError::MissingField {
                kind: Self::KIND,
                field: "assignee",
            });
        }
        self.assignee = a;
        self.updated_at = now;
        Ok(())
    }

    /// Whether a transition is in the allowed set.
    #[must_use]
    pub fn is_allowed(from: TodoState, to: TodoState) -> bool {
        use TodoState::{Blocked, Cancelled, Completed, InProgress, Pending};
        matches!(
            (from, to),
            (Pending | Blocked, InProgress)
                | (Pending | InProgress, Blocked)
                | (Pending | InProgress | Blocked, Cancelled)
                | (InProgress, Completed)
                | (Blocked, Pending)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).expect("valid epoch")
    }

    fn make() -> Todo {
        Todo::new("todo_01", "thread_01", "alice", "refactor auth", now()).expect("ctor")
    }

    #[test]
    fn starts_pending_with_normal_priority() {
        let t = make();
        assert_eq!(t.state, TodoState::Pending);
        assert_eq!(t.priority, TodoPriority::Normal);
    }

    #[test]
    fn pending_to_in_progress_to_completed() {
        let mut t = make();
        t.transition(TodoState::InProgress, now()).unwrap();
        t.transition(TodoState::Completed, now()).unwrap();
        assert!(t.state.is_terminal());
        assert!(t.completed_at.is_some());
    }

    #[test]
    fn block_and_unblock_round_trip() {
        let mut t = make();
        t.transition(TodoState::InProgress, now()).unwrap();
        t.transition(TodoState::Blocked, now()).unwrap();
        t.transition(TodoState::InProgress, now()).unwrap();
        assert_eq!(t.state, TodoState::InProgress);
    }

    #[test]
    fn cannot_skip_to_completed() {
        let mut t = make();
        assert!(t.transition(TodoState::Completed, now()).is_err());
    }

    #[test]
    fn cannot_transition_from_completed() {
        let mut t = make();
        t.transition(TodoState::InProgress, now()).unwrap();
        t.transition(TodoState::Completed, now()).unwrap();
        assert!(t.transition(TodoState::InProgress, now()).is_err());
    }

    #[test]
    fn reassign_disallowed_when_terminal() {
        let mut t = make();
        t.transition(TodoState::Cancelled, now()).unwrap();
        assert!(t.reassign("bob", now()).is_err());
    }

    #[test]
    fn reassign_rejects_empty() {
        let mut t = make();
        assert!(t.reassign("", now()).is_err());
    }

    #[test]
    fn todo_roundtrips_json() {
        let mut t = make();
        t.description = "long description".into();
        t.priority = TodoPriority::High;
        t.blocked_by = vec!["todo_02".into()];
        let s = serde_json::to_string(&t).unwrap();
        let back: Todo = serde_json::from_str(&s).unwrap();
        assert_eq!(back.id, t.id);
        assert_eq!(back.priority, TodoPriority::High);
        assert_eq!(back.blocked_by, vec!["todo_02"]);
    }
}
