//! Dedicated, bounded Xero aged receivables/payables report ingestion.

use std::sync::Arc;

use anyhow::{bail, Context};
use chrono::Utc;
use clap::Args;
use uuid::Uuid;
use xero_common::{EntityType, SyncConfig};
use xero_gcs::write_manifest;
use xero_sync::{AgedContactReportOptions, SyncJob};

use crate::resolve::resolve_tenants;
use crate::sync::{
    build_custom_auth, build_pkce_auth, gcs_config, manifest_has_failures, raw_sink,
};

#[derive(Debug, Args)]
pub struct AgedContactReportArgs {
    /// Restrict to configured tenant IDs; default is all configured tenants.
    #[arg(long, value_delimiter = ',')]
    pub tenant: Vec<String>,

    /// Aged report(s) to fetch. Empty selects both receivables and payables.
    /// Only the two aged-by-contact reports are accepted.
    #[arg(long, value_delimiter = ',')]
    pub report: Vec<EntityType>,

    /// As-of date for the aged report snapshot. Defaults to today's UTC date.
    #[arg(long)]
    pub as_of: Option<chrono::NaiveDate>,

    /// Hard cap on Contacts discovered for one tenant before any aged calls run.
    #[arg(long, default_value_t = 1_000)]
    pub max_contacts_per_tenant: usize,

    /// Hard cap on aged report calls across this whole command invocation.
    #[arg(long, default_value_t = 2_000)]
    pub max_report_requests: usize,

    /// Write raw output to local disk instead of GCS.
    #[arg(long)]
    pub dry_run: bool,

    /// Local output root for --dry-run.
    #[arg(long, default_value = "./out")]
    pub local_dir: String,
}

pub async fn run(args: AgedContactReportArgs) -> anyhow::Result<()> {
    let run_id = Uuid::new_v4();
    let run_date = Utc::now().date_naive();
    let as_of = args.as_of.unwrap_or(run_date);
    let options = AgedContactReportOptions {
        max_contacts_per_tenant: args.max_contacts_per_tenant,
        max_report_requests: args.max_report_requests,
    };
    options.validate().map_err(anyhow::Error::msg)?;
    let reports = if args.report.is_empty() {
        vec![
            EntityType::ReportAgedReceivablesByContact,
            EntityType::ReportAgedPayablesByContact,
        ]
    } else {
        args.report
    };

    let auth_mode = std::env::var("XERO_AUTH_MODE")
        .unwrap_or_else(|_| "custom".into())
        .to_ascii_lowercase();
    let (auth, configured, org_names) = match auth_mode.as_str() {
        "pkce" => build_pkce_auth().await?,
        "custom" | "cc" | "client_credentials" | "" => build_custom_auth()?,
        other => bail!("unsupported XERO_AUTH_MODE: {other} (use 'pkce' or 'custom')"),
    };
    let tenants = resolve_tenants(&args.tenant, &configured)?;
    let gcs = gcs_config(args.dry_run)?;
    anyhow::ensure!(
        gcs.prefix == "raw/xero",
        "aged contact report loader requires GCS_PREFIX=raw/xero"
    );
    let sink = raw_sink(args.dry_run, &args.local_dir, &gcs).await?;
    let sync_cfg = SyncConfig::from_env().context("sync config error")?;
    let job = SyncJob::new(
        auth,
        Arc::clone(&sink),
        gcs,
        sync_cfg,
        org_names,
        auth_mode == "pkce",
    );
    let manifest = job
        .run_aged_contact_reports(run_id, run_date, as_of, &tenants, &reports, options)
        .await
        .map_err(anyhow::Error::msg)?;
    write_manifest(sink.as_ref(), run_date, run_id, &manifest)
        .await
        .context("failed to write aged contact report manifest")?;
    tracing::info!(
        run_id = %run_id,
        tenants = tenants.len(),
        reports = reports.len(),
        outcomes = manifest.entities.len(),
        "aged contact report sync completed"
    );
    if manifest_has_failures(&manifest) {
        bail!(
            "aged contact report sync completed with failures or incomplete contact discovery; see run manifest {run_id}"
        );
    }
    Ok(())
}
