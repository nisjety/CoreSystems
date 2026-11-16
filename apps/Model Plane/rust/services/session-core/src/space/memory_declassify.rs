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

use chrono::{DateTime, Utc};
use mp_ids::new_ulid;
use sqlx::PgPool;
use tonic::Status;

use crate::space::memory_scope::ORIGIN_DECLASSIFIED;
use crate::space_memory::{SOURCE_MEMBER_SAVE, SPACE_SCOPE};

/// One declassify, already authorised: the Control decision is verified and the caller is the source row's
/// owner. `audience_hash` and `audience_revision` are the verified decision's, never the request's.
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

/// Share one personal memory row into a shared Space. A replay of the same decision, or of the same
/// (subject, Space, idempotency key), returns the first room row and writes nothing. A share of a row that
/// was retracted is refused, so a replay cannot bring a removed copy back.
pub(crate) async fn declassify_into_room(
    pool: &PgPool,
    input: DeclassifyInput<'_>,
) -> Result<DeclassifyOutcome, Status> {
    let mut tx = pool.begin().await.map_err(internal)?;

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
          content_digest, audience_hash, audience_revision) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
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
