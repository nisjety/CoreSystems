//! What needs doing, and the statements that find it, lease it and record what could not be
//! done. Everything the reconciler reads or writes about a unit outside the batch pipeline goes
//! through here.
//!
//! ## Tenancy
//!
//! Finding work is cross-organization by nature, so [`orgs_with_due_work`], [`lease`] and
//! [`counts`] run on the unscoped connection every Data Plane service uses (the superuser, for
//! which row-level security is inert) and say the organization in every statement: `lease` takes
//! one organization at a time and never a list. What the reconciler then does for ONE
//! organization (reading the state of its units, marking its exhausted units permanent,
//! releasing its zero-retention skips) takes a connection from the caller and runs on the one
//! `pg_org_scope::begin_org_scoped` opened, as the restricted application role, where the
//! database and the explicit predicate must agree; a test runs each of them both ways.
//!
//! ## What a lease is
//!
//! `FOR UPDATE SKIP LOCKED` picks the rows and `context_lease_until` records that they are
//! taken, in one statement. Two reconcilers (a second replica, a restart that overlaps its
//! predecessor, the operator's `context-backfill` next to the service) never get the same unit:
//! the one that locks a row first writes the lease, and the other either skips the locked row or,
//! having read it before the lease was committed, finds the lease when the row is re-read. A
//! lease is never released: it ends when its time is up, which is what frees the units of a
//! worker that died. Releasing it would be one more write of the unit after the pipeline marked it
//! done, and the stale-embedding detector reads `updated_at > embedded_at` as "changed after it
//! was embedded". (The lease itself is written BEFORE the batch, which marks the unit done last
//! and so makes the two timestamps equal again.)

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgPool, Row};

/// A kind of work. The order is the order an organization's share of a pass is spent in: the
/// units that have no context at all first, then the ones whose vector is not proven, then the
/// rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Kind {
    /// No usable context: `pending`, a retryable failure that is due, an exhausted budget, a
    /// unit skipped by a policy that was lifted, a unit no writer ever gave a status.
    Generate,
    /// A `legacy` unit (or one with a context and no status): embed its stored context and,
    /// the vector being proven, promote it. No model.
    Verify,
    /// A `ready` unit whose vector is not the one its stored context makes: embed again. No
    /// model.
    Repair,
    /// A `ready` unit whose document changed after the context was made and whose input hash
    /// says it matters: regenerate.
    Stale,
    /// A stored context made with another prompt version than the one in force: regenerate,
    /// inside the cap.
    Refresh,
}

impl Kind {
    #[cfg(test)]
    pub(crate) const ALL: [Kind; 5] = [
        Kind::Generate,
        Kind::Verify,
        Kind::Repair,
        Kind::Stale,
        Kind::Refresh,
    ];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Generate => "generate",
            Self::Verify => "verify",
            Self::Repair => "repair",
            Self::Stale => "stale",
            Self::Refresh => "refresh",
        }
    }
}

/// A unit as the lease returns it: what the batch is built from, and what the unit was before.
#[derive(Clone, Debug)]
pub(crate) struct Leased {
    pub kind: Kind,
    pub knowledge_id: String,
    pub document_id: String,
    pub org_id: String,
    pub chunk_index: i32,
    pub text: String,
    pub document_date: Option<DateTime<Utc>>,
    /// When its context was generated before this run. A later value afterwards means a context
    /// was written for it.
    pub generated_at: Option<DateTime<Utc>>,
}

// ── predicates ──────────────────────────────────────────────────────────────

/// A unit the reconciler may touch at all: indexed, in a live document that is not restricted.
/// A restricted document is never read, windowed, sent or re-embedded here (the embedding guard
/// would refuse it anyway, and a refusal is not a thing to retry): it is the pipeline's, at
/// ingest, and the keyword index's own predicate is the same, so a unit the reconciler cannot
/// touch is also one the index does not hold.
const ELIGIBLE: &str = "ku.embedding_status = 'done' \
    AND d.org_id = ku.org_id \
    AND d.deleted_at IS NULL \
    AND lower(btrim(d.zdr_classification)) IN ('internal', 'public', 'sensitive')";
const HAS_CONTEXT: &str = "ku.chunk_context IS NOT NULL AND btrim(ku.chunk_context) <> ''";
const NO_CONTEXT: &str = "(ku.chunk_context IS NULL OR btrim(ku.chunk_context) = '')";
const DUE: &str = "(ku.context_next_attempt_at IS NULL OR ku.context_next_attempt_at <= now())";
const LEASE_FREE: &str = "(ku.context_lease_until IS NULL OR ku.context_lease_until <= now())";

/// The statuses the work queue index covers. A unit in one of them whose time has come is found
/// without reading any other row.
const QUEUED_STATUSES: &str = "(ku.context_status IS NULL \
    OR ku.context_status IN ('pending', 'legacy', 'failed_transient', 'skipped_budget'))";

/// The bind parameters a kind's predicate may name, by number. `$1` is always the
/// organization.
struct Params {
    max_attempts: &'static str,
    include_skipped_policy: &'static str,
    include_skipped_budget: &'static str,
    refresh_target: &'static str,
}

/// What a kind of work is, as a predicate over `ku` (knowledge_units) and `d` (documents).
fn predicate(kind: Kind, params: &Params) -> String {
    match kind {
        // `skipped_policy` and `skipped_budget` carry no schedule of their own: each is a decision
        // of a gate that is read at every lease (the organization's policy, its background
        // budget), so a unit it holds is work exactly when the gate lets the organization through
        // (`include_skipped_*`), and not before. (`skipped_budget` used to be scheduled like a
        // failure, with no gate to read: it was leased and embedded again at every pass.)
        Kind::Generate => format!(
            "{NO_CONTEXT} AND ( \
                ku.context_status IS NULL \
                OR (ku.context_status IN ('pending', 'failed_transient') \
                    AND ku.context_attempts < {max} AND {DUE}) \
                OR (ku.context_status = 'skipped_policy' AND {policy}) \
                OR (ku.context_status = 'skipped_budget' AND {budget}) )",
            max = params.max_attempts,
            policy = params.include_skipped_policy,
            budget = params.include_skipped_budget,
        ),
        Kind::Verify => {
            format!("{HAS_CONTEXT} AND (ku.context_status = 'legacy' OR ku.context_status IS NULL)")
        }
        Kind::Repair => format!(
            "ku.context_status = 'ready' AND {HAS_CONTEXT} \
             AND ku.embedded_context_hash IS DISTINCT FROM context_text_hash(ku.chunk_context)"
        ),
        // A regeneration that failed leaves the unit as it was, with the time it may be tried
        // again (`context_next_attempt_at`, from the failure: the provider's wait, the ladder,
        // an hour for a zero-retention organization): until then it is not work, or a rollout
        // would ask again at every pass for what it was just told to wait for.
        Kind::Refresh => format!(
            "ku.context_status IN ('ready', 'legacy') AND {HAS_CONTEXT} AND {DUE} \
             AND ku.context_prompt_version IS DISTINCT FROM {target}",
            target = params.refresh_target,
        ),
        // Found by the document's change, then decided by each chunk's input hash: see
        // `stale_documents` and `lease_ids`. It has no predicate of its own.
        Kind::Stale => "false".to_owned(),
    }
}

/// How the pass asks for one kind.
pub(crate) struct LeaseRequest<'a> {
    pub org_id: &'a str,
    pub kind: Kind,
    /// The most units.
    pub limit: i64,
    pub lease_secs: f64,
    /// Units that must not be asked now (their cooldown has not ended).
    pub excluded: &'a [String],
    /// A unit that failed this many times is no longer retried (it is `failed_permanent`).
    pub max_attempts: i32,
    /// Whether `skipped_policy` units are work: the organization's policy is not off now.
    pub include_skipped_policy: bool,
    /// Whether `skipped_budget` units are work: the organization's background budget is not
    /// used up in `stop` mode now (CR-T9b). While it is, only the units that have no status yet
    /// are leased, so that the pipeline can mark them; the ones already marked are left alone.
    pub include_skipped_budget: bool,
    /// The prompt version in force for the organization, for `Kind::Refresh`.
    pub refresh_target: &'a str,
}

const LEASE_RETURNING: &str = "RETURNING ku.knowledge_id, ku.document_id, ku.org_id, \
    ku.chunk_index, ku.text, ku.context_generated_at, d.document_date";

fn leased(kind: Kind, rows: &[sqlx::postgres::PgRow]) -> Vec<Leased> {
    rows.iter()
        .map(|row| Leased {
            kind,
            knowledge_id: row.get("knowledge_id"),
            document_id: row.get("document_id"),
            org_id: row.get("org_id"),
            chunk_index: row.get("chunk_index"),
            text: row.get("text"),
            document_date: row.get("document_date"),
            generated_at: row.get("context_generated_at"),
        })
        .collect()
}

/// Lease up to `limit` units of one kind of one organization, a document's units together.
///
/// Unscoped on purpose (see the module documentation), and one organization at a time: the
/// organization is `$1` in the picking statement and again in the update, and the document is
/// joined under the same organization.
pub(crate) async fn lease(pool: &PgPool, request: &LeaseRequest<'_>) -> sqlx::Result<Vec<Leased>> {
    if request.limit <= 0 || request.kind == Kind::Stale {
        return Ok(Vec::new());
    }
    let params = Params {
        max_attempts: "$5",
        include_skipped_policy: "$6::bool",
        include_skipped_budget: "$8::bool",
        refresh_target: "$7::text",
    };
    let sql = format!(
        "WITH picked AS ( \
             SELECT ku.knowledge_id \
               FROM knowledge_units AS ku \
               JOIN documents AS d ON d.document_id = ku.document_id \
              WHERE ku.org_id = $1 AND {ELIGIBLE} AND {LEASE_FREE} \
                AND {kind} AND NOT (ku.knowledge_id = ANY($4::text[])) \
              ORDER BY ku.document_id, ku.chunk_index, ku.knowledge_id \
              LIMIT $2 \
                FOR UPDATE OF ku SKIP LOCKED ) \
         UPDATE knowledge_units AS ku \
            SET context_lease_until = now() + make_interval(secs => $3::float8) \
           FROM picked, documents AS d \
          WHERE ku.knowledge_id = picked.knowledge_id AND ku.org_id = $1 \
            AND d.document_id = ku.document_id AND d.org_id = ku.org_id \
         {LEASE_RETURNING}",
        kind = predicate(request.kind, &params),
    );
    let rows = sqlx::query(&sql)
        .bind(request.org_id)
        .bind(request.limit)
        .bind(request.lease_secs)
        .bind(request.excluded)
        .bind(request.max_attempts)
        .bind(request.include_skipped_policy)
        .bind(request.refresh_target)
        .bind(request.include_skipped_budget)
        .fetch_all(pool)
        .await?;
    Ok(leased(request.kind, &rows))
}

/// Lease these units, if they are still `ready`, indexed, in a live document that is not
/// restricted and not leased by someone else. The stale pass has decided by each chunk's input
/// hash which of a document's units are worth regenerating; this takes exactly those.
pub(crate) async fn lease_ids(
    pool: &PgPool,
    org_id: &str,
    knowledge_ids: &[String],
    lease_secs: f64,
) -> sqlx::Result<Vec<Leased>> {
    if knowledge_ids.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "WITH picked AS ( \
             SELECT ku.knowledge_id \
               FROM knowledge_units AS ku \
               JOIN documents AS d ON d.document_id = ku.document_id \
              WHERE ku.org_id = $1 AND {ELIGIBLE} AND {LEASE_FREE} \
                AND ku.context_status = 'ready' AND {HAS_CONTEXT} AND {DUE} \
                AND ku.knowledge_id = ANY($2::text[]) \
              ORDER BY ku.document_id, ku.chunk_index, ku.knowledge_id \
                FOR UPDATE OF ku SKIP LOCKED ) \
         UPDATE knowledge_units AS ku \
            SET context_lease_until = now() + make_interval(secs => $3::float8) \
           FROM picked, documents AS d \
          WHERE ku.knowledge_id = picked.knowledge_id AND ku.org_id = $1 \
            AND d.document_id = ku.document_id AND d.org_id = ku.org_id \
         {LEASE_RETURNING}"
    );
    let rows = sqlx::query(&sql)
        .bind(org_id)
        .bind(knowledge_ids)
        .bind(lease_secs)
        .fetch_all(pool)
        .await?;
    Ok(leased(Kind::Stale, &rows))
}

// ── discovery ───────────────────────────────────────────────────────────────

/// The organizations that have units in a status the work queue index covers whose time has
/// come: found from the index, without reading a unit that is `ready`. A unit of a restricted
/// document that no writer gave a status keeps its organization in this list for ever; the lease
/// then finds nothing, which costs one cheap statement.
pub(crate) async fn orgs_with_due_work(pool: &PgPool) -> sqlx::Result<Vec<String>> {
    let sql = format!(
        "SELECT DISTINCT ku.org_id FROM knowledge_units AS ku \
          WHERE ku.embedding_status = 'done' AND {QUEUED_STATUSES} AND {DUE} AND {LEASE_FREE} \
          ORDER BY 1"
    );
    sqlx::query_scalar(&sql).fetch_all(pool).await
}

/// The organizations that have units a reconciler has to give up on: a retryable failure that
/// has failed `max_attempts` times.
pub(crate) async fn orgs_with_exhausted_units(
    pool: &PgPool,
    max_attempts: i32,
) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar(
        "SELECT DISTINCT ku.org_id FROM knowledge_units AS ku \
          WHERE ku.embedding_status = 'done' AND ku.context_status = 'failed_transient' \
            AND ku.context_attempts >= $1 ORDER BY 1",
    )
    .bind(max_attempts)
    .fetch_all(pool)
    .await
}

/// The organizations with units skipped because they, or the route, are zero-retention. A
/// restricted DOCUMENT (`zdr_document`) is final and is not in this list: nothing about the
/// organization changes it. No index covers this status, on purpose (the migration says the
/// recheck is per organization, not per row), so this reads the table; it runs at most once per
/// recheck interval (an hour).
pub(crate) async fn orgs_with_zero_retention_skips(pool: &PgPool) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar(
        "SELECT DISTINCT org_id FROM knowledge_units \
          WHERE context_status = 'skipped_zdr' \
            AND context_error_class IN ('zdr_org', 'zdr_unavailable') ORDER BY 1",
    )
    .fetch_all(pool)
    .await
}

// ── what the organization's own statements do (the caller's scoped connection) ──

/// What became of the units of a batch, as the database holds them now.
#[derive(Clone, Debug)]
pub(crate) struct UnitState {
    pub knowledge_id: String,
    pub status: Option<String>,
    pub error_class: Option<String>,
    pub generated_at: Option<DateTime<Utc>>,
    /// Seconds until the unit is due again, when a time is set. Negative: already due.
    pub due_in_secs: Option<f64>,
    /// The stored vector is the one the stored context makes (or the unit has no context and
    /// the vector is raw).
    pub coherent: bool,
}

/// The state of these units of one organization. Runs on the caller's connection.
pub(crate) async fn states(
    conn: &mut PgConnection,
    org_id: &str,
    knowledge_ids: &[String],
) -> sqlx::Result<Vec<UnitState>> {
    if knowledge_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query(
        "SELECT ku.knowledge_id, ku.context_status, ku.context_error_class, \
                ku.context_generated_at, \
                extract(epoch FROM (ku.context_next_attempt_at - now()))::float8 AS due_in_secs, \
                ku.embedded_context_hash IS NOT DISTINCT FROM ( \
                    CASE WHEN ku.chunk_context IS NULL OR btrim(ku.chunk_context) = '' THEN NULL \
                         ELSE context_text_hash(ku.chunk_context) END) AS coherent \
           FROM knowledge_units AS ku \
          WHERE ku.org_id = $1 AND ku.knowledge_id = ANY($2::text[])",
    )
    .bind(org_id)
    .bind(knowledge_ids)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .iter()
        .map(|row| UnitState {
            knowledge_id: row.get("knowledge_id"),
            status: row.get("context_status"),
            error_class: row.get("context_error_class"),
            generated_at: row.get("context_generated_at"),
            due_in_secs: row.get("due_in_secs"),
            coherent: row.get("coherent"),
        })
        .collect())
}

impl UnitState {
    /// The unit is not waiting on a failure: a context is in force, or none was needed, or a gate
    /// decided that none is to be made now (`skipped_policy`, `skipped_budget`: each is read at
    /// every lease, and a unit it holds is not leased until the gate lets the organization
    /// through).
    pub(crate) fn status_is_settled(&self) -> bool {
        !matches!(self.status.as_deref(), Some("pending" | "failed_transient"))
    }
}

/// Give up on the units of one organization that failed `max_attempts` times: they become
/// `failed_permanent`, keeping the class they failed with (the table requires one, and it is the
/// operator's answer to "why"). Returns how many, per class. The write is one more write of a
/// unit that was embedded long ago, so `updated_at` moves past `embedded_at`; it happens once per
/// unit in its life, behind an alert, and the stale-embedding report shows exactly the units an
/// operator is being told about.
pub(crate) async fn give_up_on_exhausted(
    conn: &mut PgConnection,
    org_id: &str,
    max_attempts: i32,
) -> sqlx::Result<Vec<(String, i64)>> {
    let rows = sqlx::query(
        "UPDATE knowledge_units AS ku SET context_status = 'failed_permanent' \
          WHERE ku.org_id = $1 AND ku.context_status = 'failed_transient' \
            AND ku.context_attempts >= $2 \
         RETURNING ku.context_error_class",
    )
    .bind(org_id)
    .bind(max_attempts)
    .fetch_all(&mut *conn)
    .await?;
    let mut by_class: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
    for row in &rows {
        let class: Option<String> = row.get("context_error_class");
        *by_class.entry(class.unwrap_or_default()).or_default() += 1;
    }
    Ok(by_class.into_iter().collect())
}

/// The organization is no longer zero-retention: its skipped units are work again. They become
/// `pending` and due now, and the generate pass takes them. Only the two classes that depend on
/// the organization (a restricted document stays skipped whatever the organization is).
pub(crate) async fn release_zero_retention_skips(
    conn: &mut PgConnection,
    org_id: &str,
) -> sqlx::Result<u64> {
    let result = sqlx::query(
        "UPDATE knowledge_units AS ku \
            SET context_status = 'pending', context_error_class = NULL, \
                context_next_attempt_at = now() \
          WHERE ku.org_id = $1 AND ku.context_status = 'skipped_zdr' \
            AND ku.context_error_class IN ('zdr_org', 'zdr_unavailable')",
    )
    .bind(org_id)
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected())
}

// ── the stale pass ──────────────────────────────────────────────────────────

/// A document whose units' contexts might be older than it is.
#[derive(Debug)]
pub(crate) struct StaleDocument {
    pub document_id: String,
    pub title: String,
    pub source: String,
    pub date: Option<DateTime<Utc>>,
    pub content: String,
    pub total_chunks: i64,
    pub units: Vec<StaleUnit>,
}

/// A `ready` unit of such a document, with what its context was made from.
#[derive(Debug)]
pub(crate) struct StaleUnit {
    pub knowledge_id: String,
    pub chunk_index: i32,
    pub text: String,
    pub input_hash: String,
    pub prompt_version: Option<String>,
    pub model: Option<String>,
}

/// The next `limit` documents after `after` (in id order) of one organization that changed after
/// at least one of their contexts was made. The document's own `updated_at` moves for reasons
/// that are not the text (the pipeline writes `status = 'indexed'` at the end of every batch), so
/// this finds CANDIDATES and nothing more: the caller hashes each chunk's inputs and only a
/// chunk whose hash differs is regenerated. Reading a page of candidates, never the whole
/// corpus, is what the timestamp comparison is for.
pub(crate) async fn stale_documents(
    conn: &mut PgConnection,
    org_id: &str,
    after: &str,
    limit: i64,
) -> sqlx::Result<Vec<StaleDocument>> {
    let documents = sqlx::query(
        "SELECT d.document_id, d.title, d.source, d.document_date, d.content, \
                (SELECT count(*) FROM knowledge_units AS k \
                  WHERE k.document_id = d.document_id AND k.org_id = d.org_id) AS total_chunks \
           FROM documents AS d \
          WHERE d.org_id = $1 AND d.deleted_at IS NULL \
            AND lower(btrim(d.zdr_classification)) IN ('internal', 'public', 'sensitive') \
            AND d.document_id > $2 \
            AND EXISTS ( \
                SELECT 1 FROM knowledge_units AS ku \
                 WHERE ku.document_id = d.document_id AND ku.org_id = d.org_id \
                   AND ku.embedding_status = 'done' AND ku.context_status = 'ready' \
                   AND ku.context_input_hash IS NOT NULL AND ku.context_generated_at IS NOT NULL \
                   AND d.updated_at > ku.context_generated_at ) \
          ORDER BY d.document_id LIMIT $3",
    )
    .bind(org_id)
    .bind(after)
    .bind(limit)
    .fetch_all(&mut *conn)
    .await?;
    let mut out: Vec<StaleDocument> = documents
        .iter()
        .map(|row| StaleDocument {
            document_id: row.get("document_id"),
            title: row.get("title"),
            source: row.get("source"),
            date: row.get("document_date"),
            content: row.get("content"),
            total_chunks: row.get("total_chunks"),
            units: Vec::new(),
        })
        .collect();
    if out.is_empty() {
        return Ok(out);
    }
    let ids: Vec<&str> = out
        .iter()
        .map(|document| document.document_id.as_str())
        .collect();
    let units = sqlx::query(
        "SELECT ku.knowledge_id, ku.document_id, ku.chunk_index, ku.text, \
                ku.context_input_hash, ku.context_prompt_version, ku.context_model \
           FROM knowledge_units AS ku \
          WHERE ku.org_id = $1 AND ku.document_id = ANY($2::text[]) \
            AND ku.embedding_status = 'done' AND ku.context_status = 'ready' \
            AND ku.context_input_hash IS NOT NULL \
          ORDER BY ku.document_id, ku.chunk_index",
    )
    .bind(org_id)
    .bind(&ids)
    .fetch_all(&mut *conn)
    .await?;
    for row in &units {
        let document_id: String = row.get("document_id");
        if let Some(document) = out
            .iter_mut()
            .find(|document| document.document_id == document_id)
        {
            document.units.push(StaleUnit {
                knowledge_id: row.get("knowledge_id"),
                chunk_index: row.get("chunk_index"),
                text: row.get("text"),
                input_hash: row.get("context_input_hash"),
                prompt_version: row.get("context_prompt_version"),
                model: row.get("context_model"),
            });
        }
    }
    Ok(out)
}

// ── counts: the sweep and the dry run ───────────────────────────────────────

/// What there is to do for one organization, counted. Nothing in it is content.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct OrgCounts {
    pub org_id: String,
    /// Units with no context that are due (the `Generate` kind).
    pub generate: i64,
    /// Characters a generation would send as the document window plus the chunk, summed (an
    /// upper bound per unit: the window is capped at the configured size).
    pub generate_chars: i64,
    /// Characters of the chunks alone, which is what is embedded besides their context.
    pub generate_text_chars: i64,
    /// `skipped_policy` units (work again once the policy is not off).
    pub skipped_policy: i64,
    /// `skipped_budget` units (work again once the background budget is not used up in `stop`
    /// mode).
    pub skipped_budget: i64,
    pub verify: i64,
    pub repair: i64,
    /// Characters of the text each verification or repair embeds: the chunk and its context.
    pub embed_chars: i64,
    /// Units whose context was made with another prompt version than `refresh_target`.
    pub refresh: i64,
    pub refresh_chars: i64,
    pub refresh_text_chars: i64,
    /// Retryable failures that have failed `max_attempts` times.
    pub exhausted: i64,
    /// Units skipped for zero retention that depend on the organization.
    pub skipped_zdr: i64,
    /// Units that failed and are waiting their turn (not due yet): informational.
    pub waiting: i64,
}

/// The statement of [`counts`], in one place so that a test can read it without a database.
///
/// `$1` is the organization (or NULL for all), `$2` the attempts a failure may have, `$3` the
/// longest document text a prompt carries and `$4` the prompt version a refresh would move to.
fn counts_sql() -> String {
    let params = Params {
        max_attempts: "$2",
        include_skipped_policy: "false",
        include_skipped_budget: "false",
        refresh_target: "$4::text",
    };
    let generate = predicate(Kind::Generate, &params);
    let verify = predicate(Kind::Verify, &params);
    let repair = predicate(Kind::Repair, &params);
    let refresh = predicate(Kind::Refresh, &params);
    format!(
        "SELECT ku.org_id, \
                count(*) FILTER (WHERE {generate}) AS generate, \
                coalesce(sum(least(char_length(d.content), $3::bigint) + char_length(ku.text)) \
                    FILTER (WHERE {generate}), 0)::bigint AS generate_chars, \
                coalesce(sum(char_length(ku.text)) FILTER (WHERE {generate}), 0)::bigint                     AS generate_text_chars,                 count(*) FILTER (WHERE ku.context_status = 'skipped_policy') AS skipped_policy, \
                count(*) FILTER (WHERE ku.context_status = 'skipped_budget') AS skipped_budget, \
                count(*) FILTER (WHERE {verify}) AS verify, \
                count(*) FILTER (WHERE {repair}) AS repair, \
                coalesce(sum(char_length(ku.text) + char_length(coalesce(ku.chunk_context, ''))) \
                    FILTER (WHERE ({verify}) OR ({repair})), 0)::bigint AS embed_chars, \
                count(*) FILTER (WHERE $4::text IS NOT NULL AND ({refresh})) AS refresh, \
                coalesce(sum(least(char_length(d.content), $3::bigint) + char_length(ku.text)) \
                    FILTER (WHERE $4::text IS NOT NULL AND ({refresh})), 0)::bigint AS refresh_chars,                 coalesce(sum(char_length(ku.text))                     FILTER (WHERE $4::text IS NOT NULL AND ({refresh})), 0)::bigint                     AS refresh_text_chars, \
                count(*) FILTER (WHERE ku.context_status = 'failed_transient' \
                                   AND ku.context_attempts >= $2) AS exhausted, \
                count(*) FILTER (WHERE ku.context_status = 'skipped_zdr' \
                                   AND ku.context_error_class IN ('zdr_org', 'zdr_unavailable')) \
                    AS skipped_zdr, \
                count(*) FILTER (WHERE ku.context_status = 'failed_transient' \
                                   AND ku.context_attempts < $2 \
                                   AND ku.context_next_attempt_at > now()) AS waiting \
           FROM knowledge_units AS ku \
           JOIN documents AS d ON d.document_id = ku.document_id \
          WHERE ($1::text IS NULL OR ku.org_id = $1) AND {ELIGIBLE} \
          GROUP BY ku.org_id ORDER BY ku.org_id"
    )
}

/// Counts per organization (or for one), for the dry run and for the sweep.
///
/// `window_chars` is the longest document text a prompt carries; `refresh_target` the prompt
/// version a refresh would move to, or `None` when no refresh is being considered.
pub(crate) async fn counts(
    pool: &PgPool,
    org_id: Option<&str>,
    max_attempts: i32,
    window_chars: i64,
    refresh_target: Option<&str>,
) -> sqlx::Result<Vec<OrgCounts>> {
    let sql = counts_sql();
    let rows = sqlx::query(&sql)
        .bind(org_id)
        .bind(max_attempts)
        .bind(window_chars)
        .bind(refresh_target)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .map(|row| OrgCounts {
            org_id: row.get("org_id"),
            generate: row.get("generate"),
            generate_chars: row.get("generate_chars"),
            generate_text_chars: row.get("generate_text_chars"),
            skipped_policy: row.get("skipped_policy"),
            skipped_budget: row.get("skipped_budget"),
            verify: row.get("verify"),
            repair: row.get("repair"),
            embed_chars: row.get("embed_chars"),
            refresh: row.get("refresh"),
            refresh_chars: row.get("refresh_chars"),
            refresh_text_chars: row.get("refresh_text_chars"),
            exhausted: row.get("exhausted"),
            skipped_zdr: row.get("skipped_zdr"),
            waiting: row.get("waiting"),
        })
        .collect())
}

/// The text of the predicates, which needs no database: what a gate-held status is, and is not.
#[cfg(test)]
mod predicate_tests {
    use super::*;

    const PARAMS: Params = Params {
        max_attempts: "$5",
        include_skipped_policy: "$6::bool",
        include_skipped_budget: "$8::bool",
        refresh_target: "$7::text",
    };

    fn generate() -> String {
        predicate(Kind::Generate, &PARAMS)
    }

    /// `skipped_budget` and `skipped_policy` are decisions of a gate that is read at every lease
    /// (CR-T9b): each is work exactly when its own parameter says the gate lets the organization
    /// through, with no schedule and no attempt count of its own. A failure, by contrast, is work
    /// when it is due and has attempts left. A `skipped_budget` that were scheduled like a failure
    /// would be leased, and embedded again, at every pass.
    #[test]
    fn a_status_a_gate_holds_is_work_only_when_its_gate_lets_the_organization_through() {
        let sql = generate();
        assert!(
            sql.contains("(ku.context_status = 'skipped_budget' AND $8::bool)"),
            "{sql}"
        );
        assert!(
            sql.contains("(ku.context_status = 'skipped_policy' AND $6::bool)"),
            "{sql}"
        );
        // The failures keep their schedule, and skipped_budget is not one of them.
        assert!(
            sql.contains(
                "(ku.context_status IN ('pending', 'failed_transient') \
                    AND ku.context_attempts < $5 AND (ku.context_next_attempt_at IS NULL"
            ),
            "{sql}"
        );
        assert!(
            !sql.contains("'pending', 'failed_transient', 'skipped_budget'"),
            "{sql}"
        );
        // A unit with a context is never generated for, whatever its status says.
        assert!(sql.starts_with(NO_CONTEXT), "{sql}");
    }

    /// Only the generate kind asks a model for a first context; the budget gate is not part of
    /// any other predicate (a re-embed asks no model, so a stop holds nothing back there).
    #[test]
    fn no_other_kind_names_the_budget_gate() {
        for kind in [Kind::Verify, Kind::Repair, Kind::Refresh, Kind::Stale] {
            let sql = predicate(kind, &PARAMS);
            assert!(!sql.contains("skipped_budget"), "{kind:?}: {sql}");
            assert!(!sql.contains("$8"), "{kind:?}: {sql}");
        }
    }

    /// The sweep and the dry run count the held units apart (`include_skipped_budget` is false
    /// there), so that what a stop holds is never priced as work and never lost from the count.
    #[test]
    fn the_counts_leave_the_held_units_out_of_the_work_and_count_them_apart() {
        let sql = counts_sql();
        assert!(
            sql.contains("(ku.context_status = 'skipped_budget' AND false)"),
            "the work the counts price must not include a unit a gate holds: {sql}"
        );
        assert!(
            sql.contains(
                "count(*) FILTER (WHERE ku.context_status = 'skipped_budget') AS skipped_budget"
            ),
            "the units a stop holds are counted apart: {sql}"
        );
        // A held unit has no time of its own, so it is never "waiting" for one.
        assert!(
            sql.contains("FILTER (WHERE ku.context_status = 'failed_transient' AND ku.context_attempts < $2 AND ku.context_next_attempt_at > now()) AS waiting"),
            "{sql}"
        );
        // Every parameter the statement names is one `counts` binds (four of them).
        for number in 1..=4 {
            assert!(
                sql.contains(&format!("${number}")),
                "${number} is not used: {sql}"
            );
        }
        assert!(!sql.contains("$5"), "{sql}");
    }
}
