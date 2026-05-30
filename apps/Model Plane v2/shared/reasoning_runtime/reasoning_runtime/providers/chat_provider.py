"""Multi-provider chat dispatcher.

Routes a CompletionRequest to the correct provider module based on req.provider.

Public API
----------
async def complete(req: CompletionRequest) -> CompletionResponse
    Non-streaming completion — returns a single CompletionResponse.

async def stream(req: CompletionRequest) -> AsyncIterator[StreamChunk]
    Streaming completion — yields StreamChunk objects consumed by the gRPC servicer.
"""

from __future__ import annotations

import logging
from typing import TYPE_CHECKING, AsyncIterator

from reasoning_runtime.domain import CompletionRequest, CompletionResponse, Provider, StreamChunk

if TYPE_CHECKING:
    from types import ModuleType

logger = logging.getLogger(__name__)

# ── Provider dispatch ────────────────────────────────────────────────────────


def _get_provider_module(provider: Provider) -> "ModuleType":
    """Lazy-import and return the provider module for *provider*."""
    if provider in (Provider.OPENAI, Provider.AZURE_OPENAI):
        from reasoning_runtime.providers import openai_provider as p  # type: ignore[import]
    elif provider == Provider.ANTHROPIC:
        from reasoning_runtime.providers import anthropic_provider as p  # type: ignore[import]
    elif provider == Provider.GEMINI:
        from reasoning_runtime.providers import gemini_provider as p  # type: ignore[import]
    elif provider == Provider.COHERE:
        from reasoning_runtime.providers import cohere_provider as p  # type: ignore[import]
    elif provider == Provider.MISTRAL:
        from reasoning_runtime.providers import mistral_provider as p  # type: ignore[import]
    elif provider == Provider.OLLAMA:
        from reasoning_runtime.providers import ollama_provider as p  # type: ignore[import]
    else:
        raise ValueError(f"Unsupported provider: {provider!r}")
    return p  # type: ignore[return-value]


# ── Public functions ─────────────────────────────────────────────────────────


async def complete(req: CompletionRequest) -> CompletionResponse:
    """Route a non-streaming completion request to the appropriate provider."""
    provider_module = _get_provider_module(req.provider)
    try:
        return await provider_module.complete(req)
    except Exception:
        logger.exception(
            "chat_provider_complete_error provider=%s model=%s",
            req.provider.value,
            req.model_id,
        )
        raise


async def stream(req: CompletionRequest) -> AsyncIterator[StreamChunk]:
    """Route a streaming completion request and yield normalized StreamChunk objects.

    Converts the provider-specific dict format:
        {"type": "content", "content": str}
        {"type": "tool_call", "tool_call": {...}}
        {"type": "done", "metadata": {"model_used": str, ...}}
    into StreamChunk(delta, done, model_used) objects for the gRPC servicer.
    """
    provider_module = _get_provider_module(req.provider)
    model_used: str = req.model_id

    try:
        async for raw in provider_module.stream(req):
            chunk_type = raw.get("type", "") if isinstance(raw, dict) else ""

            if chunk_type == "content":
                yield StreamChunk(delta=raw.get("content", ""), done=False, model_used=model_used)

            elif chunk_type == "done":
                meta = raw.get("metadata", {}) or {}
                model_used = meta.get("model_used", model_used)
                yield StreamChunk(delta="", done=True, model_used=model_used)

            # tool_call chunks are silently dropped here; the servicer only handles
            # text deltas via the streaming RPC.  Tool use is handled in the unary path.

    except Exception:
        logger.exception(
            "chat_provider_stream_error provider=%s model=%s",
            req.provider.value,
            req.model_id,
        )
        raise
