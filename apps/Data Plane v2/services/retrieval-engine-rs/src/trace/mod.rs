use sqlx::PgPool;
use uuid::Uuid;

use crate::pipeline::types::*;

#[tracing::instrument(
    name = "postgres.persist_trace",
    skip_all,
    fields(
        otel.kind = "client",
        db.system = "postgresql",
        db.operation = "INSERT",
        org_id = req.org_id.as_str(),
        candidate_count = candidates.len(),
        zdr_mode = zdr_mode,
    ),
)]
pub async fn persist_trace(
    pool: &PgPool,
    req: &RetrievalRequest,
    candidates: &[ScoredCandidate],
    timings: &PipelineTimings,
    reranker_name: Option<&str>,
    zdr_mode: &str,
    mode_mix: Option<&ResolvedWeights>,
    // §16.1.3 — actions actually executed on this request (e.g.
    // ["filter_restricted","strip_pii"]). Empty slice → SQL NULL, so
    // a plain `disabled` mode that did nothing stays distinguishable
    // from "we forgot to record anything".
    zdr_actions_applied: &[&str],
) -> anyhow::Result<String> {
    let trace_id = Uuid::new_v4().to_string();

    // Serialize mode_mix for the JSONB column. None → SQL NULL.
    let mode_mix_json = mode_mix.map(|w| {
        serde_json::json!({
            "w_dense": w.w_dense,
            "w_bm25":  w.w_bm25,
            "w_graph": w.w_graph,
            "w_wiki":  w.w_wiki,
            "rerank":  w.rerank,
        })
    });

    let zdr_actions_json = if zdr_actions_applied.is_empty() {
        None
    } else {
        Some(serde_json::Value::Array(
            zdr_actions_applied
                .iter()
                .map(|s| serde_json::Value::String((*s).to_string()))
                .collect(),
        ))
    };

    // §16.1.1 — mode_mix vs mode_mix_applied. The current scorer is RRF
    // over (dense, bm25) only; graph + wiki weights are recorded but
    // ignored until the 4-way fold-in lands in v2.5. So we surface
    // honesty: requested → mode_mix; actually-used → mode_mix_applied
    // with w_graph/w_wiki zeroed out.
    let mode_mix_applied_json = mode_mix.map(|w| {
        serde_json::json!({
            "w_dense": w.w_dense,
            "w_bm25":  w.w_bm25,
            "w_graph": 0.0_f32,
            "w_wiki":  0.0_f32,
            "rerank":  w.rerank,
            "note":    "graph+wiki weights ignored by current RRF scorer; see §16.1.1",
        })
    });

    sqlx::query(
        r#"
        INSERT INTO retrieval_runs (
            trace_id, org_id, query, query_embedding_model, index_version,
            filters_json, reranker_name, zdr_mode, top_k,
            dense_retrieval_ms, sparse_retrieval_ms, rerank_ms, total_ms,
            candidate_count_dense, candidate_count_sparse,
            candidate_count_fused, candidate_count_reranked, mode_mix,
            zdr_actions_applied, mode_mix_applied
        ) VALUES (
            $1, $2, $3, $4, $5,
            $6, $7, $8, $9,
            $10, $11, $12, $13,
            $14, $15, $16, $17, $18,
            $19, $20
        )
        "#,
    )
    .bind(&trace_id)
    .bind(&req.org_id)
    .bind(&req.query)
    .bind("text-embedding-3-large")
    .bind("v2-current")
    .bind(serde_json::to_value(&req.filters).unwrap_or_default())
    .bind(reranker_name)
    .bind(zdr_mode)
    .bind(req.top_k.map(|k| k as i32))
    .bind(timings.dense_ms as i32)
    .bind(timings.sparse_ms as i32)
    .bind(timings.rerank_ms as i32)
    .bind(timings.total_ms as i32)
    .bind(timings.candidate_count_dense as i32)
    .bind(timings.candidate_count_sparse as i32)
    .bind(timings.candidate_count_fused as i32)
    .bind(timings.candidate_count_reranked as i32)
    .bind(mode_mix_json)
    .bind(zdr_actions_json)
    .bind(mode_mix_applied_json)
    .execute(pool)
    .await?;

    // Persist individual candidates
    for (rank, candidate) in candidates.iter().enumerate() {
        sqlx::query(
            r#"
            INSERT INTO retrieval_candidates (
                trace_id, rank, knowledge_id, document_id,
                dense_score, sparse_score, rerank_score, final_score,
                source_chunk_ref
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(&trace_id)
        .bind(rank as i32 + 1)
        .bind(&candidate.knowledge_id)
        .bind(&candidate.document_id)
        .bind(candidate.dense_score)
        .bind(candidate.sparse_score)
        .bind(candidate.rerank_score)
        .bind(candidate.final_score)
        .bind(&candidate.knowledge_id)
        .execute(pool)
        .await?;
    }

    tracing::debug!(trace_id = %trace_id, candidates = candidates.len(), "trace persisted");
    Ok(trace_id)
}

pub async fn get_trace(
    pool: &PgPool,
    trace_id: &str,
    org_id: &str,
) -> anyhow::Result<Option<TraceDetail>> {
    let run = sqlx::query_as::<_, TraceRun>(
        r#"
        SELECT trace_id, org_id, query, query_embedding_model, index_version,
               reranker_name, zdr_mode, dense_retrieval_ms, sparse_retrieval_ms,
               rerank_ms, total_ms, candidate_count_dense, candidate_count_sparse,
               candidate_count_fused, candidate_count_reranked, created_at,
               mode_mix
        FROM retrieval_runs
        WHERE trace_id = $1 AND org_id = $2
        "#,
    )
    .bind(trace_id)
    .bind(org_id)
    .fetch_optional(pool)
    .await?;

    let Some(run) = run else {
        return Ok(None);
    };

    let candidates = sqlx::query_as::<_, TraceCandidateRow>(
        r#"
        SELECT rank, knowledge_id, document_id,
               dense_score, sparse_score, rerank_score, final_score
        FROM retrieval_candidates
        WHERE trace_id = $1
        ORDER BY rank
        "#,
    )
    .bind(trace_id)
    .fetch_all(pool)
    .await?;

    Ok(Some(TraceDetail { run, candidates }))
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct TraceRun {
    pub trace_id: String,
    pub org_id: String,
    pub query: String,
    pub query_embedding_model: Option<String>,
    pub index_version: Option<String>,
    pub reranker_name: Option<String>,
    pub zdr_mode: Option<String>,
    pub dense_retrieval_ms: Option<i32>,
    pub sparse_retrieval_ms: Option<i32>,
    pub rerank_ms: Option<i32>,
    pub total_ms: Option<i32>,
    pub candidate_count_dense: Option<i32>,
    pub candidate_count_sparse: Option<i32>,
    pub candidate_count_fused: Option<i32>,
    pub candidate_count_reranked: Option<i32>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Blend weights actually used for this query (D4+D5 spec §7).
    /// JSONB shape: {w_dense, w_bm25, w_graph, w_wiki, rerank}.
    #[sqlx(default)]
    pub mode_mix: Option<serde_json::Value>,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct TraceCandidateRow {
    pub rank: i32,
    pub knowledge_id: Option<String>,
    pub document_id: Option<String>,
    pub dense_score: Option<f32>,
    pub sparse_score: Option<f32>,
    pub rerank_score: Option<f32>,
    pub final_score: Option<f32>,
}

#[derive(Debug, serde::Serialize)]
pub struct TraceDetail {
    pub run: TraceRun,
    pub candidates: Vec<TraceCandidateRow>,
}
