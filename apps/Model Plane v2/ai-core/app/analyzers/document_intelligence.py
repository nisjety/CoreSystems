"""Azure Document Intelligence analyzer backend (Tier 1).

Handles structured document extraction: forms, invoices, receipts,
business cards, layout, and OCR read.
"""

from __future__ import annotations

import logging
from typing import Any

from app.analyzers.base import (
    AnalyzerBackend,
    AnalyzerBackendType,
    AnalyzerRequest,
    AnalyzerResult,
    DocumentModel,
)
from app.config import get_settings

logger = logging.getLogger(__name__)

# Map our generic models → Azure DI model IDs
_MODEL_MAP: dict[DocumentModel, str] = {
    DocumentModel.LAYOUT: "prebuilt-layout",
    DocumentModel.READ: "prebuilt-read",
    DocumentModel.INVOICE: "prebuilt-invoice",
    DocumentModel.RECEIPT: "prebuilt-receipt",
    DocumentModel.BUSINESS_CARD: "prebuilt-businessCard",
    DocumentModel.ID_DOCUMENT: "prebuilt-idDocument",
    DocumentModel.FORM: "prebuilt-document",
    DocumentModel.GENERAL: "prebuilt-layout",
}


class DocumentIntelligenceAnalyzer(AnalyzerBackend):
    """Azure Document Intelligence (formerly Form Recognizer)."""

    backend_type = AnalyzerBackendType.DOCUMENT_INTELLIGENCE

    def is_available(self) -> bool:
        s = get_settings()
        return bool(s.azure_document_intelligence_endpoint and s.azure_document_intelligence_key)

    async def analyze(self, request: AnalyzerRequest) -> AnalyzerResult:
        from azure.ai.documentintelligence.aio import DocumentIntelligenceClient
        from azure.ai.documentintelligence.models import AnalyzeDocumentRequest
        from azure.core.credentials import AzureKeyCredential

        settings = get_settings()
        client = DocumentIntelligenceClient(
            endpoint=settings.azure_document_intelligence_endpoint,
            credential=AzureKeyCredential(settings.azure_document_intelligence_key),
        )

        model_id = _MODEL_MAP.get(request.model, "prebuilt-layout")
        body = self._build_request(request)

        async with client:
            poller = await client.begin_analyze_document(
                model_id,
                body=AnalyzeDocumentRequest(**body),
                pages=request.pages or None,
                locale=request.locale or None,
            )
            result = await poller.result()

        return self._to_result(request, result)

    @staticmethod
    def _build_request(request: AnalyzerRequest) -> dict[str, Any]:
        if request.url:
            return {"url_source": request.url}
        if request.content_bytes:
            return {"bytes_source": request.content_bytes}
        if request.content_base64:
            import base64

            return {"bytes_source": base64.b64decode(request.content_base64)}
        raise ValueError("AnalyzerRequest must have url, content_bytes, or content_base64")

    @staticmethod
    def _to_result(request: AnalyzerRequest, result: Any) -> AnalyzerResult:
        pages: list[dict[str, Any]] = []
        if result.pages:
            for page in result.pages:
                pages.append({
                    "page_number": page.page_number,
                    "width": getattr(page, "width", None),
                    "height": getattr(page, "height", None),
                    "lines": [
                        {"content": line.content, "confidence": getattr(line, "confidence", None)}
                        for line in (page.lines or [])
                    ],
                })

        tables: list[dict[str, Any]] = []
        if result.tables:
            for table in result.tables:
                tables.append({
                    "row_count": table.row_count,
                    "column_count": table.column_count,
                    "cells": [
                        {
                            "row": cell.row_index,
                            "col": cell.column_index,
                            "content": cell.content,
                        }
                        for cell in (table.cells or [])
                    ],
                })

        kv_pairs: list[dict[str, Any]] = []
        if result.key_value_pairs:
            for pair in result.key_value_pairs:
                kv_pairs.append({
                    "key": pair.key.content if pair.key else "",
                    "value": pair.value.content if pair.value else "",
                    "confidence": pair.confidence,
                })

        return AnalyzerResult(
            request_id=request.request_id,
            backend=AnalyzerBackendType.DOCUMENT_INTELLIGENCE,
            content=result.content or "",
            pages=pages,
            tables=tables,
            key_value_pairs=kv_pairs,
            confidence=0.0,
            raw={"model_id": result.model_id} if hasattr(result, "model_id") else {},
        )
