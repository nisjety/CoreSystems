"""Executor — dispatches CompletionRequest to the correct provider."""

from __future__ import annotations

import logging
import traceback
from typing import Any, AsyncIterator

from reasoning_runtime.domain import CompletionRequest, CompletionResponse, Provider
from reasoning_runtime.providers import (
    anthropic_provider,
    cohere_provider,
    gemini_provider,
    mistral_provider,
    ollama_provider,
    openai_provider,
)

logger = logging.getLogger(__name__)

_PROVIDERS = {
    Provider.OPENAI: openai_provider,
    Provider.AZURE_OPENAI: openai_provider,
    Provider.ANTHROPIC: anthropic_provider,
    Provider.GEMINI: gemini_provider,
    Provider.COHERE: cohere_provider,
    Provider.MISTRAL: mistral_provider,
    Provider.OLLAMA: ollama_provider,
}


def _resolve_provider(req: CompletionRequest):
    mod = _PROVIDERS.get(req.provider)
    if mod is None:
        raise ValueError(f"Unsupported provider: {req.provider}")
    return mod


async def execute(req: CompletionRequest) -> CompletionResponse:
    """Run a non-streaming completion and return the unified response."""
    mod = _resolve_provider(req)
    try:
        return await mod.complete(req)
    except Exception:
        logger.error(
            "completion_error provider=%s model=%s\n%s",
            req.provider.value,
            req.model_id,
            traceback.format_exc(),
        )
        raise


async def execute_stream(
    req: CompletionRequest,
) -> AsyncIterator[dict[str, Any]]:
    """Run a streaming completion and yield raw chunk dicts."""
    mod = _resolve_provider(req)
    try:
        async for chunk in mod.stream(req):
            yield chunk
    except Exception:
        logger.error(
            "stream_error provider=%s model=%s\n%s",
            req.provider.value,
            req.model_id,
            traceback.format_exc(),
        )
        yield {"type": "error", "error": "Internal provider error"}
