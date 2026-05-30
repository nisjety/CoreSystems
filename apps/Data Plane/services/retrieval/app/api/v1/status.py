"""Document indexing status API — internal-only endpoint.

GET /v1/documents/{document_id}/status
  Returns embedding-status counts for a document's knowledge units.
  Authenticated via x-internal-key header (service-to-service).
"""
from __future__ import annotations

import logging
from typing import Optional

from fastapi import APIRouter, Header, HTTPException
from pydantic import BaseModel

from app.config import settings

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/v1/documents", tags=["status"])


class DocumentStatusResponse(BaseModel):
    document_id: str
    total: int
    pending: int
    processing: int
    done: int
    failed: int
    is_complete: bool


def _verify_internal_key(
    x_internal_key: Optional[str] = Header(None, alias="x-internal-key"),
) -> None:
    if not settings.internal_api_key:
        raise HTTPException(status_code=500, detail="Internal API key not configured")
    if x_internal_key != settings.internal_api_key:
        raise HTTPException(status_code=403, detail="Invalid internal key")


@router.get("/{document_id}/status", response_model=DocumentStatusResponse)
async def get_document_status(
    document_id: str,
    x_internal_key: Optional[str] = Header(None, alias="x-internal-key"),
) -> DocumentStatusResponse:
    """Return embedding pipeline status for a document's knowledge units."""
    _verify_internal_key(x_internal_key)

    from sqlalchemy import text
    from app.db import _engine as engine

    query = text("""
        SELECT
            embedding_status,
            COUNT(*) AS cnt
        FROM knowledge_units
        WHERE document_id = :doc_id
        GROUP BY embedding_status
    """)

    counts: dict[str, int] = {}
    async with engine.connect() as conn:
        result = await conn.execute(query, {"doc_id": document_id})
        for row in result:
            counts[row.embedding_status] = row.cnt

    total = sum(counts.values())
    pending = counts.get("pending", 0)
    processing = counts.get("processing", 0)
    done = counts.get("done", 0) + counts.get("embedded", 0)
    failed = counts.get("failed", 0)

    return DocumentStatusResponse(
        document_id=document_id,
        total=total,
        pending=pending,
        processing=processing,
        done=done,
        failed=failed,
        is_complete=total > 0 and (done + failed) == total,
    )
