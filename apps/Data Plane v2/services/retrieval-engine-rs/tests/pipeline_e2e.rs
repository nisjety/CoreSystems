//! §16.4.1 — End-to-end pipeline test scaffold.
//!
//! Default-ignored so plain `cargo test` doesn't try to hit the stack.
//! Run with the full docker-compose stack up and `DATAPLANE_E2E_BASE`
//! pointing at retrieval-engine.
//!
//! ```ignore
//! make docker-up
//! DATAPLANE_E2E_BASE=http://localhost:8004 \
//!   cargo test --test pipeline_e2e -- --ignored --nocapture
//! ```

use std::time::Duration;

const POLL_BUDGET: Duration = Duration::from_secs(60);
const POLL_INTERVAL: Duration = Duration::from_millis(500);

fn base_url() -> String {
    std::env::var("DATAPLANE_E2E_BASE").unwrap_or_else(|_| "http://localhost:8004".to_string())
}

fn docs_url() -> String {
    std::env::var("DATAPLANE_DOCS_BASE").unwrap_or_else(|_| "http://localhost:8010".to_string())
}

async fn wait_until<F, Fut>(label: &str, mut f: F) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = std::time::Instant::now() + POLL_BUDGET;
    while std::time::Instant::now() < deadline {
        if f().await {
            return true;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    eprintln!("e2e: gave up waiting for {label}");
    false
}

#[tokio::test]
#[ignore = "requires docker-compose stack"]
async fn happy_path() {
    let client = reqwest::Client::new();
    let org = format!("e2e-{}", uuid::Uuid::new_v4());

    // 1. Plant one document via documents-api.
    let bulk_url = format!("{}/v1/documents/bulk", docs_url());
    let body = serde_json::json!({
        "documents": [{
            "title": "alpha",
            "type": "markdown",
            "source": "manual",
            "content": "The quick brown fox jumps over the lazy dog.",
        }]
    });
    let resp = client
        .post(&bulk_url)
        .header("X-Org-Id", &org)
        .json(&body)
        .send()
        .await
        .expect("bulk ingest");
    assert!(resp.status().is_success(), "bulk ingest failed");

    // 2. Wait for embedding.
    // Real assertion lives in the worker integration test — here we just
    // give the pipeline some time and then issue a retrieve.
    let _ = wait_until("embedding-done", || async { true }).await;
    tokio::time::sleep(Duration::from_secs(5)).await;

    // 3. Retrieve.
    let retrieve_url = format!("{}/v1/retrieve", base_url());
    let resp = client
        .post(&retrieve_url)
        .json(&serde_json::json!({
            "org_id": org,
            "query": "quick fox",
            "top_k": 5,
        }))
        .send()
        .await
        .expect("retrieve");
    assert!(resp.status().is_success(), "retrieve failed");
}

#[tokio::test]
#[ignore = "requires docker-compose stack — scaffold only"]
async fn zdr_reject_filters_restricted() {
    // TODO: scaffold for §16.1.3 + §16.4.1 — plant restricted doc, retrieve
    // with zdr_mode=reject, assert it's filtered and zdr_actions_applied
    // contains "reject_mode_filtered_restricted".
}

#[tokio::test]
#[ignore = "requires docker-compose stack — scaffold only"]
async fn cache_invalidation_via_org_version() {
    // TODO: scaffold for §16.2.2 — plant, retrieve, update content, retrieve
    // again, assert the new content reaches the caller within 1s of mutation.
}
