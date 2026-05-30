"""Translation provider — LLM-based translation via gpt-4o-mini.

Phase 4.4: uses the existing Azure OpenAI deployment for translation instead of
the separate Azure Translator Text API (which requires a distinct subscription key).
"""
from __future__ import annotations

import logging
import uuid
from typing import Any

from app.config import get_settings

logger = logging.getLogger(__name__)


async def close() -> None:
    """No-op: kept for API compatibility."""


async def translate(
    text: str,
    *,
    source_language: str | None = None,
    target_language: str = "no",
) -> dict[str, Any]:
    """Translate text using the LLM (gpt-4o-mini via Azure OpenAI).

    Returns:
        {"translated_text": str, "source_language": str, "target_language": str}
    """
    from app.executor import execute
    from reasoning_runtime.domain import CompletionRequest, Provider

    settings = get_settings()
    if not settings.azure_openai_api_key:
        raise RuntimeError("Azure OpenAI API key not configured")

    src_note = f"from {source_language} " if source_language else ""
    prompt = (
        f"Translate the following text {src_note}to {target_language}. "
        "Output only the translation, nothing else:\n\n"
        f"{text}"
    )
    req = CompletionRequest(
        request_id=str(uuid.uuid4()),
        org_id="internal",
        model_id="gpt-4o-mini",
        provider=Provider.AZURE_OPENAI,
        messages=[{"role": "user", "content": prompt}],
        api_endpoint=settings.azure_openai_endpoint,
    )
    result = await execute(req)
    translated = (result.content or "").strip()
    logger.info(
        "translate %s->%s chars=%d->%d",
        source_language or "auto",
        target_language,
        len(text),
        len(translated),
    )
    return {
        "translated_text": translated,
        "source_language": source_language or "auto",
        "target_language": target_language,
    }
