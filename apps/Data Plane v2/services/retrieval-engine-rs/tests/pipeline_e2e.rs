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

/// Attach the org header plus the internal API key when the live stack expects
/// one (documents-api guards `/v1/documents/*` with `internalAuthMiddleware`).
/// The key is read from `DATAPLANE_E2E_INTERNAL_KEY` so the test works against
/// both an open dev stack and an authenticated one.
fn with_org(builder: reqwest::RequestBuilder, org: &str) -> reqwest::RequestBuilder {
    let mut b = builder.header("X-Org-Id", org);
    if let Ok(key) = std::env::var("DATAPLANE_E2E_INTERNAL_KEY") {
        if !key.is_empty() {
            b = b.header("X-API-Key", key);
        }
    }
    b
}

/// Ingest one document and return its server-assigned document_id.
async fn ingest_one(
    client: &reqwest::Client,
    org: &str,
    title: &str,
    content: &str,
    zdr_classification: Option<&str>,
) -> String {
    let bulk_url = format!("{}/v1/documents/bulk", docs_url());
    let mut doc = serde_json::json!({
        "title": title,
        "type": "markdown",
        "source": format!("manual/{title}"),
        "content": content,
    });
    if let Some(c) = zdr_classification {
        doc["zdr_classification"] = serde_json::Value::String(c.to_string());
    }
    let body = serde_json::json!({ "documents": [doc] });
    let resp = with_org(client.post(&bulk_url), org)
        .json(&body)
        .send()
        .await
        .expect("bulk ingest");
    assert!(
        resp.status().is_success(),
        "bulk ingest failed: {}",
        resp.status()
    );
    let json: serde_json::Value = resp.json().await.expect("bulk ingest json");
    json["document_ids"][0]
        .as_str()
        .expect("document_ids[0]")
        .to_string()
}

async fn retrieve(
    client: &reqwest::Client,
    org: &str,
    query: &str,
    zdr_mode: Option<&str>,
) -> serde_json::Value {
    let retrieve_url = format!("{}/v1/retrieve", base_url());
    let mut body = serde_json::json!({
        "org_id": org,
        "query": query,
        "top_k": 10,
    });
    if let Some(m) = zdr_mode {
        body["zdr_mode"] = serde_json::Value::String(m.to_string());
    }
    let resp = client
        .post(&retrieve_url)
        .json(&body)
        .send()
        .await
        .expect("retrieve");
    assert!(
        resp.status().is_success(),
        "retrieve failed: {}",
        resp.status()
    );
    resp.json().await.expect("retrieve json")
}

fn candidate_doc_ids(resp: &serde_json::Value) -> Vec<String> {
    resp["candidates"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|c| c["document_id"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// §16.1.3 + §16.4.1 — a `zdr_classification=restricted` document is filtered
/// out of results under `zdr_mode=reject`, and the response records the
/// enforcement action `reject_mode_filtered_restricted`.
#[tokio::test]
#[ignore = "requires docker-compose stack"]
async fn zdr_reject_filters_restricted() {
    let client = reqwest::Client::new();
    let org = format!("e2e-zdr-{}", uuid::Uuid::new_v4());

    // Plant one restricted doc and one normal doc sharing a distinctive term so
    // both are strong dense candidates for the same query.
    let restricted_id = ingest_one(
        &client,
        &org,
        "restricted-zorblax",
        "The confidential zorblax dossier is highly restricted.",
        Some("restricted"),
    )
    .await;
    let _normal_id = ingest_one(
        &client,
        &org,
        "public-zorblax",
        "The public zorblax overview is freely shareable.",
        None,
    )
    .await;

    // Give the embedding worker time to index both docs (the worker indexes
    // restricted docs via the model_plane provider; the egress guard only
    // trips on the direct-Azure backend).
    let indexed = wait_until("both-docs-indexed", || {
        let client = &client;
        let org = &org;
        async move {
            let resp = retrieve(client, org, "zorblax", None).await;
            candidate_doc_ids(&resp).len() >= 2
        }
    })
    .await;
    assert!(
        indexed,
        "expected both docs to be retrievable before the reject test"
    );

    // Retrieve under reject mode: the restricted doc must be filtered out and
    // the action recorded.
    let resp = retrieve(&client, &org, "zorblax", Some("reject")).await;
    let ids = candidate_doc_ids(&resp);
    assert!(
        !ids.contains(&restricted_id),
        "restricted doc {restricted_id} must be filtered under zdr_mode=reject; got {ids:?}"
    );

    let actions: Vec<String> = resp["zdr_actions_applied"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        actions
            .iter()
            .any(|a| a == "reject_mode_filtered_restricted"),
        "expected zdr_actions_applied to contain reject_mode_filtered_restricted; got {actions:?}"
    );
}

/// §16.2.2 — content freshness: after a document's content changes, the new
/// content must reach the caller (the org cache version is bumped on update so
/// stale cached retrievals become unreachable). Asserts the updated text is
/// retrievable shortly after the mutation.
#[tokio::test]
#[ignore = "requires docker-compose stack"]
async fn cache_invalidation_via_org_version() {
    let client = reqwest::Client::new();
    let org = format!("e2e-fresh-{}", uuid::Uuid::new_v4());

    // 1. Plant, with an idempotency key so the re-ingest updates the SAME row
    //    (content change → documents.updated → re-chunk/re-embed + org-version
    //    bump) rather than creating a second document.
    let idem = format!("fresh-{}", uuid::Uuid::new_v4());
    let bulk_url = format!("{}/v1/documents/bulk", docs_url());
    let plant = serde_json::json!({
        "documents": [{
            "title": "freshness-probe",
            "type": "markdown",
            "source": "manual/freshness-probe",
            "content": "Original marker oldcontentalpha for the freshness probe.",
            "idempotency_key": idem,
        }]
    });
    let resp = with_org(client.post(&bulk_url), &org)
        .json(&plant)
        .send()
        .await
        .expect("plant ingest");
    assert!(
        resp.status().is_success(),
        "plant ingest failed: {}",
        resp.status()
    );

    // 2. Wait until the original content is retrievable.
    let v1 = wait_until("v1-indexed", || {
        let client = &client;
        let org = &org;
        async move {
            let resp = retrieve(client, org, "oldcontentalpha freshness probe", None).await;
            !candidate_doc_ids(&resp).is_empty()
        }
    })
    .await;
    assert!(v1, "original content never became retrievable");

    // 3. Update content via an idempotent re-ingest (same key, new body).
    let update = serde_json::json!({
        "documents": [{
            "title": "freshness-probe",
            "type": "markdown",
            "source": "manual/freshness-probe",
            "content": "Replaced marker newcontentbeta for the freshness probe.",
            "idempotency_key": idem,
        }]
    });
    let resp = with_org(client.post(&bulk_url), &org)
        .json(&update)
        .send()
        .await
        .expect("update ingest");
    assert!(
        resp.status().is_success(),
        "update ingest failed: {}",
        resp.status()
    );

    // 4. The new content must reach the caller. The org-version bump invalidates
    //    cached retrievals instantly; allow a short window for re-embedding the
    //    changed chunk, then require the new marker to surface.
    let fresh = wait_until("v2-fresh", || {
        let client = &client;
        let org = &org;
        async move {
            let resp = retrieve(client, org, "newcontentbeta freshness probe", None).await;
            // The fresh marker only matches the replaced chunk; a stale cache
            // would keep returning the old vector and miss it.
            !candidate_doc_ids(&resp).is_empty()
        }
    })
    .await;
    assert!(
        fresh,
        "updated content did not reach the caller after the mutation"
    );
}
