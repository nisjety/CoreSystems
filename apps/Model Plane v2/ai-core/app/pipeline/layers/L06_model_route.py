"""Layer 6 — Model routing.

Final provider-selection step — applies overrides, availability checks,
and fallback logic before the request reaches the execution layer.
"""

from __future__ import annotations

import logging

from reasoning_runtime import get_config

from app.domain import PipelineContext, Provider

logger = logging.getLogger(__name__)

# Provider → required config key (must be non-empty for provider to be available)
_PROVIDER_KEYS: dict[Provider, str] = {
    Provider.OPENAI: "openai_api_key",
    Provider.ANTHROPIC: "anthropic_api_key",
    Provider.GEMINI: "google_api_key",
    Provider.COHERE: "cohere_api_key",
    Provider.MISTRAL: "mistral_api_key",
    Provider.OLLAMA: "ollama_base_url",
    Provider.AZURE_OPENAI: "azure_openai_api_key",
}

# Fallback chain: if preferred provider unavailable, try these in order
_FALLBACK_CHAIN: list[tuple[Provider, str]] = [
    (Provider.OPENAI, "gpt-4o-mini"),
    (Provider.ANTHROPIC, "claude-sonnet-4-20250514"),
    (Provider.GEMINI, "gemini-2.0-flash"),
    (Provider.OLLAMA, "llama3.2"),
]


def _is_available(provider: Provider) -> bool:
    """Check if a provider's API key/endpoint is configured."""
    cfg = get_config()
    key_attr = _PROVIDER_KEYS.get(provider)
    if not key_attr:
        return False
    return bool(getattr(cfg, key_attr, ""))


async def run(ctx: PipelineContext) -> PipelineContext:
    provider = ctx.resolved_provider

    # If resolved provider is available, keep it
    if provider and _is_available(provider):
        logger.debug(
            "L06_model_route confirmed %s/%s request_id=%s",
            provider.value,
            ctx.resolved_model,
            ctx.request_id,
        )
        return ctx

    # Provider unavailable — walk the fallback chain
    for fallback_provider, fallback_model in _FALLBACK_CHAIN:
        if _is_available(fallback_provider):
            old = f"{provider.value if provider else 'none'}/{ctx.resolved_model}"
            ctx.resolved_provider = fallback_provider
            ctx.resolved_model = fallback_model
            logger.warning(
                "L06_model_route fallback %s → %s/%s request_id=%s",
                old,
                fallback_provider.value,
                fallback_model,
                ctx.request_id,
            )
            return ctx

    raise ValueError("No LLM providers are configured — at least one API key is required")
