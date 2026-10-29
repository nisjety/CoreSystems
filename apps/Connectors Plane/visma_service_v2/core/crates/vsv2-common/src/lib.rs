//! `vsv2-common` — shared types, errors, config, source enum.
//!
//! Used by every other crate in the workspace. Keep this crate dep-light.

pub mod config;
pub mod endpoint;
pub mod error;
pub mod page_metric;
pub mod page_sink;
pub mod source;
pub mod tenant;
pub mod text;

pub use endpoint::{BackgroundMode, PaginationProfile, PrimaryKey};
pub use error::{Error, Result};
pub use page_metric::{PageFetchMetric, RateLimitSnapshot};
pub use page_sink::PageSink;
pub use source::Source;
pub use tenant::{load_tenants, validate_tenants, TenantConfig, TenantId, TenantStatus};
