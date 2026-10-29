//! `vsv2-orchestrator` — sync engine.
//!
//! Phase 2 — [`registry`] loads endpoint definitions from
//!          `config/endpoints/*.json`.
//! Phase 3 — [`dispatch`] picks the right [`adapter::FetchAdapter`] for
//!          each (tenant, endpoint) using [`vsv2_common::Source`].
//! Phase 4 — [`shadow`] writes DM records to `_dm_shadow` BigQuery tables
//!          for diff vs the live REST-sourced bronze.

pub mod adapter;
pub mod bq_writer;
pub mod dispatch;
pub mod dm_adapter;
pub mod erp_adapter;
pub mod executor;
pub mod expense_adapter;
pub mod noop_adapter;
pub mod open_set;
pub mod partition;
pub mod registry;
pub mod scheduler;
pub mod shadow;
pub mod validation;

mod spool;

pub use adapter::{FetchAdapter, FetchPartitionResult, FetchWindow};
pub use bq_writer::{BqWriter, StageOutcome};
pub use dispatch::DispatchTable;
pub use dm_adapter::DatamartAdapter;
pub use erp_adapter::ErpRestAdapter;
pub use executor::{EndpointSyncFailure, SyncExecutionSummary, SyncExecutor};
pub use expense_adapter::ExpenseAdapter;
pub use noop_adapter::NoopAdapter;
pub use open_set::{OpenSetReport, OpenSetSource};
pub use partition::{build_partitions_from_dimensions, PartitionPlan};
pub use registry::{DatamartBinding, EndpointDefinition, EndpointRegistry};
pub use scheduler::{ScheduledTenant, Scheduler, Tier, TierSpec};
pub use shadow::ShadowWriter;
pub use validation::{validate_records, ValidationIssue, ValidationOutcome};
