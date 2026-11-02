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
