//! `SpaceAttention.GetSpaceAttention`: the cross-room attention aggregate (build plan WS11-S2).
//!
//! One call answers, for up to 60 Spaces, how many threads hold something new for the caller,
//! when the room last moved, how many root runs are working, which approvals wait for the
//! caller, and display titles for the thread ids the caller names.
//!
//! Authority, in order: a user principal (a service identity has no user id); `session:read`;
//! the caller's org; the user id equals the caller; `SPACE_ATTENTION_SUMMARY`; request bounds; a
//! zero-retention caller (every Space answers ZDR, no decision is verified and no row is read);
//! then per Space the Control `model.thread.read` decision (a failure, or no decision at all, is
//! DENIED for that Space only, never an error for the call) and the membership revocation row.
//! The decision's audience revision is the ceiling for every thread and message counted or
//! named. The index is never the access boundary: each query restates org, Space, archive and
//! ceiling predicates.
//!
//! No migration: the thread activity summary (migration 0078) and `threads.last_activity_at`
//! (0049) serve the candidate selection, so cost follows new activity, not room history.
//!
//! Message text never crosses this surface. The only text returned is a thread's display title,
//! for the thread ids the caller asked for, under the same ceiling, and never for a thread that
//! holds a `zdr_waived` run. Ids, titles and counts never reach a log line.

use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1 as pb;
use mp_contracts::model_plane::v1::space_attention_server::SpaceAttention;
use mp_contracts::space_attention::{MAX_HEADLINE_IDS, MAX_PENDING, MAX_SPACES, UNREAD_CAP};
use sqlx::PgPool;
use std::collections::HashSet;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tonic::metadata::MetadataValue;
use tonic::{Request, Response, Status};
use tracing::warn;

use crate::auth::{authorize_operation, identity};
use crate::grpc::{
    compact_thread_text, record_metrics, verify_thread_read_space_decision, THREAD_TITLE_MAX_CHARS,
};
use crate::run_phase::{ERROR_CODE_TRAILER, RUN_PHASE_SQL};

const MAX_ID_BYTES: usize = 128;
/// Spaces of one call queried at once.
const SPACES_IN_FLIGHT: usize = 8;
/// Spaces queried at once in this process; a Space that waits longer than the timeout below for
/// a permit answers ERROR for that Space.
const PROCESS_PERMITS: usize = 32;
const PERMIT_WAIT: Duration = Duration::from_secs(2);
const ATTENTION_TIMEOUT_SQL: &str = "SET LOCAL statement_timeout = '2000ms'";
/// A root run counts as working while its row moved within this window.
const WORKING_WINDOW_MINUTES: i32 = 20;
/// Threads whose latest message is probed for the room's latest activity.
const LATEST_ACTIVITY_PROBE: i64 = 25;
/// Characters of the first user message read to derive a title.
const TITLE_SOURCE_CHARS: i32 = 400;

static PERMITS: Semaphore = Semaphore::const_new(PROCESS_PERMITS);

fn coded(mut status: Status, code: &'static str) -> Status {
    status
        .metadata_mut()
        .insert(ERROR_CODE_TRAILER, MetadataValue::from_static(code));
    status
}

/// Handler state for the attention RPC.
#[derive(Clone)]
pub(crate) struct SpaceAttentionGrpc {
    pool: PgPool,
    /// `SPACE_ATTENTION_SUMMARY`, read once at boot. Off, every call is refused before a row is
    /// read. A rollout control, never an authorization input.
    enabled: bool,
}

impl SpaceAttentionGrpc {
    pub(crate) fn new(pool: PgPool, enabled: bool) -> Self {
        Self { pool, enabled }
    }
}

// ---------------------------------------------------------------------------------------------
// The unread rule (pure).
// ---------------------------------------------------------------------------------------------

/// The unread rule the request names. An unspecified rule is the stricter, newer one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rule {
    LegacyOwner,
    LastAuthor,
}

pub(crate) fn rule_from_proto(value: i32) -> Rule {
    if value == pb::UnreadRule::LegacyOwner as i32 {
        Rule::LegacyOwner
    } else {
        Rule::LastAuthor
    }
}

#[allow(dead_code)] // the rule's table test (owed) pins it; the SQL below states the same rule
/// Whether a thread is unread for `viewer`. `latest_at_ms` and `latest_author` describe the
/// thread's latest visible user or assistant message (`None` author = an agent reply);
/// `owner` is the thread's creator; `since_ms` the effective read marker (no marker, nothing
/// unread).
pub(crate) fn is_unread(
    rule: Rule,
    viewer: &str,
    owner: &str,
    latest_at_ms: i64,
    latest_author: Option<&str>,
    since_ms: i64,
) -> bool {
    if since_ms <= 0 || latest_at_ms <= since_ms {
        return false;
    }
    match rule {
        Rule::LegacyOwner => owner != viewer,
        Rule::LastAuthor => latest_author != Some(viewer),
    }
}

// ---------------------------------------------------------------------------------------------
// SQL.
// ---------------------------------------------------------------------------------------------

/// The thread-level predicates every query restates. Parameters: $1 org, $2 Space, $3 ceiling.
const THREAD_SCOPE: &str = "t.org_id = $1 AND t.space_id = $2 AND t.archived_at IS NULL
      AND (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $3)";

/// The newest user or assistant message of a thread the ceiling lets the caller see, on the
/// active path, not deleted. Parameter: $3 ceiling.
const LATEST_VISIBLE_MESSAGE: &str = "SELECT m.created_at, m.author_subject_id
        FROM messages m
       WHERE m.thread_id = t.id AND m.role IN ('user', 'assistant') AND m.on_active_path
         AND (m.recipient_audience_revision IS NULL OR m.recipient_audience_revision <= $3)
         AND NOT EXISTS (SELECT 1 FROM message_revisions r WHERE r.message_id = m.id AND r.action = 'deleted')
       ORDER BY m.sequence DESC
       LIMIT 1";

/// Unread candidates. Parameters: $1 org, $2 Space, $3 ceiling, $4 marker time, $5 viewer,
/// $6 `true` = last-author rule, `false` = legacy owner rule, $7 row cap. Threads are
/// pre-filtered by `last_activity_at` so the lateral probe runs only for threads that moved
/// after the marker.
fn unread_sql() -> String {
    format!(
        "SELECT t.id
           FROM threads t
           JOIN LATERAL ({LATEST_VISIBLE_MESSAGE}) lm ON TRUE
          WHERE {THREAD_SCOPE}
            AND t.last_activity_at > $4::TIMESTAMPTZ
            AND lm.created_at > $4::TIMESTAMPTZ
            AND CASE WHEN $6::BOOLEAN THEN lm.author_subject_id IS DISTINCT FROM $5
                     ELSE t.user_id <> $5 END
          ORDER BY t.last_activity_at DESC
          LIMIT $7"
    )
}

/// The room's latest visible activity. Parameters: $1 org, $2 Space, $3 ceiling, $4 probe size.
fn latest_activity_sql() -> String {
    format!(
        "SELECT max(lm.created_at)
           FROM (SELECT t.id
                   FROM threads t
                  WHERE {THREAD_SCOPE} AND t.last_message_seq IS NOT NULL
                  ORDER BY t.last_message_seq DESC
                  LIMIT $4) t
           JOIN LATERAL ({LATEST_VISIBLE_MESSAGE}) lm ON TRUE"
    )
}

/// Root runs of the Space that are live right now. Parameters: $1 org, $2 Space, $3 ceiling,
/// $4 window minutes. Persistent runs only: a zero-retention or waived run leaves no attention
/// trace.
fn working_sql() -> String {
    format!(
        "SELECT count(*)
           FROM runs r
           JOIN threads t ON t.id = r.thread_id AND t.org_id = r.org_id
          WHERE r.org_id = $1 AND r.space_id = $2 AND r.parent_run_id IS NULL
            AND r.retention_class = 'persistent'
            AND ({RUN_PHASE_SQL}) IN ('queued', 'running')
            AND r.updated_at > now() - make_interval(mins => $4)
            AND {THREAD_SCOPE}"
    )
}

/// Approvals addressed to the viewer in this Space, oldest first (until the addressee
/// predicate of WS10-S6 replaces the run owner). Parameters: $1 org, $2 Space, $3 ceiling,
/// $4 viewer, $5 row cap.
fn pending_sql() -> String {
    format!(
        "SELECT a.id, a.run_id, r.thread_id, a.requested_at, a.kind
           FROM approvals a
           JOIN runs r ON r.id = a.run_id
           JOIN threads t ON t.id = r.thread_id AND t.org_id = r.org_id
          WHERE a.org_id = $1 AND a.status = 'requested' AND a.user_id = $4
            AND r.space_id = $2
            AND (a.expires_at IS NULL OR a.expires_at > now())
            AND {THREAD_SCOPE}
          ORDER BY a.requested_at ASC, a.id ASC
          LIMIT $5"
    )
}

/// Display titles of the requested threads that pass the ceiling and hold no `zdr_waived` run.
/// Same derivation as the thread list: the presentation title, else the first user message,
/// else the session key. Parameters: $1 org, $2 Space, $3 ceiling, $4 thread ids,
/// $5 title source length.
fn headlines_sql() -> String {
    format!(
        "SELECT t.id, t.session_key,
                COALESCE(t.presentation_title, first_user.content) AS title
           FROM threads t
           LEFT JOIN LATERAL (
                SELECT left(m.content, $5) AS content
                  FROM messages m
                 WHERE m.thread_id = t.id AND m.role = 'user' AND m.on_active_path
                   AND (m.recipient_audience_revision IS NULL OR m.recipient_audience_revision <= $3)
                   AND NOT EXISTS (SELECT 1 FROM message_revisions r WHERE r.message_id = m.id)
                 ORDER BY m.sequence ASC
                 LIMIT 1
           ) first_user ON TRUE
          WHERE t.id = ANY($4) AND {THREAD_SCOPE}
            AND NOT EXISTS (SELECT 1 FROM runs wr WHERE wr.thread_id = t.id AND wr.retention_class = 'zdr_waived')"
    )
}

// ---------------------------------------------------------------------------------------------
// Handler.
// ---------------------------------------------------------------------------------------------

/// One Space whose decision verified: the ceiling every query below restates.
struct Reach {
    ceiling: i64,
}

fn result_with(space_id: &str, status: pb::AttentionStatus) -> pb::SpaceAttentionResult {
    pb::SpaceAttentionResult {
        space_id: space_id.to_owned(),
        status: status as i32,
        ..Default::default()
    }
}

fn millis_to_time(ms: i64) -> Option<DateTime<Utc>> {
    DateTime::<Utc>::from_timestamp_millis(ms)
}

impl SpaceAttentionGrpc {
    fn require_enabled(&self) -> Result<(), Status> {
        if self.enabled {
            Ok(())
        } else {
            Err(coded(Status::unimplemented("Space attention is disabled"), "space_attention_disabled"))
        }
    }

    /// Runs the five reads of one Space inside one read transaction under a statement timeout.
    async fn read_space(
        &self,
        org_id: &str,
        user_id: &str,
        rule: Rule,
        query: &pb::SpaceAttentionQuery,
        reach: &Reach,
    ) -> Result<pb::SpaceAttentionResult, sqlx::Error> {
        let mut result = result_with(&query.space_id, pb::AttentionStatus::Ok);
        let mut tx = self.pool.begin().await?;
        sqlx::query(ATTENTION_TIMEOUT_SQL).execute(&mut *tx).await?;

        // Unread: only when the caller has a marker for this Space.
        if let Some(since) = (query.since_ms > 0).then(|| millis_to_time(query.since_ms)).flatten() {
            let cap = i64::try_from(UNREAD_CAP).unwrap_or(100);
            let ids: Vec<(String,)> = sqlx::query_as(&unread_sql())
                .bind(org_id)
                .bind(&query.space_id)
                .bind(reach.ceiling)
                .bind(since)
                .bind(user_id)
                .bind(rule == Rule::LastAuthor)
                .bind(cap + 1)
                .fetch_all(&mut *tx)
                .await?;
            result.unread_capped = ids.len() > UNREAD_CAP;
            result.unread_threads = u32::try_from(ids.len().min(UNREAD_CAP)).unwrap_or(0);
        }

        let latest: Option<DateTime<Utc>> = sqlx::query_scalar(&latest_activity_sql())
            .bind(org_id)
            .bind(&query.space_id)
            .bind(reach.ceiling)
            .bind(LATEST_ACTIVITY_PROBE)
            .fetch_one(&mut *tx)
            .await?;
        result.latest_activity_ms = latest.map_or(0, |at| at.timestamp_millis());

        let working: i64 = sqlx::query_scalar(&working_sql())
            .bind(org_id)
            .bind(&query.space_id)
            .bind(reach.ceiling)
            .bind(WORKING_WINDOW_MINUTES)
            .fetch_one(&mut *tx)
            .await?;
        result.working_runs = u32::try_from(working.max(0)).unwrap_or(u32::MAX);

        let pending: Vec<(String, String, String, DateTime<Utc>, String)> = sqlx::query_as(&pending_sql())
            .bind(org_id)
            .bind(&query.space_id)
            .bind(reach.ceiling)
            .bind(user_id)
            .bind(i64::try_from(MAX_PENDING).unwrap_or(20))
            .fetch_all(&mut *tx)
            .await?;
        result.pending = pending
            .into_iter()
            .map(|(approval_id, run_id, thread_id, requested_at, kind)| pb::PendingApproval {
                approval_id,
                run_id,
                thread_id,
                requested_at_ms: requested_at.timestamp_millis(),
                kind,
            })
            .collect();

        if !query.headline_thread_ids.is_empty() {
            let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(&headlines_sql())
                .bind(org_id)
                .bind(&query.space_id)
                .bind(reach.ceiling)
                .bind(&query.headline_thread_ids)
                .bind(TITLE_SOURCE_CHARS)
                .fetch_all(&mut *tx)
                .await?;
            let mut rows = rows;
            // Request order, so a caller can zip the answer with what it asked for.
            rows.sort_by_key(|(id, _, _)| query.headline_thread_ids.iter().position(|asked| asked == id));
            result.headlines = rows
                .into_iter()
                .map(|(thread_id, session_key, title)| {
                    let fallback = if session_key.trim().is_empty() { "Verevon Chat" } else { session_key.as_str() };
                    pb::ThreadHeadline {
                        thread_id,
                        title: compact_thread_text(title, fallback, THREAD_TITLE_MAX_CHARS),
                    }
                })
                .collect();
        }
        tx.commit().await?;
        Ok(result)
    }

    /// One Space: the permit, then the reads. Any database failure is ERROR for this Space.
    async fn answer_space(
        &self,
        org_id: &str,
        user_id: &str,
        rule: Rule,
        query: &pb::SpaceAttentionQuery,
        reach: Reach,
    ) -> pb::SpaceAttentionResult {
        let permit = tokio::time::timeout(PERMIT_WAIT, PERMITS.acquire()).await;
        let Ok(Ok(_permit)) = permit else {
            return result_with(&query.space_id, pb::AttentionStatus::Error);
        };
        match self.read_space(org_id, user_id, rule, query, &reach).await {
            Ok(result) => result,
            Err(error) => {
                // The kind of failure only: parameters are bound, but ids stay out of the log.
                warn!(error = %error, "space attention read failed");
                result_with(&query.space_id, pb::AttentionStatus::Error)
            }
        }
    }

    async fn get(
        &self,
        request: Request<pb::GetSpaceAttentionRequest>,
    ) -> Result<Response<pb::GetSpaceAttentionResponse>, Status> {
        let caller = identity(&request)?;
        authorize_operation(&caller, "session:read")?;
        let req = request.get_ref();
        caller.authorize_org(&req.org_id)?;
        let caller_user = caller
            .user_id()
            .map(str::to_owned)
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| Status::permission_denied("Space attention requires a user principal"))?;
        if req.user_id.trim().is_empty() || req.user_id != caller_user {
            return Err(Status::permission_denied("user_id does not match the caller"));
        }
        self.require_enabled()?;

        // Bounds, before any work.
        let invalid = |what: &'static str| coded(Status::invalid_argument(what), "invalid_request");
        if req.spaces.len() > MAX_SPACES {
            return Err(invalid("at most 60 Spaces per call"));
        }
        for query in &req.spaces {
            let id = query.space_id.trim();
            if id.is_empty() || id.len() > MAX_ID_BYTES || id != query.space_id {
                return Err(invalid("space_id is invalid"));
            }
            if query.headline_thread_ids.len() > MAX_HEADLINE_IDS
                || query
                    .headline_thread_ids
                    .iter()
                    .any(|thread| thread.is_empty() || thread.len() > MAX_ID_BYTES)
            {
                return Err(invalid("headline_thread_ids is invalid"));
            }
        }
        let mut seen = HashSet::new();
        if !req.spaces.iter().all(|query| seen.insert(query.space_id.as_str())) {
            return Err(invalid("a Space may be named once per call"));
        }
        if req.spaces.is_empty() {
            return Ok(Response::new(pb::GetSpaceAttentionResponse::default()));
        }

        // A zero-retention caller: no decision is verified and no row is read.
        if caller.zdr() {
            return Ok(Response::new(pb::GetSpaceAttentionResponse {
                results: req
                    .spaces
                    .iter()
                    .map(|query| result_with(&query.space_id, pb::AttentionStatus::Zdr))
                    .collect(),
            }));
        }

        let rule = rule_from_proto(req.rule);
        let org_id = req.org_id.clone();
        let user_id = req.user_id.clone();

        // Decisions first (pure CPU, no database): a Space whose decision does not verify is
        // DENIED and never reaches a query. A missing decision is DENIED too: a cross-room read
        // has no owner-only form.
        let mut verified: Vec<Option<Reach>> = Vec::with_capacity(req.spaces.len());
        for query in &req.spaces {
            let reach = if query.decision_ref.trim().is_empty() || query.decision_token.trim().is_empty() {
                None
            } else {
                match verify_thread_read_space_decision(
                    &query.decision_token,
                    &query.decision_ref,
                    &org_id,
                    &user_id,
                    &query.space_id,
                ) {
                    Ok(Some(read)) => Some(Reach { ceiling: read.recipient_audience_revision }),
                    Ok(None) | Err(_) => None,
                }
            };
            verified.push(reach);
        }

        // One revocation read for every Space that verified.
        let candidates: Vec<String> = req
            .spaces
            .iter()
            .zip(&verified)
            .filter(|(_, reach)| reach.is_some())
            .map(|(query, _)| query.space_id.clone())
            .collect();
        let revoked: HashSet<String> = if candidates.is_empty() {
            HashSet::new()
        } else {
            let rows: Vec<(String,)> = sqlx::query_as(
                "SELECT space_ref FROM space_membership_revocations WHERE subject_id = $1 AND space_ref = ANY($2)",
            )
            .bind(&user_id)
            .bind(&candidates)
            .fetch_all(&self.pool)
            .await
            .map_err(|error| {
                warn!(error = %error, "space attention revocation read failed");
                coded(Status::unavailable("Space attention is unavailable"), "space_attention_unavailable")
            })?
            .into_iter()
            .collect();
            rows.into_iter().map(|(space,)| space).collect()
        };

        let mut work: Vec<(&pb::SpaceAttentionQuery, Option<Reach>)> = req
            .spaces
            .iter()
            .zip(verified)
            .map(|(query, reach)| (query, reach.filter(|_| !revoked.contains(&query.space_id))))
            .collect();
        // Request order is kept; at most SPACES_IN_FLIGHT Spaces are read at once.
        let mut results: Vec<pb::SpaceAttentionResult> = Vec::with_capacity(work.len());
        while !work.is_empty() {
            let batch: Vec<_> = work.drain(..work.len().min(SPACES_IN_FLIGHT)).collect();
            let answers = futures::future::join_all(batch.into_iter().map(|(query, reach)| {
                let (org_id, user_id) = (&org_id, &user_id);
                async move {
                    match reach {
                        None => result_with(&query.space_id, pb::AttentionStatus::Denied),
                        Some(reach) => self.answer_space(org_id, user_id, rule, query, reach).await,
                    }
                }
            }))
            .await;
            results.extend(answers);
        }
        Ok(Response::new(pb::GetSpaceAttentionResponse { results }))
    }
}

#[tonic::async_trait]
impl SpaceAttention for SpaceAttentionGrpc {
    async fn get_space_attention(
        &self,
        request: Request<pb::GetSpaceAttentionRequest>,
    ) -> Result<Response<pb::GetSpaceAttentionResponse>, Status> {
        let started = Instant::now();
        let result = self.get(request).await;
        record_metrics("space_get_attention", started, result.is_ok());
        result
    }
}
