use vsv2_common::TenantId;

/// Resolve `tenant_id` → DataMart `companyId`.
///
/// Phase 0 hard-codes Aquatiq AS via env. Phase 9 reads from
/// `tenant_company_map` table in the state store.
#[derive(Debug, Clone, Default)]
pub struct CompanyFilter {
    map: std::collections::HashMap<TenantId, String>,
}

impl CompanyFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, tenant: TenantId, company_id: impl Into<String>) -> Self {
        self.map.insert(tenant, company_id.into());
        self
    }

    pub fn company_id_for(&self, tenant: &TenantId) -> Option<&str> {
        self.map.get(tenant).map(String::as_str)
    }
}
