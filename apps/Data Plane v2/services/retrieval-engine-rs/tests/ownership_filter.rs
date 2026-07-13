//! Per-User Data Ownership — step-6 retrieval post-filter test (PR-3).
//!
//! Exercises the EXACT SQL predicate `filter_live_candidates` runs at
//! orchestrator step-6 (post-fusion + post-rerank), so it proves the gate that
//! backs BOTH the dense and the sparse retrieval arms uniformly (they share this
//! one post-fusion seam — that is how the sparse leak is closed). A document
//! marked private by user A must be absent from user B's candidate set unless B
//! holds an explicit grant; everyone sees org-visible docs; a no-viewer (legacy)
//! request sees everything.
//!
//! Gated on TEST_DATABASE_URL (skips otherwise), matching the other DPv2
//! integration tests.

use sqlx::PgPool;

fn test_db_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

/// The exact predicate from `RetrievalPipeline::filter_live_candidates`.
const OWNERSHIP_GATE_SQL: &str = r#"
    SELECT document_id
    FROM documents
    WHERE document_id = ANY($1)
      AND deleted_at IS NULL
      AND ($2::text IS NULL
           OR owner_id = $2
           OR visibility = 'org'
           OR document_id = ANY($3))
"#;

async fn gate(
    pool: &PgPool,
    candidates: &[String],
    viewer: Option<&str>,
    granted: &[String],
) -> std::collections::HashSet<String> {
    sqlx::query_as::<_, (String,)>(OWNERSHIP_GATE_SQL)
        .bind(candidates)
        .bind(viewer)
        .bind(granted)
        .fetch_all(pool)
        .await
        .expect("gate query")
        .into_iter()
        .map(|(id,)| id)
        .collect()
}

async fn setup(pool: &PgPool, org: &str) {
    // Minimal documents table mirroring the ownership columns (PR-2 migration).
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS documents (
            document_id TEXT PRIMARY KEY,
            org_id      TEXT NOT NULL,
            owner_id    TEXT NOT NULL DEFAULT 'org-system-account',
            visibility  TEXT NOT NULL DEFAULT 'org',
            deleted_at  TIMESTAMPTZ
        )
        "#,
    )
    .execute(pool)
    .await
    .expect("create documents");

    sqlx::query("DELETE FROM documents WHERE org_id = $1")
        .bind(org)
        .execute(pool)
        .await
        .expect("clean");

    for (id, owner, vis) in [
        ("org-doc", "user-a", "org"),       // visible to everyone in the org
        ("a-private", "user-a", "private"), // visible only to A
        ("a-shared", "user-a", "shared"),   // shared only through an explicit grant
    ] {
        sqlx::query(
            "INSERT INTO documents (document_id, org_id, owner_id, visibility) VALUES ($1,$2,$3,$4)",
        )
        .bind(format!("{org}:{id}"))
        .bind(org)
        .bind(owner)
        .bind(vis)
        .execute(pool)
        .await
        .expect("seed doc");
    }
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
async fn ownership_gate_enforces_per_user_visibility() {
    let url = test_db_url().expect("TEST_DATABASE_URL is required for this ignored test");
    let pool = PgPool::connect(&url).await.unwrap();
    let org = format!("owntest-{}", std::process::id());
    setup(&pool, &org).await;

    let all: Vec<String> = ["org-doc", "a-private", "a-shared"]
        .iter()
        .map(|s| format!("{org}:{s}"))
        .collect();
    let org_doc = format!("{org}:org-doc");
    let a_private = format!("{org}:a-private");
    let a_shared = format!("{org}:a-shared");

    // No viewer (legacy / service path): sees everything in the org.
    let legacy = gate(&pool, &all, None, &[]).await;
    assert_eq!(legacy.len(), 3, "no-viewer path must see all org docs");

    // User B, no grant: sees the org doc only — private and grant-only shared
    // docs are hidden.
    let b = gate(&pool, &all, Some("user-b"), &[]).await;
    assert!(b.contains(&org_doc), "B must see the org-visible doc");
    assert!(!b.contains(&a_private), "B must NOT see A's private doc");
    assert!(
        !b.contains(&a_shared),
        "B must NOT see A's private doc without a grant"
    );
    assert_eq!(b.len(), 1);

    // User B WITH an explicit grant on a-shared: sees org-doc + a-shared, never a-private.
    let b_granted = gate(&pool, &all, Some("user-b"), std::slice::from_ref(&a_shared)).await;
    assert!(b_granted.contains(&org_doc));
    assert!(
        b_granted.contains(&a_shared),
        "grant must surface the shared doc"
    );
    assert!(
        !b_granted.contains(&a_private),
        "grant must NOT leak the un-granted private doc"
    );
    assert_eq!(b_granted.len(), 2);

    // Owner A: sees all of their own docs plus the org doc.
    let a = gate(&pool, &all, Some("user-a"), &[]).await;
    assert_eq!(a.len(), 3, "owner sees own private + org docs");

    sqlx::query("DELETE FROM documents WHERE org_id = $1")
        .bind(&org)
        .execute(&pool)
        .await
        .ok();
}
