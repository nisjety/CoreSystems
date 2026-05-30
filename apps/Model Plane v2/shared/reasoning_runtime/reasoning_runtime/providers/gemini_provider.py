"""Google Gemini provider.

FIX: Streaming is now async — the synchronous ``generate_content_stream``
is wrapped in ``asyncio.to_thread()`` so it doesn't block the event loop.
"""

from __future__ import annotations

import asyncio
import logging
import time
from typing import Any, AsyncIterator

from reasoning_runtime.config import get_config
from reasoning_runtime.domain import CompletionRequest, CompletionResponse

logger = logging.getLogger(__name__)


def _build_client():
    from google import genai

    cfg = get_config()
    return genai.Client(api_key=cfg.google_api_key)


def _convert_messages(
    req: CompletionRequest,
) -> tuple[str | None, list[dict[str, Any]]]:
    system = None
    contents: list[dict[str, Any]] = []
    for m in req.messages:
        if m.role == "system":
            system = m.content if isinstance(m.content, str) else str(m.content)
        else:
            role = "model" if m.role == "assistant" else "user"
            contents.append({"role": role, "parts": [{"text": m.content}]})
    return system, contents


async def complete(req: CompletionRequest) -> CompletionResponse:
    client = _build_client()
    system, contents = _convert_messages(req)
    t0 = time.monotonic()

    config: dict[str, Any] = {
        "temperature": req.temperature,
    }
    if req.max_tokens:
        config["max_output_tokens"] = req.max_tokens
    if req.stop:
        config["stop_sequences"] = req.stop

    resp = client.models.generate_content(
        model=req.model_id,
        contents=contents,
        config=config,
    )

    latency_ms = int((time.monotonic() - t0) * 1000)
    text = resp.text or ""

    tokens_in = 0
    tokens_out = 0
    if hasattr(resp, "usage_metadata") and resp.usage_metadata:
        tokens_in = getattr(resp.usage_metadata, "prompt_token_count", 0) or 0
        tokens_out = getattr(resp.usage_metadata, "candidates_token_count", 0) or 0

    return CompletionResponse(
        request_id=req.request_id,
        content=text,
        finish_reason="stop",
        model_used=req.model_id,
        provider="gemini",
        tokens_in=tokens_in,
        tokens_out=tokens_out,
        latency_ms=latency_ms,
    )


def _sync_stream(client, model: str, contents, config):
    """Collect chunks from the synchronous Gemini streaming API."""
    chunks = []
    for chunk in client.models.generate_content_stream(
        model=model, contents=contents, config=config,
    ):
        if chunk.text:
            chunks.append(chunk.text)
    return chunks


async def stream(req: CompletionRequest) -> AsyncIterator[dict[str, Any]]:
    client = _build_client()
    _, contents = _convert_messages(req)
    t0 = time.monotonic()

    config: dict[str, Any] = {"temperature": req.temperature}
    if req.max_tokens:
        config["max_output_tokens"] = req.max_tokens

    # FIX: Gemini SDK streaming is synchronous — offload to thread pool
    chunks = await asyncio.to_thread(
        _sync_stream, client, req.model_id, contents, config,
    )

    for text in chunks:
        yield {"type": "content", "content": text}

    latency_ms = int((time.monotonic() - t0) * 1000)
    yield {
        "type": "done",
        "metadata": {
            "model_used": req.model_id,
            "finish_reason": "stop",
            "latency_ms": latency_ms,
        },
    }
