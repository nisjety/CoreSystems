"""OpenAI + Azure OpenAI provider."""

from __future__ import annotations

import json
import logging
import time
from typing import Any, AsyncIterator

import openai

from reasoning_runtime.config import get_config
from reasoning_runtime.domain import (
    CompletionRequest,
    CompletionResponse,
    Provider,
    ToolCallBlock,
)

logger = logging.getLogger(__name__)


def _build_client(req: CompletionRequest) -> openai.AsyncOpenAI:
    cfg = get_config()

    if req.provider == Provider.AZURE_OPENAI or cfg.azure_openai_endpoint:
        return openai.AsyncAzureOpenAI(
            azure_endpoint=req.api_endpoint or cfg.azure_openai_endpoint,
            api_key=cfg.azure_openai_api_key or cfg.openai_api_key,
            api_version=cfg.azure_openai_api_version,
        )

    return openai.AsyncOpenAI(
        api_key=cfg.openai_api_key,
        base_url=req.api_endpoint or None,
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
    if req.tools:
        params["tools"] = [t.model_dump() for t in req.tools]
    if req.tool_choice is not None:
        params["tool_choice"] = req.tool_choice
    return params


def _extract_tool_calls(
    raw: list | None,
) -> list[ToolCallBlock]:
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
        tool_calls=_extract_tool_calls(
            getattr(choice.message, "tool_calls", None)
        ),
        finish_reason=choice.finish_reason or "stop",
        model_used=resp.model or req.model_id,
        provider=req.provider.value,
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

    tool_call_buffers: dict[int, dict[str, str]] = {}

    async for chunk in response:
        if not chunk.choices:
            continue

        delta = chunk.choices[0].delta

        # Content
        if delta.content:
            yield {"type": "content", "content": delta.content}

        # Tool calls (streamed incrementally)
        if delta.tool_calls:
            for tc_delta in delta.tool_calls:
                idx = tc_delta.index
                if idx not in tool_call_buffers:
                    tool_call_buffers[idx] = {
                        "id": tc_delta.id or "",
                        "name": "",
                        "arguments": "",
                    }
                if tc_delta.function:
                    if tc_delta.function.name:
                        tool_call_buffers[idx]["name"] = tc_delta.function.name
                    if tc_delta.function.arguments:
                        tool_call_buffers[idx]["arguments"] += tc_delta.function.arguments

        # Finish
        if chunk.choices[0].finish_reason:
            # Emit accumulated tool calls
            for buf in tool_call_buffers.values():
                yield {
                    "type": "tool_call",
                    "tool_call": buf,
                }

            latency_ms = int((time.monotonic() - t0) * 1000)
            yield {
                "type": "done",
                "metadata": {
                    "model_used": chunk.model or req.model_id,
                    "finish_reason": chunk.choices[0].finish_reason,
                    "latency_ms": latency_ms,
                },
            }
