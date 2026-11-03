//! Space memory writers (build plan EP-8 S6, dark): what a shared room states is kept for the
//! room, as `agent_memory` rows with `scope = 'space'` and `scope_id` = the Space, never for a
//! person.
//!
//! Three writers reach it, each only when `SESSION_SPACE_MEMORY_ENABLED` is the literal `true`:
//! the inline extraction of an append ([`write_inline`]), Dreaming's pass over a shared-room
//! thread ([`mine_room_turns`]) and `IndexMemory` with topic `SPACE` ([`index_space_memory`]).
//! With the switch off each of them does what it did before: a shared room learns nothing.
//!
//! The rules every writer applies, in [`stamp_for`] and the memory gate:
//! * only a contained thread (a shared room, `space::memory_scope`) writes Space rows; a personal
//!   Space keeps learning into its owner's memory as it does today;
//! * the thread must carry a Space decision and the message (or, for a save, the thread) an
//!   audience hash, both server state: a turn without them writes nothing (D2-461);
//! * a thread with a waived run writes nothing (D11), and a ZDR credential never reaches a writer
//!   (it cannot hold `session:write`, and `IndexMemory` refuses it first);
//! * the thread owner's memory gate still applies (paused, off, thread off, org disabled), so a
//!   room never learns where its owner's own chat would not;
//! * only the deterministic extractor runs, and only an explicit "remember that" becomes a room
//!   fact: a name or a preference is the speaker's, not the room's, and is dropped;
//! * no Letta mirror: Space rows stay in session-core's Postgres, since the semantic tier filters
//!   by equality only and is another processor.
//!
//! Recall, the owner's list and erasure of these rows are EP-8 S7; nothing here reads them.

use crate::dreaming::DreamMemoryCandidate;
use crate::write_policy;
use mp_ids::new_ulid;
use sqlx::{PgPool, Postgres, Transaction};

/// The switch. Default off; only the literal `true` turns it on.
pub(crate) const SPACE_MEMORY_ENABLED_ENV: &str = "SESSION_SPACE_MEMORY_ENABLED";

/// The `agent_memory.scope` a Space row carries.
pub(crate) const SPACE_SCOPE: &str = "space";
/// `source_label` of a row extracted from a room turn.
pub(crate) const SOURCE_ROOM_STATEMENT: &str = "room_statement";
/// `source_label` of a row saved explicitly in a room (`IndexMemory`, topic `SPACE`).
pub(crate) const SOURCE_MEMBER_SAVE: &str = "member_save";

/// `dream_runs.trigger` of an inline Space extraction, so the background loader does not mine
/// the same message again.
const TRIGGER_INLINE: &str = "append_space";
/// `dream_runs.trigger` of a background Space extraction.
const TRIGGER_BACKGROUND: &str = "space_scan";
/// The trigger Dreaming already writes for a shared-room message it does not mine.
const TRIGGER_SKIPPED: &str = "skipped_space_scope";
/// The trigger Dreaming already writes for an assistant message written after external content.
const TRIGGER_SKIPPED_EXTERNAL: &str = "skipped_external_content";

/// The deterministic extractor's key family for an explicit "remember that", and the text it
/// writes in front of the fact. The only family that is a room's statement rather than a fact
/// about the speaker.
const REMEMBER_KEY_PREFIX: &str = "user:remember:";
const REMEMBER_TEXT_PREFIX: &str = "User asked Verevon to remember: ";
const ROOM_REMEMBER_TEXT_PREFIX: &str = "This Space asked Verevon to remember: ";

/// Whether Space memory is switched on for this process.
pub(crate) fn enabled() -> bool {
    enabled_from(std::env::var(SPACE_MEMORY_ENABLED_ENV).ok().as_deref())
}

/// The strict reading: only the literal `true` enables it.
fn enabled_from(raw: Option<&str>) -> bool {
    raw.is_some_and(|value| value.trim() == "true")
}

/// Whether an `IndexMemory` topic asks for the room's memory. Read only with the switch on, so
/// with it off `SPACE` stays an ordinary thread note (`dreaming::scope_for_topic`).
pub(crate) fn is_space_topic(topic: &str) -> bool {
    topic.trim().eq_ignore_ascii_case("SPACE")
}

/// Why a Space write was not made. A closed set: it is the `reason` label of
/// `mp_session_space_memory_skipped_total` and the tail of [`Skip::refusal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Skip {
    SwitchOff,
    NotContained,
    Waived,
    NoDecision,
    NoAudience,
    Gate,
}

impl Skip {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::SwitchOff => "switch_off",
            Self::NotContained => "not_contained",
            Self::Waived => "waived",
            Self::NoDecision => "no_decision",
            Self::NoAudience => "no_audience",
            Self::Gate => "gate",
        }
    }

    /// Wire form of a refused save, `failed_precondition` message text. Names the rule only.
    pub(crate) fn refusal(self) -> String {
        format!("space_memory_not_written:{}", self.label())
    }
}

fn record_skip(skip: Skip) {
    metrics::counter!("mp_session_space_memory_skipped_total", "reason" => skip.label())
        .increment(1);
}

/// What a writer read of the thread and the turn, from server state only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TurnFacts {
    pub(crate) contained: bool,
    pub(crate) space_id: Option<String>,
    pub(crate) decision_ref: Option<String>,
    pub(crate) audience_hash: Option<String>,
    pub(crate) audience_revision: Option<i64>,
    pub(crate) waived: bool,
}

/// The Space and audience a row is stamped with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpaceStamp {
    pub(crate) space_id: String,
    pub(crate) audience_hash: String,
    pub(crate) audience_revision: i64,
}

fn present(value: Option<&String>) -> Option<&str> {
    value.map(|v| v.trim()).filter(|v| !v.is_empty())
}

/// The decision table, pure. Precedence: the switch, the room, the waiver, then the two pieces
/// of server state the stamp needs.
pub(crate) fn stamp_for(enabled: bool, facts: &TurnFacts) -> Result<SpaceStamp, Skip> {
    if !enabled {
        return Err(Skip::SwitchOff);
    }
    let Some(space_id) = present(facts.space_id.as_ref()).filter(|_| facts.contained) else {
        return Err(Skip::NotContained);
    };
    if facts.waived {
        return Err(Skip::Waived);
    }
    if present(facts.decision_ref.as_ref()).is_none() {
        return Err(Skip::NoDecision);
    }
    let (Some(audience_hash), Some(audience_revision)) =
        (present(facts.audience_hash.as_ref()), facts.audience_revision)
    else {
        return Err(Skip::NoAudience);
    };
    Ok(SpaceStamp {
        space_id: space_id.to_owned(),
        audience_hash: audience_hash.to_owned(),
        audience_revision,
    })
}

/// One row to write, already in the room's words and keyed by its Space.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SpaceRow {
    pub(crate) key: String,
    pub(crate) content: String,
    pub(crate) kind: &'static str,
    pub(crate) confidence: f64,
}

/// The key of a Space row. Space rows share owner '' and the unique index
/// `(org_id, scope, owner, key) WHERE session_id IS NULL`, so the Space id in the key is what
/// keeps two rooms apart.
fn space_key(space_id: &str, key: &str) -> String {
    format!("{space_id}/{key}")
}

/// Re-scopes the deterministic extractor's candidates to the room. Only an explicit "remember
/// that" is kept: a name or a preference describes the speaker and an assistant artifact note
/// describes the thread, so neither is the room's. Nothing comes back with scope `user`.
pub(crate) fn room_rows(space_id: &str, candidates: &[DreamMemoryCandidate]) -> Vec<SpaceRow> {
    candidates
        .iter()
        .filter(|candidate| !candidate.inferred)
        .filter_map(|candidate| {
            let fragment = candidate.key.strip_prefix(REMEMBER_KEY_PREFIX)?;
            let fact = candidate
                .content
                .strip_prefix(REMEMBER_TEXT_PREFIX)
                .unwrap_or(&candidate.content);
            Some(SpaceRow {
                key: space_key(space_id, &format!("space:remember:{fragment}")),
                content: format!("{ROOM_REMEMBER_TEXT_PREFIX}{fact}"),
                kind: candidate.kind,
                confidence: candidate.confidence,
            })
        })
        .collect()
}

type FactsRow = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    bool,
);

fn facts_from_row(row: Option<FactsRow>) -> TurnFacts {
    let Some((space_id, space_kind, decision_ref, audience_hash, audience_revision, waived)) = row
    else {
        return TurnFacts::default();
    };
    TurnFacts {
        contained: crate::space::memory_scope::thread_is_contained(
            space_id.as_deref(),
            space_kind.as_deref(),
        ),
        space_id,
        decision_ref,
        audience_hash,
        audience_revision,
        waived,
    }
}

/// The facts of one message: the thread's Space and decision, the message's own audience (what
/// the room was when it was said), and whether any run of the thread was waived (the same test
/// as Dreaming's loader).
async fn message_facts(
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    thread_id: &str,
    message_id: &str,
) -> Result<TurnFacts, sqlx::Error> {
    let row: Option<FactsRow> = sqlx::query_as(
        "SELECT t.space_id, t.space_kind, t.space_decision_ref, \
                m.recipient_audience_hash, m.recipient_audience_revision, \
                EXISTS (SELECT 1 FROM runs wr WHERE wr.thread_id = t.id AND wr.retention_class = 'zdr_waived') \
         FROM messages m JOIN threads t ON t.id = m.thread_id \
         WHERE m.id = $1 AND t.id = $2 AND t.org_id = $3",
    )
    .bind(message_id)
    .bind(thread_id)
    .bind(org_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(facts_from_row(row))
}

/// The facts of a thread, for a save that has no message: the thread's current audience.
async fn thread_facts(
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    owner_user_id: &str,
    thread_id: &str,
) -> Result<TurnFacts, sqlx::Error> {
    let row: Option<FactsRow> = sqlx::query_as(
        "SELECT t.space_id, t.space_kind, t.space_decision_ref, \
                t.recipient_audience_hash, t.recipient_audience_revision, \
                EXISTS (SELECT 1 FROM runs wr WHERE wr.thread_id = t.id AND wr.retention_class = 'zdr_waived') \
         FROM threads t WHERE t.id = $1 AND t.org_id = $2 AND t.user_id = $3",
    )
    .bind(thread_id)
    .bind(org_id)
    .bind(owner_user_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(facts_from_row(row))
}

/// Insert `row`, or refresh the current row that holds its key. A row stated again under a new
/// audience takes the new stamp: it was said again in front of the room as it is now. A key held
/// only by a retired row writes nothing (the empty answer); no Space writer retires rows today.
async fn upsert_space_row(
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    stamp: &SpaceStamp,
    row: &SpaceRow,
    source_links: &[String],
    source_label: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO agent_memory \
         (id, org_id, session_id, scope, scope_id, key, content, kind, confidence, owner, source_links, \
          review_state, audience_hash, audience_revision, source_label) \
         VALUES ($1, $2, NULL, 'space', $3, $4, $5, $6, $7, '', $8, 'accepted', $9, $10, $11) \
         ON CONFLICT (org_id, scope, owner, key) WHERE session_id IS NULL \
         DO UPDATE SET \
           content = EXCLUDED.content, \
           kind = EXCLUDED.kind, \
           confidence = GREATEST(agent_memory.confidence, EXCLUDED.confidence), \
           source_links = EXCLUDED.source_links, \
           review_state = 'accepted', \
           scope_id = EXCLUDED.scope_id, \
           audience_hash = EXCLUDED.audience_hash, \
           audience_revision = EXCLUDED.audience_revision, \
           source_label = EXCLUDED.source_label, \
           updated_at = now() \
         WHERE agent_memory.is_latest \
         RETURNING id",
    )
    .bind(new_ulid())
    .bind(org_id)
    .bind(&stamp.space_id)
    .bind(&row.key)
    .bind(&row.content)
    .bind(row.kind)
    .bind(row.confidence)
    .bind(source_links)
    .bind(&stamp.audience_hash)
    .bind(stamp.audience_revision)
    .bind(source_label)
    .fetch_optional(&mut **tx)
    .await
}

/// Writes the rows of one message under the content policy, as Dreaming's own writer does: a
/// refused row is skipped, never fatal. Returns how many were written.
async fn write_message_rows(
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    thread_id: &str,
    message_id: &str,
    stamp: &SpaceStamp,
    rows: &[SpaceRow],
) -> Result<i64, sqlx::Error> {
    let source_links = [format!("thread:{thread_id}"), format!("message:{message_id}")];
    let mut saved = 0i64;
    for row in rows {
        if write_policy::gate(
            write_policy::Mode::configured(),
            &row.content,
            SPACE_SCOPE,
            write_policy::Origin::Dreaming,
        )
        .is_err()
        {
            continue;
        }
        let written =
            upsert_space_row(tx, org_id, stamp, row, &source_links, SOURCE_ROOM_STATEMENT).await?;
        saved += i64::from(written.is_some());
    }
    Ok(saved)
}

/// The rows a message would give the room, after the stamp and the owner's gate; `None` when it
/// gives nothing (the skip is counted).
async fn rows_for_message(
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    owner_user_id: &str,
    thread_id: &str,
    message_id: &str,
    role: &str,
    content: &str,
) -> Result<Option<(SpaceStamp, Vec<SpaceRow>)>, sqlx::Error> {
    let facts = message_facts(tx, org_id, thread_id, message_id).await?;
    let stamp = match stamp_for(true, &facts) {
        Ok(stamp) => stamp,
        Err(skip) => {
            record_skip(skip);
            return Ok(None);
        }
    };
    let candidates = crate::dreaming::extract_memory_candidates(role, content, thread_id);
    let rows = room_rows(&stamp.space_id, &candidates);
    if rows.is_empty() {
        return Ok(Some((stamp, rows)));
    }
    // The owner's gate under the owner's row lock, as `persist_candidates` asks it: a paused or
    // off owner, a thread set off, an org with memory off, or a message from before a resume.
    if !crate::memory_control::permits_extraction(tx, org_id, owner_user_id, message_id).await? {
        record_skip(Skip::Gate);
        return Ok(None);
    }
    Ok(Some((stamp, rows)))
}

/// Inline extraction of one shared-room append, inside the append's transaction. `enabled` is
/// [`enabled`] at the call site; off, it returns before reading anything, so the append is what
/// it was before. Records the message as dreamt when it wrote, so the background pass does not
/// mine it again. Returns how many rows were written.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn write_inline(
    enabled: bool,
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    owner_user_id: &str,
    thread_id: &str,
    message_id: &str,
    role: &str,
    content: &str,
) -> Result<i64, sqlx::Error> {
    if !enabled {
        return Ok(0);
    }
    let Some((stamp, rows)) =
        rows_for_message(tx, org_id, owner_user_id, thread_id, message_id, role, content).await?
    else {
        return Ok(0);
    };
    if rows.is_empty() {
        return Ok(0);
    }
    let saved = write_message_rows(tx, org_id, thread_id, message_id, &stamp, &rows).await?;
    crate::dreaming::record_dream_run(
        tx,
        org_id,
        thread_id,
        TRIGGER_INLINE,
        i64::try_from(rows.len()).unwrap_or(i64::MAX),
        saved,
        Some(message_id),
    )
    .await?;
    Ok(saved)
}

/// One pending message of a shared-room thread, as Dreaming loaded it.
pub(crate) struct RoomTurn<'a> {
    pub(crate) message_id: &'a str,
    pub(crate) role: &'a str,
    pub(crate) content: &'a str,
    /// An assistant message written after web, browser or third-party content: never mined.
    pub(crate) external_content: bool,
}

/// Dreaming's pass over a shared-room thread with the switch on. Deterministic extraction only:
/// the paid extractor and the grounding classifier never see room content in this slice. Every
/// message is ledgered in `dream_runs`, written or skipped, so the pending query moves on.
/// Returns how many messages were handled.
pub(crate) async fn mine_room_turns(
    pool: &PgPool,
    org_id: &str,
    owner_user_id: &str,
    thread_id: &str,
    turns: &[RoomTurn<'_>],
) -> Result<i64, sqlx::Error> {
    let mut processed = 0i64;
    for turn in turns {
        let mut tx = pool.begin().await?;
        if turn.external_content {
            crate::dreaming::record_dream_run(&mut tx, org_id, thread_id, TRIGGER_SKIPPED_EXTERNAL, 0, 0, Some(turn.message_id)).await?;
        } else {
            match rows_for_message(&mut tx, org_id, owner_user_id, thread_id, turn.message_id, turn.role, turn.content).await? {
                None => {
                    crate::dreaming::record_dream_run(&mut tx, org_id, thread_id, TRIGGER_SKIPPED, 0, 0, Some(turn.message_id)).await?;
                }
                Some((stamp, rows)) => {
                    let saved = write_message_rows(&mut tx, org_id, thread_id, turn.message_id, &stamp, &rows).await?;
                    crate::dreaming::record_dream_run(
                        &mut tx,
                        org_id,
                        thread_id,
                        TRIGGER_BACKGROUND,
                        i64::try_from(rows.len()).unwrap_or(i64::MAX),
                        saved,
                        Some(turn.message_id),
                    )
                    .await?;
                }
            }
        }
        tx.commit().await?;
        processed += 1;
    }
    Ok(processed)
}

/// An explicit save in a shared room (`IndexMemory`, topic `SPACE`), stamped with the thread's
/// current audience. The caller has already refused a waived credential and run the content
/// policy for scope `space`. Returns the row id, or why nothing was written.
pub(crate) async fn index_space_memory(
    pool: &PgPool,
    org_id: &str,
    owner_user_id: &str,
    thread_id: &str,
    content: &str,
) -> Result<Result<String, Skip>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let facts = thread_facts(&mut tx, org_id, owner_user_id, thread_id).await?;
    let stamp = match stamp_for(true, &facts) {
        Ok(stamp) => stamp,
        Err(skip) => {
            tx.rollback().await?;
            record_skip(skip);
            return Ok(Err(skip));
        }
    };
    // Under the owner's row lock with the thread's mode, as `dreaming::index_agent_memory` asks.
    if !crate::memory_control::resolve_in_tx_for_thread(&mut tx, false, org_id, owner_user_id, thread_id)
        .await?
        .generation_allowed
    {
        tx.rollback().await?;
        record_skip(Skip::Gate);
        return Ok(Err(Skip::Gate));
    }
    let content = crate::dreaming::collapse_whitespace(content);
    let row = SpaceRow {
        key: space_key(
            &stamp.space_id,
            &format!("space:saved:{}", crate::dreaming::stable_fragment(&content)),
        ),
        content,
        kind: "fact",
        confidence: 0.9,
    };
    let source_links = [format!("thread:{thread_id}"), "tool:save_memory".to_owned()];
    let written =
        upsert_space_row(&mut tx, org_id, &stamp, &row, &source_links, SOURCE_MEMBER_SAVE).await?;
    tx.commit().await?;
    Ok(Ok(written.unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> TurnFacts {
        TurnFacts {
            contained: true,
            space_id: Some("space-1".to_owned()),
            decision_ref: Some("decision-1".to_owned()),
            audience_hash: Some("sha256:audience-1".to_owned()),
            audience_revision: Some(1),
            waived: false,
        }
    }

    fn production_source(source: &str) -> &str {
        source.split("#[cfg(test)]").next().unwrap_or(source)
    }

    #[test]
    fn enabled_only_by_the_literal_true() {
        assert!(enabled_from(Some("true")));
        assert!(enabled_from(Some(" true ")));
        for off in [None, Some(""), Some("1"), Some("TRUE"), Some("yes"), Some("on"), Some("false")] {
            assert!(!enabled_from(off), "{off:?} must leave Space memory off");
        }
    }

    #[test]
    fn stamp_table() {
        assert_eq!(
            stamp_for(true, &facts()),
            Ok(SpaceStamp {
                space_id: "space-1".to_owned(),
                audience_hash: "sha256:audience-1".to_owned(),
                audience_revision: 1,
            })
        );
        assert_eq!(stamp_for(false, &facts()), Err(Skip::SwitchOff));
        let cases: [(fn(&mut TurnFacts), Skip); 7] = [
            (|f| f.contained = false, Skip::NotContained),
            (|f| f.space_id = None, Skip::NotContained),
            (|f| f.space_id = Some("  ".to_owned()), Skip::NotContained),
            (|f| f.waived = true, Skip::Waived),
            (|f| f.decision_ref = Some(String::new()), Skip::NoDecision),
            (|f| f.audience_hash = None, Skip::NoAudience),
            (|f| f.audience_revision = None, Skip::NoAudience),
        ];
        for (change, skip) in cases {
            let mut case = facts();
            change(&mut case);
            assert_eq!(stamp_for(true, &case), Err(skip), "{case:?}");
        }
        // The switch is asked first: off, nothing else is even considered.
        assert_eq!(stamp_for(false, &TurnFacts::default()), Err(Skip::SwitchOff));
    }

    #[test]
    fn skip_labels_are_closed_tokens() {
        let labels: Vec<&str> = [
            Skip::SwitchOff,
            Skip::NotContained,
            Skip::Waived,
            Skip::NoDecision,
            Skip::NoAudience,
            Skip::Gate,
        ]
        .iter()
        .map(|skip| skip.label())
        .collect();
        assert_eq!(
            labels,
            ["switch_off", "not_contained", "waived", "no_decision", "no_audience", "gate"]
        );
        assert_eq!(Skip::Waived.refusal(), "space_memory_not_written:waived");
    }

    #[test]
    fn space_candidate_never_user_scope() {
        let candidates = crate::dreaming::extract_memory_candidates(
            "user",
            "Jeg heter Ima. Jeg foretrekker korte svar. Husk at vi leverer til Bergen på fredager.",
            "thread-1",
        );
        assert!(candidates.iter().any(|c| c.scope == "user"), "the extractor still finds personal facts");
        let rows = room_rows("space-1", &candidates);
        assert_eq!(rows.len(), 1, "only the explicit remember is the room's: {rows:?}");
        let row = &rows[0];
        assert!(row.key.starts_with("space-1/space:remember:"), "{}", row.key);
        assert!(row.content.starts_with(ROOM_REMEMBER_TEXT_PREFIX), "{}", row.content);
        assert!(row.content.contains("Bergen"));
        assert!(!row.content.contains("Ima") && !row.content.contains("User"), "{}", row.content);
    }

    #[test]
    fn two_spaces_never_share_a_key() {
        let candidates = crate::dreaming::extract_memory_candidates(
            "user",
            "Husk at fakturaen går ut fredag.",
            "thread-1",
        );
        let a = room_rows("space-a", &candidates);
        let b = room_rows("space-b", &candidates);
        assert_eq!(a.len(), 1);
        assert_ne!(a[0].key, b[0].key);
    }

    #[test]
    fn assistant_and_inferred_candidates_never_become_room_rows() {
        let assistant = crate::dreaming::extract_memory_candidates(
            "assistant",
            "Generated image artifact: art-123.",
            "thread-1",
        );
        assert!(room_rows("space-1", &assistant).is_empty());
        let inferred = DreamMemoryCandidate {
            scope: "user",
            session_id: None,
            key: "user:remember:x".to_owned(),
            content: "User asked Verevon to remember: x.".to_owned(),
            kind: "fact",
            confidence: 0.5,
            inferred: true,
        };
        assert!(room_rows("space-1", &[inferred]).is_empty());
    }

    #[test]
    fn scope_for_topic_space() {
        // With the switch off a SPACE save is still an ordinary thread note: nothing changed there.
        assert_eq!(
            crate::dreaming::scope_for_topic(&crate::dreaming::normalize_topic("SPACE")),
            "thread"
        );
        assert!(is_space_topic("SPACE"));
        assert!(is_space_topic(" space "));
        for other in ["USER", "MEMORY", "", "SPACES", "POLICY"] {
            assert!(!is_space_topic(other), "{other}");
        }
        // The content policy lets the model and Dreaming write the room's scope; only org is reserved.
        for origin in [write_policy::Origin::ModelTool, write_policy::Origin::Dreaming] {
            assert!(write_policy::check("Vi leverer til Bergen på fredager.", SPACE_SCOPE, origin).is_ok());
        }
    }

    #[test]
    fn space_rows_not_mirrored_to_letta() {
        let source = production_source(include_str!("space_memory.rs"));
        for forbidden in ["LettaMemoryAdapter", "index_detailed", "sync_persisted_candidates", "letta_adapter"] {
            assert!(!source.contains(forbidden), "a Space writer must not reach the semantic mirror: {forbidden}");
        }
    }

    #[test]
    fn the_switch_off_path_reads_nothing() {
        let source = production_source(include_str!("space_memory.rs"));
        let start = source.find("pub(crate) async fn write_inline(").expect("write_inline");
        let body = &source[start..];
        let off = body.find("if !enabled {").expect("the switch is asked");
        let first_read = body.find("rows_for_message(").expect("the read");
        assert!(off < first_read, "the off path returns before any query");
    }

    #[test]
    fn every_writer_is_behind_the_switch() {
        let grpc = include_str!("grpc.rs");
        let call = grpc.find("crate::space_memory::write_inline(").expect("the append calls the writer");
        let args: String = grpc[call..].chars().take(160).collect();
        assert!(
            args.contains("crate::space_memory::enabled()"),
            "the append writes Space rows only with the switch on"
        );
        let dreaming = include_str!("dreaming.rs");
        assert!(dreaming.contains("if last.contained && crate::space_memory::enabled() {"));
        let index = include_str!("memory_grpc.rs");
        assert!(index.contains("contained && crate::space_memory::enabled() && crate::space_memory::is_space_topic(&req.topic)"));
    }

    #[test]
    fn migration_closes_the_scope_set_and_has_a_revert() {
        let migration = include_str!("../migrations/0103_agent_memory_scope_key.sql");
        assert!(migration.contains(
            "scope IN ('run', 'thread', 'session', 'workspace', 'user', 'org', 'global', 'space', 'agent')"
        ));
        assert_eq!(migration.matches(") NOT VALID;").count(), 4, "every constraint is added NOT VALID");
        assert!(!migration.contains("VALIDATE CONSTRAINT agent_memory_scope_chk;"), "validation is a separate step");
        assert!(!migration.contains("DROP COLUMN"), "the migration removes nothing");
        let revert = include_str!("../migrations/revert/0103_agent_memory_scope_key.down.sql");
        for column in ["scope_id", "audience_hash", "audience_revision", "source_label"] {
            assert!(revert.contains(&format!("DROP COLUMN IF EXISTS {column}")), "{column}");
        }
    }

    // ---- DB tier: `#[ignore]`d, needs DATABASE_URL to a disposable `*test*` Postgres ----------

    use crate::space::test_support;

    const OWNER: &str = "owner-1";
    const REMEMBER: &str = "Husk at vi leverer til Bergen på fredager.";

    struct Seeded {
        pool: PgPool,
        org: String,
        space: String,
        thread: String,
        message: String,
    }

    async fn seed(content: &str) -> Option<Seeded> {
        let pool = test_support::connect().await?;
        let nonce = new_ulid();
        let org = format!("space-memory-org-{nonce}");
        let space = format!("space-memory-{nonce}");
        let thread = format!("space-memory-thread-{nonce}");
        let message = format!("space-memory-msg-{nonce}");
        test_support::seed_space_thread(&pool, &org, &space, &thread, OWNER).await;
        test_support::seed_space_message(&pool, &space, &thread, &message, "user", Some(OWNER)).await;
        sqlx::query("UPDATE messages SET content = $2 WHERE id = $1")
            .bind(&message)
            .bind(content)
            .execute(&pool)
            .await
            .expect("set content");
        Some(Seeded { pool, org, space, thread, message })
    }

    async fn cleanup(seeded: &Seeded) {
        for sql in [
            "DELETE FROM agent_memory WHERE org_id = $1",
            "DELETE FROM dream_runs WHERE org_id = $1",
            "DELETE FROM user_memory_controls WHERE org_id = $1",
        ] {
            sqlx::query(sql).bind(&seeded.org).execute(&seeded.pool).await.ok();
        }
        test_support::clean_space(&seeded.pool, &seeded.org, &seeded.space).await;
    }

    async fn inline(seeded: &Seeded, enabled: bool) -> i64 {
        let mut tx = seeded.pool.begin().await.unwrap();
        let saved = write_inline(enabled, &mut tx, &seeded.org, OWNER, &seeded.thread, &seeded.message, "user", REMEMBER)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        saved
    }

    async fn count(seeded: &Seeded, sql: &str) -> i64 {
        sqlx::query_scalar(sql).bind(&seeded.org).fetch_one(&seeded.pool).await.unwrap()
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres whose name contains \"test\""]
    async fn space_turn_writes_space_row_with_audience_stamp() {
        let Some(seeded) = seed(REMEMBER).await else { return };
        assert_eq!(inline(&seeded, true).await, 1);
        let (scope, scope_id, owner, hash, revision, label, session): (
            String,
            Option<String>,
            String,
            Option<String>,
            Option<i64>,
            Option<String>,
            Option<String>,
        ) = sqlx::query_as(
            "SELECT scope, scope_id, owner, audience_hash, audience_revision, source_label, session_id \
             FROM agent_memory WHERE org_id = $1",
        )
        .bind(&seeded.org)
        .fetch_one(&seeded.pool)
        .await
        .unwrap();
        assert_eq!(scope, "space");
        assert_eq!(scope_id.as_deref(), Some(seeded.space.as_str()));
        assert_eq!(owner, "");
        assert_eq!(hash.as_deref(), Some("sha256:audience-1"));
        assert_eq!(revision, Some(1));
        assert_eq!(label.as_deref(), Some(SOURCE_ROOM_STATEMENT));
        assert_eq!(session, None);
        assert_eq!(count(&seeded, "SELECT COUNT(*) FROM agent_memory WHERE org_id = $1 AND scope = 'user'").await, 0);
        assert_eq!(
            count(&seeded, "SELECT COUNT(*) FROM dream_runs WHERE org_id = $1 AND trigger = 'append_space'").await,
            1,
            "the background pass must not mine the message again"
        );
        cleanup(&seeded).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres whose name contains \"test\""]
    async fn space_turn_without_decision_writes_nothing() {
        let Some(seeded) = seed(REMEMBER).await else { return };
        sqlx::query("UPDATE threads SET space_decision_ref = '' WHERE id = $1")
            .bind(&seeded.thread)
            .execute(&seeded.pool)
            .await
            .unwrap();
        assert_eq!(inline(&seeded, true).await, 0);
        assert_eq!(count(&seeded, "SELECT COUNT(*) FROM agent_memory WHERE org_id = $1").await, 0);
        cleanup(&seeded).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres whose name contains \"test\""]
    async fn waived_space_thread_not_mined() {
        let Some(seeded) = seed(REMEMBER).await else { return };
        let run = format!("space-memory-run-{}", new_ulid());
        test_support::seed_space_run(&seeded.pool, &seeded.org, &seeded.space, &seeded.thread, &run, "completed", "zdr_waived").await;
        let turn = RoomTurn { message_id: &seeded.message, role: "user", content: REMEMBER, external_content: false };
        let processed = mine_room_turns(&seeded.pool, &seeded.org, OWNER, &seeded.thread, &[turn]).await.unwrap();
        assert_eq!(processed, 1, "the message is ledgered so the loader moves on");
        assert_eq!(count(&seeded, "SELECT COUNT(*) FROM agent_memory WHERE org_id = $1").await, 0);
        assert_eq!(
            count(&seeded, "SELECT COUNT(*) FROM dream_runs WHERE org_id = $1 AND trigger = 'skipped_space_scope'").await,
            1
        );
        assert_eq!(inline(&seeded, true).await, 0, "the inline writer refuses a waived thread too");
        cleanup(&seeded).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres whose name contains \"test\""]
    async fn switch_off_space_writes_nothing() {
        let Some(seeded) = seed(REMEMBER).await else { return };
        assert_eq!(inline(&seeded, false).await, 0);
        assert_eq!(count(&seeded, "SELECT COUNT(*) FROM agent_memory WHERE org_id = $1").await, 0);
        assert_eq!(count(&seeded, "SELECT COUNT(*) FROM dream_runs WHERE org_id = $1").await, 0);
        assert_eq!(
            count(&seeded, "SELECT COUNT(*) FROM user_memory_controls WHERE org_id = $1").await,
            0,
            "the off path takes no lock and creates no row"
        );
        cleanup(&seeded).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres whose name contains \"test\""]
    async fn background_pass_writes_the_room_row_and_skips_external_content() {
        let Some(seeded) = seed(REMEMBER).await else { return };
        let turns = [
            RoomTurn { message_id: &seeded.message, role: "user", content: REMEMBER, external_content: false },
        ];
        assert_eq!(mine_room_turns(&seeded.pool, &seeded.org, OWNER, &seeded.thread, &turns).await.unwrap(), 1);
        assert_eq!(count(&seeded, "SELECT COUNT(*) FROM agent_memory WHERE org_id = $1 AND scope = 'space'").await, 1);
        assert_eq!(count(&seeded, "SELECT COUNT(*) FROM agent_memory WHERE org_id = $1 AND scope <> 'space'").await, 0);
        let tainted = [
            RoomTurn { message_id: &seeded.message, role: "assistant", content: REMEMBER, external_content: true },
        ];
        mine_room_turns(&seeded.pool, &seeded.org, OWNER, &seeded.thread, &tainted).await.unwrap();
        assert_eq!(
            count(&seeded, "SELECT COUNT(*) FROM dream_runs WHERE org_id = $1 AND trigger = 'skipped_external_content'").await,
            1
        );
        cleanup(&seeded).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres whose name contains \"test\""]
    async fn space_save_is_stamped_with_the_thread_audience() {
        let Some(seeded) = seed("canary").await else { return };
        let id = index_space_memory(&seeded.pool, &seeded.org, OWNER, &seeded.thread, "Vi leverer til Bergen på fredager.")
            .await
            .unwrap()
            .expect("written");
        assert!(!id.is_empty());
        let (label, hash): (Option<String>, Option<String>) =
            sqlx::query_as("SELECT source_label, audience_hash FROM agent_memory WHERE id = $1")
                .bind(&id)
                .fetch_one(&seeded.pool)
                .await
                .unwrap();
        assert_eq!(label.as_deref(), Some(SOURCE_MEMBER_SAVE));
        assert_eq!(hash.as_deref(), Some("sha256:audience-1"));
        let refused = index_space_memory(&seeded.pool, &seeded.org, "someone-else", &seeded.thread, "x")
            .await
            .unwrap();
        assert_eq!(refused, Err(Skip::NotContained), "another user's thread is not found, so nothing is written");
        cleanup(&seeded).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres whose name contains \"test\""]
    async fn migration_constraints_reject_unknown_scope() {
        let Some(pool) = test_support::connect().await else { return };
        let org = format!("space-memory-chk-{}", new_ulid());
        let insert = |scope: &'static str, scope_id: Option<&'static str>, hash: Option<&'static str>, label: Option<&'static str>| {
            let pool = pool.clone();
            let org = org.clone();
            async move {
                sqlx::query(
                    "INSERT INTO agent_memory (id, org_id, scope, scope_id, key, content, kind, confidence, owner, \
                     source_links, review_state, audience_hash, audience_revision, source_label) \
                     VALUES ($1, $2, $3, $4, $5, 'c', 'fact', 0.5, '', '{}', 'accepted', $6, 1, $7)",
                )
                .bind(new_ulid())
                .bind(&org)
                .bind(scope)
                .bind(scope_id)
                .bind(new_ulid())
                .bind(hash)
                .bind(label)
                .execute(&pool)
                .await
            }
        };
        assert!(insert("bogus", None, None, None).await.is_err(), "an unknown scope is refused");
        assert!(insert("space", None, Some("h"), Some("room_statement")).await.is_err(), "space needs a scope_id");
        assert!(insert("org", Some("space-1"), None, None).await.is_err(), "only space and agent carry a scope_id");
        assert!(insert("space", Some("space-1"), None, Some("room_statement")).await.is_err(), "space needs its audience");
        assert!(insert("space", Some("space-1"), Some("h"), Some("guess")).await.is_err(), "source_label is closed");
        assert!(insert("space", Some("space-1"), Some("h"), Some("room_statement")).await.is_ok());
        assert!(insert("org", None, None, None).await.is_ok(), "every older kind still writes");
        sqlx::query("DELETE FROM agent_memory WHERE org_id = $1").bind(&org).execute(&pool).await.ok();
    }
}
