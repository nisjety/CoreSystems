"""Layer 3 — Intent classification.

Heuristic classifier (keyword patterns) as the fast path.
When `intent_llm_enabled=True` and the heuristic returns UNKNOWN or low-confidence,
falls back to a cheap LLM call (default: gpt-4o-mini) via reasoning_runtime.
"""

from __future__ import annotations

import json
import logging
import re

from app.config import get_settings
from app.domain import IntentType, PipelineContext

logger = logging.getLogger(__name__)

# Ordered patterns: first match wins
_PATTERNS: list[tuple[re.Pattern[str], IntentType, float]] = [
    # Images
    (re.compile(r"\b(generate|create|draw|make)\b.*\b(image|picture|photo|illustration|icon)\b", re.I), IntentType.IMAGE_GENERATION, 0.9),
    (re.compile(r"\b(image|picture|photo|illustration|icon)\b.*\b(of|for|showing)\b", re.I), IntentType.IMAGE_GENERATION, 0.85),
    # Speech — avoid matching "say" alone (too ambiguous)
    (re.compile(r"\b(read aloud|text.to.speech|text to speech|tts|speak (this|it) (aloud|out loud)|convert.{0,30}to speech)\b", re.I), IntentType.SPEECH_TTS, 0.9),
    (re.compile(r"\b(transcribe|speech.to.text|stt|recognize speech)\b", re.I), IntentType.SPEECH_STT, 0.9),
    # Translation
    (re.compile(r"\b(translate|translation)\b", re.I), IntentType.TRANSLATION, 0.9),
    # Code
    (re.compile(r"\b(write|generate|create|fix|debug)\b.*\b(code|function|class|script|program)\b", re.I), IntentType.CODE_GENERATION, 0.85),
    (re.compile(r"\b(refactor|implement|unittest)\b", re.I), IntentType.CODE_GENERATION, 0.8),
    # Summarisation
    (re.compile(r"\b(summarize|summarise|summary|tldr|tl;dr)\b", re.I), IntentType.SUMMARIZATION, 0.9),
    # Search / RAG
    (re.compile(r"\b(search|find|look up|retrieve|what is|who is)\b", re.I), IntentType.SEARCH, 0.7),
]

_AGENT_TOOLS_THRESHOLD = 1

_INTENT_SYSTEM_PROMPT = """\
Classify the user message into exactly one intent category.
Valid categories: chat, completion, agent_task, image_generation, speech_tts,
speech_stt, translation, code_generation, summarization, search, unknown.

Respond with JSON only, no prose:
{"intent": "<category>", "confidence": <0.0-1.0>}"""


async def run(ctx: PipelineContext) -> PipelineContext:
    raw = ctx._raw
    message: str = raw.get("message", "")
    tools: list = raw.get("tools", [])

    # Tool-based agent tasks take priority
    if len(tools) >= _AGENT_TOOLS_THRESHOLD:
        ctx.intent = IntentType.AGENT_TASK
        ctx.intent_confidence = 0.95
        logger.debug("L03_intent agent_task (tools=%d) request_id=%s", len(tools), ctx.request_id)
        return ctx

    # Fast heuristic path
    for pattern, intent, confidence in _PATTERNS:
        if pattern.search(message):
            ctx.intent = intent
            ctx.intent_confidence = confidence
            logger.debug("L03_intent %s (conf=%.2f) request_id=%s", intent.value, confidence, ctx.request_id)
            return ctx

    # LLM fallback for low-confidence / unknown
    settings = get_settings()
    if settings.intent_llm_enabled:
        ctx = await _llm_classify(ctx, message, settings.intent_model)
        return ctx

    # Default: generic chat
    ctx.intent = IntentType.CHAT
    ctx.intent_confidence = 0.6
    logger.debug("L03_intent chat (default) request_id=%s", ctx.request_id)
    return ctx


async def _llm_classify(ctx: PipelineContext, message: str, model: str) -> PipelineContext:
    """Use a fast LLM to classify intent when heuristics don't match."""
    try:
        from reasoning_runtime import execute
        from app.domain import CompletionRequest, Message, Provider

        req = CompletionRequest(
            request_id=ctx.request_id,
            provider=Provider.OPENAI,
            model_id=model,
            messages=[
                Message(role="system", content=_INTENT_SYSTEM_PROMPT),
                Message(role="user", content=message[:500]),  # truncate — cost guard
            ],
            temperature=0.0,
            max_tokens=64,
            org_id=ctx.org_id,
            run_id=ctx.run_id if ctx.run_id else ctx.request_id,
        )
        result = await execute(req)
        data = json.loads(result.content)
        intent_str = data.get("intent", "chat").lower()
        confidence = float(data.get("confidence", 0.75))

        try:
            ctx.intent = IntentType(intent_str)
        except ValueError:
            ctx.intent = IntentType.CHAT

        ctx.intent_confidence = confidence
        logger.debug(
            "L03_intent llm_classify intent=%s conf=%.2f request_id=%s",
            ctx.intent.value, confidence, ctx.request_id,
        )
    except Exception as exc:
        logger.warning("L03_intent llm_classify_failed error=%s request_id=%s", exc, ctx.request_id)
        ctx.intent = IntentType.CHAT
        ctx.intent_confidence = 0.6

    return ctx
