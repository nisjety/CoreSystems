"""
Retrieval Pipeline — orchestrates the full retrieval flow.

AI-Core calls this. It never talks to Qdrant directly.

Steps:
  1. Hard filter construction (org + metadata)
  2. Query embedding (Azure OpenAI text-embedding-3-large)
  3. Vector search (Qdrant ANN)
  3b. BM25 lexical scoring + RRF fusion (Phase 2.3 — hybrid retrieval)
  4. Rerank (Cohere cross-encoder)
  4b. Confidence gating (Phase 2.4)
  5. Context-window-aware packing (Phase 2.5)
  6. Format and return facts + sources
"""
from __future__ import annotations

import logging
from time import perf_counter
from typing import Any, Dict, List, Optional

from app.retrieval.filters import build_qdrant_filter
from app.observability import RETRIEVAL_RESULTS_TOTAL, RETRIEVAL_STAGE_DURATION_SECONDS
from app.retrieval.rerank import rerank
from app.retrieval.vector_search import embed_query, vector_search
from app.retrieval.bm25_search import bm25_score, is_available as bm25_available, reciprocal_rank_fusion
from app.config import settings

logger = logging.getLogger(__name__)


async def retrieve(
    *,
    org_id: str,
    query: str,
    document_types: Optional[List[str]] = None,
    departments: Optional[List[str]] = None,
    languages: Optional[List[str]] = None,
    document_ids: Optional[List[str]] = None,
    region: Optional[str] = None,
    top_k: Optional[int] = None,
    top_n: Optional[int] = None,
    context_window: Optional[int] = None,
) -> Dict[str, Any]:
    """
    Full retrieval pipeline.

    Args:
        context_window: If provided (token count), dynamically adjust top_n
            to fit within 30% of the context window budget (Phase 2.5).

    Returns:
      {
        "facts":   [ { knowledge_id, document_id, text, score, metadata, rerank_score }, ... ],
        "sources": [ { document_id, title, source, type }, ... ],
        "query":   str,
        "org_id":  str,
        "low_confidence": bool,  # Phase 2.4
      }
    """
    effective_top_k = top_k or settings.top_k
    effective_top_n = top_n or settings.top_n_after_rerank

    logger.info(
        "retrieve org_id=%s query=%r top_k=%d top_n=%d hybrid=%s",
        org_id, query[:60], effective_top_k, effective_top_n,
        settings.hybrid_enabled,
    )
    total_started_at = perf_counter()

    # Step 1 — Hard filter
    qdrant_filter = build_qdrant_filter(
        org_id=org_id,
        document_types=document_types,
        departments=departments,
        languages=languages,
        document_ids=document_ids,
        region=region,
    )

    # Step 2 — Embed query
    embed_started_at = perf_counter()
    query_vector = await embed_query(query)
    RETRIEVAL_STAGE_DURATION_SECONDS.labels(stage="embed_query").observe(
        perf_counter() - embed_started_at
    )

    # Step 3 — Vector search (pull more candidates for hybrid fusion)
    bm25_top_k = effective_top_k * settings.hybrid_bm25_top_k_factor
    search_top_k = bm25_top_k if (settings.hybrid_enabled and bm25_available()) else effective_top_k

    vector_search_started_at = perf_counter()
    candidates = await vector_search(
        query_vector=query_vector,
        qdrant_filter=qdrant_filter,
        top_k=search_top_k,
    )
    RETRIEVAL_STAGE_DURATION_SECONDS.labels(stage="vector_search").observe(
        perf_counter() - vector_search_started_at
    )

    if not candidates:
        logger.info("retrieve returned 0 candidates for org_id=%s", org_id)
        RETRIEVAL_RESULTS_TOTAL.labels(result="empty").inc()
        RETRIEVAL_STAGE_DURATION_SECONDS.labels(stage="total").observe(
            perf_counter() - total_started_at
        )
        return {"facts": [], "sources": [], "query": query, "org_id": org_id, "low_confidence": True}

    # Step 3b — Hybrid BM25 fusion (Phase 2.3)
    if settings.hybrid_enabled and bm25_available() and len(candidates) > 1:
        bm25_started_at = perf_counter()
        bm25_ranked = bm25_score(query, candidates)
        candidates = reciprocal_rank_fusion(
            dense_ranked=candidates,
            bm25_ranked=bm25_ranked,
        )[:effective_top_k]  # trim back to top_k for reranker
        RETRIEVAL_STAGE_DURATION_SECONDS.labels(stage="bm25_fusion").observe(
            perf_counter() - bm25_started_at
        )

    # Step 4 — Rerank
    rerank_started_at = perf_counter()
    facts = await rerank(query=query, candidates=candidates, top_n=effective_top_n)
    RETRIEVAL_STAGE_DURATION_SECONDS.labels(stage="rerank").observe(
        perf_counter() - rerank_started_at
    )

    # Step 4b — Confidence gating (Phase 2.4)
    low_confidence = False
    if facts:
        top_score = facts[0].get("rerank_score", 0.0)
        if top_score < settings.confidence_threshold:
            low_confidence = True
            logger.info(
                "low_confidence org_id=%s top_rerank_score=%.3f threshold=%.3f",
                org_id, top_score, settings.confidence_threshold,
            )

    # Step 5 — Context-window-aware packing (Phase 2.5)
    if context_window and context_window > 0:
        facts = _pack_to_budget(facts, context_window)

    # Step 6 — Build sources (unique documents referenced by returned facts)
    seen_docs: set[str] = set()
    sources: List[Dict[str, Any]] = []
    for fact in facts:
        doc_id = fact["document_id"]
        if doc_id not in seen_docs:
            seen_docs.add(doc_id)
            sources.append(
                {
                    "document_id": doc_id,
                    "title":  fact["metadata"].get("title", ""),
                    "source": fact["metadata"].get("source", ""),
                    "type":   fact["metadata"].get("type", ""),
                }
            )

    RETRIEVAL_RESULTS_TOTAL.labels(result="facts").inc(len(facts))
    RETRIEVAL_STAGE_DURATION_SECONDS.labels(stage="total").observe(
        perf_counter() - total_started_at
    )

    return {
        "facts":          facts,
        "sources":        sources,
        "query":          query,
        "org_id":         org_id,
        "low_confidence": low_confidence,
    }


def _pack_to_budget(
    facts: List[Dict[str, Any]],
    context_window: int,
    budget_fraction: float = 0.3,
) -> List[Dict[str, Any]]:
    """Keep only facts that fit within *budget_fraction* of *context_window*.

    Uses a rough 4-chars-per-token heuristic (matching cl100k_base for
    English text) to avoid a tiktoken dependency in the retrieval service.
    """
    budget_tokens = int(context_window * budget_fraction)
    packed: List[Dict[str, Any]] = []
    used_tokens = 0

    for fact in facts:
        text = fact.get("text", "")
        est_tokens = len(text) // 4 + 1
        if used_tokens + est_tokens > budget_tokens:
            # Truncate the text to fit remaining budget
            remaining = budget_tokens - used_tokens
            if remaining > 20:
                truncated = fact.copy()
                truncated["text"] = text[: remaining * 4] + "..."
                packed.append(truncated)
            break
        packed.append(fact)
        used_tokens += est_tokens

    logger.debug(
        "context_packing budget=%d tokens, packed=%d/%d facts",
        budget_tokens, len(packed), len(facts),
    )
    return packed
