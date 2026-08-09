use std::collections::HashSet;

use sqlx::PgPool;

use crate::chunker::{chunk_text, Chunk, ChunkConfig};
use crate::fingerprint::{content_hash, stable_chunk_id};
use crate::normalizer::normalize;

#[derive(Debug, Clone)]
pub struct DocumentEvent {
    pub document_id: String,
    pub org_id: String,
    pub title: String,
    pub source: String,
    pub doc_type: String,
    pub user_id: Option<String>,
    pub idempotency_key: String,
    pub zdr: bool,
}

#[derive(Debug)]
pub struct BuildResult {
    #[allow(dead_code)] // surfaced via Debug + future API responses
    pub document_id: String,
    #[allow(dead_code)] // surfaced via Debug + future API responses
    pub chunks_created: usize,
    // The full durable set for this (re)build, in chunk order — reused and
    // newly-embedded alike. Surfaced via Debug + future API responses; the
    // stream drives embedding off `pending_knowledge_ids` instead.
    #[allow(dead_code)]
    pub knowledge_ids: Vec<String>,
    // Subset of `knowledge_ids` that this build actually (re)inserted as
    // 'pending' — the chunks the embedding-engine must embed. Chunks whose
    // content is byte-identical to a prior 'done' unit are reused (their vector
    // is already live in Qdrant under the identical knowledge_id) and are
    // omitted here, so the stream never re-publishes them for embedding. On a
    // first build this equals `knowledge_ids`; on a no-op re-crawl it is empty.
    pub pending_knowledge_ids: Vec<String>,
    // Knowledge IDs that existed before this (re)build but no longer do —
    // their vectors must be purged from Qdrant to avoid stale retrieval hits
    // after a content update. Empty on first build.
    #[allow(dead_code)] // persisted atomically to index_deletion_outbox
    pub orphaned_knowledge_ids: Vec<String>,
}

// orphaned_ids returns the old knowledge IDs that are absent from the new set.
// Because knowledge IDs are content-derived (stable_chunk_id), unchanged chunks
// keep their ID across a rebuild; only changed or removed chunks orphan.
fn orphaned_ids(old_kids: &[String], new_kids: &[String]) -> Vec<String> {
    let new_set: std::collections::HashSet<&str> = new_kids.iter().map(String::as_str).collect();
    old_kids
        .iter()
        .filter(|k| !new_set.contains(k.as_str()))
        .cloned()
        .collect()
}

fn canonical_document_is_indexable(deleted: bool, zdr_classification: &str) -> bool {
    if deleted {
        return false;
    }
    matches!(
        zdr_classification.trim().to_ascii_lowercase().as_str(),
        "internal" | "public" | "sensitive"
    )
}

/// The `(content_hash, knowledge_id)` pair assigned to a chunk. Both are pure
/// functions of the document id, the chunk's position, and its text, so a
/// chunk's durable identity never depends on existing database rows. This is
/// what lets the ingest loop INSERT unconditionally instead of issuing a
/// per-chunk existence/dedup SELECT.
fn chunk_identity(document_id: &str, chunk_index: usize, text: &str) -> (String, String) {
    let hash = content_hash(text);
    let kid = stable_chunk_id(document_id, chunk_index, &hash);
    (hash, kid)
}

/// A chunk's durable identity plus whether its embedding can be reused.
struct ChunkPlan {
    hash: String,
    kid: String,
    /// True when `kid` is already present as an `embedding_status = 'done'`
    /// unit — its vector is live in Qdrant under this exact id, so the chunk
    /// needs no re-insert and no re-embed.
    reused: bool,
}

/// Decide, in chunk order, which chunks can reuse an existing embedding.
///
/// A chunk is reused only when its content-derived `knowledge_id`
/// (`document_id` + `chunk_index` + `content_hash`) exactly matches a prior
/// `'done'` unit. That equality is what makes reuse correct: the embedding
/// vector lives solely in Qdrant keyed by `knowledge_id`, and the
/// embedding-engine marks a unit `'done'` only after that vector is upserted —
/// so an identical id guarantees the right vector is already retrievable.
///
/// Reuse is deliberately exact-identity only. A near-duplicate (footer/date
/// churn, whitespace) produces a different `content_hash`, hence a different
/// `knowledge_id`, and the reusable vector is keyed by the *old* id. Carrying
/// it forward would either mark a new id `'done'` with no vector under it
/// (silently unretrievable) or pin the stale old id and text — both break
/// correctness, and re-keying the Qdrant point belongs to the embedding-engine
/// across the plane boundary, not here. Exact reuse already spares every
/// unchanged chunk, which on a typical re-crawl is all but the one holding the
/// churned line.
fn plan_chunks(
    document_id: &str,
    chunks: &[Chunk],
    prior_done: &HashSet<String>,
) -> Vec<ChunkPlan> {
    chunks
        .iter()
        .map(|chunk| {
            let (hash, kid) = chunk_identity(document_id, chunk.index, &chunk.text);
            let reused = prior_done.contains(&kid);
            ChunkPlan { hash, kid, reused }
        })
        .collect()
}

pub async fn process_document(
    pool: &PgPool,
    event: &DocumentEvent,
    chunk_config: &ChunkConfig,
) -> anyhow::Result<BuildResult> {
    if event.zdr {
        anyhow::bail!("restrictive-ZDR document cannot enter durable indexing");
    }
    // Phase 1 RLS: one indexing event names exactly one org, so every read and
    // write below shares ONE org-scoped transaction rather than paying the
    // set_config/SET LOCAL ROLE round trip per statement. This build was already
    // one transaction for atomicity; scoping it changes only the role it runs
    // as. Each statement still binds `org_id` itself — the database policy is a
    // backstop against that filter being dropped or mis-edited later, not a
    // replacement for it. Two statements in this transaction carry no `org_id`
    // of their own and gain their isolation purely from the policy: the
    // `parent_window_text` refresh below (keyed by `knowledge_id` alone) and the
    // `chunk_lineage` insert, which the child-table migration isolates through
    // its `documents` parent.
    let mut tx = pg_org_scope::begin_org_scoped(pool, &event.org_id).await?;

    // The event is only a notification. Re-authorize the current canonical row
    // while holding its row lock, then keep every chunk/outbox read and write in
    // this transaction. A delayed event therefore cannot resurrect content after
    // a concurrent delete or restrictive-ZDR transition.
    let canonical: Option<(String, bool, String)> = sqlx::query_as(
        r#"
        SELECT content, deleted_at IS NOT NULL, zdr_classification
        FROM documents
        WHERE document_id = $1 AND org_id = $2
        FOR UPDATE
        "#,
    )
    .bind(&event.document_id)
    .bind(&event.org_id)
    .fetch_optional(&mut *tx)
    .await?;

    let Some((content, deleted, zdr_classification)) = canonical else {
        tracing::warn!(document_id = %event.document_id, "canonical document missing; stale indexing event ignored");
        tx.rollback().await?;
        return Ok(BuildResult {
            document_id: event.document_id.clone(),
            chunks_created: 0,
            knowledge_ids: vec![],
            pending_knowledge_ids: vec![],
            orphaned_knowledge_ids: vec![],
        });
    };
    if !canonical_document_is_indexable(deleted, &zdr_classification) {
        tracing::info!(document_id = %event.document_id, "deleted or restrictive canonical document; stale indexing event ignored");
        tx.rollback().await?;
        return Ok(BuildResult {
            document_id: event.document_id.clone(),
            chunks_created: 0,
            knowledge_ids: vec![],
            pending_knowledge_ids: vec![],
            orphaned_knowledge_ids: vec![],
        });
    }

    let normalized = normalize(&content);
    let chunks = chunk_text(&normalized, chunk_config);

    // Capture the prior knowledge units up front so an update that produces zero
    // chunks (e.g. content cleared) still purges the prior vectors, and so we can
    // reuse embeddings for chunks that are unchanged on re-crawl. We read
    // `embedding_status` because only a 'done' unit has a committed Qdrant vector
    // to reuse (the embedding-engine marks 'done' only after the upsert).
    let old_units: Vec<(String, i32, String, String)> = sqlx::query_as(
        "SELECT knowledge_id, chunk_index, content_hash, embedding_status FROM knowledge_units WHERE document_id = $1 AND org_id = $2 FOR UPDATE",
    )
    .bind(&event.document_id)
    .bind(&event.org_id)
    .fetch_all(&mut *tx)
    .await
    .unwrap_or_default();
    let old_kid_ids: Vec<String> = old_units.iter().map(|(kid, _, _, _)| kid.clone()).collect();
    // Prior chunks whose vector is already live in Qdrant, keyed by their exact
    // (content-derived) knowledge_id. Only these are reuse candidates.
    let prior_done: HashSet<String> = old_units
        .iter()
        .filter(|(_, _, _, status)| status == "done")
        .map(|(kid, _, _, _)| kid.clone())
        .collect();

    if chunks.is_empty() {
        tracing::warn!(document_id = %event.document_id, "no chunks produced");
        if !old_units.is_empty() {
            crate::outbox::enqueue_intent(
                &mut tx,
                &event.org_id,
                &event.document_id,
                &old_kid_ids,
                event.user_id.as_deref(),
                &event.idempotency_key,
                event.zdr,
            )
            .await?;
            sqlx::query("DELETE FROM knowledge_units WHERE document_id = $1 AND org_id = $2")
                .bind(&event.document_id)
                .bind(&event.org_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        return Ok(BuildResult {
            document_id: event.document_id.clone(),
            chunks_created: 0,
            knowledge_ids: vec![],
            pending_knowledge_ids: vec![],
            // Every prior chunk is now orphaned (document has no content).
            orphaned_knowledge_ids: old_kid_ids,
        });
    }

    let reindex = !old_units.is_empty();

    // Plan every chunk's identity in memory (chunk_identity is pure), marking
    // which ones can reuse an existing embedding. See `plan_chunks` for why
    // reuse is exact-identity only.
    let plans = plan_chunks(&event.document_id, &chunks, &prior_done);
    let reused_kids: Vec<String> = plans
        .iter()
        .filter(|plan| plan.reused)
        .map(|plan| plan.kid.clone())
        .collect();

    // Delete only the prior rows we are NOT reusing (changed, removed, or
    // never-embedded chunks). Reused 'done' rows stay put, so their Qdrant
    // vector and embedded_at timestamp are preserved untouched. When nothing is
    // reused, `<> ALL('{}')` is TRUE for every row, reproducing the original
    // delete-all-then-reinsert behavior exactly.
    sqlx::query(
        "DELETE FROM knowledge_units WHERE document_id = $1 AND org_id = $2 AND knowledge_id <> ALL($3::text[])",
    )
    .bind(&event.document_id)
    .bind(&event.org_id)
    .bind(&reused_kids)
    .execute(&mut *tx)
    .await?;

    let mut knowledge_ids = Vec::with_capacity(chunks.len());
    // The chunks the embedding-engine must embed — reused chunks are omitted so
    // the stream never re-publishes an already-embedded unit.
    let mut pending_knowledge_ids = Vec::new();

    for (chunk, plan) in chunks.iter().zip(plans.iter()) {
        if plan.reused {
            // Row already exists and is 'done'; its vector is live in Qdrant
            // under this identical id. Nothing to (re-)embed. `parent_text`
            // (P2-2) is NOT embedding-derived, though -- it is built from
            // this chunk's *neighbors*, which can change even when this
            // chunk's own content did not, so it is refreshed unconditionally
            // here rather than left stale for the lifetime of the reuse.
            sqlx::query(
                "UPDATE knowledge_units SET parent_window_text = $1 WHERE knowledge_id = $2",
            )
            .bind(&chunk.parent_text)
            .bind(&plan.kid)
            .execute(&mut *tx)
            .await?;
            tracing::debug!(
                document_id = %event.document_id,
                chunk_index = chunk.index,
                "unchanged chunk; reusing existing embedding (skip re-embed)"
            );
            knowledge_ids.push(plan.kid.clone());
            continue;
        }

        let metadata = serde_json::json!({
            "title": event.title,
            "source": event.source,
            "type": event.doc_type,
            "chunk_tokens": chunk.estimated_tokens,
        });

        sqlx::query(
            r#"
            INSERT INTO knowledge_units (
                knowledge_id, document_id, org_id, chunk_index, text,
                embedding_status, content_hash, chunk_version, metadata,
                parent_window_text
            ) VALUES ($1, $2, $3, $4, $5, 'pending', $6, '1', $7, $8)
            ON CONFLICT (knowledge_id) DO UPDATE
                SET parent_window_text = EXCLUDED.parent_window_text
            "#,
        )
        .bind(&plan.kid)
        .bind(&event.document_id)
        .bind(&event.org_id)
        .bind(chunk.index as i32)
        .bind(&chunk.text)
        .bind(&plan.hash)
        .bind(&metadata)
        .bind(&chunk.parent_text)
        .execute(&mut *tx)
        .await?;

        knowledge_ids.push(plan.kid.clone());
        pending_knowledge_ids.push(plan.kid.clone());
    }

    // Persist chunk lineage on reindex
    if reindex {
        for (old_kid, old_idx, old_hash, _status) in &old_units {
            let new_kid = knowledge_ids
                .get(*old_idx as usize)
                .unwrap_or(&knowledge_ids[0]);
            sqlx::query(
                r#"
                INSERT INTO chunk_lineage (
                    document_id, old_knowledge_id, new_knowledge_id,
                    old_chunk_index, old_content_hash, reason
                ) VALUES ($1, $2, $3, $4, $5, 'reindex')
                ON CONFLICT DO NOTHING
                "#,
            )
            .bind(&event.document_id)
            .bind(old_kid)
            .bind(new_kid)
            .bind(old_idx)
            .bind(old_hash)
            .execute(&mut *tx)
            .await?;
        }
        tracing::info!(
            document_id = %event.document_id,
            old_chunks = old_units.len(),
            new_chunks = knowledge_ids.len(),
            "chunk lineage recorded"
        );
    }

    // Move the document to 'processing' while its pending chunks await
    // embedding. If every chunk was reused there is nothing to embed, so the
    // embedding-engine (triggered per pending chunk) never runs for this event —
    // mark the doc 'indexed' here since all its vectors are already live in
    // Qdrant. In the partial case the embedding-engine flips it to 'indexed'
    // once the pending batch lands, because its reused peers are already 'done'.
    let doc_status = if pending_knowledge_ids.is_empty() {
        "indexed"
    } else {
        "processing"
    };
    sqlx::query("UPDATE documents SET status = $3 WHERE document_id = $1 AND org_id = $2")
        .bind(&event.document_id)
        .bind(&event.org_id)
        .bind(doc_status)
        .execute(&mut *tx)
        .await?;

    tracing::info!(
        document_id = %event.document_id,
        chunks = chunks.len(),
        "knowledge units built"
    );

    let orphaned_knowledge_ids = orphaned_ids(&old_kid_ids, &knowledge_ids);
    if !orphaned_knowledge_ids.is_empty() {
        crate::outbox::enqueue_intent(
            &mut tx,
            &event.org_id,
            &event.document_id,
            &orphaned_knowledge_ids,
            event.user_id.as_deref(),
            &event.idempotency_key,
            event.zdr,
        )
        .await?;
    }
    tx.commit().await?;

    Ok(BuildResult {
        document_id: event.document_id.clone(),
        chunks_created: chunks.len(),
        knowledge_ids,
        pending_knowledge_ids,
        orphaned_knowledge_ids,
    })
}

// Non-test helper items intentionally follow this module.
#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::{
        canonical_document_is_indexable, chunk_identity, orphaned_ids, plan_chunks,
        process_document, DocumentEvent,
    };
    use crate::chunker::{Chunk, ChunkConfig};
    use std::collections::HashSet;

    fn chunk(index: usize, text: &str) -> Chunk {
        Chunk {
            index,
            text: text.to_string(),
            estimated_tokens: text.len() / 4 + 1,
            parent_text: None,
        }
    }

    /// Creates the RLS runtime role and grants it the fixture's schema.
    ///
    /// # Why a test fixture needs a database role at all
    ///
    /// `process_document` opens its transaction through
    /// `pg_org_scope::begin_org_scoped` (see `services/pg-org-scope-rs`), which
    /// issues `SET LOCAL ROLE dataplane_app` — so every DB-gated test below
    /// runs its whole build as that role.
    ///
    /// In production the role is created by
    /// `infra/postgres/migrations/20260809120000_org_rls_isolation.sql`. These
    /// tests, however, build their own minimal schema with `CREATE TABLE` on a
    /// bare disposable database and never run the migrations — so without this
    /// helper every one of them fails at runtime with:
    ///
    /// ```text
    /// error returned from database: role "dataplane_app" does not exist
    /// ```
    ///
    /// That failure is invisible to a normal `cargo test` run, because every
    /// test that would hit it is `#[ignore]`d behind `TEST_DATABASE_URL`. Do not
    /// delete this as boilerplate: removing it silently disables the integration
    /// tests that cover stale-event refusal, embedding reuse, and parent-window
    /// refresh.
    ///
    /// # Grants only — deliberately
    ///
    /// This creates the role and grants it access, but does **not** enable
    /// row-level security or install any policy. That is the point: these
    /// fixtures keep asserting exactly what they asserted before RLS existed —
    /// that each query's own `org_id` predicate does the filtering. Having them
    /// enforce RLS too would be a strictly stronger test, but it changes what
    /// the suite covers, so it belongs in its own deliberate change.
    ///
    /// # Ordering
    ///
    /// Must be called **after** the fixture's `CREATE TABLE` statements:
    /// `GRANT ... ON ALL TABLES` applies to the tables that exist when it runs,
    /// not to ones created later.
    async fn grant_rls_runtime_role(pool: &sqlx::PgPool) {
        sqlx::raw_sql(
            r#"
            -- Roles are cluster-wide, so a concurrent test in the same binary
            -- may win the race to create it, and a plain IF NOT EXISTS check has
            -- a TOCTOU window. BOTH handlers are required, and this was verified
            -- against a real database rather than assumed:
            --   * duplicate_object (42710) is what a *sequential* re-run raises,
            --     once the role is already committed.
            --   * unique_violation (23505) is what an actual *concurrent* race
            --     raises — the losing backend faults on pg_authid_rolname_index
            --     before the duplicate_object check is ever reached. Catching
            --     only duplicate_object leaves the exact race this guard exists
            --     for unhandled; with the role absent and tests running
            --     multi-threaded it fails with `duplicate key value violates
            --     unique constraint "pg_authid_rolname_index"`.
            DO $role$
            BEGIN
                CREATE ROLE dataplane_app NOLOGIN NOSUPERUSER NOBYPASSRLS;
            EXCEPTION WHEN duplicate_object OR unique_violation THEN
                NULL;
            END
            $role$;

            GRANT USAGE ON SCHEMA public TO dataplane_app;
            GRANT SELECT, INSERT, UPDATE, DELETE
                ON ALL TABLES IN SCHEMA public TO dataplane_app;
            GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO dataplane_app;
            "#,
        )
        .execute(pool)
        .await
        .expect("create and grant the RLS runtime role");
    }

    #[test]
    fn canonical_document_gate_fails_closed_for_deleted_restricted_and_unknown_rows() {
        for classification in ["internal", "public", "sensitive"] {
            assert!(canonical_document_is_indexable(false, classification));
        }
        for classification in ["restricted", "", "future-policy", " RESTRICTED "] {
            assert!(!canonical_document_is_indexable(false, classification));
        }
        assert!(!canonical_document_is_indexable(true, "internal"));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    async fn delayed_events_for_deleted_or_restricted_documents_do_zero_durable_work() {
        let database_url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must point to disposable PostgreSQL");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("disposable postgres");
        sqlx::raw_sql(
            r#"
            DROP TABLE IF EXISTS chunk_lineage, index_deletion_outbox, knowledge_units, documents;
            CREATE TABLE documents (
              document_id TEXT PRIMARY KEY, org_id TEXT NOT NULL, content TEXT NOT NULL,
              deleted_at TIMESTAMPTZ, zdr_classification TEXT NOT NULL
            );
            CREATE TABLE knowledge_units (
              knowledge_id TEXT PRIMARY KEY, document_id TEXT NOT NULL, org_id TEXT NOT NULL,
              parent_window_text TEXT
            );
            CREATE TABLE index_deletion_outbox (outbox_id BIGSERIAL PRIMARY KEY);
            INSERT INTO documents VALUES
              ('fixture-restricted','fixture-org','canonical restricted content',NULL,'restricted'),
              ('fixture-unknown','fixture-org','canonical unknown content',NULL,'future-policy'),
              ('fixture-deleted','fixture-org','canonical deleted content',NOW(),'internal');
            INSERT INTO knowledge_units VALUES
              ('existing-restricted','fixture-restricted','fixture-org'),
              ('existing-unknown','fixture-unknown','fixture-org'),
              ('existing-deleted','fixture-deleted','fixture-org');
            "#,
        )
        .execute(&pool)
        .await
        .expect("minimal disposable schema");
        grant_rls_runtime_role(&pool).await;

        for document_id in ["fixture-restricted", "fixture-unknown", "fixture-deleted"] {
            let result = process_document(
                &pool,
                &DocumentEvent {
                    document_id: document_id.into(),
                    org_id: "fixture-org".into(),
                    title: "stale event title".into(),
                    source: "fixture".into(),
                    doc_type: "text".into(),
                    user_id: Some("fixture-user".into()),
                    idempotency_key: format!("stale-{document_id}"),
                    zdr: false,
                },
                &ChunkConfig::default(),
            )
            .await
            .expect("stale event is acknowledged as a no-op");
            assert_eq!(result.chunks_created, 0);
            assert!(result.knowledge_ids.is_empty());
            assert!(result.orphaned_knowledge_ids.is_empty());
        }

        let counts: (i64, i64) = sqlx::query_as(
            "SELECT
               (SELECT COUNT(*) FROM knowledge_units),
               (SELECT COUNT(*) FROM index_deletion_outbox)",
        )
        .fetch_one(&pool)
        .await
        .expect("durable counts");
        assert_eq!(counts, (3, 0));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    async fn re_crawl_reuses_unchanged_embeddings_and_re_embeds_only_changed_chunks() {
        let database_url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must point to disposable PostgreSQL");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("disposable postgres");
        sqlx::raw_sql(
            r#"
            DROP TABLE IF EXISTS chunk_lineage, index_deletion_outbox, knowledge_units, documents;
            CREATE TABLE documents (
              document_id TEXT PRIMARY KEY, org_id TEXT NOT NULL, content TEXT NOT NULL,
              deleted_at TIMESTAMPTZ, zdr_classification TEXT NOT NULL, status TEXT
            );
            CREATE TABLE knowledge_units (
              knowledge_id TEXT PRIMARY KEY, document_id TEXT NOT NULL, org_id TEXT NOT NULL,
              chunk_index INTEGER NOT NULL, text TEXT NOT NULL,
              embedding_status TEXT NOT NULL DEFAULT 'pending', content_hash TEXT,
              chunk_version TEXT NOT NULL DEFAULT '1', metadata JSONB NOT NULL DEFAULT '{}',
              embedding_model TEXT, embedded_at TIMESTAMPTZ, parent_window_text TEXT,
              created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            CREATE TABLE index_deletion_outbox (
              outbox_id BIGSERIAL PRIMARY KEY, org_id TEXT NOT NULL, document_id TEXT NOT NULL,
              knowledge_ids JSONB NOT NULL, user_id TEXT, idempotency_key TEXT UNIQUE NOT NULL,
              created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            CREATE TABLE chunk_lineage (
              id BIGSERIAL PRIMARY KEY, document_id TEXT NOT NULL, old_knowledge_id TEXT NOT NULL,
              new_knowledge_id TEXT NOT NULL, old_chunk_index INTEGER, old_content_hash TEXT, reason TEXT
            );
            INSERT INTO documents VALUES
              ('doc-reuse','org-reuse',E'alpha\n\nbeta',NULL,'internal','pending');
            "#,
        )
        .execute(&pool)
        .await
        .expect("disposable reuse schema");
        grant_rls_runtime_role(&pool).await;

        // Two paragraphs, tiny budget → exactly two chunks: "alpha" (0), "beta" (1).
        let cfg = ChunkConfig {
            chunk_size: 2,
            chunk_overlap: 0,
            parent_chunk_size: None,
        };
        let event = |key: &str| DocumentEvent {
            document_id: "doc-reuse".into(),
            org_id: "org-reuse".into(),
            title: "Reuse".into(),
            source: "fixture".into(),
            doc_type: "text".into(),
            user_id: Some("fixture-user".into()),
            idempotency_key: key.into(),
            zdr: false,
        };

        // ── Build 1: first ingest — every chunk is new and must be embedded. ──
        let r1 = process_document(&pool, &event("reuse-build-1"), &cfg)
            .await
            .expect("first build");
        assert_eq!(r1.chunks_created, 2);
        assert_eq!(r1.knowledge_ids.len(), 2);
        assert_eq!(
            r1.pending_knowledge_ids, r1.knowledge_ids,
            "first build must embed every chunk"
        );
        assert!(r1.orphaned_knowledge_ids.is_empty());
        let alpha_kid = r1.knowledge_ids[0].clone();
        let beta_kid = r1.knowledge_ids[1].clone();

        // Simulate the embedding-engine finishing: vectors upserted, rows 'done'.
        sqlx::query("UPDATE knowledge_units SET embedding_status = 'done', embedded_at = NOW(), embedding_model = 'test-embed'")
            .execute(&pool)
            .await
            .expect("mark done");
        let embedded_at = |kid: String| {
            let pool = pool.clone();
            async move {
                let row: (String, Option<String>, Option<String>) = sqlx::query_as(
                    "SELECT embedding_status, embedded_at::text, embedding_model FROM knowledge_units WHERE knowledge_id = $1",
                )
                .bind(&kid)
                .fetch_one(&pool)
                .await
                .expect("row present");
                row
            }
        };
        let alpha_done_before = embedded_at(alpha_kid.clone()).await;
        let beta_done_before = embedded_at(beta_kid.clone()).await;
        assert_eq!(alpha_done_before.0, "done");
        assert!(alpha_done_before.1.is_some());

        // ── Build 2: identical re-crawl — every chunk is reused, none embedded. ──
        let r2 = process_document(&pool, &event("reuse-build-2"), &cfg)
            .await
            .expect("no-op re-crawl");
        assert_eq!(
            r2.knowledge_ids,
            vec![alpha_kid.clone(), beta_kid.clone()],
            "unchanged chunks keep their exact ids"
        );
        assert!(
            r2.pending_knowledge_ids.is_empty(),
            "an unchanged re-crawl must embed nothing"
        );
        assert!(r2.orphaned_knowledge_ids.is_empty());
        // Rows are untouched: same 'done' status, same embedded_at, same model.
        assert_eq!(embedded_at(alpha_kid.clone()).await, alpha_done_before);
        assert_eq!(embedded_at(beta_kid.clone()).await, beta_done_before);
        let doc_status: (Option<String>,) =
            sqlx::query_as("SELECT status FROM documents WHERE document_id = 'doc-reuse'")
                .fetch_one(&pool)
                .await
                .expect("doc row");
        assert_eq!(
            doc_status.0.as_deref(),
            Some("indexed"),
            "a fully-reused doc is indexed immediately (embedding-engine never runs)"
        );

        // ── Build 3: chunk 0 changes; chunk 1 ("beta") is unchanged. ──
        sqlx::query(
            "UPDATE documents SET content = E'ALPHA\n\nbeta' WHERE document_id = 'doc-reuse'",
        )
        .execute(&pool)
        .await
        .expect("edit content");
        let r3 = process_document(&pool, &event("reuse-build-3"), &cfg)
            .await
            .expect("partial re-crawl");
        assert_eq!(r3.knowledge_ids.len(), 2);
        // "beta" keeps its id and is reused; the changed chunk gets a fresh id.
        assert_eq!(
            r3.knowledge_ids[1], beta_kid,
            "unchanged neighbor is reused"
        );
        let alpha2_kid = r3.knowledge_ids[0].clone();
        assert_ne!(alpha2_kid, alpha_kid, "changed chunk gets a new id");
        assert_eq!(
            r3.pending_knowledge_ids,
            vec![alpha2_kid.clone()],
            "only the changed chunk is (re)embedded"
        );
        assert_eq!(
            r3.orphaned_knowledge_ids,
            vec![alpha_kid.clone()],
            "the superseded chunk id is orphaned for Qdrant purge"
        );
        // Unchanged neighbor's embedding is fully preserved.
        assert_eq!(embedded_at(beta_kid.clone()).await, beta_done_before);
        // The changed chunk is back to 'pending' awaiting a fresh embedding.
        let alpha2 = embedded_at(alpha2_kid.clone()).await;
        assert_eq!(alpha2.0, "pending");
        assert!(alpha2.1.is_none(), "a pending chunk has no embedded_at");
        // The old chunk-0 row is gone; exactly the two current chunks remain.
        let (rows, orphan_outbox): (i64, i64) = sqlx::query_as(
            "SELECT
               (SELECT COUNT(*) FROM knowledge_units WHERE document_id = 'doc-reuse'),
               (SELECT COUNT(*) FROM index_deletion_outbox WHERE document_id = 'doc-reuse')",
        )
        .fetch_one(&pool)
        .await
        .expect("counts");
        assert_eq!(rows, 2, "only the two current chunks persist");
        assert_eq!(orphan_outbox, 1, "one orphan-deletion intent was enqueued");
        let doc_status_after: (Option<String>,) =
            sqlx::query_as("SELECT status FROM documents WHERE document_id = 'doc-reuse'")
                .fetch_one(&pool)
                .await
                .expect("doc row");
        assert_eq!(
            doc_status_after.0.as_deref(),
            Some("processing"),
            "a doc with a pending chunk stays 'processing' until the embed lands"
        );
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    async fn parent_window_text_refreshes_on_reuse_even_though_the_embedding_does_not() {
        let database_url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must point to disposable PostgreSQL");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("disposable postgres");
        sqlx::raw_sql(
            r#"
            DROP TABLE IF EXISTS chunk_lineage, index_deletion_outbox, knowledge_units, documents;
            CREATE TABLE documents (
              document_id TEXT PRIMARY KEY, org_id TEXT NOT NULL, content TEXT NOT NULL,
              deleted_at TIMESTAMPTZ, zdr_classification TEXT NOT NULL, status TEXT
            );
            CREATE TABLE knowledge_units (
              knowledge_id TEXT PRIMARY KEY, document_id TEXT NOT NULL, org_id TEXT NOT NULL,
              chunk_index INTEGER NOT NULL, text TEXT NOT NULL,
              embedding_status TEXT NOT NULL DEFAULT 'pending', content_hash TEXT,
              chunk_version TEXT NOT NULL DEFAULT '1', metadata JSONB NOT NULL DEFAULT '{}',
              embedding_model TEXT, embedded_at TIMESTAMPTZ, parent_window_text TEXT,
              created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            CREATE TABLE index_deletion_outbox (
              outbox_id BIGSERIAL PRIMARY KEY, org_id TEXT NOT NULL, document_id TEXT NOT NULL,
              knowledge_ids JSONB NOT NULL, user_id TEXT, idempotency_key TEXT UNIQUE NOT NULL,
              created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            CREATE TABLE chunk_lineage (
              id BIGSERIAL PRIMARY KEY, document_id TEXT NOT NULL, old_knowledge_id TEXT NOT NULL,
              new_knowledge_id TEXT NOT NULL, old_chunk_index INTEGER, old_content_hash TEXT, reason TEXT
            );
            INSERT INTO documents VALUES
              ('doc-parent','org-parent',E'one.\n\ntwo.\n\nthree.',NULL,'internal','pending');
            "#,
        )
        .execute(&pool)
        .await
        .expect("disposable parent-window schema");
        grant_rls_runtime_role(&pool).await;

        // Tight chunk budget, huge parent budget: three one-sentence chunks,
        // each one's parent window pulls in both neighbors.
        let cfg = ChunkConfig {
            chunk_size: 2,
            chunk_overlap: 0,
            parent_chunk_size: Some(100),
        };
        let event = |key: &str| DocumentEvent {
            document_id: "doc-parent".into(),
            org_id: "org-parent".into(),
            title: "Parent window".into(),
            source: "fixture".into(),
            doc_type: "text".into(),
            user_id: Some("fixture-user".into()),
            idempotency_key: key.into(),
            zdr: false,
        };
        let parent_text_of = |kid: String| {
            let pool = pool.clone();
            async move {
                let row: (Option<String>,) = sqlx::query_as(
                    "SELECT parent_window_text FROM knowledge_units WHERE knowledge_id = $1",
                )
                .bind(&kid)
                .fetch_one(&pool)
                .await
                .expect("row present");
                row.0
            }
        };

        // ── Build 1: first ingest. ──
        let r1 = process_document(&pool, &event("parent-build-1"), &cfg)
            .await
            .expect("first build");
        assert_eq!(r1.knowledge_ids.len(), 3);
        let (one_kid, two_kid, three_kid) = (
            r1.knowledge_ids[0].clone(),
            r1.knowledge_ids[1].clone(),
            r1.knowledge_ids[2].clone(),
        );
        assert_eq!(
            parent_text_of(two_kid.clone()).await,
            Some("one. two. three.".to_string()),
            "middle chunk's parent window pulls in both neighbors"
        );

        sqlx::query("UPDATE knowledge_units SET embedding_status = 'done', embedded_at = NOW()")
            .execute(&pool)
            .await
            .expect("mark done");
        let embedded_at_of = |kid: String| {
            let pool = pool.clone();
            async move {
                let row: (String, Option<String>) = sqlx::query_as(
                    "SELECT embedding_status, embedded_at::text FROM knowledge_units WHERE knowledge_id = $1",
                )
                .bind(&kid)
                .fetch_one(&pool)
                .await
                .expect("row present");
                row
            }
        };
        let two_done_before = embedded_at_of(two_kid.clone()).await;
        assert_eq!(two_done_before.0, "done");

        // ── Build 2: only the FIRST chunk's content changes. The middle chunk
        // ("two.") is byte-identical and must be reused (no re-embed) even
        // though its neighbor — and therefore its parent window — changed. ──
        sqlx::query("UPDATE documents SET content = E'ONE.\n\ntwo.\n\nthree.' WHERE document_id = 'doc-parent'")
            .execute(&pool)
            .await
            .expect("edit content");
        let r2 = process_document(&pool, &event("parent-build-2"), &cfg)
            .await
            .expect("partial re-crawl");
        assert_eq!(
            r2.knowledge_ids[1], two_kid,
            "unchanged middle chunk keeps its id"
        );
        assert_ne!(
            r2.knowledge_ids[0], one_kid,
            "changed first chunk gets a new id"
        );
        assert_eq!(
            r2.knowledge_ids[2], three_kid,
            "unchanged last chunk keeps its id"
        );
        assert_eq!(
            r2.pending_knowledge_ids,
            vec![r2.knowledge_ids[0].clone()],
            "only the changed chunk is (re)embedded"
        );

        // The embedding itself is untouched by the reuse path...
        assert_eq!(
            embedded_at_of(two_kid.clone()).await,
            two_done_before,
            "reused chunk's embedding state must not change"
        );
        // ...but its parent window text reflects the new neighbor.
        assert_eq!(
            parent_text_of(two_kid.clone()).await,
            Some("ONE. two. three.".to_string()),
            "reused chunk's parent window must still pick up a changed neighbor"
        );
    }

    #[test]
    fn orphaned_ids_returns_removed_chunks() {
        let old = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let new = vec!["a".to_string(), "c".to_string()]; // b removed/changed
        assert_eq!(orphaned_ids(&old, &new), vec!["b".to_string()]);
    }

    #[test]
    fn orphaned_ids_empty_when_all_retained() {
        let old = vec!["a".to_string(), "b".to_string()];
        let new = vec!["a".to_string(), "b".to_string(), "d".to_string()];
        assert!(orphaned_ids(&old, &new).is_empty());
    }

    #[test]
    fn orphaned_ids_all_when_none_retained() {
        let old = vec!["a".to_string(), "b".to_string()];
        let new = vec!["x".to_string()];
        assert_eq!(
            orphaned_ids(&old, &new),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn orphaned_ids_empty_on_first_build() {
        let old: Vec<String> = vec![];
        let new = vec!["a".to_string()];
        assert!(orphaned_ids(&old, &new).is_empty());
    }

    #[test]
    fn chunk_identity_is_pure_and_index_scoped() {
        // Deterministic: identical inputs produce an identical (hash, id) pair,
        // so the loop's knowledge_ids are a pure function of the chunks.
        let (h1, k1) = chunk_identity("doc-1", 0, "hello world");
        let (h2, k2) = chunk_identity("doc-1", 0, "hello world");
        assert_eq!((h1.as_str(), k1.as_str()), (h2.as_str(), k2.as_str()));

        // Same text at a different chunk_index yields a different knowledge_id,
        // so distinct chunks in one build never share an id — the removed
        // per-chunk "skip if exists" COUNT(*) could not have deduped within a
        // single build even before the DELETE made it unreachable.
        let (_, k_next_index) = chunk_identity("doc-1", 1, "hello world");
        assert_ne!(k1, k_next_index);

        // Different content yields a different hash and id.
        let (h_diff, k_diff) = chunk_identity("doc-1", 0, "different content");
        assert_ne!(h1, h_diff);
        assert_ne!(k1, k_diff);
    }

    #[test]
    fn plan_chunks_reuses_only_unchanged_chunks_with_a_prior_done_vector() {
        // Chunk 0 was already embedded ('done'); chunk 1 is brand new.
        let (_, kid0) = chunk_identity("doc-x", 0, "alpha");
        let prior_done: HashSet<String> = [kid0.clone()].into_iter().collect();

        let plans = plan_chunks("doc-x", &[chunk(0, "alpha"), chunk(1, "beta")], &prior_done);

        // Unchanged, already-embedded chunk → reuse the exact same id, no embed.
        assert!(plans[0].reused, "unchanged 'done' chunk must be reused");
        assert_eq!(plans[0].kid, kid0);
        // No prior vector for chunk 1 → must be (re)embedded.
        assert!(
            !plans[1].reused,
            "chunk with no prior 'done' row must re-embed"
        );
    }

    #[test]
    fn plan_chunks_re_embeds_on_content_change_or_index_shift() {
        // A prior 'done' vector exists for "alpha" at chunk_index 0.
        let (_, kid_idx0) = chunk_identity("doc-x", 0, "alpha");
        let prior_done: HashSet<String> = [kid_idx0].into_iter().collect();

        // Same text, new position: the id embeds chunk_index, so the id differs
        // and the reusable Qdrant vector is keyed by the old id → must re-embed.
        let moved = plan_chunks("doc-x", &[chunk(1, "alpha")], &prior_done);
        assert!(
            !moved[0].reused,
            "same text at a new index gets a new id — must re-embed"
        );

        // Same position, changed text: new content_hash → new id → must re-embed.
        let changed = plan_chunks("doc-x", &[chunk(0, "alpha edited")], &prior_done);
        assert!(
            !changed[0].reused,
            "changed content yields a new id — must re-embed"
        );
    }

    #[test]
    fn plan_chunks_ignores_prior_units_that_are_not_done() {
        // The prior unit exists but was never successfully embedded (empty set of
        // 'done' ids), so its vector cannot be reused even if the text matches.
        let prior_done: HashSet<String> = HashSet::new();
        let plans = plan_chunks("doc-x", &[chunk(0, "alpha")], &prior_done);
        assert!(
            !plans[0].reused,
            "a chunk with no prior 'done' unit must be embedded"
        );
    }
}
