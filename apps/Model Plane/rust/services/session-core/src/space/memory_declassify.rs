//! Declassified memory in Session Core (build plan WS17-S7; design WS17 D5).
//!
//! A member of a shared Space shares one of their personal memory rows into the room (`declassify`), or
//! removes the room copy they shared (`retract`, SD-22: the declassifying owner retracts; room managers
//! do not). Both are authorised by a verified Control `model.memory.declassify` decision
//! (`grpc::verify_memory_declassify_decision`). This module is the write: one transaction inserts the
//! `declassified` room row and its ledger row, and the retraction deletes the room row and stamps the
//! ledger. The source row is never changed. The ledger holds hashes and ids only; the reviewed text lives
//! in the room row alone.
//!
//! The caller, the org, the replay of a decision and the revocation check happen in `memory_grpc.rs`
//! before this module is called.
//!
//! W3 D1: a membership fact that removes a subject from a Space, or leaves them below editor, retracts that
//! subject's live shares in the Space through [`retract_removed_subject`], from `space_membership_retract_nats.rs`.
//! A share and that retraction take the same per-subject lock, and a share re-checks the revocation and the retraction
//! marks under it, so a removal cannot be overtaken by a share it did not see.
//!
//! W3-DEC-ORDER2: a share records the Control authority revision its decision binds (`authority_revision`). A removal
//! or demotion at revision R retracts only the subject's shares below R, so a share made after a later re-add survives.

use chrono::{DateTime, Utc};
use mp_ids::new_ulid;
use sqlx::{PgPool, Postgres, Transaction};
use tonic::Status;

use crate::space::memory_scope::ORIGIN_DECLASSIFIED;
use crate::space_memory::{SOURCE_MEMBER_SAVE, SPACE_SCOPE};

/// One declassify, already authorised: the Control decision is verified and the caller is the source row's
/// owner. `audience_hash`, `audience_revision` and `authority_revision` are the verified decision's, never the request's.
pub(crate) struct DeclassifyInput<'a> {
    pub(crate) org_id: &'a str,
    pub(crate) user_id: &'a str,
    pub(crate) space_id: &'a str,
    pub(crate) source_memory_id: &'a str,
    pub(crate) reviewed_content: &'a str,
    pub(crate) content_digest: &'a str,
    pub(crate) decision_ref: &'a str,
    pub(crate) idempotency_key: &'a str,
    pub(crate) audience_hash: &'a str,
    pub(crate) audience_revision: i64,
    /// The Space authority revision the verified decision binds (W3-DEC-ORDER2), recorded on the ledger row.
    pub(crate) authority_revision: i64,
}

/// What a declassify produced: the room row, and whether this decision or key had already produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeclassifyOutcome {
    pub(crate) memory_id: String,
    pub(crate) deduplicated: bool,
}

fn internal(error: sqlx::Error) -> Status {
    Status::internal(error.to_string())
}

/// The advisory lock a share and a membership retraction both take on (org, Space, subject) first thing in their
/// transaction (W3 D1). The two serialise per person per room, so a share that commits first is retracted by the
/// removal, and a share that starts after the removal is refused by the revocation check under the same lock.
const LOCK_MEMBER_SQL: &str =
    "SELECT pg_advisory_xact_lock(hashtextextended($1::text || '|' || $2::text || '|' || $3::text, 0))";

/// Whether a membership fact obliges the subject's shares in the Space to be retracted (W3 D1): a removal, or a
/// role below editor, the lowest role that may declassify (Control `roles.go`: viewer < editor < manager < owner).
/// The payload carries the current role of an active member. An empty or unknown role is treated as below editor,
/// so the fail-closed side is the retraction.
pub(crate) fn retraction_required(active: bool, role: &str) -> bool {
    !active || !matches!(role.trim(), "editor" | "manager" | "owner")
}

/// Share one personal memory row into a shared Space. A replay of the same decision, or of the same
/// (subject, Space, idempotency key), returns the first room row and writes nothing. A share of a row that
/// was retracted is refused, so a replay cannot bring a removed copy back.
pub(crate) async fn declassify_into_room(
    pool: &PgPool,
    input: DeclassifyInput<'_>,
) -> Result<DeclassifyOutcome, Status> {
    let mut tx = pool.begin().await.map_err(internal)?;

    // W3 D1: serialise with a retraction of this subject in this Space, then refuse a share that a removal has
    // already revoked. The handler's own revocation check runs before this transaction and can be stale by now.
    sqlx::query(LOCK_MEMBER_SQL)
        .bind(input.org_id)
        .bind(input.space_id)
        .bind(input.user_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    let revoked: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM space_membership_revocations WHERE space_ref = $1 AND subject_id = $2)",
    )
    .bind(input.space_id)
    .bind(input.user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal)?;
    if revoked {
        tx.rollback().await.map_err(internal)?;
        return Err(Status::permission_denied(
            "Space membership required for this Space's memory",
        ));
    }

    // Replay: the same decision, or the same subject, Space and idempotency key.
    let replay: Option<(String, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT room_memory_id, retracted_at FROM memory_declassifications \
         WHERE decision_ref = $1 \
            OR (org_id = $2 AND subject_id = $3 AND space_ref = $4 AND idempotency_key = $5) \
         LIMIT 1",
    )
    .bind(input.decision_ref)
    .bind(input.org_id)
    .bind(input.user_id)
    .bind(input.space_id)
    .bind(input.idempotency_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    if let Some((memory_id, retracted_at)) = replay {
        if retracted_at.is_some() {
            return Err(Status::failed_precondition(
                "memory declassification was retracted",
            ));
        }
        return Ok(DeclassifyOutcome {
            memory_id,
            deduplicated: true,
        });
    }

    // W3-DEC-ORDER2: a removal or demotion at a later revision has already retracted this subject's shares, and this
    // decision predates it. Refuse, rather than leave a copy that retraction has already passed.
    let retracted_through: Option<i64> = sqlx::query_scalar(
        "SELECT retract_seq FROM space_membership_retract_marks WHERE space_ref = $1 AND subject_id = $2",
    )
    .bind(input.space_id)
    .bind(input.user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    if retracted_through.is_some_and(|seq| seq > input.authority_revision) {
        tx.rollback().await.map_err(internal)?;
        return Err(Status::permission_denied(
            "Space membership changed after this memory decision was issued",
        ));
    }

    // The source row: the caller's own, current, accepted, not expired and not forgotten personal row. It is
    // locked for the share so a concurrent forget or edit waits for the share to finish.
    let source: Option<(String, f64)> = sqlx::query_as(
        "SELECT kind, confidence FROM agent_memory \
         WHERE id = $1 AND org_id = $2 AND owner = $3 AND scope = 'user' \
           AND origin_kind = 'personal' AND review_state = 'accepted' AND is_latest = true \
           AND (expires_at IS NULL OR expires_at > now()) \
           AND NOT EXISTS (SELECT 1 FROM forgotten_memory_ids f \
                           WHERE f.org_id = $2 AND f.user_id = $3 AND f.memory_id = $1) \
         FOR SHARE",
    )
    .bind(input.source_memory_id)
    .bind(input.org_id)
    .bind(input.user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    let Some((kind, confidence)) = source else {
        return Err(Status::not_found("memory not found"));
    };

    // The room row. Keyed by the Space and the reviewed text's digest (two rooms never share a key, the
    // `space_key` rule), so the same text shared twice in one room is one row. Labelled `declassified` with
    // the verified audience, and no Letta mirror (it is written by this statement alone).
    let source_links = vec![
        format!("declassified:{}", input.source_memory_id),
        format!("decision:{}", input.decision_ref),
    ];
    let key = format!("{}/declassified:{}", input.space_id, input.content_digest);
    let room_memory_id: Option<String> = sqlx::query_scalar(
        "INSERT INTO agent_memory \
         (id, org_id, session_id, scope, scope_id, key, content, kind, confidence, owner, source_links, \
          review_state, audience_hash, audience_revision, source_label, origin_kind, space_ref) \
         VALUES ($1, $2, NULL, $3, $4, $5, $6, $7, $8, '', $9, 'accepted', $10, $11, $12, $13, $4) \
         ON CONFLICT (org_id, scope, owner, key) WHERE session_id IS NULL \
         DO UPDATE SET \
           content = EXCLUDED.content, \
           kind = EXCLUDED.kind, \
           confidence = GREATEST(agent_memory.confidence, EXCLUDED.confidence), \
           source_links = EXCLUDED.source_links, \
           review_state = 'accepted', \
           scope_id = EXCLUDED.scope_id, \
           audience_hash = EXCLUDED.audience_hash, \
           audience_revision = EXCLUDED.audience_revision, \
           source_label = EXCLUDED.source_label, \
           origin_kind = EXCLUDED.origin_kind, \
           space_ref = EXCLUDED.space_ref, \
           updated_at = now() \
         WHERE agent_memory.is_latest \
         RETURNING id",
    )
    .bind(new_ulid())
    .bind(input.org_id)
    .bind(SPACE_SCOPE)
    .bind(input.space_id)
    .bind(&key)
    .bind(input.reviewed_content)
    .bind(&kind)
    .bind(confidence)
    .bind(source_links.as_slice())
    .bind(input.audience_hash)
    .bind(input.audience_revision)
    .bind(SOURCE_MEMBER_SAVE)
    .bind(ORIGIN_DECLASSIFIED)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    let room_memory_id = room_memory_id
        .ok_or_else(|| Status::failed_precondition("room memory row is not current"))?;

    // The ledger row. The decision reference and the (subject, Space, key) are both unique, so a concurrent
    // share of the same decision or key inserts nothing here.
    let inserted: Option<String> = sqlx::query_scalar(
        "INSERT INTO memory_declassifications \
         (decision_ref, org_id, space_ref, source_memory_id, room_memory_id, subject_id, idempotency_key, \
          content_digest, audience_hash, audience_revision, authority_revision) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
         ON CONFLICT DO NOTHING \
         RETURNING decision_ref",
    )
    .bind(input.decision_ref)
    .bind(input.org_id)
    .bind(input.space_id)
    .bind(input.source_memory_id)
    .bind(&room_memory_id)
    .bind(input.user_id)
    .bind(input.idempotency_key)
    .bind(input.content_digest)
    .bind(input.audience_hash)
    .bind(input.audience_revision)
    .bind(input.authority_revision)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    if inserted.is_none() {
        // A concurrent share won the ledger row: write nothing new and return the row it produced.
        tx.rollback().await.map_err(internal)?;
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT room_memory_id FROM memory_declassifications \
             WHERE decision_ref = $1 \
                OR (org_id = $2 AND subject_id = $3 AND space_ref = $4 AND idempotency_key = $5) \
             LIMIT 1",
        )
        .bind(input.decision_ref)
        .bind(input.org_id)
        .bind(input.user_id)
        .bind(input.space_id)
        .bind(input.idempotency_key)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
        return existing
            .map(|memory_id| DeclassifyOutcome {
                memory_id,
                deduplicated: true,
            })
            .ok_or_else(|| Status::aborted("memory declassification raced; retry"));
    }
    tx.commit().await.map_err(internal)?;
    Ok(DeclassifyOutcome {
        memory_id: room_memory_id,
        deduplicated: false,
    })
}

/// Retract the room copy one subject shared (SD-22: the declassifying owner retracts). The ledger must hold
/// a live share of that row by this subject; the room row is deleted and every live ledger row of it is
/// stamped retracted, so no replay brings it back. Returns false when this subject has no live share of the
/// row, which the caller reports as not found. The source row is never touched.
pub(crate) async fn retract_declassified_row(
    pool: &PgPool,
    org_id: &str,
    space_id: &str,
    memory_id: &str,
    subject_id: &str,
) -> Result<bool, Status> {
    let mut tx = pool.begin().await.map_err(internal)?;
    let shared_by_subject: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM memory_declassifications \
         WHERE org_id = $1 AND space_ref = $2 AND room_memory_id = $3 \
           AND subject_id = $4 AND retracted_at IS NULL)",
    )
    .bind(org_id)
    .bind(space_id)
    .bind(memory_id)
    .bind(subject_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal)?;
    if !shared_by_subject {
        tx.rollback().await.map_err(internal)?;
        return Ok(false);
    }
    let deleted: Option<String> = sqlx::query_scalar(
        "DELETE FROM agent_memory \
         WHERE id = $1 AND org_id = $2 AND scope = 'space' AND scope_id = $3 \
           AND origin_kind = 'declassified' \
         RETURNING id",
    )
    .bind(memory_id)
    .bind(org_id)
    .bind(space_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    sqlx::query(
        "UPDATE memory_declassifications SET retracted_at = now() \
         WHERE org_id = $1 AND space_ref = $2 AND room_memory_id = $3 AND retracted_at IS NULL",
    )
    .bind(org_id)
    .bind(space_id)
    .bind(memory_id)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(deleted.is_some())
}

/// W3 D1 and W3-DEC-ORDER2: retract the live declassified shares one subject made in one Space, when Control says the
/// subject was removed (`revoke`, which also writes the revocation row, as the membership consumer does), demoted below
/// editor (`revoke` false), or on the sweep. `before` is the Space authority revision of that change. Only the subject's
/// shares whose recorded revision is below it are retracted; a share with a NULL revision (written before the column
/// existed) always is. `None` (an unordered fact) retracts every live share of the subject. `before` also records the
/// revision in `space_membership_retract_marks`, so `declassify_into_room` refuses a share authorised below it from now on.
///
/// The caller holds the subject's ordering lock and runs this inside its transaction. This takes the (org, Space, subject)
/// lock that declassify shares, so the two serialise. Per room row, in row order: the room row is locked, this subject's
/// shares of it are stamped, and the row is deleted only when no live share of any member still holds it (a copy another
/// member shared after the change stays). Returns the shares retracted. A repeat finds no live share and changes nothing.
/// The source rows are never changed.
pub(crate) async fn retract_removed_subject(
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    space_id: &str,
    subject_id: &str,
    revoke: bool,
    before: Option<i64>,
) -> Result<u64, sqlx::Error> {
    sqlx::query(LOCK_MEMBER_SQL)
        .bind(org_id)
        .bind(space_id)
        .bind(subject_id)
        .execute(&mut **tx)
        .await?;
    if revoke {
        sqlx::query(
            "INSERT INTO space_membership_revocations (space_ref, org_id, subject_id) VALUES ($1, $2, $3) \
             ON CONFLICT (space_ref, subject_id) DO UPDATE SET org_id = EXCLUDED.org_id, revoked_at = NOW()",
        )
        .bind(space_id)
        .bind(org_id)
        .bind(subject_id)
        .execute(&mut **tx)
        .await?;
    }
    if let Some(space_seq) = before {
        sqlx::query(
            "INSERT INTO space_membership_retract_marks (space_ref, subject_id, org_id, retract_seq) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (space_ref, subject_id) DO UPDATE \
             SET org_id = EXCLUDED.org_id, \
                 retract_seq = GREATEST(space_membership_retract_marks.retract_seq, EXCLUDED.retract_seq), \
                 applied_at = NOW()",
        )
        .bind(space_id)
        .bind(subject_id)
        .bind(org_id)
        .bind(space_seq)
        .execute(&mut **tx)
        .await?;
    }
    let room_ids: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT room_memory_id FROM memory_declassifications \
         WHERE org_id = $1 AND space_ref = $2 AND subject_id = $3 AND retracted_at IS NULL \
           AND ($4::bigint IS NULL OR authority_revision IS NULL OR authority_revision < $4) \
         ORDER BY room_memory_id",
    )
    .bind(org_id)
    .bind(space_id)
    .bind(subject_id)
    .bind(before)
    .fetch_all(&mut **tx)
    .await?;
    let mut retracted: u64 = 0;
    for memory_id in &room_ids {
        // Lock the room row before the ledger is read again. A share of the same text holds this row until it commits, so
        // the delete below sees that share's ledger row, and a share that starts after this waits for the delete.
        sqlx::query(
            "SELECT id FROM agent_memory WHERE id = $1 AND org_id = $2 AND scope = 'space' AND scope_id = $3 \
               AND origin_kind = 'declassified' FOR UPDATE",
        )
        .bind(memory_id)
        .bind(org_id)
        .bind(space_id)
        .fetch_optional(&mut **tx)
        .await?;
        retracted += sqlx::query(
            "UPDATE memory_declassifications SET retracted_at = now() \
             WHERE org_id = $1 AND space_ref = $2 AND subject_id = $3 AND room_memory_id = $4 AND retracted_at IS NULL \
               AND ($5::bigint IS NULL OR authority_revision IS NULL OR authority_revision < $5)",
        )
        .bind(org_id)
        .bind(space_id)
        .bind(subject_id)
        .bind(memory_id)
        .bind(before)
        .execute(&mut **tx)
        .await?
        .rows_affected();
        sqlx::query(
            "DELETE FROM agent_memory \
             WHERE id = $1 AND org_id = $2 AND scope = 'space' AND scope_id = $3 \
               AND origin_kind = 'declassified' \
               AND NOT EXISTS (SELECT 1 FROM memory_declassifications \
                               WHERE org_id = $2 AND space_ref = $3 AND room_memory_id = $1 AND retracted_at IS NULL)",
        )
        .bind(memory_id)
        .bind(org_id)
        .bind(space_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(retracted)
}

/// W3 D1 backstop: the live declassified shares whose subject holds a revocation row in their Space, as
/// `(org, Space, subject)`. The revocation projection is durable, so a sweep over it converges a copy even when the
/// membership event itself was missed by the consumer that writes the projection.
pub(crate) async fn revoked_live_shares(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<(String, String, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT DISTINCT m.org_id, m.space_ref, m.subject_id FROM memory_declassifications m \
         WHERE m.retracted_at IS NULL \
           AND EXISTS(SELECT 1 FROM space_membership_revocations r \
                      WHERE r.space_ref = m.space_ref AND r.subject_id = m.subject_id) \
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}
