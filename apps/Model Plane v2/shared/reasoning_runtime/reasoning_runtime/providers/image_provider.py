"""Image generation provider (DALL-E 3, Azure, Google Gemini)."""

from __future__ import annotations

import base64
import logging
import time
from typing import Any, AsyncIterator

import openai

from reasoning_runtime.config import get_config
from reasoning_runtime.domain import CompletionRequest, CompletionResponse, Provider

logger = logging.getLogger(__name__)


def _build_openai_client(req: CompletionRequest):
    cfg = get_config()
    if req.provider == Provider.AZURE_OPENAI or cfg.azure_openai_endpoint:
        return openai.AsyncAzureOpenAI(
            azure_endpoint=req.api_endpoint or cfg.azure_openai_endpoint,
            api_key=cfg.azure_openai_api_key or cfg.openai_api_key,
            api_version=cfg.azure_openai_api_version,
        )
    return openai.AsyncOpenAI(api_key=cfg.openai_api_key)


def _extract_prompt(req: CompletionRequest) -> str:
    for m in reversed(req.messages):
        if m.role == "user" and isinstance(m.content, str):
            return m.content
    return ""


async def _generate_dalle(
    req: CompletionRequest,
) -> CompletionResponse:
    client = _build_openai_client(req)
    prompt = _extract_prompt(req)
    t0 = time.monotonic()

    model = req.model_id if req.model_id != "default" else "dall-e-3"
    resp = await client.images.generate(
        model=model,
        prompt=prompt,
        n=1,
        size="1024x1024",
        response_format="url",
    )

    latency_ms = int((time.monotonic() - t0) * 1000)
    url = resp.data[0].url or ""

    return CompletionResponse(
        request_id=req.request_id,
        content=url,
        finish_reason="stop",
        model_used=model,
        provider=req.provider.value,
        latency_ms=latency_ms,
        metadata={"type": "image_url"},
    )


async def _generate_gemini(
    req: CompletionRequest,
) -> CompletionResponse:
    from google import genai

    cfg = get_config()
    client = genai.Client(api_key=cfg.google_api_key)
    prompt = _extract_prompt(req)
    t0 = time.monotonic()

    model = req.model_id if req.model_id != "default" else "gemini-2.0-flash-exp"
    resp = client.models.generate_content(
        model=model,
        contents=prompt,
        config={"response_modalities": ["IMAGE"]},
    )

    latency_ms = int((time.monotonic() - t0) * 1000)
    image_data = ""
    if resp.candidates and resp.candidates[0].content and resp.candidates[0].content.parts:
        for part in resp.candidates[0].content.parts:
            if hasattr(part, "inline_data") and part.inline_data:
                image_data = base64.b64encode(part.inline_data.data).decode("utf-8")
                break

    return CompletionResponse(
        request_id=req.request_id,
        content=image_data,
        finish_reason="stop",
        model_used=model,
        provider="gemini",
        latency_ms=latency_ms,
        metadata={"type": "image_base64", "mime_type": "image/png"},
    )


async def complete(req: CompletionRequest) -> CompletionResponse:
    if req.provider == Provider.GEMINI:
        return await _generate_gemini(req)
    return await _generate_dalle(req)


async def stream(req: CompletionRequest) -> AsyncIterator[dict[str, Any]]:
    """Image generation is not streamable — yield single result."""
    resp = await complete(req)
    yield {"type": "content", "content": resp.content}
    yield {
        "type": "done",
        "metadata": {
            "model_used": resp.model_used,
            "finish_reason": "stop",
            "latency_ms": resp.latency_ms,
            **(resp.metadata or {}),
        },
    }
