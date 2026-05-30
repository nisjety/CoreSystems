"""Layer 4 — Capability resolution.

Maps an IntentType to a (Provider, model) pair, unless the caller
already specified both explicitly.
"""

from __future__ import annotations

import logging

from app.domain import IntentType, PipelineContext, Provider

logger = logging.getLogger(__name__)

# Default provider+model per intent type
_DEFAULTS: dict[IntentType, tuple[Provider, str]] = {
    IntentType.CHAT: (Provider.OPENAI, "gpt-4o-mini"),
    IntentType.COMPLETION: (Provider.OPENAI, "gpt-4o-mini"),
    IntentType.AGENT_TASK: (Provider.OPENAI, "gpt-4o-mini"),
    IntentType.IMAGE_GENERATION: (Provider.OPENAI, "gpt-4o-mini"),
    IntentType.SPEECH_TTS: (Provider.OPENAI, "gpt-4o-mini"),
    IntentType.SPEECH_STT: (Provider.OPENAI, "gpt-4o-mini"),
    IntentType.TRANSLATION: (Provider.OPENAI, "gpt-4o-mini"),
    IntentType.CODE_GENERATION: (Provider.OPENAI, "gpt-4o-mini"),
    IntentType.SUMMARIZATION: (Provider.OPENAI, "gpt-4o-mini"),
    IntentType.SEARCH: (Provider.OPENAI, "gpt-4o-mini"),
    IntentType.UNKNOWN: (Provider.OPENAI, "gpt-4o-mini"),
}

# Intents whose native model is unavailable on this deployment;
# re-classify them as CHAT so the pipeline completes gracefully.
_CHAT_FALLBACK_INTENTS: frozenset[IntentType] = frozenset({
    IntentType.IMAGE_GENERATION,
    IntentType.SPEECH_TTS,
    IntentType.SPEECH_STT,
})


async def run(ctx: PipelineContext) -> PipelineContext:
    # If caller pre-specified both provider and model, keep them
    if ctx.resolved_provider and ctx.resolved_model:
        logger.debug(
            "L04_capability caller-specified %s/%s request_id=%s",
            ctx.resolved_provider.value,
            ctx.resolved_model,
            ctx.request_id,
        )
        return ctx

    # Redirect intents whose native model is not deployed to CHAT
    if ctx.intent in _CHAT_FALLBACK_INTENTS:
        logger.warning(
            "L04_capability %s has no native deployment — falling back to chat request_id=%s",
            ctx.intent.value,
            ctx.request_id,
        )
        ctx.intent = IntentType.CHAT

    default_provider, default_model = _DEFAULTS.get(
        ctx.intent, (Provider.OPENAI, "gpt-4o-mini")
    )

    if not ctx.resolved_provider:
        ctx.resolved_provider = default_provider
    if not ctx.resolved_model:
        ctx.resolved_model = default_model

    logger.debug(
        "L04_capability resolved %s/%s for intent=%s request_id=%s",
        ctx.resolved_provider.value,
        ctx.resolved_model,
        ctx.intent.value,
        ctx.request_id,
    )
    return ctx
