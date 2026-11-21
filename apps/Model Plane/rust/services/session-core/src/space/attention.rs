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
//! named. The `since_join` history floor it may sign (WS04-S11) is the floor for every message
//! read: a headline's first user message, the unread probe and the room's latest activity see
//! only what the floor lets the reader see. The index is never the access boundary: each query
//! restates org, Space, archive, ceiling and floor predicates.
//!
//! No migration: the thread activity summary (migration 0078) and `threads.last_activity_at`
//! (0049) serve the candidate selection, so cost follows new activity, not room history.
//!
//! Message text never crosses this surface. The only text returned is a thread's display title,
//! for the thread ids the caller asked for, under the same ceiling and floor, and never for a
//! thread that holds a `zdr_waived` run. Ids, titles and counts never reach a log line.

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
use crate::space::audience_window::floor_gate_message_sql;

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

/// The history floor predicate of a message read (WS04-S11), `floor` being the placeholder that
/// carries it. A message stamped before the floor is hidden; an unstamped one is not, as in the
/// conversation read; no floor (`NULL`: the switch off, or no signed claim) hides nothing. The
/// message is `m`.
fn floor_predicate(floor: &str) -> String {
    format!(
        "({floor}::BIGINT IS NULL OR m.recipient_audience_revision IS NULL
                 OR m.recipient_audience_revision >= {floor}::BIGINT)"
    )
}

/// The thread-level floor gate (WS04-S11, V3e-sws-2): under a floor a thread needs a message the
/// reader may see from the floor on (on the active path, not deleted, within the ceiling, at or after
/// the floor), the same gate `ListThreads` and the Space timeline apply. No floor (`NULL`: the switch
/// off, or no signed claim) passes every thread. The thread is `t`, the ceiling is `$3`, and the floor
/// is the placeholder `floor` that carries it.
fn thread_floor_predicate(floor: &str) -> String {
    format!(
        "({floor}::BIGINT IS NULL OR EXISTS (SELECT 1 FROM messages em
                 WHERE em.thread_id = t.id
                   AND em.on_active_path
                   AND {gate}))",
        gate = floor_gate_message_sql("em", "$3", floor),
    )
}

/// The run-level floor of an attention read of runs or approvals (V3e-sws-2): a run stamped before the
/// floor is not the reader's, and an unstamped run passes, as an unstamped message does. The run is
/// `r`; the floor is the placeholder `floor` that carries it.
fn run_floor_predicate(floor: &str) -> String {
    format!(
        "({floor}::BIGINT IS NULL OR r.recipient_audience_revision IS NULL
                 OR r.recipient_audience_revision >= {floor}::BIGINT)"
    )
}

/// The newest user or assistant message of a thread the ceiling and the history floor let the
/// caller see, on the active path, not deleted. Parameters: $3 ceiling, and the floor under the
/// placeholder `floor`.
fn latest_visible_message(floor: &str) -> String {
    let predicate = floor_predicate(floor);
    format!(
        "SELECT m.created_at, m.author_subject_id
        FROM messages m
       WHERE m.thread_id = t.id AND m.role IN ('user', 'assistant') AND m.on_active_path
         AND (m.recipient_audience_revision IS NULL OR m.recipient_audience_revision <= $3)
         AND {predicate}
         AND NOT EXISTS (SELECT 1 FROM message_revisions r WHERE r.message_id = m.id AND r.action = 'deleted')
       ORDER BY m.sequence DESC
       LIMIT 1"
    )
}

/// Unread candidates. Parameters: $1 org, $2 Space, $3 ceiling, $4 marker time, $5 viewer,
/// $6 `true` = last-author rule, `false` = legacy owner rule, $7 row cap, $8 history floor.
/// Threads are pre-filtered by `last_activity_at` so the lateral probe runs only for threads that
/// moved after the marker.
fn unread_sql() -> String {
    let latest = latest_visible_message("$8");
    format!(
        "SELECT t.id
           FROM threads t
           JOIN LATERAL ({latest}) lm ON TRUE
          WHERE {THREAD_SCOPE}
            AND t.last_activity_at > $4::TIMESTAMPTZ
            AND lm.created_at > $4::TIMESTAMPTZ
            AND CASE WHEN $6::BOOLEAN THEN lm.author_subject_id IS DISTINCT FROM $5
                     ELSE t.user_id <> $5 END
          ORDER BY t.last_activity_at DESC
          LIMIT $7"
    )
}

/// The room's latest visible activity. Parameters: $1 org, $2 Space, $3 ceiling, $4 probe size,
/// $5 history floor. Under a floor the probe picks only threads the reader may see (V3e-sws-2), so a
/// run of pre-join threads cannot fill it.
fn latest_activity_sql() -> String {
    let latest = latest_visible_message("$5");
    let thread_gate = thread_floor_predicate("$5");
    format!(
        "SELECT max(lm.created_at)
           FROM (SELECT t.id
                   FROM threads t
                  WHERE {THREAD_SCOPE} AND t.last_message_seq IS NOT NULL
                    AND {thread_gate}
                  ORDER BY t.last_message_seq DESC
                  LIMIT $4) t
           JOIN LATERAL ({latest}) lm ON TRUE"
    )
}

/// Root runs of the Space that are live right now. Parameters: $1 org, $2 Space, $3 ceiling,
/// $4 window minutes, $5 history floor. Persistent runs only: a zero-retention or waived run leaves no
/// attention trace. Under a floor a run is counted only on a thread the reader may see, and only when
/// the run is stamped at or after the floor (V3e-sws-2).
fn working_sql() -> String {
    let thread_gate = thread_floor_predicate("$5");
    let run_gate = run_floor_predicate("$5");
    format!(
        "SELECT count(*)
           FROM runs r
           JOIN threads t ON t.id = r.thread_id AND t.org_id = r.org_id
          WHERE r.org_id = $1 AND r.space_id = $2 AND r.parent_run_id IS NULL
            AND r.retention_class = 'persistent'
            AND ({RUN_PHASE_SQL}) IN ('queued', 'running')
            AND r.updated_at > now() - make_interval(mins => $4)
            AND {THREAD_SCOPE}
            AND {thread_gate}
            AND {run_gate}"
    )
}

/// Approvals addressed to the viewer in this Space, oldest first. The addressee predicate of
/// WS10-S6: the requester for a requester-addressed approval, the named member for a named one.
/// An `any_editor` or `any_manager` approval is not listed: Session Core cannot see the viewer's
/// role, so those wait for the role-aware read. Under a history floor an approval is named only on a
/// thread the reader may see and only when its run is stamped at or after the floor (V3e-sws-2).
/// Parameters: $1 org, $2 Space, $3 ceiling, $4 viewer, $5 row cap, $6 history floor.
fn pending_sql() -> String {
    let thread_gate = thread_floor_predicate("$6");
    let run_gate = run_floor_predicate("$6");
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
            AND {thread_gate}
            AND {run_gate}
          ORDER BY a.requested_at ASC, a.id ASC
          LIMIT $5"
    )
}

/// Display titles of the requested threads that pass the ceiling and hold no `zdr_waived` run.
/// Under the history floor a thread is named only when it passes the gate `ListThreads` and the
/// Space timeline apply: it holds a message stamped at or after the floor. A thread with only
/// earlier messages gets no row at all, its title included. The title is the thread list's
/// derivation: the presentation title, else the first user message the floor lets the reader see,
/// else the session key. Parameters: $1 org, $2 Space, $3 ceiling, $4 thread ids, $5 title source
/// length, $6 history floor.
fn headlines_sql() -> String {
    let floor = floor_predicate("$6");
    let thread_floor = thread_floor_predicate("$6");
    format!(
        "SELECT t.id, t.session_key,
                COALESCE(t.presentation_title, first_user.content) AS title
           FROM threads t
           LEFT JOIN LATERAL (
                SELECT left(m.content, $5) AS content
                  FROM messages m
                 WHERE m.thread_id = t.id AND m.role = 'user' AND m.on_active_path
                   AND (m.recipient_audience_revision IS NULL OR m.recipient_audience_revision <= $3)
                   AND {floor}
                   AND NOT EXISTS (SELECT 1 FROM message_revisions r WHERE r.message_id = m.id)
                 ORDER BY m.sequence ASC
                 LIMIT 1
           ) first_user ON TRUE
          WHERE t.id = ANY($4) AND {THREAD_SCOPE}
            AND {thread_floor}
            AND NOT EXISTS (SELECT 1 FROM runs wr WHERE wr.thread_id = t.id AND wr.retention_class = 'zdr_waived')"
    )
}

// ---------------------------------------------------------------------------------------------
// Handler.
// ---------------------------------------------------------------------------------------------

/// One Space whose decision verified: the ceiling and the history floor every query below restates.
struct Reach {
    ceiling: i64,
    /// WS04-S11: the `since_join` floor the decision signs. `None` is full history, which the
    /// switch being off gives too.
    floor: Option<i64>,
}

/// The reach a verified Space read gives this surface: its ceiling and the floor it may carry.
fn reach_from(read: crate::grpc::VerifiedSpaceRead) -> Reach {
    Reach {
        ceiling: read.recipient_audience_revision,
        floor: read.history_floor,
    }
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
                .bind(reach.floor)
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
            .bind(reach.floor)
            .fetch_one(&mut *tx)
            .await?;
        result.latest_activity_ms = latest.map_or(0, |at| at.timestamp_millis());

        let working: i64 = sqlx::query_scalar(&working_sql())
            .bind(org_id)
            .bind(&query.space_id)
            .bind(reach.ceiling)
            .bind(WORKING_WINDOW_MINUTES)
            .bind(reach.floor)
            .fetch_one(&mut *tx)
            .await?;
        result.working_runs = u32::try_from(working.max(0)).unwrap_or(u32::MAX);

        let pending: Vec<(String, String, String, DateTime<Utc>, String)> = sqlx::query_as(&pending_sql())
            .bind(org_id)
            .bind(&query.space_id)
            .bind(reach.ceiling)
            .bind(user_id)
            .bind(i64::try_from(MAX_PENDING).unwrap_or(20))
            .bind(reach.floor)
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
                .bind(reach.floor)
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
                    Ok(Some(read)) => Some(reach_from(read)),
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
    use super::*;
    use crate::grpc::VerifiedSpaceRead;
    use crate::space::audience_window::{history_floor_enabled_from, history_floor_from_claim};
    use crate::space::test_support::{clean_space, connect, seed_space_run, seed_space_thread};
    use mp_ids::new_ulid;
    use std::collections::BTreeSet;

    /// The placeholders a statement names, by the number Postgres binds them with.
    fn placeholders(sql: &str) -> BTreeSet<usize> {
        let mut found = BTreeSet::new();
        let mut rest = sql;
        while let Some(at) = rest.find('$') {
            let after = &rest[at + 1..];
            let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(number) = digits.parse::<usize>() {
                found.insert(number);
            }
            rest = &after[digits.len()..];
        }
        found
    }

    /// WS04-S11: each attention read of messages binds the history floor last, after the binds it
    /// already had, and states the floor predicate on the message. Checked on the statement text,
    /// so no database is needed.
    #[test]
    fn each_attention_read_of_messages_binds_the_history_floor_last() {
        assert_eq!(
            placeholders(&unread_sql()),
            (1..=8).collect::<BTreeSet<usize>>()
        );
        assert!(unread_sql().contains(floor_predicate("$8").as_str()));
        assert_eq!(
            placeholders(&latest_activity_sql()),
            (1..=5).collect::<BTreeSet<usize>>()
        );
        assert!(latest_activity_sql().contains(floor_predicate("$5").as_str()));
        assert_eq!(
            placeholders(&headlines_sql()),
            (1..=6).collect::<BTreeSet<usize>>()
        );
        assert!(headlines_sql().contains(floor_predicate("$6").as_str()));
        assert!(
            headlines_sql().contains(thread_floor_predicate("$6").as_str()),
            "the thread-level floor gate is in the outer WHERE of the headline read"
        );
    }

    /// V3e-sws-2: the working-run count and the pending approvals bind the history floor last, after
    /// the binds they had, and state the thread gate and the run-level floor. The thread gate also
    /// states the ceiling, the active path and the deletion check, so a thread whose only floor-passing
    /// message is above the ceiling or deleted is named by no headline and counts no run.
    #[test]
    fn the_working_and_pending_reads_bind_the_floor_last_and_gate_their_threads() {
        assert_eq!(
            placeholders(&working_sql()),
            (1..=5).collect::<BTreeSet<usize>>()
        );
        assert!(working_sql().contains(thread_floor_predicate("$5").as_str()));
        assert!(working_sql().contains(run_floor_predicate("$5").as_str()));
        assert_eq!(
            placeholders(&pending_sql()),
            (1..=6).collect::<BTreeSet<usize>>()
        );
        assert!(pending_sql().contains(thread_floor_predicate("$6").as_str()));
        assert!(pending_sql().contains(run_floor_predicate("$6").as_str()));
        assert_eq!(
            placeholders(&latest_activity_sql()),
            (1..=5).collect::<BTreeSet<usize>>()
        );
        assert!(latest_activity_sql().contains(thread_floor_predicate("$5").as_str()));
        assert!(headlines_sql().contains("floor_gate_rev.action = 'deleted'"));
        assert!(headlines_sql().contains("em.on_active_path"));
        assert!(headlines_sql().contains("em.recipient_audience_revision <= $3::BIGINT"));
    }

    /// The reach carries the floor a verified read signs. With the switch off the claim is dropped
    /// before it gets here, so the reach has no floor: every floor bind is NULL and nothing is
    /// hidden.
    #[test]
    fn a_verified_read_reaches_the_floor_it_signs_and_the_switch_off_reaches_none() {
        let on = history_floor_enabled_from(Some("true"));
        let signed = reach_from(VerifiedSpaceRead {
            recipient_audience_revision: 9,
            history_floor: history_floor_from_claim(7, on),
        });
        assert_eq!((signed.ceiling, signed.floor), (9, Some(7)));

        let off = history_floor_enabled_from(Some("1"));
        let dropped = reach_from(VerifiedSpaceRead {
            recipient_audience_revision: 9,
            history_floor: history_floor_from_claim(7, off),
        });
        assert_eq!((dropped.ceiling, dropped.floor), (9, None));
    }

    /// One user message of a Space thread, stamped with `revision` as its recipient-audience
    /// revision, so a test can place it on either side of a `since_join` floor.
    async fn seed_user_message(
        pool: &PgPool,
        space: &str,
        thread: &str,
        message: &str,
        text: &str,
        revision: i64,
    ) {
        sqlx::query(
            "INSERT INTO messages (id, thread_id, role, content, space_id, recipient_audience_ref, \
             recipient_audience_revision, recipient_audience_hash, authority_revision, \
             resource_authorization_ref, author_subject_id) \
             VALUES ($1, $2, 'user', $3, $4, 'audience-1', $5, 'sha256:audience-1', 1, \
             'resource-1', 'member-2')",
        )
        .bind(message)
        .bind(thread)
        .bind(text)
        .bind(space)
        .bind(revision)
        .execute(pool)
        .await
        .expect("seed user message");
    }

    /// The creation time of one message, in epoch milliseconds.
    async fn created_ms(pool: &PgPool, message: &str) -> i64 {
        let (at,): (DateTime<Utc>,) =
            sqlx::query_as("SELECT created_at FROM messages WHERE id = $1")
                .bind(message)
                .fetch_one(pool)
                .await
                .expect("message creation time");
        at.timestamp_millis()
    }

    /// The display title the answer gives `thread`, when it names that thread.
    fn headline_of(result: &pb::SpaceAttentionResult, thread: &str) -> Option<String> {
        result
            .headlines
            .iter()
            .find(|headline| headline.thread_id == thread)
            .map(|headline| headline.title.clone())
    }

    /// WS04-S11 against Postgres: under a `since_join` floor the headline, the unread count and the
    /// room's latest activity read only messages from the floor on, and a thread with no message
    /// from the floor on is named by no headline. `mixed` holds a message before the floor and one
    /// from it on; `early` holds only a message before the floor, written last, so it is the room's
    /// newest message, and it carries a presentation title written before the join. The switch off
    /// gives the full view, as before.
    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn attention_under_a_history_floor_reads_only_what_the_reader_may_see() {
        let Some(pool) = connect().await else {
            return;
        };
        let suffix = new_ulid();
        let org = format!("attention-floor-{suffix}");
        let space = format!("space-attention-floor-{suffix}");
        let mixed = format!("{suffix}-mixed");
        let early = format!("{suffix}-early");
        let viewer = "viewer-x";
        // The marker is an hour before the rows are written, so every message is after it.
        let since_ms = Utc::now().timestamp_millis() - 3_600_000;
        seed_space_thread(&pool, &org, &space, &mixed, "owner-a").await;
        seed_space_thread(&pool, &org, &space, &early, "owner-a").await;
        let mixed_before = format!("{suffix}-mixed-before");
        let mixed_after = format!("{suffix}-mixed-after");
        let early_before = format!("{suffix}-early-before");
        seed_user_message(&pool, &space, &mixed, &mixed_before, "before the join", 2).await;
        seed_user_message(&pool, &space, &mixed, &mixed_after, "after the join", 4).await;
        seed_user_message(&pool, &space, &early, &early_before, "earlier only", 2).await;
        // A title written before the join is pre-join content too: under the floor the whole row
        // for `early` is hidden, title included.
        sqlx::query("UPDATE threads SET presentation_title = $1 WHERE id = $2")
            .bind("pre-join title")
            .bind(&early)
            .execute(&pool)
            .await
            .expect("title the pre-join thread");
        let after_ms = created_ms(&pool, &mixed_after).await;
        let newest_ms = created_ms(&pool, &early_before).await;

        let grpc = SpaceAttentionGrpc::new(pool.clone(), true);
        let query = pb::SpaceAttentionQuery {
            space_id: space.clone(),
            since_ms,
            headline_thread_ids: vec![mixed.clone(), early.clone()],
            ..Default::default()
        };

        // A member who joined at revision 3; the decision is at revision 5.
        let on = history_floor_enabled_from(Some("true"));
        let floor = history_floor_from_claim(3, on);
        let floored = grpc
            .read_space(
                &org,
                viewer,
                Rule::LastAuthor,
                &query,
                &Reach { ceiling: 5, floor },
            )
            .await
            .expect("floored read");
        assert_eq!(
            floored.unread_threads, 1,
            "only the thread with a message from the floor on is unread"
        );
        assert_eq!(
            floored.latest_activity_ms, after_ms,
            "the latest activity is the newest message the floor lets the reader see"
        );
        assert_eq!(
            headline_of(&floored, &mixed).as_deref(),
            Some("after the join")
        );
        assert_eq!(
            headline_of(&floored, &early),
            None,
            "a thread with only earlier messages is named by no headline, its title included"
        );
        assert_eq!(
            floored.headlines.len(),
            1,
            "only the thread with a message from the floor on is named"
        );

        // The switch off: the same signed claim is ignored, so the full view is read.
        let off = history_floor_from_claim(3, history_floor_enabled_from(Some("1")));
        let full = grpc
            .read_space(
                &org,
                viewer,
                Rule::LastAuthor,
                &query,
                &Reach {
                    ceiling: 5,
                    floor: off,
                },
            )
            .await
            .expect("unfloored read");
        assert_eq!(full.unread_threads, 2, "the switch off counts both threads");
        assert_eq!(
            full.latest_activity_ms, newest_ms,
            "the switch off sees the pre-join message too"
        );
        assert_eq!(
            headline_of(&full, &mixed).as_deref(),
            Some("before the join")
        );
        assert_eq!(
            headline_of(&full, &early).as_deref(),
            Some("pre-join title"),
            "the switch off names the early thread by its title"
        );
        assert_eq!(full.headlines.len(), 2, "the switch off names both threads");

        clean_space(&pool, &org, &space).await;
    }

    /// V3e-sws-2 against Postgres (finding 127): under a `since_join` floor the thread gate of the headline
    /// read names a thread only for a message the reader may see from the floor on. `above` holds a message
    /// from the floor on stamped above the ceiling, `deleted` holds a message from the floor on that is
    /// deleted, and `visible` holds one that stands. Each has a message from before the floor. Under the
    /// floor only `visible` is named, titled by its message from the floor on. The switch off names all three.
    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn attention_headline_gate_under_a_floor_names_only_a_thread_with_a_visible_message() {
        let Some(pool) = connect().await else {
            return;
        };
        let suffix = new_ulid();
        let org = format!("attention-gate-floor-{suffix}");
        let space = format!("space-attention-gate-floor-{suffix}");
        let viewer = "viewer-x";
        let above = format!("{suffix}-above");
        let deleted = format!("{suffix}-deleted");
        let visible = format!("{suffix}-visible");
        for thread in [&above, &deleted, &visible] {
            seed_space_thread(&pool, &org, &space, thread, "owner-a").await;
        }
        let above_before = format!("{suffix}-above-before");
        let above_late = format!("{suffix}-above-late");
        let deleted_before = format!("{suffix}-deleted-before");
        let deleted_late = format!("{suffix}-deleted-late");
        let visible_before = format!("{suffix}-visible-before");
        let visible_late = format!("{suffix}-visible-late");
        seed_user_message(&pool, &space, &above, &above_before, "before the join", 2).await;
        seed_user_message(&pool, &space, &above, &above_late, "above the ceiling", 6).await;
        seed_user_message(
            &pool,
            &space,
            &deleted,
            &deleted_before,
            "before the join",
            2,
        )
        .await;
        seed_user_message(
            &pool,
            &space,
            &deleted,
            &deleted_late,
            "deleted after the join",
            4,
        )
        .await;
        seed_user_message(
            &pool,
            &space,
            &visible,
            &visible_before,
            "before the join",
            2,
        )
        .await;
        seed_user_message(
            &pool,
            &space,
            &visible,
            &visible_late,
            "visible after the join",
            4,
        )
        .await;
        // The deletion of the message from the floor on: a delete row, which is terminal.
        sqlx::query(
            "INSERT INTO message_revisions (revision_id, org_id, thread_id, message_id, revision_no, \
             action, actor_subject_id, actor_class, moderation_reason, content, content_digest, \
             prev_content_digest, original_retention, idempotency_hash, space_id, \
             recipient_audience_revision, authority_revision, resource_authorization_ref) \
             VALUES ($1, $2, $3, $4, 1, 'deleted', 'member-2', 'author', '', NULL, 'sha256:deleted', \
             'sha256:prev', 'retain', $5, $6, 4, 1, 'resource-1')",
        )
        .bind(format!("{suffix}-deleted-rev"))
        .bind(&org)
        .bind(&deleted)
        .bind(&deleted_late)
        .bind(format!("{suffix}-deleted-idem"))
        .bind(&space)
        .execute(&pool)
        .await
        .expect("record the deletion of the message from the floor on");

        let grpc = SpaceAttentionGrpc::new(pool.clone(), true);
        let query = pb::SpaceAttentionQuery {
            space_id: space.clone(),
            headline_thread_ids: vec![above.clone(), deleted.clone(), visible.clone()],
            ..Default::default()
        };
        // A member who joined at revision 3; the decision is at revision 5.
        let on = history_floor_enabled_from(Some("true"));
        let floored = grpc
            .read_space(
                &org,
                viewer,
                Rule::LastAuthor,
                &query,
                &Reach {
                    ceiling: 5,
                    floor: history_floor_from_claim(3, on),
                },
            )
            .await
            .expect("floored read");
        assert_eq!(
            headline_of(&floored, &above),
            None,
            "a message from the floor on above the ceiling is not visible, so no headline"
        );
        assert_eq!(
            headline_of(&floored, &deleted),
            None,
            "a message from the floor on that is deleted is not visible, so no headline"
        );
        assert_eq!(
            headline_of(&floored, &visible).as_deref(),
            Some("visible after the join"),
            "a visible message from the floor on names the thread, titled by it"
        );
        assert_eq!(
            floored.headlines.len(),
            1,
            "only the thread with a visible message from the floor on is named"
        );

        // The switch off: the same signed claim is ignored, so all three threads are named.
        let off = history_floor_from_claim(3, history_floor_enabled_from(Some("1")));
        let full = grpc
            .read_space(
                &org,
                viewer,
                Rule::LastAuthor,
                &query,
                &Reach {
                    ceiling: 5,
                    floor: off,
                },
            )
            .await
            .expect("unfloored read");
        assert!(
            headline_of(&full, &above).is_some(),
            "the switch off names the thread above the ceiling"
        );
        assert!(
            headline_of(&full, &deleted).is_some(),
            "the switch off names the thread whose floor message is deleted"
        );
        assert_eq!(
            headline_of(&full, &visible).as_deref(),
            Some("before the join"),
            "the switch off titles the visible thread by its first turn"
        );
        assert_eq!(
            full.headlines.len(),
            3,
            "the switch off names all three threads"
        );

        clean_space(&pool, &org, &space).await;
    }

    /// V3e-sws-2 against Postgres: under a `since_join` floor a run is neither counted nor named when
    /// its thread holds only earlier messages, or when the run itself was stamped before the floor (even
    /// on a thread that holds a message from the floor on). The approvals of such a run are not named to
    /// the viewer. A run and an approval from the floor on are both answered. The switch off counts and
    /// names them all.
    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn attention_under_a_floor_counts_and_names_no_run_or_approval_the_reader_cannot_see() {
        let Some(pool) = connect().await else {
            return;
        };
        let suffix = new_ulid();
        let org = format!("attention-runs-floor-{suffix}");
        let space = format!("space-attention-runs-floor-{suffix}");
        let viewer = "viewer-x";
        let early = format!("{suffix}-early");
        let late = format!("{suffix}-late");
        seed_space_thread(&pool, &org, &space, &early, "owner-a").await;
        seed_space_thread(&pool, &org, &space, &late, "owner-a").await;
        seed_user_message(
            &pool,
            &space,
            &early,
            &format!("{suffix}-early-msg"),
            "earlier only",
            2,
        )
        .await;
        seed_user_message(
            &pool,
            &space,
            &late,
            &format!("{suffix}-late-msg"),
            "after the join",
            4,
        )
        .await;
        let early_run = format!("{suffix}-early-run");
        let late_run = format!("{suffix}-late-run");
        seed_space_run(
            &pool,
            &org,
            &space,
            &early,
            &early_run,
            "running",
            "persistent",
        )
        .await;
        seed_space_run(
            &pool,
            &org,
            &space,
            &late,
            &late_run,
            "running",
            "persistent",
        )
        .await;
        // A run on the thread that holds a message from the floor on, stamped before the floor: the thread
        // gate passes, so only the run-level floor keeps this run and its approval out.
        let late_early_run = format!("{suffix}-late-early-run");
        seed_space_run(
            &pool,
            &org,
            &space,
            &late,
            &late_early_run,
            "running",
            "persistent",
        )
        .await;
        // A run is stamped with the audience revision it was started under.
        for (run, revision) in [
            (&early_run, 2_i64),
            (&late_run, 4_i64),
            (&late_early_run, 2_i64),
        ] {
            sqlx::query("UPDATE runs SET recipient_audience_revision = $1 WHERE id = $2")
                .bind(revision)
                .bind(run)
                .execute(&pool)
                .await
                .expect("stamp the run");
        }
        for (approval, run) in [
            (format!("{suffix}-early-approval"), &early_run),
            (format!("{suffix}-late-approval"), &late_run),
            (format!("{suffix}-late-early-approval"), &late_early_run),
        ] {
            sqlx::query(
                "INSERT INTO approvals (id, run_id, kind, status, org_id, user_id) \
                 VALUES ($1, $2, 'plan', 'requested', $3, $4)",
            )
            .bind(approval)
            .bind(run)
            .bind(&org)
            .bind(viewer)
            .execute(&pool)
            .await
            .expect("seed an approval addressed to the viewer");
        }

        let grpc = SpaceAttentionGrpc::new(pool.clone(), true);
        let query = pb::SpaceAttentionQuery {
            space_id: space.clone(),
            ..Default::default()
        };
        let on = history_floor_enabled_from(Some("true"));
        let floored = grpc
            .read_space(
                &org,
                viewer,
                Rule::LastAuthor,
                &query,
                &Reach {
                    ceiling: 5,
                    floor: history_floor_from_claim(3, on),
                },
            )
            .await
            .expect("floored read");
        assert_eq!(
            floored.working_runs, 1,
            "only the run stamped from the floor on is counted, not the one stamped before it"
        );
        let named: Vec<&str> = floored.pending.iter().map(|p| p.run_id.as_str()).collect();
        assert_eq!(
            named,
            vec![late_run.as_str()],
            "only the approval of a run from the floor on is named"
        );

        // The switch off: no floor, so the run and the approval before the floor are answered too.
        let full = grpc
            .read_space(
                &org,
                viewer,
                Rule::LastAuthor,
                &query,
                &Reach {
                    ceiling: 5,
                    floor: None,
                },
            )
            .await
            .expect("unfloored read");
        assert_eq!(full.working_runs, 3, "the switch off counts all three runs");
        assert_eq!(
            full.pending.len(),
            3,
            "the switch off names all three approvals"
        );

        sqlx::query("DELETE FROM approvals WHERE org_id = $1")
            .bind(&org)
            .execute(&pool)
            .await
            .expect("remove the test approvals");
        clean_space(&pool, &org, &space).await;
    }
}
