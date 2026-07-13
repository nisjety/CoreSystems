use qdrant_client::qdrant::Value as QdrantValue;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ZdrMode {
    #[default]
    Disabled,
    Reject,
    Ephemeral,
}

impl ZdrMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Reject => "reject",
            Self::Ephemeral => "ephemeral",
        }
    }

    #[must_use]
    pub const fn restricts_egress(self) -> bool {
        !matches!(self, Self::Disabled)
    }

    #[must_use]
    pub const fn is_ephemeral(self) -> bool {
        matches!(self, Self::Ephemeral)
    }
}

impl FromStr for ZdrMode {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "disabled" => Ok(Self::Disabled),
            "reject" => Ok(Self::Reject),
            "ephemeral" => Ok(Self::Ephemeral),
            _ => Err("zdr_mode must be one of: disabled, reject, ephemeral"),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RetrievalRequest {
    pub org_id: String,
    pub query: String,
    pub top_k: Option<usize>,
    pub top_n: Option<usize>,
    pub filters: RetrievalFiltersInput,
    /// Viewer identity. NEVER trusted from the request body — it is set only by
    /// the auth layer from the verified principal (x-user-id / JWT `sub`).
    /// `skip_deserializing` makes a body-supplied `user_id` impossible (anti-spoof).
    #[serde(skip_deserializing)]
    pub user_id: Option<String>,
    /// Verified request proof used only for the Control grant lookup. It is
    /// boundary-injected and excluded from request/trace serialization.
    #[serde(skip)]
    pub verified_bearer: Option<String>,
    pub query_expansion: Option<String>,
    pub reranker_model: Option<String>,
    pub zdr_mode: Option<ZdrMode>,
    pub context_budget_tokens: Option<usize>,
    pub context_format: Option<String>,
    /// Optional per-request blend weights (D4+D5 spec §7).
    /// When unset, defaults from `Config::w_*` are used.
    /// Sum is renormalized to 1.0 before scoring.
    #[serde(default)]
    pub mode_mix: Option<ModeMixWeights>,
    /// §16.1.4 — optional `agent_id`. When set, the engine looks up
    /// `agent_retrieval_configs (org_id, agent_id)`. If a row exists, its
    /// `weights` JSONB overrides `mode_mix` and its `rerank` flag toggles
    /// reranking. Per-request `mode_mix` still wins if explicitly set;
    /// agent_id is the "what defaults should I use" knob.
    #[serde(default)]
    pub agent_id: Option<String>,
    /// Org-admin super-visibility (`org:data:read_all`). When true the ownership
    /// post-filter is bypassed org-wide (still org-scoped, never cross-org) and
    /// the bypass is audited. Set ONLY from a verified JWT scope on the HTTP path
    /// — the agent/api-key path leaves it false, so admin bypass is EXCLUDED from
    /// agent grounding by construction.
    #[serde(default, skip_deserializing)]
    pub admin_read_all: bool,
}

/// Per-query hybrid-retrieval blend weights. Recorded in `retrieval_runs.mode_mix`
/// after each query so the audit endpoint can show what blend was actually used.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModeMixWeights {
    #[serde(default)]
    pub w_dense: Option<f32>,
    #[serde(default)]
    pub w_bm25: Option<f32>,
    #[serde(default)]
    pub w_graph: Option<f32>,
    #[serde(default)]
    pub w_wiki: Option<f32>,
    /// Visual arm (Cohere Embed v4 page images). Defaults to 0 (shadow) until eval.
    #[serde(default)]
    pub w_visual: Option<f32>,
    /// `true` = run cross-encoder reranker after blending; `false` = skip.
    #[serde(default)]
    pub rerank: Option<bool>,
}

impl ModeMixWeights {
    /// Resolve unset weights from defaults and renormalize so they sum to 1.0.
    /// Returns the fully-specified weights that will be persisted with the trace.
    pub fn resolve(
        &self,
        default_dense: f32,
        default_bm25: f32,
        default_graph: f32,
        default_wiki: f32,
        default_visual: f32,
    ) -> ResolvedWeights {
        let d = self.w_dense.unwrap_or(default_dense);
        let b = self.w_bm25.unwrap_or(default_bm25);
        let g = self.w_graph.unwrap_or(default_graph);
        let w = self.w_wiki.unwrap_or(default_wiki);
        let v = self.w_visual.unwrap_or(default_visual);
        let sum = (d + b + g + w + v).max(f32::EPSILON);
        ResolvedWeights {
            w_dense: d / sum,
            w_bm25: b / sum,
            w_graph: g / sum,
            w_wiki: w / sum,
            w_visual: v / sum,
            rerank: self.rerank.unwrap_or(true),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ResolvedWeights {
    pub w_dense: f32,
    pub w_bm25: f32,
    pub w_graph: f32,
    pub w_wiki: f32,
    pub w_visual: f32,
    pub rerank: bool,
}

/// Which retrieval engines to run for a query, derived from the resolved blend
/// weights. Engines with ~zero weight are skipped so the pipeline never pays
/// for an embedding + Qdrant round-trip on a purely lexical query, nor a sparse
/// scan on a purely semantic one. This is "best-tool" routing over the existing
/// dense/sparse backends — not a parallel retrieval path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineRoute {
    pub dense: bool,
    pub sparse: bool,
}

impl EngineRoute {
    /// Weights at or below this share are treated as "off". Small enough that a
    /// genuine blend (e.g. 0.1 dense) still runs, large enough to drop a weight
    /// that renormalized to a rounding-error residue.
    const MIN_SHARE: f32 = 1e-4;

    pub fn from_weights(weights: &ResolvedWeights, hybrid_enabled: bool) -> Self {
        let mut dense = weights.w_dense > Self::MIN_SHARE;
        let sparse = hybrid_enabled && weights.w_bm25 > Self::MIN_SHARE;
        // Never route to nothing: graph/wiki blends are served by dedicated
        // endpoints, so if this path's two engines are both zeroed we fall back
        // to dense rather than returning an empty result.
        if !dense && !sparse {
            dense = true;
        }
        Self { dense, sparse }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RetrievalFiltersInput {
    #[serde(default)]
    pub document_types: Vec<String>,
    #[serde(default)]
    pub departments: Vec<String>,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub document_ids: Vec<String>,
    #[serde(default)]
    pub sources: Vec<String>,
    pub region: Option<String>,
    #[serde(default)]
    pub workspaces: Vec<String>,
    #[serde(default)]
    pub collections: Vec<String>,
    #[serde(default)]
    pub acl_tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScoredCandidate {
    pub knowledge_id: String,
    pub document_id: String,
    pub text: String,
    pub dense_score: f32,
    pub sparse_score: f32,
    pub rerank_score: f32,
    pub final_score: f32,
    pub chunk_index: i32,
    #[serde(skip)]
    #[allow(dead_code)] // raw payload kept for future reranker / debug inspection
    pub metadata: HashMap<String, QdrantValue>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceRef {
    pub document_id: String,
    pub title: String,
    pub source: String,
    pub r#type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RetrievalResponse {
    pub candidates: Vec<ScoredCandidate>,
    pub sources: Vec<SourceRef>,
    pub query: String,
    pub org_id: String,
    pub trace_id: String,
    pub index_version: String,
    pub zdr_mode: String,
    /// §16.1.3 — the ZDR enforcement actions actually applied to THIS retrieval
    /// (e.g. `reject_mode_filtered_restricted`, `reject_mode_no_restricted_found`,
    /// `ephemeral_no_trace_persist`). The same value is persisted on the trace;
    /// surfaced here so callers (and the e2e) can observe enforcement directly
    /// on the response without a follow-up trace fetch. Empty when ZDR is off.
    #[serde(default)]
    pub zdr_actions_applied: Vec<String>,
    pub low_confidence: bool,
    pub context_pack: Option<ContextPack>,
    /// Agent retrieval planner hints (D4+D5 spec §3). Lists which follow-up
    /// agent tools are likely to be productive given this query's outcome —
    /// e.g. low confidence → suggest `/v1/retrieve/graph`, `/v1/retrieve/wiki`;
    /// many sources → suggest `/v1/retrieve/contradictions`. Honest hints,
    /// not synthesized — empty array when nothing useful to recommend.
    #[serde(default)]
    pub suggested_next_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextPack {
    pub facts: Vec<ContextFact>,
    pub total_tokens: usize,
    pub budget_tokens: usize,
    pub format: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextFact {
    pub knowledge_id: String,
    pub document_id: String,
    pub text: String,
    pub score: f32,
    pub source_title: String,
    pub source_type: String,
    pub estimated_tokens: usize,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // surfaced via Debug + tracing; per-stage timings populated incrementally
pub struct PipelineTimings {
    pub embed_ms: u64,
    pub dense_ms: u64,
    pub sparse_ms: u64,
    pub fusion_ms: u64,
    pub rerank_ms: u64,
    pub source_join_ms: u64,
    pub total_ms: u64,
    pub candidate_count_dense: usize,
    pub candidate_count_sparse: usize,
    pub candidate_count_fused: usize,
    pub candidate_count_reranked: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zdr_mode_deserialization_is_exact_and_fail_closed() {
        #[derive(Deserialize)]
        struct Input {
            zdr_mode: ZdrMode,
        }

        let valid: Input = serde_json::from_str(r#"{"zdr_mode":"ephemeral"}"#)
            .expect("exact supported mode must parse");
        assert_eq!(valid.zdr_mode, ZdrMode::Ephemeral);
        assert!(serde_json::from_str::<Input>(r#"{"zdr_mode":"Ephemeral"}"#).is_err());
        assert!(serde_json::from_str::<Input>(r#"{"zdr_mode":"unknown"}"#).is_err());
    }

    #[test]
    fn every_restrictive_mode_suppresses_retaining_egress() {
        assert!(!ZdrMode::Disabled.restricts_egress());
        assert!(ZdrMode::Reject.restricts_egress());
        assert!(ZdrMode::Ephemeral.restricts_egress());
    }

    fn weights(w_dense: f32, w_bm25: f32) -> ResolvedWeights {
        ResolvedWeights {
            w_dense,
            w_bm25,
            w_graph: 0.0,
            w_wiki: 0.0,
            w_visual: 0.0,
            rerank: true,
        }
    }

    #[test]
    fn blended_weights_run_both_engines() {
        let route = EngineRoute::from_weights(&weights(0.7, 0.3), true);
        assert_eq!(
            route,
            EngineRoute {
                dense: true,
                sparse: true
            }
        );
    }

    #[test]
    fn zero_dense_routes_sparse_only() {
        let route = EngineRoute::from_weights(&weights(0.0, 1.0), true);
        assert_eq!(
            route,
            EngineRoute {
                dense: false,
                sparse: true
            }
        );
    }

    #[test]
    fn zero_sparse_routes_dense_only() {
        let route = EngineRoute::from_weights(&weights(1.0, 0.0), true);
        assert_eq!(
            route,
            EngineRoute {
                dense: true,
                sparse: false
            }
        );
    }

    #[test]
    fn hybrid_disabled_forces_dense_only_even_with_bm25_weight() {
        let route = EngineRoute::from_weights(&weights(0.5, 0.5), false);
        assert_eq!(
            route,
            EngineRoute {
                dense: true,
                sparse: false
            }
        );
    }

    #[test]
    fn both_zero_falls_back_to_dense() {
        // e.g. a graph/wiki-only blend reaching this path — never return empty.
        let route = EngineRoute::from_weights(&weights(0.0, 0.0), true);
        assert_eq!(
            route,
            EngineRoute {
                dense: true,
                sparse: false
            }
        );
    }
}
