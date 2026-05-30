"""Content safety service — Azure Content Safety + local heuristics.

Phase 4.3.
"""
from __future__ import annotations

import logging
import re
from dataclasses import dataclass, field
from typing import Any

import httpx

logger = logging.getLogger(__name__)

_client: httpx.AsyncClient | None = None


def _get_client() -> httpx.AsyncClient:
    global _client
    if _client is None or _client.is_closed:
        _client = httpx.AsyncClient(timeout=15.0)
    return _client


async def close() -> None:
    global _client
    if _client is not None and not _client.is_closed:
        await _client.aclose()
        _client = None


@dataclass(frozen=True)
class SafetyResult:
    """Immutable safety check result."""

    safe: bool
    categories: dict[str, int] = field(default_factory=dict)
    blocked_categories: list[str] = field(default_factory=list)
    action: str = "accept"


# ── Config thresholds ──

_DEFAULT_THRESHOLDS: dict[str, int] = {
    "Hate": 2,
    "SelfHarm": 2,
    "Sexual": 2,
    "Violence": 2,
}


async def check_text(
    text: str,
    *,
    azure_endpoint: str = "",
    azure_key: str = "",
    thresholds: dict[str, int] | None = None,
) -> SafetyResult:
    """Run content safety analysis on text.

    Falls back to local heuristics if Azure Content Safety is not configured.
    """
    if not text.strip():
        return SafetyResult(safe=True, action="accept")

    thresh = thresholds or _DEFAULT_THRESHOLDS

    if azure_endpoint and azure_key:
        return await _check_azure(text, azure_endpoint, azure_key, thresh)

    return _check_local(text, thresh)


async def _check_azure(
    text: str,
    endpoint: str,
    key: str,
    thresholds: dict[str, int],
) -> SafetyResult:
    """Call Azure Content Safety Analyze Text API."""
    client = _get_client()
    url = f"{endpoint.rstrip('/')}/contentsafety/text:analyze?api-version=2024-09-01"

    resp = await client.post(
        url,
        headers={"Ocp-Apim-Subscription-Key": key, "Content-Type": "application/json"},
        json={"text": text[:10_000], "categories": list(thresholds.keys())},
    )
    resp.raise_for_status()
    data = resp.json()

    categories: dict[str, int] = {}
    blocked: list[str] = []

    for item in data.get("categoriesAnalysis", []):
        cat = item["category"]
        severity = item.get("severity", 0)
        categories[cat] = severity
        if severity >= thresholds.get(cat, 2):
            blocked.append(cat)

    safe = len(blocked) == 0
    action = "accept" if safe else "block"
    return SafetyResult(safe=safe, categories=categories, blocked_categories=blocked, action=action)


def _check_local(text: str, thresholds: dict[str, int]) -> SafetyResult:
    """Lightweight local heuristic check (no network call).

    Only catches blatant patterns; production should use Azure.
    """
    lower = text.lower()
    blocked: list[str] = []

    # Very simple pattern lists — intentionally conservative
    _patterns: dict[str, list[str]] = {
        "Violence": [r"\bkill\s+(?:everyone|them\s+all)\b", r"\bbomb\s+threat\b"],
        "SelfHarm": [r"\bhow\s+to\s+(?:hurt|harm)\s+(?:myself|oneself)\b"],
    }

    categories: dict[str, int] = {k: 0 for k in thresholds}
    for cat, patterns in _patterns.items():
        for pat in patterns:
            if re.search(pat, lower):
                categories[cat] = 4
                blocked.append(cat)
                break

    safe = len(blocked) == 0
    return SafetyResult(safe=safe, categories=categories, blocked_categories=blocked, action="accept" if safe else "block")
