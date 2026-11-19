//! D18: a member's history window on every Space-scoped read of the retrieval engine.
//!
//! Control signs `history_floor_at` (and, for a WS04-S11 since_join room, `history_floor_revision`) on the
//! retrieval decision. The engine hides every row that entered the room before that instant: the room's
//! document set the dense search is pinned to, the by-id lookups, the room listing, a single room file, the
//! room wiki and the candidates that survive the arms. A row with no reliable instant is hidden, and a
//! revision with no instant hides everything, because no Data Plane row carries an audience revision.
//!
//! The shape checks are hermetic: they read the SQL constants out of `src/api/mod.rs` at compile time (the
//! convention of `space_files_listing.rs`), so a predicate that is dropped from a read fails here instead of
//! leaving a stale copy behind. The database tests at the end are `#[ignore]`d and need `TEST_DATABASE_URL`
//! pointing at a disposable PostgreSQL (the convention of the other database tests in this crate). They run
//! the exact SQL and the exact functions the handlers run.

use chrono::{DateTime, Utc};
use retrieval_engine::pipeline::postprocess::{NodePostprocessor, PostprocessCtx, SpaceHistoryGate};
use retrieval_engine::pipeline::types::{ScoredCandidate, ZdrMode};
use retrieval_engine::search::wiki;
use retrieval_engine::space_scope::{resolve_space_document_ids, HistoryFloor};
use sqlx::PgPool;

mod common;

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

fn at(rfc3339: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(rfc3339)
        .expect("test instant")
        .with_timezone(&Utc)
}

// ─────────────────────────────── hermetic ───────────────────────────────

#[test]
fn every_room_read_carries_the_window_as_a_predicate_on_the_instant_a_document_entered_the_room() {
    for (name, placeholder) in [
        ("SOURCES_SQL", "$6"),
        ("FRESHNESS_SQL", "$6"),
        ("SPACE_DOCUMENTS_SQL", "$9"),
        ("SPACE_DOCUMENT_SQL", "$6"),
    ] {
        let sql = squashed(&sql_const(name));
        let predicate =
            format!("AND ({placeholder}::timestamptz IS NULL OR created_at >= {placeholder}::timestamptz)");
        assert!(
            sql.contains(&predicate),
            "{name} lost the history window predicate `{predicate}`:\n{sql}"
        );
        // The window never replaces the room predicate or the ownership predicate: it only narrows.
        assert!(sql.contains("deleted_at IS NULL"), "{name} lost the liveness predicate:\n{sql}");
        assert!(sql.contains("space_ref ="), "{name} lost the room predicate:\n{sql}");
        // It is the instant the document ENTERED the room, never the latest edit.
        assert!(!predicate.contains("updated_at"), "the window must not use the update instant");
    }
    // A listing's keyset cursor still pages on the update instant; the window is a separate predicate on
    // the creation instant, so a page boundary never moves a pre-floor document into the window.
    let listing = squashed(&sql_const("SPACE_DOCUMENTS_SQL"));
    assert!(listing.contains("(updated_at, document_id) < ($7::text::timestamptz, $8::text)"));
    assert!(listing.contains("ORDER BY updated_at DESC, document_id DESC LIMIT $5"));

    let wiki_listing = squashed(&sql_const("SPACE_WIKI_PAGES_SQL"));
    assert!(
        wiki_listing.contains("AND ($4::timestamptz IS NULL OR created_at >= $4::timestamptz)"),
        "the room wiki listing lost the window predicate:\n{wiki_listing}"
    );
    assert!(wiki_listing.contains("page_status = 'published'"));
    assert!(wiki_listing.contains("ORDER BY updated_at DESC LIMIT $3"));
}

// ───────────────────────────── database (gated) ─────────────────────────────

fn test_db_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

async fn connect() -> PgPool {
    let url = test_db_url().expect("TEST_DATABASE_URL is required for this ignored test");
    PgPool::connect(&url).await.expect("connect to TEST_DATABASE_URL")
}

const SPACE: &str = "space:org:history-room";
const OTHER_SPACE: &str = "space:org:another-room";
const FLOOR: &str = "2026-11-10T12:00:00Z";

/// The tables these reads touch, shaped as the other room-file tests shape them. Existing tables are kept
/// and given the columns this file needs, so the file coexists with the others on one disposable database.
async fn setup(pool: &PgPool, org: &str) {
    sqlx::query("CREATE TABLE IF NOT EXISTS documents (document_id TEXT PRIMARY KEY, org_id TEXT NOT NULL)")
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
            .expect("add documents column");
    }
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS knowledge_units (
             knowledge_id TEXT PRIMARY KEY, document_id TEXT NOT NULL, org_id TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS wiki_pages (
             page_id TEXT PRIMARY KEY, org_id TEXT NOT NULL, workspace_id TEXT NOT NULL,
             title TEXT NOT NULL, path TEXT NOT NULL, current_version_id TEXT, page_status TEXT,
             backlinks JSONB, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
             updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), deleted_at TIMESTAMPTZ);
         CREATE TABLE IF NOT EXISTS wiki_page_versions (
             version_id TEXT PRIMARY KEY, page_id TEXT NOT NULL, content TEXT, source_refs JSONB,
             proposed_by_agent TEXT);",
    )
    .execute(pool)
    .await
    .expect("create the wiki tables");
    sqlx::query("DELETE FROM documents WHERE org_id = $1")
        .bind(org)
        .execute(pool)
        .await
        .expect("clean documents");
    sqlx::query("DELETE FROM wiki_page_versions WHERE page_id IN (SELECT page_id FROM wiki_pages WHERE org_id = $1)")
        .bind(org)
        .execute(pool)
        .await
        .expect("clean wiki versions");
    sqlx::query("DELETE FROM wiki_pages WHERE org_id = $1")
        .bind(org)
        .execute(pool)
        .await
        .expect("clean wiki pages");
    // Every read below runs org-scoped (`SET LOCAL ROLE dataplane_app`); this must follow the CREATE TABLEs.
    common::grant_rls_runtime_role(pool).await;
}

/// One room document with its own creation and update instants (the window reads the first).
async fn seed_document(
    pool: &PgPool,
    org: &str,
    id: &str,
    space: &str,
    created_at: &str,
    updated_at: &str,
    deleted: bool,
) {
    sqlx::query(
        "INSERT INTO documents (document_id, org_id, source, type, title, content, owner_id, visibility,
                                space_ref, created_at, updated_at, deleted_at, metadata)
         VALUES ($1, $2, 'space-upload', 'space_upload', $3, 'room text', 'u1', 'org', $4,
                 $5::timestamptz, $6::timestamptz, CASE WHEN $7 THEN NOW() ELSE NULL END,
                 jsonb_build_object('filename', $3::text))",
    )
    .bind(format!("{org}:{id}"))
    .bind(org)
    .bind(id)
    .bind(space)
    .bind(created_at)
    .bind(updated_at)
    .bind(deleted)
    .execute(pool)
    .await
    .expect("seed document");
}

/// The room every database test reads. `old` entered the room before the floor and was edited after it:
/// the edit must not bring it back into the window.
async fn seed_room(pool: &PgPool, org: &str) {
    seed_document(pool, org, "old", SPACE, "2026-11-01 00:00:00+00", "2026-11-15 00:00:00+00", false).await;
    seed_document(pool, org, "edge", SPACE, "2026-11-10 12:00:00+00", "2026-11-10 12:00:00+00", false).await;
    seed_document(pool, org, "new", SPACE, "2026-11-12 00:00:00+00", "2026-11-12 00:00:00+00", false).await;
    seed_document(pool, org, "gone", SPACE, "2026-11-13 00:00:00+00", "2026-11-13 00:00:00+00", true).await;
    seed_document(pool, org, "elsewhere", OTHER_SPACE, "2026-11-13 00:00:00+00", "2026-11-13 00:00:00+00", false).await;
}

fn local_ids(org: &str, ids: &[String]) -> Vec<String> {
    ids.iter()
        .map(|id| id.trim_start_matches(&format!("{org}:")).to_string())
        .collect()
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
async fn the_room_document_set_is_narrowed_to_the_window_before_any_search_runs() {
    let pool = connect().await;
    let org = format!("historyfloor-set-{}", std::process::id());
    setup(&pool, &org).await;
    seed_room(&pool, &org).await;
    let list = |floor: HistoryFloor| {
        let (pool, org) = (pool.clone(), org.clone());
        async move {
            let (ids, truncated) = resolve_space_document_ids(&pool, &org, SPACE, floor)
                .await
                .expect("the room document set resolves");
            assert!(!truncated, "five documents never reach the bound");
            local_ids(&org, &ids)
        }
    };

    // No window: the whole live room, newest edit first. The other room and the deleted file are never in it.
    assert_eq!(list(HistoryFloor::Open).await, ["old", "new", "edge"]);
    // A window: only what entered the room at or after the floor. `old` was edited after the floor and is
    // still hidden, because the edit does not remove the text the member must not see. `edge` entered at
    // exactly the floor instant and is shown.
    assert_eq!(list(HistoryFloor::from_claims(Some(at(FLOOR)), 0).unwrap()).await, ["new", "edge"]);
    // A later floor hides more.
    assert_eq!(
        list(HistoryFloor::from_claims(Some(at("2026-11-12T00:00:00Z")), 0).unwrap()).await,
        ["new"]
    );
    // A window that is a revision alone shows nothing, and does not read the table to say so.
    assert!(list(HistoryFloor::from_claims(None, 4).unwrap()).await.is_empty());
    // A floor after every document leaves an empty set, which the caller refuses like an empty room.
    assert!(list(HistoryFloor::from_claims(Some(at("2026-12-01T00:00:00Z")), 0).unwrap())
        .await
        .is_empty());
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
async fn the_by_id_lookups_the_listing_and_the_file_read_hide_what_entered_before_the_window() {
    let pool = connect().await;
    let org = format!("historyfloor-reads-{}", std::process::id());
    setup(&pool, &org).await;
    seed_room(&pool, &org).await;
    let floor = Some(at(FLOOR));
    let ids: Vec<String> = ["old", "edge", "new", "gone", "elsewhere"]
        .iter()
        .map(|id| format!("{org}:{id}"))
        .collect();
    let no_grants: Vec<String> = Vec::new();

    // sources and freshness: the same predicate, so the two never disagree about what is in a room.
    let sources = |since: Option<DateTime<Utc>>| {
        let (pool, org, ids, no_grants) = (pool.clone(), org.clone(), ids.clone(), no_grants.clone());
        async move {
            let rows = sqlx::query_as::<_, (String, String, String, String, String, String)>(&sql_const("SOURCES_SQL"))
                .bind(&ids)
                .bind(&org)
                .bind(Some("u1"))
                .bind(&no_grants)
                .bind(Some(SPACE))
                .bind(since)
                .fetch_all(&pool)
                .await
                .expect("sources query");
            let mut found = local_ids(&org, &rows.into_iter().map(|row| row.0).collect::<Vec<_>>());
            found.sort();
            found
        }
    };
    let freshness = |since: Option<DateTime<Utc>>| {
        let (pool, org, ids, no_grants) = (pool.clone(), org.clone(), ids.clone(), no_grants.clone());
        async move {
            let rows = sqlx::query_as::<_, (String, String, String, String)>(&sql_const("FRESHNESS_SQL"))
                .bind(&ids)
                .bind(&org)
                .bind(Some("u1"))
                .bind(&no_grants)
                .bind(Some(SPACE))
                .bind(since)
                .fetch_all(&pool)
                .await
                .expect("freshness query");
            let mut found = local_ids(&org, &rows.into_iter().map(|row| row.0).collect::<Vec<_>>());
            found.sort();
            found
        }
    };
    assert_eq!(sources(None).await, ["edge", "new", "old"], "no window shows the live room");
    assert_eq!(sources(floor).await, ["edge", "new"], "the window hides the document before it");
    assert_eq!(freshness(None).await, ["edge", "new", "old"]);
    assert_eq!(freshness(floor).await, ["edge", "new"], "freshness agrees with sources");

    // The room listing.
    type ListRow = (
        String, String, String, String, String, String, String, String, String,
        Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>,
    );
    let listed = |since: Option<DateTime<Utc>>| {
        let (pool, org) = (pool.clone(), org.clone());
        async move {
            sqlx::query_as::<_, ListRow>(&sql_const("SPACE_DOCUMENTS_SQL"))
                .bind(&org)
                .bind(SPACE)
                .bind(Some("u1"))
                .bind(Vec::<String>::new())
                .bind(50_i64)
                .bind(None::<Vec<String>>)
                .bind(None::<String>)
                .bind(None::<String>)
                .bind(since)
                .fetch_all(&pool)
                .await
                .expect("listing query")
                .into_iter()
                .map(|row| row.0.trim_start_matches(&format!("{org}:")).to_string())
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(listed(None).await, ["old", "new", "edge"]);
    assert_eq!(listed(floor).await, ["new", "edge"]);

    // One room file: a document before the window answers like a missing id.
    type DocRow = (String, String, String, String, String, Option<String>, String);
    let read = |id: &'static str, since: Option<DateTime<Utc>>| {
        let (pool, org) = (pool.clone(), org.clone());
        async move {
            sqlx::query_as::<_, DocRow>(&sql_const("SPACE_DOCUMENT_SQL"))
                .bind(&org)
                .bind(SPACE)
                .bind(format!("{org}:{id}"))
                .bind(Some("u1"))
                .bind(Vec::<String>::new())
                .bind(since)
                .fetch_optional(&pool)
                .await
                .expect("document query")
        }
    };
    assert!(read("old", None).await.is_some(), "with no window the earlier file reads");
    assert!(read("old", floor).await.is_none(), "inside the window the earlier file is the 404 of a missing id");
    assert!(read("edge", floor).await.is_some(), "a file that entered at the floor instant reads");
    assert!(read("new", floor).await.is_some());
}

fn candidate(document_id: &str) -> ScoredCandidate {
    ScoredCandidate {
        knowledge_id: format!("k-{document_id}"),
        document_id: document_id.to_string(),
        text: "room text".to_string(),
        dense_score: 1.0,
        sparse_score: 0.0,
        rerank_score: 0.0,
        final_score: 1.0,
        chunk_index: 0,
        metadata: Default::default(),
    }
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
async fn the_history_gate_drops_a_surviving_candidate_whose_document_entered_before_the_window() {
    let pool = connect().await;
    let org = format!("historyfloor-gate-{}", std::process::id());
    setup(&pool, &org).await;
    seed_room(&pool, &org).await;
    let candidates = || {
        ["old", "edge", "new", "gone", "not-a-document"]
            .iter()
            .map(|id| candidate(&format!("{org}:{id}")))
            .collect::<Vec<_>>()
    };
    let survivors = |floor: HistoryFloor| {
        let (pool, org) = (pool.clone(), org.clone());
        let nodes = candidates();
        async move {
            let gate = SpaceHistoryGate { pool: &pool, org_id: &org, floor };
            let mut ctx = PostprocessCtx::new("what is in the room", ZdrMode::Disabled, false);
            gate.postprocess(nodes, &mut ctx)
                .await
                .expect("the gate reads")
                .into_iter()
                .map(|c| c.document_id.trim_start_matches(&format!("{org}:")).to_string())
                .collect::<Vec<_>>()
        }
    };
    // No window passes everything, including ids that are not documents: it is not this gate's to judge.
    assert_eq!(survivors(HistoryFloor::Open).await, ["old", "edge", "new", "gone", "not-a-document"]);
    // A window keeps a candidate only when its document is live and entered the room at or after the floor.
    // A deleted document and an id with no `documents` row have no reliable instant here, so they are hidden.
    assert_eq!(survivors(HistoryFloor::from_claims(Some(at(FLOOR)), 0).unwrap()).await, ["edge", "new"]);
    assert!(survivors(HistoryFloor::Closed).await.is_empty());
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
async fn the_room_wiki_hides_a_page_created_before_the_window_even_after_an_edit() {
    let pool = connect().await;
    let org = format!("historyfloor-wiki-{}", std::process::id());
    setup(&pool, &org).await;
    let workspace = "history-workspace";
    for (page, created, updated) in [
        // Created before the floor and edited after it: its current version is a whole-page snapshot.
        ("page-old", "2026-11-01 00:00:00+00", "2026-11-15 00:00:00+00"),
        ("page-edge", "2026-11-10 12:00:00+00", "2026-11-10 12:00:00+00"),
        ("page-new", "2026-11-12 00:00:00+00", "2026-11-12 00:00:00+00"),
    ] {
        sqlx::query(
            "INSERT INTO wiki_pages (page_id, org_id, workspace_id, title, path, current_version_id,
                                     page_status, backlinks, created_at, updated_at)
             VALUES ($1, $2, $3, 'history window page', $4, $5, 'published', '[]'::jsonb,
                     $6::timestamptz, $7::timestamptz)",
        )
        .bind(format!("{org}:{page}"))
        .bind(&org)
        .bind(workspace)
        .bind(format!("/{page}"))
        .bind(format!("{org}:{page}:v1"))
        .bind(created)
        .bind(updated)
        .execute(&pool)
        .await
        .expect("seed wiki page");
        sqlx::query(
            "INSERT INTO wiki_page_versions (version_id, page_id, content, source_refs, proposed_by_agent)
             VALUES ($1, $2, 'history window text', '[]'::jsonb, NULL)",
        )
        .bind(format!("{org}:{page}:v1"))
        .bind(format!("{org}:{page}"))
        .execute(&pool)
        .await
        .expect("seed wiki version");
    }
    let workspaces = vec![workspace.to_string()];
    let found = |since: Option<DateTime<Utc>>| {
        let (pool, org, workspaces) = (pool.clone(), org.clone(), workspaces.clone());
        async move {
            let mut pages: Vec<String> = wiki::wiki_search_since(
                &pool, "history window", &org, 20, None, &workspaces, &[], since,
            )
            .await
            .expect("wiki search")
            .into_iter()
            .map(|page| page.page_id.trim_start_matches(&format!("{org}:")).to_string())
            .collect();
            pages.sort();
            pages
        }
    };
    assert_eq!(found(None).await, ["page-edge", "page-new", "page-old"], "no window shows the room wiki");
    assert_eq!(found(Some(at(FLOOR))).await, ["page-edge", "page-new"]);
    // `wiki_search` is `wiki_search_since` with no window.
    let plain = wiki::wiki_search(&pool, "history window", &org, 20, None, &workspaces, &[])
        .await
        .expect("wiki search without a window");
    assert_eq!(plain.len(), 3);
}
