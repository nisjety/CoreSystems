//! The pipeline's statements. Everything the batch reads or writes about a unit's
//! context, its vector's provenance and its document goes through here.
//!
//! ## Tenancy
//!
//! A batch mixes organizations on purpose and runs on the pool every Data Plane
//! service connects with, the superuser, for which row-level security is inert
//! (`pg_org_scope`'s header says why). So no statement here may rely on RLS: every one
//! names the organization of every unit it touches, in the join or the predicate, and a
//! unit that a caller claims under the wrong organization is simply not found. The
//! functions take a connection rather than a pool for the same reason in the other
//! direction: the reconciler runs the same statements inside `begin_org_scoped` as the
//! restricted application role, where RLS and the explicit predicate must agree, and a
//! test does exactly that.
//!
//! ## Compare-and-swap
//!
//! The writes of a context carry the input hash the caller read the unit with
//! (`expected_hash`, possibly NULL) and match only while the row still holds it. A worker
//! that read the unit earlier than another finished it writes nothing, and the caller
//! reads back what is in force instead of assuming it won.
//!
//! ## What is never written here
//!
//! `embedding_status = 'failed'`. The terminal failure belongs to the stream consumer
//! alone, in the one place that knows the broker's delivery count (`mark_units_failed`,
//! D18). A context that could not be made is a `context_status`, never an embedding
//! failure.

use std::collections::HashSet;
use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgPool, Row};

/// A unit named the way every statement here names it: with its organization.
#[derive(Clone, Copy, Debug)]
pub(super) struct UnitRef<'a> {
    pub knowledge_id: &'a str,
    pub org_id: &'a str,
}

fn ids<'a>(units: impl Iterator<Item = UnitRef<'a>>) -> (Vec<&'a str>, Vec<&'a str>) {
    units.map(|unit| (unit.knowledge_id, unit.org_id)).unzip()
}

/// A unit as the database holds it: the part of its row the context pipeline reads.
#[derive(Clone, Debug)]
pub(super) struct Stored {
    pub knowledge_id: String,
    /// The document the unit belongs to, as the database has it: what a caller claims is
    /// checked against it.
    pub document_id: String,
    pub status: Option<String>,
    /// The stored context. A blank one is no context.
    pub context: Option<String>,
    pub input_hash: Option<String>,
    pub prompt_version: Option<String>,
    pub model: Option<String>,
    pub attempts: i32,
}

/// The units that exist, under the organization they were named with. A unit that is
/// not returned is not there: deleted while its event waited, or claimed by the wrong
/// organization. (Which document it belongs to is returned for the caller to check
/// against what it was told.)
pub(super) async fn load(
    conn: &mut PgConnection,
    units: &[UnitRef<'_>],
) -> sqlx::Result<Vec<Stored>> {
    if units.is_empty() {
        return Ok(Vec::new());
    }
    let (kids, orgs) = ids(units.iter().copied());
    let rows = sqlx::query(
        r#"
        SELECT ku.knowledge_id, ku.document_id, ku.context_status, ku.chunk_context,
               ku.context_input_hash, ku.context_prompt_version, ku.context_model,
               ku.context_attempts
          FROM knowledge_units AS ku
          JOIN unnest($1::text[], $2::text[]) AS t(knowledge_id, org_id)
            ON ku.knowledge_id = t.knowledge_id AND ku.org_id = t.org_id
        "#,
    )
    .bind(&kids)
    .bind(&orgs)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .iter()
        .map(|row| Stored {
            knowledge_id: row.get("knowledge_id"),
            document_id: row.get("document_id"),
            status: row.get("context_status"),
            context: row
                .get::<Option<String>, _>("chunk_context")
                .filter(|context| !context.trim().is_empty()),
            input_hash: row.get("context_input_hash"),
            prompt_version: row.get("context_prompt_version"),
            model: row.get("context_model"),
            attempts: row.get("context_attempts"),
        })
        .collect())
}

/// A document, as far as generating contexts for its units needs it.
#[derive(Debug)]
pub(super) struct DocumentRow {
    pub document_id: String,
    pub org_id: String,
    pub title: String,
    pub source: String,
    pub date: Option<DateTime<Utc>>,
    pub content: String,
    /// The document is `zdr_classification = 'restricted'` NOW. The flag a unit's event was
    /// queued with was read by the stream earlier, and a batch can hold it for most of a
    /// minute: a document that became restricted since is read again here, where its text
    /// would be put in a prompt.
    pub restricted: bool,
    /// How many units the document has, so a chunk whose text cannot be found in a long
    /// document is placed exactly instead of estimated.
    pub total_chunks: i64,
}

/// The live documents of the given `(document_id, org_id)` pairs. One copy of each
/// document's text, however many of its chunks are in the batch.
pub(super) async fn documents(
    conn: &mut PgConnection,
    pairs: &[(&str, &str)],
) -> sqlx::Result<Vec<DocumentRow>> {
    if pairs.is_empty() {
        return Ok(Vec::new());
    }
    let (document_ids, org_ids): (Vec<&str>, Vec<&str>) = pairs.iter().copied().unzip();
    let rows = sqlx::query(
        r#"
        SELECT d.document_id, d.org_id, d.title, d.source, d.document_date, d.content,
               (d.zdr_classification = 'restricted') AS restricted,
               (SELECT count(*) FROM knowledge_units AS k
                 WHERE k.document_id = d.document_id AND k.org_id = d.org_id) AS total_chunks
          FROM documents AS d
          JOIN unnest($1::text[], $2::text[]) AS t(document_id, org_id)
            ON d.document_id = t.document_id AND d.org_id = t.org_id
         WHERE d.deleted_at IS NULL
        "#,
    )
    .bind(&document_ids)
    .bind(&org_ids)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .iter()
        .map(|row| DocumentRow {
            document_id: row.get("document_id"),
            org_id: row.get("org_id"),
            title: row.get("title"),
            source: row.get("source"),
            date: row.get("document_date"),
            content: row.get("content"),
            restricted: row.get("restricted"),
            total_chunks: row.get("total_chunks"),
        })
        .collect())
}

/// A context that was generated, with everything that says where it came from.
#[derive(Debug)]
pub(super) struct ReadyWrite<'a> {
    pub unit: UnitRef<'a>,
    /// The input hash the unit was read with; the write applies only while the row still
    /// holds it.
    pub expected_hash: Option<&'a str>,
    pub context: &'a str,
    pub prompt_version: &'a str,
    /// The configured model alias (what the input hash was made with), not the name of
    /// the deployment that answered.
    pub model: &'a str,
    pub provider: &'a str,
    pub residency: &'a str,
    pub input_hash: &'a str,
}

/// Write generated contexts, each with its provenance and `ready`, in the one statement.
/// Returns the units that were written: a unit whose row no longer held the expected hash
/// is not in the set.
pub(super) async fn write_ready(
    conn: &mut PgConnection,
    writes: &[ReadyWrite<'_>],
) -> sqlx::Result<HashSet<String>> {
    if writes.is_empty() {
        return Ok(HashSet::new());
    }
    let (kids, orgs) = ids(writes.iter().map(|write| write.unit));
    let expected: Vec<Option<&str>> = writes.iter().map(|write| write.expected_hash).collect();
    let contexts: Vec<&str> = writes.iter().map(|write| write.context).collect();
    let versions: Vec<&str> = writes.iter().map(|write| write.prompt_version).collect();
    let models: Vec<&str> = writes.iter().map(|write| write.model).collect();
    let providers: Vec<&str> = writes.iter().map(|write| write.provider).collect();
    let residencies: Vec<&str> = writes.iter().map(|write| write.residency).collect();
    let hashes: Vec<&str> = writes.iter().map(|write| write.input_hash).collect();
    let rows = sqlx::query(
        r#"
        UPDATE knowledge_units AS ku
           SET chunk_context = t.context,
               context_prompt_version = t.prompt_version,
               context_status = 'ready',
               context_model = t.model,
               context_provider = t.provider,
               context_residency = t.residency,
               context_input_hash = t.input_hash,
               context_generated_at = now(),
               context_attempts = 0,
               context_next_attempt_at = NULL,
               context_error_class = NULL
          FROM unnest($1::text[], $2::text[], $3::text[], $4::text[], $5::text[],
                      $6::text[], $7::text[], $8::text[], $9::text[])
               AS t(knowledge_id, org_id, expected_hash, context, prompt_version,
                    model, provider, residency, input_hash)
         WHERE ku.knowledge_id = t.knowledge_id
           AND ku.org_id = t.org_id
           AND ku.context_input_hash IS NOT DISTINCT FROM t.expected_hash
        RETURNING ku.knowledge_id
        "#,
    )
    .bind(&kids)
    .bind(&orgs)
    .bind(&expected)
    .bind(&contexts)
    .bind(&versions)
    .bind(&models)
    .bind(&providers)
    .bind(&residencies)
    .bind(&hashes)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.iter().map(|row| row.get("knowledge_id")).collect())
}

/// What a unit's status becomes when no context was produced for it (or, for `legacy`,
/// when it is only classified).
#[derive(Debug)]
pub(super) struct StatusWrite<'a> {
    pub unit: UnitRef<'a>,
    pub expected_hash: Option<&'a str>,
    pub status: &'static str,
    pub error_class: Option<&'static str>,
    /// This failure counts toward the unit's attempts (a posture or a credential
    /// problem is not the unit's fault and does not).
    pub count_attempt: bool,
    /// When the unit is due again, from now. `None`: nothing is scheduled.
    pub due_in: Option<Duration>,
}

/// Record that no context was produced. A unit that has a context in force keeps its
/// status (a failed regeneration of a `ready` context does not make it anything else; it
/// carries the error and the next attempt, and nothing else changes); a unit that has
/// none gets the status. Returns the units that were written.
pub(super) async fn write_status(
    conn: &mut PgConnection,
    writes: &[StatusWrite<'_>],
) -> sqlx::Result<HashSet<String>> {
    if writes.is_empty() {
        return Ok(HashSet::new());
    }
    let (kids, orgs) = ids(writes.iter().map(|write| write.unit));
    let expected: Vec<Option<&str>> = writes.iter().map(|write| write.expected_hash).collect();
    let statuses: Vec<&str> = writes.iter().map(|write| write.status).collect();
    let classes: Vec<Option<&str>> = writes.iter().map(|write| write.error_class).collect();
    let increments: Vec<i32> = writes
        .iter()
        .map(|write| i32::from(write.count_attempt))
        .collect();
    let due: Vec<Option<f64>> = writes
        .iter()
        .map(|write| write.due_in.map(|after| after.as_secs_f64()))
        .collect();
    let rows = sqlx::query(
        r#"
        UPDATE knowledge_units AS ku
           SET context_status = CASE
                   WHEN ku.chunk_context IS NOT NULL AND btrim(ku.chunk_context) <> ''
                       THEN COALESCE(ku.context_status, 'legacy')
                   ELSE t.status
               END,
               context_error_class = t.error_class,
               context_attempts = ku.context_attempts + t.attempt_increment,
               context_next_attempt_at = CASE
                   WHEN t.due_in_seconds IS NULL THEN NULL
                   ELSE now() + t.due_in_seconds * interval '1 second'
               END
          FROM unnest($1::text[], $2::text[], $3::text[], $4::text[], $5::text[],
                      $6::int4[], $7::float8[])
               AS t(knowledge_id, org_id, expected_hash, status, error_class,
                    attempt_increment, due_in_seconds)
         WHERE ku.knowledge_id = t.knowledge_id
           AND ku.org_id = t.org_id
           AND ku.context_input_hash IS NOT DISTINCT FROM t.expected_hash
        RETURNING ku.knowledge_id
        "#,
    )
    .bind(&kids)
    .bind(&orgs)
    .bind(&expected)
    .bind(&statuses)
    .bind(&classes)
    .bind(&increments)
    .bind(&due)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.iter().map(|row| row.get("knowledge_id")).collect())
}

/// One unit that was embedded.
#[derive(Debug)]
pub(super) struct Done<'a> {
    pub unit: UnitRef<'a>,
    /// `sha256` of the exact stored context the vector was built from; `None` for a raw
    /// vector.
    pub context_hash: Option<String>,
}

/// Mark units embedded. The last write of a unit in a batch: it sets `embedded_at` and, in
/// the same statement, what the vector was built from, and the trigger that keeps
/// `updated_at` honest stamps the same instant, so that nothing a later reader compares
/// (`updated_at > embedded_at` is how the stale-embedding detector sees a change after
/// the embed) sees a change that is not one.
///
/// No status predicate, and `error_message` is cleared (D18): a row that an exhausted
/// delivery marked `failed`, or that a re-drive picked up, is reconciled all the way
/// back to a clean `done`.
#[cfg(test)]
pub(super) async fn mark_done(
    conn: &mut PgConnection,
    done: &[Done<'_>],
    embedding_model: &str,
) -> sqlx::Result<u64> {
    mark_done_with(conn, done, embedding_model, false).await
}

/// What `mark_done_with` adds to the statement when it promotes: a `legacy` unit becomes `ready`
/// if the hash recorded for its vector is the hash of the context it holds now. It is spliced
/// in only for the verification pass, so the statement of every other pass (the stream's and
/// the reconciler's generation) names no verification machinery at all.
const PROMOTE_LEGACY: &str = r#",
               context_status = CASE
                   WHEN ku.context_status = 'legacy'
                    AND t.context_hash IS NOT NULL
                    AND ku.chunk_context IS NOT NULL
                    AND btrim(ku.chunk_context) <> ''
                    AND t.context_hash = context_text_hash(ku.chunk_context)
                       THEN 'ready'
                   ELSE ku.context_status
               END"#;

/// [`mark_done`], and, when `promote_legacy` (the reconciler's verification pass), a `legacy`
/// unit becomes `ready` in the same statement if the hash recorded for its vector is the hash
/// of the context it holds now. That is the proof the pass exists to obtain: the vector was
/// built from exactly this stored context (`context_text_hash`, the one definition of the
/// bytes that are hashed). A unit whose context changed since the batch read it, a raw
/// vector, and every status other than `legacy` are not promoted. Promotion clears nothing
/// else and invents nothing: the context's own provenance (`context_model`,
/// `context_input_hash`) stays unknown, because it is.
pub(super) async fn mark_done_with(
    conn: &mut PgConnection,
    done: &[Done<'_>],
    embedding_model: &str,
    promote_legacy: bool,
) -> sqlx::Result<u64> {
    if done.is_empty() {
        return Ok(0);
    }
    let (kids, orgs) = ids(done.iter().map(|done| done.unit));
    let hashes: Vec<Option<&str>> = done
        .iter()
        .map(|done| done.context_hash.as_deref())
        .collect();
    let promotion = if promote_legacy { PROMOTE_LEGACY } else { "" };
    let sql = format!(
        r#"
        UPDATE knowledge_units AS ku
           SET embedding_status = 'done',
               embedded_at = NOW(),
               error_message = NULL,
               embedding_model = $4,
               embedded_context_hash = t.context_hash,
               -- The unit is finished with, whatever became of its context: the reconciler's lease
               -- (CR-T9a) ends here, in the same statement, because ending it later would be one
               -- more write of a unit that was embedded (see the module documentation of
               -- `context_reconciler::select`). A batch that dies before this keeps its leases,
               -- and they run out by themselves.
               context_lease_until = NULL{promotion}
          FROM unnest($1::text[], $2::text[], $3::text[]) AS t(knowledge_id, org_id, context_hash)
         WHERE ku.knowledge_id = t.knowledge_id
           AND ku.org_id = t.org_id
        "#
    );
    let result = sqlx::query(&sql)
        .bind(&kids)
        .bind(&orgs)
        .bind(&hashes)
        .bind(embedding_model)
        .execute(&mut *conn)
        .await?;
    Ok(result.rows_affected())
}

/// A document that is fully embedded, as `documents.indexed` announces it.
pub(super) struct Indexed {
    pub document_id: String,
    pub org_id: String,
    pub title: String,
}

/// Which of the documents (by `(document_id, org_id)`) have no unit left to embed, marking
/// them `indexed`.
///
/// Re-announcing is deliberate: every consumer of `documents.indexed` is independently
/// idempotent on a repeat announce of the same document (graph-index upserts on
/// deterministic ids, the keyword indexes upsert by document id). Before this was
/// changed, an already-`indexed` document could never be announced again, so a backfill
/// or a replay could never reach it. The `SET status = 'indexed'` is a no-op write when
/// already set: the row is selected unconditionally so RETURNING always produces it.
///
/// This announcement is the keyword index's sync: it comes AFTER the contexts are
/// written, so the keyword arms index each unit with the context its vector was built
/// from.
pub(super) async fn indexed_documents(
    conn: &mut PgConnection,
    pairs: &[(&str, &str)],
) -> sqlx::Result<Vec<Indexed>> {
    let mut indexed = Vec::new();
    let unique: HashSet<(&str, &str)> = pairs.iter().copied().collect();
    for (document_id, org_id) in unique {
        let pending: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM knowledge_units \
              WHERE document_id = $1 AND org_id = $2 AND embedding_status != 'done'",
        )
        .bind(document_id)
        .bind(org_id)
        .fetch_one(&mut *conn)
        .await?;
        if pending.0 != 0 {
            continue;
        }
        let row = sqlx::query_as::<_, (String, String, String)>(
            "UPDATE documents SET status = 'indexed' \
              WHERE document_id = $1 AND org_id = $2 RETURNING document_id, org_id, title",
        )
        .bind(document_id)
        .bind(org_id)
        .fetch_optional(&mut *conn)
        .await?;
        if let Some((document_id, org_id, title)) = row {
            indexed.push(Indexed {
                document_id,
                org_id,
                title,
            });
        }
    }
    Ok(indexed)
}

/// The columns the pipeline reads and writes, which the provenance migration adds.
const REQUIRED_COLUMNS: [&str; 13] = [
    "chunk_context",
    "context_prompt_version",
    "context_status",
    "context_model",
    "context_provider",
    "context_residency",
    "context_input_hash",
    "embedded_context_hash",
    "context_generated_at",
    "context_attempts",
    "context_next_attempt_at",
    "context_error_class",
    "embedding_model",
];

/// Fail at startup, naming the migration, when the schema does not have what the pipeline
/// writes. Without it every batch fails with a column error and the units sit in the
/// queue until the broker gives up on them; with it the service says what to run.
pub(crate) async fn ensure_schema(pool: &PgPool) -> anyhow::Result<()> {
    let present: Vec<String> = sqlx::query_scalar(
        "SELECT column_name FROM information_schema.columns \
          WHERE table_schema = current_schema() AND table_name = 'knowledge_units' \
            AND column_name = ANY($1)",
    )
    .bind(REQUIRED_COLUMNS.to_vec())
    .fetch_all(pool)
    .await?;
    let missing: Vec<&str> = REQUIRED_COLUMNS
        .iter()
        .copied()
        .filter(|column| !present.iter().any(|found| found == column))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    anyhow::bail!(
        "knowledge_units has no column {}: apply migration 20261003130000_context_provenance \
         first (`docker compose run --rm migrate up`, which the Data stage runs before the \
         services); this build writes them on every batch",
        missing.join(", ")
    )
}
