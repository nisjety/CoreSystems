//! Space memory containment: the one resolver (build plan WS17-S1).
//!
//! Rule W: nothing said in a shared room is learned into personal or tenant memory.
//! Rule R: a room turn recalls only this thread's own rows (labels arrive in WS17-S3).
//!
//! A thread is CONTAINED when it belongs to a Space and its Control-signed `space_kind`
//! is anything but `personal`. A missing kind (every thread created before the claim
//! existed, scheduled threads) is contained: fail closed. Every containment site calls
//! this module; none re-derives the predicate. `MEMORY_SPACE_CONTAINMENT=0|false|off`
//! switches containment off (kill switch, never an authorization input).
//!
//! Counters (`mp_session_memory_contained_total{op}`) carry a closed `op` label only,
//! never ids or content.

use mp_contracts::space_memory::containment_enabled;
use sqlx::PgPool;

/// SQL twin of [`thread_is_contained_with`] for a `threads` row aliased `t`, flag on.
pub(crate) const CONTAINED_SQL: &str =
    "(t.space_id IS NOT NULL AND t.space_kind IS DISTINCT FROM 'personal')";

/// The `source_links` shape a legacy room-origin row carries (`thread:<id>`) tested against
/// contained threads, for an unaliased `agent_memory` query. `{thread}` is the placeholder
/// of the reader's thread id: the reader's own thread rows stay visible (C7).
fn legacy_room_origin_sql(thread_param: &str) -> String {
    format!(
        "(session_id = {thread_param} OR NOT EXISTS ( \
           SELECT 1 FROM unnest(agent_memory.source_links) AS sl(link) \
           JOIN threads ct ON ct.id = substr(sl.link, 8) \
           WHERE sl.link LIKE 'thread:%' \
             AND ct.space_id IS NOT NULL AND ct.space_kind IS DISTINCT FROM 'personal'))"
    )
}

/// C7 as a bindable predicate: `(NOT $flag OR <exclusion>)`. `flag_param` is a boolean
/// parameter the caller binds to [`containment_on`], `thread_param` the reader thread id.
pub(crate) fn legacy_room_origin_exclusion(flag_param: &str, thread_param: &str) -> String {
    format!(
        "(NOT {flag_param} OR {})",
        legacy_room_origin_sql(thread_param)
    )
}

/// Closed vocabulary value for `MemoryEntry.withheld_reason` (WS17-S2): a user-scope row
/// learned in a shared room before containment existed. Listed to its owner, never recalled.
pub(crate) const WITHHELD_ROOM_ORIGIN: &str = "room_origin_unverified";

/// The Space a legacy room-origin row was learned in, as a scalar subquery over an
/// unaliased `agent_memory` query (`NULL` for an ordinary row, or when `flag_param` is
/// false: with containment off the row is recalled again, so it is not withheld). Same
/// `source_links` shape as [`legacy_room_origin_exclusion`], so the list and the recall
/// filter always agree on which rows are withheld.
pub(crate) fn legacy_room_origin_space(flag_param: &str) -> String {
    format!(
        "(SELECT ct.space_id FROM unnest(agent_memory.source_links) AS sl(link)            JOIN threads ct ON ct.id = substr(sl.link, 8)           WHERE {flag_param} AND sl.link LIKE 'thread:%'             AND ct.space_id IS NOT NULL AND ct.space_kind IS DISTINCT FROM 'personal'           ORDER BY ct.space_id LIMIT 1)"
    )
}

/// Message shown when a contained thread tries to write tenant- or user-wide memory.
pub(crate) const REFUSAL_CONTAINED: &str =
    "memory_scope_contained: a shared room cannot write user, policy, agent or workspace memory";

/// C2: the Letta mirror is never called for a contained thread. Reported like the other
/// retention skips so the missing semantic copy is never silent.
pub(crate) const DEGRADED_LETTA_SPACE_SUPPRESSED: &str = "DEGRADED_LETTA_SPACE_SUPPRESSED";

/// Whether containment is switched on for this process.
pub(crate) fn containment_on() -> bool {
    containment_enabled()
}

/// What a reader thread may recall.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)] // the audience fields are read by WS17-S3 labels
pub(crate) enum ReaderScope {
    /// An ordinary thread, or a personal-Space thread: today's rules.
    Personal,
    /// A shared-room thread: only its own rows, nothing tenant- or user-wide.
    Contained {
        space_ref: String,
        audience_revision: i64,
        audience_hash: String,
    },
}

impl ReaderScope {
    pub(crate) fn is_contained(&self) -> bool {
        matches!(self, ReaderScope::Contained { .. })
    }
}

/// Pure rule: `enabled` is the flag, `space_id` and `space_kind` the thread's columns.
/// Blank values count as absent.
pub(crate) fn thread_is_contained_with(
    enabled: bool,
    space_id: Option<&str>,
    space_kind: Option<&str>,
) -> bool {
    if !enabled {
        return false;
    }
    let in_space = space_id.is_some_and(|id| !id.trim().is_empty());
    let personal = space_kind.is_some_and(|kind| kind.trim() == "personal");
    in_space && !personal
}

/// [`thread_is_contained_with`] under the process flag.
pub(crate) fn thread_is_contained(space_id: Option<&str>, space_kind: Option<&str>) -> bool {
    thread_is_contained_with(containment_on(), space_id, space_kind)
}

/// The `space_kind` a verified Control decision carried, normalised for storage. Anything
/// that would violate the column's length check, or is not a plain token, is dropped: the
/// thread then stays kind-less and contained.
pub(crate) fn normalize_claim_space_kind(raw: &str) -> Option<String> {
    let kind = raw.trim();
    let plain = !kind.is_empty()
        && kind.len() <= 32
        && kind
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    plain.then(|| kind.to_owned())
}

/// The reader scope of a thread. A thread that does not exist reads as `Personal`: there
/// is nothing of it to leak, and every caller reports a missing thread itself.
pub(crate) async fn reader_scope_for_thread(
    pool: &PgPool,
    thread_id: &str,
) -> Result<ReaderScope, sqlx::Error> {
    if !containment_on() {
        return Ok(ReaderScope::Personal);
    }
    let row: Option<(Option<String>, Option<String>, i64, String)> = sqlx::query_as(
        "SELECT space_id, space_kind, COALESCE(recipient_audience_revision, 0), \
                COALESCE(recipient_audience_hash, '') \
         FROM threads WHERE id = $1",
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await?;
    Ok(match row {
        Some((space_id, space_kind, audience_revision, audience_hash))
            if thread_is_contained(space_id.as_deref(), space_kind.as_deref()) =>
        {
            ReaderScope::Contained {
                space_ref: space_id.unwrap_or_default(),
                audience_revision,
                audience_hash,
            }
        }
        _ => ReaderScope::Personal,
    })
}

/// Fail-closed containment test for a read or retrieval path: a database error reads as
/// contained, so an outage can only withhold, never widen.
pub(crate) async fn thread_is_contained_by_id(pool: &PgPool, thread_id: &str) -> bool {
    match reader_scope_for_thread(pool, thread_id).await {
        Ok(scope) => scope.is_contained(),
        Err(_) => true,
    }
}

/// Topics that would write user-wide or tenant-wide rows (`dreaming::index_agent_memory`
/// maps them to `scope` `user` or `org`). Refused in a contained thread (C4).
pub(crate) fn topic_writes_beyond_thread(topic: &str) -> bool {
    matches!(
        topic.trim().to_ascii_uppercase().as_str(),
        "USER" | "POLICY" | "AGENT" | "WORKSPACE"
    )
}

/// Count one containment decision. `op` is a closed label set, never an id.
pub(crate) fn record_contained(op: &'static str) {
    metrics::counter!("mp_session_memory_contained_total", "op" => op).increment(1);
}
