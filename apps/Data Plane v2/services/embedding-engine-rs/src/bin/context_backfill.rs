//! Operator-invoked backfill, repair and verification of contextual retrieval (CR-T9a).
//!
//! A thin wrapper. Everything it does is the context reconciler's own code (`src/context_reconciler`):
//! the same selection, the same leases (so it can run next to the service), the same pipeline,
//! the same writes. The service's modules are compiled into this binary the way
//! `qdrant-backfill-source` compiles `source_stamp`, so there is one implementation of "make
//! these units' contexts and vectors right" and the tool cannot drift from the service. What is
//! its own is the command line (`context_reconciler::cli`, tested with the service) and the
//! construction of what the reconciler runs on.
//!
//! ```text
//! context-backfill --dry-run [--org-id ORG] [--target-version ctx-v2]
//! context-backfill --org-id ORG (--max-units N | --max-usd USD) [--target-version ctx-v2]
//! ```
//!
//! Run the dry run first. It prints, per organization, how many units need a context, how many are
//! `legacy` and will be verified (no model), how many have a vector that is not the one their
//! context makes, how many failed every attempt, and an estimate of what the run would cost: counts
//! and numbers, never content. A real run needs a cap. See `docs/contextual-retrieval-runbook.md`,
//! sections 5.2 and 5.7.
//!
//! Environment: the service's own (`DATABASE_URL`, `QDRANT_URL`, `NATS_URL`,
//! `EMBEDDING_EVENT_PRIVATE_KEY_PATH`, `EVENT_AUTH_AUDIENCE`, the token and contextual retrieval
//! settings). The estimate's prices default to the Model Plane catalogue's gpt-4o-mini row and are
//! overridden by `CONTEXT_ESTIMATE_INPUT_USD_PER_MTOK`, `..._OUTPUT_USD_PER_MTOK` and
//! `CONTEXT_ESTIMATE_EMBED_USD_PER_MTOK` (no embedding price is in the repository, so without it
//! the embedding cost is reported as unknown). Logs go to stderr; the report is JSON on stdout.
//!
//! Exit status: 0 the run did what it was asked (it finished, or stopped at its cap), 1 it failed,
//! 2 the command line is wrong, 3 it was stopped before it could finish (the pause file, the
//! circuit breaker, a pass that could not run).

// The service's own modules, compiled in. Most of what they define is the service's and this tool
// does not use it; that is not a defect of this tool.
#![allow(dead_code)]

#[path = "../batch/mod.rs"]
mod batch;
#[path = "../budget.rs"]
mod budget;
#[path = "../config.rs"]
mod config;
#[path = "../context/mod.rs"]
mod context;
#[path = "../context_reconciler.rs"]
mod context_reconciler;
#[path = "../policy.rs"]
mod policy;
#[path = "../provider/mod.rs"]
mod provider;
#[path = "../qdrant_writer/mod.rs"]
mod qdrant_writer;
#[path = "../source_stamp.rs"]
mod source_stamp;

use std::process::ExitCode;
use std::sync::Arc;

use anyhow::Context as _;
use event_envelope_rs::EventSigner;

use crate::config::Config;
use crate::context_reconciler::cli::{self, Args, Parsed};
use crate::context_reconciler::{Backfill, Prices, Reconciler, Settings};

const EVENT_ISSUER: &str = "service:embedding-engine-rs";
const EVENT_KEY_ID: &str = "embedding-events-v1";
const EVENT_SCOPE: &str = "events:embedding:publish";

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn,embedding_engine_rs=info,context_backfill=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = match cli::parse(std::env::args().skip(1)) {
        Ok(Parsed::Help) => {
            println!("{}", cli::USAGE);
            return ExitCode::SUCCESS;
        }
        Ok(Parsed::Run(args)) => args,
        Err(message) => {
            eprintln!("context-backfill: {message}\n\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };
    match run(args).await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("context-backfill: {error:#}");
            ExitCode::from(1)
        }
    }
}

async fn run(args: Args) -> anyhow::Result<ExitCode> {
    let mut cfg = Config::from_env().context("read the service's configuration")?;
    // A refresh to a version is a run that generates with that version, so that what is selected
    // (contexts made with another one) and what is generated agree, and a unit refreshed once is
    // not selected again. Its cap is the operator's `--max-units`.
    if let Some(target) = args.target_version {
        cfg.contextual_retrieval_prompt_version = target.as_str().to_owned();
        cfg.context_refresh_on_prompt_change = true;
        cfg.context_refresh_max_units = args.max_units.unwrap_or(0);
    }
    let prices = Prices::from_env()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&cfg.database_url)
        .await
        .context("connect to postgres")?;

    if args.dry_run {
        // Counts only: no model, no vector store, no broker.
        let settings = Settings::from_config(&cfg)?;
        let report = context_reconciler::dry_run(
            &pool,
            settings.max_attempts,
            settings.prompt_chars_cap,
            args.org_id.as_deref(),
            args.target_version,
            &prices,
            i64::from(cfg.contextual_retrieval_max_tokens),
        )
        .await?;
        println!("{report}");
        return Ok(ExitCode::SUCCESS);
    }

    let org_id = args
        .org_id
        .clone()
        .context("--org-id is required for a real run")?;
    let qdrant = qdrant_client::Qdrant::from_url(&cfg.qdrant_url)
        .build()
        .map_err(|error| anyhow::anyhow!("qdrant connect: {error}"))?;
    let provider = crate::provider::EmbeddingProvider::from_config(&cfg)?;
    let nats = nats_connection::connect(&cfg.nats_url)
        .await
        .context("connect to nats")?;
    anyhow::ensure!(
        !cfg.embedding_event_private_key_path.trim().is_empty(),
        "EMBEDDING_EVENT_PRIVATE_KEY_PATH must be set: a healed document is announced signed"
    );
    let signer = Arc::new(
        EventSigner::from_rsa_pem(
            &std::fs::read(&cfg.embedding_event_private_key_path)
                .with_context(|| format!("read {}", cfg.embedding_event_private_key_path))?,
            EVENT_ISSUER,
            EVENT_KEY_ID,
            &cfg.event_auth_audience,
            EVENT_SCOPE,
        )
        .context("construct the event signer")?,
    );
    // The same gate the service runs (CR-T9b): a backfill of an organization whose budget is used up
    // in `stop` mode does not generate, and says so, exactly as the service's own loop does.
    let budget = Arc::new(budget::BudgetGate::from_config(&cfg, pool.clone())?);
    let reconciler = Reconciler::from_config(
        &cfg,
        pool,
        qdrant,
        provider,
        nats.clone(),
        Some(signer),
        budget,
    )
    .await?;

    let request = Backfill {
        org_id: org_id.clone(),
        max_units: args.max_units,
        max_usd: args.max_usd,
        refresh_target: args.target_version,
        prices,
    };
    let report = reconciler
        .backfill(&request, i64::from(cfg.contextual_retrieval_max_tokens))
        .await?;
    // The announcements are buffered by the client: they must have left before the process does.
    nats.flush().await.context("flush the announcements")?;
    println!("{}", cli::render(&org_id, &report, prices.embed.is_some()));
    Ok(
        if report.stopped.starts_with("paused") || report.stopped.starts_with("stopped") {
            ExitCode::from(3)
        } else {
            ExitCode::SUCCESS
        },
    )
}
