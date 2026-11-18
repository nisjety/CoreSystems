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
    /// The sharing member's D18 timestamp floor, `None` without a window. Under a floor the share re-attaches
    /// to the room row of its key only when that row is one the floor admits; an older row the member may not
    /// see is never updated through the share (the share is then refused as a room row that is not current).
    pub(crate) history_floor_at: Option<DateTime<Utc>>,
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
    // D18: a windowed sharer may update the existing row of this key only when the floor admits that row. A row
    // said before the window is not updated (the share is refused below), so a share never rewrites or re-labels
    // a room row the member cannot see. With no floor the predicate is TRUE and the share is what it was.
    let floor_admits = crate::space::memory_scope::history_floor_admits_sql("$14");
    let room_memory_id: Option<String> = sqlx::query_scalar(&format!(
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
         WHERE agent_memory.is_latest AND {floor_admits} \
         RETURNING id",
    ))
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
    .bind(input.history_floor_at)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::test_support::connect;

    // ---- W3-DEC-RETRACT: retraction_required (pure) --------------------------------------------

    #[test]
    fn retraction_required_is_true_for_active_false_with_any_role_and_for_active_true_below_editor() {
        for role in ["viewer", "manager", "owner", "editor", "", "unknown"] {
            assert!(
                retraction_required(false, role),
                "an inactive member must retract whatever role the fact carries ({role:?})"
            );
        }
        for role in ["viewer", "", "   ", "unknown", "VIEWER", "Editor"] {
            assert!(
                retraction_required(true, role),
                "an active member below editor (or an unknown/blank role) must retract ({role:?})"
            );
        }
    }

    #[test]
    fn retraction_required_is_false_for_active_true_with_editor_manager_or_owner() {
        for role in ["editor", "manager", "owner"] {
            assert!(
                !retraction_required(true, role),
                "an active {role} keeps declassify rights"
            );
        }
    }

    // ---- DB-tier fixtures ------------------------------------------------------------------------

    /// A 64-hex-character `sha256:` digest, the only shape `memory_declassifications.content_digest`
    /// and `declassify_into_room`'s `content_digest` field accept.
    fn digest(tag: u8) -> String {
        format!("sha256:{}", hex::encode([tag; 32]))
    }

    mod hex {
        /// A tiny lowercase-hex encoder so the DB-tier tests below need no extra dependency.
        pub(super) fn encode(bytes: [u8; 32]) -> String {
            bytes.iter().map(|b| format!("{b:02x}")).collect()
        }
    }

    async fn seed_personal_memory(pool: &PgPool, org: &str, owner: &str, id: &str, key: &str) {
        sqlx::query(
            "INSERT INTO agent_memory (id, org_id, owner, scope, key, content, kind, confidence, review_state, is_latest) \
             VALUES ($1, $2, $3, 'user', $4, 'declassify test content', 'fact', 1.0, 'accepted', true)",
        )
        .bind(id)
        .bind(org)
        .bind(owner)
        .bind(key)
        .execute(pool)
        .await
        .expect("seed personal memory");
    }

    fn share<'a>(
        org: &'a str,
        user: &'a str,
        space: &'a str,
        source_memory_id: &'a str,
        content_digest: &'a str,
        decision_ref: &'a str,
        idempotency_key: &'a str,
        authority_revision: i64,
    ) -> DeclassifyInput<'a> {
        DeclassifyInput {
            org_id: org,
            user_id: user,
            space_id: space,
            source_memory_id,
            reviewed_content: "reviewed text",
            content_digest,
            decision_ref,
            idempotency_key,
            audience_hash: "sha256:test-audience",
            audience_revision: 1,
            authority_revision,
            history_floor_at: None,
        }
    }

    /// Removes everything a declassify test created, across every table this module writes
    /// (`space::test_support::clean_space` does not reach these).
    async fn clean_declassify_world(pool: &PgPool, org: &str, space: &str) {
        for sql in [
            "DELETE FROM memory_declassifications WHERE org_id = $1 AND space_ref = $2",
            "DELETE FROM space_membership_retract_marks WHERE org_id = $1 AND space_ref = $2",
            "DELETE FROM space_membership_revocations WHERE org_id = $1 AND space_ref = $2",
            "DELETE FROM agent_memory WHERE org_id = $1",
        ] {
            sqlx::query(sql).bind(org).bind(space).execute(pool).await.ok();
        }
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn a_share_replays_on_its_decision_or_key_and_writes_one_ledger_row() {
        let Some(pool) = connect().await else {
            return;
        };
        let org = format!("declassify-org-{}", mp_ids::new_ulid());
        let space = format!("declassify-space-{}", mp_ids::new_ulid());
        let user = "owner-1";
        let source_id = format!("mem-{}", mp_ids::new_ulid());
        seed_personal_memory(&pool, &org, user, &source_id, "pref-1").await;

        let decision_ref = format!("decision-{}", mp_ids::new_ulid());
        let text_digest = digest(1);
        let first = declassify_into_room(
            &pool,
            share(&org, user, &space, &source_id, &text_digest, &decision_ref, "idem-1", 1),
        )
        .await
        .expect("first share");
        assert!(!first.deduplicated);

        // A replay of the same decision_ref (the client retried) returns the same row and writes
        // nothing new, even though every other field is given again.
        let replay = declassify_into_room(
            &pool,
            share(&org, user, &space, &source_id, &text_digest, &decision_ref, "idem-1", 1),
        )
        .await
        .expect("replay");
        assert!(replay.deduplicated);
        assert_eq!(replay.memory_id, first.memory_id);

        let ledger_rows: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM memory_declassifications WHERE org_id = $1 AND space_ref = $2",
        )
        .bind(&org)
        .bind(&space)
        .fetch_one(&pool)
        .await
        .expect("count ledger rows");
        assert_eq!(ledger_rows, 1, "a replay must never write a second ledger row");

        clean_declassify_world(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn a_windowed_share_never_reattaches_to_a_room_row_said_before_its_window() {
        let Some(pool) = connect().await else {
            return;
        };
        let org = format!("declassify-org-{}", mp_ids::new_ulid());
        let space = format!("declassify-space-{}", mp_ids::new_ulid());
        let user = "owner-1";
        let source_id = format!("mem-{}", mp_ids::new_ulid());
        seed_personal_memory(&pool, &org, user, &source_id, "pref-w").await;

        // The room already holds this text, shared before the sharer's window (an unwindowed share, aged).
        let text = digest(7);
        let earlier_decision = format!("decision-{}", mp_ids::new_ulid());
        let earlier = declassify_into_room(
            &pool,
            share(&org, user, &space, &source_id, &text, &earlier_decision, "idem-e", 1),
        )
        .await
        .expect("the earlier share");
        sqlx::query("UPDATE agent_memory SET created_at = now() - interval '3 hours' WHERE id = $1")
            .bind(&earlier.memory_id)
            .execute(&pool)
            .await
            .expect("the room row predates the window");

        // A sharer whose window starts an hour ago may not re-attach to that row: the share is refused, and the
        // row is left as it was (its source and decision are the earlier ones).
        let window = chrono::Utc::now() - chrono::Duration::hours(1);
        let windowed_decision = format!("decision-{}", mp_ids::new_ulid());
        let mut refused = share(&org, user, &space, &source_id, &text, &windowed_decision, "idem-w", 1);
        refused.history_floor_at = Some(window);
        let outcome = declassify_into_room(&pool, refused).await;
        assert_eq!(
            outcome.expect_err("a pre-window room row is not re-attached").code(),
            tonic::Code::FailedPrecondition
        );
        let links: Vec<String> = sqlx::query_scalar("SELECT source_links FROM agent_memory WHERE id = $1")
            .bind(&earlier.memory_id)
            .fetch_one(&pool)
            .await
            .expect("the earlier room row");
        assert!(
            links.contains(&format!("decision:{earlier_decision}")) && !links.contains(&format!("decision:{windowed_decision}")),
            "the refused share wrote nothing to the row: {links:?}"
        );

        // A share of text the window's sharer is new to lands in the room as a new row.
        let fresh_decision = format!("decision-{}", mp_ids::new_ulid());
        let fresh_text = digest(8);
        let mut fresh = share(&org, user, &space, &source_id, &fresh_text, &fresh_decision, "idem-f", 1);
        fresh.history_floor_at = Some(window);
        let created = declassify_into_room(&pool, fresh).await.expect("a share of new text is admitted");
        assert!(!created.deduplicated);
        assert_ne!(created.memory_id, earlier.memory_id);

        clean_declassify_world(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn a_revoked_subject_is_refused_before_any_row_is_written() {
        let Some(pool) = connect().await else {
            return;
        };
        let org = format!("declassify-org-{}", mp_ids::new_ulid());
        let space = format!("declassify-space-{}", mp_ids::new_ulid());
        let user = "owner-2";
        let source_id = format!("mem-{}", mp_ids::new_ulid());
        seed_personal_memory(&pool, &org, user, &source_id, "pref-1").await;
        sqlx::query(
            "INSERT INTO space_membership_revocations (space_ref, org_id, subject_id) VALUES ($1, $2, $3)",
        )
        .bind(&space)
        .bind(&org)
        .bind(user)
        .execute(&pool)
        .await
        .expect("seed revocation");

        let decision_ref = format!("decision-{}", mp_ids::new_ulid());
        let refused = declassify_into_room(
            &pool,
            share(&org, user, &space, &source_id, &digest(2), &decision_ref, "idem-1", 1),
        )
        .await
        .expect_err("a revoked subject must be refused");
        assert_eq!(refused.code(), tonic::Code::PermissionDenied);

        let rows: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM memory_declassifications WHERE org_id = $1 AND space_ref = $2",
        )
        .bind(&org)
        .bind(&space)
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(rows, 0, "a refused share must write no ledger row");

        clean_declassify_world(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn a_decision_below_the_retract_mark_is_refused_and_one_at_or_above_it_is_not() {
        let Some(pool) = connect().await else {
            return;
        };
        let org = format!("declassify-org-{}", mp_ids::new_ulid());
        let space = format!("declassify-space-{}", mp_ids::new_ulid());
        let user = "owner-3";
        let source_id = format!("mem-{}", mp_ids::new_ulid());
        seed_personal_memory(&pool, &org, user, &source_id, "pref-1").await;
        sqlx::query(
            "INSERT INTO space_membership_retract_marks (space_ref, subject_id, org_id, retract_seq) \
             VALUES ($1, $2, $3, 5)",
        )
        .bind(&space)
        .bind(user)
        .bind(&org)
        .execute(&pool)
        .await
        .expect("seed retract mark");

        let below = declassify_into_room(
            &pool,
            share(&org, user, &space, &source_id, &digest(3), "decision-below", "idem-below", 3),
        )
        .await
        .expect_err("a decision below the retract mark must be refused");
        assert_eq!(below.code(), tonic::Code::PermissionDenied);

        // At the mark (not strictly above it) the decision is not refused by this check.
        let at_mark = declassify_into_room(
            &pool,
            share(&org, user, &space, &source_id, &digest(4), "decision-at-mark", "idem-at-mark", 5),
        )
        .await
        .expect("a decision at the retract mark is not refused by it");
        assert!(!at_mark.deduplicated);

        clean_declassify_world(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn retract_removed_subject_stamps_only_shares_below_the_revision_and_keeps_the_rest_live() {
        let Some(pool) = connect().await else {
            return;
        };
        let org = format!("declassify-org-{}", mp_ids::new_ulid());
        let space = format!("declassify-space-{}", mp_ids::new_ulid());
        let user = "owner-4";
        let old_source = format!("mem-old-{}", mp_ids::new_ulid());
        let new_source = format!("mem-new-{}", mp_ids::new_ulid());
        seed_personal_memory(&pool, &org, user, &old_source, "pref-old").await;
        seed_personal_memory(&pool, &org, user, &new_source, "pref-new").await;

        // One share authorised before the removal (revision 1) and one authorised after a later
        // re-add (revision 10); the removal below retracts revision 10 at the (not-inclusive) bound.
        let old_share = declassify_into_room(
            &pool,
            share(&org, user, &space, &old_source, &digest(5), "decision-old", "idem-old", 1),
        )
        .await
        .expect("old share");
        let new_share = declassify_into_room(
            &pool,
            share(&org, user, &space, &new_source, &digest(6), "decision-new", "idem-new", 10),
        )
        .await
        .expect("new share (after the re-add)");

        let mut tx = pool.begin().await.expect("begin");
        let retracted = retract_removed_subject(&mut tx, &org, &space, user, true, Some(5))
            .await
            .expect("retract removed subject");
        tx.commit().await.expect("commit");
        assert_eq!(retracted, 1, "only the share below revision 5 is retracted");

        let old_room: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_memory WHERE id = $1")
            .bind(&old_share.memory_id)
            .fetch_one(&pool)
            .await
            .expect("old room row count");
        assert_eq!(old_room, 0, "the pre-removal share's room row is deleted");

        let new_room: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_memory WHERE id = $1")
            .bind(&new_share.memory_id)
            .fetch_one(&pool)
            .await
            .expect("new room row count");
        assert_eq!(new_room, 1, "the share made after the re-add survives");

        let retracted_at_new: Option<chrono::DateTime<Utc>> = sqlx::query_scalar(
            "SELECT retracted_at FROM memory_declassifications WHERE org_id = $1 AND subject_id = $2 \
               AND room_memory_id = $3",
        )
        .bind(&org)
        .bind(user)
        .bind(&new_share.memory_id)
        .fetch_one(&pool)
        .await
        .expect("new ledger row");
        assert!(retracted_at_new.is_none(), "the surviving share's ledger row stays live");

        // A repeat retraction finds no live share below the revision and changes nothing.
        let mut tx = pool.begin().await.expect("begin repeat");
        let repeat = retract_removed_subject(&mut tx, &org, &space, user, false, Some(5))
            .await
            .expect("repeat retract");
        tx.commit().await.expect("commit repeat");
        assert_eq!(repeat, 0, "a repeat retraction changes nothing");

        clean_declassify_world(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn retract_removed_subject_deletes_the_room_row_only_when_no_live_share_remains() {
        let Some(pool) = connect().await else {
            return;
        };
        let org = format!("declassify-org-{}", mp_ids::new_ulid());
        let space = format!("declassify-space-{}", mp_ids::new_ulid());
        let subject_a = "owner-5a";
        let subject_b = "owner-5b";
        let source_a = format!("mem-a-{}", mp_ids::new_ulid());
        let source_b = format!("mem-b-{}", mp_ids::new_ulid());
        seed_personal_memory(&pool, &org, subject_a, &source_a, "pref-a").await;
        seed_personal_memory(&pool, &org, subject_b, &source_b, "pref-b").await;

        // Both subjects share the same reviewed text into the same room: one room row, two ledger
        // rows (the room key is the Space and the text digest, never the subject).
        let text_digest = digest(7);
        let shared = declassify_into_room(
            &pool,
            share(&org, subject_a, &space, &source_a, &text_digest, "decision-a", "idem-a", 1),
        )
        .await
        .expect("subject a shares");
        let also_shared = declassify_into_room(
            &pool,
            share(&org, subject_b, &space, &source_b, &text_digest, "decision-b", "idem-b", 1),
        )
        .await
        .expect("subject b shares the same text");
        assert_eq!(shared.memory_id, also_shared.memory_id, "one room row for one text in one Space");

        let mut tx = pool.begin().await.expect("begin a");
        retract_removed_subject(&mut tx, &org, &space, subject_a, true, None)
            .await
            .expect("retract subject a");
        tx.commit().await.expect("commit a");

        let still_live: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_memory WHERE id = $1")
            .bind(&shared.memory_id)
            .fetch_one(&pool)
            .await
            .expect("room row after a's retraction");
        assert_eq!(still_live, 1, "subject b's live share keeps the room row");

        let mut tx = pool.begin().await.expect("begin b");
        retract_removed_subject(&mut tx, &org, &space, subject_b, true, None)
            .await
            .expect("retract subject b");
        tx.commit().await.expect("commit b");

        let gone: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_memory WHERE id = $1")
            .bind(&shared.memory_id)
            .fetch_one(&pool)
            .await
            .expect("room row after both retractions");
        assert_eq!(gone, 0, "the room row is deleted once no live share references it");

        clean_declassify_world(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn revoked_live_shares_returns_only_shares_of_a_subject_holding_a_revocation() {
        let Some(pool) = connect().await else {
            return;
        };
        let org = format!("declassify-org-{}", mp_ids::new_ulid());
        let space = format!("declassify-space-{}", mp_ids::new_ulid());
        let revoked_subject = "owner-6-revoked";
        let live_subject = "owner-6-live";
        let revoked_source = format!("mem-r-{}", mp_ids::new_ulid());
        let live_source = format!("mem-l-{}", mp_ids::new_ulid());
        seed_personal_memory(&pool, &org, revoked_subject, &revoked_source, "pref-r").await;
        seed_personal_memory(&pool, &org, live_subject, &live_source, "pref-l").await;
        declassify_into_room(
            &pool,
            share(&org, revoked_subject, &space, &revoked_source, &digest(8), "decision-r", "idem-r", 1),
        )
        .await
        .expect("revoked subject shares");
        declassify_into_room(
            &pool,
            share(&org, live_subject, &space, &live_source, &digest(9), "decision-l", "idem-l", 1),
        )
        .await
        .expect("live subject shares");
        sqlx::query(
            "INSERT INTO space_membership_revocations (space_ref, org_id, subject_id) VALUES ($1, $2, $3)",
        )
        .bind(&space)
        .bind(&org)
        .bind(revoked_subject)
        .execute(&pool)
        .await
        .expect("seed revocation");

        let rows = revoked_live_shares(&pool, 1000).await.expect("revoked_live_shares");
        let matches: Vec<_> = rows
            .iter()
            .filter(|(row_org, row_space, _)| row_org == &org && row_space == &space)
            .collect();
        assert_eq!(matches.len(), 1, "only the revoked subject's share is returned");
        assert_eq!(matches[0].2, revoked_subject);

        clean_declassify_world(&pool, &org, &space).await;
    }
}
