"""
Reranker — applies Cohere cross-encoder reranking on vector search candidates.

Why rerank?
  Vector similarity measures semantic proximity in embedding space.
  A cross-encoder reads query + document together and scores relevance
  with far higher precision — at the cost of being O(N) per query.

  We run it on the TOP_K vector candidates (typically 20), then return
  the TOP_N best (typically 5). This two-stage design keeps latency low
  while maximising accuracy.

Note: Uses Azure-hosted Cohere rerank endpoint (not public API) via httpx
because the Cohere SDK doesn't support custom endpoints.
"""
from __future__ import annotations

import asyncio
import logging
from typing import Any, Dict, List

import httpx

from app.config import settings

logger = logging.getLogger(__name__)

_rerank_http_client: httpx.AsyncClient | None = None
_rerank_http_client_lock = asyncio.Lock()


def _annotate_with_rerank_score(candidate: Dict[str, Any], score: float) -> Dict[str, Any]:
    annotated_candidate = candidate.copy()
    annotated_candidate["rerank_score"] = score
    return annotated_candidate


async def get_rerank_http_client() -> httpx.AsyncClient:
    global _rerank_http_client
    if _rerank_http_client is None:
        async with _rerank_http_client_lock:
            if _rerank_http_client is None:
                _rerank_http_client = httpx.AsyncClient(
                    timeout=30.0,
                    limits=httpx.Limits(max_connections=100, max_keepalive_connections=20),
                )
    return _rerank_http_client


async def close_rerank_http_client() -> None:
    global _rerank_http_client
    if _rerank_http_client is not None:
        await _rerank_http_client.aclose()
        _rerank_http_client = None


async def rerank(
    query: str,
    candidates: List[Dict[str, Any]],
    top_n: int,
) -> List[Dict[str, Any]]:
    """
    Rerank `candidates` against `query` using Cohere rerank API.

    Always attempts reranking if candidates > 1 (to get relevance scores).
    If reranking fails, falls back to vector scores.

    Returns top_n candidates sorted by relevance_score (highest first),
    each annotated with a `rerank_score` field.
    """
    if not candidates:
        return []

    # Always rerank if we have multiple candidates (even if <= top_n)
    # This ensures all candidates get relevance scores, not just when we need to filter
    if len(candidates) > 1:
        reranked = await _rerank_async(query, candidates, top_n)
        return reranked[:top_n]  # Ensure we return at most top_n
    
    # Single candidate — no reranking needed, but keep the response contract stable
    return [
        _annotate_with_rerank_score(candidate, float(candidate.get("score", 0.0)))
        for candidate in candidates[:top_n]
    ]


async def _rerank_async(
    query: str,
    candidates: List[Dict[str, Any]],
    top_n: int,
) -> List[Dict[str, Any]]:
    """Call Azure-hosted Cohere rerank endpoint via httpx."""
    if not candidates:
        return []
    
    texts = [c["text"] for c in candidates]
    
    # Build request for Azure Cohere rerank API
    # Azure format: POST /providers/cohere/v2/rerank
    url = f"{settings.cohere_base_url}/rerank"
    headers = {
        "api-key": settings.cohere_api_key,
        "Content-Type": "application/json",
    }
    payload = {
        "query": query,
        "documents": texts,
        "model": settings.cohere_rerank_model,
        "top_n": top_n,
        "return_documents": False,
    }
    
    try:
        client = await get_rerank_http_client()
        response = await client.post(url, headers=headers, json=payload)
        response.raise_for_status()
        result = response.json()
        
        # Parse Azure Cohere rerank response
        # Format: {"results": [{"index": 0, "relevance_score": 0.92}, ...]}
        reranked_results = result.get("results", [])
        
        results: List[Dict[str, Any]] = []
        for rerank_hit in reranked_results:
            idx = rerank_hit["index"]
            candidate = candidates[idx].copy()
            candidate["rerank_score"] = rerank_hit.get("relevance_score", 0.0)
            results.append(candidate)
        
        logger.debug(
            "reranked: input=%d → output=%d", len(candidates), len(results)
        )
        return results
    
    except (httpx.RequestError, httpx.HTTPStatusError) as exc:
        logger.error("rerank api error: %s", exc)
        # Fallback: return top_n by vector score if rerank fails
        return [
            _annotate_with_rerank_score(candidate, float(candidate.get("score", 0.0)))
            for candidate in sorted(
                candidates,
                key=lambda c: c.get("score", 0),
                reverse=True,
            )[:top_n]
        ]
