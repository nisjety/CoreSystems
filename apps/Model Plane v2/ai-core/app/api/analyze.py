"""Unified Analyzer API — routes through the analyzer abstraction.

Provides a single ``/api/v1/analyze`` endpoint that auto-selects the
best backend (Document Intelligence, Content Understanding, or
Mistral Document AI) based on the request content.
"""

from __future__ import annotations

import uuid
from typing import Any

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel, Field

from app.analyzers import analyze as run_analyze
from app.analyzers.base import AnalyzerRequest, DocumentModel

router = APIRouter(prefix="/api/v1/analyze", tags=["analyzer"])


class AnalyzeHTTPRequest(BaseModel):
    """Public HTTP request for the unified analyzer endpoint."""

    url: str = ""
    content_base64: str = ""
    mime_type: str = "application/pdf"
    model: str = "layout"
    backend: str = ""  # Optional explicit backend override
    prompt: str = ""  # For Mistral Document AI reasoning
    pages: str = ""
    locale: str = ""
    org_id: str = ""
    extra: dict[str, Any] = Field(default_factory=dict)


class AnalyzeHTTPResponse(BaseModel):
    """Public HTTP response from analyzer."""

    request_id: str
    backend: str
    content: str
    pages: list[dict[str, Any]] = Field(default_factory=list)
    tables: list[dict[str, Any]] = Field(default_factory=list)
    key_value_pairs: list[dict[str, Any]] = Field(default_factory=list)
    entities: list[dict[str, Any]] = Field(default_factory=list)


@router.post("", response_model=AnalyzeHTTPResponse)
async def analyze_document(body: AnalyzeHTTPRequest) -> AnalyzeHTTPResponse:
    """Analyze a document/image through the best available backend."""
    if not body.url and not body.content_base64:
        raise HTTPException(status_code=400, detail="url or content_base64 required")

    try:
        doc_model = DocumentModel(body.model)
    except ValueError:
        raise HTTPException(status_code=400, detail=f"Unknown model: {body.model}")

    extra = dict(body.extra)
    if body.backend:
        extra["backend"] = body.backend
    if body.prompt:
        extra["prompt"] = body.prompt

    request = AnalyzerRequest(
        request_id=str(uuid.uuid4()),
        org_id=body.org_id,
        model=doc_model,
        url=body.url,
        content_base64=body.content_base64,
        mime_type=body.mime_type,
        pages=body.pages,
        locale=body.locale,
        extra=extra,
    )

    try:
        result = await run_analyze(request)
    except RuntimeError as exc:
        raise HTTPException(status_code=503, detail=str(exc))
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc))

    return AnalyzeHTTPResponse(
        request_id=result.request_id,
        backend=result.backend.value,
        content=result.content,
        pages=result.pages,
        tables=result.tables,
        key_value_pairs=result.key_value_pairs,
        entities=result.entities,
    )
