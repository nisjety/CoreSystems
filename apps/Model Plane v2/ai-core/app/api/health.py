"""Health, readiness, and feature-activation reporting endpoints."""

from __future__ import annotations

import logging

from fastapi import APIRouter

from app.config import get_settings

logger = logging.getLogger(__name__)

router = APIRouter(tags=["health"])


@router.get("/health")
async def health():
    return {"status": "ok", "service": "ai-core", "version": "2.0.0"}


@router.get("/ready")
async def ready():
    """Deep readiness check — verifies critical dependencies are reachable."""
    settings = get_settings()
    checks: dict[str, str] = {}

    # Redis
    try:
        import redis.asyncio as aioredis

        r = aioredis.from_url(settings.redis_url, socket_connect_timeout=2)
        await r.ping()
        await r.aclose()
        checks["redis"] = "ok"
    except Exception as exc:
        checks["redis"] = f"fail: {exc}"

    # NATS (optional — only check if URL set)
    if settings.nats_url:
        try:
            import asyncio
            import nats

            nc = await asyncio.wait_for(
                nats.connect(
                    settings.nats_url,
                    connect_timeout=2,
                    max_reconnect_attempts=0,  # no reconnect — just a health probe
                ),
                timeout=3,
            )
            await nc.close()
            checks["nats"] = "ok"
        except Exception as exc:
            checks["nats"] = f"fail: {type(exc).__name__}: {exc}"

    all_ok = all(v == "ok" for v in checks.values())
    return {
        "status": "ready" if all_ok else "degraded",
        "service": "ai-core",
        "checks": checks,
    }


@router.get("/activation")
async def activation():
    """Report which capabilities are active based on current configuration.

    Returns a structured map of every capability with its enabled/configured
    status, so operators can verify deployment config at a glance.
    """
    settings = get_settings()

    def _available(endpoint_attr: str, key_attr: str) -> bool:
        return bool(getattr(settings, endpoint_attr, "") and getattr(settings, key_attr, ""))

    capabilities = {
        # Providers
        "azure_openai": _available("azure_openai_endpoint", "azure_openai_api_key"),
        "openai": bool(settings.openai_api_key),
        "anthropic": bool(settings.anthropic_api_key),
        "google": bool(settings.google_api_key),
        "cohere": bool(settings.cohere_api_key),
        "mistral": bool(settings.mistral_api_key),
        "ollama": bool(settings.ollama_base_url),

        # Azure AI Services
        "azure_speech": bool(settings.azure_speech_key),
        "azure_translator": bool(settings.azure_translator_key),
        "document_intelligence": _available(
            "azure_document_intelligence_endpoint",
            "azure_document_intelligence_key",
        ),
        "content_understanding": (
            settings.enable_content_understanding
            and _available(
                "azure_content_understanding_endpoint",
                "azure_content_understanding_key",
            )
        ),
        "mistral_document_ai": (
            settings.enable_mistral_document_ai
            and _available("mistral_document_ai_endpoint", "mistral_document_ai_key")
        ),
        "content_safety": (
            settings.content_safety_enabled
            and _available("azure_content_safety_endpoint", "azure_content_safety_key")
        ),
        "ai_language": _available("azure_ai_language_endpoint", "azure_ai_language_key"),

        # Feature flags
        "chat": settings.enable_chat,
        "tts": settings.enable_tts,
        "asr": settings.enable_asr,
        "translation": settings.enable_translation,
        "image_generation": settings.enable_image_generation,
        "vision": settings.enable_vision,
        "video_generation": settings.enable_video_generation,
        "realtime": settings.enable_realtime,

        # Pipeline
        "rag_reflection": settings.rag_reflection_enabled,
        "rag_synthesis": settings.rag_synthesis_enabled,
        "intent_classification": settings.intent_llm_enabled,

        # Infrastructure
        "grpc_reflection": settings.grpc_enable_reflection,
        "agent_core_delegation": bool(settings.agent_core_url),
    }

    active = {k: v for k, v in capabilities.items() if v}
    inactive = {k: v for k, v in capabilities.items() if not v}

    return {
        "service": "ai-core",
        "active_count": len(active),
        "inactive_count": len(inactive),
        "active": sorted(active.keys()),
        "inactive": sorted(inactive.keys()),
    }
