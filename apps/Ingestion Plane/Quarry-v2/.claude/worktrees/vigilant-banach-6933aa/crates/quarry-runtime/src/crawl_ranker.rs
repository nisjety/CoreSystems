//! Model-driven crawl URL ranker.
//!
//! Closes QRY-17: when a crawl receives a discovery prompt (e.g. "find pages
//! about Q1 2026 financial results"), the Go orchestrator calls this ranker
//! to score discovered URLs against the prompt before deciding which to
//! enqueue. Ranking happens at the Rust hot path so the Go side just receives
//! a sorted slice with relevance scores.
//!
//! Failure mode: if the Model Plane is unreachable, the ranker degrades to a
//! lexical heuristic (token overlap between prompt and URL path/query) so
//! crawls keep working with a sensible default order.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use quarry_core::error::QuarryResult;

use crate::mp_client::ModelPlaneClient;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedUrl {
    pub url: String,
    pub score: f32,
    pub reason: Option<String>,
}

/// Trait so callers can swap a fake ranker into tests.
#[async_trait::async_trait]
pub trait CrawlRanker: Send + Sync {
    async fn rank(&self, prompt: &str, urls: &[String]) -> QuarryResult<Vec<RankedUrl>>;
}

/// Production ranker: calls the Model Plane gateway and falls back to lexical
/// scoring on failure.
pub struct ModelPlaneRanker {
    client: Arc<ModelPlaneClient>,
    model: Option<String>,
    fallback: LexicalRanker,
}

impl ModelPlaneRanker {
    pub fn new(client: Arc<ModelPlaneClient>) -> Self {
        Self {
            client,
            model: None,
            fallback: LexicalRanker,
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }
}

#[async_trait::async_trait]
impl CrawlRanker for ModelPlaneRanker {
    async fn rank(&self, prompt: &str, urls: &[String]) -> QuarryResult<Vec<RankedUrl>> {
        if urls.is_empty() {
            return Ok(vec![]);
        }

        let request_prompt = build_ranking_prompt(prompt, urls);

        let req = crate::mp_client::ModelPlaneInvokeRequest {
            content: request_prompt,
            model: self.model.clone(),
            session_key: None,
            thread_id: None,
        };
        let result = self.client.invoke(&req).await;

        let body = match result {
            Ok(resp) => resp.content,
            Err(e) => {
                tracing::warn!(error = %e, "ranker fell back to lexical scoring");
                return self.fallback.rank(prompt, urls).await;
            }
        };

        let stripped = strip_code_fences(&body);
        match serde_json::from_str::<Vec<RankedUrl>>(&stripped) {
            Ok(mut ranked) => {
                ranked.sort_by(|a, b| {
                    b.score
                        .partial_cmp(&a.score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                Ok(ranked)
            }
            Err(e) => {
                tracing::warn!(error = %e, "ranker JSON parse failed; falling back");
                self.fallback.rank(prompt, urls).await
            }
        }
    }
}

/// Lexical fallback ranker: scores each URL by token overlap with the prompt
/// (case-insensitive). Stable, deterministic, no network calls.
pub struct LexicalRanker;

#[async_trait::async_trait]
impl CrawlRanker for LexicalRanker {
    async fn rank(&self, prompt: &str, urls: &[String]) -> QuarryResult<Vec<RankedUrl>> {
        let prompt_tokens: Vec<String> = tokenize(prompt);
        let mut ranked: Vec<RankedUrl> = urls
            .iter()
            .map(|url| {
                let url_tokens = tokenize(url);
                let overlap = prompt_tokens
                    .iter()
                    .filter(|t| url_tokens.iter().any(|u| u == *t))
                    .count();
                let score = if prompt_tokens.is_empty() {
                    0.0
                } else {
                    overlap as f32 / prompt_tokens.len() as f32
                };
                RankedUrl {
                    url: url.clone(),
                    score,
                    reason: Some(format!("lexical overlap: {overlap} tokens")),
                }
            })
            .collect();
        ranked.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(ranked)
    }
}

fn tokenize(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty() && t.len() > 2)
        .map(|t| t.to_string())
        .collect()
}

fn build_ranking_prompt(user_prompt: &str, urls: &[String]) -> String {
    let mut sb = String::with_capacity(512 + urls.iter().map(|u| u.len() + 4).sum::<usize>());
    sb.push_str("You are a URL relevance ranker. The user is crawling for: ");
    sb.push_str(user_prompt);
    sb.push_str("\n\nScore each URL below from 0.0 (irrelevant) to 1.0 (highly relevant) ");
    sb.push_str("based on how likely the page is to contain content matching the prompt.\n\n");
    sb.push_str("URLs:\n");
    for u in urls {
        sb.push_str("- ");
        sb.push_str(u);
        sb.push('\n');
    }
    sb.push_str("\nReturn ONLY a JSON array of objects with `url`, `score`, and `reason`. No prose.\n");
    sb.push_str("Example: [{\"url\":\"...\",\"score\":0.85,\"reason\":\"path mentions topic\"}]");
    sb
}

fn strip_code_fences(body: &str) -> String {
    let trimmed = body.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        return rest.trim().trim_end_matches("```").trim().to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        return rest.trim().trim_end_matches("```").trim().to_string();
    }
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lexical_ranker_orders_by_overlap() {
        let r = LexicalRanker;
        let urls = vec![
            "https://example.com/blog/q1-2026-financial-results".to_string(),
            "https://example.com/about".to_string(),
            "https://example.com/financial".to_string(),
        ];
        let ranked = r.rank("find Q1 2026 financial results", &urls).await.unwrap();
        // First should be the deepest match
        assert!(ranked[0].url.contains("q1-2026-financial-results"));
        // Last should be /about (no overlap)
        assert!(ranked.last().unwrap().url.ends_with("/about"));
    }

    #[tokio::test]
    async fn lexical_ranker_handles_empty_input() {
        let r = LexicalRanker;
        let ranked = r.rank("anything", &[]).await.unwrap();
        assert!(ranked.is_empty());
    }

    #[tokio::test]
    async fn lexical_ranker_handles_empty_prompt() {
        let r = LexicalRanker;
        let urls = vec!["https://x".to_string()];
        let ranked = r.rank("", &urls).await.unwrap();
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].score, 0.0);
    }

    #[test]
    fn build_prompt_includes_each_url() {
        let prompt = build_ranking_prompt(
            "find docs",
            &["https://a".into(), "https://b".into(), "https://c".into()],
        );
        assert!(prompt.contains("https://a"));
        assert!(prompt.contains("https://b"));
        assert!(prompt.contains("https://c"));
        assert!(prompt.contains("find docs"));
    }

    #[test]
    fn strip_code_fences_handles_json_fence() {
        let body = "```json\n[{\"url\":\"x\",\"score\":1.0}]\n```";
        let stripped = strip_code_fences(body);
        assert_eq!(stripped, "[{\"url\":\"x\",\"score\":1.0}]");
    }

    #[test]
    fn strip_code_fences_handles_plain_fence() {
        let body = "```\nhello\n```";
        assert_eq!(strip_code_fences(body), "hello");
    }

    #[test]
    fn strip_code_fences_passes_through_no_fence() {
        assert_eq!(strip_code_fences("[1,2,3]"), "[1,2,3]");
    }

    #[test]
    fn tokenize_strips_short_tokens_and_lowercase() {
        let tokens = tokenize("Q1 2026 Financial-Results.html");
        assert!(tokens.contains(&"2026".to_string()));
        assert!(tokens.contains(&"financial".to_string()));
        assert!(tokens.contains(&"results".to_string()));
        assert!(!tokens.contains(&"q1".to_string())); // length 2, filtered
    }
}
