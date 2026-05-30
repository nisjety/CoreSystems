"""Internal Documents API — service-to-service routes.

These routes are NOT behind the Bearer-token auth middleware.
They require a valid X-Internal-Api-Key (or X-Service-Auth) header
plus X-Org-Id to identify the calling org.
"""

from __future__ import annotations

import logging
from typing import Any, Dict, Optional

from fastapi import APIRouter, Depends, HTTPException, status
from pydantic import BaseModel, Field
from sqlalchemy.ext.asyncio import AsyncSession

from app.db.postgres import create_document, get_db
from app.events.publisher import publish_document_created
from app.models.document import DocumentResponse
from shared.internal_auth import InternalAuthContext, require_internal_auth

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/internal/v1/documents", tags=["internal-documents"])


class InternalDocumentCreateRequest(BaseModel):
    """Request body for internal document creation."""

    org_id: str
    source: str
    type: str
    title: str
    content: Optional[str] = None
    metadata: Dict[str, Any] = Field(default_factory=dict)
    created_by: str = "system"


@router.post(
    "",
    response_model=DocumentResponse,
    status_code=status.HTTP_201_CREATED,
)
async def internal_create_document(
    body: InternalDocumentCreateRequest,
    auth: InternalAuthContext = Depends(require_internal_auth),
    db: AsyncSession = Depends(get_db),
) -> DocumentResponse:
    """Create a document on behalf of another service (Quarry, imports-core, etc.)."""
    if body.org_id != auth.org_id:
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Org mismatch between header and body",
        )

    doc = await create_document(
        db,
        org_id=body.org_id,
        source=body.source,
        type=body.type,
        title=body.title,
        content=body.content,
        metadata=body.metadata,
        created_by=body.created_by,
    )
    await publish_document_created(
        document_id=doc["document_id"],
        org_id=doc["org_id"],
        payload={
            "title": doc["title"],
            "source": doc["source"],
            "type": doc["type"],
            "created_by": doc.get("created_by"),
        },
    )

    logger.info(
        "Internal doc created: %s org=%s by=%s",
        doc["document_id"],
        body.org_id,
        auth.service_name,
    )
    return doc
