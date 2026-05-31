//! Deep research executor scaffolding (QRY-13).
//!
//! Per the master ownership matrix:
//!   "Deep research → Post-V2 capture executor only.  Model Plane owns
//!    the research loop; Data Plane stores artifacts; Quarry executes
//!    the per-step capture."
//!
//! This module ships the **Quarry-side capture executor** and a typed
//! contract for the per-step `ResearchTask` that Model Plane sends. It
//! deliberately does NOT implement the research loop — that lives in
//! Model Plane and ranges over: planning, query refinement, tool choice,
//! synthesis, citation harmonization. Quarry's job is narrow:
//!
//! 1. Receive a `ResearchTask` (a single capture step in a larger plan)
//! 2. Execute it deterministically — search, fetch, extract, ingest
//! 3. Return a `ResearchTaskResult` with artifacts the Model loop can reason over
//! 4. Emit step receipts + events so the loop is auditable
//!
//! All state for the loop lives in Model Plane orchestration. Quarry is
//! stateless across tasks except for cross-cutting policy (lease pool,
//! cost tracking, ZDR enforcement).

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::zdr::ZdrMode;

use crate::serp::{SearchOptions, SearchProvider, SearchResult};

/// What capture step the research loop is asking Quarry to perform.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResearchTask {
    /// Issue a SERP query and return ranked URLs.
    Search {
        query: String,
        #[serde(default = "default_max_results")]
        max_results: u32,
        #[serde(default)]
        country: Option<String>,
        #[serde(default)]
        language: Option<String>,
    },
    /// Fetch a single URL and extract markdown + links.
    Fetch {
        url: String,
        /// When set, the per-task cost ceiling Quarry enforces post-hoc.
        #[serde(default)]
        max_cost_usd: Option<f64>,
    },
    /// Extract structured JSON from already-fetched markdown.
    Extract {
        markdown: String,
        schema: serde_json::Value,
    },
}

fn default_max_results() -> u32 {
    10
}

/// Wire shape for a research task envelope. The `task_id` lets the Model
/// Plane loop correlate sub-task results with planning state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchTaskEnvelope {
    pub task_id: String,
    pub research_id: String,
    pub org_id: String,
    pub task: ResearchTask,
    #[serde(default)]
    pub zdr: ZdrMode,
    /// How long the loop is willing to wait, in seconds. Quarry caps the
    /// active operation to this and surfaces `Timeout` if exceeded.
    #[serde(default)]
    pub deadline_s: Option<u32>,
}

/// Outcome of a single research capture step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchTaskResult {
    pub task_id: String,
    pub status: ResearchStatus,
    /// Captured artifacts — populated based on the task variant.
    #[serde(default)]
    pub artifacts: ResearchArtifacts,
    /// Aggregated cost spent on this step in micro-USD (so multi-step
    /// loops can sum without floating-point drift).
    pub cost_micro_usd: u64,
    /// Latency from envelope receipt to result emission.
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResearchStatus {
    Completed,
    Failed { error_code: String, message: String },
    Skipped { reason: String },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResearchArtifacts {
    #[serde(default)]
    pub search_results: Vec<SearchResult>,
    #[serde(default)]
    pub markdown: Option<String>,
    #[serde(default)]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub structured: Option<serde_json::Value>,
}

/// Quarry's capture executor for a Deep Research task.
///
/// Holds adapters that other parts of Quarry already provide (search
/// provider, AI format runner). The executor is an orchestrator that
/// picks the right tool per task variant. Production deployments inject
/// the same adapters they use for `/v1/search`, `/v1/scrape`, etc.
pub struct ResearchExecutor {
    pub search_provider: Option<Arc<dyn SearchProvider>>,
}

impl ResearchExecutor {
    pub fn new() -> Self {
        Self {
            search_provider: None,
        }
    }

    pub fn with_search_provider(mut self, provider: Arc<dyn SearchProvider>) -> Self {
        self.search_provider = Some(provider);
        self
    }

    /// Run a single research task. Stateless — the only side effect is
    /// emitting metrics and tracing.
    pub async fn execute(
        &self,
        envelope: &ResearchTaskEnvelope,
    ) -> QuarryResult<ResearchTaskResult> {
        let started = std::time::Instant::now();
        let task_id = envelope.task_id.clone();
        let span_id = format!("research_{}", ulid::Ulid::new());
        tracing::info!(
            research_id = %envelope.research_id,
            task_id = %task_id,
            span = %span_id,
            "research task started"
        );

        let result = match &envelope.task {
            ResearchTask::Search {
                query,
                max_results,
                country,
                language,
            } => {
                self.run_search(
                    query,
                    *max_results,
                    country.as_deref(),
                    language.as_deref(),
                    &envelope.org_id,
                )
                .await
            }
            ResearchTask::Fetch { url, max_cost_usd } => self.run_fetch(url, *max_cost_usd).await,
            ResearchTask::Extract { markdown, schema } => self.run_extract(markdown, schema).await,
        };

        let latency_ms = started.elapsed().as_millis() as u64;
        let outcome = match result {
            Ok(artifacts) => ResearchTaskResult {
                task_id,
                status: ResearchStatus::Completed,
                artifacts,
                cost_micro_usd: 0,
                latency_ms,
            },
            Err(e) => ResearchTaskResult {
                task_id,
                status: ResearchStatus::Failed {
                    error_code: format!("{:?}", e.code).to_uppercase(),
                    message: e.message,
                },
                artifacts: ResearchArtifacts::default(),
                cost_micro_usd: 0,
                latency_ms,
            },
        };
        Ok(outcome)
    }

    async fn run_search(
        &self,
        query: &str,
        max_results: u32,
        country: Option<&str>,
        language: Option<&str>,
        org_id: &str,
    ) -> QuarryResult<ResearchArtifacts> {
        let provider = self.search_provider.as_ref().ok_or_else(|| {
            QuarryError::new(
                ErrorCode::Unsupported,
                "no search provider configured for research executor",
            )
        })?;

        let opts = SearchOptions {
            limit: max_results.min(50),
            country: country.map(String::from),
            language: language.map(String::from),
            safe_search: true,
            topic: None,
            time_range: None,
            exact_match: false,
            // Tenant scoping: research tasks carry the org through the
            // envelope so any private-corpus provider (TantivyLocalIndex)
            // restricts results to this org's documents.
            org_id: Some(org_id.to_string()),
        };
        let results = provider.search(query, &opts).await?;
        Ok(ResearchArtifacts {
            search_results: results,
            ..Default::default()
        })
    }

    async fn run_fetch(
        &self,
        _url: &str,
        _max_cost_usd: Option<f64>,
    ) -> QuarryResult<ResearchArtifacts> {
        // Fetch is a thin wrapper around the existing scrape pipeline.
        // The scaffolding here returns Unsupported to make the boundary
        // explicit — production Deep Research wiring connects this to
        // PageRunner via the orchestrator (avoiding a circular dep
        // between quarry-runtime modules at construction time).
        Err(QuarryError::new(
            ErrorCode::Unsupported,
            "research fetch executor wires through PageRunner; \
             call PageRunner directly from the orchestrator until that \
             integration ships in Phase Q5",
        ))
    }

    async fn run_extract(
        &self,
        _markdown: &str,
        _schema: &serde_json::Value,
    ) -> QuarryResult<ResearchArtifacts> {
        Err(QuarryError::new(
            ErrorCode::Unsupported,
            "research extract executor delegates to AiFormatRunner.json; \
             pass an AiFormatRunner reference into the executor before invoking",
        ))
    }
}

impl Default for ResearchExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct FakeSearch;

    #[async_trait]
    impl SearchProvider for FakeSearch {
        async fn search(
            &self,
            _query: &str,
            _opts: &SearchOptions,
        ) -> QuarryResult<Vec<SearchResult>> {
            Ok(vec![SearchResult {
                url: "https://x".into(),
                title: Some("X".into()),
                snippet: None,
                rank: 1,
                provider: "fake".into(),
            }])
        }
        fn name(&self) -> &str {
            "fake"
        }
    }

    fn envelope(task: ResearchTask) -> ResearchTaskEnvelope {
        ResearchTaskEnvelope {
            task_id: "task_1".into(),
            research_id: "research_1".into(),
            org_id: "org_test".into(),
            task,
            zdr: ZdrMode::Off,
            deadline_s: Some(60),
        }
    }

    #[tokio::test]
    async fn search_task_with_provider_returns_results() {
        let exec = ResearchExecutor::new().with_search_provider(Arc::new(FakeSearch));
        let env = envelope(ResearchTask::Search {
            query: "rust".into(),
            max_results: 5,
            country: None,
            language: None,
        });
        let res = exec.execute(&env).await.unwrap();
        assert!(matches!(res.status, ResearchStatus::Completed));
        assert_eq!(res.artifacts.search_results.len(), 1);
        assert_eq!(res.task_id, "task_1");
    }

    #[tokio::test]
    async fn search_task_without_provider_fails_gracefully() {
        let exec = ResearchExecutor::new();
        let env = envelope(ResearchTask::Search {
            query: "rust".into(),
            max_results: 5,
            country: None,
            language: None,
        });
        let res = exec.execute(&env).await.unwrap();
        match res.status {
            ResearchStatus::Failed { error_code, .. } => {
                assert_eq!(error_code, "UNSUPPORTED");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn fetch_task_returns_unsupported_until_orchestrator_wiring_lands() {
        let exec = ResearchExecutor::new();
        let env = envelope(ResearchTask::Fetch {
            url: "https://x.com".into(),
            max_cost_usd: Some(0.01),
        });
        let res = exec.execute(&env).await.unwrap();
        match res.status {
            ResearchStatus::Failed { error_code, .. } => {
                assert_eq!(error_code, "UNSUPPORTED");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn extract_task_returns_unsupported_until_runner_wiring_lands() {
        let exec = ResearchExecutor::new();
        let env = envelope(ResearchTask::Extract {
            markdown: "# Hi".into(),
            schema: serde_json::json!({"type":"object"}),
        });
        let res = exec.execute(&env).await.unwrap();
        match res.status {
            ResearchStatus::Failed { error_code, .. } => {
                assert_eq!(error_code, "UNSUPPORTED");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn default_max_results_returns_ten() {
        assert_eq!(default_max_results(), 10);
    }

    #[test]
    fn task_envelope_serializes_with_kind_tag() {
        let env = envelope(ResearchTask::Search {
            query: "x".into(),
            max_results: 1,
            country: None,
            language: None,
        });
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("\"kind\":\"search\""));
    }
}
