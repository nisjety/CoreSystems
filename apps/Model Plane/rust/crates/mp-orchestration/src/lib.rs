//! mp-orchestration — canonical durable record types for the orchestration shell.
//!
//! Owns the invariants for:
//!   - **Plans** — structured proposed-action records, owner `session-core`
//!   - **Todos** — durable work-item records, owner `session-core`
//!   - **Approvals** — risky-action gate records, owner `session-core`
//!   - **SubagentLineage** — parent/child run graph, owner `session-core`
//!   - **RunEvent** — envelope wrapper with typed orchestration payloads
//!
//! Design rules:
//!   - Every record has a state enum with explicit allowed transitions.
//!   - Transitions return `Result` so illegal changes are refused at the type
//!     boundary, never silently tolerated.
//!   - No IO. No async. No service dependency. This is the same seat as
//!     `mp-ids` / `mp-events`: a pure-data crate consumed by every service.
//!   - Serde JSON round-trips are part of the contract and covered by tests.
//!
//! Services that will consume this crate:
//!   - `session-core` — stores plans/todos/approvals; emits RunEvents
//!   - `execution-core` — transitions approvals + updates run state
//!   - `orchestrator-core` — watches RunEvents for recovery + coordination
//!   - `model-gateway` — serializes REST responses over these types

#![deny(missing_docs)]

pub mod approval;
pub mod error;
pub mod plan;
pub mod proto_shim;
pub mod run_event;
pub mod subagent;
pub mod todo;

pub use approval::{Approval, ApprovalKind, ApprovalState};
pub use error::{OrchestrationError, OrchestrationResult};
pub use plan::{Plan, PlanState, PlanStep, PlanStepState};
pub use proto_shim::ShimError;
pub use run_event::{OrchestrationEvent, OrchestrationEventKind};
pub use subagent::{SubagentLineage, SubagentRole};
pub use todo::{Todo, TodoPriority, TodoState};
