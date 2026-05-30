"""Provider health probe — cached per provider, 30s TTL."""

from __future__ import annotations

import logging

import httpx

from app.config import get_settings
from app.redis_client import get_provider_health, set_provider_health

logger = logging.getLogger(__name__)


async def is_healthy(provider: str) -> bool:
    """Check if a provider is healthy (cached)."""
    cached = await get_provider_health(provider)
    if cached is not None:
        return cached

    healthy = await _probe(provider)
    await set_provider_health(provider, healthy)
    return healthy


async def _probe(provider: str) -> bool:
    """Best-effort HTTP probe — returns True on any 2xx."""
    endpoint = _provider_health_url(provider)
    if endpoint is None:
        return True  # Unknown provider → assume healthy

    try:
        async with httpx.AsyncClient(timeout=5.0) as client:
            resp = await client.get(endpoint)
        return resp.status_code < 400
    except Exception:
        logger.warning("provider health probe failed: %s", provider)
        return False


def _provider_health_url(provider: str) -> str | None:
    """Map provider name to a lightweight health URL."""
    urls: dict[str, str] = {
        "openai": "https://api.openai.com/v1/models",
        "anthropic": "https://api.anthropic.com/v1/models",
    }
    return urls.get(provider)
