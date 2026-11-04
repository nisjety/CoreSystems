//! The standing user profile (build plan EP-8 S4, decisions D7 and D8).
//!
//! One short text per person, built deterministically from what they have accepted in memory and
//! editable by them. The builder reads session-core's own `agent_memory` rows only: the person's
//! accepted, current, unexpired `scope = 'user'` rows. It never reads org, thread, Space or agent rows,
//! and never letta-bridge blocks (integration round A, C8), so the profile cannot carry anything the
//! person's own memory list does not show them.
//!
//! The profile follows the memory decision exactly (D8): paused keeps it listed and editable but no
//! rebuild runs; off and an org that disabled memory hide it; a reset deletes it; a ZDR or waived
//! caller reads nothing and writes nothing. Deleting a memory drops its line from a built profile in
//! the same transaction; an edited profile is kept as written and reports how many of its sources were
//! deleted since.
//!
//! Default off behind `SESSION_USER_PROFILE_ENABLED`. With it off the RPCs refuse and Dreaming never
//! rebuilds, but erasure (reset, delete, organization purge) always reaches the table, so a profile
//! written while the switch was on is never left behind by turning it off.

use crate::dreaming::{collapse_whitespace, MemoryProvenance};
use crate::memory_control::MemoryDecision;
use chrono::{DateTime, Utc};
use mp_ids::new_ulid;
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::HashSet;

pub(crate) const ENABLED_ENV: &str = "SESSION_USER_PROFILE_ENABLED";
/// The profile's length bound, in characters. The table's CHECK holds the same number.
pub(crate) const MAX_CHARS: usize = 1400;
/// An inferred memory reaches the profile only at this confidence or above (D7).
pub(crate) const INFERRED_MIN_CONFIDENCE: f64 = 0.8;
/// The refusal while the switch is off; the SPA hides the card on this code.
pub(crate) const NOT_ENABLED: &str = "profile_not_enabled";
/// The refusal of an edit made against an older revision.
pub(crate) const REVISION_STALE: &str = "profile_revision_stale";
/// The ZDR feature key a refused write names (`zdr-refusals.json`).
pub(crate) const ZDR_FEATURE: &str = "user_profile";
pub(crate) const ORIGIN_BUILT: &str = "built";
pub(crate) const ORIGIN_EDITED: &str = "edited";
/// More rows than this cannot fit in 1400 characters anyway; the newest are read.
const MAX_SOURCE_ROWS: i64 = 200;

/// Whether the profile is switched on for this process: `true` and nothing else.
pub(crate) fn enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// One memory row as the builder sees it.
#[derive(Debug, Clone)]
pub(crate) struct SourceRow {
    pub(crate) id: String,
    pub(crate) scope: String,
    pub(crate) key: String,
    pub(crate) kind: String,
    pub(crate) content: String,
    pub(crate) confidence: f64,
    pub(crate) provenance: MemoryProvenance,
    /// Learned in a shared room before containment existed: listed to the owner, never used.
    pub(crate) withheld: bool,
    pub(crate) updated_at: DateTime<Utc>,
}

/// What the builder made: the text and, line for line, the memory each line came from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Built {
    pub(crate) content: String,
    pub(crate) source_memory_ids: Vec<String>,
}

/// A stored profile.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Profile {
    pub(crate) content: String,
    pub(crate) source_memory_ids: Vec<String>,
    pub(crate) origin: String,
    pub(crate) revision: i64,
    pub(crate) updated_at: DateTime<Utc>,
    /// For an edited profile, how many source memories no longer exist; zero for a built one.
    pub(crate) sources_deleted_since_edit: i64,
}

/// Order of the kinds in the text. Any other kind (artifact summaries, graph edges, policy text) is
/// not about the person and stays out.
fn kind_rank(kind: &str) -> Option<u8> {
    match kind {
        "preference" => Some(0),
        "fact" => Some(1),
        "instruction" => Some(2),
        _ => None,
    }
}

/// How strongly a row speaks for the person: what they said before what was inferred about them.
/// `None` keeps the row out. An unknown-provenance row is the phrase matcher's (the person's own
/// words, written before provenance was recorded); the loader admits only rows with a source link, so
/// a row written around the extractors with no trace of where it came from never counts (risk 4).
fn tier(row: &SourceRow) -> Option<u8> {
    if row.scope != "user" || row.withheld {
        return None;
    }
    match row.provenance {
        MemoryProvenance::Stated => Some(0),
        MemoryProvenance::Unknown => Some(1),
        MemoryProvenance::Inferred if row.confidence >= INFERRED_MIN_CONFIDENCE => Some(2),
        MemoryProvenance::Inferred => None,
    }
}

/// The profile for `rows`. Pure, so the same rows always give the same bytes.
///
/// Rows are taken in priority order (stated, then unknown, then inferred; newest first; id as the
/// tie-break), one per key and one per text, until the next line would pass [`MAX_CHARS`]: a line is
/// never cut. The kept lines are then written grouped by kind (preference, fact, instruction), each
/// group in the same priority order.
pub(crate) fn build(rows: &[SourceRow]) -> Built {
    let mut eligible: Vec<(u8, u8, &SourceRow, String)> = rows
        .iter()
        .filter_map(|row| {
            let tier = tier(row)?;
            let rank = kind_rank(&row.kind)?;
            let text = collapse_whitespace(&row.content);
            (!text.is_empty()).then_some((tier, rank, row, text))
        })
        .collect();
    eligible.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| b.2.updated_at.cmp(&a.2.updated_at))
            .then_with(|| a.2.id.cmp(&b.2.id))
    });

    let mut keys: HashSet<&str> = HashSet::new();
    let mut texts: HashSet<String> = HashSet::new();
    let mut used = 0usize;
    let mut kept: Vec<(u8, usize, String, String)> = Vec::new();
    for (_, rank, row, text) in eligible {
        if !keys.insert(row.key.as_str()) || !texts.insert(text.to_lowercase()) {
            continue;
        }
        let line = format!("- {text}");
        let cost = line.chars().count() + usize::from(!kept.is_empty());
        if used + cost > MAX_CHARS {
            break;
        }
        used += cost;
        let priority = kept.len();
        kept.push((rank, priority, row.id.clone(), line));
    }
    kept.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    Built {
        content: kept
            .iter()
            .map(|(_, _, _, line)| line.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        source_memory_ids: kept.into_iter().map(|(_, _, id, _)| id).collect(),
    }
}

/// A built profile with the lines of `dropped` memories taken out. A built profile has one line per
/// source; when the two disagree (a row edited by hand in the database) nothing is kept, because a
/// line whose source cannot be told must not outlive a deletion.
pub(crate) fn without_sources(content: &str, sources: &[String], dropped: &[String]) -> Built {
    let lines: Vec<&str> = if content.is_empty() {
        Vec::new()
    } else {
        content.split('\n').collect()
    };
    if lines.len() != sources.len() {
        return Built::default();
    }
    let (kept_lines, kept_ids): (Vec<&str>, Vec<String>) = lines
        .into_iter()
        .zip(sources.iter())
        .filter(|(_, id)| !dropped.contains(id))
        .map(|(line, id)| (line, id.clone()))
        .unzip();
    Built {
        content: kept_lines.join("\n"),
        source_memory_ids: kept_ids,
    }
}

type ProfileRow = (String, Vec<String>, String, i64, DateTime<Utc>, i64);

const PROFILE_COLUMNS: &str = "p.content, p.source_memory_ids, p.origin, p.revision, p.updated_at, \
     CASE WHEN p.origin = 'edited' THEN ( \
         SELECT count(*) FROM unnest(p.source_memory_ids) AS s(id) \
         WHERE NOT EXISTS (SELECT 1 FROM agent_memory m WHERE m.id = s.id AND m.org_id = p.org_id)) \
     ELSE 0 END";

fn profile_from(row: ProfileRow) -> Profile {
    let (content, source_memory_ids, origin, revision, updated_at, deleted) = row;
    Profile {
        content,
        source_memory_ids,
        origin,
        revision,
        updated_at,
        sources_deleted_since_edit: deleted,
    }
}

async fn read<'e, E>(executor: E, org: &str, user: &str) -> Result<Option<Profile>, sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    let sql = format!(
        "SELECT {PROFILE_COLUMNS} FROM user_profiles p WHERE p.org_id = $1 AND p.user_id = $2"
    );
    Ok(sqlx::query_as::<_, ProfileRow>(&sql)
        .bind(org)
        .bind(user)
        .fetch_optional(executor)
        .await?
        .map(profile_from))
}

/// The stored profile, if any. The caller has already decided the person may see it.
pub(crate) async fn load(pool: &PgPool, org: &str, user: &str) -> Result<Option<Profile>, sqlx::Error> {
    read(pool, org, user).await
}

/// The builder's input: the person's accepted, current, unexpired personal rows that carry a source
/// link, newest first. Legacy room-origin rows are excluded whatever the containment switch says: the
/// profile is a new surface and has no reason to show a row that no conversation recalls.
async fn load_sources(
    tx: &mut Transaction<'_, Postgres>,
    org: &str,
    user: &str,
) -> Result<Vec<SourceRow>, sqlx::Error> {
    let origin_space = crate::space::memory_scope::legacy_room_origin_space("$4");
    let sql = format!(
        "SELECT id, scope, key, kind, content, confidence, source_links, updated_at, {origin_space} \
         FROM agent_memory \
         WHERE org_id = $1 \
           AND owner = $2 \
           AND scope = 'user' \
           AND session_id IS NULL \
           AND review_state = 'accepted' \
           AND is_latest = true \
           AND (expires_at IS NULL OR expires_at > now()) \
           AND source_links <> '{{}}' \
         ORDER BY updated_at DESC, id \
         LIMIT $3"
    );
    let rows = sqlx::query_as::<
        _,
        (String, String, String, String, String, f64, Vec<String>, DateTime<Utc>, Option<String>),
    >(&sql)
    .bind(org)
    .bind(user)
    .bind(MAX_SOURCE_ROWS)
    .bind(true)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(id, scope, key, kind, content, confidence, source_links, updated_at, origin_space)| {
                SourceRow {
                    provenance: MemoryProvenance::classify(&source_links),
                    withheld: origin_space.is_some(),
                    id,
                    scope,
                    key,
                    kind,
                    content,
                    confidence,
                    updated_at,
                }
            },
        )
        .collect())
}

/// Content-free audit row: the event, the actor, the revision and the number of lines.
async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    org: &str,
    user: &str,
    event: &str,
    actor: &str,
    profile: &Profile,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO memory_control_events (id, org_id, user_id, event, actor, detail) \
         VALUES ($1, $2, $3, $4, $5, jsonb_build_object('revision', $6::bigint, 'lines', $7::bigint))",
    )
    .bind(new_ulid())
    .bind(org)
    .bind(user)
    .bind(event)
    .bind(actor)
    .bind(profile.revision)
    .bind(i64::try_from(profile.content.lines().count()).unwrap_or(i64::MAX))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// What [`rebuild`] did.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RebuildOutcome {
    /// The profile as it now stands: written, already identical, or none (nothing to build from).
    Current(Option<Profile>),
    /// An edited profile was kept, because the rebuild was not forced.
    KeptEdited(Profile),
    /// Memory does not allow new derivation for this person (paused, off, org disabled).
    Refused(MemoryDecision),
}

/// Rebuild the person's profile from their memory, under their control-row lock (the lock a pause,
/// a delete and a reset take), so it cannot interleave with any of them. Refused unless the memory
/// decision allows generation. An edited profile is kept unless `force`. A rebuild that would change
/// nothing writes nothing, so the revision only moves when the text or its sources do.
pub(crate) async fn rebuild(
    pool: &PgPool,
    org: &str,
    user: &str,
    force: bool,
    actor: &str,
) -> Result<RebuildOutcome, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let decision = crate::memory_control::resolve_in_tx(&mut tx, false, org, user).await?;
    if !decision.generation_allowed {
        tx.rollback().await?;
        return Ok(RebuildOutcome::Refused(decision));
    }
    let existing = read(&mut *tx, org, user).await?;
    if let Some(profile) = existing.as_ref() {
        if profile.origin == ORIGIN_EDITED && !force {
            tx.commit().await?;
            return Ok(RebuildOutcome::KeptEdited(profile.clone()));
        }
    }
    let built = build(&load_sources(&mut tx, org, user).await?);
    let unchanged = match existing.as_ref() {
        Some(profile) => {
            profile.origin == ORIGIN_BUILT
                && profile.content == built.content
                && profile.source_memory_ids == built.source_memory_ids
        }
        None => built.content.is_empty(),
    };
    if unchanged {
        tx.commit().await?;
        return Ok(RebuildOutcome::Current(existing));
    }
    let sql = format!(
        "WITH written AS ( \
             INSERT INTO user_profiles AS p \
               (org_id, user_id, content, source_memory_ids, origin, revision, built_at, updated_at) \
             VALUES ($1, $2, $3, $4, 'built', 1, now(), now()) \
             ON CONFLICT (org_id, user_id) DO UPDATE SET \
               content = EXCLUDED.content, \
               source_memory_ids = EXCLUDED.source_memory_ids, \
               origin = 'built', \
               revision = p.revision + 1, \
               built_at = now(), \
               updated_at = now() \
             RETURNING * \
         ) SELECT {PROFILE_COLUMNS} FROM written p"
    );
    let profile = profile_from(
        sqlx::query_as::<_, ProfileRow>(&sql)
            .bind(org)
            .bind(user)
            .bind(&built.content)
            .bind(&built.source_memory_ids)
            .fetch_one(&mut *tx)
            .await?,
    );
    audit(&mut tx, org, user, "profile_rebuilt", actor, &profile).await?;
    tx.commit().await?;
    metrics::counter!("mp_session_user_profile_rebuilds_total", "forced" => if force { "true" } else { "false" })
        .increment(1);
    Ok(RebuildOutcome::Current(Some(profile)))
}

/// What [`save_edit`] did.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EditOutcome {
    Saved(Profile),
    /// The edit was made against another revision than the stored one.
    Stale,
    /// The profile is hidden under this decision (memory off, or disabled by the org).
    Hidden(MemoryDecision),
}

/// Store the person's own text as their profile. Allowed while memory is paused (the profile stays
/// listed and editable, D8), refused while it is hidden. `expected_revision` is the revision the
/// edit was made against, 0 when there was no profile; it is checked under the user's lock. The
/// source list is kept, so the profile can say how many sources were deleted since the edit.
/// `content` must already be checked against [`MAX_CHARS`].
pub(crate) async fn save_edit(
    pool: &PgPool,
    org: &str,
    user: &str,
    content: &str,
    expected_revision: i64,
    actor: &str,
) -> Result<EditOutcome, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let decision = crate::memory_control::resolve_in_tx(&mut tx, false, org, user).await?;
    if !decision.list_visible() {
        tx.rollback().await?;
        return Ok(EditOutcome::Hidden(decision));
    }
    let current: i64 = sqlx::query_scalar(
        "SELECT revision FROM user_profiles WHERE org_id = $1 AND user_id = $2",
    )
    .bind(org)
    .bind(user)
    .fetch_optional(&mut *tx)
    .await?
    .unwrap_or(0);
    if current != expected_revision {
        tx.rollback().await?;
        return Ok(EditOutcome::Stale);
    }
    let sql = format!(
        "WITH written AS ( \
             INSERT INTO user_profiles AS p \
               (org_id, user_id, content, origin, revision, updated_at) \
             VALUES ($1, $2, $3, 'edited', 1, now()) \
             ON CONFLICT (org_id, user_id) DO UPDATE SET \
               content = EXCLUDED.content, \
               origin = 'edited', \
               revision = p.revision + 1, \
               updated_at = now() \
             RETURNING * \
         ) SELECT {PROFILE_COLUMNS} FROM written p"
    );
    let profile = profile_from(
        sqlx::query_as::<_, ProfileRow>(&sql)
            .bind(org)
            .bind(user)
            .bind(content)
            .fetch_one(&mut *tx)
            .await?,
    );
    audit(&mut tx, org, user, "profile_edited", actor, &profile).await?;
    tx.commit().await?;
    Ok(EditOutcome::Saved(profile))
}

/// Take the lines of deleted memories out of the person's built profile, inside the delete's own
/// transaction. An edited profile is the person's text and is kept; its read reports the deleted
/// sources. Runs whether or not the switch is on: erasure must reach a profile written while it was.
pub(crate) async fn drop_sources(
    tx: &mut Transaction<'_, Postgres>,
    org: &str,
    user: &str,
    deleted: &[String],
) -> Result<(), sqlx::Error> {
    let row: Option<(String, Vec<String>, String)> = sqlx::query_as(
        "SELECT content, source_memory_ids, origin FROM user_profiles \
         WHERE org_id = $1 AND user_id = $2 FOR UPDATE",
    )
    .bind(org)
    .bind(user)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((content, sources, origin)) = row else {
        return Ok(());
    };
    if origin != ORIGIN_BUILT || !sources.iter().any(|id| deleted.contains(id)) {
        return Ok(());
    }
    let kept = without_sources(&content, &sources, deleted);
    sqlx::query(
        "UPDATE user_profiles SET content = $3, source_memory_ids = $4, \
         revision = revision + 1, updated_at = now() \
         WHERE org_id = $1 AND user_id = $2",
    )
    .bind(org)
    .bind(user)
    .bind(&kept.content)
    .bind(&kept.source_memory_ids)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Delete the person's profile, inside the reset's own transaction.
pub(crate) async fn delete_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    org: &str,
    user: &str,
) -> Result<u64, sqlx::Error> {
    Ok(
        sqlx::query("DELETE FROM user_profiles WHERE org_id = $1 AND user_id = $2")
            .bind(org)
            .bind(user)
            .execute(&mut **tx)
            .await?
            .rows_affected(),
    )
}

/// Rebuild, without force, the profile of each person whose personal memory a Dreaming cycle wrote.
/// One read per person and no model call. A failure is logged and counted, never fatal to the cycle:
/// the next cycle that touches the person, or their own "rebuild", catches up.
pub(crate) async fn rebuild_after_cycle(pool: &PgPool, people: &[(String, String)]) {
    for (org, user) in people {
        let outcome = match rebuild(pool, org, user, false, "dreaming").await {
            Ok(RebuildOutcome::Current(_)) => "current",
            Ok(RebuildOutcome::KeptEdited(_)) => "kept_edited",
            Ok(RebuildOutcome::Refused(_)) => "refused",
            Err(error) => {
                tracing::warn!(%error, "user profile rebuild after a Dreaming cycle failed");
                "error"
            }
        };
        metrics::counter!("mp_session_user_profile_cycle_rebuilds_total", "outcome" => outcome)
            .increment(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_800_000_000 + seconds, 0).single().expect("valid time")
    }

    fn row(id: &str, kind: &str, content: &str, provenance: MemoryProvenance) -> SourceRow {
        SourceRow {
            id: id.to_owned(),
            scope: "user".to_owned(),
            key: format!("key-{id}"),
            kind: kind.to_owned(),
            content: content.to_owned(),
            confidence: 1.0,
            provenance,
            withheld: false,
            updated_at: at(0),
        }
    }

    #[test]
    fn build_orders_stated_before_inferred() {
        let mut inferred = row("m-inferred", "preference", "Likes short answers", MemoryProvenance::Inferred);
        inferred.confidence = 0.9;
        inferred.updated_at = at(100);
        let stated = row("m-stated", "preference", "Prefers tables for comparisons", MemoryProvenance::Stated);
        let fact = row("m-fact", "fact", "Works with tenders at Aquatiq", MemoryProvenance::Stated);
        let mut weak = row("m-weak", "preference", "Maybe likes emojis", MemoryProvenance::Inferred);
        weak.confidence = 0.79;

        let built = build(&[inferred, fact, weak, stated]);

        assert_eq!(
            built.content,
            "- Prefers tables for comparisons\n- Likes short answers\n- Works with tenders at Aquatiq"
        );
        assert_eq!(built.source_memory_ids, ["m-stated", "m-inferred", "m-fact"]);
    }

    #[test]
    fn build_respects_1400_chars_on_line_boundary() {
        // Each line is "- " plus 98 characters: 100, plus one newline between lines.
        let rows: Vec<SourceRow> = (0..20)
            .map(|index| {
                let mut source = row(
                    &format!("m-{index:02}"),
                    "fact",
                    &format!("{index:02}{}", "x".repeat(96)),
                    MemoryProvenance::Stated,
                );
                source.updated_at = at(-index);
                source
            })
            .collect();

        let built = build(&rows);

        // 13 lines take 13 * 100 + 12 = 1312 characters; a 14th would take 1413.
        assert_eq!(built.source_memory_ids.len(), 13);
        assert_eq!(built.content.chars().count(), 1312);
        assert!(built.content.chars().count() <= MAX_CHARS);
        assert!(built.content.split('\n').all(|line| line.chars().count() == 100), "no line is cut");
        assert_eq!(built.source_memory_ids.first().map(String::as_str), Some("m-00"), "newest kept first");
    }

    #[test]
    fn build_skips_non_user_scopes() {
        let mut thread = row("m-thread", "fact", "The deck is in draft", MemoryProvenance::Stated);
        thread.scope = "thread".to_owned();
        let mut session = row("m-session", "fact", "Session note", MemoryProvenance::Stated);
        session.scope = "session".to_owned();
        let own = row("m-own", "fact", "Speaks Norwegian", MemoryProvenance::Stated);
        let mut summary = row("m-summary", "artifact_summary", "An image was made", MemoryProvenance::Stated);
        summary.key = "summary".to_owned();

        let built = build(&[thread, session, own, summary]);

        assert_eq!(built.content, "- Speaks Norwegian");
        assert_eq!(built.source_memory_ids, ["m-own"]);
    }

    #[test]
    fn build_never_reads_space_or_org_rows() {
        let mut space = row("m-space", "fact", "The room meets on Fridays", MemoryProvenance::Stated);
        space.scope = "space".to_owned();
        let mut org = row("m-org", "fact", "Invoices go to finance", MemoryProvenance::Stated);
        org.scope = "org".to_owned();
        let mut agent = row("m-agent", "fact", "The agent drafts tenders", MemoryProvenance::Stated);
        agent.scope = "agent".to_owned();
        let mut legacy_room = row("m-room", "fact", "Said in a shared room", MemoryProvenance::Stated);
        legacy_room.withheld = true;

        assert_eq!(build(&[space, org, agent, legacy_room]), Built::default());
    }

    #[test]
    fn build_keeps_one_line_per_key_and_per_text() {
        let newest = {
            let mut source = row("m-new", "fact", "Works at Aquatiq", MemoryProvenance::Stated);
            source.key = "employer".to_owned();
            source.updated_at = at(10);
            source
        };
        let older = {
            let mut source = row("m-old", "fact", "Works at Acme", MemoryProvenance::Stated);
            source.key = "employer".to_owned();
            source
        };
        let same_text = row("m-dup", "fact", "works   at aquatiq", MemoryProvenance::Unknown);

        let built = build(&[older, same_text, newest]);

        assert_eq!(built.content, "- Works at Aquatiq");
        assert_eq!(built.source_memory_ids, ["m-new"]);
    }

    #[test]
    fn without_sources_drops_only_the_deleted_lines() {
        let sources = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        let kept = without_sources("- one\n- two\n- three", &sources, &["b".to_owned()]);
        assert_eq!(kept.content, "- one\n- three");
        assert_eq!(kept.source_memory_ids, ["a", "c"]);

        // A profile whose lines and sources disagree keeps nothing rather than a line of unknown origin.
        assert_eq!(
            without_sources("- one\n- two", &sources, &["b".to_owned()]),
            Built::default()
        );
    }

    /// The store is bounded by the database as well as by code, the audit vocabulary only grows, and
    /// the reversal undoes both.
    #[test]
    fn the_migration_and_its_revert_match() {
        let migration = include_str!("../migrations/0100_user_profiles.sql");
        assert!(migration.contains("CREATE TABLE IF NOT EXISTS user_profiles"));
        assert!(migration.contains(&format!("CHECK (char_length(content) <= {MAX_CHARS})")));
        assert!(migration.contains("CHECK (origin IN ('built', 'edited'))"));
        assert!(migration.contains(
            "CHECK (event IN ('state_changed', 'reset', 'profile_edited', 'profile_rebuilt')) NOT VALID"
        ));
        let revert = include_str!("../migrations/revert/0100_user_profiles.down.sql");
        assert!(revert.contains("DROP TABLE IF EXISTS user_profiles"));
        assert!(revert.contains("DELETE FROM memory_control_events WHERE event IN ('profile_edited', 'profile_rebuilt')"));
        assert!(revert.contains("CHECK (event IN ('state_changed', 'reset'))"));
    }

    // ---- against a disposable database (session-core DB harness) ----

    use crate::auth::VerifiedIdentity;
    use crate::memory_grpc::MemoryGrpc;
    use mp_contracts::model_plane::v1 as pb;
    use mp_contracts::model_plane::v1::memory_service_server::MemoryService;
    use tonic::Request;

    async fn scratch_pool() -> PgPool {
        let url = std::env::var("DATABASE_URL").expect("a scratch database whose name contains test");
        let database = url
            .rsplit('/')
            .next()
            .and_then(|tail| tail.split('?').next())
            .unwrap_or_default();
        assert!(
            database.contains("test"),
            "DATABASE_URL must name a scratch database whose name contains \"test\""
        );
        let pool = PgPool::connect(&url).await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    fn request<T>(value: T, identity: VerifiedIdentity) -> Request<T> {
        let mut request = Request::new(value);
        request.extensions_mut().insert(identity);
        request
    }

    fn service(pool: &PgPool) -> MemoryGrpc {
        MemoryGrpc::new(pool.clone(), None).with_profile_enabled_for_test(true)
    }

    /// A stated personal row, as `save_memory` writes it.
    async fn seed_stated(pool: &PgPool, org: &str, user: &str, content: &str) -> String {
        let id = new_ulid();
        sqlx::query(
            "INSERT INTO agent_memory (id, org_id, session_id, scope, key, content, kind, owner, source_links) \
             VALUES ($1, $2, NULL, 'user', $1, $3, 'preference', $4, ARRAY['tool:save_memory'])",
        )
        .bind(&id)
        .bind(org)
        .bind(content)
        .bind(user)
        .execute(pool)
        .await
        .unwrap();
        id
    }

    async fn set_state(pool: &PgPool, org: &str, user: &str, state: crate::memory_control::MemoryState) {
        crate::memory_control::set_state(pool, org, user, state, user).await.unwrap();
    }

    async fn stored(pool: &PgPool, org: &str, user: &str) -> Option<Profile> {
        load(pool, org, user).await.unwrap()
    }

    fn fresh_org() -> String {
        format!("profile-{}", new_ulid())
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
    async fn rebuild_refused_when_paused() {
        let pool = scratch_pool().await;
        let org = fresh_org();
        seed_stated(&pool, &org, "user-a", "Prefers tables").await;
        set_state(&pool, &org, "user-a", crate::memory_control::MemoryState::Paused).await;

        let refused = service(&pool)
            .rebuild_user_profile(request(
                pb::RebuildUserProfileRequest { org_id: org.clone(), user_id: "user-a".to_owned(), force: true },
                VerifiedIdentity::user_for_test(&org, "user-a"),
            ))
            .await
            .expect_err("a paused memory derives nothing new");
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
        assert_eq!(refused.message(), "memory_not_enabled:user_paused");
        assert_eq!(stored(&pool, &org, "user-a").await, None);

        // Paused keeps the profile editable (D8).
        let edited = service(&pool)
            .set_user_profile(request(
                pb::SetUserProfileRequest {
                    org_id: org.clone(),
                    user_id: "user-a".to_owned(),
                    content: "- Written while paused".to_owned(),
                    expected_revision: 0,
                },
                VerifiedIdentity::user_for_test(&org, "user-a"),
            ))
            .await
            .expect("a paused profile can be edited")
            .into_inner();
        assert_eq!(edited.skip_reason, "user_paused");
        assert_eq!(edited.profile.expect("profile").origin, ORIGIN_EDITED);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
    async fn edited_profile_survives_rebuild_without_force() {
        let pool = scratch_pool().await;
        let org = fresh_org();
        let source = seed_stated(&pool, &org, "user-a", "Prefers tables").await;
        let RebuildOutcome::Current(Some(built)) = rebuild(&pool, &org, "user-a", false, "test").await.unwrap() else {
            panic!("a first rebuild writes the profile");
        };
        assert_eq!(built.content, "- Prefers tables");
        assert_eq!(built.source_memory_ids, [source.clone()]);

        let EditOutcome::Saved(edited) =
            save_edit(&pool, &org, "user-a", "- Prefers tables, always", built.revision, "user-a").await.unwrap()
        else {
            panic!("the edit is saved");
        };
        assert_eq!(edited.revision, built.revision + 1);
        assert_eq!(edited.source_memory_ids, [source.clone()], "an edit keeps the provenance");

        seed_stated(&pool, &org, "user-a", "Writes in Norwegian").await;
        assert_eq!(
            rebuild(&pool, &org, "user-a", false, "dreaming").await.unwrap(),
            RebuildOutcome::KeptEdited(edited.clone())
        );
        assert_eq!(stored(&pool, &org, "user-a").await.unwrap().content, "- Prefers tables, always");

        let RebuildOutcome::Current(Some(forced)) = rebuild(&pool, &org, "user-a", true, "user-a").await.unwrap() else {
            panic!("a forced rebuild replaces the edit");
        };
        assert_eq!(forced.origin, ORIGIN_BUILT);
        assert_eq!(forced.source_memory_ids.len(), 2);

        let events: Vec<String> = sqlx::query_scalar(
            "SELECT event FROM memory_control_events WHERE org_id = $1 ORDER BY at, id",
        )
        .bind(&org)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(events, ["profile_rebuilt", "profile_edited", "profile_rebuilt"]);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
    async fn delete_memory_drops_its_line_on_rebuild() {
        let pool = scratch_pool().await;
        let org = fresh_org();
        let kept = seed_stated(&pool, &org, "user-a", "Prefers tables").await;
        let gone = seed_stated(&pool, &org, "user-a", "Lives in Bergen").await;
        rebuild(&pool, &org, "user-a", false, "test").await.unwrap();

        crate::dreaming::delete_user_memory(&pool, &org, "user-a", &gone).await.unwrap();
        let after_delete = stored(&pool, &org, "user-a").await.unwrap();
        assert_eq!(after_delete.source_memory_ids, [kept.clone()], "the delete drops the line at once");
        assert!(!after_delete.content.contains("Bergen"));

        let RebuildOutcome::Current(Some(rebuilt)) = rebuild(&pool, &org, "user-a", false, "test").await.unwrap() else {
            panic!("the profile stays");
        };
        assert_eq!(rebuilt.content, "- Prefers tables");

        // An edited profile is kept as written and counts the deleted source instead.
        save_edit(&pool, &org, "user-a", "- Prefers tables\n- Lives in Bergen", rebuilt.revision, "user-a")
            .await
            .unwrap();
        crate::dreaming::delete_user_memory(&pool, &org, "user-a", &kept).await.unwrap();
        let edited = stored(&pool, &org, "user-a").await.unwrap();
        assert_eq!(edited.origin, ORIGIN_EDITED);
        assert_eq!(edited.sources_deleted_since_edit, 1);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
    async fn reset_deletes_profile() {
        let pool = scratch_pool().await;
        let org = fresh_org();
        seed_stated(&pool, &org, "user-a", "Prefers tables").await;
        seed_stated(&pool, &org, "user-b", "Prefers prose").await;
        rebuild(&pool, &org, "user-a", false, "test").await.unwrap();
        rebuild(&pool, &org, "user-b", false, "test").await.unwrap();

        service(&pool)
            .reset_memory(request(
                pb::ResetMemoryRequest { org_id: org.clone(), user_id: "user-a".to_owned(), confirm: true },
                VerifiedIdentity::user_for_test(&org, "user-a"),
            ))
            .await
            .expect("the owner may reset");

        assert_eq!(stored(&pool, &org, "user-a").await, None);
        assert!(stored(&pool, &org, "user-b").await.is_some(), "another user's profile stays");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
    async fn stale_revision_refused() {
        let pool = scratch_pool().await;
        let org = fresh_org();
        let edit = |content: &str, expected_revision: u64| pb::SetUserProfileRequest {
            org_id: org.clone(),
            user_id: "user-a".to_owned(),
            content: content.to_owned(),
            expected_revision,
        };
        let first = service(&pool)
            .set_user_profile(request(edit("- First", 0), VerifiedIdentity::user_for_test(&org, "user-a")))
            .await
            .expect("the first edit names revision 0")
            .into_inner();
        assert_eq!(first.profile.expect("profile").revision, 1);

        let stale = service(&pool)
            .set_user_profile(request(edit("- Second", 0), VerifiedIdentity::user_for_test(&org, "user-a")))
            .await
            .expect_err("an edit against an older revision is refused");
        assert_eq!(stale.code(), tonic::Code::FailedPrecondition);
        assert_eq!(stale.message(), REVISION_STALE);
        assert_eq!(stored(&pool, &org, "user-a").await.unwrap().content, "- First");

        let too_long = service(&pool)
            .set_user_profile(request(edit(&"x".repeat(MAX_CHARS + 1), 1), VerifiedIdentity::user_for_test(&org, "user-a")))
            .await
            .expect_err("a profile over the bound is refused");
        assert_eq!(too_long.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
    async fn zdr_caller_cannot_set_profile() {
        let pool = scratch_pool().await;
        let org = fresh_org();
        seed_stated(&pool, &org, "user-a", "Prefers tables").await;
        rebuild(&pool, &org, "user-a", false, "test").await.unwrap();
        let before = stored(&pool, &org, "user-a").await;

        let zdr = || VerifiedIdentity::user_for_test_with_zdr(&org, "user-a", true);
        let waived = || VerifiedIdentity::user_for_test_with_waiver(&org, "user-a", "connection-1", "ack-1");
        for caller in [zdr(), waived()] {
            let set = service(&pool)
                .set_user_profile(request(
                    pb::SetUserProfileRequest {
                        org_id: org.clone(),
                        user_id: "user-a".to_owned(),
                        content: "- Must not persist".to_owned(),
                        expected_revision: 1,
                    },
                    caller.clone(),
                ))
                .await
                .expect_err("a ZDR or waived caller writes no profile");
            let rebuilt = service(&pool)
                .rebuild_user_profile(request(
                    pb::RebuildUserProfileRequest { org_id: org.clone(), user_id: "user-a".to_owned(), force: true },
                    caller,
                ))
                .await
                .expect_err("nor rebuilds one");
            for refused in [set, rebuilt] {
                assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
                let refusal = mp_contracts::zdr_refusal::Refusal::from_status(&refused).expect("typed refusal");
                assert_eq!(refusal.feature(), Some(ZDR_FEATURE));
            }
        }
        assert_eq!(stored(&pool, &org, "user-a").await, before, "nothing was written");

        let read = service(&pool)
            .get_user_profile(request(
                pb::GetUserProfileRequest { org_id: org.clone(), user_id: "user-a".to_owned() },
                zdr(),
            ))
            .await
            .expect("a ZDR read is empty, not an error")
            .into_inner();
        assert!(read.profile.is_none());
        assert_eq!(read.skip_reason, "zdr");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
    async fn gdpr_erasure_deletes_profile() {
        let pool = scratch_pool().await;
        let org = fresh_org();
        let other_org = format!("{org}-other");
        seed_stated(&pool, &org, "user-a", "Prefers tables").await;
        seed_stated(&pool, &other_org, "user-a", "Prefers prose").await;
        rebuild(&pool, &org, "user-a", false, "test").await.unwrap();
        rebuild(&pool, &other_org, "user-a", false, "test").await.unwrap();

        let summary = crate::gdpr::purge_organization_data(&pool, &org, None).await.expect("purge");

        assert_eq!(summary.user_profiles, 1);
        assert_eq!(stored(&pool, &org, "user-a").await, None);
        assert!(stored(&pool, &other_org, "user-a").await.is_some(), "another org's profile stays");
        crate::gdpr::purge_organization_data(&pool, &other_org, None).await.expect("cleanup");
    }

    /// With the switch off every profile RPC answers `profile_not_enabled` before any database read
    /// (the pool here cannot connect), so the SPA hides the card.
    #[tokio::test]
    async fn switch_off_refuses_every_profile_rpc() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://unreachable.invalid/session")
            .expect("lazy pool");
        let org = fresh_org();
        let off = MemoryGrpc::new(pool, None).with_profile_enabled_for_test(false);
        let user = || VerifiedIdentity::user_for_test(&org, "user-a");
        let refused = [
            off.get_user_profile(request(
                pb::GetUserProfileRequest { org_id: org.clone(), user_id: "user-a".to_owned() },
                user(),
            ))
            .await
            .unwrap_err(),
            off.rebuild_user_profile(request(
                pb::RebuildUserProfileRequest { org_id: org.clone(), user_id: "user-a".to_owned(), force: true },
                user(),
            ))
            .await
            .unwrap_err(),
            off.set_user_profile(request(
                pb::SetUserProfileRequest {
                    org_id: org.clone(),
                    user_id: "user-a".to_owned(),
                    content: "- Anything".to_owned(),
                    expected_revision: 0,
                },
                user(),
            ))
            .await
            .unwrap_err(),
        ];
        for status in refused {
            assert_eq!(status.code(), tonic::Code::FailedPrecondition);
            assert_eq!(status.message(), NOT_ENABLED);
        }
    }
}
