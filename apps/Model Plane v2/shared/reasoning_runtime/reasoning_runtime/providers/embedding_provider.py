"""OpenAI-compatible embedding provider.

Delegates to the OpenAI Embeddings API (or Azure OpenAI when configured).
The result vector is returned in CompletionResponse.metadata["vector"].
"""

from __future__ import annotations

import logging

import openai

from reasoning_runtime.config import get_config
from reasoning_runtime.domain import CompletionRequest, CompletionResponse, Provider

logger = logging.getLogger(__name__)


def _build_client(req: CompletionRequest) -> openai.AsyncOpenAI:
    cfg = get_config()
    if req.provider == Provider.AZURE_OPENAI:
        return openai.AsyncAzureOpenAI(
            api_key=cfg.azure_openai_api_key,
            azure_endpoint=req.api_endpoint or cfg.azure_openai_endpoint,
            api_version=getattr(cfg, "azure_openai_api_version", "2024-02-01"),
        )
    return openai.AsyncOpenAI(api_key=cfg.openai_api_key)


async def complete(req: CompletionRequest) -> CompletionResponse:
    """Return an embedding vector for the first message's content."""
    text = req.messages[0].content if req.messages else ""
    if isinstance(text, list):
        # Flatten content-block lists to plain text
        text = " ".join(
            b.get("text", "") if isinstance(b, dict) else str(b) for b in text
        )

    client = _build_client(req)
    try:
        resp = await client.embeddings.create(
            model=req.model_id,
            input=text,
        )
        vector = resp.data[0].embedding if resp.data else []
        return CompletionResponse(
            request_id=req.request_id,
            content="",
            model_used=resp.model or req.model_id,
            provider=req.provider.value,
            tokens_in=resp.usage.prompt_tokens if resp.usage else 0,
            metadata={"vector": vector},
        )
    except Exception:
        logger.exception(
            "embedding_provider_error model=%s provider=%s",
            req.model_id,
            req.provider.value,
        )
        raise
