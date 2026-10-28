//! CR-T15: the rerank input's context join reads only this organisation's units whose context is in
//! force. It runs on a bare disposable PostgreSQL named by `TEST_DATABASE_URL`, which the
//! rust-integration job supplies. Without that URL the test skips, and under `REQUIRE_CONTRACT` it
//! fails instead, because a database test that skips proves nothing.
//!
//! The fixture lives in a schema of its own, which is the search path of every connection of the
//! test pool. So it never touches a `knowledge_units` table another test binary keeps in `public`.

mod common;

use std::collections::HashMap;

use retrieval_engine::pipeline::orchestrator::attach_chunk_contexts;
use retrieval_engine::pipeline::types::ScoredCandidate;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

const SCHEMA: &str = "cr_t15_chunk_context";

fn test_db_url_or_skip() -> Option<String> {
    let url = std::env::var("TEST_DATABASE_URL").ok();
    if url.is_none() {
        if std::env::var_os("REQUIRE_CONTRACT").is_some() {
            panic!("TEST_DATABASE_URL is not set but REQUIRE_CONTRACT is: a database test that skips proves nothing");
        }
        eprintln!("skipped: TEST_DATABASE_URL is not set");
    }
    url
}

/// A pool whose every connection searches the fixture schema first, as the join's own SQL expects.
async fn connect(url: &str) -> PgPool {
    PgPoolOptions::new()
        .max_connections(2)
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query(&format!("SET search_path TO {SCHEMA}, public"))
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(url)
        .await
        .expect("connect to the disposable database")
}

/// The columns the join reads, with the production key shape (organisation, unit). The runtime role
/// is granted after the tables exist, and the schema is granted to it, as the join's own transaction
/// (`SET LOCAL ROLE dataplane_app`) needs.
async fn setup(pool: &PgPool) {
    common::grant_rls_runtime_role(pool).await;
    sqlx::raw_sql(&format!(
        "DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;
         CREATE SCHEMA {SCHEMA};
         CREATE TABLE {SCHEMA}.knowledge_units (
             org_id          TEXT NOT NULL,
             knowledge_id    TEXT NOT NULL,
             document_id     TEXT NOT NULL,
             text            TEXT NOT NULL,
             chunk_context   TEXT,
             context_status  TEXT,
             PRIMARY KEY (org_id, knowledge_id)
         );
         INSERT INTO {SCHEMA}.knowledge_units
             (org_id, knowledge_id, document_id, text, chunk_context, context_status) VALUES
             ('org-a', 'ready-1',   'doc-1', 'Margin 31%.', 'Fra ACMEs Q2-rapport.', 'ready'),
             ('org-a', 'legacy-1',  'doc-1', 'Lønn.',       'Lønnsdata for 2024.',   'legacy'),
             ('org-a', 'pending-1', 'doc-2', 'Pensjon.',    'Skal ikke brukes.',     'pending'),
             ('org-a', 'blank-1',   'doc-2', 'Blank.',      '   ',                   'ready'),
             ('org-a', 'null-1',    'doc-3', 'Null.',       NULL,                    'ready'),
             ('org-a', 'zdr-1',     'doc-3', 'ZDR.',        NULL,                    'skipped_zdr'),
             ('org-b', 'ready-1',   'doc-9', 'Annen org.',  'Kontekst for org B.',   'ready');
         GRANT USAGE ON SCHEMA {SCHEMA} TO dataplane_app;
         GRANT SELECT ON ALL TABLES IN SCHEMA {SCHEMA} TO dataplane_app;"
    ))
    .execute(pool)
    .await
    .expect("create the fixture schema, its rows and the runtime grants");
}

fn candidate(knowledge_id: &str) -> ScoredCandidate {
    ScoredCandidate {
        knowledge_id: knowledge_id.to_string(),
        document_id: "doc".to_string(),
        text: format!("raw text of {knowledge_id}"),
        dense_score: 0.0,
        sparse_score: 0.0,
        rerank_score: 0.0,
        final_score: 0.0,
        chunk_index: 0,
        metadata: HashMap::new(),
    }
}

#[tokio::test]
async fn the_join_reads_only_the_orgs_units_whose_context_is_in_force() {
    let Some(url) = test_db_url_or_skip() else {
        return;
    };
    let pool = connect(&url).await;
    setup(&pool).await;

    // In force: `ready` and `legacy` with a context that is not blank.
    let mut candidates: Vec<ScoredCandidate> = [
        "ready-1",
        "legacy-1",
        "pending-1",
        "blank-1",
        "null-1",
        "zdr-1",
        "missing-1",
    ]
    .iter()
    .map(|id| candidate(id))
    .collect();
    attach_chunk_contexts(&pool, "org-a", &mut candidates)
        .await
        .expect("the join reads the fixture");

    let got: HashMap<&str, Option<&str>> = candidates
        .iter()
        .map(|c| (c.knowledge_id.as_str(), c.chunk_context()))
        .collect();
    assert_eq!(got["ready-1"], Some("Fra ACMEs Q2-rapport."));
    assert_eq!(got["legacy-1"], Some("Lønnsdata for 2024."));
    for id in ["pending-1", "blank-1", "null-1", "zdr-1", "missing-1"] {
        assert_eq!(got[id], None, "{id} has no context in force");
    }

    // The same knowledge id in another organisation is another unit: org-b's context is read for
    // org-b only, and an org-a request never reads it.
    let mut other = vec![candidate("ready-1")];
    attach_chunk_contexts(&pool, "org-b", &mut other)
        .await
        .expect("the join reads org-b");
    assert_eq!(other[0].chunk_context(), Some("Kontekst for org B."));

    // Nothing to join is a no-op, not a query.
    let mut none: Vec<ScoredCandidate> = Vec::new();
    attach_chunk_contexts(&pool, "org-a", &mut none)
        .await
        .expect("an empty candidate list joins nothing");

    sqlx::raw_sql(&format!("DROP SCHEMA {SCHEMA} CASCADE"))
        .execute(&pool)
        .await
        .expect("drop the fixture schema");
}
