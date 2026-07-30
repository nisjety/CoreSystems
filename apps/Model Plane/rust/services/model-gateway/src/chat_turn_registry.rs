//! Server-side record of what one chat turn actually was, keyed by the
//! client-visible `request_id` (`AGENT_QUALITY_PLAN_2026-07-29` §1.2).
//!
//! ## Why this exists
//!
//! A thumbs-up/down from chat arrives with only the SSE `request_id`. To make
//! that rating useful we need two things the browser must NOT be trusted to
//! assert:
//!
//!   1. the **durable run id** for the turn, so ownership can be verified
//!      against Session Core (`resolve_run_owner`) — the client never sees it,
//!      and a client-asserted run id would be a rating-forgery surface;
//!   2. the **skill ids that were injected** into that turn, so a thumbs-down
//!      can demote the skill that steered a bad answer. The client does not
//!      reliably know them, and letting it name them would let a caller point a
//!      negative rating at any skill in the org.
//!
//! So the gateway records both at injection time and resolves them here.
//!
//! ## Durability tradeoff (v1, deliberate)
//!
//! This is an in-process, TTL-bounded map. It is lost on restart/deploy and is
//! per-replica. Consequences, in order of severity:
//!
//!   - **Skill attribution degrades to run-only.** A rating whose turn is no
//!     longer in the map records the run's rating with no skill ids. That is a
//!     weaker signal, not an error — see `ResolvedFeedbackTarget`.
//!   - **A turn whose run id is unknown cannot be authorized.** We deliberately
//!     fail closed there rather than trust a client-supplied id: see
//!     `http_routes::resolve_feedback_target`.
//!
//! The fix that removes both holes is to persist `(request_id → run_id)` in
//! Session Core (or emit `run_id` on the SSE `connected` event, after which the
//! client can send `run_id` directly — it is safe for the client to *name* a run
//! because ownership is still verified server-side; it is not safe for the
//! client to name *skills*, because nothing verifies those).

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use dashmap::DashMap;

/// How long a finished turn stays rateable. Generous on purpose: a user may
/// scroll back and rate an answer well after it streamed.
const TURN_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Bounded so a busy tenant cannot grow the map without limit. Expired entries
/// are reclaimed first; a full map drops the *new* record (the turn then rates
/// run-only) rather than evicting a live one.
const MAX_ENTRIES: usize = 20_000;

/// Injection is capped at 3 today (`sse::MAX_INJECTED_SKILLS`); this is the
/// retention cap, not the injection cap.
const MAX_SKILL_IDS: usize = 8;

/// Public request ids are ULIDs. Refuse anything unreasonable before retaining.
const MAX_REQUEST_ID_BYTES: usize = 128;

/// What the gateway did for one chat turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatTurnRecord {
    /// Verified tenant that produced the turn (from token claims, never a header).
    pub org_id: String,
    /// Verified acting user that produced the turn.
    pub user_id: String,
    /// Durable Session Core run id for the turn — the authorization subject.
    pub run_id: String,
    /// Skill ids injected as system context into this turn, in injection order.
    pub skill_ids: Vec<String>,
}

struct Entry {
    record: ChatTurnRecord,
    at: Instant,
}

/// `request_id` → what that turn was. Cheap (`DashMap` + `Instant`), no new deps.
#[derive(Clone, Default)]
pub struct ChatTurnRegistry {
    inner: Arc<DashMap<String, Entry>>,
}

impl ChatTurnRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
        }
    }

    /// Record one turn. Silently no-ops on an unusable key so a recording
    /// failure can never fail a user's chat turn — the turn simply rates
    /// run-only later.
    pub fn record(
        &self,
        request_id: &str,
        org_id: &str,
        user_id: &str,
        run_id: &str,
        skill_ids: Vec<String>,
    ) {
        let request_id = request_id.trim();
        if request_id.is_empty()
            || request_id.len() > MAX_REQUEST_ID_BYTES
            || org_id.trim().is_empty()
            || user_id.trim().is_empty()
            || run_id.trim().is_empty()
        {
            return;
        }
        let now = Instant::now();
        self.purge_expired(now);
        // Fail closed on capacity: do not evict a live turn to admit a new one.
        if self.inner.len() >= MAX_ENTRIES && !self.inner.contains_key(request_id) {
            return;
        }
        let mut ids: Vec<String> = Vec::new();
        for id in skill_ids {
            let id = id.trim().to_owned();
            if id.is_empty() || ids.contains(&id) {
                continue;
            }
            ids.push(id);
            if ids.len() >= MAX_SKILL_IDS {
                break;
            }
        }
        self.inner.insert(
            request_id.to_owned(),
            Entry {
                record: ChatTurnRecord {
                    org_id: org_id.to_owned(),
                    user_id: user_id.to_owned(),
                    run_id: run_id.to_owned(),
                    skill_ids: ids,
                },
                at: now,
            },
        );
    }

    /// Resolve a turn. `None` when unknown or expired.
    #[must_use]
    pub fn lookup(&self, request_id: &str) -> Option<ChatTurnRecord> {
        let request_id = request_id.trim();
        if request_id.is_empty() {
            return None;
        }
        let now = Instant::now();
        let entry = self.inner.get(request_id)?;
        if now.duration_since(entry.at) >= TURN_TTL {
            return None;
        }
        Some(entry.record.clone())
    }

    /// Number of currently-retained turns (test/metrics aid).
    #[must_use]
    pub fn tracked(&self) -> usize {
        self.inner.len()
    }

    fn purge_expired(&self, now: Instant) {
        self.inner
            .retain(|_, entry| now.duration_since(entry.at) < TURN_TTL);
    }
}

/// Process-global registry.
///
/// Deliberately a global rather than an `AppState` field: the writer lives in
/// `sse.rs` (owned by another change) and the reader in `http_routes.rs`, so a
/// global keeps the required edit to `sse.rs` down to a single call with no
/// signature or state plumbing. Tests construct their own [`ChatTurnRegistry`].
pub fn global() -> &'static ChatTurnRegistry {
    static GLOBAL: OnceLock<ChatTurnRegistry> = OnceLock::new();
    GLOBAL.get_or_init(ChatTurnRegistry::new)
}

/// The hook `sse.rs` calls immediately after skill context is injected.
///
/// Re-runs the same deterministic, in-process matcher `fetch_skill_context`
/// just ran (same store, same query, same limit) and retains the ids of the
/// blocks that were actually injected. Advisory and infallible: a failure here
/// costs skill attribution on a later rating, never the turn.
pub fn record_chat_turn(
    state: &crate::state::AppState,
    request_id: &str,
    org_id: &str,
    user_id: &str,
    run_id: &str,
    turn_query: &str,
    injected_limit: i32,
) {
    global().record(
        request_id,
        org_id,
        user_id,
        run_id,
        injected_skill_ids(state, org_id, turn_query, injected_limit),
    );
}

/// Ids of the skills `fetch_skill_context` would inject for this turn. Mirrors
/// its filter (non-empty body) so the recorded set is exactly the injected set.
fn injected_skill_ids(
    state: &crate::state::AppState,
    org_id: &str,
    turn_query: &str,
    limit: i32,
) -> Vec<String> {
    use mp_contracts::model_plane::v1::MatchSkillsRequest;

    if org_id.trim().is_empty() {
        return Vec::new();
    }
    let Ok(matched) = crate::skills::handle_match_skills(
        &state.skills,
        MatchSkillsRequest {
            request_id: String::new(),
            org_id: org_id.to_owned(),
            query: turn_query.to_owned(),
            limit,
            min_score: 0.0,
        },
    ) else {
        return Vec::new();
    };
    matched
        .matches
        .into_iter()
        .filter_map(|m| m.skill)
        .filter(|s| !s.body.trim().is_empty() && !s.id.trim().is_empty())
        .map(|s| s.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(reg: &ChatTurnRegistry, request_id: &str, skills: &[&str]) {
        reg.record(
            request_id,
            "org-1",
            "user-1",
            "run-1",
            skills.iter().map(|s| (*s).to_owned()).collect(),
        );
    }

    #[test]
    fn records_and_resolves_a_turn() {
        let reg = ChatTurnRegistry::new();
        record(&reg, "req-1", &["skill.a", "skill.b"]);
        let found = reg.lookup("req-1").expect("turn is rateable");
        assert_eq!(found.run_id, "run-1");
        assert_eq!(found.org_id, "org-1");
        assert_eq!(found.user_id, "user-1");
        assert_eq!(found.skill_ids, vec!["skill.a", "skill.b"]);
    }

    #[test]
    fn unknown_turn_resolves_to_none_rather_than_a_guess() {
        let reg = ChatTurnRegistry::new();
        assert!(reg.lookup("never-seen").is_none());
        assert!(reg.lookup("").is_none());
    }

    #[test]
    fn incomplete_records_are_dropped_not_half_stored() {
        let reg = ChatTurnRegistry::new();
        reg.record("", "org", "user", "run", Vec::new());
        reg.record("req", "", "user", "run", Vec::new());
        reg.record("req", "org", "", "run", Vec::new());
        // No run id → the record could never authorize anything, so it is useless.
        reg.record("req", "org", "user", "", Vec::new());
        assert_eq!(reg.tracked(), 0);
    }

    #[test]
    fn skill_ids_are_deduped_blank_filtered_and_capped() {
        let reg = ChatTurnRegistry::new();
        let many: Vec<String> = (0..(MAX_SKILL_IDS + 4))
            .map(|i| format!("skill.{i}"))
            .collect();
        reg.record("req-many", "org", "user", "run", many);
        let found = reg.lookup("req-many").expect("recorded");
        assert_eq!(found.skill_ids.len(), MAX_SKILL_IDS);

        record(&reg, "req-dupe", &["skill.a", "  skill.a  ", "", "skill.b"]);
        let found = reg.lookup("req-dupe").expect("recorded");
        assert_eq!(found.skill_ids, vec!["skill.a", "skill.b"]);
    }

    #[test]
    fn re_recording_the_same_turn_replaces_rather_than_duplicates() {
        let reg = ChatTurnRegistry::new();
        record(&reg, "req-1", &["skill.a"]);
        record(&reg, "req-1", &["skill.b"]);
        assert_eq!(reg.tracked(), 1);
        assert_eq!(
            reg.lookup("req-1").expect("recorded").skill_ids,
            vec!["skill.b"]
        );
    }

    #[test]
    fn oversized_request_ids_are_refused_before_retention() {
        let reg = ChatTurnRegistry::new();
        let huge = "x".repeat(MAX_REQUEST_ID_BYTES + 1);
        reg.record(&huge, "org", "user", "run", Vec::new());
        assert_eq!(reg.tracked(), 0);
    }
}
