//! Data-Plane half of cross-plane GDPR organization erasure, for the tables
//! this crate owns: `retrieval_runs`, `access_audit_log`, `admin_audit_log`,
//! `agent_retrieval_configs`, `context_pins` (see `purge` module docs for the
//! full rationale, including the `retrieval_candidates` cascade and the
//! audit-log retention check).
//!
//! `event` decodes and gates the shared `velion.gdpr.erasure.requested` fan-out
//! (organization-scoped erasure only — see its module docs for the per-user
//! safety contract); `purge` performs the actual hard-delete; `consumer` is
//! the NATS JetStream transport that wires the two together.

pub mod consumer;
pub mod event;
pub mod purge;

// Only the two functions `consumer` actually calls are re-exported here for
// ergonomic same-crate use; `ErasureEventError`/`OrganizationErasure`
// (`event`) and `PurgeSummary` (`purge`) have no cross-module consumer today
// and stay reachable via their full `gdpr::event::`/`gdpr::purge::` paths
// (e.g. for a future caller that needs to name the type) without tripping an
// `unused_imports` warning in the `retrieval-engine` binary crate root, which
// — unlike the `retrieval_engine` library crate `tests/*.rs` compiles
// against — has no external consumer that could ever use an unconsumed
// re-export.
pub use event::parse_erasure_event;
pub use purge::purge_organization_data;
