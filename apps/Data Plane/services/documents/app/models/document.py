"""
Document Pydantic models — request/response shapes + DB row mapping.
"""
from __future__ import annotations

from datetime import datetime
from typing import Any, Dict, Optional

from pydantic import BaseModel, Field


# ── Inbound (API request) ─────────────────────────────────────────────────────

class DocumentCreateRequest(BaseModel):
    # org_id removed — derived from authenticated session (Phase 1.4)
    source:   str = Field(..., examples=["sharepoint", "web", "pdf", "notion"])
    type:     str = Field(..., examples=["policy", "invoice", "handbook"])
    title:    str
    content:  str
    metadata: Dict[str, Any] = Field(default_factory=dict)


class DocumentUpdateRequest(BaseModel):
    title:    Optional[str] = None
    content:  Optional[str] = None
    metadata: Optional[Dict[str, Any]] = None


# ── Outbound (API response) ───────────────────────────────────────────────────

class DocumentResponse(BaseModel):
    document_id:   str
    org_id:        str
    source:        str
    type:          str
    title:         str
    status:        str
    metadata:      Dict[str, Any]
    created_by:    Optional[str] = None  # user_id of uploader (GDPR audit)
    created_at:    datetime
    updated_at:    datetime

    # content excluded from list responses for size; included in detail view
    content: Optional[str] = None


class DocumentListResponse(BaseModel):
    documents: list[DocumentResponse]
    total:     int


# ── Search (autocomplete) ─────────────────────────────────────────────────────

class DocumentSearchItem(BaseModel):
    """Minimal document shape returned by the autocomplete search endpoint."""
    document_id: str
    title:       str
    type:        str
    source:      str


class DocumentSearchResponse(BaseModel):
    results: list[DocumentSearchItem]
    query:   str
    total:   int
