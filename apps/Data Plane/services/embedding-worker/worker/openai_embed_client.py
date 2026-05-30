"""
Azure OpenAI Embedding Client — wraps the openai SDK for async batch embedding.

Model:    text-embedding-3-large  (3072-dim, dense)
API:      Azure OpenAI Embeddings v1

Input type is implicit in this model; we use the same client for both
document indexing and query embedding.  All calls are async-native.
"""
from __future__ import annotations

import logging
from typing import List

from openai import AsyncAzureOpenAI

from worker.config import settings

logger = logging.getLogger(__name__)

_client: AsyncAzureOpenAI | None = None


def get_client() -> AsyncAzureOpenAI:
    global _client
    if _client is None:
        _client = AsyncAzureOpenAI(
            api_key=settings.azure_openai_api_key,
            azure_endpoint=settings.azure_openai_endpoint,
            api_version=settings.azure_openai_api_version,
        )
    return _client


# ── Public API ────────────────────────────────────────────────────────────────

async def embed_texts(
    texts: List[str],
    input_type: str = "search_document",  # kept for API compatibility; unused by OpenAI
) -> List[List[float]]:
    """
    Embed a batch of texts using Azure OpenAI text-embedding-3-large.
    Returns a list of float vectors (3072-dim), one per input text.

    The `input_type` parameter is accepted for drop-in compatibility with the
    previous Cohere client but has no effect — OpenAI does not distinguish
    document vs query input at embed time.
    """
    response = await get_client().embeddings.create(
        input=texts,
        model=settings.azure_openai_embedding_deployment,
    )
    vectors = [item.embedding for item in response.data]
    logger.debug(
        "azure openai embedded %d texts model=%s dim=%d",
        len(texts),
        settings.azure_openai_embedding_deployment,
        len(vectors[0]) if vectors else 0,
    )
    return vectors
