//! Dreaming Core memory consolidation primitives.
//!
//! This first slice is intentionally deterministic: it only records explicit
//! facts/preferences/artifacts from the transcript. LLM-assisted consolidation
//! can build on the same `agent_memory` rows and `dream_runs` ledger.

use crate::letta_adapter::LettaMemoryAdapter;
use chrono::{DateTime, Utc};
use metrics::{counter, histogram};
use mp_ids::new_ulid;
use sqlx::{PgPool, Postgres, Transaction};
use std::fmt::Write;
use std::time::{Duration, Instant};
use tracing::{info, warn};

pub(crate) const AGENT_MEMORY_CONTEXT_LIMIT: i64 = 24;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DreamMemoryCandidate {
    pub scope: &'static str,
    pub session_id: Option<String>,
    pub key: String,
    pub content: String,
    pub kind: &'static str,
    pub confidence: f64,
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
    let mut tick = tokio::time::interval(Duration::from_secs(interval_secs));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    info!(interval_secs, batch_size, "Dreaming Core loop started");

    loop {
        tick.tick().await;
        let start = Instant::now();
        match dream_once(&pool, letta.as_ref(), batch_size).await {
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
    limit: i64,
) -> anyhow::Result<i64> {
    let messages = load_pending_messages(pool, limit).await?;
    let mut processed = 0i64;

    for message in messages {
        let candidates =
            extract_memory_candidates(&message.role, &message.content, &message.thread_id);
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
        sync_candidates_to_letta(letta, &message.org_id, &message.thread_id, &candidates).await;
        processed += 1;
    }

    Ok(processed)
}

pub(crate) async fn sync_candidates_to_letta(
    letta: Option<&LettaMemoryAdapter>,
    org_id: &str,
    thread_id: &str,
    candidates: &[DreamMemoryCandidate],
) {
    let Some(letta) = letta else {
        return;
    };

    for candidate in candidates {
        letta
            .index(
                org_id,
                thread_id,
                memory_topic(candidate.scope, candidate.kind),
                &candidate.content,
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

    let source_links = vec![
        format!("thread:{thread_id}"),
        format!("message:{message_id}"),
    ];
    let mut saved = 0i64;

    for candidate in candidates {
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
                OR (session_id IS NULL AND scope = 'user' AND (owner = $3 OR owner = '')) \
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

pub(crate) async fn search_agent_memory(
    pool: &PgPool,
    org_id: &str,
    thread_id: &str,
    query: &str,
    topic_filter: &[String],
    limit: u32,
    updated_after: Option<DateTime<Utc>>,
) -> Result<Vec<MemorySearchRow>, sqlx::Error> {
    let limit = i64::from(limit.clamp(1, 50));
    let filters = normalize_topic_filters(topic_filter);
    let query = query.trim();

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
           AND (session_id = $2 OR session_id IS NULL) \
           AND ($3::timestamptz IS NULL OR updated_at > $3) \
           AND (cardinality($4::text[]) = 0 \
                OR CASE \
                    WHEN scope = 'user' THEN 'USER' \
                    WHEN scope = 'agent' THEN 'AGENT' \
                    WHEN scope = 'workspace' THEN 'WORKSPACE' \
                    WHEN kind = 'policy' THEN 'POLICY' \
                    ELSE 'MEMORY' \
                  END = ANY($4::text[])) \
         ORDER BY \
           CASE \
             WHEN $5 = '' THEN 0 \
             WHEN lower(content) LIKE ('%' || lower($5) || '%') THEN 0 \
             WHEN lower(key) LIKE ('%' || lower($5) || '%') THEN 1 \
             ELSE 2 \
           END, \
           confidence DESC, \
           updated_at DESC \
         LIMIT $6",
    )
    .bind(org_id)
    .bind(thread_id)
    .bind(updated_after)
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
                        thread_id.to_owned()
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
    topic: &str,
    content: &str,
) -> Result<String, sqlx::Error> {
    let topic = normalize_topic(topic);
    let Some((_, user_id)) = sqlx::query_as::<_, (String, String)>(
        "SELECT org_id, user_id FROM threads WHERE id = $1 AND org_id = $2",
    )
    .bind(thread_id)
    .bind(org_id)
    .fetch_optional(pool)
    .await?
    else {
        return Ok(String::new());
    };

    let (scope, session_id, kind, owner) = match topic.as_str() {
        "USER" => ("user", None, "fact", user_id.as_str()),
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
         ON CONFLICT (org_id, scope, key) WHERE session_id IS NULL \
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
}
