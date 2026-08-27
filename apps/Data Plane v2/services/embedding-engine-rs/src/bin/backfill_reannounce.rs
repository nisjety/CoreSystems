//! Operator-invoked backfill tool for `dataplane.documents.indexed`.
//!
//! ## Why this exists
//!
//! `check_documents_indexed` (`src/batch/mod.rs`) used to publish
//! `dataplane.documents.indexed` only on a genuine `status != 'indexed' →
//! 'indexed'` Postgres transition. That guard is gone now — a repeat call
//! always re-announces — but the guard's removal only helps documents that
//! flow back through the real embedding pipeline (a content edit, or a
//! future failure-recovery re-drive). It does nothing for documents that are
//! already `status = 'indexed'` today and always will be, because
//! index-engine's exact-reuse chunking optimization never re-emits
//! `dataplane.knowledge.units.created` for unchanged content, so
//! `process_batch` (and the fixed guard inside it) never runs.
//!
//! This tool re-publishes `dataplane.documents.indexed` directly for
//! operator-specified, already-`'indexed'` documents — bypassing the
//! embedding pipeline entirely rather than paying for a real re-embed of
//! unchanged content. It signs with the same producer identity
//! (`service:embedding-engine-rs` / `embedding-events-v1`) `process_batch`
//! uses for this exact subject, and publishes the same way (plain core-NATS
//! `publish`, no JetStream headers) — every consumer durably captures it via
//! its own stream regardless of publish style, and all are idempotent on a
//! repeat, so re-announcing is always safe to retry.
//!
//! Consumers of this subject, and what a re-announce costs each:
//!
//!   - `graph-index-rs` — deterministic content-derived ids, so a repeat
//!     rewrites the same nodes.
//!   - `meilisearch-adapter-rs` — upsert by document id.
//!   - `quickwit-adapter-rs` — deletes the document's chunks and re-indexes
//!     them from Postgres. This is the ONLY path that refreshes a chunk's
//!     `context_body` (from `knowledge_units.chunk_context`), because
//!     contextual retrieval writes that column after the chunk was first
//!     indexed and nothing else re-announces. So after any contextualization
//!     backfill, this tool is what makes contextual BM25 actually reach the
//!     primary sparse backend.
//!
//!     Note that Quickwit's delete is asynchronous — it lands at the next
//!     merge, not before the re-index — so a re-announce transiently leaves
//!     two generations of each chunk in the index. Retrieval is unaffected:
//!     the search path de-duplicates by `knowledge_id`. See the note in
//!     `quickwit-adapter-rs/src/stream.rs` on `SUBJECT_DOCUMENT_INDEXED`.
//!
//! This tool touches no `knowledge_units` row and never re-embeds — it is
//! purely a re-announcement of documents that are already correctly
//! `'indexed'`.
//!
//! Usage:
//!   cargo run --bin backfill-reannounce -- \
//!       --org-id <ORG_ID> [--document-id <ID>]... [--dry-run]
//!
//! With no `--document-id`, every non-deleted, non-restricted document in
//! the org that is already `status = 'indexed'` is targeted.
//!
//! Env: `DATABASE_URL`, `NATS_URL`, `EMBEDDING_EVENT_PRIVATE_KEY_PATH`,
//! `EVENT_AUTH_AUDIENCE` (default `dataplane-events`) — the same variables
//! the `embedding-engine` service itself runs with.

use anyhow::{anyhow, Context};
use event_envelope_rs::EventSigner;

const SUBJECT_DOC_INDEXED: &str = "dataplane.documents.indexed";
const EVENT_ISSUER: &str = "service:embedding-engine-rs";
const EVENT_KEY_ID: &str = "embedding-events-v1";
const EVENT_SCOPE: &str = "events:embedding:publish";

#[derive(Debug)]
struct Args {
    org_id: String,
    document_ids: Vec<String>,
    dry_run: bool,
}

fn parse_args() -> anyhow::Result<Args> {
    let mut org_id: Option<String> = None;
    let mut document_ids = Vec::new();
    let mut dry_run = false;

    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--org-id" => {
                org_id = Some(it.next().ok_or_else(|| anyhow!("--org-id needs a value"))?)
            }
            "--document-id" => document_ids.push(
                it.next()
                    .ok_or_else(|| anyhow!("--document-id needs a value"))?,
            ),
            "--dry-run" => dry_run = true,
            "-h" | "--help" => {
                println!(
                    "backfill-reannounce --org-id <ORG_ID> [--document-id <ID>]... [--dry-run]"
                );
                std::process::exit(0);
            }
            other => return Err(anyhow!("unknown arg: {other}")),
        }
    }

    Ok(Args {
        org_id: org_id.ok_or_else(|| anyhow!("--org-id required"))?,
        document_ids,
        dry_run,
    })
}

struct TargetDoc {
    document_id: String,
    org_id: String,
    title: String,
}

/// Only organization-scoped, live, non-restricted, already-indexed
/// documents are eligible — mirrors the safety exclusions
/// `index-engine-rs::reconcile::claim_stranded` already applies to its own
/// re-drive query.
async fn load_targets(pool: &sqlx::PgPool, args: &Args) -> anyhow::Result<Vec<TargetDoc>> {
    let filter_ids = (!args.document_ids.is_empty()).then(|| args.document_ids.clone());
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT document_id, org_id, title FROM documents \
         WHERE org_id = $1 AND status = 'indexed' AND deleted_at IS NULL \
         AND zdr_classification <> 'restricted' \
         AND ($2::text[] IS NULL OR document_id = ANY($2)) \
         ORDER BY document_id",
    )
    .bind(&args.org_id)
    .bind(filter_ids)
    .fetch_all(pool)
    .await
    .context("query target documents")?;

    Ok(rows
        .into_iter()
        .map(|(document_id, org_id, title)| TargetDoc {
            document_id,
            org_id,
            title,
        })
        .collect())
}

fn idempotency_key(document_id: &str, org_id: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    "backfill.doc.indexed".hash(&mut hasher);
    document_id.hash(&mut hasher);
    org_id.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Same payload shape `process_batch` publishes for `SUBJECT_DOC_INDEXED`,
/// minus fields that only exist mid-embed (`embedding_model`/`provider`
/// become an honest `"backfill-reannounce"` marker rather than a fabricated
/// provider name) and with no acting user — there isn't one for an
/// operator-triggered backfill.
fn build_event(doc: &TargetDoc) -> serde_json::Value {
    serde_json::json!({
        "document_id": doc.document_id,
        "org_id": doc.org_id,
        "title": doc.title,
        "embedding_model": "backfill-reannounce",
        "embedding_provider": "backfill-reannounce",
        "embedded_at": chrono::Utc::now().to_rfc3339(),
        "idempotency_key": idempotency_key(&doc.document_id, &doc.org_id),
        "user_id": null,
        "zdr": false,
        "backfill": true,
    })
}

fn env_var(name: &str) -> anyhow::Result<String> {
    std::env::var(name).with_context(|| format!("{name} must be set"))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = parse_args()?;
    tracing::info!(?args, "backfill-reannounce starting");

    let database_url = env_var("DATABASE_URL")?;
    let nats_url = env_var("NATS_URL")?;
    let key_path = env_var("EMBEDDING_EVENT_PRIVATE_KEY_PATH")?;
    let audience =
        std::env::var("EVENT_AUTH_AUDIENCE").unwrap_or_else(|_| "dataplane-events".to_owned());

    let signer = EventSigner::from_rsa_pem(
        &std::fs::read(&key_path).with_context(|| format!("read {key_path}"))?,
        EVENT_ISSUER,
        EVENT_KEY_ID,
        &audience,
        EVENT_SCOPE,
    )
    .context("construct event signer")?;

    let pool = sqlx::PgPool::connect(&database_url)
        .await
        .context("connect to postgres")?;

    let targets = load_targets(&pool, &args).await?;
    if targets.is_empty() {
        tracing::warn!(org_id = %args.org_id, "no matching indexed documents found");
        return Ok(());
    }

    let client = nats_connection::connect(&nats_url)
        .await
        .context("connect to nats")?;

    let mut sent = 0usize;
    for doc in &targets {
        let event = build_event(doc);
        let raw = serde_json::to_vec(&event)?;
        let envelope = signer
            .sign(SUBJECT_DOC_INDEXED, &doc.org_id, None, false, &raw)
            .context("sign documents.indexed envelope")?;

        if args.dry_run {
            tracing::info!(
                document_id = %doc.document_id,
                title = %doc.title,
                bytes = envelope.len(),
                "would re-announce (dry-run)"
            );
        } else {
            client
                .publish(SUBJECT_DOC_INDEXED, envelope.into())
                .await
                .with_context(|| format!("publish for document {}", doc.document_id))?;
            tracing::info!(document_id = %doc.document_id, title = %doc.title, "re-announced");
            // Confirmed live: publishing a batch back-to-back with no pacing
            // (10 signed envelopes inside ~10ms) got 4/10 rejected downstream
            // with "invalid signed event envelope" — isolated, individually
            // spaced retries of the exact same payloads succeeded every time,
            // so this is a burst-rate artifact rather than a payload defect.
            // The real pipeline never publishes this subject faster than one
            // per completed embedding batch; this delay just keeps a bulk
            // backfill inside that same envelope instead of inventing a rate
            // no consumer has ever had to handle.
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        sent += 1;
    }

    if !args.dry_run {
        client.flush().await.context("flush nats client")?;
    }
    tracing::info!(
        count = sent,
        dry_run = args.dry_run,
        "backfill-reannounce done"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idempotency_key_is_stable_and_scoped_by_document_and_org() {
        let a = idempotency_key("doc-1", "org-1");
        let b = idempotency_key("doc-1", "org-1");
        let c = idempotency_key("doc-1", "org-2");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn build_event_carries_no_acting_user_and_non_restricted_zdr() {
        let doc = TargetDoc {
            document_id: "doc-1".to_owned(),
            org_id: "org-1".to_owned(),
            title: "t".to_owned(),
        };
        let event = build_event(&doc);
        assert_eq!(event["zdr"], false);
        assert!(event["user_id"].is_null());
        assert_eq!(event["backfill"], true);
        assert_eq!(event["document_id"], "doc-1");
    }
}
