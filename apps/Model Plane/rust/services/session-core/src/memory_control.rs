//! User edits/forgetting take precedence over delayed extraction and mirrors, and
//! the user's own memory state (on, paused, off) is decided here, once.
//!
//! [`resolve`] and [`resolve_in_tx`] are the single gate every memory writer and
//! reader asks. A writer asks under the user's row lock, so a pause that commits
//! first is seen and one that waits for the writer lands after it; a reader asks
//! without the lock (a read cannot hold one), so at worst it answers with the
//! state from just before a concurrent change.
use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1::MemoryEntry;
use sqlx::{PgPool, Postgres, Transaction};

/// Kill switch for the whole gate. Default ON; the operator lever is a falsey
/// value, which restores the behaviour before the state existed (every user is
/// treated as `on`). A restored deployment logs an ERROR once per Dreaming cycle
/// so the lever is never left pulled by accident.
pub(crate) const CONTROLS_ENABLED_ENV: &str = "MEMORY_CONTROLS_ENABLED";

pub(crate) fn controls_enabled() -> bool {
    enabled_from_env(std::env::var(CONTROLS_ENABLED_ENV).ok().as_deref())
}

/// Pure resolution so the default-ON contract is testable without mutating
/// process env (`unsafe_code` is forbidden workspace-wide).
fn enabled_from_env(value: Option<&str>) -> bool {
    !value.is_some_and(crate::learning_events::is_falsey)
}

/// The user's own choice, stored in `user_memory_controls.memory_state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemoryState {
    On,
    Paused,
    Off,
}

impl MemoryState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Paused => "paused",
            Self::Off => "off",
        }
    }

    fn from_storage(value: &str) -> Option<Self> {
        match value {
            "on" => Some(Self::On),
            "paused" => Some(Self::Paused),
            "off" => Some(Self::Off),
            _ => None,
        }
    }
}

/// One thread's own choice, stored in `threads.memory_mode`. It narrows the
/// user's state and never widens it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThreadMemoryMode {
    Inherit,
    Off,
}

impl ThreadMemoryMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Inherit => "inherit",
            Self::Off => "off",
        }
    }

    /// Strict, for a value a caller supplied: anything but the two spellings is
    /// refused rather than guessed at.
    pub(crate) fn from_wire(value: &str) -> Option<Self> {
        match value {
            "inherit" => Some(Self::Inherit),
            "off" => Some(Self::Off),
            _ => None,
        }
    }
}

/// What the organization's memory policy says. Session-core has no mirror of it
/// yet (build plan WS07 S4), so [`resolve`] passes `Allow` today; the other two
/// are decided here already so the mirror only has to supply the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OrgMemoryPolicy {
    Allow,
    Disabled,
    Unavailable,
}

impl OrgMemoryPolicy {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Disabled => "disabled",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Which `agent_memory` rows a reader may use. Org-owned rows
/// (`scope IN ('org', 'global')`, no session) stay readable while a user is
/// paused or off: they are the organization's, not the user's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemoryAccess {
    Full,
    OrgOnly,
    None,
}

/// Closed set of reasons, spelled the same on the wire
/// (`MemoryControls.reason`, `SearchMemoryResponse.skip_reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemoryReason {
    On,
    UserPaused,
    UserOff,
    ThreadOff,
    OrgDisabled,
    Zdr,
    PolicyUnavailable,
}

impl MemoryReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::UserPaused => "user_paused",
            Self::UserOff => "user_off",
            Self::ThreadOff => "thread_off",
            Self::OrgDisabled => "org_disabled",
            Self::Zdr => "zdr",
            Self::PolicyUnavailable => "policy_unavailable",
        }
    }
}

/// The one answer. `generation_allowed` covers every writer of `agent_memory`
/// and its semantic mirrors; `access` every reader; `learning_allowed` the
/// announcement of a completed run to the skill-learning loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MemoryDecision {
    pub(crate) reason: MemoryReason,
    pub(crate) access: MemoryAccess,
    pub(crate) generation_allowed: bool,
    pub(crate) learning_allowed: bool,
}

impl MemoryDecision {
    const fn denied(reason: MemoryReason) -> Self {
        Self {
            reason,
            access: MemoryAccess::None,
            generation_allowed: false,
            learning_allowed: false,
        }
    }

    /// The typed reason a reader reports when it left the user's memory out;
    /// empty when nothing was left out.
    pub(crate) fn skip_reason(&self) -> &'static str {
        match self.reason {
            MemoryReason::On => "",
            other => other.as_str(),
        }
    }

    /// Whether the user's own rows are listed. A paused user still sees (and
    /// can delete) every row; an off user, or an org that disabled memory, does
    /// not. A thread set off hides nothing: the list is the user's, not the
    /// thread's.
    pub(crate) fn list_visible(&self) -> bool {
        matches!(
            self.reason,
            MemoryReason::On | MemoryReason::UserPaused | MemoryReason::ThreadOff
        )
    }

    /// Wire form of a refusal to write, `failed_precondition` message text.
    pub(crate) fn refusal(&self) -> String {
        format!("memory_not_enabled:{}", self.reason.as_str())
    }
}

/// The decision table. Precedence: zero data retention, then the organization,
/// then the user. Pure, so the whole table is one test.
pub(crate) fn decide(zdr: bool, org: OrgMemoryPolicy, user: MemoryState) -> MemoryDecision {
    if zdr {
        return MemoryDecision::denied(MemoryReason::Zdr);
    }
    match org {
        OrgMemoryPolicy::Allow => {}
        OrgMemoryPolicy::Disabled => return MemoryDecision::denied(MemoryReason::OrgDisabled),
        // Fails closed, unlike org_quota: a policy that cannot be read is not a
        // policy that allows.
        OrgMemoryPolicy::Unavailable => {
            return MemoryDecision::denied(MemoryReason::PolicyUnavailable)
        }
    }
    match user {
        MemoryState::On => MemoryDecision {
            reason: MemoryReason::On,
            access: MemoryAccess::Full,
            generation_allowed: true,
            learning_allowed: true,
        },
        MemoryState::Paused => MemoryDecision {
            reason: MemoryReason::UserPaused,
            access: MemoryAccess::OrgOnly,
            generation_allowed: false,
            learning_allowed: false,
        },
        MemoryState::Off => MemoryDecision {
            reason: MemoryReason::UserOff,
            access: MemoryAccess::OrgOnly,
            generation_allowed: false,
            learning_allowed: false,
        },
    }
}

/// Folds a thread's own mode into a user-level decision. A thread set off
/// reads and writes nothing, org rows included. A denial that already leaves
/// nothing readable (ZDR, org policy) keeps its reason, which says more than
/// `thread_off`.
pub(crate) fn apply_thread_mode(
    decision: MemoryDecision,
    mode: ThreadMemoryMode,
) -> MemoryDecision {
    match mode {
        ThreadMemoryMode::Inherit => decision,
        ThreadMemoryMode::Off if decision.access == MemoryAccess::None => decision,
        ThreadMemoryMode::Off => MemoryDecision::denied(MemoryReason::ThreadOff),
    }
}

/// What the gate answers when [`CONTROLS_ENABLED_ENV`] is off: no restriction.
fn unrestricted() -> MemoryDecision {
    decide(false, OrgMemoryPolicy::Allow, MemoryState::On)
}

/// The stored choice and when it last changed.
#[derive(Debug, Clone, Copy)]
pub(crate) struct StoredState {
    pub(crate) state: MemoryState,
    pub(crate) changed_at: Option<DateTime<Utc>>,
}

/// Read the stored state without locking or creating the row. No row is `on`.
pub(crate) async fn stored_state<'e, E>(
    executor: E,
    org: &str,
    user: &str,
) -> Result<StoredState, sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    let row: Option<(String, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT memory_state, state_changed_at FROM user_memory_controls WHERE org_id = $1 AND user_id = $2",
    )
    .bind(org)
    .bind(user)
    .fetch_optional(executor)
    .await?;
    Ok(match row {
        None => StoredState {
            state: MemoryState::On,
            changed_at: None,
        },
        Some((state, changed_at)) => StoredState {
            // The CHECK constraint closes the set; an unknown value is treated as
            // the most protective one.
            state: MemoryState::from_storage(&state).unwrap_or(MemoryState::Off),
            changed_at,
        },
    })
}

/// The decision for a reader. `zdr` is the caller's retention posture; a ZDR
/// caller is answered without touching the stored state.
pub(crate) async fn resolve<'e, E>(
    executor: E,
    zdr: bool,
    org: &str,
    user: &str,
) -> Result<MemoryDecision, sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    if zdr {
        return Ok(decide(true, OrgMemoryPolicy::Allow, MemoryState::On));
    }
    if !controls_enabled() {
        return Ok(unrestricted());
    }
    let stored = stored_state(executor, org, user).await?;
    Ok(decide(false, OrgMemoryPolicy::Allow, stored.state))
}

/// The decision for a writer, taken under the user's row lock and so ordered
/// against a concurrent state change, delete or forget: the lock is held to the
/// end of the writer's transaction.
pub(crate) async fn resolve_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    zdr: bool,
    org: &str,
    user: &str,
) -> Result<MemoryDecision, sqlx::Error> {
    let stored = lock_row(tx, org, user).await?;
    if zdr {
        return Ok(decide(true, OrgMemoryPolicy::Allow, MemoryState::On));
    }
    if !controls_enabled() {
        return Ok(unrestricted());
    }
    Ok(decide(false, OrgMemoryPolicy::Allow, stored))
}

/// A thread's own mode. A thread that does not exist (or is another
/// organization's) has no mode to restrict.
async fn thread_mode<'e, E>(
    executor: E,
    org: &str,
    thread_id: &str,
) -> Result<ThreadMemoryMode, sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    let mode: Option<String> =
        sqlx::query_scalar("SELECT memory_mode FROM threads WHERE id = $1 AND org_id = $2")
            .bind(thread_id)
            .bind(org)
            .fetch_optional(executor)
            .await?;
    // The CHECK constraint closes the set; an unknown value is treated as the
    // most protective one.
    Ok(mode.map_or(ThreadMemoryMode::Inherit, |value| {
        ThreadMemoryMode::from_wire(&value).unwrap_or(ThreadMemoryMode::Off)
    }))
}

/// [`resolve`] for a reader that works on one thread: the thread's own mode is
/// folded in, so a thread set off recalls nothing while the user's other
/// threads are untouched. The kill switch restores the behaviour before either
/// choice existed.
pub(crate) async fn resolve_for_thread(
    pool: &PgPool,
    zdr: bool,
    org: &str,
    user: &str,
    thread_id: &str,
) -> Result<MemoryDecision, sqlx::Error> {
    let decision = resolve(pool, zdr, org, user).await?;
    if zdr || !controls_enabled() {
        return Ok(decision);
    }
    Ok(apply_thread_mode(
        decision,
        thread_mode(pool, org, thread_id).await?,
    ))
}

/// [`resolve_in_tx`] for a writer that works on one thread. The thread's mode is
/// read under the user's row lock, which the presentation update also takes
/// ([`lock_user`]) when it changes the mode, so a thread set off while an
/// extraction is in flight is seen at write time.
pub(crate) async fn resolve_in_tx_for_thread(
    tx: &mut Transaction<'_, Postgres>,
    zdr: bool,
    org: &str,
    user: &str,
    thread_id: &str,
) -> Result<MemoryDecision, sqlx::Error> {
    let decision = resolve_in_tx(tx, zdr, org, user).await?;
    if zdr || !controls_enabled() {
        return Ok(decision);
    }
    Ok(apply_thread_mode(
        decision,
        thread_mode(&mut **tx, org, thread_id).await?,
    ))
}

/// Takes the row lock and reads the state under it. [`lock_user`] is the same
/// lock without the read, for the delete and forget paths that must keep working
/// whatever the state.
async fn lock_row(
    tx: &mut Transaction<'_, Postgres>,
    org: &str,
    user: &str,
) -> Result<MemoryState, sqlx::Error> {
    sqlx::query(
        "INSERT INTO user_memory_controls (org_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(org)
    .bind(user)
    .execute(&mut **tx)
    .await?;
    let state: String = sqlx::query_scalar(
        "SELECT memory_state FROM user_memory_controls WHERE org_id = $1 AND user_id = $2 FOR UPDATE",
    )
    .bind(org)
    .bind(user)
    .fetch_one(&mut **tx)
    .await?;
    Ok(MemoryState::from_storage(&state).unwrap_or(MemoryState::Off))
}

/// What [`set_state`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StateChange {
    pub(crate) from: MemoryState,
    pub(crate) changed: bool,
}

/// Sets the user's state under their row lock and records a content-free event.
///
/// A transition INTO `on` moves `extract_after` to now, which is what keeps
/// everything said while paused or off from being mined after the resume: the
/// extractors only read messages newer than it.
pub(crate) async fn set_state(
    pool: &PgPool,
    org: &str,
    user: &str,
    to: MemoryState,
    actor: &str,
) -> Result<StateChange, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let from = lock_row(&mut tx, org, user).await?;
    if from == to {
        tx.commit().await?;
        return Ok(StateChange {
            from,
            changed: false,
        });
    }
    // clock_timestamp is measured after the lock, not when a waiting tx began.
    sqlx::query(
        "UPDATE user_memory_controls SET memory_state = $3, state_changed_at = clock_timestamp(), \
         state_changed_by = $4, \
         extract_after = CASE WHEN $3 = 'on' THEN clock_timestamp() ELSE extract_after END \
         WHERE org_id = $1 AND user_id = $2",
    )
    .bind(org)
    .bind(user)
    .bind(to.as_str())
    .bind(actor)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO memory_control_events (id, org_id, user_id, event, from_state, to_state, actor) \
         VALUES ($1, $2, $3, 'state_changed', $4, $5, $6)",
    )
    .bind(mp_ids::new_ulid())
    .bind(org)
    .bind(user)
    .bind(from.as_str())
    .bind(to.as_str())
    .bind(actor)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    metrics::counter!("mp_session_memory_state_changes_total", "to" => to.as_str()).increment(1);
    Ok(StateChange {
        from,
        changed: true,
    })
}

pub(crate) async fn lock_user(
    tx: &mut Transaction<'_, Postgres>,
    org: &str,
    user: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO user_memory_controls (org_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(org)
    .bind(user)
    .execute(&mut **tx)
    .await?;
    sqlx::query("SELECT 1 FROM user_memory_controls WHERE org_id = $1 AND user_id = $2 FOR UPDATE")
        .bind(org)
        .bind(user)
        .fetch_one(&mut **tx)
        .await?;
    Ok(())
}

pub(crate) async fn permits_extraction(
    tx: &mut Transaction<'_, Postgres>,
    org: &str,
    user: &str,
    message_id: &str,
) -> Result<bool, sqlx::Error> {
    // Under the same lock as a state change, so an extraction already in flight
    // when a pause commits is refused here instead of persisting after it.
    // A background writer has no caller, so the retention posture is not asked
    // here: it is decided where the credential is (`MemoryRetention`).
    if !resolve_in_tx(tx, false, org, user)
        .await?
        .generation_allowed
    {
        return Ok(false);
    }
    // A message of a thread set off is not extracted from either (unless the kill
    // switch restored the behaviour before the mode existed).
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM messages m JOIN threads t ON t.id = m.thread_id JOIN user_memory_controls c ON c.org_id = t.org_id AND c.user_id = t.user_id WHERE m.id = $3 AND t.org_id = $1 AND t.user_id = $2 AND m.created_at > c.extract_after AND (t.memory_mode = 'inherit' OR NOT $4))")
        .bind(org).bind(user).bind(message_id).bind(controls_enabled()).fetch_one(&mut **tx).await
}

pub(crate) async fn record_forgetting(
    tx: &mut Transaction<'_, Postgres>,
    org: &str,
    user: &str,
    memory_id: &str,
) -> Result<(), sqlx::Error> {
    lock_user(tx, org, user).await?;
    // clock_timestamp is measured after the lock, not when a waiting tx began.
    sqlx::query("UPDATE user_memory_controls SET extract_after = clock_timestamp() WHERE org_id = $1 AND user_id = $2")
        .bind(org).bind(user).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO forgotten_memory_ids (org_id, user_id, memory_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
        .bind(org).bind(user).bind(memory_id).execute(&mut **tx).await?;
    Ok(())
}

/// Drops semantic-store entries that must not be recalled: ids this user
/// forgot, and ids of durable rows that are no longer current
/// (`is_latest = false`, retired by a lineage write or the dedup backfill).
///
/// The durable read paths filter `is_latest = true` in SQL, but a semantic
/// mirror carries no such flag. Without the second half of the query a
/// superseded row that was ever mirrored keeps resurfacing through chat
/// recall, the memory panel and grounding's own semantic tier. An entry
/// whose id has no durable row at all is kept: the semantic tier can hold
/// memories the durable index never had.
pub(crate) async fn filter_forgotten(
    pool: &PgPool,
    org: &str,
    user: &str,
    entries: &mut Vec<MemoryEntry>,
) -> Result<(), sqlx::Error> {
    if entries.is_empty() {
        return Ok(());
    }
    let ids: Vec<&str> = entries
        .iter()
        .map(|entry| entry.memory_id.as_str())
        .collect();
    let hidden: Vec<String> = sqlx::query_scalar(
        "SELECT memory_id FROM forgotten_memory_ids WHERE org_id = $1 AND user_id = $2 AND memory_id = ANY($3) \
         UNION \
         SELECT id FROM agent_memory WHERE org_id = $1 AND id = ANY($3) AND is_latest = false",
    )
    .bind(org)
    .bind(user)
    .bind(ids)
    .fetch_all(pool)
    .await?;
    retain_recallable(entries, &hidden);
    Ok(())
}

fn retain_recallable(entries: &mut Vec<MemoryEntry>, hidden: &[String]) {
    entries.retain(|entry| !hidden.contains(&entry.memory_id));
}

/// Best-effort removal of the semantic mirrors of rows a lineage write just
/// retired. [`filter_forgotten`] already keeps them out of every read, so a
/// degraded delete here costs storage, not correctness.
pub(crate) async fn erase_retired_mirrors(
    letta: &crate::letta_adapter::LettaMemoryAdapter,
    org: &str,
    user: &str,
    retired_ids: &[String],
) {
    for memory_id in retired_ids {
        let outcome = letta.delete_detailed(org, user, memory_id).await;
        if outcome.degradation_reason.is_some() {
            tracing::warn!(
                memory_id = %memory_id,
                "retired memory mirror cleanup degraded; recall remains suppressed"
            );
        }
    }
}

pub(crate) async fn erase_late_mirror(
    pool: &PgPool,
    letta: &crate::letta_adapter::LettaMemoryAdapter,
    org: &str,
    user: &str,
    memory_id: &str,
) {
    let forgotten = sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM forgotten_memory_ids WHERE org_id = $1 AND user_id = $2 AND memory_id = $3)")
        .bind(org).bind(user).bind(memory_id).fetch_one(pool).await;
    match forgotten {
        Ok(true) => {
            let outcome = letta.delete_detailed(org, user, memory_id).await;
            if outcome.degradation_reason.is_some() {
                tracing::warn!(
                    memory_id,
                    "late forgotten memory mirror cleanup degraded; recall remains suppressed"
                );
            }
        }
        Ok(false) => {}
        Err(_) => tracing::warn!(memory_id, "late memory mirror deletion check unavailable"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(memory_id: &str) -> MemoryEntry {
        MemoryEntry {
            memory_id: memory_id.to_owned(),
            content: format!("content of {memory_id}"),
            ..MemoryEntry::default()
        }
    }

    #[test]
    fn hidden_ids_are_dropped_and_everything_else_keeps_its_order() {
        let mut entries = vec![entry("a"), entry("retired"), entry("b"), entry("forgotten")];
        retain_recallable(
            &mut entries,
            &["forgotten".to_owned(), "retired".to_owned()],
        );
        let ids: Vec<&str> = entries.iter().map(|e| e.memory_id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
    }

    /// A superseded row's semantic mirror must not come back through recall,
    /// the memory panel or grounding. Before this filter checked `is_latest`,
    /// the retired row below survived `filter_forgotten` untouched.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn a_retired_rows_mirror_is_filtered_but_a_current_or_unknown_one_is_not() {
        let pool =
            PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
                .await
                .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let org = format!("retired-filter-{}", mp_ids::new_ulid());
        let (current, retired) = (mp_ids::new_ulid(), mp_ids::new_ulid());
        for (id, key, latest) in [
            (&current, "user:llm:tone", true),
            (&retired, "user:llm:preferred_tone", false),
        ] {
            sqlx::query(
                "INSERT INTO agent_memory (id, org_id, scope, key, content, owner, is_latest) \
                 VALUES ($1, $2, 'user', $3, 'User prefers a warmer tone.', 'user-a', $4)",
            )
            .bind(id)
            .bind(&org)
            .bind(key)
            .bind(latest)
            .execute(&pool)
            .await
            .unwrap();
        }

        let mut entries = vec![entry(&current), entry(&retired), entry("semantic-only")];
        filter_forgotten(&pool, &org, "user-a", &mut entries)
            .await
            .unwrap();
        let ids: Vec<&str> = entries.iter().map(|e| e.memory_id.as_str()).collect();
        assert_eq!(ids, [current.as_str(), "semantic-only"]);

        // Retirement is tenant-scoped like forgetting: another org's filter
        // never consults this org's rows.
        let mut foreign = vec![entry(&retired)];
        filter_forgotten(&pool, "another-org", "user-a", &mut foreign)
            .await
            .unwrap();
        assert_eq!(foreign.len(), 1);
    }

    // ---- the decision table ----

    /// The whole gate, over every (zdr, org, user) combination: ZDR first, then
    /// the organization, then the user. A row changes only deliberately.
    #[test]
    fn decision_table() {
        use MemoryAccess::{Full, None as NoAccess, OrgOnly};
        use MemoryReason as R;
        use MemoryState::{Off, On, Paused};
        use OrgMemoryPolicy::{Allow, Disabled, Unavailable};
        // (zdr, org, user) -> (reason, access, generation, learning)
        let table = [
            (false, Allow, On, R::On, Full, true, true),
            (false, Allow, Paused, R::UserPaused, OrgOnly, false, false),
            (false, Allow, Off, R::UserOff, OrgOnly, false, false),
            (false, Disabled, On, R::OrgDisabled, NoAccess, false, false),
            (
                false,
                Disabled,
                Paused,
                R::OrgDisabled,
                NoAccess,
                false,
                false,
            ),
            (false, Disabled, Off, R::OrgDisabled, NoAccess, false, false),
            (
                false,
                Unavailable,
                On,
                R::PolicyUnavailable,
                NoAccess,
                false,
                false,
            ),
            (
                false,
                Unavailable,
                Paused,
                R::PolicyUnavailable,
                NoAccess,
                false,
                false,
            ),
            (
                false,
                Unavailable,
                Off,
                R::PolicyUnavailable,
                NoAccess,
                false,
                false,
            ),
            (true, Allow, On, R::Zdr, NoAccess, false, false),
            (true, Allow, Paused, R::Zdr, NoAccess, false, false),
            (true, Allow, Off, R::Zdr, NoAccess, false, false),
            (true, Disabled, On, R::Zdr, NoAccess, false, false),
            (true, Disabled, Paused, R::Zdr, NoAccess, false, false),
            (true, Disabled, Off, R::Zdr, NoAccess, false, false),
            (true, Unavailable, On, R::Zdr, NoAccess, false, false),
            (true, Unavailable, Paused, R::Zdr, NoAccess, false, false),
            (true, Unavailable, Off, R::Zdr, NoAccess, false, false),
        ];
        for (zdr, org, user, reason, access, generation, learning) in table {
            let decision = decide(zdr, org, user);
            assert_eq!(
                decision,
                MemoryDecision {
                    reason,
                    access,
                    generation_allowed: generation,
                    learning_allowed: learning,
                },
                "zdr={zdr} org={org:?} user={user:?}"
            );
        }
    }

    /// A thread set off reads and writes nothing, whatever the user's state; a
    /// thread that inherits changes nothing; an existing denial keeps its reason.
    #[test]
    fn a_thread_set_off_narrows_every_user_state_and_never_widens_one() {
        use MemoryState::{Off, On, Paused};
        use OrgMemoryPolicy::{Allow, Disabled};
        for user in [On, Paused, Off] {
            let base = decide(false, Allow, user);
            assert_eq!(apply_thread_mode(base, ThreadMemoryMode::Inherit), base);
        }
        let off = apply_thread_mode(decide(false, Allow, On), ThreadMemoryMode::Off);
        assert_eq!(
            off,
            MemoryDecision {
                reason: MemoryReason::ThreadOff,
                access: MemoryAccess::None,
                generation_allowed: false,
                learning_allowed: false,
            }
        );
        // Paused leaves the org's rows readable; a thread set off does not.
        let paused = apply_thread_mode(decide(false, Allow, Paused), ThreadMemoryMode::Off);
        assert_eq!(paused.access, MemoryAccess::None);
        assert_eq!(paused.reason, MemoryReason::ThreadOff);
        // ZDR and org policy already deny everything and say why.
        assert_eq!(
            apply_thread_mode(decide(true, Allow, On), ThreadMemoryMode::Off).reason,
            MemoryReason::Zdr
        );
        assert_eq!(
            apply_thread_mode(decide(false, Disabled, On), ThreadMemoryMode::Off).reason,
            MemoryReason::OrgDisabled
        );
        assert_eq!(off.skip_reason(), "thread_off");
        assert!(off.list_visible(), "a thread set off hides no listed row");
    }

    #[test]
    fn a_thread_mode_value_is_one_of_two_spellings() {
        for mode in [ThreadMemoryMode::Inherit, ThreadMemoryMode::Off] {
            assert_eq!(ThreadMemoryMode::from_wire(mode.as_str()), Some(mode));
        }
        for refused in ["", "OFF", "on", "paused", "inherit "] {
            assert_eq!(ThreadMemoryMode::from_wire(refused), None);
        }
    }

    #[test]
    fn the_thread_migration_defaults_to_inherit_and_has_a_revert() {
        let migration = include_str!("../migrations/0067_thread_memory_mode.sql");
        assert!(migration
            .contains("ADD COLUMN IF NOT EXISTS memory_mode TEXT NOT NULL DEFAULT 'inherit'"));
        assert!(migration.contains("CHECK (memory_mode IN ('inherit', 'off'))"));
        let revert = include_str!("../migrations/revert/0067_thread_memory_mode.down.sql");
        assert!(revert.contains("DROP COLUMN IF EXISTS memory_mode"));
    }

    #[test]
    fn reasons_are_the_closed_wire_tokens() {
        let tokens: Vec<&str> = [
            MemoryReason::On,
            MemoryReason::UserPaused,
            MemoryReason::UserOff,
            MemoryReason::ThreadOff,
            MemoryReason::OrgDisabled,
            MemoryReason::Zdr,
            MemoryReason::PolicyUnavailable,
        ]
        .into_iter()
        .map(MemoryReason::as_str)
        .collect();
        assert_eq!(
            tokens,
            [
                "on",
                "user_paused",
                "user_off",
                "thread_off",
                "org_disabled",
                "zdr",
                "policy_unavailable"
            ]
        );
    }

    #[test]
    fn only_a_skipped_decision_carries_a_skip_reason_and_a_refusal_names_it() {
        let on = decide(false, OrgMemoryPolicy::Allow, MemoryState::On);
        assert_eq!(on.skip_reason(), "");
        let paused = decide(false, OrgMemoryPolicy::Allow, MemoryState::Paused);
        assert_eq!(paused.skip_reason(), "user_paused");
        assert_eq!(paused.refusal(), "memory_not_enabled:user_paused");
        assert_eq!(
            decide(true, OrgMemoryPolicy::Allow, MemoryState::On).skip_reason(),
            "zdr"
        );
    }

    /// D2: paused keeps the rows visible (and deletable), off hides them, and an
    /// org that disabled memory hides them too.
    #[test]
    fn a_paused_list_stays_visible_and_an_off_or_org_disabled_one_does_not() {
        let visible = |zdr, org, user| decide(zdr, org, user).list_visible();
        assert!(visible(false, OrgMemoryPolicy::Allow, MemoryState::On));
        assert!(visible(false, OrgMemoryPolicy::Allow, MemoryState::Paused));
        assert!(!visible(false, OrgMemoryPolicy::Allow, MemoryState::Off));
        assert!(!visible(false, OrgMemoryPolicy::Disabled, MemoryState::On));
        assert!(!visible(
            false,
            OrgMemoryPolicy::Unavailable,
            MemoryState::On
        ));
    }

    #[test]
    fn storage_spelling_round_trips_and_an_unknown_value_is_not_a_state() {
        for state in [MemoryState::On, MemoryState::Paused, MemoryState::Off] {
            assert_eq!(MemoryState::from_storage(state.as_str()), Some(state));
        }
        assert_eq!(MemoryState::from_storage("ON"), None);
        assert_eq!(MemoryState::from_storage(""), None);
    }

    /// Default ON, and only an explicit falsey value pulls the lever (the same
    /// vocabulary as `LEARNING_RUN_EVENTS_ENABLED`).
    #[test]
    fn the_kill_switch_defaults_on_and_honours_an_explicit_off() {
        assert!(enabled_from_env(None));
        assert!(enabled_from_env(Some("")));
        assert!(enabled_from_env(Some("true")));
        assert!(enabled_from_env(Some("anything-else")));
        for value in ["false", "FALSE", " 0 ", "off", "No"] {
            assert!(!enabled_from_env(Some(value)), "{value} must pull it");
        }
    }

    /// A switched-off gate restricts nobody: that is the behaviour before the
    /// state existed.
    #[test]
    fn an_unrestricted_decision_is_the_on_decision() {
        assert_eq!(
            unrestricted(),
            decide(false, OrgMemoryPolicy::Allow, MemoryState::On)
        );
    }

    #[test]
    fn the_migration_adds_the_state_with_a_neutral_default_and_a_content_free_log() {
        let migration = include_str!("../migrations/0051_user_memory_state.sql");
        assert!(
            migration.contains("ADD COLUMN IF NOT EXISTS memory_state TEXT NOT NULL DEFAULT 'on'")
        );
        assert!(migration.contains("CHECK (memory_state IN ('on', 'paused', 'off'))"));
        assert!(migration.contains("CREATE TABLE IF NOT EXISTS memory_control_events"));
        // Counts and states only: no column could hold a memory or a message.
        let table = &migration[migration
            .find("CREATE TABLE IF NOT EXISTS memory_control_events")
            .unwrap()..];
        for forbidden in ["content", "message", "text"] {
            assert!(
                !table.contains(&format!(" {forbidden} "))
                    && !table.contains(&format!(" {forbidden},")),
                "the event log must not have a {forbidden} column"
            );
        }
        let revert = include_str!("../migrations/revert/0051_user_memory_state.down.sql");
        assert!(revert.contains("DROP TABLE IF EXISTS memory_control_events"));
        assert!(revert.contains("DROP COLUMN IF EXISTS memory_state"));
    }

    // ---- every writer calls the gate ----

    /// Slice of `source` from `start` up to (not including) `end`.
    fn between<'a>(source: &'a str, start: &str, end: &str) -> (usize, usize, &'a str) {
        let from = source
            .find(start)
            .unwrap_or_else(|| panic!("{start:?} not found - re-point this test"));
        let to = source[from..]
            .find(end)
            .map(|offset| from + offset)
            .unwrap_or_else(|| panic!("{end:?} not found after {start:?}"));
        (from, to, &source[from..to])
    }

    /// Every path that writes `agent_memory` asks the gate, and no new one can
    /// appear without being routed through it: the three entry points (inline
    /// and Dreaming persist, grounded persist, manual index) each name the gate
    /// in their own body, every call to a low-level writer sits inside one of
    /// them, and every write statement sits inside the low-level blocks. A new
    /// writer elsewhere fails here and has to join the gate first.
    #[test]
    fn every_agent_memory_writer_calls_the_gate() {
        let source = include_str!("dreaming.rs");
        let production = &source[..source.find("#[cfg(test)]").expect("test module")];

        let entry_points = [
            between(
                production,
                "pub(crate) async fn persist_candidates(",
                "/// [`persist_candidates`]'s grounding-aware sibling",
            ),
            between(
                production,
                "pub(crate) async fn apply_grounded_candidates(",
                "/// Writes an EXTEND or SUPERSEDE decision",
            ),
            between(
                production,
                "pub(crate) async fn index_agent_memory(",
                "/// Insert `candidate`, or refresh the row",
            ),
        ];
        let gates = [
            "permits_extraction(",
            "permits_extraction(",
            "resolve_in_tx(",
        ];
        for ((_, _, body), gate) in entry_points.iter().zip(gates) {
            assert!(
                body.contains(&format!("memory_control::{gate}")),
                "a memory writer entry point must ask the gate ({gate})"
            );
        }

        let low_level = [
            between(
                production,
                "async fn write_lineage_memory(",
                "pub(crate) async fn record_dream_run(",
            ),
            between(
                production,
                "async fn upsert_agent_memory(",
                "/// Lists durable `agent_memory` rows owned directly",
            ),
        ];
        // A low-level writer may call another (the lineage write falls back to a
        // plain upsert); only a call from outside both sets is a new path.
        let inside = |index: usize| {
            entry_points
                .iter()
                .chain(low_level.iter())
                .any(|(from, to, _)| (*from..*to).contains(&index))
        };
        for needle in ["upsert_agent_memory(", "write_lineage_memory("] {
            for (index, _) in production.match_indices(needle) {
                let line_start = production[..index].rfind('\n').map_or(0, |at| at + 1);
                let line = production[line_start..]
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .trim_start();
                if line.starts_with("//") || line.contains("fn ") {
                    continue;
                }
                assert!(
                    inside(index),
                    "{needle} is called outside a gated writer entry point: {line}"
                );
            }
        }

        for statement in [
            "INSERT INTO agent_memory",
            "UPDATE agent_memory",
            "DELETE FROM agent_memory",
        ] {
            for (index, _) in production.match_indices(statement) {
                assert!(
                    low_level
                        .iter()
                        .any(|(from, to, _)| (*from..*to).contains(&index))
                        || (production
                            .find("pub(crate) async fn delete_user_memory(")
                            .unwrap()..)
                            .contains(&index),
                    "{statement} appears outside the gated low-level writers"
                );
            }
        }

        // The inline path asks the gate under the thread lock, before persisting.
        let grpc = include_str!("grpc.rs");
        let (_, _, append) = between(
            grpc,
            "pub(crate) async fn append_message_in_tx(",
            "/// Mirror an append's extracted memories into Letta",
        );
        assert!(append.contains("memory_control::resolve_in_tx_for_thread("));
        assert!(
            append.find("memory_control::resolve_in_tx_for_thread(").unwrap()
                < append.find("persist_candidates(").unwrap(),
            "the inline gate must precede the persist"
        );
    }

    // ---- against a disposable database ----

    use crate::auth::VerifiedIdentity;
    use mp_contracts::model_plane::v1 as pb;
    use mp_contracts::model_plane::v1::memory_service_server::MemoryService;

    async fn disposable_pool() -> PgPool {
        let pool =
            PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
                .await
                .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn seed_thread(pool: &PgPool, org: &str, user: &str) -> String {
        let thread = mp_ids::new_ulid();
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)",
        )
        .bind(&thread)
        .bind(org)
        .bind(user)
        .execute(pool)
        .await
        .unwrap();
        thread
    }

    /// The inline path: a user message appended the way `AppendMessage` does,
    /// which extracts and persists memory in the same transaction.
    async fn append_user_message(pool: &PgPool, thread: &str, user: &str, content: &str) -> String {
        let mut tx = pool.begin().await.unwrap();
        let appended = crate::grpc::append_message_in_tx(
            &mut tx,
            &pb::AppendMessageRequest {
                thread_id: thread.to_owned(),
                role: "user".to_owned(),
                content: content.to_owned(),
                ..Default::default()
            },
            user,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        appended.message_id
    }

    async fn memory_rows(pool: &PgPool, org: &str) -> Vec<String> {
        sqlx::query_scalar("SELECT content FROM agent_memory WHERE org_id = $1 ORDER BY content")
            .bind(org)
            .fetch_all(pool)
            .await
            .unwrap()
    }

    async fn seed_row(
        pool: &PgPool,
        org: &str,
        scope: &str,
        owner: &str,
        key: &str,
        content: &str,
    ) -> String {
        let id = mp_ids::new_ulid();
        sqlx::query(
            "INSERT INTO agent_memory (id, org_id, session_id, scope, key, content, owner) \
             VALUES ($1, $2, NULL, $3, $4, $5, $6)",
        )
        .bind(&id)
        .bind(org)
        .bind(scope)
        .bind(key)
        .bind(content)
        .bind(owner)
        .execute(pool)
        .await
        .unwrap();
        id
    }

    fn request<T>(value: T, identity: VerifiedIdentity) -> tonic::Request<T> {
        let mut request = tonic::Request::new(value);
        request.extensions_mut().insert(identity);
        request
    }

    async fn dream(pool: &PgPool) {
        crate::dreaming::dream_once(pool, None, None, None, 1000)
            .await
            .unwrap();
    }

    const FACT: &str = "Husk at jeg heter Ima.";

    /// The headline: a paused user's turn writes nothing, inline or in the
    /// background, while another user in the same org is unaffected.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn pause_blocks_inline_and_dreaming_extraction() {
        let pool = disposable_pool().await;
        let control_org = format!("pause-test-{}", mp_ids::new_ulid());
        let control_thread = seed_thread(&pool, &control_org, "user-a").await;
        append_user_message(&pool, &control_thread, "user-a", FACT).await;
        assert_eq!(
            memory_rows(&pool, &control_org).await.len(),
            1,
            "the inline row appears while on"
        );

        let org = format!("pause-test-{}", mp_ids::new_ulid());
        let thread = seed_thread(&pool, &org, "user-a").await;
        let other_thread = seed_thread(&pool, &org, "user-b").await;
        set_state(&pool, &org, "user-a", MemoryState::Paused, "user-a")
            .await
            .unwrap();
        append_user_message(&pool, &thread, "user-a", FACT).await;
        dream(&pool).await;
        assert!(
            memory_rows(&pool, &org).await.is_empty(),
            "no inline or Dreaming row while paused"
        );
        let runs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dream_runs WHERE org_id = $1")
            .bind(&org)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(runs, 0, "a paused user's messages are not even read");

        append_user_message(&pool, &other_thread, "user-b", "Husk at jeg heter Kari.").await;
        assert_eq!(
            memory_rows(&pool, &org).await.len(),
            1,
            "another user's memory is untouched"
        );
    }

    /// What is said while paused is never mined after the resume: the
    /// transition into `on` moves `extract_after` past those messages.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn resume_does_not_mine_messages_written_while_paused() {
        let pool = disposable_pool().await;
        let org = format!("resume-test-{}", mp_ids::new_ulid());
        let thread = seed_thread(&pool, &org, "user-a").await;
        set_state(&pool, &org, "user-a", MemoryState::Paused, "user-a")
            .await
            .unwrap();
        append_user_message(&pool, &thread, "user-a", FACT).await;
        let change = set_state(&pool, &org, "user-a", MemoryState::On, "user-a")
            .await
            .unwrap();
        assert!(change.changed && change.from == MemoryState::Paused);
        dream(&pool).await;
        assert!(
            memory_rows(&pool, &org).await.is_empty(),
            "the paused-period message is never extracted"
        );

        append_user_message(&pool, &thread, "user-a", "Mitt navn er Kari.").await;
        let rows = memory_rows(&pool, &org).await;
        assert!(
            rows.iter().any(|row| row.contains("Kari")),
            "learning resumes for new messages"
        );
        assert!(rows.iter().all(|row| !row.contains("Ima")));

        let events: Vec<(String, String)> = sqlx::query_as(
            "SELECT from_state, to_state FROM memory_control_events WHERE org_id = $1 ORDER BY at",
        )
        .bind(&org)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            events,
            [
                ("on".to_owned(), "paused".to_owned()),
                ("paused".to_owned(), "on".to_owned())
            ]
        );
    }

    /// Paused and off both leave the org's own rows readable and the user's
    /// out, through the search RPC and the context assembly alike.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn paused_user_still_gets_org_rows_and_no_user_rows() {
        let pool = disposable_pool().await;
        let org = format!("org-rows-test-{}", mp_ids::new_ulid());
        let thread = seed_thread(&pool, &org, "user-a").await;
        seed_row(
            &pool,
            &org,
            "org",
            "",
            "org:policy",
            "Org fact: invoices close on the 5th.",
        )
        .await;
        seed_row(
            &pool,
            &org,
            "user",
            "user-a",
            "user:name",
            "User fact: the name is Ima.",
        )
        .await;
        let service = crate::memory_grpc::MemoryGrpc::new(pool.clone(), None);

        for (state, expected_reason) in [
            (MemoryState::On, ""),
            (MemoryState::Paused, "user_paused"),
            (MemoryState::Off, "user_off"),
        ] {
            set_state(&pool, &org, "user-a", state, "user-a")
                .await
                .unwrap();
            let response = service
                .search_memory(request(
                    pb::SearchMemoryRequest {
                        org_id: org.clone(),
                        thread_id: thread.clone(),
                        limit: 10,
                        ..Default::default()
                    },
                    VerifiedIdentity::user_for_test(&org, "user-a"),
                ))
                .await
                .unwrap()
                .into_inner();
            let contents: Vec<&str> = response
                .entries
                .iter()
                .map(|e| e.content.as_str())
                .collect();
            assert_eq!(response.skip_reason, expected_reason);
            assert!(
                contents.iter().any(|c| c.contains("Org fact")),
                "org rows stay readable in {state:?}"
            );
            assert_eq!(
                contents.iter().any(|c| c.contains("User fact")),
                state == MemoryState::On,
                "the user's own rows are recalled only while on ({state:?})"
            );

            let access = decide(false, OrgMemoryPolicy::Allow, state).access;
            let context =
                crate::dreaming::load_agent_memory_context_rows(&pool, &thread, None, 10, access)
                    .await
                    .unwrap();
            assert!(context.iter().any(|(_, row)| row.contains("Org fact")));
            assert_eq!(
                context.iter().any(|(_, row)| row.contains("User fact")),
                state == MemoryState::On
            );
        }
    }

    /// Off hides the list and says how many rows it hides; delete is never
    /// blocked, so the user can still erase what is hidden.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn off_hides_list_but_delete_still_works() {
        let pool = disposable_pool().await;
        let org = format!("off-test-{}", mp_ids::new_ulid());
        let id = seed_row(
            &pool,
            &org,
            "user",
            "user-a",
            "user:name",
            "User fact: the name is Ima.",
        )
        .await;
        let service = crate::memory_grpc::MemoryGrpc::new(pool.clone(), None);
        let identity = || VerifiedIdentity::user_for_test(&org, "user-a");
        let list = || {
            request(
                pb::ListMemoryRequest {
                    org_id: org.clone(),
                    user_id: "user-a".into(),
                    limit: 10,
                },
                identity(),
            )
        };

        let listed = service.list_memory(list()).await.unwrap().into_inner();
        assert_eq!(
            (listed.entries.len(), listed.hidden_reason.as_str()),
            (1, "")
        );

        set_state(&pool, &org, "user-a", MemoryState::Paused, "user-a")
            .await
            .unwrap();
        let listed = service.list_memory(list()).await.unwrap().into_inner();
        assert_eq!(
            (listed.entries.len(), listed.hidden_reason.as_str()),
            (1, ""),
            "paused keeps the list"
        );

        set_state(&pool, &org, "user-a", MemoryState::Off, "user-a")
            .await
            .unwrap();
        let listed = service.list_memory(list()).await.unwrap().into_inner();
        assert_eq!(
            (listed.entries.len(), listed.hidden_reason.as_str()),
            (0, "user_off")
        );
        let controls = service
            .get_memory_controls(request(
                pb::GetMemoryControlsRequest {
                    org_id: org.clone(),
                    user_id: "user-a".into(),
                },
                identity(),
            ))
            .await
            .unwrap()
            .into_inner()
            .controls
            .unwrap();
        assert_eq!(
            (controls.hidden_rows, controls.reason.as_str()),
            (1, "user_off")
        );
        assert!(
            !controls.use_allowed && !controls.generation_allowed && !controls.learning_allowed
        );

        let deleted = service
            .delete_memory(request(
                pb::DeleteMemoryRequest {
                    org_id: org.clone(),
                    user_id: "user-a".into(),
                    memory_id: id,
                },
                identity(),
            ))
            .await
            .unwrap()
            .into_inner();
        assert!(deleted.deleted, "delete is never gated");
        assert!(memory_rows(&pool, &org).await.is_empty());
    }

    /// An extraction whose candidates were computed before the pause cannot
    /// persist after it: the writer asks the gate at write time, under the lock.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn inflight_extraction_cannot_persist_after_pause() {
        let pool = disposable_pool().await;
        let org = format!("inflight-test-{}", mp_ids::new_ulid());
        let thread = seed_thread(&pool, &org, "user-a").await;
        // A message that yields nothing inline, so no row exists yet.
        let message_id =
            append_user_message(&pool, &thread, "user-a", "Hei, hvordan går det?").await;
        let candidates = crate::dreaming::extract_memory_candidates("user", FACT, &thread);
        assert!(
            !candidates.is_empty(),
            "the extraction in flight has something to write"
        );

        set_state(&pool, &org, "user-a", MemoryState::Paused, "user-a")
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        let (saved, persisted) = crate::dreaming::persist_candidates(
            &mut tx,
            &org,
            "user-a",
            &thread,
            &message_id,
            &candidates,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        assert_eq!((saved, persisted.len()), (0, 0));
        assert!(memory_rows(&pool, &org).await.is_empty());

        // The same holds for a manual save, which is refused with its reason.
        let service = crate::memory_grpc::MemoryGrpc::new(pool.clone(), None);
        let refusal = service
            .index_memory(request(
                pb::IndexMemoryRequest {
                    org_id: org.clone(),
                    thread_id: thread.clone(),
                    topic: "USER".into(),
                    content: FACT.into(),
                    ..Default::default()
                },
                VerifiedIdentity::user_for_test(&org, "user-a"),
            ))
            .await
            .unwrap_err();
        assert_eq!(refusal.code(), tonic::Code::FailedPrecondition);
        assert_eq!(refusal.message(), "memory_not_enabled:user_paused");
        assert!(memory_rows(&pool, &org).await.is_empty());
    }

    /// A ZDR or waived credential neither sets nor stores a state; a ZDR read is
    /// answered `zdr` and touches nothing.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn zdr_caller_cannot_set_state() {
        let pool = disposable_pool().await;
        let org = format!("zdr-state-test-{}", mp_ids::new_ulid());
        let service = crate::memory_grpc::MemoryGrpc::new(pool.clone(), None);
        let set = |identity: VerifiedIdentity| {
            request(
                pb::SetMemoryControlsRequest {
                    org_id: org.clone(),
                    user_id: "user-a".into(),
                    state: pb::MemoryState::Off as i32,
                },
                identity,
            )
        };
        for identity in [
            VerifiedIdentity::user_for_test_with_zdr(&org, "user-a", true),
            VerifiedIdentity::user_for_test_with_waiver(&org, "user-a", "connection-1", "ack-1"),
        ] {
            let refusal = service
                .set_memory_controls(set(identity))
                .await
                .unwrap_err();
            assert_eq!(refusal.code(), tonic::Code::FailedPrecondition);
        }
        let by_service = service
            .set_memory_controls(set(VerifiedIdentity::service_for_test(
                &org,
                &["memory:write"],
                false,
            )))
            .await
            .unwrap_err();
        assert_eq!(by_service.code(), tonic::Code::PermissionDenied);
        let rows: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM user_memory_controls WHERE org_id = $1")
                .bind(&org)
                .fetch_one(&pool)
                .await
                .unwrap();
        let events: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM memory_control_events WHERE org_id = $1")
                .bind(&org)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!((rows, events), (0, 0), "a refused set writes nothing");

        let controls = service
            .get_memory_controls(request(
                pb::GetMemoryControlsRequest {
                    org_id: org.clone(),
                    user_id: "user-a".into(),
                },
                VerifiedIdentity::user_for_test_with_zdr(&org, "user-a", true),
            ))
            .await
            .unwrap()
            .into_inner()
            .controls
            .unwrap();
        assert_eq!(controls.reason, "zdr");
        assert!(!controls.use_allowed && !controls.generation_allowed);
        assert_eq!(controls.state, pb::MemoryState::Unspecified as i32);
    }
}
