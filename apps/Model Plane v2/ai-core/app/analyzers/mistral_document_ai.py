"""Mistral Document AI analyzer backend (Tier 2).

Handles complex document reasoning tasks where structured extraction
alone is insufficient — deep comprehension, summarization, and Q&A
over documents.

Deployed via Azure AI Foundry → Model Catalog → mistral-document-ai-2505.
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


class MistralDocumentAIAnalyzer(AnalyzerBackend):
    """Mistral Document AI via Azure AI Foundry serverless endpoint."""

    backend_type = AnalyzerBackendType.MISTRAL_DOCUMENT_AI

    def is_available(self) -> bool:
        s = get_settings()
        return bool(s.mistral_document_ai_endpoint and s.mistral_document_ai_key)

    async def analyze(self, request: AnalyzerRequest) -> AnalyzerResult:
        settings = get_settings()

        image_url = self._build_image_url(request)
        prompt = request.extra.get("prompt", "Extract all text and structure from this document.")

        messages = [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": prompt},
                    {
                        "type": "image_url",
                        "image_url": {"url": image_url},
                    },
                ],
            }
        ]

        payload = {
            "model": "mistral-document-ai-2505",
            "messages": messages,
            "max_tokens": request.extra.get("max_tokens", 4096),
            "include_image_base64": True,
            "document_image_limit": request.extra.get("document_image_limit", 8),
        }

        endpoint = settings.mistral_document_ai_endpoint.rstrip("/")
        url = f"{endpoint}/v1/chat/completions"

        async with httpx.AsyncClient(timeout=120) as client:
            resp = await client.post(
                url,
                json=payload,
                headers={
                    "Authorization": f"Bearer {settings.mistral_document_ai_key}",
                    "Content-Type": "application/json",
                },
            )
            resp.raise_for_status()
            data = resp.json()

        content = ""
        if data.get("choices"):
            content = data["choices"][0].get("message", {}).get("content", "")

        usage = data.get("usage", {})

        return AnalyzerResult(
            request_id=request.request_id,
            backend=AnalyzerBackendType.MISTRAL_DOCUMENT_AI,
            content=content,
            raw={
                "model": data.get("model", ""),
                "input_tokens": usage.get("prompt_tokens", 0),
                "output_tokens": usage.get("completion_tokens", 0),
            },
        )

    @staticmethod
    def _build_image_url(request: AnalyzerRequest) -> str:
        if request.url:
            return request.url
        if request.content_base64:
            return f"data:{request.mime_type};base64,{request.content_base64}"
        if request.content_bytes:
            b64 = base64.b64encode(request.content_bytes).decode()
            return f"data:{request.mime_type};base64,{b64}"
        raise ValueError("AnalyzerRequest must have url, content_bytes, or content_base64")
