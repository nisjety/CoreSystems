//! Accounting attachment metadata command.

use std::{collections::HashSet, sync::Arc};

use anyhow::{bail, Context};
use chrono::Utc;
use clap::Args;
use uuid::Uuid;
use xero_client::attachments::supports_attachments;
use xero_common::{EntityType, SyncConfig};
use xero_gcs::write_manifest;
use xero_sync::{AttachmentDiscoveryOptions, SyncJob};

const DEFAULT_MAX_SCAN_PAGES: u32 = 20;
const DEFAULT_MAX_PARENTS: usize = 100;

use crate::resolve::resolve_tenants;
use crate::sync::{
    build_custom_auth, build_pkce_auth, gcs_config, manifest_has_failures, raw_sink,
};

#[derive(Debug, Args)]
pub struct AttachmentArgs {
    /// Accounting parent type supporting Xero attachments.
    #[arg(long)]
    pub entity: EntityType,

    /// Exact parent UUIDs; repeat or comma-separate.
    #[arg(
        long,
        required_unless_present = "discover",
        conflicts_with = "discover",
        value_delimiter = ','
    )]
    pub parent_id: Vec<Uuid>,

    /// Scan a bounded parent collection and look up attachment metadata.
    #[arg(long, conflicts_with = "parent_id")]
    pub discover: bool,

    /// Maximum parent-list pages to scan; the run fails if no empty page is reached.
    #[arg(long, requires = "discover")]
    pub max_scan_pages: Option<u32>,

    /// Maximum distinct selected parents to look up.
    #[arg(long, requires = "discover")]
    pub max_parents: Option<usize>,

    /// Recheck every scanned parent; required for Quotes, which omit HasAttachments.
    #[arg(long, requires = "discover")]
    pub include_unflagged: bool,

    /// Restrict to configured tenant IDs; default is all.
    #[arg(long, value_delimiter = ',')]
    pub tenant: Vec<String>,

    /// Write raw JSON and sidecars to local disk instead of GCS.
    #[arg(long)]
    pub dry_run: bool,

    /// Local output root for --dry-run.
    #[arg(long, default_value = "./out")]
    pub local_dir: String,
}

fn validate(args: &AttachmentArgs) -> anyhow::Result<()> {
    if !supports_attachments(&args.entity) {
        bail!("{} does not support Accounting attachments", args.entity);
    }
    if args.discover {
        if !args.parent_id.is_empty() {
            bail!("--discover cannot be combined with --parent-id");
        }
        return discovery_options(args)
            .validate(&args.entity)
            .map_err(anyhow::Error::msg);
    }
    if args.parent_id.is_empty() {
        bail!("at least one --parent-id is required");
    }
    let distinct: HashSet<Uuid> = args.parent_id.iter().copied().collect();
    if distinct.len() != args.parent_id.len() {
        bail!("duplicate --parent-id values are not allowed");
    }
    Ok(())
}

fn discovery_options(args: &AttachmentArgs) -> AttachmentDiscoveryOptions {
    AttachmentDiscoveryOptions {
        max_scan_pages: args.max_scan_pages.unwrap_or(DEFAULT_MAX_SCAN_PAGES),
        max_parents: args.max_parents.unwrap_or(DEFAULT_MAX_PARENTS),
        include_unflagged: args.include_unflagged,
    }
}

pub async fn run(args: AttachmentArgs) -> anyhow::Result<()> {
    validate(&args)?;
    let auth_mode = std::env::var("XERO_AUTH_MODE")
        .unwrap_or_else(|_| "custom".into())
        .to_ascii_lowercase();
    let (auth, configured, org_names) = match auth_mode.as_str() {
        "pkce" => build_pkce_auth().await?,
        "custom" | "cc" | "client_credentials" | "" => build_custom_auth()?,
        other => bail!("unsupported XERO_AUTH_MODE: {other}"),
    };
    let tenants = resolve_tenants(&args.tenant, &configured)?;
    let gcs = gcs_config(args.dry_run)?;
    if gcs.prefix != "raw/xero" {
        bail!("sync-attachments requires GCS_PREFIX=raw/xero for the warehouse loader");
    }
    let sink = raw_sink(args.dry_run, &args.local_dir, &gcs).await?;
    let job = SyncJob::new(
        auth,
        Arc::clone(&sink),
        gcs,
        SyncConfig::from_env().context("sync config error")?,
        org_names,
        auth_mode == "pkce",
    );
    let run_id = Uuid::new_v4();
    let run_date = Utc::now().date_naive();
    let manifest = if args.discover {
        job.run_discovered_attachments(
            run_id,
            run_date,
            &tenants,
            &args.entity,
            discovery_options(&args),
        )
        .await
    } else {
        job.run_attachments(run_id, run_date, &tenants, &args.entity, &args.parent_id)
            .await
    };
    write_manifest(sink.as_ref(), run_date, run_id, &manifest)
        .await
        .context("failed to write attachment metadata manifest")?;
    tracing::info!(run_id = %run_id, parents = manifest.entities.len(), "attachment metadata sync finished");
    if manifest_has_failures(&manifest) {
        bail!("attachment metadata sync had failed requests; see run manifest {run_id}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        args: AttachmentArgs,
    }

    #[test]
    fn requires_parent_ids_and_rejects_unsupported_entity() {
        assert!(TestCli::try_parse_from(["x", "--entity", "invoices"]).is_err());
        let args = TestCli::try_parse_from([
            "x",
            "--entity",
            "report_profit_and_loss",
            "--parent-id",
            "5f3c9a2e-0000-0000-0000-000000000000",
        ])
        .unwrap()
        .args;
        assert!(validate(&args).is_err());
    }

    #[test]
    fn discovery_is_explicit_and_bounded() {
        let args = TestCli::try_parse_from([
            "x",
            "--entity",
            "invoices",
            "--discover",
            "--max-scan-pages",
            "4",
            "--max-parents",
            "25",
            "--include-unflagged",
        ])
        .unwrap()
        .args;
        assert!(validate(&args).is_ok());
        assert!(discovery_options(&args).include_unflagged);
        assert!(TestCli::try_parse_from([
            "x",
            "--entity",
            "invoices",
            "--discover",
            "--parent-id",
            "5f3c9a2e-0000-0000-0000-000000000000",
        ])
        .is_err());
        let valid = TestCli::try_parse_from(["x", "--entity", "accounts", "--discover"])
            .unwrap()
            .args;
        assert!(validate(&valid).is_ok());
    }
}
