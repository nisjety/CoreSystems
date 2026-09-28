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
//!
//! # Write-time grounding
//!
//! Before a candidate whose `key` does not already exist is stored, an
//! optional grounding pass (behind `MEMORY_GROUNDING_ENABLED`; see
//! [`crate::memory_grounding`]) checks whether it duplicates, extends or
//! corrects an existing same-scope memory and stores it accordingly, without
//! ever overwriting or deleting the row it acts on. See
//! [`ground_candidates`]/[`apply_grounded_candidates`] and
//! `migrations/0037_agent_memory_lineage.sql`. With the flag off, `dream_once`
//! takes the exact code path it always has — [`persist_candidates`] and
//! [`upsert_agent_memory`] are untouched by grounding's existence.

use crate::dream_extractor::{DreamExtractor, ExistingSlots, WindowMessage, LLM_SOURCE_LINK};
use crate::letta_adapter::LettaMemoryAdapter;
use crate::memory_grounding::{ExistingMatch, GroundingAction, GroundingClassifier};
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
    /// at "what do you remember about me" can tell what they asked Verevon to
    /// remember from what it decided to remember, and delete the latter.
    pub inferred: bool,
}

/// A durable memory identifier paired with the input candidate that produced
/// it. Only thread-scoped entries are eligible for semantic mirroring: their
/// `session_id` is the exact thread boundary required for later Space erasure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PersistedMemoryId {
    pub(crate) candidate_index: usize,
    pub(crate) memory_id: String,
}

/// How a memory came to exist.
///
/// Three-valued on purpose. A bool would force pre-provenance rows into one of
/// the two real answers, and the wrong one is worse than none: presenting an
/// unknown row as "you told me this" is exactly the claim a user would rely on
/// when deciding whether to keep it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MemoryProvenance {
    /// Written before provenance was recorded. Render as neither.
    Unknown,
    /// The user's own words — an explicit `save_memory` tool call, or a phrase
    /// the deterministic matcher recognised ("husk at ...").
    Stated,
    /// A model's reading of a conversation.
    Inferred,
}

impl MemoryProvenance {
    /// Classify a stored row.
    ///
    /// `source_links` is authoritative and needs no migration: the column is
    /// `TEXT[] NOT NULL DEFAULT '{}'`, so it is never NULL, only empty. Deriving
    /// from the `key` namespace instead would be one fewer column to select and
    /// one more thing to break the day a namespace is renamed.
    pub(crate) fn classify(source_links: &[String]) -> Self {
        if source_links.iter().any(|link| link == LLM_SOURCE_LINK) {
            return Self::Inferred;
        }
        // `tool:save_memory` is a user asking, in as many words, to store this.
        if source_links.iter().any(|link| link == "tool:save_memory") {
            return Self::Stated;
        }
        // Everything the phrase matcher wrote carries only thread/message links.
        // Those rows ARE stated, but so is anything older, and we cannot tell
        // them apart — so they stay Unknown rather than being claimed as either.
        Self::Unknown
    }
}

#[derive(Debug)]
pub(crate) struct MemorySearchRow {
    pub id: String,
    pub thread_id: String,
    pub topic: String,
    pub content: String,
    pub score: f32,
    pub updated_at: DateTime<Utc>,
    pub provenance: MemoryProvenance,
    /// True when `query` literally appears in this row's content or key.
    ///
    /// pgstore's candidate pool is ordered by `confidence` — a self-reported
    /// certainty about the FACT, unrelated to whether the fact answers THIS
    /// query — so most of the pool is not a relevance signal at all, only
    /// background noise a caller should treat as a last resort. This flag is
    /// what lets a caller (`memory_grpc::merge_memory_search_results`) tell
    /// the two apart: an exact-match row genuinely IS relevance-ranked
    /// (better match → better rank among exact matches) and can be fused
    /// with a semantic backend's results via RRF; a non-match row cannot.
    pub exact_match: bool,
}

struct PendingMessage {
    message_id: String,
    thread_id: String,
    role: String,
    content: String,
    org_id: String,
    user_id: String,
    conversation_only: bool,
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
    let grounding = GroundingClassifier::from_env();
    let mut tick = tokio::time::interval(Duration::from_secs(interval_secs));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    info!(
        interval_secs,
        batch_size,
        llm_extraction = extractor.is_some(),
        memory_grounding = grounding.is_some(),
        "Dreaming Core loop started"
    );

    loop {
        tick.tick().await;
        let start = Instant::now();
        match dream_once(
            &pool,
            letta.as_ref(),
            extractor.as_ref(),
            grounding.as_ref(),
            batch_size,
        )
        .await
        {
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
    grounding: Option<&GroundingClassifier>,
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

        if last.conversation_only {
            let mut tx = pool.begin().await?;
            for message in &group {
                record_dream_run(&mut tx, &org_id, &thread_id, "skipped_conversation_scope", 0, 0, Some(&message.message_id)).await?;
                processed += 1;
            }
            tx.commit().await?;
            info!(%thread_id, messages = group.len(), "memory extraction skipped: conversation-only source scope");
            continue;
        }

        // Attached to the newest message of the group: that is the turn whose
        // arrival justified re-reading the window, and source_links should point
        // at it rather than at whichever message happened to sort first.
        let extracted = match extractor {
            Some(extractor) => {
                let window = extraction_window(pool, &thread_id, &group).await;
                let existing = if window.is_empty() {
                    ExistingSlots::default()
                } else {
                    existing_slots(pool, &org_id, &last.user_id, &thread_id).await
                };
                extractor.extract(&org_id, &thread_id, &existing, &window).await
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

            // Grounding's lookups (a bounded read plus, when configured, a
            // semantic search and an LLM classification) are network calls and
            // must never run with a Postgres transaction held open across
            // them -- so, when enabled, they happen here, before `pool.begin()`,
            // and only the resulting decision is applied inside the
            // transaction below. With the flag off this whole branch does not
            // exist at runtime: the `else` arm is `persist_candidates`
            // unchanged, exactly as it was before grounding existed.
            let mut retired_ids = Vec::new();
            let (_saved, persisted) = if let Some(classifier) = grounding {
                let actions = ground_candidates(
                    pool,
                    letta,
                    classifier,
                    &message.org_id,
                    &message.user_id,
                    &message.thread_id,
                    &candidates,
                )
                .await;
                let mut tx = pool.begin().await?;
                let result = apply_grounded_candidates(
                    &mut tx,
                    &message.org_id,
                    &message.user_id,
                    &message.thread_id,
                    &message.message_id,
                    &candidates,
                    &actions,
                    &mut retired_ids,
                )
                .await?;
                record_dream_run(
                    &mut tx,
                    &message.org_id,
                    &message.thread_id,
                    "background_scan",
                    i64::try_from(candidates.len()).unwrap_or(i64::MAX),
                    result.0,
                    Some(&message.message_id),
                )
                .await?;
                tx.commit().await?;
                result
            } else {
                let mut tx = pool.begin().await?;
                let result = persist_candidates(
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
                    result.0,
                    Some(&message.message_id),
                )
                .await?;
                tx.commit().await?;
                result
            };
            if grounding.is_some() {
                sync_persisted_candidates_to_letta_for_grounding(
                    pool,
                    letta,
                    &message.org_id,
                    &message.user_id,
                    &message.thread_id,
                    &candidates,
                    &persisted,
                )
                .await;
                // After commit, never inside the transaction: this is a
                // network call. Reads already hide these rows' mirrors
                // (`memory_control::filter_forgotten`); this removes them.
                if let Some(letta) = letta {
                    crate::memory_control::erase_retired_mirrors(
                        letta,
                        &message.org_id,
                        &message.user_id,
                        &retired_ids,
                    )
                    .await;
                }
            } else {
                sync_persisted_candidates_to_letta(
                    pool,
                    letta,
                    &message.org_id,
                    &message.user_id,
                    &message.thread_id,
                    &candidates,
                    &persisted,
                )
                .await;
            }
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
    // A late assistant reply must not re-teach a fact after the user erased
    // its originating statement. An extraction needs eligible user evidence.
    if !window.iter().any(|message| message.role == "user") {
        window.clear();
    }
    window
}

/// How many stored keys are read to find this user's existing slot names.
/// [`ExistingSlots::from_keys`] keeps a smaller, fixed number of them.
const EXISTING_SLOT_KEYS_READ: i64 = 200;

/// The slot names this user (and this thread) already has current memories
/// under, so the extractor can reuse one instead of minting a near-duplicate
/// key. Keys only, never content, newest first before
/// [`ExistingSlots::from_keys`] bounds and sorts them.
///
/// A read failure costs the extractor the hint, not the extraction.
async fn existing_slots(
    pool: &PgPool,
    org_id: &str,
    user_id: &str,
    thread_id: &str,
) -> ExistingSlots {
    let keys = sqlx::query_scalar::<_, String>(
        "SELECT key FROM agent_memory \
         WHERE org_id = $1 \
           AND review_state = 'accepted' \
           AND is_latest = true \
           AND (expires_at IS NULL OR expires_at > now()) \
           AND ((session_id IS NULL AND scope = 'user' AND owner = $2 AND key LIKE 'user:llm:%') \
                OR (session_id = $3 AND scope = 'thread' AND key LIKE 'thread:llm:%')) \
         ORDER BY updated_at DESC \
         LIMIT $4",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(thread_id)
    .bind(EXISTING_SLOT_KEYS_READ)
    .fetch_all(pool)
    .await;
    match keys {
        Ok(keys) => ExistingSlots::from_keys(&keys),
        Err(error) => {
            warn!(%error, %thread_id, "dreaming could not read existing memory slots");
            ExistingSlots::default()
        }
    }
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

pub(crate) async fn sync_persisted_candidates_to_letta(
    pool: &PgPool,
    letta: Option<&LettaMemoryAdapter>,
    org_id: &str,
    user_id: &str,
    thread_id: &str,
    candidates: &[DreamMemoryCandidate],
    persisted: &[PersistedMemoryId],
) {
    let Some(letta) = letta else {
        return;
    };

    for (candidate, memory_id) in correlated_thread_candidates(candidates, persisted) {
        letta
            .index(
                org_id,
                thread_id,
                memory_topic(candidate.scope, candidate.kind),
                &candidate.content,
                Some(user_id),
                Some(memory_id),
            )
            .await;
        crate::memory_control::erase_late_mirror(pool, letta, org_id, user_id, memory_id).await;
    }
}

fn correlated_thread_candidates<'a>(
    candidates: &'a [DreamMemoryCandidate],
    persisted: &'a [PersistedMemoryId],
) -> Vec<(&'a DreamMemoryCandidate, &'a str)> {
    persisted
        .iter()
        .filter_map(|entry| {
            let candidate = candidates.get(entry.candidate_index)?;
            (candidate.scope == "thread").then_some((candidate, entry.memory_id.as_str()))
        })
        .collect()
}

/// Same mirroring `sync_persisted_candidates_to_letta` does for thread-scoped
/// candidates, widened to `user`-scoped ones too.
///
/// The investigation behind this module found that `scope = "user"` Dreaming
/// candidates are never indexed into Letta by the plain sync above, so
/// grounding's semantic tier (`ground_one_candidate`'s call to
/// `LettaMemoryAdapter::list_detailed` for user-scope candidates) would query
/// an index that never has anything in it — an empty result that looks
/// exactly like "no duplicate exists" while actually meaning "never asked."
/// This is that prerequisite fix, kept as a SEPARATE function rather than a
/// widening of the original so the flag-off path's Letta traffic is
/// byte-for-byte unchanged: this is only ever called from the
/// grounding-enabled branch of `dream_once`.
pub(crate) async fn sync_persisted_candidates_to_letta_for_grounding(
    pool: &PgPool,
    letta: Option<&LettaMemoryAdapter>,
    org_id: &str,
    user_id: &str,
    thread_id: &str,
    candidates: &[DreamMemoryCandidate],
    persisted: &[PersistedMemoryId],
) {
    let Some(letta) = letta else {
        return;
    };

    for entry in persisted {
        let Some(candidate) = candidates.get(entry.candidate_index) else {
            continue;
        };
        let owner = (candidate.scope == "user").then_some(user_id);
        letta
            .index(
                org_id,
                thread_id,
                memory_topic(candidate.scope, candidate.kind),
                &candidate.content,
                owner,
                Some(entry.memory_id.as_str()),
            )
            .await;
        crate::memory_control::erase_late_mirror(pool, letta, org_id, user_id, &entry.memory_id).await;
    }
}

/// How many existing same-scope memories grounding compares a candidate
/// against before falling back to a semantic search. Same latest-first
/// ordering as `load_agent_memory_context_rows`, and a small, flat LIMIT
/// rather than `search_agent_memory`'s wide 200-row relevance pool: this is a
/// pre-write dedup check on a background loop, not a query answering a user's
/// question, and it runs once per ungrounded candidate.
const GROUNDING_CANDIDATE_POOL: i64 = 20;

struct GroundingPoolRow {
    id: String,
    key: String,
    content: String,
}

/// Existing same-scope memories to compare a new candidate against, in the
/// same partition the unique index already enforces --
/// `agent_memory_org_session_scope_key_uq` `(org_id, session_id, scope, key)`
/// for thread-scoped candidates, `agent_memory_org_scope_key_uq`
/// `(org_id, scope, owner, key)` for the rest -- just without requiring `key`
/// to match. Filtered to `is_latest = true`: a row already superseded is not
/// something grounding should compare against or supersede a second time.
async fn grounding_candidate_pool(
    pool: &PgPool,
    org_id: &str,
    owner: &str,
    candidate: &DreamMemoryCandidate,
) -> Result<Vec<GroundingPoolRow>, sqlx::Error> {
    let rows = if let Some(session_id) = candidate.session_id.as_deref() {
        sqlx::query_as::<_, (String, String, String)>(
            "SELECT id, key, content FROM agent_memory \
             WHERE org_id = $1 AND session_id = $2 AND scope = $3 \
               AND is_latest = true AND review_state = 'accepted' \
             ORDER BY updated_at DESC LIMIT $4",
        )
        .bind(org_id)
        .bind(session_id)
        .bind(candidate.scope)
        .bind(GROUNDING_CANDIDATE_POOL)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, (String, String, String)>(
            "SELECT id, key, content FROM agent_memory \
             WHERE org_id = $1 AND session_id IS NULL AND scope = $2 AND owner = $3 \
               AND is_latest = true AND review_state = 'accepted' \
             ORDER BY updated_at DESC LIMIT $4",
        )
        .bind(org_id)
        .bind(candidate.scope)
        .bind(owner)
        .bind(GROUNDING_CANDIDATE_POOL)
        .fetch_all(pool)
        .await?
    };

    Ok(rows
        .into_iter()
        .map(|(id, key, content)| GroundingPoolRow { id, key, content })
        .collect())
}

/// Ground every candidate of one message against existing memory before any
/// of it is written. No transaction is open while this runs -- see the
/// module doc and the call site in `dream_once`.
pub(crate) async fn ground_candidates(
    pool: &PgPool,
    letta: Option<&LettaMemoryAdapter>,
    classifier: &GroundingClassifier,
    org_id: &str,
    user_id: &str,
    thread_id: &str,
    candidates: &[DreamMemoryCandidate],
) -> Vec<GroundingAction> {
    let mut actions = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        actions.push(
            ground_one_candidate(pool, letta, classifier, org_id, user_id, thread_id, candidate)
                .await,
        );
    }
    actions
}

async fn ground_one_candidate(
    pool: &PgPool,
    letta: Option<&LettaMemoryAdapter>,
    classifier: &GroundingClassifier,
    org_id: &str,
    user_id: &str,
    thread_id: &str,
    candidate: &DreamMemoryCandidate,
) -> GroundingAction {
    let owner = if candidate.scope == "user" { user_id } else { "" };

    let pool_rows = match grounding_candidate_pool(pool, org_id, owner, candidate).await {
        Ok(rows) => rows,
        Err(error) => {
            warn!(%error, "memory grounding candidate lookup failed; adding without grounding");
            return GroundingAction::Add;
        }
    };

    // Tier 1: an exact key match is the SAME identity the existing unique
    // index already dedups on. `upsert_agent_memory`'s own ON CONFLICT
    // handles it exactly as well as grounding could (content refresh,
    // `GREATEST(confidence)`), so grounding stands down and lets that path
    // run rather than asking a classifier a question that is already answered.
    if pool_rows.iter().any(|row| row.key == candidate.key) {
        return GroundingAction::Add;
    }

    // Tier 2: cheap, deterministic near-match on a normalized key fragment.
    let mut matches: Vec<ExistingMatch> = pool_rows
        .iter()
        .filter(|row| crate::memory_grounding::keys_loosely_match(&row.key, &candidate.key))
        .map(|row| ExistingMatch {
            id: row.id.clone(),
            content: row.content.clone(),
        })
        .collect();

    // Tier 3: semantic recall, when configured. A degraded, timed-out or
    // empty outcome is read as "unknown" here, never as "confirmed no
    // duplicate" -- it simply contributes no additional matches, and whatever
    // tier 2 already found still stands. `list_detailed` (never thread-scoped)
    // is used for `user`-scope candidates since the fact they might duplicate
    // could have been written from any thread; `search_detailed` (thread-
    // scoped) for `thread`-scope ones, matching `search_memory`'s own choice
    // of the two RPCs for the same distinction.
    if let Some(letta) = letta {
        let mut entries = if candidate.scope == "thread" {
            letta
                .search_detailed(org_id, thread_id, user_id, &candidate.content, &[], 5)
                .await
                .entries
        } else {
            letta.list_detailed(org_id, user_id, 5).await.entries
        };
        if crate::memory_control::filter_forgotten(pool, org_id, user_id, &mut entries).await.is_err() {
            entries.clear();
        }
        for entry in entries {
            if entry.memory_id.is_empty() || matches.iter().any(|m| m.id == entry.memory_id) {
                continue;
            }
            matches.push(ExistingMatch {
                id: entry.memory_id,
                content: entry.content,
            });
        }
    }

    matches.truncate(crate::memory_grounding::MAX_MATCHES);

    if matches.is_empty() {
        // Nothing looks related by any signal available -- genuinely new.
        return GroundingAction::Add;
    }

    classifier
        .classify(org_id, &candidate.content, candidate.scope, &matches)
        .await
}

pub(crate) fn extract_memory_candidates(
    role: &str,
    content: &str,
    thread_id: &str,
) -> Vec<DreamMemoryCandidate> {
    let content = if role.trim().eq_ignore_ascii_case("user") {
        crate::dream_extractor::strip_document_blocks(content)
    } else { content.to_owned() };
    let normalized = collapse_whitespace(&content);
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
) -> Result<(i64, Vec<PersistedMemoryId>), sqlx::Error> {
    if candidates.is_empty() || !crate::memory_control::permits_extraction(tx, org_id, user_id, message_id).await? {
        return Ok((0, Vec::new()));
    }

    let base_source_links = vec![
        format!("thread:{thread_id}"),
        format!("message:{message_id}"),
    ];
    let mut saved = 0i64;
    let mut persisted = Vec::with_capacity(candidates.len());

    for (candidate_index, candidate) in candidates.iter().enumerate() {
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
        if !memory_id.is_empty() {
            persisted.push(PersistedMemoryId {
                candidate_index,
                memory_id,
            });
        }
    }

    Ok((saved, persisted))
}

/// [`persist_candidates`]'s grounding-aware sibling: writes each candidate
/// according to a precomputed [`GroundingAction`] (from [`ground_candidates`],
/// run before this transaction was opened) instead of always calling
/// [`upsert_agent_memory`] directly. `persist_candidates` itself is untouched
/// -- this is a parallel path, not a modification of it, so the flag-off
/// behaviour it backs stays byte-for-byte identical to before this module
/// existed.
///
/// Every row an EXTEND/SUPERSEDE retires is pushed onto `retired_ids`, so the
/// caller can remove its semantic mirror once this transaction commits.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn apply_grounded_candidates(
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    user_id: &str,
    thread_id: &str,
    message_id: &str,
    candidates: &[DreamMemoryCandidate],
    actions: &[GroundingAction],
    retired_ids: &mut Vec<String>,
) -> Result<(i64, Vec<PersistedMemoryId>), sqlx::Error> {
    if candidates.is_empty() || !crate::memory_control::permits_extraction(tx, org_id, user_id, message_id).await? {
        return Ok((0, Vec::new()));
    }

    let base_source_links = vec![
        format!("thread:{thread_id}"),
        format!("message:{message_id}"),
    ];
    let mut saved = 0i64;
    let mut persisted = Vec::with_capacity(candidates.len());

    for (candidate_index, (candidate, action)) in candidates.iter().zip(actions.iter()).enumerate() {
        let mut source_links = if candidate.inferred {
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

        let memory_id = match action {
            GroundingAction::Noop => String::new(),
            GroundingAction::Add => {
                upsert_agent_memory(tx, org_id, owner, candidate, &source_links).await?
            }
            GroundingAction::Extend { target_id, content } => {
                source_links.push("lineage:extend".to_owned());
                let (memory_id, retired) =
                    write_lineage_memory(tx, org_id, owner, candidate, &source_links, target_id, content)
                        .await?;
                retired_ids.extend(retired);
                memory_id
            }
            GroundingAction::Supersede { target_id, content } => {
                source_links.push("lineage:supersede".to_owned());
                let (memory_id, retired) =
                    write_lineage_memory(tx, org_id, owner, candidate, &source_links, target_id, content)
                        .await?;
                retired_ids.extend(retired);
                memory_id
            }
        };
        saved += i64::from(!memory_id.is_empty());
        if !memory_id.is_empty() {
            persisted.push(PersistedMemoryId {
                candidate_index,
                memory_id,
            });
        }
    }

    Ok((saved, persisted))
}

/// Writes an EXTEND or SUPERSEDE decision: flips `target_id`'s `is_latest` to
/// `false`, then inserts a brand new row carrying `content`, `supersedes_id =
/// target_id` and `is_latest = true` -- in this same transaction, so a reader
/// can never observe both states or neither. Never deletes or overwrites
/// `target_id`'s row.
///
/// If `target_id` no longer has `is_latest = true` (raced by a concurrent
/// writer, already superseded by an earlier candidate this same cycle, or a
/// stale classifier answer for a row deleted since) this falls back to an
/// ordinary [`upsert_agent_memory`] ADD rather than creating a lineage row
/// that points at a fact no longer treated as current -- never guessing at a
/// lineage that might not hold any more.
///
/// The insert itself is an upsert with the same `ON CONFLICT` targets
/// [`upsert_agent_memory`] uses, not a plain `INSERT`: `candidate.key` is
/// guaranteed different from `target_id`'s own key (an exact match would have
/// taken the tier-1 fast path in `ground_one_candidate` and never reached
/// here), but it could rarely still collide with some OTHER unrelated
/// existing row's key, and merging into that row is a far safer outcome than
/// a hard constraint-violation error aborting the whole Dreaming cycle for
/// this message.
///
/// The key is resolved BEFORE `target_id` is flipped, because the upsert
/// below would otherwise take over whatever row already holds
/// `candidate.key`:
///
///   * a RETIRED row (`is_latest = false`): reviving it with
///     `supersedes_id = target_id` resurrects a fact something already
///     replaced, and when that row is `target_id`'s own ancestor it closes a
///     `target -> retired -> target` cycle. The write goes through
///     [`upsert_agent_memory`] instead, which redirects it to the retired
///     row's lineage head, and `target_id` is left alone;
///   * a current row that `target_id`'s chain already reaches (including
///     `target_id` itself): pointing it at `target_id` would close a cycle,
///     so the row's content is refreshed in place and nothing is flipped.
///
/// Returns the written row's id and, when `target_id` was actually retired,
/// that id -- its semantic mirror is the caller's to remove after commit.
async fn write_lineage_memory(
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    owner: &str,
    candidate: &DreamMemoryCandidate,
    source_links: &[String],
    target_id: &str,
    content: &str,
) -> Result<(String, Option<String>), sqlx::Error> {
    let mut resolved = candidate.clone();
    resolved.content = content.to_owned();

    if let Some((occupant_id, occupant_is_latest)) =
        key_occupant(tx, org_id, owner, candidate).await?
    {
        if !occupant_is_latest {
            info!(
                target_memory_id = %target_id,
                retired_memory_id = %occupant_id,
                "lineage write landed on a retired key; writing through the key's lineage instead"
            );
            let memory_id = upsert_agent_memory(tx, org_id, owner, &resolved, source_links).await?;
            return Ok((memory_id, None));
        }
        if lineage_reaches(tx, org_id, target_id, &occupant_id).await? {
            warn!(
                target_memory_id = %target_id,
                occupant_memory_id = %occupant_id,
                "lineage write would close a supersedes cycle; refreshing the key's row instead"
            );
            let memory_id = upsert_agent_memory(tx, org_id, owner, &resolved, source_links).await?;
            return Ok((memory_id, None));
        }
    }

    let flipped = sqlx::query(
        "UPDATE agent_memory SET is_latest = false, updated_at = now() \
         WHERE id = $1 AND org_id = $2 AND is_latest = true",
    )
    .bind(target_id)
    .bind(org_id)
    .execute(&mut **tx)
    .await?;

    if flipped.rows_affected() == 0 {
        let memory_id = upsert_agent_memory(tx, org_id, owner, &resolved, source_links).await?;
        return Ok((memory_id, None));
    }

    if resolved.session_id.is_some() {
        let row: (String,) = sqlx::query_as(
            "INSERT INTO agent_memory \
             (id, org_id, session_id, scope, key, content, kind, confidence, owner, source_links, review_state, supersedes_id, is_latest) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'accepted', $11, true) \
             ON CONFLICT (org_id, session_id, scope, key) WHERE session_id IS NOT NULL \
             DO UPDATE SET \
               content = EXCLUDED.content, \
               kind = EXCLUDED.kind, \
               confidence = GREATEST(agent_memory.confidence, EXCLUDED.confidence), \
               owner = EXCLUDED.owner, \
               source_links = EXCLUDED.source_links, \
               review_state = 'accepted', \
               supersedes_id = EXCLUDED.supersedes_id, \
               is_latest = true, \
               updated_at = now() \
             RETURNING id",
        )
        .bind(new_ulid())
        .bind(org_id)
        .bind(resolved.session_id.as_deref())
        .bind(resolved.scope)
        .bind(&resolved.key)
        .bind(&resolved.content)
        .bind(resolved.kind)
        .bind(resolved.confidence)
        .bind(owner)
        .bind(source_links)
        .bind(target_id)
        .fetch_one(&mut **tx)
        .await?;
        return Ok((row.0, Some(target_id.to_owned())));
    }

    let row: (String,) = sqlx::query_as(
        "INSERT INTO agent_memory \
         (id, org_id, session_id, scope, key, content, kind, confidence, owner, source_links, review_state, supersedes_id, is_latest) \
         VALUES ($1, $2, NULL, $3, $4, $5, $6, $7, $8, $9, 'accepted', $10, true) \
         ON CONFLICT (org_id, scope, owner, key) WHERE session_id IS NULL \
         DO UPDATE SET \
           content = EXCLUDED.content, \
           kind = EXCLUDED.kind, \
           confidence = GREATEST(agent_memory.confidence, EXCLUDED.confidence), \
           owner = EXCLUDED.owner, \
           source_links = EXCLUDED.source_links, \
           review_state = 'accepted', \
           supersedes_id = EXCLUDED.supersedes_id, \
           is_latest = true, \
           updated_at = now() \
         RETURNING id",
    )
    .bind(new_ulid())
    .bind(org_id)
    .bind(resolved.scope)
    .bind(&resolved.key)
    .bind(&resolved.content)
    .bind(resolved.kind)
    .bind(resolved.confidence)
    .bind(owner)
    .bind(source_links)
    .bind(target_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok((row.0, Some(target_id.to_owned())))
}

/// The row that currently holds `candidate.key` in its unique-index
/// partition, locked for the rest of the transaction, with its `is_latest`.
async fn key_occupant(
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    owner: &str,
    candidate: &DreamMemoryCandidate,
) -> Result<Option<(String, bool)>, sqlx::Error> {
    if let Some(session_id) = candidate.session_id.as_deref() {
        return sqlx::query_as(
            "SELECT id, is_latest FROM agent_memory \
             WHERE org_id = $1 AND session_id = $2 AND scope = $3 AND key = $4 \
             FOR UPDATE",
        )
        .bind(org_id)
        .bind(session_id)
        .bind(candidate.scope)
        .bind(&candidate.key)
        .fetch_optional(&mut **tx)
        .await;
    }
    sqlx::query_as(
        "SELECT id, is_latest FROM agent_memory \
         WHERE org_id = $1 AND session_id IS NULL AND scope = $2 AND owner = $3 AND key = $4 \
         FOR UPDATE",
    )
    .bind(org_id)
    .bind(candidate.scope)
    .bind(owner)
    .bind(&candidate.key)
    .fetch_optional(&mut **tx)
    .await
}

/// Whether `needle` is `start` or one of its ancestors through
/// `supersedes_id`. Pointing `needle` at `start` would then close a cycle.
async fn lineage_reaches(
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    start: &str,
    needle: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "WITH RECURSIVE chain(id, supersedes_id) AS ( \
             SELECT id, supersedes_id FROM agent_memory WHERE id = $1 AND org_id = $2 \
           UNION ALL \
             SELECT m.id, m.supersedes_id FROM agent_memory m \
             JOIN chain c ON m.id = c.supersedes_id \
             WHERE m.org_id = $2 \
         ) CYCLE id SET is_cycle USING path \
         SELECT EXISTS (SELECT 1 FROM chain WHERE id = $3)",
    )
    .bind(start)
    .bind(org_id)
    .bind(needle)
    .fetch_one(&mut **tx)
    .await
}

/// The current head of the lineage `retired_id` belongs to: the newest
/// `is_latest` row reachable by following `supersedes_id` forwards (rows that
/// carry `retired_id` forward, then rows that carry those forward, ...),
/// restricted to the same unique-index partition and cycle-guarded.
async fn lineage_head(
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    owner: &str,
    candidate: &DreamMemoryCandidate,
    retired_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "WITH RECURSIVE descendants(id, depth) AS ( \
             SELECT id, 1 FROM agent_memory WHERE supersedes_id = $1 AND org_id = $2 \
           UNION ALL \
             SELECT m.id, d.depth + 1 FROM agent_memory m \
             JOIN descendants d ON m.supersedes_id = d.id \
             WHERE m.org_id = $2 \
         ) CYCLE id SET is_cycle USING path \
         SELECT m.id FROM descendants d JOIN agent_memory m ON m.id = d.id \
         WHERE NOT d.is_cycle AND m.id <> $1 AND m.is_latest \
           AND m.review_state = 'accepted' \
           AND m.scope = $3 \
           AND m.session_id IS NOT DISTINCT FROM $4 \
           AND (m.session_id IS NOT NULL OR m.owner = $5) \
         ORDER BY d.depth DESC, m.updated_at DESC, m.id DESC \
         LIMIT 1",
    )
    .bind(retired_id)
    .bind(org_id)
    .bind(candidate.scope)
    .bind(candidate.session_id.as_deref())
    .bind(owner)
    .fetch_optional(&mut **tx)
    .await
}

/// [`upsert_agent_memory`]'s answer when `candidate.key` belongs to a retired
/// row: refreshing that row in place would store the write where no reader
/// looks, so a correction would silently vanish.
///
/// When the retired row's lineage has a current head, the write lands on the
/// head instead -- the same content refresh an exact-key upsert performs,
/// applied to the row that now represents this slot. Nothing is flipped and
/// no `supersedes_id` changes, so this can never form a cycle or a second
/// current row. When the lineage has no head any more (its current row was
/// deleted, and the delete's `ON DELETE SET NULL` left this one orphaned)
/// there is nothing to compete with, and the retired row itself is refreshed
/// and made current again.
async fn write_on_retired_key(
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    owner: &str,
    candidate: &DreamMemoryCandidate,
    source_links: &[String],
) -> Result<String, sqlx::Error> {
    // The conflict arm locked this row, so it is still here and still
    // retired; the re-check only keeps a surprise from being written blind.
    let Some((retired_id, false)) = key_occupant(tx, org_id, owner, candidate).await? else {
        return upsert_current_key(tx, org_id, owner, candidate, source_links)
            .await?
            .ok_or(sqlx::Error::RowNotFound);
    };

    if let Some(head_id) = lineage_head(tx, org_id, owner, candidate, &retired_id).await? {
        let memory_id: String = sqlx::query_scalar(
            "UPDATE agent_memory SET \
               content = $2, \
               kind = $3, \
               confidence = GREATEST(confidence, $4), \
               source_links = $5, \
               review_state = 'accepted', \
               updated_at = now() \
             WHERE id = $1 \
             RETURNING id",
        )
        .bind(&head_id)
        .bind(&candidate.content)
        .bind(candidate.kind)
        .bind(candidate.confidence)
        .bind(source_links)
        .fetch_one(&mut **tx)
        .await?;
        counter!("mp_session_memory_retired_key_writes_total", "outcome" => "redirected").increment(1);
        info!(
            retired_memory_id = %retired_id,
            head_memory_id = %memory_id,
            "memory write on a retired key redirected to its lineage head"
        );
        return Ok(memory_id);
    }

    let memory_id: String = sqlx::query_scalar(
        "UPDATE agent_memory SET \
           content = $2, \
           kind = $3, \
           confidence = GREATEST(confidence, $4), \
           owner = $5, \
           source_links = $6, \
           review_state = 'accepted', \
           is_latest = true, \
           updated_at = now() \
         WHERE id = $1 \
         RETURNING id",
    )
    .bind(&retired_id)
    .bind(&candidate.content)
    .bind(candidate.kind)
    .bind(candidate.confidence)
    .bind(owner)
    .bind(source_links)
    .fetch_one(&mut **tx)
    .await?;
    counter!("mp_session_memory_retired_key_writes_total", "outcome" => "revived").increment(1);
    info!(
        retired_memory_id = %memory_id,
        "memory write on a retired key made it current again: its lineage has no current head"
    );
    Ok(memory_id)
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
    let rows = sqlx::query_as::<_, (String, String, String, String, String, String, bool)>(
        "SELECT m.id, m.thread_id, m.role, m.content, t.org_id, t.user_id, t.source_scope = 'conversation' \
         FROM messages m \
         JOIN threads t ON t.id = m.thread_id \
         WHERE m.role IN ('user', 'assistant') \
           AND NOT EXISTS (SELECT 1 FROM user_memory_controls c WHERE c.org_id = t.org_id AND c.user_id = t.user_id AND m.created_at <= c.extract_after) \
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
            |(message_id, thread_id, role, content, org_id, user_id, conversation_only)| PendingMessage {
                message_id,
                thread_id,
                role,
                content,
                org_id,
                user_id,
                conversation_only,
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
         JOIN threads t ON t.id = m.thread_id \
         WHERE m.thread_id = $1 \
           AND m.role IN ('user', 'assistant') \
           AND NOT EXISTS (SELECT 1 FROM user_memory_controls c WHERE c.org_id = t.org_id AND c.user_id = t.user_id AND m.created_at <= c.extract_after) \
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
           AND is_latest = true \
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

/// SQL candidate pool size, before relevance is scored and the result is
/// truncated back down to the caller's requested `limit`.
///
/// The `WHERE` clause below has no relevance filter at all — it returns up to
/// `LIMIT` rows regardless of match quality — so whatever ordering picks the
/// first `LIMIT` rows decides which candidates ever reach the exact-match
/// scoring below. At the old behaviour (candidate pool == requested limit,
/// typically 5-10), a relevant row sitting past the cutoff under
/// `confidence DESC, updated_at DESC` was silently excluded before its
/// content was ever compared against the query.
///
/// This is not a narrow edge case: live data shows most rows in a real org
/// share the same confidence value (the extractor defaults to 0.85 for
/// almost everything it writes — see the LEARNING_SYSTEM_REDESIGN doc's
/// duplicate-key finding), so a caller-scaled widening (e.g. `limit * 4`)
/// still leaves most of a tied block outside the window — an org of 24
/// memories can have 21 tied at the same confidence, and a relevant one can
/// sit at position 23. The candidate pool is therefore a flat, generous
/// constant rather than scaled to the caller's `limit`: exact-match
/// promotion needs to see the WHOLE realistic candidate space to work at
/// all, and 200 rows is a trivial, index-backed query regardless of a org's
/// history size — this bounds cost, it does not chase it down to the
/// caller's typically-small `limit`.
const MAX_CANDIDATE_POOL: i64 = 200;

pub(crate) async fn search_agent_memory(
    pool: &PgPool,
    request: &AgentMemorySearch<'_>,
) -> Result<Vec<MemorySearchRow>, sqlx::Error> {
    let limit = i64::from(request.limit.clamp(1, 50));
    let candidate_limit = MAX_CANDIDATE_POOL.max(limit);
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
            Vec<String>,
        ),
    >(
        "SELECT id, COALESCE(session_id, ''), scope, kind, key, content, confidence, updated_at, \
         source_links \
         FROM agent_memory \
         WHERE org_id = $1 \
           AND review_state = 'accepted' \
           AND is_latest = true \
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
         ORDER BY confidence DESC, updated_at DESC \
         LIMIT $6",
    )
    .bind(request.org_id)
    .bind(request.thread_id)
    .bind(request.owner_user_id)
    .bind(request.updated_after)
    .bind(&filters)
    .bind(candidate_limit)
    .fetch_all(pool)
    .await?;

    let mut scored: Vec<MemorySearchRow> = rows
        .into_iter()
        .map(
            |(id, session_id, scope, kind, key, content, confidence, updated_at, source_links)| {
                let exact_match = !query.is_empty()
                    && (content
                        .to_ascii_lowercase()
                        .contains(&query.to_ascii_lowercase())
                        || key
                            .to_ascii_lowercase()
                            .contains(&query.to_ascii_lowercase()));
                // A literal restatement of the query gets a real head start on
                // its RANK within this source's own list — which is what the
                // RRF merge (memory_grpc::merge_memory_search_results) actually
                // reads, not this raw score directly.
                let exact_bonus = if query.is_empty() || exact_match {
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
                    // Derived from source_links exactly like the list surface
                    // (`MemoryProvenance::classify` is the single authority).
                    // This used to hardcode Unknown — reasonable for the
                    // management surface it was written against, but SearchMemory
                    // is what feeds the CHAT recall notice, so the hardcoded
                    // Unknown made `stated`/`inferred` unreachable on the one
                    // surface built to display them: every recalled memory
                    // rendered as pre-provenance no matter how it was written.
                    provenance: MemoryProvenance::classify(&source_links),
                    content,
                    score: memory_score(confidence, exact_bonus),
                    updated_at,
                    exact_match,
                }
            },
        )
        .collect();

    // The SQL ORDER BY above is only a candidate-selection heuristic now (it
    // decides who is in the widened pool, not the final order) — the real
    // ranking happens here, where `score` already reflects the exact-match
    // bonus the SQL couldn't see until content left the database. Ties keep
    // the same confidence/recency tiebreak the old SQL ORDER BY used.
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.updated_at.cmp(&a.updated_at))
    });
    scored.truncate(limit as usize);
    Ok(scored)
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

/// Insert `candidate`, or refresh the row that already holds its key.
///
/// A key held by a retired row (`is_latest = false`) is not refreshed in
/// place: see [`write_on_retired_key`]. Until a lineage write or the dedup
/// backfill retires something, every key is current and this is the single
/// upsert statement it has always been.
async fn upsert_agent_memory(
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    owner: &str,
    candidate: &DreamMemoryCandidate,
    source_links: &[String],
) -> Result<String, sqlx::Error> {
    if let Some(memory_id) = upsert_current_key(tx, org_id, owner, candidate, source_links).await? {
        return Ok(memory_id);
    }
    write_on_retired_key(tx, org_id, owner, candidate, source_links).await
}

/// The upsert itself. The conflict arm only fires on a current row
/// (`WHERE agent_memory.is_latest`); on a retired one Postgres still locks the
/// row but updates nothing and returns no id, which is the caller's cue.
async fn upsert_current_key(
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    owner: &str,
    candidate: &DreamMemoryCandidate,
    source_links: &[String],
) -> Result<Option<String>, sqlx::Error> {
    if candidate.session_id.is_some() {
        return sqlx::query_scalar(
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
             WHERE agent_memory.is_latest \
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
        .fetch_optional(&mut **tx)
        .await;
    }

    sqlx::query_scalar(
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
         WHERE agent_memory.is_latest \
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
    .fetch_optional(&mut **tx)
    .await
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
    let rows = sqlx::query_as::<_, (String, String, String, f64, DateTime<Utc>, Vec<String>)>(
        "SELECT id, kind, content, confidence, updated_at, source_links \
         FROM agent_memory \
         WHERE org_id = $1 \
           AND owner = $2 \
           AND scope = 'user' \
           AND review_state = 'accepted' \
           AND is_latest = true \
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
        .map(
            |(id, kind, content, confidence, updated_at, source_links)| MemorySearchRow {
                id,
                thread_id: String::new(),
                provenance: MemoryProvenance::classify(&source_links),
                topic: memory_topic("user", &kind).to_owned(),
                content,
                score: memory_score(confidence, 0.0),
                updated_at,
                // No query on this listing surface — nothing to match
                // against, so nothing here is a relevance signal.
                exact_match: false,
            },
        )
        .collect())
}

/// What [`delete_user_memory`] found and did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MemoryDeletion {
    /// The row was deleted, together with every retired ancestor its
    /// `supersedes_id` chain reached, and each was recorded as forgotten.
    /// The requested id comes first.
    Deleted { ids: Vec<String> },
    /// No row with this id exists in the org. The id is still recorded as
    /// forgotten: the semantic tier can hold memories the durable index never
    /// had, and a late mirror of one must not come back.
    Absent,
    /// A row with this id exists in the org but is not the caller's -- another
    /// user's, or an org-wide memory. Nothing was changed or recorded.
    NotOwned,
}

impl MemoryDeletion {
    pub(crate) fn deleted(&self) -> bool {
        matches!(self, Self::Deleted { .. })
    }
}

/// Deletes one `agent_memory` row the caller owns, in either scope a user
/// can own a memory in:
///
///   * `scope = 'user'` rows (no session), owned when `owner` is the caller;
///   * thread-bound rows (`session_id` set). These are written with
///     `owner = ''`, so ownership is the thread's: the row belongs to the
///     caller when `threads.user_id` of its `session_id` is the caller, in the
///     same org -- the same boundary `SearchMemory` recalls them under.
///
/// This used to match `scope = 'user'` only, so a thread memory the user
/// deleted survived here and kept being injected into that thread.
///
/// The row's retired ancestors (reachable through `supersedes_id`,
/// `is_latest = false`, owned by the same caller) go with it: forgetting the
/// one visible version of a fact must not leave hidden copies behind that a
/// later write or a rollback could bring back. The walk is cycle-guarded.
///
/// Ownership is part of the `WHERE` clause itself, not checked after the
/// fact, so a caller can never delete another user's or another org's memory
/// by guessing an id.
pub(crate) async fn delete_user_memory(
    pool: &PgPool,
    org_id: &str,
    owner_user_id: &str,
    memory_id: &str,
) -> Result<MemoryDeletion, sqlx::Error> {
    let mut tx = pool.begin().await?;
    crate::memory_control::record_forgetting(&mut tx, org_id, owner_user_id, memory_id).await?;
    let deleted: Vec<String> = sqlx::query_scalar(
        "WITH RECURSIVE lineage(id, supersedes_id) AS ( \
             SELECT m.id, m.supersedes_id FROM agent_memory m \
             WHERE m.id = $1 AND m.org_id = $2 \
               AND ((m.session_id IS NULL AND m.scope = 'user' AND m.owner = $3) \
                    OR (m.session_id IS NOT NULL AND EXISTS ( \
                          SELECT 1 FROM threads t \
                          WHERE t.id = m.session_id AND t.org_id = m.org_id AND t.user_id = $3))) \
           UNION ALL \
             SELECT a.id, a.supersedes_id FROM agent_memory a \
             JOIN lineage l ON a.id = l.supersedes_id \
             WHERE a.org_id = $2 AND a.is_latest = false \
               AND ((a.session_id IS NULL AND a.scope = 'user' AND a.owner = $3) \
                    OR (a.session_id IS NOT NULL AND EXISTS ( \
                          SELECT 1 FROM threads t \
                          WHERE t.id = a.session_id AND t.org_id = a.org_id AND t.user_id = $3))) \
         ) CYCLE id SET is_cycle USING path \
         DELETE FROM agent_memory \
         WHERE id IN (SELECT id FROM lineage WHERE NOT is_cycle) \
         RETURNING id",
    )
    .bind(memory_id)
    .bind(org_id)
    .bind(owner_user_id)
    .fetch_all(&mut *tx)
    .await?;

    if deleted.iter().any(|id| id == memory_id) {
        let mut ids = vec![memory_id.to_owned()];
        let mut ancestors: Vec<String> = deleted.into_iter().filter(|id| id != memory_id).collect();
        ancestors.sort();
        for ancestor in &ancestors {
            crate::memory_control::record_forgetting(&mut tx, org_id, owner_user_id, ancestor).await?;
        }
        tx.commit().await?;
        ids.extend(ancestors);
        return Ok(MemoryDeletion::Deleted { ids });
    }

    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM agent_memory WHERE id = $1 AND org_id = $2)",
    )
    .bind(memory_id)
    .bind(org_id)
    .fetch_one(&mut *tx)
    .await?;
    if exists {
        tx.rollback().await?;
        return Ok(MemoryDeletion::NotOwned);
    }
    tx.commit().await?;
    Ok(MemoryDeletion::Absent)
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
                "User asked Verevon to remember: {}.",
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

    fn candidate(scope: &'static str) -> DreamMemoryCandidate {
        DreamMemoryCandidate {
            scope,
            session_id: (scope == "thread").then_some("thread-a".to_owned()),
            key: format!("key-{scope}"),
            content: "content".to_owned(),
            kind: "fact",
            confidence: 0.9,
            inferred: false,
        }
    }

    #[test]
    fn semantic_mirroring_keeps_only_exact_thread_memory_ids() {
        let candidates = vec![candidate("user"), candidate("thread"), candidate("org")];
        let persisted = vec![
            PersistedMemoryId {
                candidate_index: 0,
                memory_id: "user-memory".to_owned(),
            },
            PersistedMemoryId {
                candidate_index: 1,
                memory_id: "thread-memory".to_owned(),
            },
            PersistedMemoryId {
                candidate_index: 2,
                memory_id: "org-memory".to_owned(),
            },
            PersistedMemoryId {
                candidate_index: 99,
                memory_id: "forged-index".to_owned(),
            },
        ];
        let mirrored = correlated_thread_candidates(&candidates, &persisted);
        assert_eq!(mirrored.len(), 1);
        assert_eq!(mirrored[0].0.scope, "thread");
        assert_eq!(mirrored[0].1, "thread-memory");
    }

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
    fn quoted_attachment_preferences_do_not_become_user_memories() {
        let content = "Analyser vedlegget.\n--- VEDLEGG: profil.md ---\nJeg foretrekker oransje. Husk at kunden heter Fiktiv.\n--- SLUTT VEDLEGG ---";
        assert!(extract_memory_candidates("user", content, "thread-1").is_empty());
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
            conversation_only: false,
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

    // ---- write-time grounding ----

    /// With no classifier configured, `dream_once` must take exactly the
    /// pre-grounding code path: `persist_candidates` untouched, no grounding
    /// lookup, no new SQL run. This is the flag-off contract from the design
    /// doc ("the extractor must behave EXACTLY as it does today"), pinned as a
    /// structural fact about `persist_candidates`'s own body rather than by
    /// running the loop against a live database (nothing in this test suite
    /// does; see `search_agent_memory`'s own lack of a DB-backed test).
    #[test]
    fn persist_candidates_itself_has_no_grounding_dependency() {
        let source = include_str!("dreaming.rs");
        let start = source
            .find("pub(crate) async fn persist_candidates(")
            .expect("persist_candidates exists");
        let end = source[start..]
            .find("\n/// [`persist_candidates`]'s grounding-aware sibling")
            .map(|offset| start + offset)
            .expect("apply_grounded_candidates' doc comment follows persist_candidates");
        let body = &source[start..end];

        assert!(
            !body.contains("Grounding") && !body.contains("grounding"),
            "persist_candidates must not know grounding exists"
        );
        assert!(
            body.contains(
                "let memory_id = upsert_agent_memory(tx, org_id, owner, candidate, &source_links).await?;"
            ),
            "the flag-off write is a plain upsert, unconditionally"
        );
    }

    /// `dream_once` must only take the grounding path when a classifier is
    /// actually configured -- `Option::is_some()` is the single feature gate,
    /// exactly like `extractor: Option<&DreamExtractor>` already is.
    #[test]
    fn dream_once_only_grounds_when_a_classifier_is_configured() {
        let source = include_str!("dreaming.rs");
        assert!(source.contains("if let Some(classifier) = grounding {"));
    }

    /// EXTEND/SUPERSEDE must never delete or destructively overwrite the row
    /// they act on -- only flip `is_latest` and insert a new row referencing
    /// it in the same transaction.
    #[test]
    fn write_lineage_memory_flips_the_old_row_and_never_deletes_it() {
        let source = include_str!("dreaming.rs");
        let start = source
            .find("async fn write_lineage_memory(")
            .expect("write_lineage_memory exists");
        let end = source[start..]
            .find("pub(crate) async fn record_dream_run(")
            .map(|offset| start + offset)
            .expect("record_dream_run follows write_lineage_memory");
        let body = &source[start..end];

        assert!(
            body.contains("UPDATE agent_memory SET is_latest = false, updated_at = now()"),
            "the old row must be marked non-current, not removed"
        );
        assert!(
            !body.contains("DELETE FROM agent_memory"),
            "write_lineage_memory must never delete the row it acts on"
        );
        assert!(
            body.contains("INSERT INTO agent_memory"),
            "the resulting fact must be a NEW row"
        );
        assert!(
            body.contains("supersedes_id"),
            "the new row must record what it carries forward from"
        );
        assert!(
            body.contains("is_latest = true"),
            "the new row must be the current version"
        );
    }

    /// A hallucinated, stale or conversation-injected `target_id` must never
    /// reach the database layer as something to UPDATE -- validation lives in
    /// `memory_grounding::parse_classifier_response`, and this pins that
    /// `apply_grounded_candidates` has no path that bypasses it: every
    /// `Extend`/`Supersede` action is only ever constructed there.
    #[test]
    fn grounded_writes_only_ever_come_from_a_validated_classifier_action() {
        let source = include_str!("dreaming.rs");
        assert!(source.contains("GroundingAction::Extend { target_id, content } => {"));
        assert!(source.contains("GroundingAction::Supersede { target_id, content } => {"));
        assert!(source.contains("write_lineage_memory(tx, org_id, owner, candidate, &source_links, target_id, content)"));
    }

    /// The three read surfaces over `agent_memory` (search, the
    /// context-injection listing, and the "what do you remember about me"
    /// listing) must all stop surfacing a superseded row once corrected --
    /// the whole point of `is_latest`, per the design doc's read-side
    /// requirement. Pinned once per query so a future edit to any one of them
    /// cannot silently drop the filter.
    #[test]
    fn every_agent_memory_read_query_filters_to_the_latest_row() {
        let source = include_str!("dreaming.rs");
        for (function, next_function) in [
            (
                "pub(crate) async fn load_agent_memory_context_rows(",
                "pub(crate) struct AgentMemorySearch",
            ),
            (
                "pub(crate) async fn search_agent_memory(",
                "pub(crate) async fn index_agent_memory(",
            ),
            (
                "pub(crate) async fn list_user_memory(",
                "/// What [`delete_user_memory`] found and did.",
            ),
        ] {
            let start = source
                .find(function)
                .unwrap_or_else(|| panic!("{function} exists"));
            let end = source[start..]
                .find(next_function)
                .map(|offset| start + offset)
                .unwrap_or_else(|| panic!("{next_function} follows {function}"));
            let body = &source[start..end];
            assert!(
                body.contains("AND is_latest = true"),
                "{function} must filter to the latest row of each memory"
            );
        }
    }

    /// The migration is additive and non-destructive: both new columns are
    /// nullable/defaulted, so a deployment that never enables grounding never
    /// produces a row that differs from today's shape.
    #[test]
    fn the_lineage_migration_adds_exactly_the_documented_columns() {
        let migration = include_str!("../migrations/0037_agent_memory_lineage.sql");
        assert!(migration.contains(
            "ADD COLUMN IF NOT EXISTS supersedes_id TEXT REFERENCES agent_memory(id) ON DELETE SET NULL"
        ));
        assert!(migration.contains("ADD COLUMN IF NOT EXISTS is_latest BOOLEAN NOT NULL DEFAULT true"));
    }

    /// An exact `key` match is the same identity the unique index already
    /// dedups on, so grounding must stand down and let the ordinary upsert
    /// run -- never asking a classifier a question the schema already
    /// answers, and never risking a duplicate lineage row for a key that was
    /// never actually ambiguous.
    #[test]
    fn ground_one_candidate_treats_grounding_pool_rows_by_exact_key_as_the_fast_path() {
        let source = include_str!("dreaming.rs");
        let start = source
            .find("async fn ground_one_candidate(")
            .expect("ground_one_candidate exists");
        let end = source[start..]
            .find("pub(crate) fn extract_memory_candidates(")
            .map(|offset| start + offset)
            .expect("extract_memory_candidates follows ground_one_candidate");
        let body = &source[start..end];
        assert!(body.contains("if pool_rows.iter().any(|row| row.key == candidate.key) {"));
        assert!(body.contains("return GroundingAction::Add;"));
    }

    // ---- lineage and deletion, against a disposable database ----

    async fn disposable_pool() -> PgPool {
        let pool = PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    #[allow(clippy::too_many_arguments)]
    async fn seed_memory(
        pool: &PgPool,
        org: &str,
        session: Option<&str>,
        scope: &str,
        owner: &str,
        key: &str,
        is_latest: bool,
        supersedes: Option<&str>,
    ) -> String {
        let id = new_ulid();
        sqlx::query(
            "INSERT INTO agent_memory (id, org_id, session_id, scope, key, content, owner, is_latest, supersedes_id) \
             VALUES ($1, $2, $3, $4, $5, 'seeded', $6, $7, $8)",
        )
        .bind(&id)
        .bind(org)
        .bind(session)
        .bind(scope)
        .bind(key)
        .bind(owner)
        .bind(is_latest)
        .bind(supersedes)
        .execute(pool)
        .await
        .unwrap();
        id
    }

    async fn seed_user_memory(pool: &PgPool, org: &str, key: &str, is_latest: bool, supersedes: Option<&str>) -> String {
        seed_memory(pool, org, None, "user", "user-a", key, is_latest, supersedes).await
    }

    async fn seed_thread(pool: &PgPool, org: &str, user: &str) -> String {
        let thread = new_ulid();
        sqlx::query("INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)")
            .bind(&thread)
            .bind(org)
            .bind(user)
            .execute(pool)
            .await
            .unwrap();
        thread
    }

    /// `(content, is_latest, supersedes_id)`, or `None` once deleted.
    async fn memory_state(pool: &PgPool, id: &str) -> Option<(String, bool, Option<String>)> {
        sqlx::query_as("SELECT content, is_latest, supersedes_id FROM agent_memory WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .unwrap()
    }

    fn user_candidate(key: &str, content: &str) -> DreamMemoryCandidate {
        DreamMemoryCandidate {
            scope: "user",
            session_id: None,
            key: key.to_owned(),
            content: content.to_owned(),
            kind: "fact",
            confidence: 0.85,
            inferred: true,
        }
    }

    fn links() -> Vec<String> {
        vec!["thread:t".to_owned(), LLM_SOURCE_LINK.to_owned()]
    }

    /// The extractor re-using a slot the dedup backfill retired used to
    /// refresh the hidden row, so a real correction became invisible. It now
    /// lands on the row that represents the slot today.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn a_write_on_a_retired_key_lands_on_its_lineage_head() {
        let pool = disposable_pool().await;
        let org = format!("retired-key-{}", new_ulid());
        let retired = seed_user_memory(&pool, &org, "user:llm:tools", false, None).await;
        let middle = seed_user_memory(&pool, &org, "user:llm:recurring_tool", false, Some(&retired)).await;
        let head = seed_user_memory(&pool, &org, "user:llm:tool_use", true, Some(&middle)).await;

        let mut tx = pool.begin().await.unwrap();
        let written = upsert_agent_memory(
            &mut tx,
            &org,
            "user-a",
            &user_candidate("user:llm:tools", "User now uses Tripletex."),
            &links(),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        assert_eq!(written, head);
        assert_eq!(
            memory_state(&pool, &head).await,
            Some(("User now uses Tripletex.".to_owned(), true, Some(middle.clone())))
        );
        assert_eq!(
            memory_state(&pool, &retired).await,
            Some(("seeded".to_owned(), false, None)),
            "the retired row is neither refreshed nor revived"
        );
    }

    /// With no current head left (it was deleted and `ON DELETE SET NULL`
    /// orphaned the row), nothing competes with the retired row, and the new
    /// write must not vanish into it.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn a_write_on_an_orphaned_retired_key_makes_it_current_again() {
        let pool = disposable_pool().await;
        let org = format!("orphan-key-{}", new_ulid());
        let orphan = seed_user_memory(&pool, &org, "user:llm:tone", false, None).await;

        let mut tx = pool.begin().await.unwrap();
        let written = upsert_agent_memory(
            &mut tx,
            &org,
            "user-a",
            &user_candidate("user:llm:tone", "User prefers a warmer tone."),
            &links(),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        assert_eq!(written, orphan);
        assert_eq!(
            memory_state(&pool, &orphan).await,
            Some(("User prefers a warmer tone.".to_owned(), true, None))
        );
    }

    async fn has_cycle(pool: &PgPool, ids: &[&str]) -> bool {
        sqlx::query_scalar(
            "WITH RECURSIVE chain(id, supersedes_id) AS ( \
                 SELECT id, supersedes_id FROM agent_memory WHERE id = ANY($1) \
               UNION ALL \
                 SELECT m.id, m.supersedes_id FROM agent_memory m JOIN chain c ON m.id = c.supersedes_id \
             ) CYCLE id SET is_cycle USING path \
             SELECT EXISTS (SELECT 1 FROM chain WHERE is_cycle)",
        )
        .bind(ids)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    /// A SUPERSEDE whose candidate key is the target's own retired
    /// ancestor used to revive that ancestor with `supersedes_id = target`
    /// while retiring the target: `target -> ancestor -> target`. A target
    /// whose own key the candidate carries used to point at itself.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn a_lineage_write_never_closes_a_supersedes_cycle() {
        let pool = disposable_pool().await;
        let org = format!("lineage-cycle-{}", new_ulid());
        let ancestor = seed_user_memory(&pool, &org, "user:llm:tools", false, None).await;
        let target = seed_user_memory(&pool, &org, "user:llm:tool_use", true, Some(&ancestor)).await;

        for key in ["user:llm:tools", "user:llm:tool_use"] {
            let mut tx = pool.begin().await.unwrap();
            let (written, retired) = write_lineage_memory(
                &mut tx,
                &org,
                "user-a",
                &user_candidate(key, "unused candidate text"),
                &links(),
                &target,
                "User uses Visma.net daily.",
            )
            .await
            .unwrap();
            tx.commit().await.unwrap();

            assert_eq!(written, target, "{key}: the write lands on the current head");
            assert_eq!(retired, None, "{key}: nothing was retired");
            assert!(!has_cycle(&pool, &[&ancestor, &target]).await, "{key}");
            assert_eq!(
                memory_state(&pool, &target).await,
                Some(("User uses Visma.net daily.".to_owned(), true, Some(ancestor.clone())))
            );
            assert_eq!(memory_state(&pool, &ancestor).await, Some(("seeded".to_owned(), false, None)));
        }
    }

    /// The id a lineage write retires is handed back, so its semantic mirror
    /// can be removed after commit.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn a_lineage_write_reports_the_row_it_retired() {
        let pool = disposable_pool().await;
        let org = format!("lineage-retire-{}", new_ulid());
        let target = seed_user_memory(&pool, &org, "user:llm:employer", true, None).await;

        let mut tx = pool.begin().await.unwrap();
        let (written, retired) = write_lineage_memory(
            &mut tx,
            &org,
            "user-a",
            &user_candidate("user:llm:employer_name", "User works at Nordvik."),
            &links(),
            &target,
            "User works at Nordvik.",
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        assert_ne!(written, target);
        assert_eq!(retired.as_deref(), Some(target.as_str()));
        assert_eq!(memory_state(&pool, &target).await, Some(("seeded".to_owned(), false, None)));
        assert_eq!(
            memory_state(&pool, &written).await,
            Some(("User works at Nordvik.".to_owned(), true, Some(target.clone())))
        );
    }

    async fn is_forgotten(pool: &PgPool, org: &str, user: &str, id: &str) -> bool {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM forgotten_memory_ids WHERE org_id = $1 AND user_id = $2 AND memory_id = $3)",
        )
        .bind(org)
        .bind(user)
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn deleting_a_memory_takes_its_retired_ancestors_and_nothing_else() {
        let pool = disposable_pool().await;
        let org = format!("delete-lineage-{}", new_ulid());
        let oldest = seed_user_memory(&pool, &org, "user:llm:tools", false, None).await;
        let older = seed_user_memory(&pool, &org, "user:llm:recurring_tool", false, Some(&oldest)).await;
        let head = seed_user_memory(&pool, &org, "user:llm:tool_use", true, Some(&older)).await;
        let unrelated = seed_user_memory(&pool, &org, "user:llm:employer", true, None).await;

        let outcome = delete_user_memory(&pool, &org, "user-a", &head).await.unwrap();
        let mut expected = vec![head.clone()];
        let mut ancestors = vec![older.clone(), oldest.clone()];
        ancestors.sort();
        expected.extend(ancestors);
        assert_eq!(outcome, MemoryDeletion::Deleted { ids: expected });
        for id in [&head, &older, &oldest] {
            assert_eq!(memory_state(&pool, id).await, None);
            assert!(is_forgotten(&pool, &org, "user-a", id).await);
        }
        assert!(memory_state(&pool, &unrelated).await.is_some());

        // A cycle through retired rows cannot make the walk run forever (the
        // requested row is always included, current or not: a stale mirror
        // id can name a retired row).
        let loop_a = seed_user_memory(&pool, &org, "user:llm:loop_a", false, None).await;
        let loop_b = seed_user_memory(&pool, &org, "user:llm:loop_b", false, Some(&loop_a)).await;
        sqlx::query("UPDATE agent_memory SET supersedes_id = $2 WHERE id = $1")
            .bind(&loop_a)
            .bind(&loop_b)
            .execute(&pool)
            .await
            .unwrap();
        let outcome = delete_user_memory(&pool, &org, "user-a", &loop_a).await.unwrap();
        assert_eq!(outcome, MemoryDeletion::Deleted { ids: vec![loop_a.clone(), loop_b.clone()] });

        // An ancestor that is not the caller's is never deleted with theirs.
        let foreign = seed_memory(&pool, &org, None, "user", "user-b", "user:llm:foreign", false, None).await;
        let own = seed_user_memory(&pool, &org, "user:llm:own", true, Some(&foreign)).await;
        let outcome = delete_user_memory(&pool, &org, "user-a", &own).await.unwrap();
        assert_eq!(outcome, MemoryDeletion::Deleted { ids: vec![own] });
        assert!(memory_state(&pool, &foreign).await.is_some());
    }

    /// A thread memory is owned through its thread. It used to be skipped by
    /// the `scope = 'user'` delete and survive.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn a_thread_memory_is_deleted_by_the_threads_user_and_nobody_else() {
        let pool = disposable_pool().await;
        let org = format!("delete-thread-{}", new_ulid());
        let thread = seed_thread(&pool, &org, "user-a").await;
        let memory = seed_memory(&pool, &org, Some(&thread), "thread", "", "thread:llm:project", true, None).await;

        assert_eq!(
            delete_user_memory(&pool, &org, "user-b", &memory).await.unwrap(),
            MemoryDeletion::NotOwned
        );
        assert!(memory_state(&pool, &memory).await.is_some());
        assert!(!is_forgotten(&pool, &org, "user-b", &memory).await, "a refused delete records nothing");
        assert_eq!(
            delete_user_memory(&pool, "another-org", "user-a", &memory).await.unwrap(),
            MemoryDeletion::Absent,
            "another org cannot see the row at all"
        );

        assert!(delete_user_memory(&pool, &org, "user-a", &memory).await.unwrap().deleted());
        assert_eq!(memory_state(&pool, &memory).await, None);
        assert!(is_forgotten(&pool, &org, "user-a", &memory).await);

        let semantic_only = new_ulid();
        assert_eq!(
            delete_user_memory(&pool, &org, "user-a", &semantic_only).await.unwrap(),
            MemoryDeletion::Absent
        );
        assert!(
            is_forgotten(&pool, &org, "user-a", &semantic_only).await,
            "a semantic-only memory is still recorded as forgotten"
        );
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn the_extractor_sees_only_this_users_current_slot_names() {
        let pool = disposable_pool().await;
        let org = format!("slots-{}", new_ulid());
        let thread = seed_thread(&pool, &org, "user-a").await;
        let other_thread = seed_thread(&pool, &org, "user-a").await;
        seed_user_memory(&pool, &org, "user:llm:tool_use", true, None).await;
        seed_user_memory(&pool, &org, "user:llm:tools", false, None).await;
        seed_user_memory(&pool, &org, "user:name", true, None).await;
        seed_memory(&pool, &org, None, "user", "user-b", "user:llm:someone_else", true, None).await;
        seed_memory(&pool, &org, Some(&thread), "thread", "", "thread:llm:project_context", true, None).await;
        seed_memory(&pool, &org, Some(&other_thread), "thread", "", "thread:llm:other_thread", true, None).await;

        let slots = existing_slots(&pool, &org, "user-a", &thread).await;
        assert_eq!(slots.user, ["tool_use"]);
        assert_eq!(slots.thread, ["project_context"]);
    }
}
