"""Image generation provider — Azure OpenAI DALL-E + Google Gemini.

Phase 4.2.
"""
from __future__ import annotations

import base64
import logging
from typing import Any

import httpx

from app.config import get_settings

logger = logging.getLogger(__name__)

_client: httpx.AsyncClient | None = None


def _get_client() -> httpx.AsyncClient:
    global _client
    if _client is None or _client.is_closed:
        _client = httpx.AsyncClient(timeout=60.0)
    return _client


async def close() -> None:
    global _client
    if _client is not None and not _client.is_closed:
        await _client.aclose()
        _client = None


async def generate_image(
    prompt: str,
    *,
    provider: str = "openai",
    model: str | None = None,
    size: str = "1024x1024",
    quality: str = "standard",
    n: int = 1,
) -> list[dict[str, Any]]:
    """Generate images from a text prompt.

    Returns list of dicts: [{"b64_json": "...", "revised_prompt": "..."}]
    """
    settings = get_settings()
    client = _get_client()

    if provider == "openai":
        return await _generate_openai(client, settings, prompt, model or "dall-e-3", size, quality, n)
    if provider == "azure":
        return await _generate_azure(client, settings, prompt, model or "dall-e-3", size, quality, n)
    if provider == "google":
        return await _generate_google(client, settings, prompt, model or "gemini-2.0-flash-exp", n)
    raise ValueError(f"Unsupported image provider: {provider}")


async def _generate_openai(
    client: httpx.AsyncClient,
    settings: Any,
    prompt: str,
    model: str,
    size: str,
    quality: str,
    n: int,
) -> list[dict[str, Any]]:
    if not settings.openai_api_key:
        raise RuntimeError("OPENAI_API_KEY not configured")

    resp = await client.post(
        "https://api.openai.com/v1/images/generations",
        headers={"Authorization": f"Bearer {settings.openai_api_key}"},
        json={
            "model": model,
            "prompt": prompt,
            "n": n,
            "size": size,
            "quality": quality,
            "response_format": "b64_json",
        },
    )
    resp.raise_for_status()
    data = resp.json()
    return [
        {"b64_json": item["b64_json"], "revised_prompt": item.get("revised_prompt", prompt)}
        for item in data["data"]
    ]


async def _generate_azure(
    client: httpx.AsyncClient,
    settings: Any,
    prompt: str,
    model: str,
    size: str,
    quality: str,
    n: int,
) -> list[dict[str, Any]]:
    if not settings.azure_openai_endpoint or not settings.azure_openai_api_key:
        raise RuntimeError("Azure OpenAI endpoint/key not configured")

    url = (
        f"{settings.azure_openai_endpoint.rstrip('/')}"
        f"/openai/deployments/{model}/images/generations"
        f"?api-version={settings.azure_openai_api_version}"
    )
    resp = await client.post(
        url,
        headers={"api-key": settings.azure_openai_api_key},
        json={
            "prompt": prompt,
            "n": n,
            "size": size,
            "quality": quality,
            "response_format": "b64_json",
        },
    )
    resp.raise_for_status()
    data = resp.json()
    return [
        {"b64_json": item["b64_json"], "revised_prompt": item.get("revised_prompt", prompt)}
        for item in data["data"]
    ]


async def _generate_google(
    client: httpx.AsyncClient,
    settings: Any,
    prompt: str,
    model: str,
    n: int,
) -> list[dict[str, Any]]:
    if not settings.google_api_key:
        raise RuntimeError("GOOGLE_API_KEY not configured")

    url = f"https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent"
    resp = await client.post(
        url,
        params={"key": settings.google_api_key},
        json={
            "contents": [{"parts": [{"text": prompt}]}],
            "generationConfig": {"responseMimeType": "image/png"},
        },
    )
    resp.raise_for_status()
    data = resp.json()

    results: list[dict[str, Any]] = []
    for candidate in data.get("candidates", []):
        for part in candidate.get("content", {}).get("parts", []):
            if "inlineData" in part:
                results.append({
                    "b64_json": part["inlineData"]["data"],
                    "revised_prompt": prompt,
                })
    if not results:
        raise RuntimeError("No images returned from Google API")
    return results[:n]
