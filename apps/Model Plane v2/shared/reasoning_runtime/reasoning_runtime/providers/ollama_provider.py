"""Ollama provider (OpenAI-compatible endpoint)."""

from __future__ import annotations

import logging
import time
from typing import Any, AsyncIterator

import openai

from reasoning_runtime.config import get_config
from reasoning_runtime.domain import CompletionRequest, CompletionResponse

logger = logging.getLogger(__name__)

_DEFAULT_OLLAMA_BASE = "http://ollama:11434/v1"


def _build_client(req: CompletionRequest) -> openai.AsyncOpenAI:
    cfg = get_config()
    base_url = req.api_endpoint or cfg.ollama_base_url or _DEFAULT_OLLAMA_BASE
    return openai.AsyncOpenAI(
        api_key="ollama",  # Ollama doesn't need a real key
        base_url=base_url,
    )


def _build_params(req: CompletionRequest) -> dict[str, Any]:
    params: dict[str, Any] = {
        "model": req.model_id,
        "messages": [m.model_dump(exclude_none=True) for m in req.messages],
        "temperature": req.temperature,
    }
    if req.max_tokens is not None:
        params["max_tokens"] = req.max_tokens
    if req.top_p is not None:
        params["top_p"] = req.top_p
    if req.stop:
        params["stop"] = req.stop
    return params


async def complete(req: CompletionRequest) -> CompletionResponse:
    client = _build_client(req)
    params = _build_params(req)
    t0 = time.monotonic()

    resp = await client.chat.completions.create(**params)
    latency_ms = int((time.monotonic() - t0) * 1000)

    choice = resp.choices[0]
    usage = resp.usage

    return CompletionResponse(
        request_id=req.request_id,
        content=choice.message.content or "",
        finish_reason=choice.finish_reason or "stop",
        model_used=resp.model or req.model_id,
        provider="ollama",
        tokens_in=usage.prompt_tokens if usage else 0,
        tokens_out=usage.completion_tokens if usage else 0,
        latency_ms=latency_ms,
    )


async def stream(req: CompletionRequest) -> AsyncIterator[dict[str, Any]]:
    client = _build_client(req)
    params = _build_params(req)
    params["stream"] = True
    t0 = time.monotonic()

    response = await client.chat.completions.create(**params)

    async for chunk in response:
        if not chunk.choices:
            continue

        delta = chunk.choices[0].delta

        if delta.content:
            yield {"type": "content", "content": delta.content}

        if chunk.choices[0].finish_reason:
            latency_ms = int((time.monotonic() - t0) * 1000)
            yield {
                "type": "done",
                "metadata": {
                    "model_used": chunk.model or req.model_id,
                    "finish_reason": chunk.choices[0].finish_reason,
                    "latency_ms": latency_ms,
                },
            }
