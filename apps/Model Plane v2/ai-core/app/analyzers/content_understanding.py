"""Azure Content Understanding analyzer backend.

Handles multimodal content analysis: images, video frames, audio segments,
and documents through a unified Azure cognitive endpoint.

Uses the 2025-05-01-preview GA API.
"""

from __future__ import annotations

import base64
import logging
from typing import Any

import httpx

from app.analyzers.base import (
    AnalyzerBackend,
    AnalyzerBackendType,
    AnalyzerRequest,
    AnalyzerResult,
)
from app.config import get_settings

logger = logging.getLogger(__name__)


class ContentUnderstandingAnalyzer(AnalyzerBackend):
    """Azure AI Content Understanding (multimodal)."""

    backend_type = AnalyzerBackendType.CONTENT_UNDERSTANDING

    def is_available(self) -> bool:
        s = get_settings()
        return bool(
            s.azure_content_understanding_endpoint
            and s.azure_content_understanding_key
            and s.enable_content_understanding
        )

    async def analyze(self, request: AnalyzerRequest) -> AnalyzerResult:
        settings = get_settings()

        endpoint = settings.azure_content_understanding_endpoint.rstrip("/")
        api_version = settings.azure_content_understanding_api_version
        url = f"{endpoint}/contentunderstanding/analyzers:analyze?api-version={api_version}"

        body = self._build_body(request)

        async with httpx.AsyncClient(timeout=120) as client:
            resp = await client.post(
                url,
                json=body,
                headers={
                    "Ocp-Apim-Subscription-Key": settings.azure_content_understanding_key,
                    "Content-Type": "application/json",
                },
            )
            resp.raise_for_status()
            data = resp.json()

        return self._to_result(request, data)

    @staticmethod
    def _build_body(request: AnalyzerRequest) -> dict[str, Any]:
        body: dict[str, Any] = {}

        if request.url:
            body["url"] = request.url
        elif request.content_base64:
            body["data"] = request.content_base64
        elif request.content_bytes:
            body["data"] = base64.b64encode(request.content_bytes).decode()
        else:
            raise ValueError("AnalyzerRequest must have url, content_bytes, or content_base64")

        body["mimeType"] = request.mime_type
        body["analyzerId"] = request.extra.get("analyzer_id", "prebuilt-document")

        if request.extra.get("fields"):
            body["fieldSchema"] = request.extra["fields"]

        return body

    @staticmethod
    def _to_result(request: AnalyzerRequest, data: dict[str, Any]) -> AnalyzerResult:
        contents = data.get("result", {}).get("contents", [])
        text_parts = []
        entities: list[dict[str, Any]] = []

        for item in contents:
            if item.get("kind") == "text":
                text_parts.append(item.get("content", ""))
            if item.get("fields"):
                for field_name, field_val in item["fields"].items():
                    entities.append({
                        "field": field_name,
                        "value": field_val.get("valueString", field_val.get("content", "")),
                        "confidence": field_val.get("confidence", 0.0),
                    })

        return AnalyzerResult(
            request_id=request.request_id,
            backend=AnalyzerBackendType.CONTENT_UNDERSTANDING,
            content="\n".join(text_parts),
            entities=entities,
            raw=data,
        )
