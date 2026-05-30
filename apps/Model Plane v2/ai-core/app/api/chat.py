"""Chat API — public conversational endpoint (session-core → ai-core)."""

from __future__ import annotations

import logging
import uuid

from fastapi import APIRouter, HTTPException, Request

from app.domain import ChatRequest, ChatResponse
from app.pipeline.runner import run_pipeline

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/api/v1", tags=["chat"])


@router.post("/chat", response_model=ChatResponse)
async def chat(body: ChatRequest, request: Request):
    """Process a chat message through the 10-layer pipeline."""
    request_id = str(uuid.uuid4())

    try:
        result = await run_pipeline(
            request_id=request_id,
            message=body.message,
            org_id=body.org_id,
            session_id=body.session_id,
            model=body.model,
            provider=body.provider,
            temperature=body.temperature,
            max_tokens=body.max_tokens,
            stream=body.stream,
            tools=body.tools,
            context=body.context,
        )
        return result
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc))
    except Exception:
        logger.exception("chat_error request_id=%s", request_id)
        raise HTTPException(status_code=500, detail="Internal pipeline error")


@router.post("/chat/stream")
async def chat_stream(body: ChatRequest, request: Request):
    """Streaming chat via SSE — same pipeline, streaming output."""
    from app.pipeline.runner import run_pipeline_stream
    from reasoning_runtime.streaming import sse_response

    request_id = str(uuid.uuid4())

    try:
        chunks = run_pipeline_stream(
            request_id=request_id,
            message=body.message,
            org_id=body.org_id,
            session_id=body.session_id,
            model=body.model,
            provider=body.provider,
            temperature=body.temperature,
            max_tokens=body.max_tokens,
            tools=body.tools,
            context=body.context,
        )
        return sse_response(chunks, request_id)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc))
