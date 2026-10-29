//! D19 (P4) — re-drive `knowledge_units` stranded in `embedding_status='failed'`.
//!
//! ## The gap
//!
//! Nothing recovered a failed embedding. `data-quality-go`'s gate checker only
//! *counts* them; `data-orchestrator-go`'s stale detector filtered on
//! `= 'pending'`, so failed rows fell outside its stuck query entirely;
//! `documents-api-go` deliberately exposes no reprocess route, and
//! `documentContentUnchanged()` makes an idempotent re-POST of the same
//! document a no-op. The only recovery anyone had was a hand-written
//! `documents_outbox` re-enqueue. So when a transient dependency failed and
//! was then fixed, the corpus did not heal — the affected chunks stayed
//! unreachable in *both* retrieval arms, because `search/sparse.rs` excludes
//! `'failed'` and Qdrant never received a vector.
//!
//! ## Why this lives in index-engine-rs and not data-orchestrator-go
//!
//! The obvious home looked like `data-orchestrator-go`, which already owns
//! reindex/refresh jobs and the stale detector. It is the wrong home, for a
//! reason that only shows up on reading the event-auth contract:
//!
//! - The recovery event has to be `dataplane.knowledge.units.created` — that
//!   is the only subject that re-drives the embedding pipeline.
//! - `embedding-engine-rs` verifies that subject with an `EventVerifier`
//!   pinned to issuer `service:index-engine-rs`, key id `index-events-v1`,
//!   scope `events:index:publish`
//!   (`embedding-engine-rs/src/main.rs`). `event-envelope-rs` enforces the
//!   issuer twice — `Validation::set_issuer` plus an explicit
//!   `claims.iss != expected_issuer` check.
//! - `index-engine-rs` is the only service that holds that private key.
//!   `data-orchestrator-go` holds no event signing key at all: its executor's
//!   publisher is `disabledEventPublisher` unless the unsigned-legacy dev gate
//!   is open.
//!
//! Putting the re-drive in data-orchestrator would therefore have meant either
//! publishing unsigned events (rejected by the consumer, and explicitly
//! forbidden) or copying index-engine's private key into a second service and
//! letting it impersonate `service:index-engine-rs` — trading a durability bug
//! for a much worse authenticity bug. This module runs beside the key that
//! already legitimately signs this subject, and reuses the same
//! `EventSigner` instance the live pipeline uses.
//!
//! ## Boundedness
//!
//! A permanently-poisoned unit must not spin forever. Every claim increments
//! `embedding_retry_count` and stamps `embedding_retry_at`; a unit is eligible
//! only while `embedding_retry_count < max_attempts`, and only after an
//! exponential cooldown (`base * 2^attempts`) has elapsed. Past the ceiling it
//! is left `'failed'` and surfaces in the stale-detector report as exhausted —
//! visible, and nobody's background loop.

use std::sync::Arc;
use std::time::Duration;

use async_nats::jetstream::Context as JsContext;
use event_envelope_rs::EventSigner;
use sqlx::PgPool;

/// The subject that re-drives the embedding pipeline. Must match
/// `stream::run_consumer`'s own publish, and must be inside
/// `events:index:publish` scope.
pub const REEMIT_SUBJECT: &str = "dataplane.knowledge.units.created";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileConfig {
    pub enabled: bool,
    /// How often to poll for eligible units.
    pub interval: Duration,
    /// Base cooldown. The Nth re-drive waits `base * 2^(N-1)`.
    pub cooldown: Duration,
    /// Hard ceiling on re-drives per unit, for all time.
    pub max_attempts: i32,
    /// Units claimed per tick.
    pub batch_size: i64,
}

impl Default for ReconcileConfig {
    fn default() -> Self {
        Self {
            // On by default. The constraint that shaped this: a concurrent
            // session owns `docker-compose.yml` and every `.env` file, so a
            // config-gated fix would ship inert. Defaulting on is defensible
            // because the blast radius is hard-bounded — at most
            // `max_attempts` events per unit, ever, `batch_size` per tick —
            // and every event is signed, tenant-scoped, and idempotent by
            // `knowledge_id` in Qdrant. Set EMBEDDING_RECONCILE_ENABLED=0 to
            // disable.
            enabled: true,
            interval: Duration::from_secs(60),
            cooldown: Duration::from_secs(300),
            max_attempts: 5,
            batch_size: 25,
        }
    }
}

impl ReconcileConfig {
    /// Read overrides from the environment, falling back to the in-code
    /// defaults above. Every knob is optional on purpose: this feature must
    /// work on a deployment whose compose file and `.env` were never touched.
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            enabled: !matches!(
                std::env::var("EMBEDDING_RECONCILE_ENABLED").as_deref(),
                Ok("0") | Ok("false")
            ),
            interval: env_secs("EMBEDDING_RECONCILE_INTERVAL_SECS", defaults.interval),
            cooldown: env_secs("EMBEDDING_RECONCILE_COOLDOWN_SECS", defaults.cooldown),
            max_attempts: env_parse("EMBEDDING_RECONCILE_MAX_ATTEMPTS", defaults.max_attempts)
                .clamp(0, 100),
            batch_size: env_parse("EMBEDDING_RECONCILE_BATCH", defaults.batch_size).clamp(1, 500),
        }
    }
}

fn env_secs(key: &str, fallback: Duration) -> Duration {
    std::env::var(key)
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .map(Duration::from_secs)
        .unwrap_or(fallback)
}

fn env_parse<T: std::str::FromStr>(key: &str, fallback: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|raw| raw.parse::<T>().ok())
        .unwrap_or(fallback)
}

/// One unit claimed for re-drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrandedUnit {
    pub knowledge_id: String,
    pub document_id: String,
    pub org_id: String,
    /// Best available actor attribution, taken from `documents.created_by`.
    /// `None` for a system-ingested document — the envelope's `user_id` claim
    /// is optional, and inventing one would be worse than omitting it.
    pub user_id: Option<String>,
    /// Value AFTER this claim's increment, i.e. how many re-drives this unit
    /// has now had. Used for the dedup key so a re-run is distinguishable.
    pub attempt: i32,
}

/// Claim a batch of stranded units and re-emit a signed event for each.
///
/// The claim and the publishes share one transaction: if any publish fails the
/// transaction rolls back, so a unit is never left flipped to `'pending'` with
/// nothing in flight to embed it (which is exactly the "stuck pending" state
/// the stale detector reports and nobody fixes).
pub async fn run_once(
    pool: &PgPool,
    js: &JsContext,
    signer: &EventSigner,
    config: &ReconcileConfig,
) -> anyhow::Result<usize> {
    // Phase 1 RLS: unscoped on purpose. This reconciler is a background loop
    // that heals the corpus for EVERY org — `claim_stranded` below picks the
    // oldest eligible units plane-wide with `FOR UPDATE ... SKIP LOCKED` and
    // has no org in hand at all (each claimed row's `org_id` is an *output*,
    // read back to address the re-drive event). An org-scoped transaction here
    // would not fail; it would silently reduce the reconciler to a single
    // tenant and leave every other org's failed embeddings stranded — exactly
    // the gap this module exists to close.
    let mut tx = pool.begin().await?;
    let claimed = claim_stranded(&mut tx, config).await?;
    if claimed.is_empty() {
        tx.rollback().await?;
        return Ok(0);
    }

    for unit in &claimed {
        publish_reembed(js, signer, unit).await?;
    }

    tx.commit().await?;
    for unit in &claimed {
        tracing::info!(
            knowledge_id = %unit.knowledge_id,
            document_id = %unit.document_id,
            attempt = unit.attempt,
            "re-drove stranded embedding"
        );
    }
    Ok(claimed.len())
}

/// The claim query.
///
/// Notable exclusions, each load-bearing:
/// - `d.deleted_at IS NULL` — never resurrect work for a deleted document.
/// - `d.zdr_classification <> 'restricted'` — a Zero-Data-Retention document's
///   chunks must not be re-driven. Both index-engine and embedding-engine drop
///   ZDR events without durable writes, so re-emitting one would burn the
///   entire retry budget on an event guaranteed to be discarded, and would
///   look like an attempt to smuggle restricted content into the embedder.
/// - `embedding_retry_count < max_attempts` — the hard ceiling.
/// - `COALESCE(embedding_retry_at, updated_at) < now() - base * 2^attempts` —
///   exponential backoff, seeded from `updated_at` for units that failed
///   before this column existed.
///
/// `FOR UPDATE ... SKIP LOCKED` is the plane's standard claim idiom, so two
/// index-engine replicas cannot claim the same unit.
async fn claim_stranded(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    config: &ReconcileConfig,
) -> anyhow::Result<Vec<StrandedUnit>> {
    let rows: Vec<(String, String, String, Option<String>, i32)> = sqlx::query_as(
        r#"
        UPDATE knowledge_units k
           SET embedding_status      = 'pending',
               embedding_retry_count = k.embedding_retry_count + 1,
               embedding_retry_at    = NOW(),
               error_message         = NULL
         WHERE k.knowledge_id IN (
                SELECT ku.knowledge_id
                  FROM knowledge_units ku
                  JOIN documents d ON d.document_id = ku.document_id
                 WHERE ku.embedding_status = 'failed'
                   AND ku.embedding_retry_count < $1
                   AND COALESCE(ku.embedding_retry_at, ku.updated_at)
                       < NOW() - make_interval(
                             secs => $2::double precision
                                     * POWER(2::double precision, ku.embedding_retry_count))
                   AND d.deleted_at IS NULL
                   AND d.zdr_classification <> 'restricted'
                 ORDER BY COALESCE(ku.embedding_retry_at, ku.updated_at)
                 LIMIT $3
                 FOR UPDATE OF ku SKIP LOCKED
               )
        RETURNING k.knowledge_id,
                  k.document_id,
                  k.org_id,
                  (SELECT NULLIF(d2.created_by, '') FROM documents d2
                    WHERE d2.document_id = k.document_id),
                  k.embedding_retry_count
        "#,
    )
    .bind(config.max_attempts)
    .bind(config.cooldown.as_secs_f64())
    .bind(config.batch_size)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(knowledge_id, document_id, org_id, user_id, attempt)| StrandedUnit {
                knowledge_id,
                document_id,
                org_id,
                user_id,
                attempt,
            },
        )
        .collect())
}

async fn publish_reembed(
    js: &JsContext,
    signer: &EventSigner,
    unit: &StrandedUnit,
) -> anyhow::Result<()> {
    let payload = reembed_payload(unit)?;
    // `zdr: false` is not an assumption — the claim query excludes restricted
    // documents outright, so a claimed unit is non-ZDR by construction.
    let envelope = signer.sign(
        REEMIT_SUBJECT,
        &unit.org_id,
        unit.user_id.as_deref(),
        false,
        &serde_json::to_vec(&payload)?,
    )?;

    let mut headers = async_nats::HeaderMap::new();
    headers.insert("Nats-Msg-Id", dedup_key(unit).as_str());
    // JetStream publish with an ack, not core-NATS: a re-drive that vanished
    // into an unbound subject would be D17 all over again, and the transaction
    // above depends on this returning an error when the event did not land.
    js.publish_with_headers(REEMIT_SUBJECT, headers, envelope.into())
        .await?
        .await?;
    Ok(())
}

/// The event body. Byte-for-byte the same shape `stream::run_consumer`
/// publishes for a freshly chunked unit — including the empty `text`, which
/// tells embedding-engine to read the chunk from Postgres rather than trust a
/// copy carried on the wire.
pub fn reembed_payload(unit: &StrandedUnit) -> anyhow::Result<serde_json::Value> {
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

/// JetStream message-dedup id. Includes the attempt number so a *later*
/// re-drive of the same unit is a distinct message, while a retry of the same
/// attempt (e.g. after a rolled-back transaction) is deduplicated inside the
/// stream's dedup window.
pub fn dedup_key(unit: &StrandedUnit) -> String {
    format!("reembed:{}:{}", unit.knowledge_id, unit.attempt)
}

/// The poll loop. Never returns; errors are logged and retried on the next
/// tick, because a reconciler that exits on a transient database blip is
/// exactly as useless as no reconciler.
pub async fn run(
    pool: PgPool,
    js: JsContext,
    signer: Arc<EventSigner>,
    config: ReconcileConfig,
) -> anyhow::Result<()> {
    if !config.enabled {
        tracing::warn!("failed-embedding reconciler disabled by EMBEDDING_RECONCILE_ENABLED");
        return std::future::pending().await;
    }
    tracing::info!(
        interval_secs = config.interval.as_secs(),
        cooldown_secs = config.cooldown.as_secs(),
        max_attempts = config.max_attempts,
        batch_size = config.batch_size,
        "failed-embedding reconciler online"
    );

    loop {
        match run_once(&pool, &js, signer.as_ref(), &config).await {
            Ok(0) => {}
            Ok(count) => tracing::info!(count, "failed-embedding reconciliation tick"),
            Err(error) => {
                tracing::warn!(%error, "failed-embedding reconciliation tick failed; retrying")
            }
        }
        tokio::time::sleep(config.interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit() -> StrandedUnit {
        StrandedUnit {
            knowledge_id: "kid-1".into(),
            document_id: "doc-1".into(),
            org_id: "org-a".into(),
            user_id: Some("user-1".into()),
            attempt: 1,
        }
    }

    #[test]
    fn reembed_payload_matches_the_live_producer_shape() {
        let payload = reembed_payload(&unit()).expect("valid");
        assert_eq!(payload["knowledge_id"], "kid-1");
        assert_eq!(payload["document_id"], "doc-1");
        assert_eq!(payload["org_id"], "org-a");
        assert_eq!(payload["user_id"], "user-1");
        // Empty text is the contract: embedding-engine re-reads the chunk from
        // Postgres. Shipping the text here would let a stale event re-embed
        // superseded content.
        assert_eq!(payload["text"], "");
        assert_eq!(payload["zdr"], false);
    }

    #[test]
    fn reembed_payload_rejects_ambiguous_scope() {
        let mut u = unit();
        u.org_id.clear();
        assert!(reembed_payload(&u).is_err());

        let mut u = unit();
        u.document_id = "   ".into();
        assert!(reembed_payload(&u).is_err());

        let mut u = unit();
        u.knowledge_id.clear();
        assert!(reembed_payload(&u).is_err());

        let mut u = unit();
        u.user_id = Some("  ".into());
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
    fn dedup_key_separates_attempts_but_not_retries_of_one_attempt() {
        let mut u = unit();
        let first = dedup_key(&u);
        assert_eq!(first, dedup_key(&u), "same attempt must dedup");
        u.attempt = 2;
        assert_ne!(first, dedup_key(&u), "a later re-drive must not be deduped");
    }

    #[test]
    fn defaults_are_bounded() {
        let config = ReconcileConfig::default();
        assert!(config.enabled);
        assert!(config.max_attempts > 0 && config.max_attempts <= 10);
        assert!(config.batch_size > 0);
        assert!(config.cooldown >= Duration::from_secs(60));
        // The whole point of D19's boundedness: a permanently-poisoned unit
        // costs a finite, small number of events for all time.
        assert!(config.max_attempts <= 5);
    }

    #[test]
    fn backoff_grows_exponentially_and_is_bounded_in_total() {
        let config = ReconcileConfig::default();
        let base = config.cooldown.as_secs();
        let waits: Vec<u64> = (0..config.max_attempts)
            .map(|n| base * 2u64.pow(n as u32))
            .collect();
        assert_eq!(waits, vec![300, 600, 1200, 2400, 4800]);
        // ~2.6h of retrying, then the unit is left alone forever.
        assert_eq!(waits.iter().sum::<u64>(), 9300);
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    async fn claim_is_bounded_scoped_and_backed_off() {
        let database_url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must point to disposable PostgreSQL");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(4)
            .connect(&database_url)
            .await
            .expect("connect disposable postgres");

        sqlx::raw_sql(
            "DROP TABLE IF EXISTS knowledge_units;
             DROP TABLE IF EXISTS documents;
             CREATE TABLE documents (
               document_id TEXT PRIMARY KEY,
               org_id TEXT NOT NULL,
               created_by TEXT,
               zdr_classification TEXT NOT NULL DEFAULT 'internal',
               deleted_at TIMESTAMPTZ
             );
             CREATE TABLE knowledge_units (
               knowledge_id TEXT PRIMARY KEY,
               document_id TEXT NOT NULL REFERENCES documents(document_id),
               org_id TEXT NOT NULL,
               embedding_status TEXT NOT NULL DEFAULT 'pending',
               error_message TEXT,
               embedding_retry_count INTEGER NOT NULL DEFAULT 0,
               embedding_retry_at TIMESTAMPTZ,
               updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
             );
             INSERT INTO documents (document_id, org_id, created_by) VALUES
               ('doc-live', 'org-a', 'user-1');
             INSERT INTO documents (document_id, org_id, created_by, deleted_at) VALUES
               ('doc-deleted', 'org-a', 'user-1', NOW());
             INSERT INTO documents (document_id, org_id, created_by, zdr_classification) VALUES
               ('doc-zdr', 'org-a', 'user-1', 'restricted');
             INSERT INTO knowledge_units
               (knowledge_id, document_id, org_id, embedding_status, error_message,
                embedding_retry_count, updated_at) VALUES
               ('kid-eligible',  'doc-live',    'org-a', 'failed', 'boom', 0, NOW() - INTERVAL '1 day'),
               ('kid-cooldown',  'doc-live',    'org-a', 'failed', 'boom', 0, NOW()),
               ('kid-exhausted', 'doc-live',    'org-a', 'failed', 'boom', 5, NOW() - INTERVAL '1 day'),
               ('kid-deleted',   'doc-deleted', 'org-a', 'failed', 'boom', 0, NOW() - INTERVAL '1 day'),
               ('kid-zdr',       'doc-zdr',     'org-a', 'failed', 'boom', 0, NOW() - INTERVAL '1 day'),
               ('kid-done',      'doc-live',    'org-a', 'done',   NULL,   0, NOW() - INTERVAL '1 day');",
        )
        .execute(&pool)
        .await
        .expect("seed schema");

        let config = ReconcileConfig::default();
        let mut tx = pool.begin().await.expect("begin");
        let claimed = claim_stranded(&mut tx, &config).await.expect("claim");
        tx.commit().await.expect("commit");

        let ids: Vec<&str> = claimed.iter().map(|u| u.knowledge_id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["kid-eligible"],
            "only the eligible unit may be claimed"
        );
        assert_eq!(claimed[0].attempt, 1);
        assert_eq!(claimed[0].user_id.as_deref(), Some("user-1"));

        let (status, count, error): (String, i32, Option<String>) = sqlx::query_as(
            "SELECT embedding_status, embedding_retry_count, error_message
               FROM knowledge_units WHERE knowledge_id = 'kid-eligible'",
        )
        .fetch_one(&pool)
        .await
        .expect("read back");
        assert_eq!(status, "pending", "claimed unit must become retryable");
        assert_eq!(count, 1, "retry budget must be consumed");
        assert_eq!(error, None, "stale error must be cleared on re-drive");

        // Immediately re-claiming must find nothing: the unit is no longer
        // 'failed', and its cooldown has just been reset.
        let mut tx = pool.begin().await.expect("begin");
        let again = claim_stranded(&mut tx, &config).await.expect("claim");
        tx.rollback().await.expect("rollback");
        assert!(again.is_empty(), "a claimed unit must not be re-claimed");

        // The exhausted unit stays exhausted no matter how long it waits.
        sqlx::raw_sql(
            "UPDATE knowledge_units SET embedding_retry_at = NOW() - INTERVAL '365 days'
              WHERE knowledge_id = 'kid-exhausted';",
        )
        .execute(&pool)
        .await
        .expect("age the exhausted unit");
        let mut tx = pool.begin().await.expect("begin");
        let exhausted = claim_stranded(&mut tx, &config).await.expect("claim");
        tx.rollback().await.expect("rollback");
        assert!(
            exhausted.is_empty(),
            "a poisoned unit past the ceiling must never spin again"
        );

        sqlx::raw_sql("DROP TABLE IF EXISTS knowledge_units; DROP TABLE IF EXISTS documents;")
            .execute(&pool)
            .await
            .expect("cleanup");
    }
}
