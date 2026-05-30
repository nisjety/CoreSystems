//! Vector retrieval abstraction for hybrid search (OSS-parity P0 1C, Path A).
//!
//! Quarry is the evidence engine — it does NOT own a vector store. Semantic
//! retrieval is delegated to the **Data Plane v2** `retrieval_v2` service
//! (which owns embed-on-ingest + Qdrant). `DataPlaneVectorIndex` calls the
//! Data Plane's `/v1/retrieve` endpoint; results are RRF-fused with Quarry's
//! local lexical (`TantivyLocalIndex`) recall in `hybrid::HybridSearchProvider`.
//!
//! The `VectorIndex` trait keeps the door open for a future Quarry-local
//! `QdrantVectorIndex` (Path B) without touching callers.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

/// A single semantic-retrieval hit, normalized for RRF fusion with lexical
/// `SearchResult`s.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorHit {
    /// Canonical identifier used for fusion dedup. Prefers a source URL, falls
    /// back to the Data Plane `document_id`.
    pub url: String,
    pub score: f32,
    pub title: Option<String>,
    pub snippet: Option<String>,
    pub document_id: Option<String>,
}

/// Semantic retrieval backend. Org-scoped: implementations MUST restrict hits
/// to the given `org_id`.
#[async_trait]
pub trait VectorIndex: Send + Sync {
    async fn retrieve(
        &self,
        org_id: &str,
        query: &str,
        top_k: usize,
    ) -> QuarryResult<Vec<VectorHit>>;
}

// ── Data Plane delegation (Path A) ──────────────────────────────────────────

/// Calls the Data Plane v2 `retrieval_v2` service over HTTP/JSON. The Data
/// Plane owns embeddings + Qdrant; Quarry just forwards the query text.
pub struct DataPlaneVectorIndex {
    client: reqwest::Client,
    retrieve_url: String,
    api_key: Option<String>,
}

impl DataPlaneVectorIndex {
    /// `base_url` is the Data Plane root (e.g. `http://dpv2-documents-api:8080`);
    /// the retrieve path `/v1/retrieve` is appended.
    pub fn new(base_url: impl AsRef<str>) -> Self {
        let base = base_url.as_ref().trim_end_matches('/');
        Self {
            client: reqwest::Client::new(),
            retrieve_url: format!("{base}/v1/retrieve"),
            api_key: None,
        }
    }

    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    #[cfg(test)]
    pub fn with_retrieve_url(mut self, url: impl Into<String>) -> Self {
        self.retrieve_url = url.into();
        self
    }
}

#[derive(Debug, Serialize)]
struct RetrieveRequestBody<'a> {
    org_id: &'a str,
    query: &'a str,
    top_k: i32,
    /// Retrieval is read-only; never persists. ZDR is therefore irrelevant,
    /// but we pass "off" explicitly so the Data Plane doesn't gate the call.
    zdr_mode: &'a str,
}

// Tolerant mirror of the Data Plane `RetrieveResponse` (retrieval_v2.proto):
// candidates carry score + text + document_id; sources carry titles.
#[derive(Debug, Deserialize, Default)]
struct RetrieveResponseBody {
    #[serde(default)]
    candidates: Vec<Candidate>,
    #[serde(default)]
    sources: Vec<Source>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    #[serde(default)]
    document_id: Option<String>,
    #[serde(default, alias = "content", alias = "chunk")]
    text: Option<String>,
    #[serde(default)]
    score: f32,
    #[serde(default, alias = "source_url", alias = "uri")]
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Source {
    #[serde(default)]
    document_id: Option<String>,
    #[serde(default)]
    title: Option<String>,
}

#[async_trait]
impl VectorIndex for DataPlaneVectorIndex {
    async fn retrieve(
        &self,
        org_id: &str,
        query: &str,
        top_k: usize,
    ) -> QuarryResult<Vec<VectorHit>> {
        let body = RetrieveRequestBody {
            org_id,
            query,
            top_k: top_k.min(i32::MAX as usize) as i32,
            zdr_mode: "off",
        };
        let mut req = self.client.post(&self.retrieve_url).json(&body);
        if let Some(key) = &self.api_key {
            req = req.header("x-internal-key", key);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| QuarryError::new(ErrorCode::UpstreamBlocked, format!("retrieve request failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(QuarryError::new(
                ErrorCode::UpstreamBlocked,
                format!("retrieve returned status {}", resp.status()),
            ));
        }
        let parsed: RetrieveResponseBody = resp
            .json()
            .await
            .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("retrieve decode failed: {e}")))?;
        Ok(map_candidates(parsed))
    }
}

/// Map a Data Plane response into `VectorHit`s: title comes from the matching
/// `Source` (by document_id), url prefers an explicit URL then document_id.
fn map_candidates(body: RetrieveResponseBody) -> Vec<VectorHit> {
    use std::collections::HashMap;
    let titles: HashMap<String, String> = body
        .sources
        .into_iter()
        .filter_map(|s| {
            let id = s.document_id.clone()?;
            s.title.map(|t| (id, t))
        })
        .collect();
    body.candidates
        .into_iter()
        .filter_map(|c| {
            let url = c
                .url
                .clone()
                .or_else(|| c.document_id.clone())
                .filter(|u| !u.is_empty())?;
            let title = c
                .document_id
                .as_ref()
                .and_then(|id| titles.get(id).cloned());
            Some(VectorHit {
                url,
                score: c.score,
                title,
                snippet: c.text,
                document_id: c.document_id,
            })
        })
        .collect()
}

/// No-op backend for tests / deployments without a Data Plane.
pub struct NoopVectorIndex;

#[async_trait]
impl VectorIndex for NoopVectorIndex {
    async fn retrieve(&self, _org: &str, _q: &str, _k: usize) -> QuarryResult<Vec<VectorHit>> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn maps_candidates_with_source_titles() {
        let body = RetrieveResponseBody {
            candidates: vec![Candidate {
                document_id: Some("doc1".into()),
                text: Some("alpha beta".into()),
                score: 0.9,
                url: Some("https://a.com/1".into()),
            }],
            sources: vec![Source {
                document_id: Some("doc1".into()),
                title: Some("Title One".into()),
            }],
        };
        let hits = map_candidates(body);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].url, "https://a.com/1");
        assert_eq!(hits[0].title.as_deref(), Some("Title One"));
        assert_eq!(hits[0].snippet.as_deref(), Some("alpha beta"));
    }

    #[test]
    fn falls_back_to_document_id_when_no_url() {
        let body = RetrieveResponseBody {
            candidates: vec![Candidate {
                document_id: Some("doc2".into()),
                text: None,
                score: 0.5,
                url: None,
            }],
            sources: vec![],
        };
        let hits = map_candidates(body);
        assert_eq!(hits[0].url, "doc2");
        assert!(hits[0].title.is_none());
    }

    #[tokio::test]
    async fn retrieve_calls_dataplane_and_maps() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/retrieve"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "candidates": [
                    {"document_id": "d1", "text": "hit one", "score": 0.8, "source_url": "https://x.com/1"}
                ],
                "sources": [{"document_id": "d1", "title": "X One"}]
            })))
            .mount(&server)
            .await;
        let idx = DataPlaneVectorIndex::new("http://unused")
            .with_retrieve_url(format!("{}/v1/retrieve", server.uri()));
        let hits = idx.retrieve("org_a", "query", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].url, "https://x.com/1");
        assert_eq!(hits[0].title.as_deref(), Some("X One"));
    }

    #[tokio::test]
    async fn retrieve_maps_non_2xx_to_typed_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/retrieve"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let idx = DataPlaneVectorIndex::new("http://unused")
            .with_retrieve_url(format!("{}/v1/retrieve", server.uri()));
        let err = idx.retrieve("org_a", "q", 5).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::UpstreamBlocked);
    }
}
