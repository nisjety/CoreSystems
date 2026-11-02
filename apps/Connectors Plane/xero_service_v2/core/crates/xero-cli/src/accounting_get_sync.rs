//! Direct, catalogued read-only Accounting GET landing path.

use std::sync::Arc;

use anyhow::{bail, Context};
use chrono::Utc;
use clap::Args;
use uuid::Uuid;
use xero_client::accounting_get::operation;
use xero_gcs::write_manifest;
use xero_sync::{accounting_get::AccountingGetSyncJob, is_actionable_failure};

use crate::resolve::resolve_tenants;
use crate::sync::{build_custom_auth, build_pkce_auth, gcs_config, raw_sink};

#[derive(Debug, Args)]
pub struct AccountingGetArgs {
    /// OpenAPI operation ID, such as getInvoiceHistory or getInvoiceAsPdf.
    #[arg(long)]
    pub operation: String,

    /// Required path value NAME=VALUE; repeat for multiple path parameters.
    #[arg(long = "path-param")]
    pub path_params: Vec<String>,

    /// Catalogued query value NAME=VALUE; repeat as needed.
    #[arg(long)]
    pub query: Vec<String>,

    /// Catalogued header NAME=VALUE (for example contentType for attachments).
    #[arg(long)]
    pub header: Vec<String>,

    /// Restrict to configured tenant IDs; default is all.
    #[arg(long, value_delimiter = ',')]
    pub tenant: Vec<String>,

    /// Write raw output to local disk instead of GCS.
    #[arg(long)]
    pub dry_run: bool,

    /// Local output root for --dry-run.
    #[arg(long, default_value = "./out")]
    pub local_dir: String,
}

fn pairs(values: &[String]) -> anyhow::Result<Vec<(String, String)>> {
    values
        .iter()
        .map(|value| {
            let (name, value) = value
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("parameter must be NAME=VALUE"))?;
            anyhow::ensure!(
                !name.is_empty() && !value.is_empty(),
                "parameter must be NAME=VALUE"
            );
            Ok((name.to_owned(), value.to_owned()))
        })
        .collect()
}

pub async fn run(args: AccountingGetArgs) -> anyhow::Result<()> {
    let selected = operation(&args.operation).map_err(anyhow::Error::msg)?;
    let path = pairs(&args.path_params)?;
    let query = pairs(&args.query)?;
    let headers = pairs(&args.header)?;

    let auth_mode = std::env::var("XERO_AUTH_MODE")
        .unwrap_or_else(|_| "custom".into())
        .to_ascii_lowercase();
    let (auth, configured, org_names) = match auth_mode.as_str() {
        "pkce" => build_pkce_auth().await?,
        "custom" | "cc" | "client_credentials" | "" => build_custom_auth()?,
        other => bail!("unsupported XERO_AUTH_MODE: {other}"),
    };
    let tenants = resolve_tenants(&args.tenant, &configured)?;
    let mut gcs = gcs_config(args.dry_run)?;
    anyhow::ensure!(
        gcs.prefix == "raw/xero",
        "Accounting GET warehouse loader requires GCS_PREFIX=raw/xero"
    );
    if selected.is_binary() && !args.dry_run {
        let bucket = std::env::var("XERO_DOCUMENT_GCS_BUCKET")
            .context("XERO_DOCUMENT_GCS_BUCKET is required for PDF and attachment GETs")?;
        anyhow::ensure!(
            !bucket.trim().is_empty() && bucket != gcs.bucket,
            "document bucket must be nonempty and separate from GCS_BUCKET"
        );
        gcs.bucket = bucket;
    }
    let sink = raw_sink(args.dry_run, &args.local_dir, &gcs).await?;
    let run_id = Uuid::new_v4();
    let date = Utc::now().date_naive();
    let job =
        AccountingGetSyncJob::new(auth, Arc::clone(&sink), gcs, org_names, auth_mode == "pkce");
    let manifest = job
        .run(
            run_id,
            date,
            &tenants,
            &args.operation,
            &path,
            &query,
            &headers,
        )
        .await?;
    write_manifest(sink.as_ref(), date, run_id, &manifest).await?;
    if manifest.entities.iter().any(is_actionable_failure) {
        bail!("Accounting GET had failed tenants; see run manifest {run_id}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_pairs_require_name_equals_value() {
        assert!(pairs(&["missing-equals".into()]).is_err());
        assert_eq!(
            pairs(&["page=2".into()]).unwrap(),
            vec![("page".into(), "2".into())]
        );
    }
}
