use std::collections::HashMap;

use reqwest::Client;
use serde::Deserialize;
use serde_json::json;

use crate::pipeline::types::ScoredCandidate;

/// Query-side Meilisearch client for the keyword retrieval arm — typo-tolerant,
/// exact-ID/code-friendly lookup over the same `knowledge_units` corpus the
/// dense/sparse/graph arms already search. See `meilisearch-adapter-rs` for
/// the write side that keeps the index converged with that corpus.
///
/// Deliberately a SEPARATE HTTP client from `meilisearch-adapter-rs`'s own
/// `MeilisearchClient` (ingest), never a shared crate — the same split
/// `quickwit-adapter-rs::quickwit::QuickwitClient` (ingest) and this crate's
/// own `search/sparse.rs` (query) already use for the Quickwit arm. A query
/// must fail fast so a live retrieval request never stalls on a stuck HTTP
/// call; an ingest failure can simply retry via NATS redelivery.
#[derive(Clone)]
pub struct MeilisearchQueryClient {
    http: Client,
    base_url: String,
    index_uid: String,
    api_key: String,
}

impl MeilisearchQueryClient {
    /// `None` when either `url` or `api_key` is empty — fail CLOSED, not
    /// open: sending an unauthenticated request to a master-keyed
    /// Meilisearch would just 401 per call, but treating "not configured" as
    /// "arm disabled" (mirroring `VisualQueryEmbedder::from_config`'s
    /// `Ok(None)` path) keeps that failure mode a clean, logged no-op instead
    /// of a per-query warning flood.
    pub fn from_config(url: &str, api_key: &str, index_uid: &str) -> Option<Self> {
        if url.trim().is_empty() || api_key.trim().is_empty() {
            return None;
        }
        Self::new(url, index_uid, api_key).ok()
    }

    pub fn new(
        base_url: impl Into<String>,
        index_uid: impl Into<String>,
        api_key: impl Into<String>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_millis(1500))
                .build()?,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            index_uid: index_uid.into(),
            api_key: api_key.into(),
        })
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    hits: Vec<SearchHit>,
}

#[derive(Debug, Deserialize)]
struct SearchHit {
    knowledge_id: String,
    document_id: String,
    #[serde(default)]
    body: String,
}

/// Keyword retrieval ARM for RRF fusion. Returns chunk candidates from
/// Meilisearch's typo-tolerant match, ordered best-first (Meilisearch's own
/// relevance order). RRF consumes the order (not raw scores, which
/// Meilisearch does not return by default), so scores are left at 0 and set
/// during fusion — identical contract to `search::graph::graph_arm_candidates`.
///
/// Org-scoped via a Meilisearch **filter expression** (never free text) built
/// from [`quote_filter_value`], so an org_id containing a `"` or `\` can
/// never widen the match to another tenant. This is the arm's entire
/// isolation boundary: unlike the SQL arms, Meilisearch has no per-request
/// transaction-scoped GUC, so the filter clause is load-bearing on every
/// single call — there is no defense-in-depth layer under it inside this
/// client. (There IS a layer above it: the pipeline's step-6 canonical
/// ownership gate re-filters every candidate this arm returns, exactly as it
/// does for dense/sparse/graph/wiki, using the real `knowledge_id`/
/// `document_id` this arm carries — see `pipeline/orchestrator.rs`.)
///
/// Non-fatal by contract: the orchestrator logs + skips on error, matching
/// the wiki/visual arms rather than the FATAL dense/sparse arms.
pub async fn keyword_arm_candidates(
    client: &MeilisearchQueryClient,
    query: &str,
    org_id: &str,
    limit: i64,
) -> anyhow::Result<Vec<ScoredCandidate>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    // Anchor on distinctive terms when the query has any — same defect as the
    // sparse arms, independently confirmed on this arm rather than assumed from
    // the fix there. Meilisearch's default ranking rules (`words, typo,
    // proximity, attribute, sort, exactness`) do not weight a rare exact token
    // over several ordinary ones, so wrapping an identifier in prose buries it
    // the same way an OR-joined disjunction did on the other two backends.
    //
    // Measured directly against this arm (bypassing fusion), on the mined
    // lexical golden set — a query set with verified single-document ground
    // truth, not an assumed one:
    //
    // | query shape | found in top 10 | MRR |
    // |---|---|---|
    // | bare identifier | 23/23 | 0.9783 |
    // | wrapped in prose, unchanged | **14/23** | 0.5181 |
    // | wrapped in prose, anchor-only | 23/23 | 0.9783 |
    //
    // Nine of twenty-three identifiers were unfindable in the arm's own top 10
    // once wrapped in ordinary words — worse than the sparse arms' degradation,
    // because Meilisearch has no "required" operator to fall back on the way
    // Quickwit's `+()` does; dropping the prose entirely is what works here.
    //
    // No behaviour change for a pure prose question (no anchors): the full
    // query still reaches Meilisearch, which is the typo-tolerant, fuzzy-match
    // case this arm also exists to serve and that the measurement above never
    // touched.
    let anchored = crate::search::textquery::distinctive_terms(query);
    let effective_query = if anchored.is_empty() {
        query.to_string()
    } else {
        anchored.join(" ")
    };
    let filter = format!("org_id = {}", quote_filter_value(org_id));
    let url = format!("{}/indexes/{}/search", client.base_url, client.index_uid);
    let response = client
        .http
        .post(&url)
        .bearer_auth(&client.api_key)
        .json(&json!({
            "q": effective_query,
            "filter": filter,
            "limit": limit,
            "attributesToRetrieve": ["knowledge_id", "document_id", "body"],
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        anyhow::bail!(
            "Meilisearch search failed: {} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        );
    }

    let parsed: SearchResponse = response.json().await?;
    Ok(parsed
        .hits
        .into_iter()
        .map(|hit| ScoredCandidate {
            knowledge_id: hit.knowledge_id,
            document_id: hit.document_id,
            text: hit.body,
            dense_score: 0.0,
            sparse_score: 0.0,
            rerank_score: 0.0,
            final_score: 0.0,
            chunk_index: 0,
            metadata: HashMap::new(),
        })
        .collect())
}

/// Escapes a value for embedding inside a Meilisearch filter expression: a
/// double-quoted string literal with `\` and `"` escaped. This crate's own
/// copy of the same defense `meilisearch-adapter-rs::meilisearch::quote_filter_value`
/// applies on the write side — independent HTTP clients, independent
/// (identical) escaping, same as `quickwit-adapter-rs`'s and this crate's own
/// separate `quote_query_value`-style helpers for the Quickwit arm.
pub fn quote_filter_value(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_config_is_none_when_url_or_key_is_empty() {
        assert!(MeilisearchQueryClient::from_config("", "key", "idx").is_none());
        assert!(
            MeilisearchQueryClient::from_config("http://meilisearch:7700", "", "idx").is_none()
        );
        assert!(MeilisearchQueryClient::from_config("   ", "key", "idx").is_none());
    }

    #[test]
    fn from_config_is_some_when_both_url_and_key_are_present() {
        assert!(
            MeilisearchQueryClient::from_config("http://meilisearch:7700", "key", "idx").is_some()
        );
    }

    #[test]
    fn org_filter_escapes_quotes_and_backslashes() {
        assert_eq!(quote_filter_value("org-1"), "\"org-1\"");
        assert_eq!(quote_filter_value(r#"org"1\evil"#), r#""org\"1\\evil""#);
    }

    #[test]
    fn org_filter_cannot_widen_past_its_own_org() {
        // A naive, unescaped filter would let a hostile org_id close its own
        // quote early and append a second clause (e.g. `OR org_id = "other"`).
        // Every quote embedded in the input must come out escaped, leaving no
        // bare quote able to terminate the filter clause early.
        let hostile = "org-a\" OR org_id=\"org-b";
        let quoted = quote_filter_value(hostile);
        assert_eq!(hostile.matches('"').count(), quoted.matches("\\\"").count());
        assert!(quoted.starts_with('"') && quoted.ends_with('"'));
    }

    #[tokio::test]
    async fn empty_query_short_circuits_without_a_network_call() {
        // Points at a port nothing listens on: if `keyword_arm_candidates`
        // tried to make the request, this would return an `Err` (connection
        // refused), not `Ok(vec![])`. Getting `Ok(vec![])` back proves the
        // empty-query guard returned before any I/O.
        let client =
            MeilisearchQueryClient::new("http://127.0.0.1:1", "idx", "key").expect("client");
        let result = keyword_arm_candidates(&client, "   ", "org-a", 10).await;
        // `ScoredCandidate` has no `PartialEq` (its metadata map is opaque),
        // so the empty-result contract is checked via `is_empty()`.
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn hits_are_shape_adapted_into_scored_candidates_in_meilisearch_order() {
        use axum::{extract::Json as JsonExtract, routing::post, Router};

        async fn search(
            JsonExtract(request): JsonExtract<serde_json::Value>,
        ) -> axum::Json<serde_json::Value> {
            // The org filter must be present on every request — this is the
            // arm's entire isolation boundary (see the module docs).
            assert_eq!(request["filter"], "org_id = \"org-a\"");
            // "SKU-1" is itself a distinctive term (digit + letter), so
            // anchoring is a no-op here and the query must reach Meilisearch
            // unchanged.
            assert_eq!(request["q"], "SKU-1");
            axum::Json(serde_json::json!({
                "hits": [
                    { "knowledge_id": "kid-1", "document_id": "doc-1", "body": "SKU-1 shipped" },
                    { "knowledge_id": "kid-2", "document_id": "doc-2", "body": "SKU-2 shipped" },
                ]
            }))
        }

        let app = Router::new().route("/indexes/{index}/search", post(search));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("mock server");
        });

        let client = MeilisearchQueryClient::new(format!("http://{address}"), "idx", "test-key")
            .expect("client");
        let candidates = keyword_arm_candidates(&client, "SKU-1", "org-a", 10)
            .await
            .expect("keyword_arm_candidates");

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].knowledge_id, "kid-1");
        assert_eq!(candidates[0].document_id, "doc-1");
        assert_eq!(candidates[0].text, "SKU-1 shipped");
        // RRF consumes list order, not a score field — every candidate's raw
        // score stays 0.0 here, exactly like `graph_arm_candidates`.
        assert_eq!(candidates[0].final_score, 0.0);
    }

    /// The regression this exists for. Measured directly against a live
    /// Meilisearch index with verified single-document ground truth (the mined
    /// lexical golden set): wrapping an identifier in ordinary prose dropped it
    /// out of the arm's own top 10 for 9 of 23 queries (MRR 0.9783 -> 0.5181).
    /// Sending Meilisearch just the anchor term restored 23/23 (MRR 0.9783).
    #[tokio::test]
    async fn prose_around_an_identifier_is_dropped_before_it_reaches_meilisearch() {
        use axum::{extract::Json as JsonExtract, routing::post, Router};
        use std::sync::{Arc, Mutex};

        let seen = Arc::new(Mutex::new(None));
        let seen_in_handler = seen.clone();

        let app = Router::new().route(
            "/indexes/{index}/search",
            post(
                move |JsonExtract(request): JsonExtract<serde_json::Value>| {
                    let seen = seen_in_handler.clone();
                    async move {
                        *seen.lock().expect("lock") = Some(request["q"].clone());
                        axum::Json(serde_json::json!({ "hits": [] }))
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("mock server");
        });

        let client = MeilisearchQueryClient::new(format!("http://{address}"), "idx", "test-key")
            .expect("client");
        keyword_arm_candidates(
            &client,
            "where is idx_ku_content_tsv_gin defined and how is it used",
            "org-a",
            10,
        )
        .await
        .expect("keyword_arm_candidates");

        assert_eq!(
            seen.lock().expect("lock").take(),
            Some(serde_json::Value::String("idx_ku_content_tsv_gin".into())),
            "only the identifier should reach Meilisearch — the surrounding \
             prose is what buried it"
        );
    }

    /// A pure prose question has no anchor, so the arm must fall back to the
    /// full query — this is the typo-tolerant, fuzzy-match case the arm also
    /// exists to serve, and the anchoring change must not touch it.
    #[tokio::test]
    async fn pure_prose_reaches_meilisearch_unchanged() {
        use axum::{extract::Json as JsonExtract, routing::post, Router};
        use std::sync::{Arc, Mutex};

        let seen = Arc::new(Mutex::new(None));
        let seen_in_handler = seen.clone();

        let app = Router::new().route(
            "/indexes/{index}/search",
            post(
                move |JsonExtract(request): JsonExtract<serde_json::Value>| {
                    let seen = seen_in_handler.clone();
                    async move {
                        *seen.lock().expect("lock") = Some(request["q"].clone());
                        axum::Json(serde_json::json!({ "hits": [] }))
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("mock server");
        });

        let client = MeilisearchQueryClient::new(format!("http://{address}"), "idx", "test-key")
            .expect("client");
        keyword_arm_candidates(
            &client,
            "how does retrieval combine dense and sparse",
            "org-a",
            10,
        )
        .await
        .expect("keyword_arm_candidates");

        assert_eq!(
            seen.lock().expect("lock").take(),
            Some(serde_json::Value::String(
                "how does retrieval combine dense and sparse".into()
            ))
        );
    }
}
