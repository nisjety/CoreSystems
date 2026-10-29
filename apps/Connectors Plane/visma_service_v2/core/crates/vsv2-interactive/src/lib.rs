//! `vsv2-interactive` — Layer 3: the only place in v2 that changes data in Visma.
//!
//! Layer 2 (`vsv2-erp-rest`) stays read-only on the read scopes. Everything that creates or
//! updates an ERP record goes through here, in two steps:
//!
//! 1. [`Executor::dry_run`] builds the exact request (reading the current ETag where needed),
//!    audits it and returns a [`Plan`] plus a confirm token bound to that plan.
//! 2. [`Executor::execute`] re-checks policy and the token, audits *before* sending, sends, and
//!    audits the outcome.
//!
//! Guardrails are structural, not conventions: there is no DELETE method in [`WriteMethod`],
//! writes are denied unless tenant *and* operation are allow-listed, and a plan can be executed
//! at most once.

pub mod audit;
pub mod error;
pub mod executor;
pub mod operation;
pub mod operations;
pub mod pg_audit;
pub mod plan;
pub mod policy;
pub mod request;
mod schema;
pub mod transport;

pub use audit::{AuditEntry, AuditSink, AuditStage, MemoryAuditSink};
pub use error::{InteractiveError, Result};
pub use executor::{DryRun, Executed, Executor};
pub use operation::{Built, Operation, OperationRegistry};
pub use operations::{default_registry, SalesOrderPatch};
pub use pg_audit::PgAuditSink;
pub use plan::{ConfirmSigner, Plan};
pub use policy::WritePolicy;
pub use request::{validate_path, WriteMethod, WriteRequest, WriteTarget};
pub use transport::{HttpTransport, ReadResponse, Reader, Transport, WriteResponse};
