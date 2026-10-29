//! `vsv2-expense` — Visma Expense Public API client.
//!
//! Separate domain from ERP/DataMart. Independent rollout track per Phase 7.

pub mod client;
pub mod paginate;

pub use client::{ExpenseClient, JsonResponse};
pub use paginate::{paginate, paginate_with_metrics, ExpensePageOpts, PaginatedRecords};
