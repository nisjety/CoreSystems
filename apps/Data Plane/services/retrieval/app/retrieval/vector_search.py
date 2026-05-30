"""
Vector Search — embed the query and search Qdrant for semantic candidates.

Step 1: embed query text via Cohere (input_type="search_query")
Step 2: search Qdrant with hard filter + HNSW approximate nearest neighbors
Step 3: return raw candidates with scores and payloads

The candidates are then passed to the reranker before being returned to AI-Core.
"""
from __future__ import annotations

import asyncio
import logging
import threading
from typing import Any, Dict, List

import httpx
from qdrant_client import QdrantClient
from qdrant_client.http import models as qm

from app.config import settings

logger = logging.getLogger(__name__)

_qdrant: QdrantClient | None = None
_embedding_http_client: httpx.AsyncClient | None = None
_qdrant_lock = threading.Lock()
_embedding_http_client_lock = asyncio.Lock()


def get_qdrant() -> QdrantClient:
    global _qdrant
    if _qdrant is None:
        with _qdrant_lock:
            if _qdrant is None:
                _qdrant = QdrantClient(host=settings.qdrant_host, port=settings.qdrant_port)
    return _qdrant


async def get_embedding_http_client() -> httpx.AsyncClient:
    global _embedding_http_client
    if _embedding_http_client is None:
        async with _embedding_http_client_lock:
            if _embedding_http_client is None:
                _embedding_http_client = httpx.AsyncClient(
                    timeout=30.0,
                    limits=httpx.Limits(max_connections=100, max_keepalive_connections=20),
                )
    return _embedding_http_client


async def close_embedding_http_client() -> None:
    global _embedding_http_client
    if _embedding_http_client is not None:
        await _embedding_http_client.aclose()
        _embedding_http_client = None


async def embed_query(query: str) -> List[float]:
    """Embed a single retrieval query (non-batched, low-latency path)."""
    url = (
        f"{settings.azure_openai_endpoint}/openai/deployments"
        f"/{settings.azure_openai_deployment}/embeddings"
        f"?api-version={settings.azure_openai_api_version}"
    )
    headers = {
        "api-key": settings.azure_openai_api_key,
        "Content-Type": "application/json",
    }
    data = {"input": query}

    try:
        client = await get_embedding_http_client()
        response = await client.post(url, headers=headers, json=data)
        response.raise_for_status()
    except httpx.HTTPStatusError as exc:
        logger.error("Azure OpenAI embedding request failed: status=%s", exc.response.status_code)
        raise RuntimeError(
            f"Azure OpenAI embedding request failed with status {exc.response.status_code}"
        ) from exc
    except httpx.RequestError as exc:
        logger.error("Azure OpenAI embedding request could not be completed: %s", exc)
        raise RuntimeError("Azure OpenAI embedding request failed") from exc

    try:
        result = response.json()
        return result["data"][0]["embedding"]
    except (KeyError, IndexError, TypeError, ValueError) as exc:
        logger.error("Azure OpenAI embedding response had an unexpected shape: %s", exc)
        raise RuntimeError("Azure OpenAI embedding response was invalid") from exc


async def vector_search(
    query_vector: List[float],
    qdrant_filter: qm.Filter,
    top_k: int,
) -> List[Dict[str, Any]]:
    """
    Perform ANN search in Qdrant.

    Returns a list of dicts:
      knowledge_id, document_id, org_id, text, score, metadata
    """
    loop = asyncio.get_event_loop()
    results = await loop.run_in_executor(
        None,
        lambda: get_qdrant().search(
            collection_name=settings.qdrant_collection,
            query_vector=query_vector,
            query_filter=qdrant_filter,
            limit=top_k,
            with_payload=True,
            with_vectors=False,
        ),
    )

    candidates = []
    for hit in results:
        payload = hit.payload or {}
        candidates.append(
            {
                "knowledge_id": payload.get("knowledge_id", str(hit.id)),
                "document_id":  payload.get("document_id", ""),
                "org_id":       payload.get("org_id", ""),
                "text":         payload.get("text", ""),
                "score":        hit.score,
                "metadata":     {
                    k: v
                    for k, v in payload.items()
                    if k not in {"knowledge_id", "document_id", "org_id", "text"}
                },
            }
        )

    logger.debug("vector_search top_k=%d returned=%d", top_k, len(candidates))
    return candidates
