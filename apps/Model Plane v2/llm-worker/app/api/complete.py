"""Completion endpoints — POST /v1/complete and POST /v1/stream."""

from __future__ import annotations

import logging

from fastapi import APIRouter, HTTPException, Request

from app.domain import CompletionEvent, CompletionRequest, CompletionResponse
from app.executor import execute, execute_stream
from app.nats_client import publish_completed
from app.redis_client import check_rate_limit
from app.result_store import should_store, store
from app.streaming import sse_response

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/v1", tags=["completions"])


@router.post("/complete", response_model=CompletionResponse)
async def complete(req: CompletionRequest, request: Request):
    """Non-streaming completion."""

    # Rate limit
    allowed = await check_rate_limit(req.org_id)
    if not allowed:
        raise HTTPException(429, "Rate limit exceeded")

    resp = await execute(req)

    # Store large payloads
    if resp.content and should_store(resp.content):
        result_id = store(resp.content, req.org_id, req.request_id)
        resp = resp.model_copy(update={"result_id": result_id})

    # Publish NATS event (fire-and-forget)
    try:
        await publish_completed(
            CompletionEvent(
                request_id=req.request_id,
                org_id=req.org_id,
                provider=resp.provider,
                model_used=resp.model_used,
                tokens_in=resp.tokens_in,
                tokens_out=resp.tokens_out,
                latency_ms=resp.latency_ms,
                finish_reason=resp.finish_reason,
                session_id=req.session_id,
                run_id=req.run_id,
            )
        )
    except Exception:
        logger.warning("nats_publish_failed request_id=%s", req.request_id)

    return resp


@router.post("/stream")
async def stream_complete(req: CompletionRequest, request: Request):
    """Streaming completion via SSE."""

    allowed = await check_rate_limit(req.org_id)
    if not allowed:
        raise HTTPException(429, "Rate limit exceeded")

    chunks = execute_stream(req)
    return sse_response(chunks, req.request_id)
