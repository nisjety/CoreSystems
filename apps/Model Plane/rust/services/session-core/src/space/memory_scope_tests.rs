//! Pure tests of the memory containment resolver (WS17-S1, WS17-S2). No database, no network:
//! the table of the containment rule, the claim normaliser, the shape of the SQL the readers
//! splice in, and the closed vocabulary the wire carries. The database tier lives in
//! `grpc_space_memory_tests.rs`.

use super::*;

/// The SQL fragments are written over several lines with continuation backslashes; compare them
/// with the whitespace collapsed so a reflow never breaks a test.
fn flat(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn thread_is_contained_with_follows_the_rule_table() {
    // (name, enabled, space_id, space_kind, contained)
    let table: &[(&str, bool, Option<&str>, Option<&str>, bool)] = &[
        ("no space, no kind", true, None, None, false),
        ("no space, room kind is ignored", true, None, Some("room"), false),
        ("personal space", true, Some("space-1"), Some("personal"), false),
        ("room", true, Some("space-1"), Some("room"), true),
        ("project", true, Some("space-1"), Some("project"), true),
        ("case", true, Some("space-1"), Some("case"), true),
        ("dm", true, Some("space-1"), Some("dm"), true),
        ("group dm", true, Some("space-1"), Some("group_dm"), true),
        ("an unknown future kind fails closed", true, Some("space-1"), Some("brand-new"), true),
        ("missing kind fails closed", true, Some("space-1"), None, true),
        ("blank kind fails closed", true, Some("space-1"), Some(""), true),
        ("blank space id is no space", true, Some(""), Some("room"), false),
        ("whitespace space id is no space", true, Some("   "), Some("room"), false),
        ("kind is trimmed before the personal test", true, Some("space-1"), Some(" personal "), false),
        ("personal is exact, not a prefix", true, Some("space-1"), Some("personal2"), true),
        ("personal is case sensitive", true, Some("space-1"), Some("Personal"), true),
        ("flag off: room", false, Some("space-1"), Some("room"), false),
        ("flag off: missing kind", false, Some("space-1"), None, false),
        ("flag off: no space", false, None, None, false),
    ];
    for (name, enabled, space_id, space_kind, expected) in table {
        assert_eq!(
            thread_is_contained_with(*enabled, *space_id, *space_kind),
            *expected,
            "{name}"
        );
    }
}

#[test]
fn normalize_claim_space_kind_keeps_plain_tokens_only() {
    for (raw, expected) in [
        ("personal", Some("personal")),
        ("room", Some("room")),
        ("group_dm", Some("group_dm")),
        ("group-dm", Some("group-dm")),
        ("  room  ", Some("room")),
        ("Room2", Some("Room2")),
        // exactly the column's 32-character limit
        ("abcdefghijklmnopqrstuvwxyz012345", Some("abcdefghijklmnopqrstuvwxyz012345")),
    ] {
        assert_eq!(normalize_claim_space_kind(raw).as_deref(), expected, "{raw:?}");
    }
    for rejected in [
        "",
        "   ",
        // 33 characters: would violate the column CHECK
        "abcdefghijklmnopqrstuvwxyz0123456",
        "room kind",
        "room;drop",
        "room'",
        "a.b",
        "room\n",
        "r\u{f6}om",
        "\u{200b}",
    ] {
        // a trailing newline is trimmed, so judge the trimmed form for that one case
        let expected = if rejected == "room\n" { Some("room".to_owned()) } else { None };
        assert_eq!(normalize_claim_space_kind(rejected), expected, "{rejected:?}");
    }
}

#[test]
fn a_dropped_kind_leaves_the_thread_contained() {
    // normalize drops it -> the column stays NULL -> the thread reads as contained.
    let stored = normalize_claim_space_kind("not a token!");
    assert_eq!(stored, None);
    assert!(thread_is_contained_with(true, Some("space-1"), stored.as_deref()));
}

#[test]
fn contained_sql_is_the_twin_of_the_rule() {
    let sql = flat(CONTAINED_SQL);
    assert_eq!(
        sql,
        "(t.space_id IS NOT NULL AND t.space_kind IS DISTINCT FROM 'personal')"
    );
    // IS DISTINCT FROM keeps a NULL kind contained (`<>` would drop it).
    assert!(!sql.contains("<>"));
}

#[test]
fn legacy_room_origin_exclusion_sql_text() {
    let sql = flat(&legacy_room_origin_exclusion("$7", "$2"));
    // The flag switches the whole predicate off.
    assert!(sql.starts_with("(NOT $7 OR "), "{sql}");
    // The reader's own thread rows stay visible (C7).
    assert!(sql.contains("session_id = $2"), "{sql}");
    // The legacy marker: a source link `thread:<id>` naming a contained thread.
    assert!(sql.contains("unnest(agent_memory.source_links) AS sl(link)"), "{sql}");
    assert!(sql.contains("JOIN threads ct ON ct.id = substr(sl.link, 8)"), "{sql}");
    assert!(sql.contains("sl.link LIKE 'thread:%'"), "{sql}");
    assert!(
        sql.contains("ct.space_id IS NOT NULL AND ct.space_kind IS DISTINCT FROM 'personal'"),
        "{sql}"
    );
    assert!(sql.contains("NOT EXISTS"), "{sql}");
    // The parameters are the caller's, not hard coded.
    let other = flat(&legacy_room_origin_exclusion("$9", "$4"));
    assert!(other.starts_with("(NOT $9 OR "), "{other}");
    assert!(other.contains("session_id = $4"), "{other}");
    assert!(!other.contains("$7") && !other.contains("$2"), "{other}");
    // `thread:` is seven characters, so the id starts at the eighth.
    assert_eq!("thread:".len() + 1, 8);
}

#[test]
fn legacy_room_origin_space_sql_carries_the_flag_and_the_same_shape() {
    let sql = flat(&legacy_room_origin_space("$4"));
    let exclusion = flat(&legacy_room_origin_exclusion("$7", "$2"));
    // The flag is part of the lookup: with containment off the row is recalled, so not withheld.
    assert!(sql.contains("WHERE $4 AND sl.link LIKE 'thread:%'"), "{sql}");
    // The same shape as the recall filter, so the list and the recall always agree.
    for shared in [
        "unnest(agent_memory.source_links) AS sl(link)",
        "JOIN threads ct ON ct.id = substr(sl.link, 8)",
        "sl.link LIKE 'thread:%'",
        "ct.space_id IS NOT NULL AND ct.space_kind IS DISTINCT FROM 'personal'",
    ] {
        assert!(sql.contains(shared), "space lookup lacks {shared:?}: {sql}");
        assert!(exclusion.contains(shared), "exclusion lacks {shared:?}: {exclusion}");
    }
    // It answers with the Space, deterministically, as a scalar subquery.
    assert!(sql.starts_with("(SELECT ct.space_id FROM"), "{sql}");
    assert!(sql.contains("ORDER BY ct.space_id LIMIT 1)"), "{sql}");
    // A different flag parameter is spliced, not hard coded.
    let other = flat(&legacy_room_origin_space("$11"));
    assert!(other.contains("WHERE $11 AND"), "{other}");
    assert!(!other.contains("$4"), "{other}");
}

#[test]
fn withheld_reason_is_the_closed_wire_word() {
    assert_eq!(WITHHELD_ROOM_ORIGIN, "room_origin_unverified");
}

#[test]
fn refusal_and_degradation_words_are_stable() {
    assert!(REFUSAL_CONTAINED.starts_with("memory_scope_contained"));
    assert_eq!(DEGRADED_LETTA_SPACE_SUPPRESSED, "DEGRADED_LETTA_SPACE_SUPPRESSED");
}

#[test]
fn topics_that_write_beyond_the_thread_are_refused_in_a_room() {
    for topic in [
        "USER", "user", "User", " user ", "POLICY", "policy", "AGENT", "agent", "WORKSPACE",
        "Workspace", "\tWORKSPACE\n",
    ] {
        assert!(topic_writes_beyond_thread(topic), "{topic:?}");
    }
    for topic in ["MEMORY", "memory", "", "  ", "NOTES", "USERS", "THREAD", "ORG", "GLOBAL"] {
        assert!(!topic_writes_beyond_thread(topic), "{topic:?}");
    }
}

#[test]
fn reader_scope_reports_containment() {
    assert!(!ReaderScope::Personal.is_contained());
    let contained = ReaderScope::Contained {
        space_ref: "space-1".to_owned(),
        audience_revision: 3,
        audience_hash: "sha256:abc".to_owned(),
    };
    assert!(contained.is_contained());
    assert_ne!(ReaderScope::Personal, contained);
}

// ---- WS17-S3: the label resolver ---------------------------------------------------------------

/// The thread the contained reader below is reading in.
const READER_THREAD: &str = "thread-reader";
/// The contained reader's Space, audience hash and audience revision (R).
const READER_SPACE: &str = "space-1";
const READER_HASH: &str = "sha256:audience-a";
const READER_REVISION: i64 = 5;

fn contained_reader() -> ReaderScope {
    ReaderScope::Contained {
        space_ref: READER_SPACE.to_owned(),
        audience_revision: READER_REVISION,
        audience_hash: READER_HASH.to_owned(),
    }
}

/// A room row of the reader's Space, at the reader's exact audience hash and revision R. Each case
/// below changes one field of it.
fn room_row() -> RowLabel<'static> {
    RowLabel {
        scope: "space",
        session_id: None,
        origin_kind: ORIGIN_ROOM,
        space_ref: Some(READER_SPACE),
        audience_hash: Some(READER_HASH),
        audience_revision: Some(READER_REVISION),
    }
}

/// An ordinary personal user row: no Space, no audience, no thread.
fn personal_row() -> RowLabel<'static> {
    RowLabel {
        scope: "user",
        session_id: None,
        origin_kind: ORIGIN_PERSONAL,
        space_ref: None,
        audience_hash: None,
        audience_revision: None,
    }
}

#[test]
fn the_origin_words_are_the_closed_label_vocabulary() {
    assert_eq!(ORIGIN_PERSONAL, "personal");
    assert_eq!(ORIGIN_ROOM, "room");
    assert_eq!(ORIGIN_DECLASSIFIED, "declassified");
    assert_eq!(ORIGIN_ROOM_UNVERIFIED, "room_unverified");
}

#[test]
fn memory_visible_for_a_personal_reader_follows_the_label_switch() {
    // (case, row, labels_on, visible)
    let table: Vec<(&str, RowLabel<'static>, bool, bool)> = vec![
        ("a personal row, labels on", personal_row(), true, true),
        ("a personal row, labels off", personal_row(), false, true),
        ("a room row never reaches a personal reader, labels on", room_row(), true, false),
        ("a room row is visible again with the labels off (the S1 rules)", room_row(), false, true),
        (
            "a declassified row never reaches a personal reader, labels on",
            RowLabel { origin_kind: ORIGIN_DECLASSIFIED, ..room_row() },
            true,
            false,
        ),
        (
            "a declassified row is visible again with the labels off",
            RowLabel { origin_kind: ORIGIN_DECLASSIFIED, ..room_row() },
            false,
            true,
        ),
        (
            "a room_unverified row is never visible, labels on",
            RowLabel { origin_kind: ORIGIN_ROOM_UNVERIFIED, ..personal_row() },
            true,
            false,
        ),
        (
            "a room_unverified row is never visible, labels off",
            RowLabel { origin_kind: ORIGIN_ROOM_UNVERIFIED, ..personal_row() },
            false,
            false,
        ),
        (
            "a personal thread's own room-labelled row is still a room row, labels on",
            RowLabel { session_id: Some(READER_THREAD), ..room_row() },
            true,
            false,
        ),
        (
            "a personal thread's own personal row, labels on",
            RowLabel { session_id: Some(READER_THREAD), ..personal_row() },
            true,
            true,
        ),
    ];
    for (case, row, labels_on, visible) in table {
        assert_eq!(
            memory_visible(row, &ReaderScope::Personal, READER_THREAD, labels_on),
            visible,
            "{case}: {row:?}"
        );
    }
}

#[test]
fn memory_visible_for_a_contained_reader_is_its_own_thread_or_its_room_at_or_below_the_audience() {
    let sibling = "thread-sibling";
    // (case, row, visible)
    let table: Vec<(&str, RowLabel<'static>, bool)> = vec![
        // The reader's own thread rows, whatever their label, except the withheld ones.
        ("an own-thread personal-labelled row", RowLabel { session_id: Some(READER_THREAD), scope: "thread", ..personal_row() }, true),
        ("an own-thread room-labelled row", RowLabel { session_id: Some(READER_THREAD), scope: "thread", ..room_row() }, true),
        (
            "an own-thread declassified row",
            RowLabel { session_id: Some(READER_THREAD), scope: "thread", origin_kind: ORIGIN_DECLASSIFIED, ..room_row() },
            true,
        ),
        (
            "an own-thread room_unverified row is withheld even from its own thread",
            RowLabel { session_id: Some(READER_THREAD), scope: "thread", origin_kind: ORIGIN_ROOM_UNVERIFIED, ..personal_row() },
            false,
        ),
        ("a sibling thread's personal-labelled row", RowLabel { session_id: Some(sibling), scope: "thread", ..personal_row() }, false),
        // The room's labelled rows of the reader's own Space.
        ("a room row at the exact hash and revision R", room_row(), true),
        ("a room row below R", RowLabel { audience_revision: Some(READER_REVISION - 3), ..room_row() }, true),
        ("a room row at revision 1", RowLabel { audience_revision: Some(1), ..room_row() }, true),
        ("a room row above R", RowLabel { audience_revision: Some(READER_REVISION + 1), ..room_row() }, false),
        ("a room row far above R", RowLabel { audience_revision: Some(i64::MAX), ..room_row() }, false),
        ("a room row with no revision", RowLabel { audience_revision: None, ..room_row() }, false),
        ("a declassified row of the Space at the exact hash and R", RowLabel { origin_kind: ORIGIN_DECLASSIFIED, ..room_row() }, true),
        (
            "a declassified row above R",
            RowLabel { origin_kind: ORIGIN_DECLASSIFIED, audience_revision: Some(READER_REVISION + 1), ..room_row() },
            false,
        ),
        ("a room row of another Space", RowLabel { space_ref: Some("space-2"), ..room_row() }, false),
        ("a room row with no Space", RowLabel { space_ref: None, ..room_row() }, false),
        ("a room row at another audience hash", RowLabel { audience_hash: Some("sha256:audience-b"), ..room_row() }, false),
        ("a room row with no audience hash", RowLabel { audience_hash: None, ..room_row() }, false),
        ("a room row with a blank audience hash", RowLabel { audience_hash: Some(""), ..room_row() }, false),
        ("a room_unverified row of the Space", RowLabel { origin_kind: ORIGIN_ROOM_UNVERIFIED, ..room_row() }, false),
        // Rows that are not the room's: personal memory and the wide scopes.
        ("a personal-origin user row", personal_row(), false),
        ("a user-scope row labelled room", RowLabel { scope: "user", ..room_row() }, false),
        ("an org-scope row labelled room", RowLabel { scope: "org", ..room_row() }, false),
        ("an agent-scope row labelled room", RowLabel { scope: "agent", ..room_row() }, false),
        ("a space-scope row labelled personal", RowLabel { origin_kind: ORIGIN_PERSONAL, ..room_row() }, false),
    ];
    let reader = contained_reader();
    for (case, row, visible) in table {
        for labels_on in [true, false] {
            assert_eq!(
                memory_visible(row, &reader, READER_THREAD, labels_on),
                visible,
                "{case} (labels_on {labels_on}): {row:?}"
            );
        }
    }
}

#[test]
fn a_contained_reader_with_a_blank_audience_hash_sees_no_room_row() {
    let reader = ReaderScope::Contained {
        space_ref: READER_SPACE.to_owned(),
        audience_revision: READER_REVISION,
        audience_hash: String::new(),
    };
    for row in [
        room_row(),
        RowLabel { audience_hash: Some(""), ..room_row() },
        RowLabel { audience_hash: None, ..room_row() },
        RowLabel { origin_kind: ORIGIN_DECLASSIFIED, ..room_row() },
    ] {
        assert!(!memory_visible(row, &reader, READER_THREAD, true), "{row:?}");
    }
    // Its own thread's rows are the thread's own, so the blank audience does not hide them.
    let own = RowLabel { session_id: Some(READER_THREAD), scope: "thread", ..personal_row() };
    assert!(memory_visible(own, &reader, READER_THREAD, true));
}

#[test]
fn label_read_sql_text_withholds_room_unverified_and_gates_personal_readers_on_the_switch() {
    let sql = flat(&label_read_sql("$6", "$7"));
    assert_eq!(
        sql,
        "(origin_kind <> 'room_unverified' AND (NOT $6 OR $7 OR origin_kind = 'personal'))"
    );
    // The parameters are the caller's, not hard coded.
    let other = flat(&label_read_sql("$1", "$2"));
    assert!(other.contains("NOT $1 OR $2 OR"), "{other}");
    assert!(!other.contains("$6") && !other.contains("$7"), "{other}");
}

// ---- WS17-S3: the SQL twins agree with the resolver (needs Postgres, `#[ignore]`d) -------------
//
// Run with `DATABASE_URL` pointing at a disposable Postgres whose database name contains `test`:
// `cargo test -p session-core --bin session-core space::memory_scope -- --ignored --test-threads=1`.

/// One seeded `agent_memory` row of the grid, with the label fields the resolver reads.
struct GridRow {
    id: String,
    scope: &'static str,
    scope_id: Option<String>,
    session_id: Option<String>,
    origin_kind: &'static str,
    space_ref: Option<String>,
    audience_hash: Option<String>,
    audience_revision: Option<i64>,
}

impl GridRow {
    fn label(&self) -> RowLabel<'_> {
        RowLabel {
            scope: self.scope,
            session_id: self.session_id.as_deref(),
            origin_kind: self.origin_kind,
            space_ref: self.space_ref.as_deref(),
            audience_hash: self.audience_hash.as_deref(),
            audience_revision: self.audience_revision,
        }
    }
}

/// A grid that varies each label field around the reader (Space `space`, hash `hash`, revision 5, thread
/// `reader_thread`): every origin, scope, Space, hash and revision the resolver tells apart.
fn grid(suffix: &str, space: &str, hash: &str, reader_thread: &str) -> Vec<GridRow> {
    let other_space = format!("{space}-other");
    let other_hash = format!("{hash}-other");
    let row = |name: &str,
               scope: &'static str,
               session: Option<&str>,
               origin: &'static str,
               in_space: Option<&str>,
               audience_hash: Option<&str>,
               revision: Option<i64>| {
        let scope_id = match scope {
            "space" => in_space.map(str::to_owned),
            "agent" => in_space.map(|owner_space| format!("{owner_space}:agent-1")),
            _ => None,
        };
        GridRow {
            id: format!("grid-{name}-{suffix}"),
            scope,
            scope_id,
            session_id: session.map(str::to_owned),
            origin_kind: origin,
            space_ref: in_space.map(str::to_owned),
            audience_hash: audience_hash.map(str::to_owned),
            audience_revision: revision,
        }
    };
    vec![
        // Personal memory and thread notes.
        row("p-user", "user", None, "personal", None, None, None),
        row("p-org", "org", None, "personal", None, None, None),
        row("p-thread-own", "thread", Some(reader_thread), "personal", None, None, None),
        row("p-thread-other", "thread", Some("thread-other"), "personal", None, None, None),
        row("unv-user", "user", None, "room_unverified", None, None, None),
        row("unv-thread-own", "thread", Some(reader_thread), "room_unverified", None, None, None),
        row("unv-space", "space", None, "room_unverified", Some(space), Some(hash), Some(3)),
        row("room-thread-own", "thread", Some(reader_thread), "room", Some(space), Some(hash), Some(3)),
        // The room's labelled rows, each differing from the reader in one respect.
        row("room-below", "space", None, "room", Some(space), Some(hash), Some(3)),
        row("room-at", "space", None, "room", Some(space), Some(hash), Some(5)),
        row("room-above", "space", None, "room", Some(space), Some(hash), Some(6)),
        row("room-other-hash", "space", None, "room", Some(space), Some(other_hash.as_str()), Some(3)),
        row("room-other-space", "space", None, "room", Some(other_space.as_str()), Some(hash), Some(3)),
        row("decl-below", "space", None, "declassified", Some(space), Some(hash), Some(3)),
        row("decl-above", "space", None, "declassified", Some(space), Some(hash), Some(6)),
        row("decl-other-hash", "space", None, "declassified", Some(space), Some(other_hash.as_str()), Some(3)),
        // Labelled rows of another scope are never the room's.
        row("agent-room", "agent", None, "room", Some(space), Some(hash), Some(3)),
        row("user-room", "user", None, "room", Some(space), Some(hash), Some(3)),
    ]
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
async fn label_sql_and_room_recall_agree_with_memory_visible_on_a_seeded_table() {
    use std::collections::BTreeSet;

    let Some(pool) = crate::space::test_support::connect().await else { return };
    let suffix = mp_ids::new_ulid();
    let org = format!("org-grid-{suffix}");
    let space = format!("space-grid-{suffix}");
    let hash = "sha256:audience-a".to_owned();
    let reader_thread = "thread-reader".to_owned();
    let rows = grid(&suffix, &space, &hash, &reader_thread);
    for row in &rows {
        sqlx::query(
            "INSERT INTO agent_memory (id, org_id, session_id, scope, key, content, owner, origin_kind, scope_id, \
             space_ref, audience_hash, audience_revision, review_state, is_latest) \
             VALUES ($1, $2, $3, $4, $1, 'grid row', 'owner-1', $5, $6, $7, $8, $9, 'accepted', true)",
        )
        .bind(&row.id)
        .bind(&org)
        .bind(&row.session_id)
        .bind(row.scope)
        .bind(row.origin_kind)
        .bind(&row.scope_id)
        .bind(&row.space_ref)
        .bind(&row.audience_hash)
        .bind(row.audience_revision)
        .execute(&pool)
        .await
        .unwrap_or_else(|error| panic!("seed {}: {error}", row.id));
    }
    let contained = ReaderScope::Contained {
        space_ref: space.clone(),
        audience_revision: 5,
        audience_hash: hash.clone(),
    };
    let ids = |rows: Vec<String>| rows.into_iter().collect::<BTreeSet<String>>();

    for labels_on in [true, false] {
        // The generic reads of a personal reader: the label predicate alone decides.
        let sql = format!("SELECT id FROM agent_memory WHERE org_id = $1 AND {}", label_read_sql("$2", "$3"));
        let seen = ids(
            sqlx::query_scalar(&sql)
                .bind(&org)
                .bind(labels_on)
                .bind(false)
                .fetch_all(&pool)
                .await
                .expect("personal read"),
        );
        let expected = ids(
            rows.iter()
                .filter(|row| memory_visible(row.label(), &ReaderScope::Personal, &reader_thread, labels_on))
                .map(|row| row.id.clone())
                .collect(),
        );
        assert_eq!(seen, expected, "a personal reader, labels_on {labels_on}");

        // The generic reads of a contained reader are bound to its own thread's rows; the label predicate
        // only withholds `room_unverified` among them.
        let sql = format!(
            "SELECT id FROM agent_memory WHERE org_id = $1 AND session_id = $4 AND {}",
            label_read_sql("$2", "$3")
        );
        let seen = ids(
            sqlx::query_scalar(&sql)
                .bind(&org)
                .bind(labels_on)
                .bind(true)
                .bind(&reader_thread)
                .fetch_all(&pool)
                .await
                .expect("contained read"),
        );
        let expected = ids(
            rows.iter()
                .filter(|row| row.session_id.as_deref() == Some(reader_thread.as_str()))
                .filter(|row| memory_visible(row.label(), &contained, &reader_thread, labels_on))
                .map(|row| row.id.clone())
                .collect(),
        );
        assert_eq!(seen, expected, "a contained reader's own thread rows, labels_on {labels_on}");
    }

    // The room branch of recall: the Space's labelled rows at the thread's audience. Without a full memory gate the
    // thread's own rows stay out.
    let target = crate::dreaming::SpaceRecallTarget {
        space_id: space.clone(),
        audience_hash: hash.clone(),
    };
    let recalled = crate::dreaming::recall_space_memory(&pool, &org, &reader_thread, &target, "", 50, false)
        .await
        .expect("room recall");
    let seen = ids(recalled.rows.iter().map(|row| row.id.clone()).collect());
    let expected = ids(
        rows.iter()
            .filter(|row| row.scope == "space" && row.session_id.is_none())
            .filter(|row| memory_visible(row.label(), &contained, &reader_thread, true))
            .map(|row| row.id.clone())
            .collect(),
    );
    assert_eq!(seen, expected, "the room branch of recall and the resolver must name the same rows");

    // D22al (Rule R): with a full memory gate the room recall also names the thread's own session-bound rows, exactly
    // as the resolver admits them (`room_unverified` never).
    let recalled = crate::dreaming::recall_space_memory(&pool, &org, &reader_thread, &target, "", 50, true)
        .await
        .expect("room recall with the thread's own rows");
    let seen = ids(recalled.rows.iter().map(|row| row.id.clone()).collect());
    let expected = ids(
        rows.iter()
            .filter(|row| {
                row.session_id.as_deref() == Some(reader_thread.as_str())
                    || (row.scope == "space" && row.session_id.is_none())
            })
            .filter(|row| memory_visible(row.label(), &contained, &reader_thread, true))
            .map(|row| row.id.clone())
            .collect(),
    );
    assert_eq!(seen, expected, "Rule R: the room's rows and the thread's own rows, as the resolver names them");
    for row in &recalled.rows {
        let own = rows
            .iter()
            .any(|seeded| seeded.id == row.id && seeded.session_id.as_deref() == Some(reader_thread.as_str()));
        assert_eq!(recalled.thread_row_scopes.contains_key(&row.id), own, "only the thread's own rows carry their scope");
    }
    sqlx::query("DELETE FROM agent_memory WHERE org_id = $1")
        .bind(&org)
        .execute(&pool)
        .await
        .ok();
}

// ---- D18: the member's timestamp floor on room-derived memory rows ---------------------------

#[test]
fn history_floor_admits_sql_names_the_floor_and_the_source_rule() {
    let sql = flat(&history_floor_admits_sql("$6"));
    // No floor (NULL) admits every row, so the predicate changes nothing for a reader without a window.
    assert!(sql.starts_with("($6::TIMESTAMPTZ IS NULL OR NOT ("), "{sql}");
    // Room-derived: not a personal origin, a Space or agent scope, or a shared-room thread (by link or session).
    assert!(sql.contains("agent_memory.origin_kind <> 'personal'"), "{sql}");
    assert!(sql.contains("agent_memory.scope IN ('space', 'agent')"), "{sql}");
    assert!(sql.contains("sl.link LIKE 'thread:%'"), "{sql}");
    assert!(sql.contains("st.id = agent_memory.session_id"), "{sql}");
    // An agent note is never admitted: its text is made from its run's whole context.
    assert!(sql.contains("agent_memory.scope <> 'agent'"), "{sql}");
    // A message-derived row: every linked message exists and is posted at or after the floor.
    assert!(sql.contains("sl.link LIKE 'message:%'"), "{sql}");
    assert!(sql.contains("substr(sl.link, 9)"), "{sql}");
    assert!(sql.contains("fm.created_at >= $6::TIMESTAMPTZ"), "{sql}");
    // A member's own save or share with no message link: its first write instant decides.
    assert!(
        sql.contains("agent_memory.source_label = 'member_save' OR agent_memory.origin_kind = 'declassified'"),
        "{sql}"
    );
    assert!(sql.contains("agent_memory.created_at >= $6::TIMESTAMPTZ"), "{sql}");
    // One placeholder, and every format brace is filled in.
    assert!(!sql.contains("$7") && !sql.contains('{') && !sql.contains('}'), "{sql}");
}

/// One seeded `agent_memory` row of the D18 grid. `key` names the row; the other fields are the source shape under test.
struct FloorRow {
    key: &'static str,
    scope: &'static str,
    scope_id: Option<String>,
    owner: &'static str,
    session_id: Option<String>,
    origin_kind: &'static str,
    space_ref: Option<String>,
    source_label: Option<&'static str>,
    source_links: Vec<String>,
    created_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// The D18 grid: every source shape a room or personal row can have, around a floor `floor`. A room message is
/// `pre-<suffix>` (before the floor) or `post-<suffix>` (after it); `gone-<suffix>` names no message.
fn floor_grid(suffix: &str, space: &str, room: &str, personal: &str, floor: chrono::DateTime<chrono::Utc>) -> Vec<FloorRow> {
    let before = floor - chrono::Duration::hours(2);
    let after = floor + chrono::Duration::minutes(5);
    let agent_scope = format!("{space}:agent-1");
    let room_row = |key: &'static str| FloorRow {
        key,
        scope: "space",
        scope_id: Some(space.to_owned()),
        owner: "",
        session_id: None,
        origin_kind: "room",
        space_ref: Some(space.to_owned()),
        source_label: Some("room_statement"),
        source_links: Vec::new(),
        created_at: None,
    };
    let personal_row = |key: &'static str| FloorRow {
        key,
        scope: "user",
        scope_id: None,
        owner: "owner-1",
        session_id: None,
        origin_kind: "personal",
        space_ref: None,
        source_label: None,
        source_links: Vec::new(),
        created_at: None,
    };
    vec![
        // A room statement is derived from one message: that message's instant decides.
        FloorRow { source_links: vec![format!("thread:{room}"), format!("message:pre-{suffix}")], ..room_row("A-stmt-pre") },
        FloorRow { source_links: vec![format!("thread:{room}"), format!("message:post-{suffix}")], ..room_row("B-stmt-post") },
        FloorRow { source_links: vec![format!("thread:{room}"), format!("message:gone-{suffix}")], ..room_row("C-stmt-missing") },
        FloorRow { source_links: vec![format!("message:pre-{suffix}"), format!("message:post-{suffix}")], ..room_row("D-stmt-both") },
        // A member's own save and a share: the first write instant decides.
        FloorRow {
            source_label: Some("member_save"),
            source_links: vec![format!("thread:{room}"), "tool:save_memory".to_owned()],
            created_at: Some(after),
            ..room_row("E-save-post")
        },
        FloorRow {
            source_label: Some("member_save"),
            source_links: vec![format!("thread:{room}"), "tool:save_memory".to_owned()],
            created_at: Some(before),
            ..room_row("F-save-pre")
        },
        FloorRow {
            origin_kind: "declassified",
            source_label: Some("member_save"),
            source_links: vec![format!("declassified:src-{suffix}"), format!("decision:d-{suffix}")],
            created_at: Some(after),
            ..room_row("G-share-post")
        },
        // An agent note, and a room row with no reliable instant: always hidden.
        FloorRow {
            scope: "agent",
            scope_id: Some(agent_scope),
            source_label: Some("agent_memory"),
            source_links: vec![format!("thread:{room}"), format!("run:r-{suffix}"), "tool:save_memory".to_owned()],
            created_at: Some(after),
            ..room_row("H-agent-note")
        },
        FloorRow { source_links: Vec::new(), ..room_row("L-no-source") },
        FloorRow { source_links: vec![format!("thread:{room}")], ..room_row("M-thread-only") },
        // The member's personal memory: narrowed only when it is derived from the room.
        personal_row("I-personal-plain"),
        FloorRow { source_links: vec![format!("thread:{personal}")], ..personal_row("K-personal-thread") },
        FloorRow { source_links: vec![format!("thread:{room}"), format!("message:pre-{suffix}")], ..personal_row("J-personal-from-room") },
        FloorRow { session_id: Some(room.to_owned()), ..personal_row("N-personal-room-session") },
    ]
}

/// The keys (without the per-test suffix) of the org's rows the floor admits; `None` admits all of them.
async fn floor_admitted(pool: &sqlx::PgPool, org: &str, suffix: &str, floor_at: Option<chrono::DateTime<chrono::Utc>>) -> std::collections::BTreeSet<String> {
    let sql = format!("SELECT key FROM agent_memory WHERE org_id = $1 AND {}", history_floor_admits_sql("$2"));
    let keys: Vec<String> = sqlx::query_scalar(&sql)
        .bind(org)
        .bind(floor_at)
        .fetch_all(pool)
        .await
        .expect("the floor predicate runs");
    keys.into_iter()
        .map(|key| key.strip_suffix(&format!("-{suffix}")).unwrap_or(&key).to_owned())
        .collect()
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
async fn history_floor_admits_a_room_row_only_by_its_own_source_instant() {
    use std::collections::BTreeSet;

    let Some(pool) = crate::space::test_support::connect().await else { return };
    let suffix = mp_ids::new_ulid();
    let org = format!("org-d18-{suffix}");
    let space = format!("space-d18-{suffix}");
    let room = format!("thread-d18-room-{suffix}");
    let personal = format!("thread-d18-own-{suffix}");
    crate::space::test_support::seed_space_thread(&pool, &org, &space, &room, "owner-1").await;
    sqlx::query("INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, 'owner-1')")
        .bind(&personal)
        .bind(&org)
        .execute(&pool)
        .await
        .expect("seed an ordinary chat thread");
    // Two messages in the room, one posted before the floor and one after it.
    let floor = chrono::Utc::now() - chrono::Duration::hours(1);
    for (id, at) in [
        (format!("pre-{suffix}"), floor - chrono::Duration::hours(2)),
        (format!("post-{suffix}"), chrono::Utc::now()),
    ] {
        crate::space::test_support::seed_space_message(&pool, &space, &room, &id, "user", Some("owner-1")).await;
        sqlx::query("UPDATE messages SET created_at = $2 WHERE id = $1")
            .bind(&id)
            .bind(at)
            .execute(&pool)
            .await
            .expect("place the message in time");
    }

    for grid_row in floor_grid(&suffix, &space, &room, &personal, floor) {
        sqlx::query(
            "INSERT INTO agent_memory (id, org_id, session_id, scope, scope_id, key, content, owner, source_links, \
             origin_kind, space_ref, audience_hash, audience_revision, source_label, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, 'grid', $7, $8, $9, $10, \
             CASE WHEN $10::TEXT IS NULL THEN NULL ELSE 'sha256:audience-1' END, \
             CASE WHEN $10::TEXT IS NULL THEN NULL ELSE 1::BIGINT END, $11, COALESCE($12, now()))",
        )
        .bind(mp_ids::new_ulid())
        .bind(&org)
        .bind(grid_row.session_id.as_deref())
        .bind(grid_row.scope)
        .bind(grid_row.scope_id.as_deref())
        .bind(format!("{}-{suffix}", grid_row.key))
        .bind(grid_row.owner)
        .bind(&grid_row.source_links)
        .bind(grid_row.origin_kind)
        .bind(grid_row.space_ref.as_deref())
        .bind(grid_row.source_label)
        .bind(grid_row.created_at)
        .execute(&pool)
        .await
        .unwrap_or_else(|error| panic!("seed {}: {error}", grid_row.key));
    }

    // A reader without a window sees every row: the predicate is a no-op.
    let whole = floor_admitted(&pool, &org, &suffix, None).await;
    assert_eq!(whole.len(), 14, "no floor, no row narrowed: {whole:?}");

    // A windowed reader sees only what its own source instant admits: the post-floor statement, the save and the
    // share made after the floor, and the personal memory that no room message derives.
    let windowed = floor_admitted(&pool, &org, &suffix, Some(floor)).await;
    let expected: BTreeSet<String> = [
        "B-stmt-post",
        "E-save-post",
        "G-share-post",
        "I-personal-plain",
        "K-personal-thread",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(windowed, expected, "the D18 source rule over a seeded grid");

    sqlx::query("DELETE FROM agent_memory WHERE org_id = $1")
        .bind(&org)
        .execute(&pool)
        .await
        .ok();
    crate::space::test_support::clean_space(&pool, &org, &space).await;
    sqlx::query("DELETE FROM threads WHERE org_id = $1")
        .bind(&org)
        .execute(&pool)
        .await
        .ok();
}
