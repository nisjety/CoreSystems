"""Mistral AI provider."""

from __future__ import annotations

import logging
import time
from typing import Any, AsyncIterator

from mistralai import Mistral

from reasoning_runtime.config import get_config
from reasoning_runtime.domain import (
    CompletionRequest,
    CompletionResponse,
    ToolCallBlock,
)

logger = logging.getLogger(__name__)


def _build_client() -> Mistral:
    cfg = get_config()
    return Mistral(api_key=cfg.mistral_api_key)


def _build_params(req: CompletionRequest) -> dict[str, Any]:
    params: dict[str, Any] = {
        "model": req.model_id,
        "messages": [m.model_dump(exclude_none=True) for m in req.messages],
        "temperature": req.temperature,
    }
    if req.max_tokens:
        params["max_tokens"] = req.max_tokens
    if req.top_p is not None:
        params["top_p"] = req.top_p
    if req.tools:
        params["tools"] = [t.model_dump() for t in req.tools]
    return params


def _extract_tool_calls(raw) -> list[ToolCallBlock]:
    if not raw:
        return []
    result: list[ToolCallBlock] = []
    for tc in raw:
        fn = tc.function if hasattr(tc, "function") else tc.get("function", {})
        name = fn.name if hasattr(fn, "name") else fn.get("name", "")
        args = fn.arguments if hasattr(fn, "arguments") else fn.get("arguments", "{}")
        tc_id = tc.id if hasattr(tc, "id") else tc.get("id", "")
        result.append(ToolCallBlock(id=tc_id, name=name, arguments=args))
    return result


async def complete(req: CompletionRequest) -> CompletionResponse:
    client = _build_client()
    params = _build_params(req)
    t0 = time.monotonic()

    resp = await client.chat.complete_async(**params)
    latency_ms = int((time.monotonic() - t0) * 1000)

    choice = resp.choices[0]
    usage = resp.usage

    return CompletionResponse(
        request_id=req.request_id,
        content=choice.message.content or "",
        tool_calls=_extract_tool_calls(getattr(choice.message, "tool_calls", None)),
        finish_reason=choice.finish_reason or "stop",
        model_used=resp.model or req.model_id,
        provider="mistral",
        tokens_in=usage.prompt_tokens if usage else 0,
        tokens_out=usage.completion_tokens if usage else 0,
        latency_ms=latency_ms,
    )


async def stream(req: CompletionRequest) -> AsyncIterator[dict[str, Any]]:
    client = _build_client()
    params = _build_params(req)
    t0 = time.monotonic()

    response = await client.chat.stream_async(**params)

    tool_buffers: dict[int, dict[str, str]] = {}

    async for event in response:
        chunk = event.data
        if not chunk.choices:
            continue

        delta = chunk.choices[0].delta

        if delta.content:
            yield {"type": "content", "content": delta.content}

        if delta.tool_calls:
            for tc_delta in delta.tool_calls:
                idx = tc_delta.index if hasattr(tc_delta, "index") else 0
                if idx not in tool_buffers:
                    tool_buffers[idx] = {"id": "", "name": "", "arguments": ""}
                if hasattr(tc_delta, "id") and tc_delta.id:
                    tool_buffers[idx]["id"] = tc_delta.id
                if tc_delta.function:
                    if tc_delta.function.name:
                        tool_buffers[idx]["name"] = tc_delta.function.name
                    if tc_delta.function.arguments:
                        tool_buffers[idx]["arguments"] += tc_delta.function.arguments

        if chunk.choices[0].finish_reason:
            for buf in tool_buffers.values():
                yield {"type": "tool_call", "tool_call": buf}

            latency_ms = int((time.monotonic() - t0) * 1000)
            yield {
                "type": "done",
                "metadata": {
                    "model_used": chunk.model or req.model_id,
                    "finish_reason": chunk.choices[0].finish_reason,
                    "latency_ms": latency_ms,
                },
            }
