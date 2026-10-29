//! `xero-common` — shared types, errors, and configuration for xero_service_v2.

pub mod config;
pub mod error;
pub mod types;

pub use config::{
    load_dotenv, load_dotenv_file, load_dotenv_path, AppConfig, GcsConfig, SyncConfig,
    XeroCustomConnectionConfig,
};
pub use error::{Error, Result};
pub use types::{EntityType, TenantId};
