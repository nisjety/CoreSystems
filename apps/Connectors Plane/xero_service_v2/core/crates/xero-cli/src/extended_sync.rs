//! Explicit, opt-in sync command for non-Accounting Xero API families.

use std::sync::Arc;

use anyhow::{bail, Context};
use chrono::{NaiveDate, Utc};
use clap::Args;
use uuid::Uuid;
use xero_gcs::write_manifest;
use xero_sync::extended::{ExtendedFamily, ExtendedOptions, ExtendedSyncJob};

use crate::resolve::resolve_tenants;
use crate::sync::{
    build_custom_auth, build_pkce_auth, gcs_config, manifest_has_failures, raw_sink,
};

#[derive(Debug, Args)]
pub struct ExtendedArgs {
    /// API families to read. Repeat or comma-separate: assets, projects,
    /// bankfeeds, identity, finance. Nothing runs by default.
    #[arg(long, required = true, value_delimiter = ',')]
    pub family: Vec<ExtendedFamily>,

    /// Restrict to configured tenant IDs; default is all.
    #[arg(long, value_delimiter = ',')]
    pub tenant: Vec<String>,

    /// Start of a Finance snapshot period (YYYY-MM-DD).
    #[arg(long)]
    pub from: Option<NaiveDate>,

    /// End of a Finance snapshot period (YYYY-MM-DD).
    #[arg(long)]
    pub to: Option<NaiveDate>,

    /// Bank account UUID for Finance BankStatementsPlus; repeat per account.
    #[arg(long = "bank-account-id")]
    pub bank_account_ids: Vec<Uuid>,

    /// Skip per-ID detail calls (faster, but some nested fields may be absent).
    #[arg(long)]
    pub no_details: bool,

    /// Write raw bodies and sidecars to local disk instead of GCS.
    #[arg(long)]
    pub dry_run: bool,

    /// Local output root for --dry-run.
    #[arg(long, default_value = "./out")]
    pub local_dir: String,
}

pub async fn run(args: ExtendedArgs) -> anyhow::Result<()> {
    let options = ExtendedOptions {
        from: args.from,
        to: args.to,
        bank_account_ids: args.bank_account_ids,
        details: !args.no_details,
    };
    options.validate(&args.family).map_err(anyhow::Error::msg)?;

    let auth_mode = std::env::var("XERO_AUTH_MODE")
        .unwrap_or_else(|_| "custom".into())
        .to_ascii_lowercase();
    let (auth, configured, org_names) = match auth_mode.as_str() {
        "pkce" => build_pkce_auth().await?,
        "custom" | "cc" | "client_credentials" | "" => build_custom_auth()?,
        other => bail!("unsupported XERO_AUTH_MODE: {other}"),
    };
    let tenants = resolve_tenants(&args.tenant, &configured)?;
    if auth_mode == "pkce"
        && args.family.contains(&ExtendedFamily::Identity)
        && !args.tenant.is_empty()
    {
        bail!("PKCE Identity /connections is app-wide; omit --tenant and use the restricted application dataset");
    }
    let gcs = gcs_config(args.dry_run)?;
    validate_warehouse_prefix(&gcs.prefix)?;
    if !args.dry_run
        && args.family.iter().any(|f| {
            matches!(
                f,
                ExtendedFamily::BankFeeds | ExtendedFamily::Finance | ExtendedFamily::Identity
            )
        })
    {
        let restricted_bucket = std::env::var("XERO_SENSITIVE_GCS_BUCKET").context(
            "XERO_SENSITIVE_GCS_BUCKET is required for Bank Feeds, Finance and Identity",
        )?;
        if gcs.bucket != restricted_bucket {
            bail!("GCS_BUCKET must equal XERO_SENSITIVE_GCS_BUCKET for sensitive API families");
        }
    }
    let sink = raw_sink(args.dry_run, &args.local_dir, &gcs).await?;
    let job = ExtendedSyncJob::new(auth, Arc::clone(&sink), gcs, org_names, auth_mode == "pkce");
    let run_id = Uuid::new_v4();
    let run_date = Utc::now().date_naive();
    let manifest = job
        .run(run_id, run_date, &tenants, &args.family, &options)
        .await;
    write_manifest(sink.as_ref(), run_date, run_id, &manifest)
        .await
        .context("failed to write extended sync manifest")?;

    let failed = manifest
        .entities
        .iter()
        .filter(|e| e.error.is_some())
        .count();
    tracing::info!(run_id = %run_id, requests = manifest.entities.len(), failed, "extended sync finished");
    if manifest_has_failures(&manifest) {
        bail!("extended sync had failed/truncated requests; see run manifest {run_id}");
    }
    Ok(())
}

fn validate_warehouse_prefix(prefix: &str) -> anyhow::Result<()> {
    if prefix != "raw/xero" {
        bail!("sync-extended requires GCS_PREFIX=raw/xero for the warehouse loader");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_warehouse_prefix;

    #[test]
    fn extended_sync_rejects_prefixes_the_loader_cannot_read() {
        assert!(validate_warehouse_prefix("raw/xero").is_ok());
        assert!(validate_warehouse_prefix("other/xero").is_err());
        assert!(validate_warehouse_prefix("/raw/xero").is_err());
    }
}
