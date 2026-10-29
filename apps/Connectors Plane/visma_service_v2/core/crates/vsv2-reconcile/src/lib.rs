//! `vsv2-reconcile` — Phase 5 delete reconciliation.
//!
//! Strategy: full ERP REST ID scan → diff against BQ live PKs → soft-delete
//! missing rows via `_deleted_at` timestamp (Python `vsv2-bq-reconcile`).
//!
//! Safety: if ERP returns 0 records the reconciler aborts — a transient API
//! failure must not wipe the entire table.

use std::io::Write as _;

use tempfile::NamedTempFile;
use tokio::process::Command;
use tracing::{info, warn};
use vsv2_common::{Error, Result, TenantId};
use vsv2_erp_rest::{paginate, ErpRestClient, PageOpts};

const COMPOSITE_PK_SEPARATOR: &str = "\u{1f}";

pub struct DeleteReconciler {
    pub client: ErpRestClient,
    pub project: String,
    pub dataset: String,
}

impl DeleteReconciler {
    /// Full ERP REST ID scan for `api_path`, then soft-delete BQ rows absent upstream.
    ///
    /// - `api_path`  — ERP REST endpoint path (e.g. `"controller/api/v1/customer"`)
    /// - `bq_table`  — bronze BigQuery table name (e.g. `"visma_customers"`)
    /// - `pk`        — primary key field name in both ERP records and BQ (e.g. `"number"`)
    pub async fn reconcile_endpoint(
        &self,
        tenant: &TenantId,
        api_path: &str,
        bq_table: &str,
        pk: &str,
    ) -> Result<u64> {
        info!(tenant = %tenant, api = api_path, table = bq_table, "reconcile: full ID scan");

        let records = paginate(
            &self.client,
            tenant,
            api_path,
            &PageOpts {
                page_size: 500,
                ..Default::default()
            },
        )
        .await?;

        if records.is_empty() {
            warn!(
                tenant = %tenant,
                api = api_path,
                "reconcile: ERP returned 0 records — skipping to avoid wiping table"
            );
            return Ok(0);
        }

        info!(tenant = %tenant, n = records.len(), "reconcile: writing ERP IDs to temp file");

        let mut tmp = NamedTempFile::new()
            .map_err(|e| Error::Other(format!("reconcile: create temp file: {e}")))?;

        let pk_parts = parse_pk(pk);
        for r in &records {
            if let Some(id_val) = extract_pk(r, &pk_parts) {
                let line = serde_json::to_string(&id_val)?;
                writeln!(tmp, "{line}").map_err(Error::Io)?;
            }
        }

        let tmp_path = tmp.path().to_owned();

        let output = Command::new("vsv2-bq-reconcile")
            .args([
                "--project",
                &self.project,
                "--dataset",
                &self.dataset,
                "--table",
                bq_table,
                "--pk",
                pk,
                "--tenant-id",
                tenant.as_str(),
                "--erp-ids-file",
            ])
            .arg(&tmp_path)
            .output()
            .await
            .map_err(|e| Error::Other(format!("vsv2-bq-reconcile: spawn failed: {e}")))?;

        drop(tmp);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Other(format!("vsv2-bq-reconcile: {stderr}")));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let v: serde_json::Value = serde_json::from_str(stdout.trim())
            .map_err(|e| Error::Other(format!("reconcile: bad output {e}: {stdout}")))?;

        let deleted = v["deleted"].as_u64().unwrap_or(0);
        info!(tenant = %tenant, table = bq_table, deleted, "reconcile: done");
        Ok(deleted)
    }
}

fn parse_pk(pk: &str) -> Vec<&str> {
    pk.split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect()
}

fn get_path<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    path.split('.')
        .try_fold(value, |current, part| current.get(part))
}

fn stringify_pk_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    }
}

fn extract_pk(record: &serde_json::Value, pk_parts: &[&str]) -> Option<String> {
    if pk_parts.is_empty() {
        return None;
    }

    let values = pk_parts
        .iter()
        .map(|part| get_path(record, part).map(stringify_pk_value))
        .collect::<Option<Vec<_>>>()?;

    Some(values.join(COMPOSITE_PK_SEPARATOR))
}
