"""KnowledgeSearchTool — retrieves documents from Data Plane retrieval-service.

Calls the Data Plane's ``POST /v1/retrieve`` endpoint which performs:
1. Embedding via Azure OpenAI text-embedding-3-large (3072-dim)
2. Vector search in Qdrant (top_k configurable)
3. Cohere reranking (rerank-english-v3.0)
4. Metadata join from PostgreSQL

Returns ranked document chunks with relevance scores, metadata, and source info.
"""

from __future__ import annotations

import logging
from typing import Any

import httpx

from app.config import settings
from app.tools.base import ToolResult

logger = logging.getLogger(__name__)

_CLIENT: httpx.AsyncClient | None = None


async def _get_client() -> httpx.AsyncClient:
    global _CLIENT
    if _CLIENT is None or _CLIENT.is_closed:
        _CLIENT = httpx.AsyncClient(
            base_url=settings.data_plane_retrieval_url,
            timeout=httpx.Timeout(30, connect=10),
        )
    return _CLIENT


class KnowledgeSearchTool:
    """Search the organization's knowledge base for relevant documents and context.

    Calls Data Plane retrieval-service for hybrid vector + reranked search.
    """

    name = "knowledge_search"
    description = (
        "Search the organization's knowledge base for relevant documents, "
        "code snippets, and context. Returns ranked results with relevance scores."
    )
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Natural language search query.",
            },
            "top_k": {
                "type": "integer",
                "description": "Number of results to return (1-50).",
                "default": 5,
                "minimum": 1,
                "maximum": 50,
            },
            "collection": {
                "type": "string",
                "description": "Optional collection/namespace to search within.",
                "default": "",
            },
            "filters": {
                "type": "object",
                "description": "Optional metadata filters (e.g. {\"file_type\": \"python\"}).",
                "default": {},
            },
        },
        "required": ["query"],
    }
    search_hint = "knowledge search documents retrieval context rag vector"
    should_defer = False  # Always eager — primary knowledge tool

    def is_read_only(self) -> bool:
        return True

    def is_concurrent_safe(self) -> bool:
        return True

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return (
            "Search the organization's knowledge base for relevant documents "
            "and context. Use when you need information from internal docs, "
            "codebase, or uploaded files."
        )

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        query = input_data.get("query")
        if not query or not isinstance(query, str):
            raise ValueError("'query' is required and must be a non-empty string")
        top_k = input_data.get("top_k", 5)
        if not isinstance(top_k, int) or top_k < 1 or top_k > 50:
            input_data["top_k"] = max(1, min(50, int(top_k)))
        return input_data

    async def call(
        self,
        input_data: dict[str, Any],
        *,
        org_id: str = "",
        **kwargs: Any,
    ) -> ToolResult:
        """Execute knowledge search against Data Plane retrieval-service."""
        query = input_data["query"]
        top_k = input_data.get("top_k", 5)
        collection = input_data.get("collection", "")
        filters = input_data.get("filters", {})

        if not settings.data_plane_retrieval_url:
            return ToolResult(
                output="Knowledge search unavailable: Data Plane not configured.",
                metadata={"configured": False},
            )

        body: dict[str, Any] = {
            "org_id": org_id or "default",
            "query": query,
            "top_k": top_k,
        }
        if collection:
            body["collection"] = collection
        if filters:
            body["filters"] = filters

        client = await _get_client()
        try:
            resp = await client.post("/v1/retrieve", json=body)
            resp.raise_for_status()
            data = resp.json()
        except httpx.ConnectError:
            logger.warning("data_plane_unreachable", extra={"url": settings.data_plane_retrieval_url})
            return ToolResult(
                error="Knowledge search unavailable: Data Plane unreachable.",
                metadata={"configured": True, "reachable": False},
            )
        except httpx.HTTPStatusError as exc:
            logger.warning(
                "data_plane_error",
                extra={"status": exc.response.status_code, "detail": exc.response.text[:200]},
            )
            return ToolResult(
                error=f"Knowledge search failed: HTTP {exc.response.status_code}",
                metadata={"status_code": exc.response.status_code},
            )
        except Exception as exc:
            logger.error("knowledge_search_error", extra={"error": str(exc)})
            return ToolResult(error=f"Knowledge search error: {exc}")

        # Format results for LLM consumption
        results = data.get("results", [])
        if not results:
            return ToolResult(
                output=f"No results found for: {query}",
                metadata={"query": query, "result_count": 0},
            )

        formatted_parts: list[str] = []
        for i, result in enumerate(results, 1):
            score = result.get("score", 0)
            content = result.get("content", result.get("text", ""))
            source = result.get("metadata", {}).get("source", "unknown")
            title = result.get("metadata", {}).get("title", "")
            chunk_id = result.get("id", "")

            header = f"[{i}] (score: {score:.3f})"
            if title:
                header += f" {title}"
            if source:
                header += f" — {source}"

            formatted_parts.append(f"{header}\n{content}")

        output = "\n\n---\n\n".join(formatted_parts)
        return ToolResult(
            output=output,
            metadata={
                "query": query,
                "result_count": len(results),
                "top_score": results[0].get("score", 0) if results else 0,
            },
        )


async def close_knowledge_client() -> None:
    """Shutdown hook."""
    global _CLIENT
    if _CLIENT and not _CLIENT.is_closed:
        await _CLIENT.aclose()
        _CLIENT = None
