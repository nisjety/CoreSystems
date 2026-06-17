//! Autoprompt — LLM query rewriting for the search router.
//!
//! Exa-inspired: turn a verbose natural-language question into a tighter,
//! keyword-dense web-search query before fan-out. Wired into
//! [`crate::smart_router::SmartSearchRouter`] for `Research`/`Comparative`
//! intents only — the verbose, multi-clause queries that benefit most.
//! Navigational/phrase/fresh queries are already well-formed and skip it.
//!
//! Degrade-safe: any Model Plane failure, an empty rewrite, or a suspiciously
//! long rewrite (the model returned prose, not a query) returns the original
//! query unchanged — the search path is never blocked by the rewriter.

use std::sync::Arc;

use async_trait::async_trait;

use crate::mp_client::{ModelPlaneClient, ModelPlaneInvokeRequest};

/// Rewrites a search query. Implementations MUST be degrade-safe: return the
/// original query verbatim on any failure rather than erroring.
#[async_trait]
pub trait QueryRewriter: Send + Sync {
    async fn rewrite(&self, query: &str) -> String;
}

/// Production rewriter backed by the Model Plane. Falls back to the original
/// query on any failure, empty reply, or implausibly long reply.
pub struct ModelPlaneQueryRewriter {
    client: Arc<ModelPlaneClient>,
    model: Option<String>,
}

impl ModelPlaneQueryRewriter {
    pub fn new(client: Arc<ModelPlaneClient>) -> Self {
        Self {
            client,
            model: None,
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }
}

/// Upper bound on a rewritten query (chars). A longer reply signals the model
/// returned prose rather than a query — reject it and keep the original.
const MAX_REWRITE_CHARS: usize = 400;

#[async_trait]
impl QueryRewriter for ModelPlaneQueryRewriter {
    async fn rewrite(&self, query: &str) -> String {
        let original = query.trim();
        if original.is_empty() {
            return original.to_string();
        }
        let req = ModelPlaneInvokeRequest {
            content: build_rewrite_prompt(original),
            model: self.model.clone(),
            session_key: None,
            thread_id: None,
        };
        match self.client.invoke(&req).await {
            Ok(resp) => sanitize_rewrite(&resp.content, original),
            Err(e) => {
                tracing::warn!(error = %e, "autoprompt: rewrite failed; using original query");
                original.to_string()
            }
        }
    }
}

fn build_rewrite_prompt(query: &str) -> String {
    format!(
        "You optimize web-search queries. Rewrite the user's question into a single, \
concise web-search query that maximizes recall of relevant results. Keep key \
entities and qualifiers; drop filler words and question phrasing; you may use \
search operators. Return ONLY the rewritten query on one line — no quotes, no \
explanation, no prefix.\n\nUser question: {query}\nRewritten query:"
    )
}

/// Clean the model's reply into a usable query: strip code fences, take the
/// first non-empty line, drop a leading "Rewritten query:" echo and wrapping
/// quotes. Falls back to `original` when the result is empty or implausibly
/// long (the model returned prose).
fn sanitize_rewrite(body: &str, original: &str) -> String {
    let mut s = body.trim();
    if let Some(rest) = s.strip_prefix("```") {
        s = rest
            .trim_start_matches("json")
            .trim()
            .trim_end_matches("```")
            .trim();
    }
    let line = s
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let line = line
        .strip_prefix("Rewritten query:")
        .unwrap_or(line)
        .trim()
        .trim_matches('"')
        .trim();
    if line.is_empty() || line.chars().count() > MAX_REWRITE_CHARS {
        return original.to_string();
    }
    line.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn sanitize_strips_quotes_and_prefix() {
        assert_eq!(
            sanitize_rewrite("Rewritten query: \"rust async runtime\"", "orig"),
            "rust async runtime"
        );
    }

    #[test]
    fn sanitize_strips_code_fence_and_takes_first_line() {
        assert_eq!(
            sanitize_rewrite("```\ntokio scheduler internals\n```", "orig"),
            "tokio scheduler internals"
        );
    }

    #[test]
    fn sanitize_falls_back_on_empty() {
        assert_eq!(sanitize_rewrite("   ", "the original"), "the original");
    }

    #[test]
    fn sanitize_falls_back_on_prose() {
        let prose = "x".repeat(MAX_REWRITE_CHARS + 1);
        assert_eq!(sanitize_rewrite(&prose, "keep me"), "keep me");
    }

    #[tokio::test]
    async fn rewrite_returns_model_output() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/invoke"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "request_id": "r1",
                "content": "rust vs go concurrency benchmark",
                "model_used": "test"
            })))
            .mount(&server)
            .await;
        let client = Arc::new(ModelPlaneClient::new(server.uri()).unwrap());
        let rw = ModelPlaneQueryRewriter::new(client);
        let out = rw
            .rewrite("which is faster for concurrency, rust or go?")
            .await;
        assert_eq!(out, "rust vs go concurrency benchmark");
    }

    #[tokio::test]
    async fn rewrite_degrades_to_original_on_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/invoke"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let client = Arc::new(ModelPlaneClient::new(server.uri()).unwrap());
        let rw = ModelPlaneQueryRewriter::new(client);
        let original = "compare rust and go";
        assert_eq!(rw.rewrite(original).await, original);
    }

    #[tokio::test]
    async fn rewrite_empty_query_is_noop() {
        // No server call needed — empty input short-circuits.
        let client = Arc::new(ModelPlaneClient::new("http://127.0.0.1:1").unwrap());
        let rw = ModelPlaneQueryRewriter::new(client);
        assert_eq!(rw.rewrite("   ").await, "");
    }
}
