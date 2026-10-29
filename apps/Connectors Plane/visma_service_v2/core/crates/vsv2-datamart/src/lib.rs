//! `vsv2-datamart` — Visma Net DataMart GraphQL client.
//!
//! Endpoint: `https://datamart.visma.net/graphql`.
//! Pagination: `(pageSize, lastId)` with `id` ascending.
//! Watermark: `updatedInDataMartDateTime`.
//! Filtering: every entity's PK starts with `companyId`.

pub mod client;
pub mod company_filter;
pub mod paginator;
pub mod query;

pub use client::{DatamartClient, JsonResponse};
pub use company_filter::CompanyFilter;
pub use paginator::{
    paginate_all, paginate_all_stream, paginate_all_with_metrics, CursorPaginator, CursorWalk,
    PaginatedRecords,
};
pub use query::QueryFile;
