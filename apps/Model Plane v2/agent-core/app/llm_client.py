"""LLM client — model execution via shared reasoning_runtime (direct, no HTTP hop).

Replaces the previous HTTP-based client that called llm-worker.
Now calls reasoning_runtime.execute() directly, but preserves the same
dict return format so VelionModel and all callers remain unchanged.
"""

from __future__ import annotations

import logging
from typing import Any

from reasoning_runtime import execute
from reasoning_runtime.domain import (
    CompletionRequest,
    Message,
    Provider,
    ToolDefinition,
)

from app.config import settings
from app.domain import CacheSafeParams

logger = logging.getLogger(__name__)


class LLMClient:
    """Direct LLM client via reasoning_runtime — same interface as the old HTTP client.

    Keeps dict-based return values for backward compatibility with
    VelionModel._parse_llm_response() and AgentService callers.
    """

    def __init__(self) -> None:
        self.last_response_data: dict[str, Any] | None = None

    async def open(self) -> None:
        """No-op — reasoning_runtime is configured via configure() in lifespan."""

    async def close(self) -> None:
        """No-op — runtime lifecycle managed centrally."""

    # ---- Full completion (model already selected) ----

    async def complete(
        self,
        *,
        request_id: str,
        org_id: str,
        model_id: str,
        provider: str,
        messages: list[dict[str, Any]],
        api_endpoint: str = "",
        tools: list[dict[str, Any]] | None = None,
        temperature: float = 0.7,
        max_tokens: int | None = None,
        session_id: str = "",
        run_id: str = "",
    ) -> dict[str, Any]:
        """Execute an LLM completion via reasoning_runtime.execute()."""
        req = CompletionRequest(
            request_id=request_id,
            model_id=model_id,
            provider=_resolve_provider(provider),
            api_endpoint=api_endpoint or "",
            messages=_convert_messages(messages),
            temperature=temperature,
            max_tokens=max_tokens,
            tools=_convert_tools(tools) if tools else None,
            stream=False,
            org_id=org_id,
            session_id=session_id,
            run_id=run_id or request_id,
        )

        result = await execute(req)

        # Build dict matching the old llm-worker JSON response format
        data = _completion_to_dict(result)
        self.last_response_data = data
        return data

    # ---- Convenience: planner completion (select model + execute) ----

    async def planner_complete(
        self,
        messages: list[dict[str, str]],
        model: str | None = None,
        temperature: float | None = None,
        cache_params: CacheSafeParams | None = None,
    ) -> str:
        """Quick LLM call for the agent planner."""
        from uuid import uuid4

        resolved_model = model or settings.planner_model

        cache_control: dict[str, str] | None = None
        if cache_params and cache_params.enabled:
            if resolved_model not in cache_params.disabled_for_models:
                cc: dict[str, str] = {"type": cache_params.cache_type}
                if cache_params.ttl:
                    cc["ttl"] = cache_params.ttl
                if cache_params.scope:
                    cc["scope"] = cache_params.scope
                cache_control = cc

        req = CompletionRequest(
            request_id=uuid4().hex,
            model_id=resolved_model,
            provider=_resolve_provider(settings.planner_provider),
            api_endpoint="",
            messages=_convert_messages(messages),
            temperature=temperature if temperature is not None else settings.planner_temperature,
            stream=False,
            org_id="system",
            run_id=uuid4().hex,
            cache_control=cache_control,
        )

        result = await execute(req)
        data = _completion_to_dict(result)
        self.last_response_data = data
        return data.get("content", "")


# ── Internal helpers ────────────────────────────────────────────────────


def _resolve_provider(name: str) -> Provider:
    """Map provider name string to Provider enum."""
    try:
        return Provider(name)
    except ValueError:
        # Fallback: try common aliases
        _aliases = {
            "openai": Provider.OPENAI,
            "anthropic": Provider.ANTHROPIC,
            "google": Provider.GEMINI,
            "gemini": Provider.GEMINI,
            "cohere": Provider.COHERE,
            "mistral": Provider.MISTRAL,
            "ollama": Provider.OLLAMA,
            "azure_openai": Provider.AZURE_OPENAI,
            "azure-openai": Provider.AZURE_OPENAI,
        }
        return _aliases.get(name.lower(), Provider.OPENAI)


def _convert_messages(msgs: list[dict[str, Any]]) -> list[Message]:
    """Convert OpenAI-format message dicts to reasoning_runtime Message objects."""
    return [
        Message(
            role=m.get("role", "user"),
            content=m.get("content", ""),
            tool_call_id=m.get("tool_call_id"),
            tool_calls=m.get("tool_calls"),
        )
        for m in msgs
    ]


def _convert_tools(tools: list[dict[str, Any]]) -> list[ToolDefinition]:
    """Convert OpenAI function-calling tool dicts to ToolDefinition objects."""
    result: list[ToolDefinition] = []
    for t in tools:
        fn = t.get("function", t)
        result.append(ToolDefinition(
            name=fn.get("name", ""),
            description=fn.get("description", ""),
            parameters=fn.get("parameters", {}),
        ))
    return result


def _completion_to_dict(result: Any) -> dict[str, Any]:
    """Convert a CompletionResponse to the dict format callers expect."""
    data: dict[str, Any] = {
        "content": result.content or "",
        "model": result.model_used,
        "provider": result.provider,
        "tokens_in": result.tokens_in,
        "tokens_out": result.tokens_out,
        "finish_reason": result.finish_reason,
    }

    # Tool calls: convert back to OpenAI format (list of dicts)
    if result.tool_calls:
        data["tool_calls"] = [
            {
                "id": tc.id,
                "type": "function",
                "function": {
                    "name": tc.name,
                    "arguments": tc.arguments,
                },
            }
            for tc in result.tool_calls
        ]
    else:
        data["tool_calls"] = []

    return data
