//! WS14-S5: the room-file listing and `space-document` read.
//!
//! These run the EXACT SQL the two handlers run: the constants are read out of
//! `src/api/mod.rs` at compile time, so a changed predicate changes the test's
//! input instead of leaving a stale copy behind. The shape checks are hermetic.
//! The database tests at the end are `#[ignore]`d and need `TEST_DATABASE_URL`
//! pointing at a disposable PostgreSQL (same convention as
//! `ownership_filter.rs`).
//!
//! The handler-level rules (the grants union, the cursor codec, the metadata
//! subset) are unit tests in `src/api/mod.rs` (`space_files_tests`) and
//! `src/authz/visibility.rs`, where the private functions are reachable.

use sqlx::PgPool;

const API_SOURCE: &str = include_str!("../src/api/mod.rs");

/// The string literal assigned to `const <name>: &str =` in `src/api/mod.rs`.
fn sql_const(name: &str) -> String {
    let marker = format!("const {name}: &str =");
    let start = API_SOURCE
        .find(&marker)
        .unwrap_or_else(|| panic!("{name} is not defined in src/api/mod.rs"));
    let rest = &API_SOURCE[start + marker.len()..];
    let open = rest.find('"').expect("opening quote") + 1;
    let close = open + rest[open..].find("\";").expect("closing quote");
    rest[open..close].replace("\r\n", "\n")
}

fn squashed(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn the_listing_sql_scopes_to_one_live_room_with_the_viewer_predicate_and_a_total_order() {
    let sql = squashed(&sql_const("SPACE_DOCUMENTS_SQL"));
    for needle in [
        "WHERE org_id = $1 AND space_ref = $2 AND deleted_at IS NULL",
        "($3::text IS NULL OR owner_id = $3 OR visibility = 'org' OR document_id = ANY($4))",
        "($6::text[] IS NULL OR type = ANY($6))",
        "($7::text IS NULL OR (updated_at, document_id) < ($7::text::timestamptz, $8::text))",
        "ORDER BY updated_at DESC, document_id DESC",
        "LIMIT $5",
        "owner_id, created_at::TEXT",
        "metadata->>'filename'",
        "metadata->>'sha256'",
    ] {
        assert!(sql.contains(needle), "listing SQL lost `{needle}`:\n{sql}");
    }
    let selects_text_column = sql
        .split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')')
        .any(|token| token == "content");
    assert!(
        !selects_text_column,
        "the listing must never select the document text (content_type metadata is fine)"
    );
}

#[test]
fn the_document_sql_is_the_same_predicate_for_one_id_and_returns_the_text() {
    let sql = squashed(&sql_const("SPACE_DOCUMENT_SQL"));
    for needle in [
        "WHERE org_id = $1 AND space_ref = $2 AND document_id = $3 AND deleted_at IS NULL",
        "($4::text IS NULL OR owner_id = $4 OR visibility = 'org' OR document_id = ANY($5))",
        "content",
    ] {
        assert!(sql.contains(needle), "document SQL lost `{needle}`:\n{sql}");
    }
}

// ───────────────────────────── database (gated) ─────────────────────────────

fn test_db_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

type ListRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

const SPACE: &str = "space:org:room-1";

async fn setup(pool: &PgPool, org: &str) {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS documents (document_id TEXT PRIMARY KEY, org_id TEXT NOT NULL)",
    )
    .execute(pool)
    .await
    .expect("create documents");
    for column in [
        "source TEXT NOT NULL DEFAULT ''",
        "type TEXT NOT NULL DEFAULT ''",
        "title TEXT NOT NULL DEFAULT ''",
        "content TEXT NOT NULL DEFAULT ''",
        "status TEXT NOT NULL DEFAULT 'pending'",
        "zdr_classification TEXT NOT NULL DEFAULT 'internal'",
        "metadata JSONB NOT NULL DEFAULT '{}'",
        "owner_id TEXT NOT NULL DEFAULT 'org-system-account'",
        "visibility TEXT NOT NULL DEFAULT 'org'",
        "space_ref TEXT",
        "created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()",
        "updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()",
        "deleted_at TIMESTAMPTZ",
    ] {
        sqlx::query(&format!("ALTER TABLE documents ADD COLUMN IF NOT EXISTS {column}"))
            .execute(pool)
            .await
            .expect("add column");
    }
    sqlx::query("DELETE FROM documents WHERE org_id = $1")
        .bind(org)
        .execute(pool)
        .await
        .expect("clean");
}

#[allow(clippy::too_many_arguments)]
async fn seed(
    pool: &PgPool,
    org: &str,
    id: &str,
    space: &str,
    owner: &str,
    visibility: &str,
    doc_type: &str,
    updated_at: &str,
    deleted: bool,
    content: &str,
) {
    sqlx::query(
        "INSERT INTO documents (document_id, org_id, source, type, title, content, owner_id, visibility,
                                space_ref, updated_at, created_at, deleted_at, metadata)
         VALUES ($1,$2,'space-upload',$3,$4,$5,$6,$7,$8,$9::timestamptz,$9::timestamptz,
                 CASE WHEN $10 THEN NOW() ELSE NULL END,
                 jsonb_build_object('filename', $4::text, 'sha256', $1::text, 'size_bytes', '2048', 'text_chars', '12'))",
    )
    .bind(format!("{org}:{id}"))
    .bind(org)
    .bind(doc_type)
    .bind(id)
    .bind(content)
    .bind(owner)
    .bind(visibility)
    .bind(space)
    .bind(updated_at)
    .bind(deleted)
    .execute(pool)
    .await
    .expect("seed document");
}

async fn list(
    pool: &PgPool,
    org: &str,
    viewer: Option<&str>,
    granted: &[String],
    limit: i64,
    types: Option<Vec<String>>,
    cursor: Option<(String, String)>,
) -> Vec<ListRow> {
    let (cursor_ts, cursor_id) = match cursor {
        Some((ts, id)) => (Some(ts), Some(id)),
        None => (None, None),
    };
    sqlx::query_as::<_, ListRow>(&sql_const("SPACE_DOCUMENTS_SQL"))
        .bind(org)
        .bind(SPACE)
        .bind(viewer)
        .bind(granted)
        .bind(limit)
        .bind(types)
        .bind(cursor_ts)
        .bind(cursor_id)
        .fetch_all(pool)
        .await
        .expect("listing query")
}

fn ids(org: &str, rows: &[ListRow]) -> Vec<String> {
    rows.iter()
        .map(|r| r.0.trim_start_matches(&format!("{org}:")).to_string())
        .collect()
}

async fn connect() -> PgPool {
    let url = test_db_url().expect("TEST_DATABASE_URL is required for this ignored test");
    PgPool::connect(&url).await.unwrap()
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
async fn the_keyset_cursor_pages_a_room_in_a_stable_order_without_gaps_or_repeats() {
    let pool = connect().await;
    let org = format!("spacefiles-page-{}", std::process::id());
    setup(&pool, &org).await;
    // Five files share one timestamp (the id breaks the tie), two are older.
    for id in ["f1", "f2", "f3", "f4", "f5"] {
        seed(&pool, &org, id, SPACE, "u1", "shared", "space_upload", "2026-11-02 06:00:00.5+00", false, "text").await;
    }
    seed(&pool, &org, "old1", SPACE, "u1", "shared", "space_upload", "2026-11-01 06:00:00+00", false, "text").await;
    seed(&pool, &org, "old2", SPACE, "u1", "shared", "space_upload", "2026-10-31 06:00:00+00", false, "text").await;

    let page = 3_i64;
    let mut collected: Vec<String> = Vec::new();
    let mut cursor = None;
    for _ in 0..10 {
        // One extra row tells a full page from a complete room (the handler's rule).
        let mut rows = list(&pool, &org, Some("u1"), &[], page + 1, None, cursor.clone()).await;
        let more = rows.len() as i64 > page;
        rows.truncate(page as usize);
        collected.extend(ids(&org, &rows));
        match (more, rows.last()) {
            (true, Some(last)) => cursor = Some((last.6.clone(), last.0.clone())),
            _ => break,
        }
    }
    assert_eq!(
        collected,
        ["f5", "f4", "f3", "f2", "f1", "old1", "old2"],
        "newest first, id descending on a tie, every file exactly once"
    );
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
async fn the_types_filter_and_the_room_boundary_hold() {
    let pool = connect().await;
    let org = format!("spacefiles-types-{}", std::process::id());
    setup(&pool, &org).await;
    seed(&pool, &org, "file", SPACE, "u1", "shared", "space_upload", "2026-11-02 06:00:00+00", false, "t").await;
    seed(&pool, &org, "note", SPACE, "u1", "shared", "note", "2026-11-02 06:00:01+00", false, "t").await;
    seed(&pool, &org, "other-room", "space:org:room-2", "u1", "shared", "space_upload", "2026-11-02 06:00:02+00", false, "t").await;
    seed(&pool, &org, "no-room", "", "u1", "org", "space_upload", "2026-11-02 06:00:03+00", false, "t").await;
    seed(&pool, &org, "gone", SPACE, "u1", "shared", "space_upload", "2026-11-02 06:00:04+00", true, "t").await;
    sqlx::query("UPDATE documents SET space_ref = NULL WHERE document_id = $1")
        .bind(format!("{org}:no-room"))
        .execute(&pool)
        .await
        .unwrap();

    let all = list(&pool, &org, Some("u1"), &[], 50, None, None).await;
    assert_eq!(ids(&org, &all), ["note", "file"], "other room, no room and deleted rows are absent");
    let only_files = list(&pool, &org, Some("u1"), &[], 50, Some(vec!["space_upload".into()]), None).await;
    assert_eq!(ids(&org, &only_files), ["file"]);
    let none = list(&pool, &org, Some("u1"), &[], 50, Some(vec!["pdf".into()]), None).await;
    assert!(none.is_empty());
    // Another org's identical Space name reads nothing.
    assert!(list(&pool, "another-org", Some("u1"), &[], 50, None, None).await.is_empty());
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
async fn the_viewer_sees_a_private_room_file_only_through_ownership_or_the_grants_union() {
    let pool = connect().await;
    let org = format!("spacefiles-vis-{}", std::process::id());
    setup(&pool, &org).await;
    // Room files are stored `shared`: owner-only unless the grant list names them.
    seed(&pool, &org, "mine", SPACE, "u1", "shared", "space_upload", "2026-11-02 06:00:00+00", false, "t").await;
    seed(&pool, &org, "theirs", SPACE, "u2", "shared", "space_upload", "2026-11-02 06:00:01+00", false, "t").await;
    seed(&pool, &org, "org-wide", SPACE, "u2", "org", "space_upload", "2026-11-02 06:00:02+00", false, "t").await;

    // No union (no verified Space decision, flag off, revoked member): own + org only.
    let own_only = list(&pool, &org, Some("u1"), &[], 50, None, None).await;
    assert_eq!(ids(&org, &own_only), ["org-wide", "mine"]);

    // The union (the ids Control's space-visible join returned) adds exactly those.
    let granted = vec![format!("{org}:theirs")];
    let with_union = list(&pool, &org, Some("u1"), &granted, 50, None, None).await;
    assert_eq!(ids(&org, &with_union), ["org-wide", "theirs", "mine"]);

    // An id the join did not return stays hidden even if it is in the room.
    let wrong = vec![format!("{org}:does-not-exist")];
    assert_eq!(ids(&org, &list(&pool, &org, Some("u1"), &wrong, 50, None, None).await), ["org-wide", "mine"]);

    // The org-admin bypass (viewer None) sees the whole live room.
    let admin = list(&pool, &org, None, &[], 50, None, None).await;
    assert_eq!(admin.len(), 3);

    // Provenance the file library renders.
    let mine = own_only.iter().find(|r| r.0.ends_with(":mine")).unwrap();
    assert_eq!(mine.7, "u1", "uploaded_by");
    assert_eq!(mine.9.as_deref(), Some("mine"), "metadata filename");
    assert_eq!(mine.11.as_deref(), Some("2048"), "metadata size_bytes");
    assert!(!mine.8.is_empty(), "created_at");
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
async fn space_document_returns_text_and_404s_missing_deleted_other_room_and_hidden_ids_alike() {
    let pool = connect().await;
    let org = format!("spacefiles-doc-{}", std::process::id());
    setup(&pool, &org).await;
    seed(&pool, &org, "mine", SPACE, "u1", "shared", "space_upload", "2026-11-02 06:00:00+00", false, "the extracted text").await;
    seed(&pool, &org, "theirs", SPACE, "u2", "shared", "space_upload", "2026-11-02 06:00:00+00", false, "private to u2").await;
    seed(&pool, &org, "gone", SPACE, "u1", "shared", "space_upload", "2026-11-02 06:00:00+00", true, "deleted").await;
    seed(&pool, &org, "elsewhere", "space:org:room-2", "u1", "shared", "space_upload", "2026-11-02 06:00:00+00", false, "other room").await;

    type DocRow = (String, String, String, String, String, Option<String>, String);
    let read = |id: &'static str, viewer: Option<&'static str>, granted: Vec<String>| {
        let pool = pool.clone();
        let org = org.clone();
        async move {
            sqlx::query_as::<_, DocRow>(&sql_const("SPACE_DOCUMENT_SQL"))
                .bind(&org)
                .bind(SPACE)
                .bind(format!("{org}:{id}"))
                .bind(viewer)
                .bind(granted)
                .fetch_optional(&pool)
                .await
                .expect("document query")
        }
    };

    let found = read("mine", Some("u1"), vec![]).await.expect("owner reads their file");
    assert_eq!(found.6, "the extracted text");
    assert_eq!(found.5.as_deref(), Some("mine"));
    assert!(read("missing", Some("u1"), vec![]).await.is_none());
    assert!(read("gone", Some("u1"), vec![]).await.is_none(), "deleted");
    assert!(read("elsewhere", Some("u1"), vec![]).await.is_none(), "other room");
    assert!(read("theirs", Some("u1"), vec![]).await.is_none(), "not visible to u1");
    // Visible through the union; the admin bypass reads it too.
    assert!(read("theirs", Some("u1"), vec![format!("{org}:theirs")]).await.is_some());
    assert!(read("theirs", None, vec![]).await.is_some());
}
