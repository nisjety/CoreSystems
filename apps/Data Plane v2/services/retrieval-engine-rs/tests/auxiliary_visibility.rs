use sqlx::PgPool;

use retrieval_engine::search::{contradictions, graph, timeline, wiki};

fn test_db_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

async fn seed(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        CREATE TABLE documents (
            document_id TEXT PRIMARY KEY,
            org_id TEXT NOT NULL,
            owner_id TEXT NOT NULL,
            visibility TEXT NOT NULL,
            title TEXT NOT NULL,
            source TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            deleted_at TIMESTAMPTZ
        );
        CREATE TABLE knowledge_units (
            knowledge_id TEXT PRIMARY KEY,
            document_id TEXT NOT NULL,
            org_id TEXT NOT NULL
        );
        CREATE TABLE graph_entities (
            entity_id TEXT PRIMARY KEY,
            org_id TEXT NOT NULL,
            entity_type TEXT NOT NULL,
            entity_text TEXT NOT NULL,
            confidence DOUBLE PRECISION,
            source_refs JSONB
        );
        CREATE TABLE graph_relationships (
            rel_id TEXT PRIMARY KEY,
            org_id TEXT NOT NULL,
            entity_a_id TEXT NOT NULL,
            entity_b_id TEXT NOT NULL,
            relation_type TEXT NOT NULL,
            confidence DOUBLE PRECISION,
            source_refs JSONB
        );
        CREATE TABLE graph_claims (
            claim_id TEXT PRIMARY KEY,
            org_id TEXT NOT NULL,
            claim_text TEXT NOT NULL,
            entity_ids JSONB,
            confidence DOUBLE PRECISION,
            source_refs JSONB,
            contradicted_by_claim_ids JSONB,
            claim_status TEXT
        );
        CREATE TABLE graph_communities (
            community_id TEXT PRIMARY KEY,
            org_id TEXT NOT NULL,
            entity_ids JSONB NOT NULL,
            summary TEXT,
            level INTEGER NOT NULL
        );
        CREATE TABLE wiki_pages (
            page_id TEXT PRIMARY KEY,
            org_id TEXT NOT NULL,
            workspace_id TEXT NOT NULL,
            title TEXT NOT NULL,
            path TEXT NOT NULL,
            current_version_id TEXT,
            page_status TEXT,
            backlinks JSONB,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            deleted_at TIMESTAMPTZ
        );
        CREATE TABLE wiki_page_versions (
            version_id TEXT PRIMARY KEY,
            page_id TEXT NOT NULL,
            content TEXT,
            source_refs JSONB,
            proposed_by_agent TEXT
        );

        INSERT INTO documents (document_id, org_id, owner_id, visibility, title, source, status) VALUES
          ('doc-org', 'org-a', 'user-b', 'org', 'Org', 'fixture', 'ready'),
          ('doc-owner', 'org-a', 'user-a', 'private', 'Owner', 'fixture', 'ready'),
          ('doc-grant', 'org-a', 'user-b', 'shared', 'Grant', 'fixture', 'ready'),
          ('doc-hidden', 'org-a', 'user-b', 'shared', 'Hidden', 'fixture', 'ready'),
          ('doc-other-org', 'org-b', 'user-a', 'org', 'Other', 'fixture', 'ready');
        INSERT INTO knowledge_units (knowledge_id, document_id, org_id) VALUES
          ('ku-org', 'doc-org', 'org-a'),
          ('ku-owner', 'doc-owner', 'org-a'),
          ('ku-grant', 'doc-grant', 'org-a'),
          ('ku-hidden', 'doc-hidden', 'org-a'),
          ('ku-other-org', 'doc-other-org', 'org-b');
        INSERT INTO graph_entities (entity_id, org_id, entity_type, entity_text, confidence, source_refs) VALUES
          ('entity-org', 'org-a', 'fixture', 'synthetic term org', 1, '["ku-org"]'),
          ('entity-owner', 'org-a', 'fixture', 'synthetic term owner', 1, '["ku-owner"]'),
          ('entity-grant', 'org-a', 'fixture', 'synthetic term grant', 1, '["ku-grant"]'),
          ('entity-hidden', 'org-a', 'fixture', 'synthetic term hidden', 1, '["ku-hidden"]'),
          ('entity-mixed', 'org-a', 'fixture', 'synthetic term mixed', 1, '["ku-org", "ku-hidden"]'),
          ('entity-other-org', 'org-b', 'fixture', 'synthetic term other', 1, '["ku-other-org"]');
        INSERT INTO graph_relationships
          (rel_id, org_id, entity_a_id, entity_b_id, relation_type, confidence, source_refs) VALUES
          ('rel-visible', 'org-a', 'entity-org', 'entity-grant', 'related', 1, '["ku-grant"]'),
          ('rel-hidden', 'org-a', 'entity-org', 'entity-hidden', 'related', 1, '["ku-hidden"]');
        INSERT INTO graph_claims
          (claim_id, org_id, claim_text, entity_ids, confidence, source_refs,
           contradicted_by_claim_ids, claim_status) VALUES
          ('claim-visible', 'org-a', 'synthetic visible claim', '["entity-org"]', 1,
           '["ku-org"]', '["claim-granted"]', 'active'),
          ('claim-granted', 'org-a', 'synthetic granted claim', '["entity-grant"]', 1,
           '["ku-grant"]', '["claim-visible"]', 'active'),
          ('claim-hidden', 'org-a', 'synthetic hidden claim', '["entity-hidden"]', 1,
           '["ku-hidden"]', '["claim-visible"]', 'active'),
          ('claim-mixed', 'org-a', 'synthetic mixed claim', '["entity-org"]', 1,
           '["ku-org", "ku-hidden"]', '["claim-visible"]', 'active'),
          ('claim-hidden-target', 'org-a', 'synthetic hidden target claim', '["entity-org"]', 1,
           '["ku-org"]', '["claim-hidden"]', 'active');
        INSERT INTO graph_communities (community_id, org_id, entity_ids, summary, level) VALUES
          ('community-visible', 'org-a', '["entity-org", "entity-grant"]', 'synthetic visible summary', 0),
          ('community-mixed', 'org-a', '["entity-org", "entity-hidden"]', 'synthetic hidden summary', 0);

        INSERT INTO wiki_pages
          (page_id, org_id, workspace_id, title, path, current_version_id, page_status, backlinks, deleted_at) VALUES
          ('page-manual', 'org-a', 'workspace-a', 'Synthetic manual', '/manual', 'version-manual', 'published', '[]', NULL),
          ('page-visible', 'org-a', 'workspace-a', 'Synthetic visible', '/visible', 'version-visible', 'published', '[]', NULL),
          ('page-hidden', 'org-a', 'workspace-a', 'Synthetic hidden', '/hidden', 'version-hidden', 'published', '[]', NULL),
          ('page-mixed', 'org-a', 'workspace-a', 'Synthetic mixed', '/mixed', 'version-mixed', 'published', '[]', NULL),
          ('page-workspace', 'org-a', 'workspace-b', 'Synthetic workspace', '/workspace', 'version-workspace', 'published', '[]', NULL),
          ('page-draft', 'org-a', 'workspace-a', 'Synthetic draft', '/draft', 'version-draft', 'draft', '[]', NULL),
          ('page-deleted', 'org-a', 'workspace-a', 'Synthetic deleted', '/deleted', 'version-deleted', 'published', '[]', NOW());
        INSERT INTO wiki_page_versions (version_id, page_id, content, source_refs, proposed_by_agent) VALUES
          ('version-manual', 'page-manual', 'synthetic term manual', '[]', NULL),
          ('version-visible', 'page-visible', 'synthetic term visible', '["ku-org"]', 'fixture-agent'),
          ('version-hidden', 'page-hidden', 'synthetic term hidden', '["ku-hidden"]', 'fixture-agent'),
          ('version-mixed', 'page-mixed', 'synthetic term mixed', '["ku-org", "ku-hidden"]', 'fixture-agent'),
          ('version-workspace', 'page-workspace', 'synthetic term workspace', '["ku-org"]', 'fixture-agent'),
          ('version-draft', 'page-draft', 'synthetic term draft', '["ku-org"]', 'fixture-agent'),
          ('version-deleted', 'page-deleted', 'synthetic term deleted', '["ku-org"]', 'fixture-agent');
        "#,
    )
    .execute(pool)
    .await
    .expect("seed isolated auxiliary visibility fixture");
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
async fn auxiliary_results_never_cross_tenant_or_document_visibility() {
    let url = test_db_url().expect("TEST_DATABASE_URL is required for this ignored test");
    let pool = PgPool::connect(&url)
        .await
        .expect("connect isolated postgres");
    seed(&pool).await;
    let grants = vec!["doc-grant".to_string()];

    let timeline =
        timeline::temporal_search(&pool, "org-a", Some("user-a"), &grants, None, None, 50)
            .await
            .expect("timeline");
    let timeline_ids: std::collections::HashSet<_> = timeline
        .iter()
        .map(|entry| entry.document_id.as_str())
        .collect();
    assert_eq!(timeline_ids.len(), 3);
    assert!(timeline_ids.contains("doc-org"));
    assert!(timeline_ids.contains("doc-owner"));
    assert!(timeline_ids.contains("doc-grant"));

    let entities = graph::graph_expansion_search(
        &pool,
        "synthetic term",
        "org-a",
        20,
        Some("user-a"),
        &grants,
    )
    .await
    .expect("graph expansion");
    let entity_ids: Vec<_> = entities
        .iter()
        .map(|entity| entity.entity_id.as_str())
        .collect();
    assert_eq!(entity_ids.len(), 3);
    assert!(!entity_ids.contains(&"entity-hidden"));
    assert!(!entity_ids.contains(&"entity-mixed"));
    assert!(!entity_ids.contains(&"entity-other-org"));

    let visible_entity_ids: Vec<String> = entities
        .iter()
        .map(|entity| entity.entity_id.clone())
        .collect();
    let communities = graph::community_summary_search(&pool, &visible_entity_ids, "org-a")
        .await
        .expect("community summaries");
    assert_eq!(communities.len(), 1);
    assert_eq!(communities[0].community_id, "community-visible");

    let claims =
        contradictions::search_contradictions(&pool, "org-a", None, 20, Some("user-a"), &grants)
            .await
            .expect("contradictions");
    let claim_ids: std::collections::HashSet<_> =
        claims.iter().map(|claim| claim.claim_id.as_str()).collect();
    assert_eq!(claim_ids.len(), 2);
    assert!(claim_ids.contains("claim-visible"));
    assert!(claim_ids.contains("claim-granted"));

    let pages = wiki::wiki_search(
        &pool,
        "synthetic term",
        "org-a",
        20,
        Some("user-a"),
        &["workspace-a".to_string()],
        &grants,
    )
    .await
    .expect("wiki search");
    let page_ids: std::collections::HashSet<_> =
        pages.iter().map(|page| page.page_id.as_str()).collect();
    assert_eq!(page_ids.len(), 2);
    assert!(page_ids.contains("page-manual"));
    assert!(page_ids.contains("page-visible"));
}
