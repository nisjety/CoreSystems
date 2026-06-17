//! quarry-core — shared contracts for Quarry V2 Rust plane.
//!
//! Mirrors `docs/CONTRACTS.md` (Phase 0). Go side: `pkg/quarrycontracts`.
//!
//! No I/O. No runtime. Only types, IDs, envelopes, policy, errors.

pub mod artifact;
pub mod benchmark;
pub mod cache;
pub mod change_history;
pub mod contracts;
pub mod crawl_denial;
pub mod envelope;
pub mod error;
pub mod event;
pub mod ids;
pub mod job_history;
pub mod json_schema;
pub mod lease;
pub mod output;
pub mod output_profile;
pub mod pagination;
pub mod policy;
pub mod presets;
pub mod privacy;
pub mod resources;
pub mod zdr;

pub use error::{ErrorCode, QuarryError, QuarryResult};
