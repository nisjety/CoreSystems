"""Anthropic Claude provider."""

from __future__ import annotations

import json
import logging
import time
from typing import Any, AsyncIterator

import anthropic

from reasoning_runtime.config import get_config
from reasoning_runtime.domain import (
    CompletionRequest,
    CompletionResponse,
    ToolCallBlock,
)

logger = logging.getLogger(__name__)


def _build_client() -> anthropic.AsyncAnthropic:
    cfg = get_config()
    return anthropic.AsyncAnthropic(api_key=cfg.anthropic_api_key)


def _convert_messages(
    req: CompletionRequest,
) -> tuple[str, list[dict[str, Any]]]:
    """Extract system message and convert to Anthropic format."""
    system = ""
    messages: list[dict[str, Any]] = []

    for m in req.messages:
        if m.role == "system":
            system = m.content if isinstance(m.content, str) else str(m.content)
        else:
            messages.append({"role": m.role, "content": m.content})

    return system, messages


def _convert_tools(req: CompletionRequest) -> list[dict[str, Any]] | None:
    """Convert OpenAI-format tools to Anthropic format."""
    if not req.tools:
        return None

    result: list[dict[str, Any]] = []
    for t in req.tools:
        fn = t.function
        result.append({
            "name": fn.get("name", ""),
            "description": fn.get("description", ""),
            "input_schema": fn.get("parameters", {}),
        })
    return result


def _extract_tool_calls(content: list) -> list[ToolCallBlock]:
    result: list[ToolCallBlock] = []
    for block in content:
        if getattr(block, "type", None) == "tool_use":
            result.append(ToolCallBlock(
                id=block.id,
                name=block.name,
                arguments=json.dumps(block.input) if isinstance(block.input, dict) else str(block.input),
            ))
    return result


async def complete(req: CompletionRequest) -> CompletionResponse:
    client = _build_client()
    system, messages = _convert_messages(req)
    t0 = time.monotonic()

    params: dict[str, Any] = {
        "model": req.model_id,
        "messages": messages,
        "max_tokens": req.max_tokens or 4096,
        "temperature": req.temperature,
    }
    if system:
        params["system"] = system
    if req.top_p is not None:
        params["top_p"] = req.top_p
    if req.stop:
        params["stop_sequences"] = req.stop

    tools = _convert_tools(req)
    if tools:
        params["tools"] = tools

    resp = await client.messages.create(**params)
    latency_ms = int((time.monotonic() - t0) * 1000)

    content_text = ""
    for block in resp.content:
        if getattr(block, "type", None) == "text":
            content_text += block.text

    return CompletionResponse(
        request_id=req.request_id,
        content=content_text,
        tool_calls=_extract_tool_calls(resp.content),
        finish_reason=resp.stop_reason or "end_turn",
        model_used=resp.model,
        provider="anthropic",
        tokens_in=resp.usage.input_tokens,
        tokens_out=resp.usage.output_tokens,
        latency_ms=latency_ms,
    )


async def stream(req: CompletionRequest) -> AsyncIterator[dict[str, Any]]:
    client = _build_client()
    system, messages = _convert_messages(req)
    t0 = time.monotonic()

    params: dict[str, Any] = {
        "model": req.model_id,
        "messages": messages,
        "max_tokens": req.max_tokens or 4096,
        "temperature": req.temperature,
    }
    if system:
        params["system"] = system

    tools = _convert_tools(req)
    if tools:
        params["tools"] = tools

    tool_buffers: dict[str, dict[str, Any]] = {}

    async with client.messages.stream(**params) as stream_resp:
        async for event in stream_resp:
            event_type = getattr(event, "type", "")

            if event_type == "content_block_delta":
                delta = event.delta
                if getattr(delta, "type", "") == "text_delta":
                    yield {"type": "content", "content": delta.text}
                elif getattr(delta, "type", "") == "input_json_delta":
                    idx = str(event.index)
                    if idx in tool_buffers:
                        tool_buffers[idx]["arguments"] += delta.partial_json

            elif event_type == "content_block_start":
                block = event.content_block
                if getattr(block, "type", "") == "tool_use":
                    tool_buffers[str(event.index)] = {
                        "id": block.id,
                        "name": block.name,
                        "arguments": "",
                    }

            elif event_type == "message_stop":
                for buf in tool_buffers.values():
                    yield {"type": "tool_call", "tool_call": buf}

                latency_ms = int((time.monotonic() - t0) * 1000)
                yield {
                    "type": "done",
                    "metadata": {
                        "model_used": req.model_id,
                        "finish_reason": "end_turn",
                        "latency_ms": latency_ms,
                    },
                }
