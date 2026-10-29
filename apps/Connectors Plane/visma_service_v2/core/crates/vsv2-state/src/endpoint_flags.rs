//! Per-tenant operator override for endpoint source routing.
//!
//! Backed by `vsv2.endpoint_flags` (migration 0005). The registry resolver
//! consumes the result of `get_flag` as the highest-priority override.

use sqlx::PgPool;
use vsv2_common::error::{Error, Result};
use vsv2_common::source::Source;
use vsv2_common::tenant::TenantId;

/// One row in `vsv2.endpoint_flags`.
#[derive(Debug, Clone)]
pub struct EndpointFlag {
    pub tenant_id: String,
    pub endpoint: String,
    pub source: Source,
    pub enabled: bool,
}

fn parse_source(s: &str) -> Result<Source> {
    match s {
        "erp_rest" => Ok(Source::ErpRest),
        "datamart_graphql" => Ok(Source::DatamartGraphql),
        "expense_api" => Ok(Source::ExpenseApi),
        other => Err(Error::StateStore(format!(
            "unknown source in endpoint_flags: {other}"
        ))),
    }
}

/// Read the active flag for `(tenant, endpoint)`.
/// Returns `Ok(Some(source))` only when a row exists AND `enabled = true`.
pub async fn get_flag(pool: &PgPool, tenant: &TenantId, endpoint: &str) -> Result<Option<Source>> {
    let row: Option<(String, bool)> = sqlx::query_as(
        "SELECT source, enabled \
           FROM vsv2.endpoint_flags \
          WHERE tenant_id = $1 AND endpoint = $2",
    )
    .bind(tenant.as_str())
    .bind(endpoint)
    .fetch_optional(pool)
    .await
    .map_err(|e| Error::StateStore(e.to_string()))?;

    match row {
        Some((src, true)) => Ok(Some(parse_source(&src)?)),
        _ => Ok(None),
    }
}

/// Upsert a flag. Set `enabled=false` to disable an override without deleting it.
pub async fn set_flag(
    pool: &PgPool,
    tenant: &TenantId,
    endpoint: &str,
    source: Source,
    enabled: bool,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO vsv2.endpoint_flags (tenant_id, endpoint, source, enabled, updated_at) \
         VALUES ($1, $2, $3, $4, NOW()) \
         ON CONFLICT (tenant_id, endpoint) DO UPDATE \
            SET source = EXCLUDED.source, \
                enabled = EXCLUDED.enabled, \
                updated_at = NOW()",
    )
    .bind(tenant.as_str())
    .bind(endpoint)
    .bind(source.as_str())
    .bind(enabled)
    .execute(pool)
    .await
    .map_err(|e| Error::StateStore(e.to_string()))?;
    Ok(())
}

/// List every flag for `tenant` (enabled + disabled).
pub async fn list_flags(pool: &PgPool, tenant: &TenantId) -> Result<Vec<EndpointFlag>> {
    let rows: Vec<(String, String, String, bool)> = sqlx::query_as(
        "SELECT tenant_id, endpoint, source, enabled \
           FROM vsv2.endpoint_flags \
          WHERE tenant_id = $1 \
          ORDER BY endpoint",
    )
    .bind(tenant.as_str())
    .fetch_all(pool)
    .await
    .map_err(|e| Error::StateStore(e.to_string()))?;

    rows.into_iter()
        .map(|(tenant_id, endpoint, source, enabled)| {
            Ok(EndpointFlag {
                tenant_id,
                endpoint,
                source: parse_source(&source)?,
                enabled,
            })
        })
        .collect()
}
