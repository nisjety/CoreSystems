"""Base classes and domain types for the Analyzer abstraction."""

from __future__ import annotations

import abc
from dataclasses import dataclass, field
from enum import Enum
from typing import Any


class AnalyzerBackendType(str, Enum):
    """Supported analyzer backends."""

    DOCUMENT_INTELLIGENCE = "document_intelligence"
    CONTENT_UNDERSTANDING = "content_understanding"
    MISTRAL_DOCUMENT_AI = "mistral_document_ai"


class DocumentModel(str, Enum):
    """Common document analysis models (maps across backends)."""

    # Layout / structure
    LAYOUT = "layout"
    READ = "read"

    # Prebuilt extractions
    INVOICE = "invoice"
    RECEIPT = "receipt"
    BUSINESS_CARD = "business_card"
    ID_DOCUMENT = "id_document"
    FORM = "form"

    # Multimodal / reasoning
    GENERAL = "general"


@dataclass(frozen=True)
class AnalyzerRequest:
    """Unified request to any analyzer backend."""

    request_id: str
    org_id: str = ""
    model: DocumentModel = DocumentModel.LAYOUT

    # Content source — exactly one should be set
    url: str = ""
    content_bytes: bytes = b""
    content_base64: str = ""
    mime_type: str = "application/pdf"

    # Optional hints
    pages: str = ""  # e.g. "1-3" or "1,3,5"
    locale: str = ""
    extra: dict[str, Any] = field(default_factory=dict)


@dataclass
class AnalyzerResult:
    """Unified result from any analyzer backend."""

    request_id: str
    backend: AnalyzerBackendType
    content: str = ""  # Full extracted text
    pages: list[dict[str, Any]] = field(default_factory=list)
    tables: list[dict[str, Any]] = field(default_factory=list)
    key_value_pairs: list[dict[str, Any]] = field(default_factory=list)
    entities: list[dict[str, Any]] = field(default_factory=list)
    confidence: float = 0.0
    raw: dict[str, Any] = field(default_factory=dict)  # Backend-specific payload


class AnalyzerBackend(abc.ABC):
    """Abstract base for all analyzer implementations."""

    backend_type: AnalyzerBackendType

    @abc.abstractmethod
    async def analyze(self, request: AnalyzerRequest) -> AnalyzerResult:
        """Run analysis and return a unified result."""

    @abc.abstractmethod
    def is_available(self) -> bool:
        """Return True if this backend is configured and operational."""
