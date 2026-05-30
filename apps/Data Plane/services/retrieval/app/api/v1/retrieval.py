"""
Retrieval API — v1 routes.

POST /v1/retrieve
  The single endpoint AI-Core calls.
  Returns structured facts, never raw documents.

Internal callers (agent-core v2) may authenticate via x-internal-key +
x-org-id headers instead of gRPC session auth.
"""
from __future__ import annotations

import logging
from typing import Any, Dict, List, Optional, Tuple

from fastapi import APIRouter, Depends, Header, HTTPException, Request, status
from pydantic import BaseModel, Field

from app.config import settings
from app.retrieval.service import QuotaExceededError, execute_retrieval

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/v1", tags=["retrieval"])


# ── Auth helpers ──────────────────────────────────────────────────────────────

def _get_auth(request: Request):
    """Extract AuthContext from request.state (set by auth middleware)."""
    from shared.auth_middleware import get_auth_context
    return get_auth_context(request)


def _get_org_access(request: Request):
    """Extract OrgAccess from request.state (set by authz middleware)."""
    access = getattr(request.state, "org_access", None)
    if access is None:
        raise HTTPException(status_code=403, detail="Organization access not resolved")
    return access


def _resolve_org_id(
    request: Request,
    x_internal_key: Optional[str] = Header(None, alias="x-internal-key"),
    x_org_id: Optional[str] = Header(None, alias="x-org-id"),
) -> str:
    """Return org_id from internal-key auth or session auth.

    Internal callers (agent-core, etc.) send x-internal-key + x-org-id.
    External callers go through the normal gRPC auth middleware path.
    """
    if x_internal_key is not None:
        if not settings.internal_api_key:
            raise HTTPException(
                status_code=500,
                detail="Internal API key not configured on server",
            )
        if x_internal_key != settings.internal_api_key:
            raise HTTPException(status_code=403, detail="Invalid internal key")
        if not x_org_id:
            raise HTTPException(status_code=400, detail="x-org-id header required for internal auth")
        logger.debug("internal_auth org_id=%s", x_org_id)
        return x_org_id

    # Fall back to session-based auth
    auth = _get_auth(request)
    access = _get_org_access(request)
    if not access.has_permission("resources:read"):
        raise HTTPException(status_code=403, detail="Permission 'resources:read' required")
    return auth.org_id


# ── Request / Response models ─────────────────────────────────────────────────

class FiltersModel(BaseModel):
    document_types: Optional[List[str]] = None
    departments:    Optional[List[str]] = None
    languages:      Optional[List[str]] = None
    document_ids:   Optional[List[str]] = None
    region:         Optional[str]       = None


class RetrieveRequest(BaseModel):
    # org_id removed — derived from authenticated session
    query:   str  = Field(..., min_length=1, max_length=2048)
    filters: FiltersModel = Field(default_factory=FiltersModel)
    top_k:   Optional[int] = Field(None, ge=1, le=100, description="Override candidate pool size")
    top_n:   Optional[int] = Field(None, ge=1, le=20,  description="Override returned facts count")
    context_window: Optional[int] = Field(None, ge=1, description="Model context window (tokens) for packing")


class FactModel(BaseModel):
    knowledge_id: str
    document_id:  str
    text:         str
    score:        float
    rerank_score: Optional[float] = None
    bm25_score:   Optional[float] = None
    rrf_score:    Optional[float] = None
    metadata:     Dict[str, Any]  = {}


class SourceModel(BaseModel):
    document_id: str
    title:       str
    source:      str
    type:        str


class RetrieveResponse(BaseModel):
    facts:   List[FactModel]
    sources: List[SourceModel]
    query:   str
    org_id:  str
    low_confidence: bool = False


# ── Endpoint ──────────────────────────────────────────────────────────────────

@router.post("/retrieve", response_model=RetrieveResponse)
async def retrieve_endpoint(
    body: RetrieveRequest,
    request: Request,
    org_id: str = Depends(_resolve_org_id),
) -> RetrieveResponse:
    """
    Core retrieval endpoint.

    AI-Core sends: query + optional filters (org_id from authenticated session).
    Internal callers: x-internal-key + x-org-id headers.
    Data Plane returns: ranked facts + source references.

    AI NEVER sees raw documents. AI NEVER talks to Qdrant.
    """
    try:
        result = await execute_retrieval(
            org_id=org_id,
            query=body.query,
            document_types=body.filters.document_types,
            departments=body.filters.departments,
            languages=body.filters.languages,
            document_ids=body.filters.document_ids,
            region=body.filters.region,
            top_k=body.top_k,
            top_n=body.top_n,
            context_window=body.context_window,
        )
    except QuotaExceededError as exc:
        raise HTTPException(
            status_code=status.HTTP_429_TOO_MANY_REQUESTS,
            detail=str(exc),
        )
    except ValueError as exc:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=str(exc),
        )
    except Exception as exc:
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(exc),
        )

    return RetrieveResponse(**result)


# ── Agentic RAG endpoint ──────────────────────────────────────────────────────


class AgenticRetrieveRequest(BaseModel):
    """Request body for the 6-agent agentic retrieval pipeline.

    Identical to RetrieveRequest but returns a synthesised answer in addition
    to the raw facts.  Requires ``AGENTIC_RAG_ENABLED=true`` on the server.
    """

    query:          str = Field(..., min_length=1, max_length=2048)
    filters:        FiltersModel = Field(default_factory=FiltersModel)
    top_k:          Optional[int] = Field(None, ge=1, le=100)
    top_n:          Optional[int] = Field(None, ge=1, le=20)
    context_window: Optional[int] = Field(None, ge=1)


class AgenticRetrieveResponse(BaseModel):
    """Full agentic RAG response.

    Includes the synthesised answer, the supporting facts, source metadata,
    the sub-queries that were generated by the Planning agent, and observability
    timings.
    """

    answer:               str
    facts:                List[FactModel]
    sources:              List[SourceModel]
    sub_queries:          List[str]
    retrieval_iterations: int
    model_used:           str
    input_tokens:         int = 0
    output_tokens:        int = 0
    low_confidence:       bool = False
    latency_ms:           float = 0.0
    timings:              Dict[str, float] = {}


@router.post("/retrieve/agentic", response_model=AgenticRetrieveResponse)
async def agentic_retrieve_endpoint(
    body: AgenticRetrieveRequest,
    request: Request,
    org_id: str = Depends(_resolve_org_id),
) -> AgenticRetrieveResponse:
    """6-agent agentic RAG: plan → route → retrieve → rerank → reflect → synthesise.

    Returns a grounded, cited answer in addition to the supporting facts.

    Requires ``AGENTIC_RAG_ENABLED=true``.  Falls back to the standard retrieval
    pipeline when agentic RAG is disabled (returns an empty answer with facts only).

    Architecture:
    1. **Planning**   — Decomposes query into focused sub-queries
    2. **Routing**    — Selects optimal retrieval strategy per sub-query
    3. **Retrieval**  — Executes parallel retrieval for all sub-queries
    4. **Reranking**  — Merges and cross-encodes the combined candidate pool
    5. **Reflection** — Evaluates sufficiency and triggers extra passes if needed
    6. **Synthesis**  — Generates a grounded, cited final answer
    """
    if not settings.agentic_rag_enabled:
        # Graceful degradation: run standard pipeline, return empty answer
        try:
            result = await execute_retrieval(
                org_id=org_id,
                query=body.query,
                document_types=body.filters.document_types,
                departments=body.filters.departments,
                languages=body.filters.languages,
                document_ids=body.filters.document_ids,
                region=body.filters.region,
                top_k=body.top_k,
                top_n=body.top_n,
                context_window=body.context_window,
            )
        except QuotaExceededError as exc:
            raise HTTPException(status_code=status.HTTP_429_TOO_MANY_REQUESTS, detail=str(exc))
        except Exception as exc:
            raise HTTPException(status_code=status.HTTP_500_INTERNAL_SERVER_ERROR, detail=str(exc))

        return AgenticRetrieveResponse(
            answer="",  # synthesis disabled
            facts=result.get("facts", []),
            sources=result.get("sources", []),
            sub_queries=[body.query],
            retrieval_iterations=1,
            model_used="",
            low_confidence=result.get("low_confidence", False),
        )

    from app.retrieval.agentic_pipeline import AgenticRAGPipeline

    try:
        pipeline = AgenticRAGPipeline()
        agentic_result = await pipeline.run(
            query=body.query,
            org_id=org_id,
            document_types=body.filters.document_types,
            departments=body.filters.departments,
            languages=body.filters.languages,
            document_ids=body.filters.document_ids,
            region=body.filters.region,
            top_k=body.top_k,
            top_n=body.top_n,
            context_window=body.context_window,
        )
    except QuotaExceededError as exc:
        raise HTTPException(status_code=status.HTTP_429_TOO_MANY_REQUESTS, detail=str(exc))
    except Exception as exc:
        logger.exception("agentic_rag_pipeline_error")
        raise HTTPException(status_code=status.HTTP_500_INTERNAL_SERVER_ERROR, detail=str(exc))

    return AgenticRetrieveResponse(
        answer=agentic_result.answer,
        facts=agentic_result.facts,
        sources=agentic_result.sources,
        sub_queries=agentic_result.sub_queries,
        retrieval_iterations=agentic_result.retrieval_iterations,
        model_used=agentic_result.model_used,
        input_tokens=agentic_result.input_tokens,
        output_tokens=agentic_result.output_tokens,
        low_confidence=agentic_result.low_confidence,
        latency_ms=agentic_result.latency_ms,
        timings=agentic_result.timings,
    )
