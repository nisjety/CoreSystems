"""Azure Translator Text v3 provider."""

from __future__ import annotations

import logging
import time
import uuid
from typing import Any, AsyncIterator

import httpx

from reasoning_runtime.config import get_config
from reasoning_runtime.domain import CompletionRequest, CompletionResponse

logger = logging.getLogger(__name__)

_TRANSLATE_ENDPOINT = "https://api.cognitive.microsofttranslator.com"


def _extract_params(req: CompletionRequest) -> tuple[str, str, str]:
    """Extract text, source language, and target language from the request."""
    text = ""
    target_lang = "en"
    source_lang = ""

    for m in reversed(req.messages):
        if m.role == "user" and isinstance(m.content, str):
            text = m.content
            break

    # model_id encodes "source-target" e.g. "no-en"
    if "-" in req.model_id:
        parts = req.model_id.split("-", 1)
        source_lang = parts[0]
        target_lang = parts[1]
    elif req.model_id and req.model_id != "default":
        target_lang = req.model_id

    return text, source_lang, target_lang


async def complete(req: CompletionRequest) -> CompletionResponse:
    cfg = get_config()
    text, source_lang, target_lang = _extract_params(req)
    t0 = time.monotonic()

    params: dict[str, str] = {
        "api-version": "3.0",
        "to": target_lang,
    }
    if source_lang:
        params["from"] = source_lang

    headers = {
        "Ocp-Apim-Subscription-Key": cfg.azure_translator_key,
        "Content-Type": "application/json",
        "X-ClientTraceId": str(uuid.uuid4()),
    }
    if cfg.azure_translator_region:
        headers["Ocp-Apim-Subscription-Region"] = cfg.azure_translator_region

    body = [{"Text": text}]

    async with httpx.AsyncClient() as client:
        resp = await client.post(
            f"{_TRANSLATE_ENDPOINT}/translate",
            params=params,
            headers=headers,
            json=body,
        )
        resp.raise_for_status()

    result = resp.json()
    translated = ""
    if result and isinstance(result, list) and result[0].get("translations"):
        translated = result[0]["translations"][0].get("text", "")

    latency_ms = int((time.monotonic() - t0) * 1000)

    return CompletionResponse(
        request_id=req.request_id,
        content=translated,
        finish_reason="stop",
        model_used=f"translator-{target_lang}",
        provider="azure_translator",
        latency_ms=latency_ms,
        metadata={"type": "translation", "target_language": target_lang},
    )


async def stream(req: CompletionRequest) -> AsyncIterator[dict[str, Any]]:
    """Translation is not streamable — yield single result."""
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
