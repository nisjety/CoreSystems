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

/// Approvals addressed to the viewer in this Space, oldest first. The addressee predicate of
/// WS10-S6: the requester for a requester-addressed approval, the named member for a named one.
/// An `any_editor` or `any_manager` approval is not listed: Session Core cannot see the viewer's
/// role, so those wait for the role-aware read. Parameters: $1 org, $2 Space, $3 ceiling,
/// $4 viewer, $5 row cap.
fn pending_sql() -> String {
    format!(
        "SELECT a.id, a.run_id, r.thread_id, a.requested_at, a.kind
           FROM approvals a
           JOIN runs r ON r.id = a.run_id
           JOIN threads t ON t.id = r.thread_id AND t.org_id = r.org_id
          WHERE a.org_id = $1 AND a.status = 'requested'
            AND ((a.addressee_kind = 'requester' AND a.user_id = $4)
                 OR (a.addressee_kind = 'named' AND a.addressee_subject_id = $4))
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
                    // D18: a member with a history window (a floor of either form) reads a room the
                    // attention queries do not window yet (their titles, first messages and latest
                    // authors would reach pre-floor content). Hold the attention back: no query runs.
                    Ok(Some(read))
                        if crate::space::audience_window::audience_window(Some(read))
                            .has_floor() =>
                    {
                        None
                    }
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

#[cfg(test)]
mod tests {
    //! Unit tests (no database: the pool is lazy and never connects) and, marked `--ignored`, the
    //! real-Postgres proof of the five reads. Every DB test uses its own org and Space, mints its own
    //! Control `model.thread.read` decisions with the test key, and cleans up after itself.

    use super::*;
    use crate::auth::VerifiedIdentity;
    use crate::grpc::space_w1_tests::{live_stream_token, mint, StreamKeysEnv, DECISION_REF};
    use crate::space::test_support::{clean_space, connect, seed_space_run, seed_space_thread};
    use serde_json::json;

    const ORG: &str = "org-1";
    const VIEWER: &str = "user-1";

    fn lazy_pool() -> PgPool {
        sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://unreachable.invalid/session")
            .expect("lazy pool")
    }

    fn trailer(status: &Status) -> Option<String> {
        status
            .metadata()
            .get(ERROR_CODE_TRAILER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    }

    fn squash(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Binds a decision's claims to `org`, `user`, `space` and `ceiling`, valid for two minutes from now.
    fn stamp(claims: &mut serde_json::Value, org: &str, user: &str, space: &str, ceiling: i64) {
        claims["org_id"] = json!(org);
        claims["subject_id"] = json!(user);
        claims["space_ref"] = json!(space);
        claims["recipient_audience_revision"] = json!(ceiling);
        claims["issued_at"] = json!(Utc::now().to_rfc3339());
        claims["expires_at"] = json!((Utc::now() + chrono::Duration::minutes(2)).to_rfc3339());
    }

    /// A current `model.thread.read` decision (audience `model-plane`) with `ceiling` as its audience revision.
    fn read_token(org: &str, user: &str, space: &str, ceiling: i64) -> String {
        mint(
            "model-plane",
            &["thread:read"],
            false,
            |claims| stamp(claims, org, user, space, ceiling),
            false,
        )
    }

    fn query(space: &str, token: String) -> pb::SpaceAttentionQuery {
        pb::SpaceAttentionQuery {
            space_id: space.to_owned(),
            decision_ref: DECISION_REF.to_owned(),
            decision_token: token,
            ..Default::default()
        }
    }

    fn request(
        identity: VerifiedIdentity,
        org: &str,
        user: &str,
        spaces: Vec<pb::SpaceAttentionQuery>,
    ) -> Request<pb::GetSpaceAttentionRequest> {
        let mut request = Request::new(pb::GetSpaceAttentionRequest {
            org_id: org.to_owned(),
            user_id: user.to_owned(),
            rule: pb::UnreadRule::LastAuthor as i32,
            spaces,
        });
        request.extensions_mut().insert(identity);
        request
    }

    /// The handler's answer for the viewer `user-1` of `org-1`.
    async fn ask(
        service: &SpaceAttentionGrpc,
        spaces: Vec<pb::SpaceAttentionQuery>,
    ) -> Result<pb::GetSpaceAttentionResponse, Status> {
        service
            .get_space_attention(request(VerifiedIdentity::user_for_test(ORG, VIEWER), ORG, VIEWER, spaces))
            .await
            .map(Response::into_inner)
    }

    /// `count` distinct Spaces, none with a decision.
    fn plain(count: usize) -> Vec<pb::SpaceAttentionQuery> {
        (0..count)
            .map(|index| query(&format!("space-{index}"), String::new()))
            .collect()
    }

    // ---- the unread rule (pure) ---------------------------------------------------------------

    #[test]
    fn attention_unread_rule_table() {
        const NOW: i64 = 1_800_000_000_000;
        const MARK: i64 = NOW - 60_000;
        // (rule, thread owner, latest message time, latest author, read marker, unread, why); the viewer is "viewer".
        let table: &[(Rule, &str, i64, Option<&str>, i64, bool, &str)] = &[
            (Rule::LastAuthor, "other", NOW, Some("other"), MARK, true, "another member posted after the marker"),
            (Rule::LastAuthor, "other", NOW, Some("viewer"), MARK, false, "the viewer's own post is never unread"),
            (Rule::LastAuthor, "other", NOW, None, MARK, true, "an agent reply is unread"),
            (Rule::LastAuthor, "viewer", NOW, None, MARK, true, "an agent reply is unread in the viewer's own thread"),
            (Rule::LastAuthor, "viewer", NOW, Some("other"), MARK, true, "another member's post is unread in the viewer's own thread"),
            (Rule::LastAuthor, "viewer", NOW, Some("viewer"), MARK, false, "the viewer's own post in their own thread"),
            (Rule::LastAuthor, "other", MARK, Some("other"), MARK, false, "a message at the marker is read"),
            (Rule::LastAuthor, "other", MARK - 1, Some("other"), MARK, false, "a message before the marker is read"),
            (Rule::LastAuthor, "other", MARK + 1, Some("other"), MARK, true, "one millisecond after the marker is unread"),
            (Rule::LastAuthor, "other", NOW, Some("other"), 0, false, "no marker: nothing is unread"),
            (Rule::LastAuthor, "other", NOW, Some("other"), -5, false, "a negative marker is no marker"),
            (Rule::LegacyOwner, "other", NOW, Some("other"), MARK, true, "a thread of another owner with a newer message"),
            (Rule::LegacyOwner, "viewer", NOW, Some("other"), MARK, false, "the viewer's own thread is never unread"),
            (Rule::LegacyOwner, "other", NOW, Some("viewer"), MARK, true, "the legacy rule ignores the author"),
            (Rule::LegacyOwner, "viewer", NOW, None, MARK, false, "an agent reply in the viewer's own thread"),
            (Rule::LegacyOwner, "other", NOW, None, MARK, true, "an agent reply in another owner's thread"),
            (Rule::LegacyOwner, "other", MARK, Some("other"), MARK, false, "a message at the marker is read"),
            (Rule::LegacyOwner, "other", MARK - 1, None, MARK, false, "a message before the marker is read"),
            (Rule::LegacyOwner, "other", NOW, Some("other"), 0, false, "no marker: nothing is unread"),
        ];
        for (rule, owner, latest_at, author, since, expected, why) in table {
            assert_eq!(
                is_unread(*rule, "viewer", owner, *latest_at, *author, *since),
                *expected,
                "{rule:?}: {why}"
            );
        }
    }

    #[test]
    fn attention_rule_from_proto_defaults_to_the_stricter_last_author_rule() {
        assert_eq!(rule_from_proto(pb::UnreadRule::Unspecified as i32), Rule::LastAuthor);
        assert_eq!(rule_from_proto(pb::UnreadRule::LastAuthor as i32), Rule::LastAuthor);
        assert_eq!(rule_from_proto(pb::UnreadRule::LegacyOwner as i32), Rule::LegacyOwner);
        assert_eq!(rule_from_proto(99), Rule::LastAuthor, "an unknown rule is the newer one");
    }

    #[test]
    fn the_unread_sql_states_the_rule_the_table_pins() {
        let sql = squash(&unread_sql());
        // LAST_AUTHOR: unread unless the viewer wrote the latest message (an agent reply has no author, so it is
        // distinct from the viewer). LEGACY_OWNER: unread unless the viewer owns the thread.
        assert!(
            sql.contains("CASE WHEN $6::BOOLEAN THEN lm.author_subject_id IS DISTINCT FROM $5 ELSE t.user_id <> $5 END"),
            "{sql}"
        );
        // Both the thread and its latest visible message must be newer than the marker.
        assert!(sql.contains("t.last_activity_at > $4::TIMESTAMPTZ"), "{sql}");
        assert!(sql.contains("lm.created_at > $4::TIMESTAMPTZ"), "{sql}");
    }

    #[test]
    fn pending_sql_lists_requester_addressed_to_the_requester_and_named_to_the_named_member_only() {
        let sql = squash(&pending_sql());
        assert!(
            sql.contains(
                "((a.addressee_kind = 'requester' AND a.user_id = $4) \
                 OR (a.addressee_kind = 'named' AND a.addressee_subject_id = $4))"
            ),
            "{sql}"
        );
        // A role-addressed approval needs the viewer's role, which Session Core cannot see.
        assert!(!sql.contains("any_editor") && !sql.contains("any_manager"), "{sql}");
        // Only a still-open, unexpired approval of this org, in this Space, under the ceiling.
        for required in [
            "a.org_id = $1",
            "a.status = 'requested'",
            "(a.expires_at IS NULL OR a.expires_at > now())",
            "r.space_id = $2",
            "t.recipient_audience_revision <= $3",
            "ORDER BY a.requested_at ASC, a.id ASC",
        ] {
            assert!(sql.contains(required), "{required}: {sql}");
        }
    }

    // ---- the handler's gates, answered before any row is read ---------------------------------

    #[tokio::test]
    async fn attention_gate_denies_wrong_subject_space_or_ceiling() {
        let _keys = StreamKeysEnv::publish();
        // The fixture is sound: the unaltered decision verifies, with the ceiling it signed.
        let sound = verify_thread_read_space_decision(
            &read_token(ORG, VIEWER, "space-ok", 4),
            DECISION_REF,
            ORG,
            VIEWER,
            "space-ok",
        )
        .expect("a current decision verifies")
        .expect("a decision was offered");
        assert_eq!(sound.recipient_audience_revision, 4);

        // Each Space below differs from that sound decision in exactly one respect.
        let mut cases: Vec<(&str, pb::SpaceAttentionQuery)> = Vec::new();
        let mut add = |why: &'static str, query: pb::SpaceAttentionQuery| cases.push((why, query));
        add("another subject", query("space-0", read_token(ORG, "user-2", "space-0", 4)));
        add("another Space", query("space-1", read_token(ORG, VIEWER, "space-elsewhere", 4)));
        add("another org", query("space-2", read_token("org-2", VIEWER, "space-2", 4)));
        add(
            "the stream audience",
            query("space-3", live_stream_token(ORG, VIEWER, "space-3", 4, false, |_| {})),
        );
        add(
            "a write permission beside the read",
            query(
                "space-4",
                mint(
                    "model-plane",
                    &["thread:read", "thread:append"],
                    false,
                    |claims| stamp(claims, ORG, VIEWER, "space-4", 4),
                    false,
                ),
            ),
        );
        add(
            "an expired decision",
            query(
                "space-5",
                mint(
                    "model-plane",
                    &["thread:read"],
                    false,
                    |claims| {
                        stamp(claims, ORG, VIEWER, "space-5", 4);
                        claims["issued_at"] = json!((Utc::now() - chrono::Duration::minutes(10)).to_rfc3339());
                        claims["expires_at"] = json!((Utc::now() - chrono::Duration::minutes(5)).to_rfc3339());
                    },
                    false,
                ),
            ),
        );
        add("a zero ceiling", query("space-6", read_token(ORG, VIEWER, "space-6", 0)));
        add(
            "a forged ceiling (tampered digest)",
            query(
                "space-7",
                mint(
                    "model-plane",
                    &["thread:read"],
                    false,
                    |claims| stamp(claims, ORG, VIEWER, "space-7", 9),
                    true,
                ),
            ),
        );
        add(
            "another decision ref",
            pb::SpaceAttentionQuery {
                decision_ref: "decision-other".to_owned(),
                ..query("space-8", read_token(ORG, VIEWER, "space-8", 4))
            },
        );
        add(
            "a token without its ref",
            pb::SpaceAttentionQuery {
                decision_ref: String::new(),
                ..query("space-9", read_token(ORG, VIEWER, "space-9", 4))
            },
        );
        add("a ref without its token", query("space-10", String::new()));
        add("an unreadable token", query("space-11", "not-a-token".to_owned()));

        let service = SpaceAttentionGrpc::new(lazy_pool(), true);
        let spaces: Vec<pb::SpaceAttentionQuery> = cases.iter().map(|(_, query)| query.clone()).collect();
        let response = ask(&service, spaces)
            .await
            .expect("a denied Space is an answer, never an error for the call");
        assert_eq!(response.results.len(), cases.len());
        for ((why, query), result) in cases.iter().zip(&response.results) {
            assert_eq!(result.space_id, query.space_id, "request order is kept: {why}");
            assert_eq!(result.status, pb::AttentionStatus::Denied as i32, "{why}");
            assert_eq!(
                (result.unread_threads, result.working_runs, result.latest_activity_ms),
                (0, 0, 0),
                "{why}"
            );
            assert!(result.pending.is_empty() && result.headlines.is_empty(), "{why}");
        }
        assert_eq!(service.pool.size(), 0, "a Space that was denied never reached a query");
    }

    #[tokio::test]
    async fn zdr_caller_gets_zdr_status_without_queries() {
        let _keys = StreamKeysEnv::publish();
        let service = SpaceAttentionGrpc::new(lazy_pool(), true);
        let spaces = vec![
            query("space-a", read_token(ORG, VIEWER, "space-a", 4)),
            query("space-b", String::new()),
            query("space-c", "not-a-token".to_owned()),
        ];
        let identity = VerifiedIdentity::user_for_test_with_zdr(ORG, VIEWER, true);
        let response = service
            .get_space_attention(request(identity, ORG, VIEWER, spaces))
            .await
            .expect("a zero-retention caller is answered")
            .into_inner();
        let answered: Vec<(&str, i32)> = response
            .results
            .iter()
            .map(|result| (result.space_id.as_str(), result.status))
            .collect();
        let zdr = pb::AttentionStatus::Zdr as i32;
        assert_eq!(answered, [("space-a", zdr), ("space-b", zdr), ("space-c", zdr)]);
        for result in &response.results {
            assert_eq!(
                (result.unread_threads, result.working_runs, result.latest_activity_ms),
                (0, 0, 0)
            );
            assert!(result.pending.is_empty() && result.headlines.is_empty());
        }
        // The valid decision of space-a was not even verified (the other two would have been DENIED), and
        // no row was read.
        assert_eq!(service.pool.size(), 0, "no query ran for a zero-retention caller");
    }

    #[tokio::test]
    async fn attention_flag_off_refuses_before_any_row_is_read() {
        let service = SpaceAttentionGrpc::new(lazy_pool(), false);
        let status = ask(&service, vec![query("space-a", String::new())])
            .await
            .expect_err("the switch is off");
        assert_eq!(status.code(), tonic::Code::Unimplemented);
        assert_eq!(trailer(&status).as_deref(), Some("space_attention_disabled"));
        // An empty request is refused too: the switch is read before the request is looked at.
        let status = ask(&service, Vec::new()).await.expect_err("the switch is off");
        assert_eq!(status.code(), tonic::Code::Unimplemented);
        // The caller checks come first: someone who is not the named user learns nothing about the switch.
        let status = service
            .get_space_attention(request(VerifiedIdentity::user_for_test(ORG, "user-2"), ORG, VIEWER, Vec::new()))
            .await
            .expect_err("a caller that is not the user");
        assert_eq!(status.code(), tonic::Code::PermissionDenied);
        assert_eq!(service.pool.size(), 0);
    }

    #[tokio::test]
    async fn attention_refuses_a_service_identity_and_another_org() {
        let service = SpaceAttentionGrpc::new(lazy_pool(), true);
        let service_identity =
            VerifiedIdentity::service_for_test_as(ORG, "service:model-gateway", &["session:read"], false);
        let status = service
            .get_space_attention(request(service_identity, ORG, VIEWER, plain(1)))
            .await
            .expect_err("a service identity has no user id");
        assert_eq!(status.code(), tonic::Code::PermissionDenied);
        let status = service
            .get_space_attention(request(VerifiedIdentity::user_for_test("org-2", VIEWER), ORG, VIEWER, plain(1)))
            .await
            .expect_err("another tenant");
        assert_eq!(status.code(), tonic::Code::PermissionDenied);
        assert_eq!(service.pool.size(), 0);
    }

    #[tokio::test]
    async fn attention_rejects_more_than_60_spaces_and_a_duplicate_space_id() {
        let service = SpaceAttentionGrpc::new(lazy_pool(), true);
        let invalid = |status: Status, why: &str| {
            assert_eq!(status.code(), tonic::Code::InvalidArgument, "{why}");
            assert_eq!(trailer(&status).as_deref(), Some("invalid_request"), "{why}");
        };

        // 60 is the most one call may ask; 61 is refused before anything runs.
        invalid(ask(&service, plain(MAX_SPACES + 1)).await.expect_err("over 60"), "61 Spaces");
        let sixty = ask(&service, plain(MAX_SPACES)).await.expect("exactly 60 are served");
        assert_eq!(sixty.results.len(), MAX_SPACES);
        for (index, result) in sixty.results.iter().enumerate() {
            assert_eq!(result.space_id, format!("space-{index}"), "request order is kept");
            assert_eq!(result.status, pb::AttentionStatus::Denied as i32, "no decision, no read");
        }

        // A Space may be named once per call.
        invalid(
            ask(&service, vec![query("space-a", String::new()), query("space-b", String::new()), query("space-a", String::new())])
                .await
                .expect_err("a duplicate"),
            "a Space named twice",
        );

        // The ids are bounded and carry no surrounding space.
        let too_long = "s".repeat(129);
        for bad in ["", "   ", " space-a", "space-a ", too_long.as_str()] {
            invalid(
                ask(&service, vec![query(bad, String::new())]).await.expect_err("a bad Space id"),
                "a bad Space id",
            );
        }
        assert!(ask(&service, vec![query(&"s".repeat(128), String::new())]).await.is_ok());

        // So are the headline ids: at most 20, none empty, none over 128 bytes.
        let with_headlines = |ids: Vec<String>| {
            vec![pb::SpaceAttentionQuery {
                headline_thread_ids: ids,
                ..query("space-a", String::new())
            }]
        };
        let ids = |count: usize| (0..count).map(|index| format!("thread-{index}")).collect::<Vec<_>>();
        invalid(
            ask(&service, with_headlines(ids(MAX_HEADLINE_IDS + 1))).await.expect_err("21 ids"),
            "21 headline ids",
        );
        assert!(ask(&service, with_headlines(ids(MAX_HEADLINE_IDS))).await.is_ok());
        invalid(
            ask(&service, with_headlines(vec![String::new()])).await.expect_err("an empty id"),
            "an empty headline id",
        );
        invalid(
            ask(&service, with_headlines(vec!["t".repeat(129)])).await.expect_err("a long id"),
            "a headline id over 128 bytes",
        );

        // An empty request is an empty answer.
        let empty = ask(&service, Vec::new()).await.expect("nothing asked");
        assert!(empty.results.is_empty());
        assert_eq!(service.pool.size(), 0, "none of these reached a query");
    }

    // ---- against a disposable database (`--ignored`) ------------------------------------------

    /// The ceiling every DB test signs unless it says otherwise.
    const CEILING: i64 = 3;

    fn ids(prefix: &str) -> (String, String) {
        let suffix = mp_ids::new_ulid();
        (format!("org-{prefix}-{suffix}"), format!("space-{prefix}-{suffix}"))
    }

    /// "Now" to the whole second, so every time derived from it is exact in milliseconds.
    fn now_ms() -> i64 {
        Utc::now().timestamp() * 1000
    }

    fn ago(now_ms: i64, seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(now_ms - seconds * 1000).expect("a valid time")
    }

    /// One message to seed. `author` is `None` for an agent reply.
    struct Post<'a> {
        thread: &'a str,
        id: &'a str,
        role: &'a str,
        author: Option<&'a str>,
        revision: i64,
        at: DateTime<Utc>,
        content: &'a str,
    }

    fn say<'a>(
        thread: &'a str,
        id: &'a str,
        role: &'a str,
        author: Option<&'a str>,
        revision: i64,
        at: DateTime<Utc>,
    ) -> Post<'a> {
        Post {
            thread,
            id,
            role,
            author,
            revision,
            at,
            content: "canary body",
        }
    }

    async fn post(pool: &PgPool, space: &str, post: Post<'_>) {
        sqlx::query(
            "INSERT INTO messages (id, thread_id, role, content, created_at, space_id, recipient_audience_ref, \
             recipient_audience_revision, recipient_audience_hash, authority_revision, \
             resource_authorization_ref, author_subject_id) \
             VALUES ($1, $2, $3, $4, $5, $6, 'audience-1', $7, 'sha256:audience-1', 1, 'resource-1', $8)",
        )
        .bind(post.id)
        .bind(post.thread)
        .bind(post.role)
        .bind(post.content)
        .bind(post.at)
        .bind(space)
        .bind(post.revision)
        .bind(post.author)
        .execute(pool)
        .await
        .expect("seed message");
    }

    /// A Space thread owned by `owner`, stamped with audience `revision` and last active at `active`.
    async fn thread_at(
        pool: &PgPool,
        org: &str,
        space: &str,
        thread: &str,
        owner: &str,
        revision: i64,
        active: DateTime<Utc>,
    ) {
        seed_space_thread(pool, org, space, thread, owner).await;
        sqlx::query("UPDATE threads SET recipient_audience_revision = $2, last_activity_at = $3 WHERE id = $1")
            .bind(thread)
            .bind(revision)
            .bind(active)
            .execute(pool)
            .await
            .expect("place the thread");
    }

    /// One call as `viewer` with the named unread rule.
    async fn attend(
        pool: &PgPool,
        org: &str,
        viewer: &str,
        rule: pb::UnreadRule,
        spaces: Vec<pb::SpaceAttentionQuery>,
    ) -> Vec<pb::SpaceAttentionResult> {
        let service = SpaceAttentionGrpc::new(pool.clone(), true);
        let mut request = request(VerifiedIdentity::user_for_test(org, viewer), org, viewer, spaces);
        request.get_mut().rule = rule as i32;
        service
            .get_space_attention(request)
            .await
            .expect("attention answers")
            .into_inner()
            .results
    }

    /// The query of one Space for `viewer`: a decision at `ceiling` and a read marker at `since_ms`.
    fn ask_for(org: &str, viewer: &str, space: &str, ceiling: i64, since_ms: i64) -> pb::SpaceAttentionQuery {
        pb::SpaceAttentionQuery {
            since_ms,
            ..query(space, read_token(org, viewer, space, ceiling))
        }
    }

    async fn clean(pool: &PgPool, org: &str, spaces: &[&str]) {
        sqlx::query("DELETE FROM approvals WHERE org_id = $1")
            .bind(org)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM space_membership_revocations WHERE org_id = $1")
            .bind(org)
            .execute(pool)
            .await
            .ok();
        for space in spaces {
            clean_space(pool, org, space).await;
        }
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn unread_counts_respect_audience_ceiling() {
        let Some(pool) = connect().await else { return };
        let _keys = StreamKeysEnv::publish();
        let (org, space) = ids("unread");
        let now = now_ms();
        let marker = ago(now, 3600).timestamp_millis();
        let thread = |n: u8| format!("thread-{n}-{space}");
        let id = |name: &str| format!("{name}-{space}");
        let other = "member-2";

        // T1: the viewer's own thread; another member posted last, after the marker.
        thread_at(&pool, &org, &space, &thread(1), VIEWER, 1, ago(now, 600)).await;
        post(&pool, &space, say(&thread(1), &id("m1a"), "user", Some(VIEWER), 1, ago(now, 7200))).await;
        post(&pool, &space, say(&thread(1), &id("m1b"), "user", Some(other), 1, ago(now, 600))).await;
        // T2: another member's thread; the viewer posted last.
        thread_at(&pool, &org, &space, &thread(2), other, 1, ago(now, 300)).await;
        post(&pool, &space, say(&thread(2), &id("m2a"), "user", Some(other), 1, ago(now, 7200))).await;
        post(&pool, &space, say(&thread(2), &id("m2b"), "user", Some(VIEWER), 1, ago(now, 300))).await;
        // T3: another member's thread; an agent replied last.
        thread_at(&pool, &org, &space, &thread(3), other, 1, ago(now, 180)).await;
        post(&pool, &space, say(&thread(3), &id("m3a"), "user", Some(other), 1, ago(now, 7200))).await;
        post(&pool, &space, say(&thread(3), &id("m3b"), "assistant", None, 1, ago(now, 180))).await;
        // T4: the viewer's own thread; an agent replied last.
        thread_at(&pool, &org, &space, &thread(4), VIEWER, 1, ago(now, 120)).await;
        post(&pool, &space, say(&thread(4), &id("m4a"), "user", Some(VIEWER), 1, ago(now, 7200))).await;
        post(&pool, &space, say(&thread(4), &id("m4b"), "assistant", None, 1, ago(now, 120))).await;
        // T5: quiet since before the marker.
        thread_at(&pool, &org, &space, &thread(5), other, 1, ago(now, 7200)).await;
        post(&pool, &space, say(&thread(5), &id("m5a"), "user", Some(other), 1, ago(now, 7200))).await;
        // T6: inside the ceiling, but its newest message is stamped with revision 5, above it. The newest
        // message the decision may see is the old one, so the thread is not unread at ceiling 3.
        thread_at(&pool, &org, &space, &thread(6), other, 1, ago(now, 60)).await;
        post(&pool, &space, say(&thread(6), &id("m6a"), "user", Some(other), 1, ago(now, 7200))).await;
        post(&pool, &space, say(&thread(6), &id("m6b"), "user", Some(other), 5, ago(now, 60))).await;
        // T7: a thread of audience revision 5, above the ceiling.
        thread_at(&pool, &org, &space, &thread(7), other, 5, ago(now, 60)).await;
        post(&pool, &space, say(&thread(7), &id("m7a"), "user", Some(other), 5, ago(now, 60))).await;
        // T8: archived, and the newest thing in the room.
        thread_at(&pool, &org, &space, &thread(8), other, 1, ago(now, 30)).await;
        post(&pool, &space, say(&thread(8), &id("m8a"), "user", Some(other), 1, ago(now, 30))).await;
        sqlx::query("UPDATE threads SET archived_at = now() WHERE id = $1")
            .bind(thread(8))
            .execute(&pool)
            .await
            .expect("archive T8");

        // (ceiling, unread under LAST_AUTHOR, unread under LEGACY_OWNER, newest visible message in seconds ago).
        // Ceiling 3: LAST_AUTHOR = T1, T3, T4; LEGACY_OWNER = T2, T3. Ceiling 5 also admits T6's newest message
        // and T7, and nothing admits the archived T8.
        for (ceiling, last_author, legacy, latest_secs) in [(CEILING, 3_u32, 2_u32, 120), (5, 5, 4, 60)] {
            for (rule, expected) in [
                (pb::UnreadRule::LastAuthor, last_author),
                (pb::UnreadRule::Unspecified, last_author),
                (pb::UnreadRule::LegacyOwner, legacy),
            ] {
                let results =
                    attend(&pool, &org, VIEWER, rule, vec![ask_for(&org, VIEWER, &space, ceiling, marker)]).await;
                assert_eq!(results.len(), 1);
                let result = &results[0];
                assert_eq!(result.status, pb::AttentionStatus::Ok as i32, "ceiling {ceiling}, {rule:?}");
                assert_eq!(result.unread_threads, expected, "ceiling {ceiling}, {rule:?}");
                assert!(!result.unread_capped);
                assert_eq!(
                    result.latest_activity_ms,
                    ago(now, latest_secs).timestamp_millis(),
                    "the newest message under ceiling {ceiling}"
                );
                assert_eq!(result.working_runs, 0);
                assert!(result.pending.is_empty() && result.headlines.is_empty());
            }
        }

        // No marker: nothing is unread, but the room's latest activity is still reported.
        let results = attend(
            &pool,
            &org,
            VIEWER,
            pb::UnreadRule::LastAuthor,
            vec![ask_for(&org, VIEWER, &space, CEILING, 0)],
        )
        .await;
        assert_eq!(results[0].unread_threads, 0);
        assert_eq!(results[0].latest_activity_ms, ago(now, 120).timestamp_millis());
        clean(&pool, &org, &[space.as_str()]).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn unread_query_uses_the_thread_activity_plan() {
        let Some(pool) = connect().await else { return };
        let mut tx = pool.begin().await.expect("begin");
        // On a near-empty test table the planner may take a sequential scan whatever indexes exist; with it
        // discouraged the plan shows which access paths the query can use.
        sqlx::query("SET LOCAL enable_seqscan = off")
            .execute(&mut *tx)
            .await
            .expect("planner setting");
        let plan: Vec<String> = sqlx::query_scalar(&format!("EXPLAIN {}", unread_sql()))
            .bind("org-plan")
            .bind("space-plan")
            .bind(CEILING)
            .bind(Utc::now() - chrono::Duration::hours(1))
            .bind(VIEWER)
            .bind(true)
            .bind(101_i64)
            .fetch_all(&mut *tx)
            .await
            .expect("explain");
        let plan = plan.join("\n");
        // The plan WS11-S2 planned (an index of the messages of a Space by time) was never created: the
        // candidate threads come from `threads.last_activity_at`, and only they are probed for their latest message.
        assert!(!plan.contains("idx_messages_space_created"), "{plan}");
        assert!(
            plan.contains("last_activity_at"),
            "the candidate threads are selected by their activity time: {plan}"
        );
        for line in plan.lines().filter(|line| line.contains(" on messages")) {
            assert!(!line.contains("Seq Scan"), "the latest message of a thread is read by index: {line}");
        }
        tx.rollback().await.ok();
    }

    /// An approval to seed. A requester-addressed, requested, unexpiring one unless the test overrides it.
    struct Approval<'a> {
        id: &'a str,
        run: &'a str,
        requester: &'a str,
        addressee_kind: &'a str,
        addressee: Option<&'a str>,
        status: &'a str,
        requested_at: DateTime<Utc>,
        expires_at: Option<DateTime<Utc>>,
    }

    fn requested<'a>(id: &'a str, run: &'a str, requester: &'a str, requested_at: DateTime<Utc>) -> Approval<'a> {
        Approval {
            id,
            run,
            requester,
            addressee_kind: "requester",
            addressee: None,
            status: "requested",
            requested_at,
            expires_at: None,
        }
    }

    async fn approval(pool: &PgPool, org: &str, approval: Approval<'_>) {
        sqlx::query(
            "INSERT INTO approvals (id, run_id, kind, status, org_id, user_id, requested_at, expires_at, \
             addressee_kind, addressee_subject_id) \
             VALUES ($1, $2, 'tool_call', $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(approval.id)
        .bind(approval.run)
        .bind(approval.status)
        .bind(org)
        .bind(approval.requester)
        .bind(approval.requested_at)
        .bind(approval.expires_at)
        .bind(approval.addressee_kind)
        .bind(approval.addressee)
        .execute(pool)
        .await
        .expect("seed approval");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn pending_approvals_are_only_the_viewers() {
        let Some(pool) = connect().await else { return };
        let _keys = StreamKeysEnv::publish();
        let (org, space) = ids("pending");
        let elsewhere = format!("{space}-other");
        let now = now_ms();
        let id = |name: &str| format!("{name}-{space}");
        let (thread, over, away) = (id("thread"), id("thread-over"), id("thread-away"));
        let (run, run_over, run_away) = (id("run"), id("run-over"), id("run-away"));
        thread_at(&pool, &org, &space, &thread, "owner-1", 1, ago(now, 10)).await;
        thread_at(&pool, &org, &space, &over, "owner-1", 5, ago(now, 10)).await;
        thread_at(&pool, &org, &elsewhere, &away, "owner-1", 1, ago(now, 10)).await;
        seed_space_run(&pool, &org, &space, &thread, &run, "queued", "persistent").await;
        seed_space_run(&pool, &org, &space, &over, &run_over, "queued", "persistent").await;
        seed_space_run(&pool, &org, &elsewhere, &away, &run_away, "queued", "persistent").await;

        let (a_mine, a_other, a_named_me, a_named_v2) =
            (id("a-mine"), id("a-other"), id("a-named-me"), id("a-named-v2"));
        let (a_editor, a_manager, a_granted, a_expired, a_live) =
            (id("a-editor"), id("a-manager"), id("a-granted"), id("a-expired"), id("a-live"));
        let (a_over, a_away) = (id("a-over"), id("a-away"));
        // Addressed to the viewer: the requester's own, a named one, and a live one with an expiry ahead.
        approval(&pool, &org, requested(&a_mine, &run, VIEWER, ago(now, 500))).await;
        approval(
            &pool,
            &org,
            Approval {
                addressee_kind: "named",
                addressee: Some(VIEWER),
                ..requested(&a_named_me, &run, "owner-1", ago(now, 400))
            },
        )
        .await;
        approval(
            &pool,
            &org,
            Approval {
                expires_at: Some(Utc::now() + chrono::Duration::hours(1)),
                ..requested(&a_live, &run, VIEWER, ago(now, 300))
            },
        )
        .await;
        // Not the viewer's: another requester's, one named for another member, role-addressed ones (even
        // with the viewer as requester), a decided one, an expired one, one above the ceiling.
        approval(&pool, &org, requested(&a_other, &run, "member-2", ago(now, 450))).await;
        approval(
            &pool,
            &org,
            Approval {
                addressee_kind: "named",
                addressee: Some("viewer-2"),
                ..requested(&a_named_v2, &run, "owner-1", ago(now, 350))
            },
        )
        .await;
        for (approval_id, kind) in [(&a_editor, "any_editor"), (&a_manager, "any_manager")] {
            approval(
                &pool,
                &org,
                Approval {
                    addressee_kind: kind,
                    ..requested(approval_id, &run, VIEWER, ago(now, 340))
                },
            )
            .await;
        }
        approval(
            &pool,
            &org,
            Approval {
                status: "granted",
                ..requested(&a_granted, &run, VIEWER, ago(now, 320))
            },
        )
        .await;
        approval(
            &pool,
            &org,
            Approval {
                expires_at: Some(ago(now, 10)),
                ..requested(&a_expired, &run, VIEWER, ago(now, 310))
            },
        )
        .await;
        approval(&pool, &org, requested(&a_over, &run_over, VIEWER, ago(now, 290))).await;
        approval(&pool, &org, requested(&a_away, &run_away, VIEWER, ago(now, 280))).await;

        let pending_of = |viewer: &'static str, spaces: Vec<(&str, i64)>| {
            let pool = pool.clone();
            let org = org.clone();
            let queries: Vec<pb::SpaceAttentionQuery> = spaces
                .into_iter()
                .map(|(space, ceiling)| ask_for(&org, viewer, space, ceiling, 0))
                .collect();
            async move {
                attend(&pool, &org, viewer, pb::UnreadRule::LastAuthor, queries)
                    .await
                    .into_iter()
                    .map(|result| {
                        result
                            .pending
                            .into_iter()
                            .map(|approval| approval.approval_id)
                            .collect::<Vec<String>>()
                    })
                    .collect::<Vec<_>>()
            }
        };
        // Oldest first, only what is addressed to the viewer, only inside the Space and under the ceiling.
        assert_eq!(
            pending_of(VIEWER, vec![(space.as_str(), CEILING), (elsewhere.as_str(), CEILING)]).await,
            [vec![a_mine.clone(), a_named_me.clone(), a_live.clone()], vec![a_away.clone()]]
        );
        // The ceiling is what hides the approval of the newer thread.
        assert_eq!(
            pending_of(VIEWER, vec![(space.as_str(), 5)]).await,
            [vec![a_mine.clone(), a_named_me.clone(), a_live.clone(), a_over.clone()]]
        );
        assert_eq!(
            pending_of("viewer-2", vec![(space.as_str(), CEILING)]).await,
            [vec![a_named_v2.clone()]]
        );
        assert_eq!(
            pending_of("member-2", vec![(space.as_str(), CEILING)]).await,
            [vec![a_other.clone()]]
        );
        assert_eq!(
            pending_of("viewer-3", vec![(space.as_str(), CEILING)]).await,
            [Vec::<String>::new()]
        );

        // The card carries ids, the kind and the time: nothing else.
        let results = attend(
            &pool,
            &org,
            VIEWER,
            pb::UnreadRule::LastAuthor,
            vec![ask_for(&org, VIEWER, &space, CEILING, 0)],
        )
        .await;
        assert_eq!(
            results[0].pending[0],
            pb::PendingApproval {
                approval_id: a_mine.clone(),
                run_id: run.clone(),
                thread_id: thread.clone(),
                requested_at_ms: ago(now, 500).timestamp_millis(),
                kind: "tool_call".to_owned(),
            }
        );
        clean(&pool, &org, &[space.as_str(), elsewhere.as_str()]).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn working_runs_ignore_stale_rows() {
        let Some(pool) = connect().await else { return };
        let _keys = StreamKeysEnv::publish();
        let (org, space) = ids("working");
        let elsewhere = format!("{space}-other");
        let now = now_ms();
        let id = |name: &str| format!("{name}-{space}");
        let (thread, over, away) = (id("thread"), id("thread-over"), id("thread-away"));
        thread_at(&pool, &org, &space, &thread, "owner-1", 1, ago(now, 10)).await;
        thread_at(&pool, &org, &space, &over, "owner-1", 5, ago(now, 10)).await;
        thread_at(&pool, &org, &elsewhere, &away, "owner-1", 1, ago(now, 10)).await;

        let (live, stale, done, waived, child) = (id("live"), id("stale"), id("done"), id("waived"), id("child"));
        let (waiting, running, above, away_run) = (id("waiting"), id("running"), id("above"), id("away-run"));
        seed_space_run(&pool, &org, &space, &thread, &live, "queued", "persistent").await;
        seed_space_run(&pool, &org, &space, &thread, &stale, "queued", "persistent").await;
        seed_space_run(&pool, &org, &space, &thread, &done, "completed", "persistent").await;
        seed_space_run(&pool, &org, &space, &thread, &waived, "queued", "zdr_waived").await;
        seed_space_run(&pool, &org, &space, &thread, &child, "queued", "persistent").await;
        seed_space_run(&pool, &org, &space, &thread, &waiting, "awaiting_approval", "persistent").await;
        seed_space_run(&pool, &org, &space, &thread, &running, "running", "persistent").await;
        seed_space_run(&pool, &org, &space, &over, &above, "queued", "persistent").await;
        seed_space_run(&pool, &org, &elsewhere, &away, &away_run, "queued", "persistent").await;
        // A run that moved half an hour ago is a leftover, not work in progress. A sub-run is not a root run.
        sqlx::query("UPDATE runs SET updated_at = now() - interval '30 minutes' WHERE id = $1")
            .bind(&stale)
            .execute(&pool)
            .await
            .expect("age the run");
        sqlx::query("UPDATE runs SET parent_run_id = $2 WHERE id = $1")
            .bind(&child)
            .bind(&live)
            .execute(&pool)
            .await
            .expect("make a sub-run");

        let working = |ceiling: i64| {
            let (pool, org, space) = (pool.clone(), org.clone(), space.clone());
            async move {
                let results = attend(
                    &pool,
                    &org,
                    VIEWER,
                    pb::UnreadRule::LastAuthor,
                    vec![ask_for(&org, VIEWER, &space, ceiling, 0)],
                )
                .await;
                assert_eq!(results[0].status, pb::AttentionStatus::Ok as i32);
                results[0].working_runs
            }
        };
        // Only the live root runs of this Space under the ceiling, persistent ones only: `live` and `running`.
        // Not the stale, finished, waived, waiting, sub-, above-ceiling or other-room runs.
        assert_eq!(working(CEILING).await, 2);
        // Once the stale run moves again it counts, and the ceiling is what held the newer thread's run back.
        sqlx::query("UPDATE runs SET updated_at = now() WHERE id = $1")
            .bind(&stale)
            .execute(&pool)
            .await
            .expect("touch the run");
        assert_eq!(working(CEILING).await, 3);
        assert_eq!(working(5).await, 4);
        clean(&pool, &org, &[space.as_str(), elsewhere.as_str()]).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn headlines_only_for_ceiling_passing_threads() {
        let Some(pool) = connect().await else { return };
        let _keys = StreamKeysEnv::publish();
        let (org, space) = ids("headline");
        let elsewhere = format!("{space}-other");
        let now = now_ms();
        let id = |name: &str| format!("{name}-{space}");
        let titled = |thread: String, title: &'static str| {
            let pool = pool.clone();
            async move {
                sqlx::query("UPDATE threads SET presentation_title = $2 WHERE id = $1")
                    .bind(thread)
                    .bind(title)
                    .execute(&pool)
                    .await
                    .expect("title the thread");
            }
        };
        let (named, derived, plain_key) = (id("named"), id("derived"), id("plain-key"));
        let (above, waived, away, archived) = (id("above"), id("waived"), id("away"), id("archived"));
        for (thread, space, revision) in [
            (&named, &space, 1),
            (&derived, &space, 1),
            (&plain_key, &space, 1),
            (&above, &space, 5),
            (&waived, &space, 1),
            (&archived, &space, 1),
            (&away, &elsewhere, 1),
        ] {
            thread_at(&pool, &org, space, thread, "owner-1", revision, ago(now, 10)).await;
        }
        // Titles by derivation: the presentation title, else the first user message, else the session key.
        titled(named.clone(), "Roadmap review").await;
        titled(above.clone(), "Above the ceiling").await;
        titled(waived.clone(), "Held under a waiver").await;
        titled(away.clone(), "In another room").await;
        titled(archived.clone(), "Archived thread").await;
        post(
            &pool,
            &space,
            Post {
                content: "Plan   the\nlaunch",
                ..say(&derived, &id("m-derived"), "user", Some("member-2"), 1, ago(now, 100))
            },
        )
        .await;
        // The only user message of this thread is above the ceiling, so it cannot name the thread.
        post(
            &pool,
            &space,
            Post {
                content: "Secret words above the ceiling",
                ..say(&plain_key, &id("m-plain"), "user", Some("member-2"), 5, ago(now, 100))
            },
        )
        .await;
        seed_space_run(&pool, &org, &space, &waived, &id("run-waived"), "completed", "zdr_waived").await;
        sqlx::query("UPDATE threads SET archived_at = now() WHERE id = $1")
            .bind(&archived)
            .execute(&pool)
            .await
            .expect("archive");

        let asked = vec![
            plain_key.clone(),
            derived.clone(),
            named.clone(),
            above.clone(),
            waived.clone(),
            away.clone(),
            archived.clone(),
            id("never-existed"),
        ];
        let query = pb::SpaceAttentionQuery {
            headline_thread_ids: asked,
            ..ask_for(&org, VIEWER, &space, CEILING, 0)
        };
        let results = attend(&pool, &org, VIEWER, pb::UnreadRule::LastAuthor, vec![query]).await;
        let headlines: Vec<(String, String)> = results[0]
            .headlines
            .iter()
            .map(|headline| (headline.thread_id.clone(), headline.title.clone()))
            .collect();
        // Request order, only threads of this Space under the ceiling, archived or waived ones omitted.
        assert_eq!(
            headlines,
            [
                (plain_key.clone(), plain_key.clone()),
                (derived.clone(), "Plan the launch".to_owned()),
                (named.clone(), "Roadmap review".to_owned()),
            ]
        );
        let text = format!("{:?}", results[0]);
        for leaked in ["Above the ceiling", "Held under a waiver", "In another room", "Archived thread", "Secret words"] {
            assert!(!text.contains(leaked), "{leaked} must not cross: {text}");
        }
        clean(&pool, &org, &[space.as_str(), elsewhere.as_str()]).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn revoked_member_is_denied_per_space() {
        let Some(pool) = connect().await else { return };
        let _keys = StreamKeysEnv::publish();
        let (org, space) = ids("revoked");
        let kept = format!("{space}-kept");
        let now = now_ms();
        for (space, thread) in [(&space, format!("thread-a-{space}")), (&kept, format!("thread-b-{space}"))] {
            thread_at(&pool, &org, space, &thread, "owner-1", 1, ago(now, 10)).await;
        }
        sqlx::query(
            "INSERT INTO space_membership_revocations (space_ref, org_id, subject_id) VALUES ($1, $2, $3)",
        )
        .bind(&space)
        .bind(&org)
        .bind(VIEWER)
        .execute(&pool)
        .await
        .expect("revoke the viewer's membership of the first Space");

        // The viewer holds a current decision for both Spaces, but is removed from the first: only that
        // Space is DENIED, the other answers, and the call itself is not an error.
        let results = attend(
            &pool,
            &org,
            VIEWER,
            pb::UnreadRule::LastAuthor,
            vec![
                ask_for(&org, VIEWER, &space, CEILING, 0),
                ask_for(&org, VIEWER, &kept, CEILING, 0),
            ],
        )
        .await;
        let answered: Vec<(&str, i32)> = results
            .iter()
            .map(|result| (result.space_id.as_str(), result.status))
            .collect();
        assert_eq!(
            answered,
            [
                (space.as_str(), pb::AttentionStatus::Denied as i32),
                (kept.as_str(), pb::AttentionStatus::Ok as i32)
            ]
        );
        assert_eq!(results[0].latest_activity_ms, 0, "a denied Space carries no counts");
        // The revocation is the viewer's own: another member of the first Space is unaffected.
        let other = attend(
            &pool,
            &org,
            "viewer-2",
            pb::UnreadRule::LastAuthor,
            vec![ask_for(&org, "viewer-2", &space, CEILING, 0)],
        )
        .await;
        assert_eq!(other[0].status, pb::AttentionStatus::Ok as i32);
        clean(&pool, &org, &[space.as_str(), kept.as_str()]).await;
    }
}
