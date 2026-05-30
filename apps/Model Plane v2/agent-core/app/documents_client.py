"""Data Plane HTTP client — document ingestion, retrieval, and management.

Talks to two v1 Data Plane services:
  - retrieval-service (:8004)  — POST /v1/retrieve, GET /v1/documents/{id}/status
  - documents-service (:8001)  — POST /v1/documents, GET /v1/documents, DELETE /v1/documents/{id}

Authenticates via x-internal-key + x-org-id headers (A5 internal bypass).
"""

from __future__ import annotations

import logging
from typing import Any

import httpx

from app.config import settings

logger = logging.getLogger(__name__)


class DocumentsClient:
    """Async HTTP client to Data Plane v1 services.

    Replaces the old documents-worker client. Handles document retrieval
    (vector search + reranking) and document management (CRUD + indexing).
    """

    def __init__(self) -> None:
        self._retrieval: httpx.AsyncClient | None = None
        self._documents: httpx.AsyncClient | None = None

    async def open(self) -> None:
        headers = self._base_headers()
        self._retrieval = httpx.AsyncClient(
            base_url=settings.data_plane_retrieval_url,
            timeout=httpx.Timeout(30, connect=10),
            headers=headers,
        )
        self._documents = httpx.AsyncClient(
            base_url=settings.data_plane_documents_url,
            timeout=httpx.Timeout(30, connect=10),
            headers=headers,
        )

    async def close(self) -> None:
        if self._retrieval:
            await self._retrieval.aclose()
            self._retrieval = None
        if self._documents:
            await self._documents.aclose()
            self._documents = None

    def _base_headers(self) -> dict[str, str]:
        h: dict[str, str] = {}
        if settings.data_plane_internal_key:
            h["x-internal-key"] = settings.data_plane_internal_key
        elif settings.internal_api_key:
            # Fallback for local dev without DATA_PLANE_INTERNAL_KEY set
            h["x-internal-key"] = settings.internal_api_key
        return h

    @property
    def retrieval(self) -> httpx.AsyncClient:
        if self._retrieval is None:
            raise RuntimeError("DocumentsClient not opened")
        return self._retrieval

    @property
    def documents(self) -> httpx.AsyncClient:
        if self._documents is None:
            raise RuntimeError("DocumentsClient not opened")
        return self._documents

    # ── Retrieval (via retrieval-service) ─────────────────────────────────────

    async def retrieve(
        self,
        org_id: str,
        query: str,
        top_k: int = 5,
        *,
        workspace_id: str = "",
        filters: dict[str, Any] | None = None,
    ) -> list[dict[str, Any]]:
        """Retrieve ranked document chunks via Data Plane retrieval-service.

        org_id is sent via X-Org-ID header (v1 retrieval derives org from
        internal auth, not from the request body).
        """
        body: dict[str, Any] = {
            "query": query,
            "top_k": top_k,
        }
        if filters:
            # Map to v1 filter shape
            v1_filters: dict[str, Any] = {}
            for key in ("document_types", "departments", "languages", "document_ids", "region"):
                if key in filters:
                    v1_filters[key] = filters[key]
            if workspace_id and "document_ids" not in v1_filters:
                # workspace_id has no direct v1 mapping; skip silently
                pass
            body["filters"] = v1_filters

        resp = await self.retrieval.post(
            "/v1/retrieve",
            json=body,
            headers={"x-org-id": org_id},
        )
        resp.raise_for_status()
        data = resp.json()
        # v1 returns {facts, sources, query, org_id}
        return data.get("facts", [])

    # ── Document indexing (via documents-service) ─────────────────────────────

    async def index_document(
        self,
        org_id: str,
        *,
        title: str = "",
        content: str = "",
        file_key: str = "",
        content_type: str = "text/plain",
        workspace_id: str = "",
        metadata: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        """Create/ingest a document via Data Plane documents-service."""
        body: dict[str, Any] = {
            "content_type": content_type,
        }
        if title:
            body["title"] = title
        if content:
            body["content"] = content
        if file_key:
            body["file_key"] = file_key
        if metadata:
            body["metadata"] = metadata

        resp = await self.documents.post(
            "/v1/documents",
            json=body,
            headers={"x-org-id": org_id},
        )
        resp.raise_for_status()
        return resp.json()

    # ── Document management ───────────────────────────────────────────────────

    async def get_document(self, org_id: str, document_id: str) -> dict[str, Any]:
        """Fetch a single document by ID."""
        resp = await self.documents.get(
            f"/v1/documents/{document_id}",
            headers={"x-org-id": org_id},
        )
        resp.raise_for_status()
        return resp.json()

    async def delete_document(self, org_id: str, document_id: str) -> None:
        """Delete a document and trigger vector cleanup."""
        resp = await self.documents.delete(
            f"/v1/documents/{document_id}",
            headers={"x-org-id": org_id},
        )
        resp.raise_for_status()

    async def get_index_status(self, document_id: str) -> dict[str, Any]:
        """Check indexing/embedding progress for a document."""
        resp = await self.retrieval.get(
            f"/v1/documents/{document_id}/status",
        )
        resp.raise_for_status()
        return resp.json()
