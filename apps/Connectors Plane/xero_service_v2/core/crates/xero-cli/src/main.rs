//! `xero` — operator CLI for the stateless raw → GCS uploader.
//!
//! Commands
//! ────────
//!   sync        Fetch raw Xero pages and land them in GCS (or local disk).
//!   healthcheck Mint a token per tenant; optionally probe the GCS bucket.
//!
//! The CLI is a one-shot job driven by an external scheduler. Authentication
//! can use Custom Connections or stored PKCE grants from the onboarding BFF.

mod accounting_get_sync;
mod aged_report_sync;
mod attachment_sync;
mod extended_sync;
mod health;
mod resolve;
mod sync;

use clap::{Parser, Subcommand};

use crate::sync::SyncArgs;

// ── CLI definition ──────────────────────────────────────────────────────────

#[derive(Debug, Parser)]
#[command(name = "xero", version, about = "xero_service_v2 raw → GCS uploader")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Fetch raw Xero pages for the selected layer and upload them verbatim.
    ///
    /// The sync *mode* is derived from the flags (precedence top→bottom):
    ///   --backfill FROM:TO            → backfill (business-date chunks)
    ///   --reports                     → report snapshots (needs --as-of)
    ///   --where <expr> / --no-window  → open-sweep (status filter, no window)
    ///   --business-from/--business-to → rolling-full (explicit business window)
    ///   --full                        → master (no filter) — master entities
    ///   (default)                     → incremental (modified ≥ now − window)
    Sync(SyncArgs),

    /// Fetch non-Accounting APIs into raw GCS (explicit family selection).
    SyncExtended(extended_sync::ExtendedArgs),

    /// Fetch Accounting attachment metadata for explicit or discovered parents.
    SyncAttachments(attachment_sync::AttachmentArgs),

    /// Fetch one catalogued Accounting GET operation for each selected tenant.
    SyncAccountingGet(accounting_get_sync::AccountingGetArgs),

    /// Capture tenant Contacts and fetch the bounded aged AR/AP reports for each contact.
    SyncAgedContactReports(aged_report_sync::AgedContactReportArgs),

    /// Mint a token for each configured tenant and (optionally) probe the bucket.
    Healthcheck(health::HealthArgs),
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Fail before dispatching a long-running sync if an opt-in shared quota
    // backend cannot enforce its reservations.
    xero_client::validate_quota_configuration()?;

    // Init tracing — respects RUST_LOG, defaulting xero_cli to debug.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("info").add_directive(
                    "xero_cli=debug"
                        .parse()
                        .expect("static directive is always valid"),
                )
            }),
        )
        .init();

    match cli.cmd {
        Cmd::Sync(args) => sync::run(args).await,
        Cmd::SyncExtended(args) => extended_sync::run(args).await,
        Cmd::SyncAttachments(args) => attachment_sync::run(args).await,
        Cmd::SyncAccountingGet(args) => accounting_get_sync::run(args).await,
        Cmd::SyncAgedContactReports(args) => aged_report_sync::run(args).await,
        Cmd::Healthcheck(args) => health::run(args).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_flags_parse_with_a_positive_limit() {
        let cli = Cli::try_parse_from([
            "xero",
            "sync",
            "--entity",
            "invoices",
            "--expand-details",
            "--max-detail-records",
            "500",
        ])
        .unwrap();
        let Cmd::Sync(args) = cli.cmd else {
            panic!("expected sync")
        };
        assert!(args.expand_details);
        assert_eq!(args.max_detail_records.get(), 500);
        assert!(Cli::try_parse_from([
            "xero",
            "sync",
            "--expand-details",
            "--max-detail-records",
            "0",
        ])
        .is_err());
    }

    #[test]
    fn aged_contact_reports_parse_as_a_dedicated_bounded_command() {
        let cli = Cli::try_parse_from([
            "xero",
            "sync-aged-contact-reports",
            "--tenant",
            "tenant-a",
            "--as-of",
            "2026-10-18",
            "--max-contacts-per-tenant",
            "25",
            "--max-report-requests",
            "50",
            "--dry-run",
        ])
        .unwrap();
        let Cmd::SyncAgedContactReports(args) = cli.cmd else {
            panic!("expected aged contact report command")
        };
        assert_eq!(args.tenant, vec!["tenant-a"]);
        assert_eq!(args.max_contacts_per_tenant, 25);
        assert_eq!(args.max_report_requests, 50);
        assert!(args.dry_run);
    }
}
