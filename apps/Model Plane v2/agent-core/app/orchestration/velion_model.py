"""VelionModel — PydanticAI Model backed by the v2 LLMClient → llm-worker.

Instead of calling OpenAI/Anthropic directly, this Model routes every
request through our llm-worker HTTP service, preserving:
  - Multi-provider routing (openai, anthropic, google, …)
  - Prompt caching
  - Token cost tracking / audit trail
  - Internal API-key auth

Only the synchronous ``request()`` path is implemented for Phase 1.2.
Streaming will be added when the llm-worker SSE endpoint is ready.
"""

from __future__ import annotations

import json
import logging
from contextlib import asynccontextmanager
from typing import Any, AsyncIterator
from uuid import uuid4

from pydantic_ai.messages import (
    ModelMessage,
    ModelRequest,
    ModelResponse,
    RetryPromptPart,
    SystemPromptPart,
    TextPart,
    ToolCallPart,
    ToolReturnPart,
    UserPromptPart,
)
from pydantic_ai.models import Model, ModelRequestParameters, ModelSettings
from pydantic_ai.result import Usage
from pydantic_ai.tools import ToolDefinition as PAToolDefinition

from app.config import settings as app_settings
from app.llm_client import LLMClient

logger = logging.getLogger(__name__)


class VelionModel(Model):
    """Custom PydanticAI Model that delegates to the v2 llm-worker.

    Parameters
    ----------
    llm_client:
        An *opened* ``LLMClient`` instance.
    model_id:
        The LLM model identifier (e.g. ``"gpt-4o-mini"``).
        Defaults to the planner model from config.
    provider:
        The LLM provider name (e.g. ``"openai"``).
        Defaults to the planner provider from config.
    org_id:
        Organisation scope for cost tracking.
    temperature:
        Sampling temperature.  Defaults to config's planner_temperature.
    """

    def __init__(
        self,
        llm_client: LLMClient,
        *,
        model_id: str | None = None,
        provider: str | None = None,
        org_id: str = "system",
        temperature: float | None = None,
    ) -> None:
        super().__init__()
        self._llm = llm_client
        self._model_id = model_id or app_settings.planner_model
        self._provider = provider or app_settings.planner_provider
        self._org_id = org_id
        self._temperature = (
            temperature if temperature is not None else app_settings.planner_temperature
        )

    # ------------------------------------------------------------------
    # Required Model properties
    # ------------------------------------------------------------------

    @property
    def model_name(self) -> str:
        return f"{self._provider}:{self._model_id}"

    @property
    def system(self) -> str:
        return "velion"

    # ------------------------------------------------------------------
    # Core request  (non-streaming)
    # ------------------------------------------------------------------

    async def request(
        self,
        messages: list[ModelMessage],
        model_settings: ModelSettings | None,
        model_request_parameters: ModelRequestParameters,
    ) -> ModelResponse:
        """Send a request to llm-worker and translate the response."""

        oai_messages = _to_openai_messages(messages)

        # Build tools list from PydanticAI's function tool definitions
        tools_payload: list[dict[str, Any]] | None = None
        if model_request_parameters.function_tools:
            tools_payload = [
                _tool_def_to_openai(t)
                for t in model_request_parameters.function_tools
            ]

        # Merge per-call settings
        temperature = self._temperature
        max_tokens: int | None = None
        if model_settings:
            if model_settings.temperature is not None:
                temperature = model_settings.temperature
            if model_settings.max_tokens is not None:
                max_tokens = model_settings.max_tokens

        request_id = uuid4().hex

        resp_data = await self._llm.complete(
            request_id=request_id,
            org_id=self._org_id,
            model_id=self._model_id,
            provider=self._provider,
            messages=oai_messages,
            tools=tools_payload,
            temperature=temperature,
            max_tokens=max_tokens,
        )

        return _parse_llm_response(resp_data, self.model_name)

    # ------------------------------------------------------------------
    # Streaming  (stub — Phase 1.4)
    # ------------------------------------------------------------------

    @asynccontextmanager
    async def request_stream(
        self,
        messages: list[ModelMessage],
        model_settings: ModelSettings | None,
        model_request_parameters: ModelRequestParameters,
    ) -> AsyncIterator[Any]:
        """Streaming is not yet supported; falls back to a full request."""
        response = await self.request(messages, model_settings, model_request_parameters)
        yield _FakeStream(response)


# ======================================================================
# Internal helpers — message format conversion
# ======================================================================


def _to_openai_messages(messages: list[ModelMessage]) -> list[dict[str, Any]]:
    """Convert PydanticAI typed messages → OpenAI chat-completions format."""
    out: list[dict[str, Any]] = []

    for msg in messages:
        if isinstance(msg, ModelRequest):
            _convert_request_parts(msg, out)
        elif isinstance(msg, ModelResponse):
            _convert_response_parts(msg, out)

    return out


def _convert_request_parts(
    req: ModelRequest,
    out: list[dict[str, Any]],
) -> None:
    """Expand a ModelRequest into one or more OpenAI messages."""
    for part in req.parts:
        if isinstance(part, SystemPromptPart):
            out.append({"role": "system", "content": part.content})

        elif isinstance(part, UserPromptPart):
            out.append({"role": "user", "content": part.content})

        elif isinstance(part, ToolReturnPart):
            content = (
                part.content
                if isinstance(part.content, str)
                else json.dumps(part.content, default=str)
            )
            out.append({
                "role": "tool",
                "tool_call_id": part.tool_call_id,
                "content": content,
            })

        elif isinstance(part, RetryPromptPart):
            # Retry prompts are sent as user messages with context
            content = part.content if isinstance(part.content, str) else str(part.content)
            out.append({"role": "user", "content": f"[retry] {content}"})


def _convert_response_parts(
    resp: ModelResponse,
    out: list[dict[str, Any]],
) -> None:
    """Expand a ModelResponse into an OpenAI assistant message."""
    text_parts: list[str] = []
    tool_calls: list[dict[str, Any]] = []

    for part in resp.parts:
        if isinstance(part, TextPart):
            text_parts.append(part.content)
        elif isinstance(part, ToolCallPart):
            args_str = (
                part.args.args_json
                if hasattr(part.args, "args_json")
                else json.dumps(part.args, default=str)
            )
            tool_calls.append({
                "id": part.tool_call_id or uuid4().hex[:8],
                "type": "function",
                "function": {
                    "name": part.tool_name,
                    "arguments": args_str,
                },
            })

    msg: dict[str, Any] = {"role": "assistant"}
    if text_parts:
        msg["content"] = "\n".join(text_parts)
    if tool_calls:
        msg["tool_calls"] = tool_calls
    out.append(msg)


def _tool_def_to_openai(tool: PAToolDefinition) -> dict[str, Any]:
    """Convert a PydanticAI tool definition to OpenAI function-calling format."""
    return {
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.parameters_json_schema,
        },
    }


def _parse_llm_response(data: dict[str, Any], model_name: str) -> ModelResponse:
    """Convert an llm-worker JSON response → PydanticAI ModelResponse."""
    parts: list[TextPart | ToolCallPart] = []

    # Text content
    content = data.get("content")
    if content:
        parts.append(TextPart(content=content))

    # Tool calls (OpenAI format)
    for tc in data.get("tool_calls", []):
        fn = tc.get("function", {})
        tool_name = fn.get("name", "")
        args_raw = fn.get("arguments", "{}")
        tool_call_id = tc.get("id", uuid4().hex[:8])

        parts.append(ToolCallPart(
            tool_name=tool_name,
            args=args_raw,  # PydanticAI accepts raw JSON string
            tool_call_id=tool_call_id,
        ))

    # If the response is a plain string (e.g. from planner_complete wrapper)
    if not parts and isinstance(data, str):
        parts.append(TextPart(content=data))

    # Fallback — empty response safeguard
    if not parts:
        parts.append(TextPart(content=""))

    return ModelResponse(parts=parts, model_name=model_name)


# ======================================================================
# Fake stream adapter (used until real SSE streaming is wired)
# ======================================================================


class _FakeStream:
    """Wraps a completed ModelResponse to satisfy the StreamedResponse interface."""

    def __init__(self, response: ModelResponse) -> None:
        self._response = response
        self._used = False

    def __aiter__(self):
        return self

    async def __anext__(self):
        if self._used:
            raise StopAsyncIteration
        self._used = True
        return self._response

    def get(self) -> ModelResponse:
        return self._response

    def usage(self) -> Usage:
        return Usage()
