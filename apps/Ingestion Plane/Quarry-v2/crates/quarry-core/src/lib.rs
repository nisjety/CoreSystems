//! quarry-core — shared contracts for Quarry V2 Rust plane.
//!
//! Mirrors `docs/CONTRACTS.md` (Phase 0). Go side: `pkg/quarrycontracts`.
//!
//! No I/O. No runtime. Only types, IDs, envelopes, policy, errors.

pub mod ai_spec;
pub mod artifact;
pub mod benchmark;
pub mod cache;
pub mod capture;
pub mod change_history;
pub mod contracts;
pub mod crawl_denial;
pub mod crawl_scope;
pub mod driver_meta;
pub mod envelope;
pub mod error;
pub mod event;
pub mod ids;
pub mod job_history;
pub mod json_schema;
pub mod lease;
pub mod output;
pub mod output_profile;
pub mod page_emulation;
pub mod pagination;
pub mod policy;
pub mod presets;
pub mod privacy;
pub mod redaction;
pub mod resources;
pub mod scrape;
pub mod scrape_actions;
pub mod step_expect;
pub mod tracked_snapshot;
pub mod web_policy;
pub mod zdr;
pub mod zdr_matrix;

pub use error::{ErrorCode, QuarryError, QuarryResult};
