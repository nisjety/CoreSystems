use serde::{Deserialize, Serialize};
use vsv2_common::TenantId;

/// Maps a Visma.net `tenant_id` to a DataMart `companyId`.
///
/// Filled from `config/tenants/*.json` by `vsv2 tenants-register`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantCompanyMap {
    pub tenant: TenantId,
    pub company_id: String,
    pub company_name: Option<String>,
}

/// What registering one tenant did, or would do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registration {
    Created,
    Updated,
    Unchanged,
    /// The tenant's config has no `datamart_company_id` yet.
    SkippedNoCompanyId,
}

impl Registration {
    pub fn label(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Updated => "updated",
            Self::Unchanged => "unchanged",
            Self::SkippedNoCompanyId => "skipped",
        }
    }
}
