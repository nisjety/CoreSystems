"""Content Safety Service — Azure AI Content Safety SDK integration.

Replaces the regex-only stubs in L07/L09 with real severity-scored
text analysis. Fails open: on any SDK error the caller falls back
to the existing regex patterns.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field
from typing import Any

from app.config import get_settings
from app.domain import SafetyVerdict

logger = logging.getLogger(__name__)


@dataclass
class CategoryScore:
    category: str
    severity: int  # 0-6


@dataclass
class ContentSafetyResult:
    verdict: SafetyVerdict
    categories: list[CategoryScore] = field(default_factory=list)
    max_severity: int = 0
    sdk_error: bool = False
    error_detail: str = ""


_service: ContentSafetyService | None = None


def get_content_safety_service() -> ContentSafetyService:
    global _service
    if _service is None:
        _service = ContentSafetyService()
    return _service


class ContentSafetyService:
    """Thin wrapper around Azure AI Content Safety SDK.

    Lazily imports the SDK so the service still starts when the
    package is not installed (e.g. unit tests without the dep).
    """

    # Azure returns integer severity in multiples of 2: 0, 2, 4, 6
    # We treat 4+ as block, 2+ as flag (configurable via settings).

    def __init__(self) -> None:
        self._client: Any = None  # azure.ai.contentsafety.ContentSafetyClient
        s = get_settings()
        self._endpoint = s.azure_content_safety_endpoint
        self._key = s.azure_content_safety_key
        self._block_threshold = s.content_safety_block_threshold
        self._flag_threshold = s.content_safety_flag_threshold

    def _get_client(self) -> Any:
        if self._client is not None:
            return self._client
        if not self._endpoint or not self._key:
            raise RuntimeError("azure_content_safety_endpoint / _key not configured")
        try:
            from azure.ai.contentsafety import ContentSafetyClient
            from azure.core.credentials import AzureKeyCredential

            self._client = ContentSafetyClient(
                endpoint=self._endpoint,
                credential=AzureKeyCredential(self._key),
            )
        except ImportError as exc:
            raise RuntimeError("azure-ai-contentsafety package not installed") from exc
        return self._client

    async def analyze_text(self, text: str, org_id: str = "") -> ContentSafetyResult:
        """Analyze text and return a safety verdict with per-category scores.

        Always returns a result — never raises.  If the SDK is unavailable
        or misconfigured, returns SAFE with ``sdk_error=True`` so the caller
        falls back to regex patterns.
        """
        try:
            from azure.ai.contentsafety.models import AnalyzeTextOptions, TextCategory

            client = self._get_client()
            request = AnalyzeTextOptions(
                text=text[:10_000],  # SDK hard limit
                categories=[
                    TextCategory.HATE,
                    TextCategory.SELF_HARM,
                    TextCategory.SEXUAL,
                    TextCategory.VIOLENCE,
                ],
            )

            # The Azure SDK is synchronous; run in threadpool to avoid blocking
            import asyncio

            loop = asyncio.get_event_loop()
            response = await loop.run_in_executor(None, client.analyze_text, request)

            categories: list[CategoryScore] = []
            max_severity = 0
            for item in response.categories_analysis:
                sev = item.severity or 0
                categories.append(CategoryScore(category=item.category.value, severity=sev))
                if sev > max_severity:
                    max_severity = sev

            verdict: SafetyVerdict
            if max_severity >= self._block_threshold:
                verdict = SafetyVerdict.BLOCKED
            elif max_severity >= self._flag_threshold:
                verdict = SafetyVerdict.FLAGGED
            else:
                verdict = SafetyVerdict.SAFE

            logger.info(
                "content_safety_analyzed org_id=%s max_severity=%d verdict=%s",
                org_id,
                max_severity,
                verdict.value,
            )
            return ContentSafetyResult(
                verdict=verdict,
                categories=categories,
                max_severity=max_severity,
            )

        except Exception as exc:
            logger.warning(
                "content_safety_sdk_error org_id=%s error=%s — falling back to regex",
                org_id,
                exc,
            )
            return ContentSafetyResult(
                verdict=SafetyVerdict.SAFE,
                sdk_error=True,
                error_detail=str(exc),
            )
