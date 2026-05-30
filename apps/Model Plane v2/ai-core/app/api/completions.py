"""Completions API — low-level completion endpoint (v1 compatible)."""

from __future__ import annotations

import logging

from fastapi import APIRouter, HTTPException

from reasoning_runtime.domain import CompletionRequest, CompletionResponse
from reasoning_runtime.executor import execute, execute_stream
from reasoning_runtime.streaming import sse_response

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/api/v1", tags=["completions"])


@router.post("/complete", response_model=CompletionResponse)
async def complete(req: CompletionRequest):
    """Low-level non-streaming completion — direct provider dispatch."""
    try:
        return await execute(req)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc))
    except Exception:
        logger.exception("completion_error request_id=%s", req.request_id)
        raise HTTPException(status_code=500, detail="Completion failed")


@router.post("/stream")
async def stream_complete(req: CompletionRequest):
    """Low-level streaming completion — direct provider dispatch."""
    try:
        chunks = execute_stream(req)
        return sse_response(chunks, req.request_id)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc))
