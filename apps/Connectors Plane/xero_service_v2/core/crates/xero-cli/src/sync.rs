//! The `sync` subcommand: flags → [`SyncMode`] → [`SyncJob::run`] → manifest.
//!
//! This module owns the flag surface ([`SyncArgs`]) and the run orchestration;
//! the pure flag → run-parameter mapping lives in [`crate::resolve`]. Backfill
//! is special-cased: it expands the requested range into business-date chunks
//! (oldest→newest) and runs the job once per chunk, merging the per-chunk
//! manifests into one.

use std::num::NonZeroUsize;
use std::sync::Arc;

use anyhow::Context;
use chrono::{NaiveDate, Utc};
use clap::Args;
use uuid::Uuid;

use xero_common::{AppConfig, EntityType, GcsConfig, SyncConfig};
use xero_gcs::{write_manifest, GcsRawSink, LocalDirSink, RawSink, RunManifest};
use xero_sync::{is_actionable_failure, ReportOptions, SyncJob, SyncMode};

use crate::resolve::{
    backfill_chunks, parse_range, resolve_entities, resolve_mode, resolve_tenants, BackfillScope,
    ModeScope,
};

/// Flags for `xero sync`. The active [`SyncMode`] is derived from these via
/// [`crate::resolve::resolve_mode`]; see the precedence on the `Sync` command doc.
#[derive(Debug, Args)]
pub struct SyncArgs {
    /// Incremental look-back window in days (modified ≥ now − N). Overrides
    /// `SYNC_WINDOW_DAYS` for this run.
    #[arg(long)]
    pub window_days: Option<i64>,

    /// Raw Xero `where=` clause (open-sweep). Implies `--no-window`.
    #[arg(long = "where")]
    pub where_clause: Option<String>,

    /// Drop the modified window without a `--where` clause (rare; pairs with
    /// `--where`, but allowed standalone for an unfiltered open-sweep).
    #[arg(long)]
    pub no_window: bool,

    /// Rolling-full business window lower bound (`YYYY-MM-DD`). Pairs with
    /// `--business-to`.
    #[arg(long)]
    pub business_from: Option<NaiveDate>,

    /// Rolling-full business window upper bound (`YYYY-MM-DD`). Defaults to the
    /// run date when only `--business-from` is given.
    #[arg(long)]
    pub business_to: Option<NaiveDate>,

    /// Master-data full refresh (no filter) for master entities.
    #[arg(long)]
    pub full: bool,

    /// Rolling-full over the configured tight window when no explicit business
    /// dates are given (transactional entities).
    #[arg(long)]
    pub full_with_window: bool,

    /// One-time historical backfill over `FROM:TO` (`YYYY-MM-DD:YYYY-MM-DD`),
    /// expanded into business-date chunks.
    #[arg(long, value_name = "FROM:TO")]
    pub backfill: Option<String>,

    /// Backfill chunk size in months (default 1).
    #[arg(long, default_value_t = 1)]
    pub chunk_months: u32,

    /// Report-snapshot mode. Requires `--as-of`.
    #[arg(long)]
    pub reports: bool,

    /// As-of date for `--reports` (`YYYY-MM-DD`). Defaults to the run date.
    #[arg(long)]
    pub as_of: Option<NaiveDate>,

    /// Explicit start date for ProfitAndLoss or BankSummary reports.
    #[arg(long, requires = "reports")]
    pub report_from: Option<NaiveDate>,

    /// Explicit end date for ProfitAndLoss or BankSummary reports.
    #[arg(long, requires = "reports")]
    pub report_to: Option<NaiveDate>,

    /// Comparison periods (1..=12) for ProfitAndLoss or BalanceSheet.
    #[arg(long, requires = "reports")]
    pub report_periods: Option<u8>,

    /// Restrict to a CSV of entity keys (e.g. `invoices,payments`). Empty →
    /// the mode's default entity set.
    #[arg(long, value_delimiter = ',')]
    pub entity: Vec<EntityType>,

    /// Restrict to a CSV of tenant ids. Empty → all configured tenants.
    #[arg(long, value_delimiter = ',')]
    pub tenant: Vec<String>,

    /// Write to local disk (`--local-dir`) instead of GCS.
    #[arg(long)]
    pub dry_run: bool,

    /// Local output root for `--dry-run` (default `./out`).
    #[arg(long, default_value = "./out")]
    pub local_dir: String,

    /// Max concurrent (tenant, entity) tasks. Clamped to 1..=6 internally.
    #[arg(long)]
    pub max_concurrent: Option<usize>,

    /// Fetch UUID details for Accounting list records (extra API calls).
    #[arg(long)]
    pub expand_details: bool,

    /// Stop an entity if detail fan-out exceeds this many records.
    #[arg(long, default_value = "1000")]
    pub max_detail_records: NonZeroUsize,
}

/// The auth collaborators a sync run needs: a token provider, the list of
/// tenants it can serve, and their display names for the `x-org-name` tag.
pub(crate) type AuthBundle = (
    Arc<dyn xero_auth::TokenProvider>,
    Vec<String>,
    std::collections::HashMap<String, String>,
);

/// Legacy custom-connection auth from `XERO_CC_*` / `XERO_ORG_N_*`.
pub(crate) fn build_custom_auth() -> anyhow::Result<AuthBundle> {
    let cfg = AppConfig::from_env().context("config error — check your .env file")?;
    let auth: Arc<dyn xero_auth::TokenProvider> = Arc::new(
        xero_auth::MultiTenantCustomConnectionClient::new(cfg.xero_cc_connections.clone()),
    );
    let configured: Vec<String> = cfg
        .xero_cc_connections
        .iter()
        .map(|c| c.tenant_id.clone())
        .collect();
    let org_names = cfg
        .xero_cc_connections
        .iter()
        .filter_map(|c| c.tenant_name.clone().map(|n| (c.tenant_id.clone(), n)))
        .collect();
    Ok((auth, configured, org_names))
}

/// PKCE auth: refresh tokens from the onboarding connection store + the Xero app
/// credentials. Tenants and display names come from the stored connections.
pub(crate) async fn build_pkce_auth() -> anyhow::Result<AuthBundle> {
    let client_id = std::env::var("XERO_PKCE_CLIENT_ID")
        .context("XERO_PKCE_CLIENT_ID must be set for XERO_AUTH_MODE=pkce")?;
    let client_secret = std::env::var("XERO_PKCE_CLIENT_SECRET")
        .ok()
        .filter(|s| !s.is_empty());
    let store: Arc<dyn xero_store::ConnectionStore> = xero_store::open_from_env()
        .await
        .context("open PKCE connection store")?;
    let conns = store.list().await.context("read connection store")?;
    if conns.is_empty() {
        anyhow::bail!(
            "no onboarded Xero connections — connect an org via the onboarding app first"
        );
    }
    let configured: Vec<String> = conns.iter().map(|c| c.tenant_id.clone()).collect();
    let org_names = conns
        .iter()
        .map(|c| (c.tenant_id.clone(), c.tenant_name.clone()))
        .collect();
    let auth: Arc<dyn xero_auth::TokenProvider> = Arc::new(xero_auth::PkceTokenProvider::new(
        client_id,
        client_secret,
        Arc::clone(&store),
    ));
    Ok((auth, configured, org_names))
}

/// Execute the `sync` subcommand end-to-end.
pub async fn run(args: SyncArgs) -> anyhow::Result<()> {
    // Apply CLI overrides on top of the env-derived sync config. `--window-days`
    // controls the incremental modified window the SyncJob computes.
    let sync_cfg = apply_overrides(SyncConfig::from_env().context("sync config error")?, &args);

    let run_id = Uuid::new_v4();
    let run_date = Utc::now().date_naive();
    let max_concurrent = args.max_concurrent.unwrap_or(sync_cfg.max_concurrent);
    let report_options = ReportOptions {
        from: args.report_from,
        to: args.report_to,
        periods: args.report_periods,
    };
    if args.backfill.is_some() && report_options != ReportOptions::default() {
        anyhow::bail!("report overrides cannot be combined with --backfill");
    }

    // Auth source: `pkce` uses the refresh tokens captured by the onboarding app
    // (one Xero app, all connected tenants); `custom` uses the legacy per-org
    // client_credentials creds. Selected by XERO_AUTH_MODE (default `custom`).
    let auth_mode = std::env::var("XERO_AUTH_MODE")
        .unwrap_or_else(|_| "custom".into())
        .to_ascii_lowercase();
    let (auth, configured, org_names) = match auth_mode.as_str() {
        "pkce" => build_pkce_auth().await?,
        "custom" | "cc" | "client_credentials" | "" => build_custom_auth()?,
        other => anyhow::bail!("unsupported XERO_AUTH_MODE: {other} (use 'pkce' or 'custom')"),
    };
    let tenants = resolve_tenants(&args.tenant, &configured)?;

    // GcsConfig carries the bucket (real sink) and prefix (object keys). The
    // bucket is only required for a real upload; dry-run still needs the prefix
    // so keys match the production layout, so synthesize a config when unset.
    let gcs_cfg = resolve_gcs_config(&args)?;
    let sink = build_sink(&args, &gcs_cfg).await?;

    // PKCE tokens cover multiple tenants → the Xero-tenant-id header is
    // required; single-tenant custom-connection tokens do not send it.
    let send_tenant_header = auth_mode == "pkce";
    let mut job = SyncJob::new(
        auth,
        Arc::clone(&sink),
        gcs_cfg,
        sync_cfg.clone(),
        org_names,
        send_tenant_header,
    )
    .with_report_options(report_options);
    if args.expand_details {
        job = job.with_detail_expansion(args.max_detail_records.get());
    }

    // Backfill runs N chunked sub-runs; everything else is a single run.
    let manifest = if let Some(spec) = args.backfill.as_deref() {
        let entities = resolve_entities(&args, BackfillScope)?;
        run_backfill(
            &job,
            run_id,
            spec,
            args.chunk_months,
            &tenants,
            &entities,
            max_concurrent,
        )
        .await?
    } else {
        let (mode, anchor_date) = resolve_mode(&args, run_date, &sync_cfg)?;
        let entities = resolve_entities(&args, ModeScope(&mode))?;
        if entities.iter().any(|entity| {
            matches!(
                entity,
                EntityType::ReportAgedReceivablesByContact
                    | EntityType::ReportAgedPayablesByContact
            )
        }) {
            anyhow::bail!(
                "aged-by-contact reports require `xero sync-aged-contact-reports`; the dedicated command discovers contacts per tenant and enforces request budgets"
            );
        }
        if args.reports {
            report_options
                .validate(&entities)
                .map_err(anyhow::Error::msg)?;
        }
        job.run(
            run_id,
            anchor_date,
            &tenants,
            &entities,
            mode,
            max_concurrent,
        )
        .await
    };

    write_manifest(sink.as_ref(), run_date, run_id, &manifest)
        .await
        .context("failed to write run manifest")?;

    report_summary(&manifest);
    if manifest_has_failures(&manifest) {
        anyhow::bail!(
            "Xero sync completed with failed or truncated entities; see run manifest {}",
            run_id
        );
    }
    Ok(())
}

pub(crate) fn manifest_has_failures(manifest: &RunManifest) -> bool {
    manifest.entities.iter().any(is_actionable_failure)
}

/// Apply CLI overrides onto the env-derived [`SyncConfig`].
fn apply_overrides(mut cfg: SyncConfig, args: &SyncArgs) -> SyncConfig {
    if let Some(window_days) = args.window_days {
        cfg.window_days = window_days;
    }
    if let Some(max_concurrent) = args.max_concurrent {
        cfg.max_concurrent = max_concurrent;
    }
    cfg
}

/// Resolve the [`GcsConfig`]. A real run requires `GCS_BUCKET`; a `--dry-run`
/// tolerates its absence (the bucket is unused) but still honours `GCS_PREFIX`
/// so the local key layout matches production.
fn resolve_gcs_config(args: &SyncArgs) -> anyhow::Result<GcsConfig> {
    gcs_config(args.dry_run)
}

pub(crate) fn gcs_config(dry_run: bool) -> anyhow::Result<GcsConfig> {
    match GcsConfig::from_env() {
        Ok(cfg) => Ok(cfg),
        Err(e) if dry_run => {
            tracing::debug!(error = %e, "dry-run: GCS_BUCKET unset, using default prefix");
            let prefix = std::env::var("GCS_PREFIX").unwrap_or_else(|_| "raw/xero".to_string());
            Ok(GcsConfig {
                bucket: String::new(),
                prefix,
            })
        }
        Err(e) => Err(e).context("GCS config error (set GCS_BUCKET)"),
    }
}

/// Build the destination sink: local disk for `--dry-run`, else GCS.
async fn build_sink(args: &SyncArgs, gcs_cfg: &GcsConfig) -> anyhow::Result<Arc<dyn RawSink>> {
    raw_sink(args.dry_run, &args.local_dir, gcs_cfg).await
}

pub(crate) async fn raw_sink(
    dry_run: bool,
    local_dir: &str,
    gcs_cfg: &GcsConfig,
) -> anyhow::Result<Arc<dyn RawSink>> {
    // Validate before GCS authentication or any destination construction.
    let generation = match std::env::var("XERO_INGESTION_GENERATION") {
        Ok(value) => Some(
            Uuid::parse_str(&value)
                .context("XERO_INGESTION_GENERATION must be a valid UUID")?
                .to_string(),
        ),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            anyhow::bail!("XERO_INGESTION_GENERATION must be a valid Unicode UUID");
        }
    };
    let namespace = match std::env::var("XERO_INGESTION_NAMESPACE") {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => "v2".to_string(),
        Err(std::env::VarError::NotUnicode(_)) => {
            anyhow::bail!("XERO_INGESTION_NAMESPACE must be valid Unicode");
        }
    };
    if !matches!(namespace.as_str(), "v2" | "main") {
        anyhow::bail!("XERO_INGESTION_NAMESPACE must be v2 or main");
    }
    if namespace == "main" && generation.is_none() {
        anyhow::bail!("XERO_INGESTION_NAMESPACE=main requires XERO_INGESTION_GENERATION");
    }
    let sink: Arc<dyn RawSink> = if dry_run {
        tracing::info!(local_dir, "dry-run: writing to local disk");
        Arc::new(LocalDirSink::new(local_dir))
    } else {
        let sink = GcsRawSink::new(gcs_cfg.bucket.clone())
            .await
            .context("failed to build GCS sink — check GOOGLE_APPLICATION_CREDENTIALS")?;
        tracing::info!(bucket = gcs_cfg.bucket.as_str(), "writing to GCS bucket");
        Arc::new(sink)
    };
    match generation {
        Some(generation) if namespace == "main" => Ok(Arc::new(
            xero_gcs::GenerationSink::new_main(sink, &generation)?,
        )),
        Some(generation) => Ok(Arc::new(xero_gcs::GenerationSink::new(sink, &generation)?)),
        None => Ok(sink),
    }
}

#[cfg(test)]
mod raw_sink_tests {
    use super::*;
    use std::ffi::OsString;
    use std::process::Command;
    use xero_gcs::{sidecar_key, ObjectMeta};

    #[test]
    fn ingestion_generation_environment_cases_are_isolated() {
        let cases = [
            ("absent", None),
            (
                "valid",
                Some(OsString::from("550E8400-E29B-41D4-A716-446655440000")),
            ),
            (
                "main",
                Some(OsString::from("550e8400-e29b-41d4-a716-446655440000")),
            ),
            ("empty", Some(OsString::from(""))),
            ("invalid", Some(OsString::from("not-a-uuid"))),
        ];
        for (case, generation) in cases {
            check_environment_case(case, generation);
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            check_environment_case("non-unicode", Some(OsString::from_vec(vec![0xff])));
            check_environment_case("non-unicode-namespace", None);
        }
    }

    fn check_environment_case(case: &str, generation: Option<OsString>) {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "sync::raw_sink_tests::environment_case",
                "--nocapture",
            ])
            .env("XERO_GENERATION_SINK_TEST_CASE", case)
            .env_remove("XERO_INGESTION_GENERATION")
            .env_remove("XERO_INGESTION_NAMESPACE");
        if case == "main" {
            command.env("XERO_INGESTION_NAMESPACE", "main");
        }
        #[cfg(unix)]
        if case == "non-unicode-namespace" {
            use std::os::unix::ffi::OsStringExt;
            command.env("XERO_INGESTION_NAMESPACE", OsString::from_vec(vec![0xff]));
        }
        if let Some(generation) = generation {
            command.env("XERO_INGESTION_GENERATION", generation);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "case {case}:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[tokio::test]
    async fn environment_case() {
        let Ok(case) = std::env::var("XERO_GENERATION_SINK_TEST_CASE") else {
            return;
        };
        let root = std::env::temp_dir().join(format!("xero-generation-test-{}", Uuid::new_v4()));
        let config = GcsConfig {
            bucket: String::new(),
            prefix: "raw/xero".into(),
        };
        let local_dir = root.to_str().unwrap();
        if matches!(case.as_str(), "empty" | "invalid" | "non-unicode") {
            for dry_run in [true, false] {
                let result = raw_sink(dry_run, local_dir, &config).await;
                let error = match result {
                    Ok(_) => panic!("invalid generation must fail before constructing a sink"),
                    Err(error) => error,
                };
                assert!(format!("{error:#}").contains("XERO_INGESTION_GENERATION"));
            }
            assert!(!root.exists());
            return;
        }
        if case == "non-unicode-namespace" {
            let error = match raw_sink(true, local_dir, &config).await {
                Ok(_) => panic!("invalid namespace must fail before constructing a sink"),
                Err(error) => error,
            };
            assert!(format!("{error:#}").contains("XERO_INGESTION_NAMESPACE"));
            assert!(!root.exists());
            return;
        }
        let sink = raw_sink(true, local_dir, &config).await.unwrap();
        let meta = ObjectMeta::default();
        let original_key = "raw/xero/document.pdf";
        let key = match case.as_str() {
            "absent" => original_key.to_string(),
            "main" => format!("xero/550e8400-e29b-41d4-a716-446655440000/{original_key}"),
            _ => format!("v2/550e8400-e29b-41d4-a716-446655440000/{original_key}"),
        };
        sink.put_raw_typed(original_key, b"%PDF\0\xff", &meta, "application/pdf")
            .await
            .unwrap();
        assert_eq!(
            tokio::fs::read(root.join(&key)).await.unwrap(),
            b"%PDF\0\xff"
        );
        let body = tokio::fs::read(root.join(sidecar_key(&key))).await.unwrap();
        let tags: std::collections::BTreeMap<String, String> =
            serde_json::from_slice(&body).unwrap();
        if case != "absent" {
            assert!(!root.join(original_key).exists());
            assert!(!root.join(sidecar_key(original_key)).exists());
        }
        tokio::fs::remove_dir_all(&root).await.unwrap();
        if case == "absent" {
            assert_eq!(tags, meta.0);
        } else {
            assert_eq!(
                tags.get("x-source-system").map(String::as_str),
                Some(if case == "main" { "xero" } else { "xero_v2" })
            );
            assert_eq!(
                tags.get("x-ingestion-generation").map(String::as_str),
                Some("550e8400-e29b-41d4-a716-446655440000")
            );
            assert_eq!(tags.len(), 2);
        }
    }
}

/// Parse `FROM:TO`, expand into `chunk_months`-sized business-date chunks
/// (oldest→newest), run the job per chunk, and merge the manifests.
async fn run_backfill(
    job: &SyncJob,
    run_id: Uuid,
    spec: &str,
    chunk_months: u32,
    tenants: &[String],
    entities: &[EntityType],
    max_concurrent: usize,
) -> anyhow::Result<RunManifest> {
    let (from, to) = parse_range(spec)?;

    // Entity-aware split: date-windowable entities are pulled in business-date
    // chunks; master/reference + offset endpoints (e.g. Journals) have no `Date`
    // field, so they get a SINGLE full (no-filter) pull — not a per-chunk
    // re-fetch that would error on an invalid `where` and burn the daily cap.
    let (windowable, full_once): (Vec<EntityType>, Vec<EntityType>) = entities
        .iter()
        .cloned()
        .partition(EntityType::is_date_windowable);

    let mut parts: Vec<RunManifest> = Vec::new();

    if !full_once.is_empty() {
        tracing::info!(
            entities = full_once.len(),
            "backfill: single full (no-date) pull for non-windowable entities"
        );
        parts.push(
            job.run(
                run_id,
                to,
                tenants,
                &full_once,
                SyncMode::Master,
                max_concurrent,
            )
            .await,
        );
    }

    if !windowable.is_empty() {
        let chunks = backfill_chunks(from, to, chunk_months);
        tracing::info!(
            from = %from, to = %to, chunks = chunks.len(), chunk_months,
            entities = windowable.len(),
            "backfill: date-chunked pull for windowable entities (oldest→newest)"
        );
        for (chunk_from, chunk_to) in chunks {
            let mode = SyncMode::Backfill {
                from: chunk_from,
                to: chunk_to,
            };
            parts.push(
                job.run(run_id, chunk_to, tenants, &windowable, mode, max_concurrent)
                    .await,
            );
        }
    }

    // Merge every sub-run's manifest into one (entities concatenated; the latest
    // finished_at wins).
    let mut iter = parts.into_iter();
    let mut merged = iter
        .next()
        .context("backfill produced no work (no entities or empty date range)")?;
    for m in iter {
        merged.finished_at = m.finished_at;
        merged.entities.extend(m.entities);
    }
    Ok(merged)
}

/// Print a one-line-per-entity summary plus run-level totals to stdout.
fn report_summary(m: &RunManifest) {
    let errors = m
        .entities
        .iter()
        .filter(|e| e.error.is_some() || e.termination == "hit_max_pages_cap")
        .count();
    let pages: u32 = m.entities.iter().map(|e| e.pages).sum();
    let records: i64 = m.entities.iter().map(|e| e.records).sum();

    println!("\n  run {} — mode={}", m.run_id, m.mode);
    println!("  endpoint                 │ tenant     │ pages │ records │ termination");
    println!("  ─────────────────────────┼────────────┼───────┼─────────┼────────────");
    for e in &m.entities {
        let tenant_short = &e.tenant[..e.tenant.len().min(10)];
        println!(
            "  {:<25}│ {:<11}│ {:>5} │ {:>7} │ {}",
            e.endpoint, tenant_short, e.pages, e.records, e.termination
        );
    }
    println!(
        "\n  {} entities · {pages} pages · {records} records · {errors} error(s)",
        m.entities.len()
    );
    if errors > 0 {
        for e in m
            .entities
            .iter()
            .filter(|e| e.error.is_some() || e.termination == "hit_max_pages_cap")
        {
            if let Some(err) = &e.error {
                eprintln!("  ✗ {} / {}: {err}", e.tenant, e.endpoint);
            } else {
                eprintln!(
                    "  ✗ {} / {}: hit page cap; result may be truncated",
                    e.tenant, e.endpoint
                );
            }
        }
    }
}

/// Test-only constructors shared with the `resolve` test module.
#[cfg(test)]
pub(crate) mod test_support {
    use super::{EntityType, NonZeroUsize, SyncArgs};

    /// A [`SyncArgs`] with every flag at its default, overriding only `entity`.
    pub fn args_with(entity: Vec<EntityType>) -> SyncArgs {
        SyncArgs {
            window_days: None,
            where_clause: None,
            no_window: false,
            business_from: None,
            business_to: None,
            full: false,
            full_with_window: false,
            backfill: None,
            chunk_months: 1,
            reports: false,
            as_of: None,
            report_from: None,
            report_to: None,
            report_periods: None,
            entity,
            tenant: Vec::new(),
            dry_run: false,
            local_dir: "./out".to_string(),
            max_concurrent: None,
            expand_details: false,
            max_detail_records: NonZeroUsize::new(1000).unwrap(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::args_with;
    use super::*;

    use xero_gcs::{EntityOutcome, OutcomeClass};

    #[test]
    fn failed_or_truncated_entity_marks_run_failed() {
        let mut manifest = RunManifest {
            run_id: "run".into(),
            started_at: "start".into(),
            finished_at: "end".into(),
            mode: "incremental".into(),
            window_days: None,
            entities: vec![EntityOutcome {
                tenant: "tenant".into(),
                endpoint: "invoices".into(),
                pages: 1,
                records: 1,
                termination: "ended_on_empty_page".into(),
                request_key: None,
                error: None,
                outcome_class: None,
                http_status: None,
            }],
        };
        assert!(!manifest_has_failures(&manifest));
        manifest.entities[0].error = Some("403".into());
        manifest.entities[0].outcome_class = Some(OutcomeClass::AccessDenied);
        manifest.entities[0].http_status = Some(403);
        assert!(manifest_has_failures(&manifest));
        manifest.entities[0].outcome_class = Some(OutcomeClass::FailedRetryable);
        assert!(manifest_has_failures(&manifest));
        manifest.entities[0].error = None;
        manifest.entities[0].outcome_class = None;
        manifest.entities[0].http_status = None;
        manifest.entities[0].termination = "hit_max_pages_cap".into();
        assert!(manifest_has_failures(&manifest));
    }

    #[test]
    fn apply_overrides_replaces_window_and_concurrency() {
        let base = SyncConfig {
            window_days: 3,
            rolling_full_tight_days: 30,
            rolling_full_wide_days: 90,
            max_concurrent: 6,
        };
        let mut a = args_with(vec![]);
        a.window_days = Some(7);
        a.max_concurrent = Some(2);
        let cfg = apply_overrides(base.clone(), &a);
        assert_eq!(cfg.window_days, 7);
        assert_eq!(cfg.max_concurrent, 2);

        // No flags → config untouched.
        let cfg = apply_overrides(base.clone(), &args_with(vec![]));
        assert_eq!(cfg.window_days, 3);
        assert_eq!(cfg.max_concurrent, 6);
    }

    #[test]
    fn resolve_gcs_config_yields_prefix_in_dry_run() {
        // Dry-run must always succeed and always carry a non-empty prefix,
        // whether or not GCS_BUCKET happens to be set in the environment.
        let mut a = args_with(vec![]);
        a.dry_run = true;
        let cfg = resolve_gcs_config(&a).expect("dry-run must not require a bucket");
        assert!(!cfg.prefix.is_empty());
    }
}
