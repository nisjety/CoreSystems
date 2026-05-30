"""Layer 5 — Context enrichment.

Injects system prompts, enriches context with RAG documents,
and builds the final message array.
"""

from __future__ import annotations

import logging

from app.domain import IntentType, PipelineContext

logger = logging.getLogger(__name__)

# Default system prompts per intent category
_SYSTEM_PROMPTS: dict[IntentType, str] = {
    IntentType.CHAT: (
        "You are a helpful assistant. Answer the user's questions clearly and concisely."
    ),
    IntentType.CODE_GENERATION: (
        "You are an expert software engineer. Write clean, well-documented code. "
        "Include type hints and follow best practices for the target language."
    ),
    IntentType.SUMMARIZATION: (
        "You are a summarisation assistant. Provide clear, factual summaries. "
        "Preserve key information while being concise."
    ),
    IntentType.AGENT_TASK: (
        "You are an AI agent. Use the provided tools to accomplish the user's task."
    ),
    IntentType.TRANSLATION: (
        "You are a professional translator. Provide accurate, natural translations."
    ),
    IntentType.SEARCH: (
        "You are a search assistant. Answer grounded in the provided context documents. "
        "If the context is insufficient, say so."
    ),
}

_DEFAULT_PROMPT = "You are a helpful assistant."


async def run(ctx: PipelineContext) -> PipelineContext:
    raw = ctx._raw
    extra_context: dict = raw.get("extra_context", {})

    # System prompt: caller override > intent default > generic
    caller_prompt = extra_context.get("system_prompt", "")
    if caller_prompt:
        ctx.system_prompt = caller_prompt
    elif not ctx.system_prompt:
        ctx.system_prompt = _SYSTEM_PROMPTS.get(ctx.intent, _DEFAULT_PROMPT)

    # RAG documents (if provided by session-core or caller)
    docs = extra_context.get("documents", [])
    if docs:
        ctx.context_documents = docs

    # Format instructions
    fmt = extra_context.get("format_instructions", "")
    if fmt:
        ctx.format_instructions = fmt

    logger.debug(
        "L05_context system_prompt_len=%d docs=%d request_id=%s",
        len(ctx.system_prompt),
        len(ctx.context_documents),
        ctx.request_id,
    )
    return ctx
