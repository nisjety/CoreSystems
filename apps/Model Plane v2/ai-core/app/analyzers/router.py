"""Analyzer router — selects the best backend for a given request.

Selection strategy:
1. Explicit override in request.extra["backend"]
2. Structured prebuilt models (invoice, receipt, etc.) → Document Intelligence
3. Content Understanding enabled + multimodal content → Content Understanding
4. Complex reasoning / general → Mistral Document AI (if available)
5. Fallback → Document Intelligence
"""

from __future__ import annotations

import logging
from typing import Sequence

from app.analyzers.base import (
    AnalyzerBackend,
    AnalyzerBackendType,
    AnalyzerRequest,
    AnalyzerResult,
    DocumentModel,
)
from app.analyzers.content_understanding import ContentUnderstandingAnalyzer
from app.analyzers.document_intelligence import DocumentIntelligenceAnalyzer
from app.analyzers.mistral_document_ai import MistralDocumentAIAnalyzer

logger = logging.getLogger(__name__)

# Models that are best served by Document Intelligence (structured extraction)
_STRUCTURED_MODELS = frozenset({
    DocumentModel.INVOICE,
    DocumentModel.RECEIPT,
    DocumentModel.BUSINESS_CARD,
    DocumentModel.ID_DOCUMENT,
    DocumentModel.FORM,
    DocumentModel.LAYOUT,
    DocumentModel.READ,
})

# Mime types that benefit from multimodal Content Understanding
_MULTIMODAL_MIMES = frozenset({
    "image/png", "image/jpeg", "image/gif", "image/webp", "image/tiff",
    "video/mp4", "video/webm",
    "audio/wav", "audio/mp3", "audio/mpeg",
})


def _all_backends() -> Sequence[AnalyzerBackend]:
    """Instantiate all backend implementations."""
    return (
        DocumentIntelligenceAnalyzer(),
        ContentUnderstandingAnalyzer(),
        MistralDocumentAIAnalyzer(),
    )


def get_analyzer(request: AnalyzerRequest) -> AnalyzerBackend:
    """Select the best analyzer backend for *request*.

    Raises ``RuntimeError`` if no backend is available.
    """
    backends = {b.backend_type: b for b in _all_backends()}

    # 1. Explicit override
    explicit = request.extra.get("backend")
    if explicit:
        try:
            backend_type = AnalyzerBackendType(explicit)
        except ValueError:
            raise ValueError(f"Unknown analyzer backend: {explicit}")
        backend = backends.get(backend_type)
        if backend and backend.is_available():
            return backend
        raise RuntimeError(f"Requested backend {explicit} is not available")

    # 2. Structured prebuilt → Document Intelligence
    if request.model in _STRUCTURED_MODELS:
        di = backends[AnalyzerBackendType.DOCUMENT_INTELLIGENCE]
        if di.is_available():
            return di

    # 3. Multimodal content → Content Understanding
    if request.mime_type in _MULTIMODAL_MIMES:
        cu = backends[AnalyzerBackendType.CONTENT_UNDERSTANDING]
        if cu.is_available():
            return cu

    # 4. General / reasoning → Mistral Document AI
    if request.model == DocumentModel.GENERAL:
        mistral = backends[AnalyzerBackendType.MISTRAL_DOCUMENT_AI]
        if mistral.is_available():
            return mistral

    # 5. Fallback chain: DI → CU → Mistral
    for bt in (
        AnalyzerBackendType.DOCUMENT_INTELLIGENCE,
        AnalyzerBackendType.CONTENT_UNDERSTANDING,
        AnalyzerBackendType.MISTRAL_DOCUMENT_AI,
    ):
        backend = backends[bt]
        if backend.is_available():
            return backend

    raise RuntimeError(
        "No analyzer backend is available. "
        "Configure at least one of: Azure Document Intelligence, "
        "Azure Content Understanding, or Mistral Document AI."
    )


async def analyze(request: AnalyzerRequest) -> AnalyzerResult:
    """Convenience: select backend and run analysis in one call."""
    backend = get_analyzer(request)
    logger.info(
        "analyzer_selected backend=%s model=%s mime=%s request_id=%s",
        backend.backend_type.value,
        request.model.value,
        request.mime_type,
        request.request_id,
    )
    return await backend.analyze(request)
