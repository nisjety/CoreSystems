//! Dreaming Core memory consolidation primitives.
//!
//! Two extractors run over the same transcript and their results are merged:
//!
//!   * a deterministic phrase matcher ([`extract_memory_candidates`]) that
//!     recognises explicit instructions — "husk at ...", "jeg heter ...";
//!   * [`crate::dream_extractor`], which asks a model what in the window is
//!     worth keeping.
//!
//! The matcher is the floor. It wins any key collision, and it is all that runs
//! when extraction is unconfigured or inference is down, so the loop degrades to
//! its previous behaviour rather than stopping. Both write the same
//! `agent_memory` rows through the same `dream_runs` ledger.

use crate::dream_extractor::{DreamExtractor, WindowMessage, LLM_SOURCE_LINK};
use crate::letta_adapter::LettaMemoryAdapter;
use chrono::{DateTime, Utc};
use metrics::{counter, histogram};
use mp_ids::new_ulid;
use sqlx::{PgPool, Postgres, Transaction};
use std::fmt::Write;
use std::time::{Duration, Instant};
use tracing::{info, warn};

pub(crate) const AGENT_MEMORY_CONTEXT_LIMIT: i64 = 24;

/// How much already-consolidated history to prepend to an extraction window.
///
/// Pending messages alone are not enough context on an ongoing thread: after
/// the first cycle only the newest turns are pending, and "yes, that team" says
/// nothing without what came before. Bounded tightly because this is read on
/// every thread of every cycle.
const EXTRACTION_CONTEXT_MESSAGES: i64 = 6;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DreamMemoryCandidate {
    pub scope: &'static str,
    pub session_id: Option<String>,
    pub key: String,
    pub content: String,
    pub kind: &'static str,
    pub confidence: f64,
    /// True when a model inferred this rather than the user stating it.
    ///
    /// Carried into `source_links` as [`LLM_SOURCE_LINK`] so the row's
    /// provenance survives into the memory-management surface: a user looking
    /// at "what do you remember about me" can tell what they asked Velion to
    /// remember from what it decided to remember, and delete the latter.
    pub inferred: bool,
}

#[derive(Debug)]
pub(crate) struct MemorySearchRow {
    pub id: String,
    pub thread_id: String,
    pub topic: String,
    pub content: String,
    pub score: f32,
    pub updated_at: DateTime<Utc>,
}

struct PendingMessage {
    message_id: String,
    thread_id: String,
    role: String,
    content: String,
    org_id: String,
    user_id: String,
}

pub(crate) async fn run(pool: PgPool, letta: Option<LettaMemoryAdapter>) -> anyhow::Result<()> {
    let interval_secs = std::env::var("DREAMING_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300u64);
    let batch_size = std::env::var("DREAMING_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100i64)
        .clamp(1, 1_000);
    let extractor = DreamExtractor::from_env();
    let mut tick = tokio::time::interval(Duration::from_secs(interval_secs));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    info!(
        interval_secs,
        batch_size,
        llm_extraction = extractor.is_some(),
        "Dreaming Core loop started"
    );

    loop {
        tick.tick().await;
        let start = Instant::now();
        match dream_once(&pool, letta.as_ref(), extractor.as_ref(), batch_size).await {
            Ok(processed) => {
                counter!("mp_session_dream_runs_total", "status" => "ok").increment(1);
                histogram!("mp_session_dream_duration_seconds")
                    .record(start.elapsed().as_secs_f64());
                if processed > 0 {
                    info!(processed, "Dreaming Core cycle complete");
                }
            }
            Err(error) => {
                counter!("mp_session_dream_runs_total", "status" => "error").increment(1);
                histogram!("mp_session_dream_duration_seconds")
                    .record(start.elapsed().as_secs_f64());
                warn!(%error, "Dreaming Core cycle failed");
            }
        }
    }
}

pub(crate) async fn dream_once(
    pool: &PgPool,
    letta: Option<&LettaMemoryAdapter>,
    extractor: Option<&DreamExtractor>,
    limit: i64,
) -> anyhow::Result<i64> {
    let messages = load_pending_messages(pool, limit).await?;
    let mut processed = 0i64;

    // Grouped by thread because extraction needs a conversation, not a line.
    // The ledger stays per-message — every message still gets its `dream_runs`
    // row, so the pending query is unchanged and a partial cycle resumes
    // exactly where it stopped.
    for group in group_by_thread(messages) {
        let Some(last) = group.last() else { continue };
        let org_id = last.org_id.clone();
        let thread_id = last.thread_id.clone();

        // Attached to the newest message of the group: that is the turn whose
        // arrival justified re-reading the window, and source_links should point
        // at it rather than at whichever message happened to sort first.
        let extracted = match extractor {
            Some(extractor) => {
                let window = extraction_window(pool, &thread_id, &group).await;
                extractor.extract(&org_id, &thread_id, &window).await
            }
            None => Vec::new(),
        };
        let anchor_message_id = last.message_id.clone();

        for message in &group {
            let mut candidates =
                extract_memory_candidates(&message.role, &message.content, &message.thread_id);
            if message.message_id == anchor_message_id {
                merge_extracted(&mut candidates, &extracted);
            }

            let mut tx = pool.begin().await?;
            let saved = persist_candidates(
                &mut tx,
                &message.org_id,
                &message.user_id,
                &message.thread_id,
                &message.message_id,
                &candidates,
            )
            .await?;
            record_dream_run(
                &mut tx,
                &message.org_id,
                &message.thread_id,
                "background_scan",
                i64::try_from(candidates.len()).unwrap_or(i64::MAX),
                saved,
                Some(&message.message_id),
            )
            .await?;
            tx.commit().await?;
            sync_candidates_to_letta(
                letta,
                &message.org_id,
                &message.user_id,
                &message.thread_id,
                &candidates,
            )
            .await;
            processed += 1;
        }
    }

    Ok(processed)
}

/// Partition pending messages by thread, preserving the load order within each.
fn group_by_thread(messages: Vec<PendingMessage>) -> Vec<Vec<PendingMessage>> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: std::collections::HashMap<String, Vec<PendingMessage>> =
        std::collections::HashMap::new();
    for message in messages {
        let thread_id = message.thread_id.clone();
        if !groups.contains_key(&thread_id) {
            order.push(thread_id.clone());
        }
        groups.entry(thread_id).or_default().push(message);
    }
    order
        .into_iter()
        .filter_map(|thread_id| groups.remove(&thread_id))
        .collect()
}

/// The pending messages preceded by a little already-consolidated history.
///
/// A read failure costs the window its context, not the extraction: the pending
/// messages alone are still worth reading.
async fn extraction_window(
    pool: &PgPool,
    thread_id: &str,
    pending: &[PendingMessage],
) -> Vec<WindowMessage> {
    let oldest_pending = pending.first().map(|message| message.message_id.as_str());
    let mut window = match load_preceding_context(pool, thread_id, oldest_pending).await {
        Ok(rows) => rows,
        Err(error) => {
            warn!(%error, %thread_id, "dreaming could not read prior thread context");
            Vec::new()
        }
    };
    window.extend(pending.iter().map(|message| WindowMessage {
        role: message.role.clone(),
        content: message.content.clone(),
    }));
    window
}

/// Fold extracted candidates into the deterministic ones.
///
/// The matcher wins any key collision. "husk at X" is an instruction to obey
/// exactly; the model's reading of the same sentence is an inference, and an
/// inference must never displace what someone actually asked for.
fn merge_extracted(candidates: &mut Vec<DreamMemoryCandidate>, extracted: &[DreamMemoryCandidate]) {
    for candidate in extracted {
        if candidates
            .iter()
            .any(|existing| existing.key == candidate.key)
        {
            continue;
        }
        candidates.push(candidate.clone());
    }
}

pub(crate) async fn sync_candidates_to_letta(
    letta: Option<&LettaMemoryAdapter>,
    org_id: &str,
    user_id: &str,
    thread_id: &str,
    candidates: &[DreamMemoryCandidate],
) {
    let Some(letta) = letta else {
        return;
    };

    for candidate in candidates {
        // Mirrors persist_candidates' ownership rule: only `scope == "user"`
        // candidates are tagged with an owner. No memory_id is threaded
        // through here, so the durable and semantic copies of a
        // background-dreamed candidate are not id-correlated -- only the
        // explicit IndexMemory RPC path (memory_grpc::index_memory) gets
        // that. Acceptable today because background dreaming has not yet
        // produced any real candidates.
        let owner = (candidate.scope == "user").then_some(user_id);
        letta
            .index(
                org_id,
                thread_id,
                memory_topic(candidate.scope, candidate.kind),
                &candidate.content,
                owner,
                None,
            )
            .await;
    }
}

pub(crate) fn extract_memory_candidates(
    role: &str,
    content: &str,
    thread_id: &str,
) -> Vec<DreamMemoryCandidate> {
    let normalized = collapse_whitespace(content);
    if normalized.is_empty() {
        return Vec::new();
    }

    let role = role.trim().to_ascii_lowercase();
    match role.as_str() {
        "user" => extract_user_candidates(&normalized),
        "assistant" => extract_assistant_candidates(&normalized, thread_id),
        _ => Vec::new(),
    }
}

pub(crate) async fn persist_candidates(
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    user_id: &str,
    thread_id: &str,
    message_id: &str,
    candidates: &[DreamMemoryCandidate],
) -> Result<i64, sqlx::Error> {
    if candidates.is_empty() {
        return Ok(0);
    }

    let base_source_links = vec![
        format!("thread:{thread_id}"),
        format!("message:{message_id}"),
    ];
    let mut saved = 0i64;

    for candidate in candidates {
        // The upsert REPLACES source_links rather than appending, so the marker
        // has to be rebuilt per candidate: a re-extraction that dropped it would
        // silently relabel an inferred memory as a stated one.
        let source_links = if candidate.inferred {
            let mut links = base_source_links.clone();
            links.push(LLM_SOURCE_LINK.to_owned());
            links
        } else {
            base_source_links.clone()
        };
        let owner = if candidate.scope == "user" {
            user_id
        } else {
            ""
        };
        let memory_id = upsert_agent_memory(tx, org_id, owner, candidate, &source_links).await?;
        saved += i64::from(!memory_id.is_empty());
    }

    Ok(saved)
}

pub(crate) async fn record_dream_run(
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    thread_id: &str,
    trigger: &str,
    found: i64,
    saved: i64,
    message_id: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO dream_runs \
         (id, org_id, thread_id, trigger, status, memories_found, memories_saved, completed_at, metadata) \
         VALUES ($1, $2, $3, $4, 'completed', $5, $6, now(), $7)",
    )
    .bind(new_ulid())
    .bind(org_id)
    .bind(thread_id)
    .bind(trigger)
    .bind(i32::try_from(found).unwrap_or(i32::MAX))
    .bind(i32::try_from(saved).unwrap_or(i32::MAX))
    .bind(serde_json::json!({ "message_id": message_id }))
    .execute(&mut **tx)
    .await?;

    Ok(())
}

async fn load_pending_messages(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<PendingMessage>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (String, String, String, String, String, String)>(
        "SELECT m.id, m.thread_id, m.role, m.content, t.org_id, t.user_id \
         FROM messages m \
         JOIN threads t ON t.id = m.thread_id \
         WHERE m.role IN ('user', 'assistant') \
           AND NOT EXISTS ( \
             SELECT 1 FROM dream_runs d \
             WHERE d.thread_id = m.thread_id \
               AND d.metadata ->> 'message_id' = m.id \
           ) \
         ORDER BY m.created_at ASC, m.sequence ASC \
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(message_id, thread_id, role, content, org_id, user_id)| PendingMessage {
                message_id,
                thread_id,
                role,
                content,
                org_id,
                user_id,
            },
        )
        .collect())
}

/// The last few already-consolidated turns of a thread, oldest first.
///
/// Ordered DESC then reversed so the LIMIT keeps the messages nearest the
/// pending window; ordering ASC with a LIMIT would return the thread's opening
/// turns instead, which is the least useful context available.
async fn load_preceding_context(
    pool: &PgPool,
    thread_id: &str,
    before_message_id: Option<&str>,
) -> Result<Vec<WindowMessage>, sqlx::Error> {
    let Some(before_message_id) = before_message_id else {
        return Ok(Vec::new());
    };
    let mut rows = sqlx::query_as::<_, (String, String)>(
        "SELECT m.role, m.content \
         FROM messages m \
         WHERE m.thread_id = $1 \
           AND m.role IN ('user', 'assistant') \
           AND (m.created_at, m.sequence) < ( \
             SELECT b.created_at, b.sequence FROM messages b WHERE b.id = $2 \
           ) \
         ORDER BY m.created_at DESC, m.sequence DESC \
         LIMIT $3",
    )
    .bind(thread_id)
    .bind(before_message_id)
    .bind(EXTRACTION_CONTEXT_MESSAGES)
    .fetch_all(pool)
    .await?;
    rows.reverse();
    Ok(rows
        .into_iter()
        .map(|(role, content)| WindowMessage { role, content })
        .collect())
}

pub(crate) async fn load_agent_memory_context_rows(
    pool: &PgPool,
    thread_id: &str,
    user_id_hint: Option<&str>,
    limit: i64,
) -> Result<Vec<(String, String)>, sqlx::Error> {
    let Some((org_id, thread_user_id)) =
        sqlx::query_as::<_, (String, String)>("SELECT org_id, user_id FROM threads WHERE id = $1")
            .bind(thread_id)
            .fetch_optional(pool)
            .await?
    else {
        return Ok(Vec::new());
    };

    let user_id = user_id_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(thread_user_id.as_str());

    let rows = sqlx::query_as::<_, (String, String, String, String, String)>(
        "SELECT scope, kind, key, content, COALESCE(session_id, '') \
         FROM agent_memory \
         WHERE org_id = $1 \
           AND review_state = 'accepted' \
           AND (expires_at IS NULL OR expires_at > now()) \
           AND ( \
                session_id = $2 \
                OR (session_id IS NULL AND scope IN ('org', 'global')) \
                OR (session_id IS NULL AND scope = 'user' AND owner = $3) \
           ) \
         ORDER BY \
           CASE scope \
             WHEN 'user' THEN 0 \
             WHEN 'thread' THEN 1 \
             WHEN 'session' THEN 2 \
             WHEN 'org' THEN 3 \
             ELSE 4 \
           END, \
           updated_at DESC \
         LIMIT $4",
    )
    .bind(&org_id)
    .bind(thread_id)
    .bind(user_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(scope, kind, key, content, session_id)| {
            let topic = memory_topic(&scope, &kind).to_owned();
            let source = if session_id.is_empty() {
                scope
            } else {
                format!("{scope}:{session_id}")
            };
            (topic, format!("{source}/{key}: {content}"))
        })
        .collect())
}

pub(crate) struct AgentMemorySearch<'a> {
    pub(crate) org_id: &'a str,
    pub(crate) thread_id: &'a str,
    pub(crate) owner_user_id: &'a str,
    pub(crate) query: &'a str,
    pub(crate) topic_filter: &'a [String],
    pub(crate) limit: u32,
    pub(crate) updated_after: Option<DateTime<Utc>>,
}

pub(crate) async fn search_agent_memory(
    pool: &PgPool,
    request: &AgentMemorySearch<'_>,
) -> Result<Vec<MemorySearchRow>, sqlx::Error> {
    let limit = i64::from(request.limit.clamp(1, 50));
    let filters = normalize_topic_filters(request.topic_filter);
    let query = request.query.trim();

    let rows = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            String,
            String,
            String,
            f64,
            DateTime<Utc>,
        ),
    >(
        "SELECT id, COALESCE(session_id, ''), scope, kind, key, content, confidence, updated_at \
         FROM agent_memory \
         WHERE org_id = $1 \
           AND review_state = 'accepted' \
           AND (expires_at IS NULL OR expires_at > now()) \
           AND (session_id = $2 \
                OR (session_id IS NULL AND scope <> 'user') \
                OR (session_id IS NULL AND scope = 'user' AND owner = $3)) \
           AND ($4::timestamptz IS NULL OR updated_at > $4) \
           AND (cardinality($5::text[]) = 0 \
                OR CASE \
                    WHEN scope = 'user' THEN 'USER' \
                    WHEN scope = 'agent' THEN 'AGENT' \
                    WHEN scope = 'workspace' THEN 'WORKSPACE' \
                    WHEN kind = 'policy' THEN 'POLICY' \
                    ELSE 'MEMORY' \
                  END = ANY($5::text[])) \
         ORDER BY \
           CASE \
             WHEN $6 = '' THEN 0 \
             WHEN lower(content) LIKE ('%' || lower($6) || '%') THEN 0 \
             WHEN lower(key) LIKE ('%' || lower($6) || '%') THEN 1 \
             ELSE 2 \
           END, \
           confidence DESC, \
           updated_at DESC \
         LIMIT $7",
    )
    .bind(request.org_id)
    .bind(request.thread_id)
    .bind(request.owner_user_id)
    .bind(request.updated_after)
    .bind(&filters)
    .bind(query)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(id, session_id, scope, kind, key, content, confidence, updated_at)| {
                let exact_bonus = if query.is_empty()
                    || content
                        .to_ascii_lowercase()
                        .contains(&query.to_ascii_lowercase())
                    || key
                        .to_ascii_lowercase()
                        .contains(&query.to_ascii_lowercase())
                {
                    0.15
                } else {
                    0.0
                };
                MemorySearchRow {
                    id,
                    thread_id: if session_id.is_empty() {
                        request.thread_id.to_owned()
                    } else {
                        session_id
                    },
                    topic: memory_topic(&scope, &kind).to_owned(),
                    content,
                    score: memory_score(confidence, exact_bonus),
                    updated_at,
                }
            },
        )
        .collect())
}

pub(crate) async fn index_agent_memory(
    pool: &PgPool,
    org_id: &str,
    thread_id: &str,
    owner_user_id: &str,
    topic: &str,
    content: &str,
) -> Result<String, sqlx::Error> {
    let topic = normalize_topic(topic);
    let thread_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM threads WHERE id = $1 AND org_id = $2 AND user_id = $3)",
    )
    .bind(thread_id)
    .bind(org_id)
    .bind(owner_user_id)
    .fetch_one(pool)
    .await?;
    if !thread_exists {
        return Ok(String::new());
    }

    let (scope, session_id, kind, owner) = match topic.as_str() {
        "USER" => ("user", None, "fact", owner_user_id),
        "POLICY" => ("org", None, "policy", ""),
        "AGENT" | "WORKSPACE" => ("org", None, "fact", ""),
        _ => ("thread", Some(thread_id.to_owned()), "fact", ""),
    };
    let candidate = DreamMemoryCandidate {
        scope,
        session_id,
        key: format!(
            "manual:{}:{}",
            topic.to_ascii_lowercase(),
            stable_fragment(content)
        ),
        content: collapse_whitespace(content),
        kind,
        confidence: 0.9,
        // A manual index call is a person choosing to store something, which is
        // the strongest provenance there is.
        inferred: false,
    };

    let mut tx = pool.begin().await?;
    let memory_id = upsert_agent_memory(
        &mut tx,
        org_id,
        owner,
        &candidate,
        &[format!("thread:{thread_id}"), "tool:save_memory".to_owned()],
    )
    .await?;
    tx.commit().await?;
    Ok(memory_id)
}

async fn upsert_agent_memory(
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    owner: &str,
    candidate: &DreamMemoryCandidate,
    source_links: &[String],
) -> Result<String, sqlx::Error> {
    if candidate.session_id.is_some() {
        let row: (String,) = sqlx::query_as(
            "INSERT INTO agent_memory \
             (id, org_id, session_id, scope, key, content, kind, confidence, owner, source_links, review_state) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'accepted') \
             ON CONFLICT (org_id, session_id, scope, key) WHERE session_id IS NOT NULL \
             DO UPDATE SET \
               content = EXCLUDED.content, \
               kind = EXCLUDED.kind, \
               confidence = GREATEST(agent_memory.confidence, EXCLUDED.confidence), \
               owner = EXCLUDED.owner, \
               source_links = EXCLUDED.source_links, \
               review_state = 'accepted', \
               updated_at = now() \
             RETURNING id",
        )
        .bind(new_ulid())
        .bind(org_id)
        .bind(candidate.session_id.as_deref())
        .bind(candidate.scope)
        .bind(&candidate.key)
        .bind(&candidate.content)
        .bind(candidate.kind)
        .bind(candidate.confidence)
        .bind(owner)
        .bind(source_links)
        .fetch_one(&mut **tx)
        .await?;
        return Ok(row.0);
    }

    let row: (String,) = sqlx::query_as(
        "INSERT INTO agent_memory \
         (id, org_id, session_id, scope, key, content, kind, confidence, owner, source_links, review_state) \
         VALUES ($1, $2, NULL, $3, $4, $5, $6, $7, $8, $9, 'accepted') \
         ON CONFLICT (org_id, scope, owner, key) WHERE session_id IS NULL \
         DO UPDATE SET \
           content = EXCLUDED.content, \
           kind = EXCLUDED.kind, \
           confidence = GREATEST(agent_memory.confidence, EXCLUDED.confidence), \
           owner = EXCLUDED.owner, \
           source_links = EXCLUDED.source_links, \
           review_state = 'accepted', \
           updated_at = now() \
         RETURNING id",
    )
    .bind(new_ulid())
    .bind(org_id)
    .bind(candidate.scope)
    .bind(&candidate.key)
    .bind(&candidate.content)
    .bind(candidate.kind)
    .bind(candidate.confidence)
    .bind(owner)
    .bind(source_links)
    .fetch_one(&mut **tx)
    .await?;
    Ok(row.0)
}

/// Lists durable `agent_memory` rows owned directly by a user
/// (`scope = 'user'`), across every thread. Backs the memory-management
/// "what do you remember about me" surface: unlike `search_agent_memory` this
/// is never thread-scoped, since the point is to show memory independent of
/// which conversation produced it.
pub(crate) async fn list_user_memory(
    pool: &PgPool,
    org_id: &str,
    owner_user_id: &str,
    limit: i64,
) -> Result<Vec<MemorySearchRow>, sqlx::Error> {
    let limit = limit.clamp(1, 200);
    let rows = sqlx::query_as::<_, (String, String, String, f64, DateTime<Utc>)>(
        "SELECT id, kind, content, confidence, updated_at \
         FROM agent_memory \
         WHERE org_id = $1 \
           AND owner = $2 \
           AND scope = 'user' \
           AND review_state = 'accepted' \
           AND (expires_at IS NULL OR expires_at > now()) \
         ORDER BY updated_at DESC \
         LIMIT $3",
    )
    .bind(org_id)
    .bind(owner_user_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(id, kind, content, confidence, updated_at)| MemorySearchRow {
            id,
            thread_id: String::new(),
            topic: memory_topic("user", &kind).to_owned(),
            content,
            score: memory_score(confidence, 0.0),
            updated_at,
        })
        .collect())
}

/// Deletes a single `agent_memory` row owned directly by a user
/// (`scope = 'user'`). Returns whether a row existed and was removed.
/// `(org_id, owner)` is part of the `WHERE` clause itself, not just checked
/// after the fact, so a caller can never delete another user's or another
/// org's memory by guessing an id.
pub(crate) async fn delete_user_memory(
    pool: &PgPool,
    org_id: &str,
    owner_user_id: &str,
    memory_id: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "DELETE FROM agent_memory \
         WHERE id = $1 AND org_id = $2 AND owner = $3 AND scope = 'user'",
    )
    .bind(memory_id)
    .bind(org_id)
    .bind(owner_user_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

fn extract_user_candidates(content: &str) -> Vec<DreamMemoryCandidate> {
    let mut candidates = Vec::new();
    if let Some(name) = extract_first_marker_value(
        content,
        &[
            "mitt navn er ",
            "jeg heter ",
            "my name is ",
            "call me ",
            "du kan kalle meg ",
        ],
        4,
    ) {
        candidates.push(DreamMemoryCandidate {
            scope: "user",
            session_id: None,
            key: "user:name".to_owned(),
            content: format!("User's name is {}.", title_name(&name)),
            kind: "fact",
            confidence: 0.98,
            inferred: false,
        });
    }

    if let Some(preference) = extract_first_marker_value(
        content,
        &["jeg foretrekker ", "i prefer ", "eg foretrekker "],
        16,
    ) {
        let key = format!("user:preference:{}", stable_fragment(&preference));
        candidates.push(DreamMemoryCandidate {
            scope: "user",
            session_id: None,
            key,
            content: format!("User prefers {}.", trim_sentence_end(&preference)),
            kind: "preference",
            confidence: 0.86,
            inferred: false,
        });
    }

    if let Some(fact) = extract_first_marker_value(content, &["husk at ", "remember that "], 24) {
        let key = format!("user:remember:{}", stable_fragment(&fact));
        candidates.push(DreamMemoryCandidate {
            scope: "user",
            session_id: None,
            key,
            content: format!(
                "User asked Velion to remember: {}.",
                trim_sentence_end(&fact)
            ),
            kind: "fact",
            confidence: 0.88,
            inferred: false,
        });
    }

    candidates
}

fn extract_assistant_candidates(content: &str, thread_id: &str) -> Vec<DreamMemoryCandidate> {
    let lower = content.to_ascii_lowercase();
    let markers = ["generated image artifact:", "generated an image artifact:"];
    let Some((start, marker)) = markers
        .iter()
        .find_map(|marker| lower.find(marker).map(|start| (start, *marker)))
    else {
        return Vec::new();
    };
    let raw = &content[start + marker.len()..];
    let Some(artifact) = take_value(raw, 10) else {
        return Vec::new();
    };

    vec![DreamMemoryCandidate {
        scope: "thread",
        session_id: Some(thread_id.to_owned()),
        key: "thread:last_image_artifact".to_owned(),
        content: format!(
            "Assistant generated image artifact {}.",
            trim_sentence_end(&artifact)
        ),
        kind: "artifact_summary",
        confidence: 0.9,
        inferred: false,
    }]
}

fn extract_first_marker_value(content: &str, markers: &[&str], max_words: usize) -> Option<String> {
    let lower = content.to_ascii_lowercase();
    markers.iter().find_map(|marker| {
        lower
            .find(marker)
            .and_then(|idx| take_value(&content[idx + marker.len()..], max_words))
    })
}

fn take_value(raw: &str, max_words: usize) -> Option<String> {
    let before_punctuation = raw
        .split(['.', '!', '?', ';', '\n'])
        .next()
        .unwrap_or(raw)
        .trim()
        .trim_matches(|c| matches!(c, '"' | '\'' | '`' | ':' | '-' | ',' | ' '));

    let words: Vec<&str> = before_punctuation
        .split_whitespace()
        .take(max_words)
        .collect();
    let value = words.join(" ");
    let value = trim_sentence_end(&value);
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn trim_sentence_end(value: &str) -> String {
    value
        .trim()
        .trim_matches(|c| matches!(c, '"' | '\'' | '`' | ':' | '-' | ',' | ' '))
        .to_owned()
}

fn title_name(value: &str) -> String {
    value
        .split_whitespace()
        .map(|part| {
            let mut chars = part.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            let rest = chars.as_str();
            if part.chars().all(char::is_lowercase) {
                format!("{}{}", first.to_uppercase(), rest)
            } else {
                part.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn stable_fragment(value: &str) -> String {
    let mut fragment = value
        .chars()
        .filter_map(|c| {
            if c.is_ascii_alphanumeric() {
                Some(c.to_ascii_lowercase())
            } else if c.is_whitespace() || matches!(c, '-' | '_' | '/') {
                Some('_')
            } else {
                None
            }
        })
        .collect::<String>();

    while fragment.contains("__") {
        fragment = fragment.replace("__", "_");
    }
    fragment = fragment.trim_matches('_').to_owned();
    if fragment.is_empty() {
        return format!("h{:016x}", fnv1a64(value.as_bytes()));
    }
    if fragment.len() > 48 {
        fragment.truncate(48);
        fragment.push('_');
        let suffix = fnv1a64(value.as_bytes()) & 0xffff_ffff;
        let _ = write!(fragment, "{suffix:08x}");
    }
    fragment
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf_29ce_4842_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[allow(clippy::cast_possible_truncation)]
fn memory_score(confidence: f64, exact_bonus: f64) -> f32 {
    (confidence + exact_bonus).clamp(0.0, 1.0) as f32
}

fn normalize_topic_filters(filters: &[String]) -> Vec<String> {
    filters
        .iter()
        .map(|filter| normalize_topic(filter))
        .filter(|filter| !filter.is_empty())
        .collect()
}

fn normalize_topic(topic: &str) -> String {
    match topic.trim().to_ascii_uppercase().as_str() {
        "POLICY" => "POLICY".to_owned(),
        "WORKSPACE" => "WORKSPACE".to_owned(),
        "AGENT" => "AGENT".to_owned(),
        "USER" => "USER".to_owned(),
        "MEMORY" | "EPISODIC" | "" => "MEMORY".to_owned(),
        other => other.to_owned(),
    }
}

fn memory_topic(scope: &str, kind: &str) -> &'static str {
    match (scope, kind) {
        ("user", _) => "USER",
        ("agent", _) => "AGENT",
        ("workspace", _) => "WORKSPACE",
        (_, "policy") => "POLICY",
        _ => "MEMORY",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durable_user_memory_is_owner_scoped_in_queries_and_uniqueness() {
        let source = include_str!("dreaming.rs");
        let unsafe_search = ["AND (session_id = $2 OR", " session_id IS NULL)"].concat();
        let owner_search = ["scope = 'user' AND", " owner = $3"].concat();
        assert!(!source.contains(&unsafe_search));
        assert!(source.contains(&owner_search));

        let migration = include_str!("../migrations/0011_identity_scoping.sql");
        assert!(migration.contains("ON agent_memory (org_id, scope, owner, key)"));
    }

    #[test]
    fn extracts_norwegian_user_name() {
        let candidates = extract_memory_candidates("user", "mitt navn er ima", "thread-1");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].scope, "user");
        assert_eq!(candidates[0].key, "user:name");
        assert_eq!(candidates[0].content, "User's name is Ima.");
    }

    #[test]
    fn ignores_name_questions() {
        let candidates =
            extract_memory_candidates("user", "kan du finne ut hva navnet mitt betyr?", "t1");
        assert!(candidates.is_empty());
    }

    #[test]
    fn extracts_explicit_preference() {
        let candidates =
            extract_memory_candidates("user", "jeg foretrekker korte norske svar.", "thread-1");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].kind, "preference");
        assert!(candidates[0].content.contains("korte norske svar"));
    }

    #[test]
    fn extracts_assistant_image_artifact_for_thread_scope() {
        let candidates = extract_memory_candidates(
            "assistant",
            "I generated an image artifact: generated-image.png for this request.",
            "thread-1",
        );
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].scope, "thread");
        assert_eq!(candidates[0].session_id.as_deref(), Some("thread-1"));
        assert_eq!(candidates[0].key, "thread:last_image_artifact");
    }

    fn pending(thread_id: &str, message_id: &str) -> PendingMessage {
        PendingMessage {
            message_id: message_id.to_owned(),
            thread_id: thread_id.to_owned(),
            role: "user".to_owned(),
            content: "hei".to_owned(),
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
        }
    }

    /// Extraction reads a conversation, so a cycle that interleaves two threads
    /// must not hand the model a window mixing both.
    #[test]
    fn pending_messages_are_grouped_per_thread_in_load_order() {
        let groups = group_by_thread(vec![
            pending("thread-a", "m1"),
            pending("thread-b", "m2"),
            pending("thread-a", "m3"),
            pending("thread-b", "m4"),
            pending("thread-a", "m5"),
        ]);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0][0].thread_id, "thread-a");
        let ids: Vec<&str> = groups[0]
            .iter()
            .map(|message| message.message_id.as_str())
            .collect();
        assert_eq!(
            ids,
            ["m1", "m3", "m5"],
            "order within a thread must survive"
        );
        assert_eq!(groups[1].len(), 2);
        assert_eq!(groups[1][0].thread_id, "thread-b");
    }

    /// The phrase matcher is a floor. A model reading "husk at ..." must not be
    /// able to replace what the user literally asked to be remembered.
    #[test]
    fn a_stated_memory_always_beats_an_inferred_one_on_the_same_key() {
        let mut stated = vec![DreamMemoryCandidate {
            scope: "user",
            session_id: None,
            key: "user:name".to_owned(),
            content: "User's name is Ima.".to_owned(),
            kind: "fact",
            confidence: 0.98,
            inferred: false,
        }];
        let extracted = vec![
            DreamMemoryCandidate {
                scope: "user",
                session_id: None,
                key: "user:name".to_owned(),
                content: "User's name is someone else.".to_owned(),
                kind: "fact",
                confidence: 0.85,
                inferred: true,
            },
            DreamMemoryCandidate {
                scope: "user",
                session_id: None,
                key: "user:llm:employer".to_owned(),
                content: "User works at Aquatiq.".to_owned(),
                kind: "fact",
                confidence: 0.8,
                inferred: true,
            },
        ];

        merge_extracted(&mut stated, &extracted);

        assert_eq!(stated.len(), 2, "the new slot should still be added");
        assert_eq!(stated[0].content, "User's name is Ima.");
        assert!(!stated[0].inferred);
        assert_eq!(stated[1].key, "user:llm:employer");
        assert!(stated[1].inferred);
    }

    /// Provenance has to be rebuilt on every write: the upsert REPLACES
    /// `source_links`, so a re-extraction that dropped the marker would quietly
    /// relabel an inferred memory as a stated one.
    #[test]
    fn only_inferred_candidates_carry_the_extractor_marker() {
        let source = include_str!("dreaming.rs");
        assert!(
            source.contains("links.push(LLM_SOURCE_LINK.to_owned());"),
            "inferred rows must be tagged in source_links"
        );
        assert!(
            source.contains("if candidate.inferred {"),
            "the marker must be conditional on provenance, not added to every row"
        );
    }
}
