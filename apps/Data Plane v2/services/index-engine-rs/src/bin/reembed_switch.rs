//! Operator-invoked re-embed tool for a dense-embedder provider switch (D-B,
//! `docs/sovereign-rag-phased-plan.md` — migrating text from
//! `text-embedding-3-large` to Cohere Embed v4).
//!
//! ## Why this replays `knowledge.units.created`, unlike `backfill-reannounce`
//!
//! `embedding-engine-rs/src/bin/backfill_reannounce.rs` re-announces
//! `dataplane.documents.indexed` directly, deliberately bypassing the
//! embedding pipeline — right for that case, because the units were already
//! correctly embedded and only the *downstream* graph/keyword arms needed a
//! signal. A provider switch is the opposite case: the whole point is a real
//! re-embed into a new vector space (new Qdrant collection, new dimension),
//! so the embedding pipeline is exactly what must run. `documents.indexed`
//! would skip the one step that matters here.
//!
//! This mirrors `reconcile.rs`'s signing identity and payload shape (the only
//! service holding `service:index-engine-rs` / `index-events-v1`, the issuer
//! `embedding-engine-rs`'s verifier for this subject is pinned to) but is a
//! genuinely different tool, not a mode of it: `reconcile::claim_stranded`
//! targets `embedding_status = 'failed'` and mutates it to `'pending'` as
//! part of the claim — correct for recovering a transient failure, wrong
//! here. This tool targets already-`'done'` units, is a plain read (no
//! `UPDATE`, no `FOR UPDATE` claim), and never touches `embedding_status` —
//! the units are not stranded, they are correct today and simply need to
//! exist in the new provider's vector space too.
//!
//! Usage:
//!   cargo run --bin reembed-switch -- \
//!       --org-id <ORG_ID> [--document-id <ID>]... [--dry-run]
//!
//! With no `--document-id`, every non-deleted, non-restricted, already-
//! `'done'` unit in the org is targeted.
//!
//! Env: `DATABASE_URL`, `NATS_URL`, `INDEX_EVENT_PRIVATE_KEY_PATH`,
//! `EVENT_AUTH_AUDIENCE` (default `dataplane-events`) — the same variables
//! the `index-engine` service itself runs with.

use anyhow::{anyhow, Context};
use event_envelope_rs::EventSigner;

const REEMIT_SUBJECT: &str = "dataplane.knowledge.units.created";
const EVENT_ISSUER: &str = "service:index-engine-rs";
const EVENT_KEY_ID: &str = "index-events-v1";
const EVENT_SCOPE: &str = "events:index:publish";
/// Matches `backfill-reannounce`'s empirically-confirmed fix: publishing many
/// signed envelopes with no pacing tripped the downstream verifier under a
/// sub-10ms burst. This tool's publish already waits for a JetStream ack
/// before moving on (unlike that core-NATS fire-and-forget case), but the
/// pacing is kept anyway as cheap, consistent insurance.
const PUBLISH_PACING: std::time::Duration = std::time::Duration::from_millis(100);

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
                println!("reembed-switch --org-id <ORG_ID> [--document-id <ID>]... [--dry-run]");
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

struct TargetUnit {
    knowledge_id: String,
    document_id: String,
    org_id: String,
    user_id: Option<String>,
}

/// Only organization-scoped, live, non-restricted, already-`'done'` units are
/// eligible — the same safety exclusions `index-engine-rs::reconcile::
/// claim_stranded` applies to its own re-drive query, minus the failed-only
/// filter and the claim/mutate step (this is a plain read).
async fn load_targets(pool: &sqlx::PgPool, args: &Args) -> anyhow::Result<Vec<TargetUnit>> {
    let filter_ids = (!args.document_ids.is_empty()).then(|| args.document_ids.clone());
    // Phase 1 RLS: this tool refuses to run without `--org-id` and targets that
    // one org only, so the read goes through an org-scoped transaction. It is
    // an operator tool rather than an event consumer, which is exactly why the
    // policy is worth having here — the org is whatever the operator typed, and
    // a scoped transaction makes the database, not the hand-written `d.org_id =
    // $1` predicate, the thing that keeps a mistyped or over-broad invocation
    // from re-embedding another tenant's corpus. The predicate stays: it is the
    // primary filter, and the policy is the backstop.
    let mut tx = pg_org_scope::begin_org_scoped(pool, &args.org_id).await?;
    let rows: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT k.knowledge_id, k.document_id, k.org_id, \
                (SELECT NULLIF(d2.created_by, '') FROM documents d2 \
                  WHERE d2.document_id = k.document_id) AS user_id \
         FROM knowledge_units k \
         JOIN documents d ON d.document_id = k.document_id \
         WHERE d.org_id = $1 AND k.embedding_status = 'done' AND d.deleted_at IS NULL \
           AND d.zdr_classification <> 'restricted' \
           AND ($2::text[] IS NULL OR k.document_id = ANY($2)) \
         ORDER BY k.document_id, k.knowledge_id",
    )
    .bind(&args.org_id)
    .bind(filter_ids)
    .fetch_all(&mut *tx)
    .await
    .context("query target knowledge_units")?;
    tx.commit().await?;

    Ok(rows
        .into_iter()
        .map(|(knowledge_id, document_id, org_id, user_id)| TargetUnit {
            knowledge_id,
            document_id,
            org_id,
            user_id,
        })
        .collect())
}

/// Byte-for-byte the same shape `stream::run_consumer` publishes for a
/// freshly chunked unit — including the empty `text`, which tells
/// embedding-engine to read the chunk from Postgres rather than trust a copy
/// carried on the wire (so this stays correct even if the chunk text has
/// since changed).
fn reembed_payload(unit: &TargetUnit) -> anyhow::Result<serde_json::Value> {
    if unit.org_id.trim().is_empty()
        || unit.document_id.trim().is_empty()
        || unit.knowledge_id.trim().is_empty()
        || unit.user_id.as_deref().is_some_and(|u| u.trim().is_empty())
    {
        anyhow::bail!("invalid tenant-scoped re-embed identity");
    }
    Ok(serde_json::json!({
        "knowledge_id": unit.knowledge_id,
        "document_id": unit.document_id,
        "org_id": unit.org_id,
        "text": "",
        "user_id": unit.user_id,
        "zdr": false,
    }))
}

/// JetStream message-dedup id. No attempt counter (unlike `reconcile.rs`'s
/// automatic retries) — this is a one-shot operator action, so a stable key
/// per unit is enough to protect against an accidental double-invocation
/// landing inside the stream's dedup window.
fn dedup_key(unit: &TargetUnit) -> String {
    format!("reembed-switch:{}", unit.knowledge_id)
}

fn env_var(name: &str) -> anyhow::Result<String> {
    std::env::var(name).with_context(|| format!("{name} must be set"))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = parse_args()?;
    tracing::info!(?args, "reembed-switch starting");

    let database_url = env_var("DATABASE_URL")?;
    let nats_url = env_var("NATS_URL")?;
    let key_path = env_var("INDEX_EVENT_PRIVATE_KEY_PATH")?;
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
        tracing::warn!(org_id = %args.org_id, "no matching done knowledge_units found");
        return Ok(());
    }

    let client = nats_connection::connect(&nats_url)
        .await
        .context("connect to nats")?;
    let js = async_nats::jetstream::new(client);

    let mut sent = 0usize;
    for unit in &targets {
        let payload = reembed_payload(unit)?;
        let raw = serde_json::to_vec(&payload)?;
        let envelope = signer
            .sign(
                REEMIT_SUBJECT,
                &unit.org_id,
                unit.user_id.as_deref(),
                false,
                &raw,
            )
            .context("sign knowledge.units.created envelope")?;

        if args.dry_run {
            tracing::info!(
                knowledge_id = %unit.knowledge_id,
                document_id = %unit.document_id,
                bytes = envelope.len(),
                "would re-embed (dry-run)"
            );
        } else {
            let mut headers = async_nats::HeaderMap::new();
            headers.insert("Nats-Msg-Id", dedup_key(unit).as_str());
            js.publish_with_headers(REEMIT_SUBJECT, headers, envelope.into())
                .await
                .with_context(|| format!("publish for knowledge_id {}", unit.knowledge_id))?
                .await
                .with_context(|| format!("ack for knowledge_id {}", unit.knowledge_id))?;
            tracing::info!(knowledge_id = %unit.knowledge_id, document_id = %unit.document_id, "re-embed queued");
            tokio::time::sleep(PUBLISH_PACING).await;
        }
        sent += 1;
    }

    tracing::info!(count = sent, dry_run = args.dry_run, "reembed-switch done");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit() -> TargetUnit {
        TargetUnit {
            knowledge_id: "kid-1".to_owned(),
            document_id: "doc-1".to_owned(),
            org_id: "org-a".to_owned(),
            user_id: Some("user-1".to_owned()),
        }
    }

    #[test]
    fn reembed_payload_matches_the_live_producer_shape() {
        let payload = reembed_payload(&unit()).expect("valid");
        assert_eq!(payload["knowledge_id"], "kid-1");
        assert_eq!(payload["document_id"], "doc-1");
        assert_eq!(payload["org_id"], "org-a");
        assert_eq!(payload["user_id"], "user-1");
        assert_eq!(payload["text"], "");
        assert_eq!(payload["zdr"], false);
    }

    #[test]
    fn reembed_payload_rejects_ambiguous_scope() {
        let mut u = unit();
        u.org_id.clear();
        assert!(reembed_payload(&u).is_err());

        let mut u = unit();
        u.document_id = "   ".to_owned();
        assert!(reembed_payload(&u).is_err());

        let mut u = unit();
        u.knowledge_id.clear();
        assert!(reembed_payload(&u).is_err());
    }

    #[test]
    fn a_system_ingested_document_may_have_no_actor() {
        let mut u = unit();
        u.user_id = None;
        let payload = reembed_payload(&u).expect("no actor is valid");
        assert!(payload["user_id"].is_null());
    }

    #[test]
    fn dedup_key_is_stable_per_unit_with_no_attempt_concept() {
        let u = unit();
        assert_eq!(dedup_key(&u), dedup_key(&u));
        assert_eq!(dedup_key(&u), "reembed-switch:kid-1");
    }
}
