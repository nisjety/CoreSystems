//! Live, read-only access to Visma: tenant resolution, ERP REST reads, DataMart GraphQL.

pub mod graphql_guard;
pub mod tenants;

use async_trait::async_trait;
use serde_json::Value;
use vsv2_common::TenantId;

pub use tenants::Tenants;

/// One GET against an ERP REST/SalesOrder URL (catalog-supplied path, never caller-supplied).
#[async_trait]
pub trait ErpGet: Send + Sync {
    async fn get(
        &self,
        tenant: &TenantId,
        path: &str,
        query: &[(String, String)],
    ) -> Result<Value, String>;
}

#[async_trait]
pub trait GraphQl: Send + Sync {
    async fn run(&self, tenant: &TenantId, query: &str, variables: Value) -> Result<Value, String>;
}

pub struct LiveErp(pub vsv2_erp_rest::ErpRestClient);

#[async_trait]
impl ErpGet for LiveErp {
    async fn get(
        &self,
        tenant: &TenantId,
        path: &str,
        query: &[(String, String)],
    ) -> Result<Value, String> {
        self.0
            .get_json(
                tenant,
                path,
                query,
                &vsv2_erp_rest::BackgroundOpts::default(),
            )
            .await
            .map_err(|e| e.to_string())
    }
}

pub struct LiveGraphQl(pub vsv2_datamart::DatamartClient);

#[async_trait]
impl GraphQl for LiveGraphQl {
    async fn run(&self, tenant: &TenantId, query: &str, variables: Value) -> Result<Value, String> {
        self.0
            .execute(tenant, query, variables)
            .await
            .map_err(|e| e.to_string())
    }
}
