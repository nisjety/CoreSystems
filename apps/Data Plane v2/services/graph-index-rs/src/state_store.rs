//! The SQL of graph extraction state (design task CR-T12): claiming a chunk, recording what came
//! of it, marking why a chunk was not attempted, choosing what is due, and reading the policy and
//! the spend the gates need.
//!
//! Every statement runs in an organization-scoped transaction (`pg_org_scope::begin_org_scoped`:
//! `app.current_org` set, then `SET LOCAL ROLE dataplane_app`), so row-level security backs the
//! `org_id` each statement binds itself. The services connect as a superuser, on which RLS is
//! inert; the scoped role is what makes the policy real, and the database tests below run on the
//! migrated schema for that reason.
//!
//! ## One definition of "due"
//!
//! [`due_predicate`] is the rule, as SQL over a row of `graph_extraction_state`. The claim of one
//! chunk and the scan for documents with work both build their statements from it, so the two
//! cannot disagree about whether a chunk is due:
//!
//! * a row is due when no worker holds an unexpired lease on it and either
//!   * its status is one a worker may pick up (everything but `ready`, `empty` and
//!     `failed_permanent`) and `next_attempt_at` is NULL or past, or
//!   * it is `empty` under another prompt version than the current one;
//! * `ready` and `failed_permanent` are never due, whatever their timestamps.
//!
//! A chunk with no row is due.

use std::sync::LazyLock;
use std::time::Duration;

use sqlx::Row;

use crate::budget::{BudgetMode, OrgPolicy, Switch};
use crate::state::{
    Claim, Settlement, Standing, EGRESS_ALLOWED_CLASSIFICATIONS, PROMPT_VERSION, WORK_STATUSES,
};
use crate::store::GraphStore;

fn quoted(names: impl IntoIterator<Item = &'static str>) -> String {
    names
        .into_iter()
        .map(|name| format!("'{name}'"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn work_statuses() -> String {
    quoted(WORK_STATUSES.iter().map(|status| status.as_str()))
}

fn allowed_classifications() -> String {
    quoted(EGRESS_ALLOWED_CLASSIFICATIONS)
}

/// The classification test of a `documents` row aliased `d`: the same normalisation
/// `egress_allowed` applies in Rust.
fn allowed_classification_sql() -> String {
    format!(
        "lower(btrim(d.zdr_classification)) IN ({})",
        allowed_classifications()
    )
}

/// "This row is due", over the row aliased `alias`, with the current prompt version in
/// `prompt_param`. See the module documentation.
pub(crate) fn due_predicate(alias: &str, prompt_param: &str) -> String {
    format!(
        "(({a}.lease_until IS NULL OR {a}.lease_until <= now()) AND (\
         ({a}.status IN ({work}) AND ({a}.next_attempt_at IS NULL OR {a}.next_attempt_at <= now())) \
         OR ({a}.status = 'empty' AND {a}.prompt_version IS DISTINCT FROM {p})))",
        a = alias,
        work = work_statuses(),
        p = prompt_param,
    )
}

/// Claim one chunk: the row is created or taken, `attempts` goes up by one and a lease is set,
/// only when the chunk is due and its document is visible to the organization and of a
/// classification that may reach the model (the claim is the last gate before a chunk is sent, so
/// it holds the standing itself and does not rely on its callers having looked). One statement, so
/// two workers cannot both win. `$1` organization, `$2` chunk, `$3` lease in seconds, `$4` the
/// current prompt version. No row back means not claimed.
static CLAIM_SQL: LazyLock<String> = LazyLock::new(|| {
    format!(
        "INSERT INTO graph_extraction_state AS s \
             (org_id, knowledge_id, status, attempts, lease_until, last_attempt_at) \
         SELECT ku.org_id, ku.knowledge_id, 'pending', 1, \
                now() + make_interval(secs => $3::float8), now() \
           FROM knowledge_units AS ku \
           JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id \
          WHERE ku.knowledge_id = $2 AND ku.org_id = $1 \
            AND d.visibility = 'org' AND d.deleted_at IS NULL AND {allowed} \
         ON CONFLICT (org_id, knowledge_id) DO UPDATE \
            SET status = 'pending', \
                attempts = s.attempts + 1, \
                lease_until = EXCLUDED.lease_until, \
                last_attempt_at = EXCLUDED.last_attempt_at \
          WHERE {due} \
         RETURNING s.attempts",
        due = due_predicate("s", "$4::text"),
        allowed = allowed_classification_sql(),
    )
});

/// Record what came of an attempt. `$1` organization, `$2` chunk, `$3` status, `$4` error class,
/// `$5` seconds until due (NULL: none), `$6` hand the attempt back, `$7`-`$12` the stamp (NULL
/// leaves the stored one alone: only a completed extraction replaces it).
const SETTLE_SQL: &str = "UPDATE graph_extraction_state SET \
        status = $3::text, \
        error_class = $4::text, \
        next_attempt_at = CASE WHEN $5::float8 IS NULL THEN NULL \
                               ELSE now() + make_interval(secs => $5::float8) END, \
        lease_until = NULL, \
        attempts = CASE WHEN $6::bool THEN GREATEST(attempts - 1, 0) ELSE attempts END, \
        prompt_version = COALESCE($7::text, prompt_version), \
        model = COALESCE($8::text, model), \
        provider = COALESCE($9::text, provider), \
        extracted_at = CASE WHEN $3::text IN ('ready', 'empty') THEN now() ELSE extracted_at END, \
        entity_count = COALESCE($10::int, entity_count), \
        relationship_count = COALESCE($11::int, relationship_count), \
        claim_count = COALESCE($12::int, claim_count) \
  WHERE org_id = $1 AND knowledge_id = $2";

/// Say why chunks were not attempted. Inserts the rows that are missing and rewrites a row only
/// while it is still waiting (a status a worker picks up), unleased and saying something else:
/// it never touches `ready`, `empty` or `failed_permanent`, and a re-run that finds every row
/// already marked changes nothing. `$1` organization, `$2` status, `$3` error class, `$4` seconds
/// until due (NULL: none), `$5` how many rows at most; `population` says which chunks.
fn mark_sql(population: &str) -> String {
    format!(
        "INSERT INTO graph_extraction_state AS s (org_id, knowledge_id, status, error_class, next_attempt_at) \
         SELECT ku.org_id, ku.knowledge_id, $2::text, $3::text, \
                CASE WHEN $4::float8 IS NULL THEN NULL \
                     ELSE now() + make_interval(secs => $4::float8) END \
           FROM knowledge_units AS ku \
           JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id \
           LEFT JOIN graph_extraction_state AS cur \
             ON cur.org_id = ku.org_id AND cur.knowledge_id = ku.knowledge_id \
          WHERE ku.org_id = $1 \
            AND {population} \
            AND (cur.knowledge_id IS NULL \
                 OR (cur.status IN ({work}) \
                     AND (cur.status IS DISTINCT FROM $2::text OR cur.error_class IS DISTINCT FROM $3::text) \
                     AND (cur.lease_until IS NULL OR cur.lease_until <= now()))) \
          ORDER BY ku.knowledge_id \
          LIMIT $5 \
         ON CONFLICT (org_id, knowledge_id) DO UPDATE \
            SET status = EXCLUDED.status, \
                error_class = EXCLUDED.error_class, \
                next_attempt_at = EXCLUDED.next_attempt_at \
          WHERE s.status IN ({work}) AND (s.lease_until IS NULL OR s.lease_until <= now())",
        work = work_statuses(),
    )
}

/// The chunks of one document (`$6`).
static MARK_DOCUMENT_SQL: LazyLock<String> =
    LazyLock::new(|| mark_sql("ku.document_id = $6::text"));

/// The chunks of every live, organization-visible document whose classification may reach the
/// model: what a zero-retention organization, a policy that is off or a used-up budget withholds.
static MARK_ORG_SQL: LazyLock<String> = LazyLock::new(|| {
    mark_sql(&format!(
        "d.visibility = 'org' AND d.deleted_at IS NULL AND {}",
        allowed_classification_sql()
    ))
});

/// The chunks of live, organization-visible documents that are classified so they must not reach
/// the model.
static MARK_RESTRICTED_SQL: LazyLock<String> = LazyLock::new(|| {
    mark_sql(&format!(
        "d.visibility = 'org' AND d.deleted_at IS NULL AND NOT ({})",
        allowed_classification_sql()
    ))
});

/// The documents with work: live, organization-visible, allowed to reach the model, with at
/// least one chunk that has no state or is due. `$1` organization, `$2` how many, `$3` the
/// current prompt version. Ordered by id so the scan is deterministic; a chunk that keeps failing
/// is not due, so it cannot hold a document at the front of the line.
static DUE_DOCUMENTS_SQL: LazyLock<String> = LazyLock::new(|| {
    format!(
        "SELECT d.document_id \
           FROM documents AS d \
          WHERE d.org_id = $1 \
            AND d.visibility = 'org' \
            AND d.deleted_at IS NULL \
            AND {allowed} \
            AND EXISTS ( \
                  SELECT 1 \
                    FROM knowledge_units AS ku \
                    LEFT JOIN graph_extraction_state AS s \
                      ON s.org_id = ku.org_id AND s.knowledge_id = ku.knowledge_id \
                   WHERE ku.document_id = d.document_id AND ku.org_id = d.org_id \
                     AND (s.knowledge_id IS NULL OR {due})) \
          ORDER BY d.document_id \
          LIMIT $2",
        allowed = allowed_classification_sql(),
        due = due_predicate("s", "$3::text"),
    )
});

const STANDING_SQL: &str = "SELECT d.visibility, d.zdr_classification \
       FROM documents AS d \
      WHERE d.document_id = $1 AND d.org_id = $2 AND d.deleted_at IS NULL";

const POLICY_SQL: &str = "SELECT graph_extraction, budget_mode, monthly_budget_usd::float8 \
       FROM org_processing_policy WHERE org_id = $1";

/// The month's background LLM tokens of an organization, from the usage ledger: every version 2
/// event of kind `llm` (contextual retrieval, graph extraction, community summaries) since the
/// first of the month, UTC.
const MONTH_TOKENS_SQL: &str = "SELECT COALESCE(SUM(input_tokens), 0)::bigint, \
            COALESCE(SUM(output_tokens), 0)::bigint \
       FROM cost_events \
      WHERE org_id = $1 AND schema_version = 2 AND event_type = 'llm' \
        AND created_at >= date_trunc('month', now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'";

/// What the boot-time check reads: the objects this service cannot run without, and whether the
/// state table is protected.
const SCHEMA_SQL: &str = "SELECT \
       to_regclass('public.graph_extraction_state') IS NOT NULL, \
       COALESCE((SELECT relrowsecurity FROM pg_class WHERE oid = to_regclass('public.graph_extraction_state')), false), \
       to_regclass('public.org_processing_policy') IS NOT NULL, \
       EXISTS (SELECT 1 FROM information_schema.columns \
                WHERE table_schema = 'public' AND table_name = 'cost_events' AND column_name = 'input_tokens')";

/// The state table is missing or unprotected, or a table the gates read is not there. The message
/// names the migration to apply.
fn schema_problem(
    state_table: bool,
    state_protected: bool,
    policy_table: bool,
    ledger_columns: bool,
) -> Option<String> {
    let mut missing = Vec::new();
    if !state_table {
        missing.push(
            "table graph_extraction_state (migration 20261003150000_graph_extraction_state)"
                .to_owned(),
        );
    } else if !state_protected {
        missing.push(
            "row-level security on graph_extraction_state (migration 20261003150000_graph_extraction_state)"
                .to_owned(),
        );
    }
    if !policy_table {
        missing.push(
            "table org_processing_policy (migration 20261003130000_context_provenance)".to_owned(),
        );
    }
    if !ledger_columns {
        missing.push(
            "the version 2 columns of cost_events (migration 20261003140000_cost_events_v2)"
                .to_owned(),
        );
    }
    if missing.is_empty() {
        return None;
    }
    Some(format!(
        "the database is not at the schema this graph-index needs: missing {}. Apply the Data \
         Plane migrations (`docker compose run --rm migrate up`) before starting graph-index",
        missing.join("; ")
    ))
}

fn seconds(duration: Option<Duration>) -> Option<f64> {
    duration.map(|duration| duration.as_secs_f64())
}

impl GraphStore {
    /// The boot-time check: refuse to start against a database that lacks what the state machine
    /// reads and writes, with a message that names the migration. Without it the first chunk
    /// would fail on a missing relation and be counted against its poison cap.
    pub(crate) async fn verify_state_schema(&self) -> anyhow::Result<()> {
        let row = sqlx::query(SCHEMA_SQL).fetch_one(&self.pool).await?;
        match schema_problem(row.get(0), row.get(1), row.get(2), row.get(3)) {
            None => Ok(()),
            Some(problem) => anyhow::bail!(problem),
        }
    }

    /// Claim one chunk for an attempt. `None`: it is not due (final, waiting out its backoff,
    /// held by another worker's lease) or its document is no longer visible to the organization.
    pub(crate) async fn claim_chunk(
        &self,
        org_id: &str,
        knowledge_id: &str,
        lease: Duration,
    ) -> anyhow::Result<Option<Claim>> {
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;
        let row: Option<(i32,)> = sqlx::query_as(CLAIM_SQL.as_str())
            .bind(org_id)
            .bind(knowledge_id)
            .bind(lease.as_secs_f64())
            .bind(PROMPT_VERSION)
            .fetch_optional(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(row.map(|(attempts,)| Claim { attempts }))
    }

    /// Record what came of an attempt and release the lease. Returns whether the row was there
    /// (it goes with its knowledge unit when a document is re-chunked mid-attempt).
    pub(crate) async fn settle_chunk(
        &self,
        org_id: &str,
        knowledge_id: &str,
        settlement: &Settlement,
    ) -> anyhow::Result<bool> {
        let stamp = settlement.stamp.as_ref();
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;
        let result = sqlx::query(SETTLE_SQL)
            .bind(org_id)
            .bind(knowledge_id)
            .bind(settlement.status.as_str())
            .bind(settlement.error_class.map(|class| class.as_str()))
            .bind(seconds(settlement.retry_in))
            .bind(settlement.refund_attempt)
            .bind(stamp.map(|stamp| stamp.prompt_version.as_str()))
            .bind(stamp.map(|stamp| stamp.model.as_str()))
            .bind(stamp.map(|stamp| stamp.provider.as_str()))
            .bind(stamp.map(|stamp| stamp.entities))
            .bind(stamp.map(|stamp| stamp.relationships))
            .bind(stamp.map(|stamp| stamp.claims))
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(result.rows_affected() > 0)
    }

    /// Mark the chunks of one document with the reason they were not attempted.
    pub(crate) async fn mark_document(
        &self,
        org_id: &str,
        document_id: &str,
        settlement: &Settlement,
        limit: i64,
    ) -> anyhow::Result<u64> {
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;
        let result = sqlx::query(MARK_DOCUMENT_SQL.as_str())
            .bind(org_id)
            .bind(settlement.status.as_str())
            .bind(settlement.error_class.map(|class| class.as_str()))
            .bind(seconds(settlement.retry_in))
            .bind(limit)
            .bind(document_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(result.rows_affected())
    }

    /// Mark the chunks the organization-level gate withholds, a bounded batch at a time.
    pub(crate) async fn mark_org(
        &self,
        org_id: &str,
        settlement: &Settlement,
        limit: i64,
    ) -> anyhow::Result<u64> {
        self.mark_population(MARK_ORG_SQL.as_str(), org_id, settlement, limit)
            .await
    }

    /// Mark the chunks of restricted documents `skipped_zdr` / `zdr_document`: they are never
    /// sent, and a coverage figure should say why they have no graph.
    pub(crate) async fn mark_restricted_documents(
        &self,
        org_id: &str,
        settlement: &Settlement,
        limit: i64,
    ) -> anyhow::Result<u64> {
        self.mark_population(MARK_RESTRICTED_SQL.as_str(), org_id, settlement, limit)
            .await
    }

    async fn mark_population(
        &self,
        statement: &str,
        org_id: &str,
        settlement: &Settlement,
        limit: i64,
    ) -> anyhow::Result<u64> {
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;
        let result = sqlx::query(statement)
            .bind(org_id)
            .bind(settlement.status.as_str())
            .bind(settlement.error_class.map(|class| class.as_str()))
            .bind(seconds(settlement.retry_in))
            .bind(limit)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(result.rows_affected())
    }

    /// What a document may be used for right now. `None`: there is no such live document.
    pub(crate) async fn document_standing(
        &self,
        org_id: &str,
        document_id: &str,
    ) -> anyhow::Result<Option<Standing>> {
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;
        let row: Option<(String, String)> = sqlx::query_as(STANDING_SQL)
            .bind(document_id)
            .bind(org_id)
            .fetch_optional(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(row.map(|(visibility, classification)| Standing {
            visible: visibility == "org",
            classification,
        }))
    }

    /// The documents of one organization with work to do: see [`DUE_DOCUMENTS_SQL`].
    pub(crate) async fn list_due_documents(
        &self,
        org_id: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<String>> {
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;
        let rows: Vec<(String,)> = sqlx::query_as(DUE_DOCUMENTS_SQL.as_str())
            .bind(org_id)
            .bind(limit)
            .bind(PROMPT_VERSION)
            .fetch_all(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(rows.into_iter().map(|(document_id,)| document_id).collect())
    }

    /// The organization's processing policy, or the default of an absent row (everything
    /// inherited). An unknown value is an error, never a guess: the table's CHECKs make it
    /// unreachable, and a policy that cannot be read must not be read as "on".
    pub(crate) async fn org_policy(&self, org_id: &str) -> anyhow::Result<OrgPolicy> {
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;
        let row: Option<(String, String, Option<f64>)> = sqlx::query_as(POLICY_SQL)
            .bind(org_id)
            .fetch_optional(&mut *tx)
            .await?;
        tx.commit().await?;
        let Some((graph_extraction, budget_mode, monthly_budget_usd)) = row else {
            return Ok(OrgPolicy::DEFAULT);
        };
        Ok(OrgPolicy {
            graph_extraction: Switch::parse(&graph_extraction).ok_or_else(|| {
                anyhow::anyhow!("org_processing_policy.graph_extraction holds an unknown value")
            })?,
            budget_mode: match budget_mode.as_str() {
                "inherit" => None,
                other => Some(BudgetMode::parse(other).ok_or_else(|| {
                    anyhow::anyhow!("org_processing_policy.budget_mode holds an unknown value")
                })?),
            },
            monthly_budget_usd,
        })
    }

    /// The month's background LLM tokens (input, output) of an organization.
    pub(crate) async fn month_llm_tokens(&self, org_id: &str) -> anyhow::Result<(i64, i64)> {
        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;
        let row: (i64, i64) = sqlx::query_as(MONTH_TOKENS_SQL)
            .bind(org_id)
            .fetch_one(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(row)
    }
}

#[cfg(test)]
mod tests;
