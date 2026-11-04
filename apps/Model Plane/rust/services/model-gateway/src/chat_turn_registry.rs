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
//!
//! ## Shared mirror (`MODEL_GATEWAY_SHARED_TURN_RECORD`, default off)
//!
//! With several gateway replicas, a rating can land on a replica that did not
//! stream the turn. When the switch is on, the turn's ids are also written,
//! best-effort, to the shared Redis the stream buffer already uses, and a rating
//! that misses locally reads them from there. The local map stays the first
//! lookup and the only record when the switch is off. The shared record is ids
//! only (`run_id`, skill ids), keyed by the verified identity, never holds a
//! bearer or any content, and is never written for a ZDR or waived turn. It
//! shares the stream buffer's breaker, so a slow or dead store costs a rating
//! its skill attribution (the behaviour above), never the turn or the rating.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::stream_buffer::{scoped_stream_key, StreamBufferStore};

/// Switch for the shared mirror. Parsed by [`shared_turn_record_from`].
pub const SHARED_TURN_RECORD_ENV: &str = "MODEL_GATEWAY_SHARED_TURN_RECORD";

/// Namespace of the shared record, beside the stream buffer's `mp:gw:stream:`.
const SHARED_TURN_PREFIX: &str = "mp:gw:turn:";

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

    /// When this replica does not know the turn, adopt the shared record of the
    /// verified caller's own turn so the sync [`Self::lookup`] path resolves it.
    /// The key is derived from the caller's identity, so a hit is the caller's by
    /// construction and another user or tenant misses. A miss, an unusable
    /// store, or an open breaker leaves the registry as it was.
    pub(crate) async fn adopt_shared(
        &self,
        store: &impl SharedTurnStore,
        org_id: &str,
        user_id: &str,
        request_id: &str,
    ) {
        if self.lookup(request_id).is_some() {
            return;
        }
        if let Some(turn) = lookup_shared_turn(store, org_id, user_id, request_id).await {
            self.record(
                request_id,
                &turn.org_id,
                &turn.user_id,
                &turn.run_id,
                turn.skill_ids,
            );
        }
    }
}

/// The shared mirror's switch: `true` in any case and nothing else, like the other
/// Model Plane switches, so a typo never turns it on.
#[must_use]
pub fn shared_turn_record_from(value: Option<&str>) -> bool {
    mp_contracts::loop_fold::switch_from(value)
}

#[must_use]
pub fn shared_turn_record_enabled() -> bool {
    shared_turn_record_from(std::env::var(SHARED_TURN_RECORD_ENV).ok().as_deref())
}

/// What the shared record holds: ids only. Identity lives in the key.
#[derive(Serialize, Deserialize)]
struct SharedTurn {
    run_id: String,
    #[serde(default)]
    skill_ids: Vec<String>,
}

/// The shared key of one turn: `mp:gw:turn:{org}\x1f{user}\x1f{request_id}`.
///
/// Identity-scoped for the reason the stream buffer's key is: the request id is
/// not a secret, so the verified caller's identity in the key is the access
/// control. `None` for a part that is blank or oversized, or that carries the
/// separator and could therefore be made to collide with another identity.
#[must_use]
pub fn shared_turn_key(org_id: &str, user_id: &str, request_id: &str) -> Option<String> {
    let (org, user, request) = (org_id.trim(), user_id.trim(), request_id.trim());
    let unusable = |part: &str| part.is_empty() || part.contains('\u{1f}');
    if unusable(org) || unusable(user) || unusable(request) || request.len() > MAX_REQUEST_ID_BYTES
    {
        return None;
    }
    Some(format!(
        "{SHARED_TURN_PREFIX}{}",
        scoped_stream_key(org, user, request)
    ))
}

/// Where the shared record lives. The gateway's stream-buffer store is the only
/// implementation; the trait exists so the mirror is tested against a fake.
pub(crate) trait SharedTurnStore {
    async fn put(&self, key: &str, value: &str, ttl: Duration);
    async fn get(&self, key: &str) -> Option<String>;
}

impl SharedTurnStore for StreamBufferStore {
    async fn put(&self, key: &str, value: &str, ttl: Duration) {
        self.kv_set(key, value, ttl.as_secs()).await;
    }

    async fn get(&self, key: &str) -> Option<String> {
        self.kv_get(key).await
    }
}

/// Write one turn's ids to the shared store. Best-effort and infallible: a write
/// that does not happen costs a later rating on another replica its skill
/// attribution, nothing more.
pub(crate) async fn mirror_turn(
    store: &impl SharedTurnStore,
    request_id: &str,
    turn: &ChatTurnRecord,
) {
    let Some(key) = shared_turn_key(&turn.org_id, &turn.user_id, request_id) else {
        return;
    };
    let Ok(value) = serde_json::to_string(&SharedTurn {
        run_id: turn.run_id.clone(),
        skill_ids: turn.skill_ids.clone(),
    }) else {
        return;
    };
    store.put(&key, &value, TURN_TTL).await;
}

/// Read a turn back for the verified caller. Anything unreadable is a miss.
pub(crate) async fn lookup_shared_turn(
    store: &impl SharedTurnStore,
    org_id: &str,
    user_id: &str,
    request_id: &str,
) -> Option<ChatTurnRecord> {
    let key = shared_turn_key(org_id, user_id, request_id)?;
    let shared: SharedTurn = serde_json::from_str(&store.get(&key).await?).ok()?;
    if shared.run_id.trim().is_empty() {
        return None;
    }
    Some(ChatTurnRecord {
        org_id: org_id.trim().to_owned(),
        user_id: user_id.trim().to_owned(),
        run_id: shared.run_id,
        skill_ids: shared.skill_ids,
    })
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
///
/// `pins` are the versions the turn injected (EP-11 S3, empty while the switch is
/// off). A pinned skill is retained as `id@version`, so a rating names the version
/// that steered the answer; every reader that needs the bare id takes it with
/// [`skill_id_of_ref`].
#[allow(clippy::too_many_arguments)] // one turn's ids: each argument is request context
pub fn record_chat_turn(
    state: &crate::state::AppState,
    request_id: &str,
    org_id: &str,
    user_id: &str,
    run_id: &str,
    turn_query: &str,
    requested_skill_ids: &[String],
    injected_limit: i32,
    pins: &[mp_contracts::model_plane::v1::SkillPin],
) {
    let ids = injected_skill_ids(
        state,
        org_id,
        user_id,
        turn_query,
        requested_skill_ids,
        injected_limit,
    );
    global().record(request_id, org_id, user_id, run_id, skill_refs(ids, pins));
    run_pins().record(org_id, run_id, pins);
}

/// The ids as references: `id@version` for a skill the turn pinned at a numbered
/// version, the bare id otherwise (a builtin, a row from before versions, or pins
/// off), so with the switch off the record holds exactly what it did before.
#[must_use]
pub fn skill_refs(ids: Vec<String>, pins: &[mp_contracts::model_plane::v1::SkillPin]) -> Vec<String> {
    ids.into_iter()
        .map(|id| {
            match pins
                .iter()
                .find(|pin| pin.skill_id == id && pin.version > 0)
            {
                Some(pin) => format!("{id}@{}", pin.version),
                None => id,
            }
        })
        .collect()
}

/// The skill id inside a reference [`skill_refs`] made. Only a numeric suffix is
/// a version, so an id that happens to hold `@` is left whole.
#[must_use]
pub fn skill_id_of_ref(reference: &str) -> &str {
    match reference.rsplit_once('@') {
        Some((id, version))
            if !id.is_empty()
                && !version.is_empty()
                && version.bytes().all(|b| b.is_ascii_digit()) =>
        {
            id
        }
        _ => reference,
    }
}

/// How long a run's pins stay readable for recovery inside the run. A run that
/// outlives it recovers from the current row, as with the switch off.
const RUN_PIN_TTL: Duration = Duration::from_secs(6 * 60 * 60);

/// The skill versions each live run injected (EP-11 S3), keyed by
/// `(org, run_id)`. In-process like the turn record: the durable copy is the
/// run record's `run_context.skills`, and recovery inside a run happens on the
/// replica streaming it. Ids, numbers and hashes only.
#[derive(Clone, Default)]
pub struct RunPinRegistry {
    inner: Arc<DashMap<String, (Vec<mp_contracts::model_plane::v1::SkillPin>, Instant)>>,
}

impl RunPinRegistry {
    fn key(org_id: &str, run_id: &str) -> Option<String> {
        let (org, run) = (org_id.trim(), run_id.trim());
        if org.is_empty() || run.is_empty() || org.contains('\u{1f}') {
            return None;
        }
        Some(format!("{org}\u{1f}{run}"))
    }

    /// Keep a run's pins. No pins (the switch off) records nothing; a full map
    /// drops the new run's pins rather than a live run's, like [`ChatTurnRegistry`].
    pub fn record(
        &self,
        org_id: &str,
        run_id: &str,
        pins: &[mp_contracts::model_plane::v1::SkillPin],
    ) {
        if pins.is_empty() {
            return;
        }
        let Some(key) = Self::key(org_id, run_id) else {
            return;
        };
        let now = Instant::now();
        self.inner
            .retain(|_, (_, at)| now.duration_since(*at) < RUN_PIN_TTL);
        if self.inner.len() >= MAX_ENTRIES && !self.inner.contains_key(&key) {
            return;
        }
        self.inner.insert(key, (pins.to_vec(), now));
    }

    /// The pin this run holds for one skill, if any.
    #[must_use]
    pub fn pin(
        &self,
        org_id: &str,
        run_id: &str,
        skill_id: &str,
    ) -> Option<mp_contracts::model_plane::v1::SkillPin> {
        let entry = self.inner.get(&Self::key(org_id, run_id)?)?;
        let (pins, at) = entry.value();
        if Instant::now().duration_since(*at) >= RUN_PIN_TTL {
            return None;
        }
        pins.iter().find(|pin| pin.skill_id == skill_id).cloned()
    }
}

/// Process-wide run pin registry.
pub fn run_pins() -> &'static RunPinRegistry {
    static GLOBAL: OnceLock<RunPinRegistry> = OnceLock::new();
    GLOBAL.get_or_init(RunPinRegistry::default)
}

/// The hook `sse.rs` calls right after [`record_chat_turn`]: mirror what that call
/// just recorded to the shared store, when the switch is on. Spawned, so the
/// turn never waits on the store. The caller does not call it for a waived turn
/// (a ZDR turn never gets here); an unrecorded turn mirrors nothing.
pub fn mirror_chat_turn(
    state: &crate::state::AppState,
    request_id: &str,
    org_id: &str,
    user_id: &str,
) {
    if !shared_turn_record_enabled() {
        return;
    }
    let Some(turn) = global()
        .lookup(request_id)
        .filter(|turn| turn.org_id == org_id && turn.user_id == user_id)
    else {
        return;
    };
    let store = state.stream_buffers.clone();
    let request_id = request_id.to_owned();
    tokio::spawn(async move { mirror_turn(&store, &request_id, &turn).await });
}

/// The previous turn in one conversation, for implicit-dissatisfaction detection.
///
/// Separate from [`ChatTurnRecord`] and keyed differently on purpose. That map is
/// `request_id → turn`, which answers "what was this turn?" for a rating that
/// names a turn. This one is `(org, user, thread) → last turn`, which answers
/// "what came just before?" — the question a detector asks, and one the request
/// id cannot answer because the client does not know the previous turn's id.
#[derive(Clone, Debug)]
pub struct PreviousTurn {
    /// What the user asked last time.
    pub message: String,
    /// The run that ANSWERED it. Signals attach here, not to the new turn: the
    /// user is unhappy with the previous answer, and pointing the evidence at the
    /// turn that expressed the complaint would blame the wrong run.
    pub run_id: String,
    /// The skills injected into that previous turn — the ones that actually
    /// steered the answer being complained about.
    pub skill_ids: Vec<String>,
    /// How long ago it was.
    pub elapsed: Duration,
    /// Whether that answer ran under zero retention, or under a waiver for one connection. The
    /// signal about that answer is not recorded: it attaches to that run, and a learning review of
    /// the run would read content the owner's rules keep off every processor but the user's own.
    pub private_run: bool,
}

struct PreviousEntry {
    message: String,
    run_id: String,
    skill_ids: Vec<String>,
    private_run: bool,
    at: Instant,
}

/// How long a previous turn is retained.
///
/// Much shorter than [`TURN_TTL`]: this only feeds a near-duplicate window
/// measured in minutes, so holding conversations for a day would be memory spent
/// on data no detector will look at.
const PREVIOUS_TTL: Duration = Duration::from_secs(30 * 60);

/// Longest user message retained for comparison. A re-ask is a question, not a
/// pasted document, and retaining unbounded turn text would make this map a
/// content store.
const MAX_PREVIOUS_MESSAGE_BYTES: usize = 4096;

/// `(org, user, thread)` → the previous turn.
#[derive(Clone, Default)]
pub struct PreviousTurnRegistry {
    inner: Arc<DashMap<String, PreviousEntry>>,
}

impl PreviousTurnRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
        }
    }

    /// Read the previous turn, then replace it with this one.
    ///
    /// One operation because every caller wants both and doing them separately
    /// invites a path that reads without advancing — which would compare every
    /// future turn against the same stale message forever.
    pub fn swap(
        &self,
        org_id: &str,
        user_id: &str,
        thread_id: &str,
        message: &str,
        run_id: &str,
        skill_ids: Vec<String>,
        private_run: bool,
    ) -> Option<PreviousTurn> {
        let key = Self::key(org_id, user_id, thread_id)?;
        let now = Instant::now();
        self.purge_expired(now);

        let previous = self.inner.get(&key).and_then(|entry| {
            let elapsed = now.saturating_duration_since(entry.at);
            (elapsed <= PREVIOUS_TTL).then(|| PreviousTurn {
                message: entry.message.clone(),
                run_id: entry.run_id.clone(),
                skill_ids: entry.skill_ids.clone(),
                elapsed,
                private_run: entry.private_run,
            })
        });

        // Fail closed on capacity exactly like `record`: never evict a live
        // conversation to admit a new one.
        if self.inner.len() >= MAX_ENTRIES && !self.inner.contains_key(&key) {
            return previous;
        }
        self.inner.insert(
            key,
            PreviousEntry {
                message: truncate_bytes(message, MAX_PREVIOUS_MESSAGE_BYTES),
                run_id: run_id.to_owned(),
                skill_ids,
                private_run,
                at: now,
            },
        );
        previous
    }

    /// A key only exists when all three parts do. A conversation without a
    /// thread has no "previous turn" to speak of.
    fn key(org_id: &str, user_id: &str, thread_id: &str) -> Option<String> {
        let (org, user, thread) = (org_id.trim(), user_id.trim(), thread_id.trim());
        if org.is_empty() || user.is_empty() || thread.is_empty() {
            return None;
        }
        Some(format!("{org}\u{1f}{user}\u{1f}{thread}"))
    }

    fn purge_expired(&self, now: Instant) {
        self.inner
            .retain(|_, entry| now.saturating_duration_since(entry.at) <= PREVIOUS_TTL);
    }

    #[must_use]
    pub fn tracked(&self) -> usize {
        self.inner.len()
    }
}

/// Process-wide previous-turn registry.
pub fn previous_turns() -> &'static PreviousTurnRegistry {
    static GLOBAL: OnceLock<PreviousTurnRegistry> = OnceLock::new();
    GLOBAL.get_or_init(PreviousTurnRegistry::new)
}

/// Truncate on a char boundary so a multi-byte rune is never split.
fn truncate_bytes(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_owned();
    }
    let mut end = max;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

/// Ids of the skills `fetch_skill_context` would inject for this turn. Mirrors
/// its filter (non-empty body) so the recorded set is exactly the injected set.
/// `requested_skill_ids` are the skills the member picked explicitly, AFTER
/// `sse::fetch_skill_context` resolved them against the catalogue and the
/// ownership rule — so they are the server's list of what was injected, not
/// the client's claim. They lead, in the order picked; keyword matches follow.
fn injected_skill_ids(
    state: &crate::state::AppState,
    org_id: &str,
    user_id: &str,
    turn_query: &str,
    requested_skill_ids: &[String],
    limit: i32,
) -> Vec<String> {
    use mp_contracts::model_plane::v1::MatchSkillsRequest;

    let mut out: Vec<String> = requested_skill_ids
        .iter()
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty())
        .collect();
    out.dedup();
    if org_id.trim().is_empty() {
        return out;
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
        &state.ownership,
        user_id,
    ) else {
        return out;
    };
    for id in matched
        .matches
        .into_iter()
        .filter_map(|m| m.skill)
        .filter(|s| !s.body.trim().is_empty() && !s.id.trim().is_empty())
        .map(|s| s.id)
    {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// A stand-in for the shared store, for this file's tests and the feedback route's.
#[cfg(test)]
pub(crate) mod fake {
    use super::*;

    #[derive(Default)]
    pub(crate) struct FakeSharedStore {
        pub(crate) entries: std::sync::Mutex<std::collections::HashMap<String, (String, Duration)>>,
    }

    impl SharedTurnStore for FakeSharedStore {
        async fn put(&self, key: &str, value: &str, ttl: Duration) {
            self.entries
                .lock()
                .unwrap()
                .insert(key.to_owned(), (value.to_owned(), ttl));
        }

        async fn get(&self, key: &str) -> Option<String> {
            self.entries
                .lock()
                .unwrap()
                .get(key)
                .map(|(v, _)| v.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeSharedStore;
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

    /// The previous turn carries whether its own answer was private, so the turn after a ZDR or
    /// waived answer can refuse the signal that answer attaches to. A persistent answer says so too.
    #[test]
    fn a_private_answer_is_flagged_for_the_turn_that_follows_it() {
        let turns = PreviousTurnRegistry::new();
        assert!(turns
            .swap("org", "user", "thread", "first", "run-1", Vec::new(), true)
            .is_none());
        let second = turns
            .swap(
                "org",
                "user",
                "thread",
                "second",
                "run-2",
                Vec::new(),
                false,
            )
            .expect("the first turn is the previous one");
        assert_eq!(second.run_id, "run-1");
        assert!(second.private_run, "the first answer was private");
        let third = turns
            .swap("org", "user", "thread", "third", "run-3", Vec::new(), false)
            .expect("the second turn is the previous one");
        assert_eq!(third.run_id, "run-2");
        assert!(!third.private_run, "the second answer was persistent");
    }

    fn turn(skills: &[&str]) -> ChatTurnRecord {
        ChatTurnRecord {
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            run_id: "run-1".to_owned(),
            skill_ids: skills.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn shared_turn_key_is_identity_scoped() {
        let key = shared_turn_key("org-1", "user-1", "req-1").expect("usable");
        assert_eq!(key, "mp:gw:turn:org-1\u{1f}user-1\u{1f}req-1");
        assert_ne!(
            Some(key.clone()),
            shared_turn_key("org-2", "user-1", "req-1")
        );
        assert_ne!(Some(key), shared_turn_key("org-1", "user-2", "req-1"));
        // A part carrying the separator could be shifted into another identity.
        assert!(shared_turn_key("org-1\u{1f}user-1", "x", "req-1").is_none());
        assert!(shared_turn_key("org-1", "user-1", "req\u{1f}1").is_none());
        assert!(shared_turn_key("", "user-1", "req-1").is_none());
        assert!(shared_turn_key("org-1", " ", "req-1").is_none());
        assert!(
            shared_turn_key("org-1", "user-1", &"x".repeat(MAX_REQUEST_ID_BYTES + 1)).is_none()
        );
    }

    #[test]
    fn the_shared_turn_record_switch_is_strict() {
        assert!(!shared_turn_record_from(None));
        for off in ["", "1", "on", "yes", "false", "enabled", "ture"] {
            assert!(
                !shared_turn_record_from(Some(off)),
                "{off:?} must not enable it"
            );
        }
        assert!(shared_turn_record_from(Some("true")));
    }

    #[tokio::test]
    async fn a_turn_streamed_on_one_replica_resolves_on_another() {
        let shared = FakeSharedStore::default();
        mirror_turn(&shared, "req-1", &turn(&["skill.a", "skill.b"])).await;

        // The other replica's own map has never seen the turn.
        let other_replica = ChatTurnRegistry::new();
        assert!(other_replica.lookup("req-1").is_none());
        other_replica
            .adopt_shared(&shared, "org-1", "user-1", "req-1")
            .await;
        let found = other_replica.lookup("req-1").expect("adopted");
        assert_eq!(found, turn(&["skill.a", "skill.b"]));
    }

    #[tokio::test]
    async fn another_user_or_tenant_cannot_adopt_the_turn() {
        let shared = FakeSharedStore::default();
        mirror_turn(&shared, "req-1", &turn(&["skill.a"])).await;
        for (org, user) in [("org-1", "user-2"), ("org-2", "user-1")] {
            let reg = ChatTurnRegistry::new();
            reg.adopt_shared(&shared, org, user, "req-1").await;
            assert!(reg.lookup("req-1").is_none(), "{org}/{user} must miss");
        }
    }

    #[tokio::test]
    async fn the_shared_record_holds_ids_and_a_24_hour_ttl_and_nothing_else() {
        let shared = FakeSharedStore::default();
        mirror_turn(&shared, "req-1", &turn(&["skill.a"])).await;
        let entries = shared.entries.lock().unwrap();
        let (value, ttl) = entries.values().next().expect("one record");
        assert_eq!(*ttl, TURN_TTL);
        let json: serde_json::Value = serde_json::from_str(value).unwrap();
        let mut keys: Vec<&String> = json.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(
            keys,
            ["run_id", "skill_ids"],
            "no identity, bearer or content"
        );
    }

    #[tokio::test]
    async fn a_local_record_is_not_replaced_by_the_shared_one() {
        let shared = FakeSharedStore::default();
        mirror_turn(&shared, "req-1", &turn(&["skill.shared"])).await;
        let reg = ChatTurnRegistry::new();
        record(&reg, "req-1", &["skill.local"]);
        reg.adopt_shared(&shared, "org-1", "user-1", "req-1").await;
        assert_eq!(reg.lookup("req-1").unwrap().skill_ids, vec!["skill.local"]);
    }

    #[tokio::test]
    async fn an_unusable_or_corrupt_shared_record_is_a_miss() {
        let shared = FakeSharedStore::default();
        let reg = ChatTurnRegistry::new();
        // Nothing written, a blank id, a record that is not JSON, and one with no run.
        reg.adopt_shared(&shared, "org-1", "user-1", "req-1").await;
        reg.adopt_shared(&shared, "org-1", "user-1", " ").await;
        let key = shared_turn_key("org-1", "user-1", "req-bad").unwrap();
        shared.put(&key, "not json", TURN_TTL).await;
        reg.adopt_shared(&shared, "org-1", "user-1", "req-bad")
            .await;
        let key = shared_turn_key("org-1", "user-1", "req-norun").unwrap();
        shared
            .put(&key, r#"{"run_id":" ","skill_ids":[]}"#, TURN_TTL)
            .await;
        reg.adopt_shared(&shared, "org-1", "user-1", "req-norun")
            .await;
        assert_eq!(reg.tracked(), 0);
    }

    #[tokio::test]
    async fn an_unusable_turn_is_not_mirrored() {
        let shared = FakeSharedStore::default();
        let mut blank_org = turn(&[]);
        blank_org.org_id.clear();
        mirror_turn(&shared, "req-1", &blank_org).await;
        mirror_turn(&shared, "", &turn(&[])).await;
        assert!(shared.entries.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_in_memory_store_mirrors_nothing() {
        let store = StreamBufferStore::new();
        mirror_turn(&store, "req-1", &turn(&["skill.a"])).await;
        assert!(lookup_shared_turn(&store, "org-1", "user-1", "req-1")
            .await
            .is_none());
    }
}

// EP-11 S3: the turn record names the version a rating attaches to, and a run's
// pins are readable for recovery inside that run only.
#[cfg(test)]
mod skill_pin_tests {
    use super::*;
    use mp_contracts::model_plane::v1::SkillPin;

    fn pin(id: &str, version: i32) -> SkillPin {
        SkillPin {
            skill_id: id.into(),
            version,
            digest: "e".repeat(64),
            source: "matched".into(),
        }
    }

    #[test]
    fn a_pinned_skill_is_recorded_as_id_at_version() {
        let refs = skill_refs(
            vec!["s1".into(), "builtin:office/pdf".into(), "s3".into()],
            &[pin("s1", 4), pin("builtin:office/pdf", 0)],
        );
        assert_eq!(refs, vec!["s1@4", "builtin:office/pdf", "s3"]);
    }

    #[test]
    fn switch_off_records_bare_ids() {
        let ids = vec!["s1".to_owned(), "s2".to_owned()];
        assert_eq!(skill_refs(ids.clone(), &[]), ids);
    }

    #[test]
    fn a_reference_gives_back_its_id() {
        assert_eq!(skill_id_of_ref("s1@4"), "s1");
        assert_eq!(skill_id_of_ref("s1"), "s1");
        // Only a numeric suffix is a version.
        assert_eq!(skill_id_of_ref("team@home"), "team@home");
        assert_eq!(skill_id_of_ref("@4"), "@4");
        assert_eq!(skill_id_of_ref("s1@"), "s1@");
    }

    #[test]
    fn a_run_reads_only_its_own_pins() {
        let pins = RunPinRegistry::default();
        pins.record("org-1", "run-1", &[pin("s1", 2)]);
        assert_eq!(pins.pin("org-1", "run-1", "s1").map(|p| p.version), Some(2));
        assert!(pins.pin("org-1", "run-1", "s2").is_none());
        assert!(pins.pin("org-1", "run-2", "s1").is_none());
        assert!(pins.pin("org-2", "run-1", "s1").is_none());
    }

    #[test]
    fn no_pins_record_nothing() {
        let pins = RunPinRegistry::default();
        pins.record("org-1", "run-1", &[]);
        pins.record("", "run-1", &[pin("s1", 2)]);
        assert!(pins.inner.is_empty());
    }
}
