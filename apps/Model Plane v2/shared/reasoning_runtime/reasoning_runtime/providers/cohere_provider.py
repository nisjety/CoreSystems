"""Cohere Command provider."""

from __future__ import annotations

import logging
import time
from typing import Any, AsyncIterator

import cohere

from reasoning_runtime.config import get_config
from reasoning_runtime.domain import CompletionRequest, CompletionResponse

logger = logging.getLogger(__name__)


def _build_client() -> cohere.AsyncClientV2:
    cfg = get_config()
    return cohere.AsyncClientV2(api_key=cfg.cohere_api_key)


def _convert_messages(
    req: CompletionRequest,
) -> list[dict[str, Any]]:
    messages: list[dict[str, Any]] = []
    for m in req.messages:
        messages.append({"role": m.role, "content": m.content})
    return messages


async def complete(req: CompletionRequest) -> CompletionResponse:
    client = _build_client()
    messages = _convert_messages(req)
    t0 = time.monotonic()

    params: dict[str, Any] = {
        "model": req.model_id,
        "messages": messages,
        "temperature": req.temperature,
    }
    if req.max_tokens:
        params["max_tokens"] = req.max_tokens
    if req.stop:
        params["stop_sequences"] = req.stop

    resp = await client.chat(**params)
    latency_ms = int((time.monotonic() - t0) * 1000)

    content = ""
    if hasattr(resp, "message") and resp.message:
        for block in resp.message.content:
            if hasattr(block, "text"):
                content += block.text

    tokens_in = 0
    tokens_out = 0
    if hasattr(resp, "usage") and resp.usage:
        tokens_in = getattr(resp.usage, "tokens", {}).get("input_tokens", 0) if isinstance(getattr(resp.usage, "tokens", None), dict) else 0
        tokens_out = getattr(resp.usage, "tokens", {}).get("output_tokens", 0) if isinstance(getattr(resp.usage, "tokens", None), dict) else 0

    return CompletionResponse(
        request_id=req.request_id,
        content=content,
        finish_reason="stop",
        model_used=req.model_id,
        provider="cohere",
        tokens_in=tokens_in,
        tokens_out=tokens_out,
        latency_ms=latency_ms,
    )


async def stream(req: CompletionRequest) -> AsyncIterator[dict[str, Any]]:
    client = _build_client()
    messages = _convert_messages(req)
    t0 = time.monotonic()

    params: dict[str, Any] = {
        "model": req.model_id,
        "messages": messages,
        "temperature": req.temperature,
    }

    response = client.chat_stream(**params)

    async for event in response:
        event_type = getattr(event, "type", "")

        if event_type == "content-delta":
            delta = event.delta
            if hasattr(delta, "message") and delta.message:
                text = ""
                if hasattr(delta.message, "content") and delta.message.content:
                    text = delta.message.content.text if hasattr(delta.message.content, "text") else str(delta.message.content)
                if text:
                    yield {"type": "content", "content": text}

        elif event_type == "stream-end":
            latency_ms = int((time.monotonic() - t0) * 1000)
            yield {
                "type": "done",
                "metadata": {
                    "model_used": req.model_id,
                    "finish_reason": "stop",
                    "latency_ms": latency_ms,
                },
            }
