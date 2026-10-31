//! `vsv2-erp-rest` — ERP REST client (api.finance.visma.net and absolute
//! endpoint URLs for adjacent Visma services).
//!
//! Supports per-endpoint pagination profiles so Finance API endpoints use
//! `pageSize`/`pageNumber`, while SalesOrder and JournalTx endpoints use
//! zero-based `pageIndex`.

mod body;
pub mod client;
pub mod paginate;
mod walk_guard;

pub use client::{BackgroundOpts, ErpRestClient, JsonResponse};
pub use paginate::{
    paginate, paginate_stream, paginate_with_metrics, PageOpts, PaginatedRecords, WalkSummary,
};
pub use vsv2_common::PaginationProfile;
