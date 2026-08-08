//! Live Meilisearch round-trip. Gated on `MEILISEARCH_TEST_URL` (+ optional
//! `MEILISEARCH_TEST_API_KEY`) pointing at a disposable Meilisearch instance,
//! and `#[ignore]` by default — mirrors this repo's existing convention for
//! external-dependency tests (see `graph-index-rs::neo4j`'s
//! `NEO4J_TEST_URL`-gated test, `retrieval-engine-rs/tests/*`'s
//! `TEST_DATABASE_URL`-gated ones).
//!
//! Proves upsert → search-visible and delete → search-invisible against a
//! REAL server, including Meilisearch's asynchronous indexing (add/delete
//! calls return a queued task; this polls search results rather than
//! trusting the 202 alone). Uses a throwaway, uniquely-named index per run so
//! concurrent runs and repeated runs never collide or accumulate state.

use meilisearch_adapter_rs::meilisearch::MeilisearchClient;
use meilisearch_adapter_rs::model::KeywordDocument;
use std::time::Duration;

fn test_config() -> Option<(String, String)> {
    let url = std::env::var("MEILISEARCH_TEST_URL").ok()?;
    let key = std::env::var("MEILISEARCH_TEST_API_KEY").unwrap_or_default();
    Some((url, key))
}

async fn poll_until<F, Fut>(attempts: u32, delay: Duration, mut check: F) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..attempts {
        if check().await {
            return true;
        }
        tokio::time::sleep(delay).await;
    }
    false
}

async fn search_hits(
    base_url: &str,
    index_uid: &str,
    api_key: &str,
    filter: &str,
) -> Vec<serde_json::Value> {
    let http = reqwest::Client::new();
    let url = format!("{base_url}/indexes/{index_uid}/search");
    let mut req = http
        .post(&url)
        .json(&serde_json::json!({ "q": "", "filter": filter }));
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }
    let resp = req.send().await.expect("search request");
    let body: serde_json::Value = resp.json().await.expect("search response json");
    body["hits"].as_array().cloned().unwrap_or_default()
}

async fn delete_index(base_url: &str, index_uid: &str, api_key: &str) {
    let http = reqwest::Client::new();
    let url = format!("{base_url}/indexes/{index_uid}");
    let mut req = http.delete(&url);
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }
    let _ = req.send().await;
}

#[tokio::test]
#[ignore = "requires MEILISEARCH_TEST_URL pointing to disposable Meilisearch"]
async fn upsert_makes_a_document_searchable_and_delete_removes_it() {
    let Some((base_url, api_key)) = test_config() else {
        panic!("MEILISEARCH_TEST_URL");
    };
    // Unique per-run index so repeated/concurrent runs never interfere.
    let index_uid = format!("test-keyword-{}", uuid::Uuid::new_v4().simple());
    let org_id = format!("org-test-{}", uuid::Uuid::new_v4().simple());
    let document_id = format!("doc-{}", uuid::Uuid::new_v4().simple());
    let knowledge_id = format!("kid-{}", uuid::Uuid::new_v4().simple());

    let client = MeilisearchClient::new(base_url.clone(), index_uid.clone(), api_key.clone())
        .expect("client");
    client.ensure_index().await.expect("ensure_index");

    let doc = KeywordDocument {
        id: knowledge_id.clone(),
        org_id: org_id.clone(),
        document_id: document_id.clone(),
        knowledge_id: knowledge_id.clone(),
        chunk_index: 0,
        source: "upload".into(),
        title: "Invoice for order INV-99182".into(),
        body: "Shipped under tracking code SKU-77102-B to Trondheim.".into(),
        content_hash: Some("test-hash".into()),
        acl_tags: vec![],
        updated_at: chrono::Utc::now().timestamp(),
    };
    client.upsert(&[doc]).await.expect("upsert");

    let filter = format!("org_id = \"{org_id}\" AND document_id = \"{document_id}\"");
    let found = poll_until(20, Duration::from_millis(250), || {
        let base_url = base_url.clone();
        let index_uid = index_uid.clone();
        let api_key = api_key.clone();
        let filter = filter.clone();
        async move {
            !search_hits(&base_url, &index_uid, &api_key, &filter)
                .await
                .is_empty()
        }
    })
    .await;
    assert!(
        found,
        "document did not become searchable within the poll window"
    );

    client
        .delete_by_document(&org_id, &document_id)
        .await
        .expect("delete_by_document");

    let gone = poll_until(20, Duration::from_millis(250), || {
        let base_url = base_url.clone();
        let index_uid = index_uid.clone();
        let api_key = api_key.clone();
        let filter = filter.clone();
        async move {
            search_hits(&base_url, &index_uid, &api_key, &filter)
                .await
                .is_empty()
        }
    })
    .await;
    assert!(
        gone,
        "document was still searchable after delete_by_document"
    );

    delete_index(&base_url, &index_uid, &api_key).await;
}
