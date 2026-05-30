"""Auto-compaction — LLM-powered summarization at token thresholds.

Mirrors CC's autoCompact.ts: when conversation history approaches
the token budget (90% threshold), older messages are summarized by
the LLM and replaced with a compact summary message.

Strategies (in priority order):
1. SNIP — just drop middle messages (cheap, lossy) — existing compact.py
2. AUTO — LLM summarizes trimmed messages into a compact block
3. MICRO — incremental: summarize oldest N messages, append summary

This module implements strategy 2 (AUTO) and 3 (MICRO).
Summaries can optionally be persisted to Redis so resumed runs avoid
re-summarization (``run_id`` as cache key).
"""

from __future__ import annotations

import json
import logging
from typing import Any

from app.context.compact import KEEP_HEAD, compact_history
from app.context.tokenizer import estimate_messages_tokens

logger = logging.getLogger(__name__)

# Trigger compaction when usage exceeds this fraction of budget
COMPACT_THRESHOLD = 0.90

# After compaction, target this fraction of budget
COMPACT_TARGET = 0.60

# How many oldest messages to summarize per MICRO step
MICRO_BATCH_SIZE = 10

# Redis key prefix + TTL for cached summaries
_SUMMARY_KEY_PREFIX = "compact:summary:"
_SUMMARY_TTL = 7200  # 2 hours

# Summary prompt sent to LLM to produce the compacted history
SUMMARY_SYSTEM_PROMPT = (
    "You are a conversation summarizer. Given a sequence of conversation messages, "
    "produce a concise summary that preserves:\n"
    "- All tool calls made and their key results\n"
    "- All file paths, names, and identifiers mentioned\n"
    "- Key decisions and reasoning steps\n"
    "- Any errors encountered and how they were resolved\n\n"
    "Output ONLY the summary as plain text. Be concise but preserve critical details.\n"
    "Do NOT add preamble like 'Here is the summary:'. Just output the summary directly."
)


async def auto_compact(
    messages: list[dict[str, Any]],
    token_budget: int,
    llm_client: Any | None = None,
    run_id: str | None = None,
    redis: Any | None = None,
    model: str = "",
) -> tuple[list[dict[str, Any]], bool]:
    """Compact messages if they exceed the token budget threshold.

    Args:
        messages: Full message history.
        token_budget: Maximum token budget for the conversation.
        llm_client: LLM client for summarization (if None, falls back to SNIP).
        run_id: Optional run ID used to cache the summary in Redis.
        redis: Optional Redis client for summary persistence.
        model: Optional model name for per-model threshold calculation.

    Returns:
        (compacted_messages, was_compacted) tuple.
    """
    current_tokens = estimate_messages_tokens(messages)

    # Use per-model thresholds from token_budget if model is provided
    if model:
        from app.token_budget import get_auto_compact_config
        config = get_auto_compact_config(model, custom_window=token_budget)
        threshold_tokens = config.compact_threshold
        if not config.should_compact:
            logger.warning("auto_compact_max_failures", extra={"model": model})
            return messages, False
    else:
        threshold_tokens = int(token_budget * COMPACT_THRESHOLD)

    if current_tokens <= threshold_tokens:
        return messages, False

    logger.info(
        "auto_compact_triggered",
        extra={
            "current_tokens": current_tokens,
            "budget": token_budget,
            "threshold": COMPACT_THRESHOLD,
            "message_count": len(messages),
        },
    )

    # If no LLM client, fall back to SNIP compaction
    if llm_client is None:
        target_messages = _estimate_target_message_count(
            messages, int(token_budget * COMPACT_TARGET)
        )
        compacted = compact_history(messages, max_messages=target_messages)
        return compacted, True

    # Try to load a cached summary from Redis (allow resuming mid-run)
    prior_summary: str | None = None
    if redis is not None and run_id:
        prior_summary = await _load_cached_summary(redis, run_id)

    # LLM-based summarization
    compacted = await _llm_summarize_compact(
        messages, token_budget, llm_client, prior_summary=prior_summary
    )

    # Persist the new summary to Redis
    if redis is not None and run_id:
        summary_text = _extract_summary_text(compacted)
        if summary_text:
            await _save_cached_summary(redis, run_id, summary_text)

    return compacted, True


async def micro_compact(
    messages: list[dict[str, Any]],
    token_budget: int,
    llm_client: Any,
    batch_size: int = MICRO_BATCH_SIZE,
    run_id: str | None = None,
    redis: Any | None = None,
    model: str = "",
) -> tuple[list[dict[str, Any]], bool]:
    """Incremental compaction: summarize the oldest ``batch_size`` messages.

    Unlike ``auto_compact`` (which always processes the full middle section),
    MICRO compaction summarizes just the oldest chunk.  Call it repeatedly
    until the history fits within budget.

    Returns:
        (compacted_messages, was_compacted) tuple.
    """
    current_tokens = estimate_messages_tokens(messages)

    # Use per-model thresholds from token_budget if model is provided
    if model:
        from app.token_budget import get_auto_compact_config
        config = get_auto_compact_config(model, custom_window=token_budget)
        threshold_tokens = config.compact_threshold
    else:
        threshold_tokens = int(token_budget * COMPACT_THRESHOLD)

    if current_tokens <= threshold_tokens:
        return messages, False

    if len(messages) <= KEEP_HEAD + batch_size:
        # Nothing to compress incrementally, fall back to full auto-compact
        return await auto_compact(
            messages, token_budget, llm_client, run_id=run_id, redis=redis,
            model=model,
        )

    head = messages[:KEEP_HEAD]
    batch = messages[KEEP_HEAD : KEEP_HEAD + batch_size]
    rest = messages[KEEP_HEAD + batch_size :]

    prior_summary: str | None = None
    if redis is not None and run_id:
        prior_summary = await _load_cached_summary(redis, run_id)

    summary_input = _format_messages_for_summary(batch)
    if prior_summary:
        summary_input = (
            f"[Previous summary]\n{prior_summary}\n\n"
            f"[New messages to incorporate]\n{summary_input}"
        )

    summary_messages = [
        {"role": "system", "content": SUMMARY_SYSTEM_PROMPT},
        {"role": "user", "content": summary_input},
    ]

    try:
        summary_text = await llm_client.planner_complete(summary_messages)
    except Exception as exc:
        logger.warning("micro_compact_llm_failed", extra={"error": str(exc)})
        return compact_history(messages, max_messages=KEEP_HEAD + len(rest)), True

    summary_msg = {
        "role": "system",
        "content": (
            f"[MICRO SUMMARY: {len(batch)} messages]\n\n{summary_text}"
        ),
    }

    compacted = head + [summary_msg] + rest

    if redis is not None and run_id:
        await _save_cached_summary(redis, run_id, summary_text)

    logger.info(
        "micro_compact_completed",
        extra={
            "batch_size": len(batch),
            "final_messages": len(compacted),
            "original_tokens": current_tokens,
            "final_tokens": estimate_messages_tokens(compacted),
        },
    )
    return compacted, True


# ────────────────────────────────────────────────────────────────────────────
# Redis helpers
# ────────────────────────────────────────────────────────────────────────────


async def _load_cached_summary(redis: Any, run_id: str) -> str | None:
    key = f"{_SUMMARY_KEY_PREFIX}{run_id}"
    try:
        raw = await redis.get(key)
        if raw:
            return raw.decode() if isinstance(raw, bytes) else raw
    except Exception as exc:
        logger.debug("compact_cache_load_error", extra={"error": str(exc)})
    return None


async def _save_cached_summary(redis: Any, run_id: str, summary: str) -> None:
    key = f"{_SUMMARY_KEY_PREFIX}{run_id}"
    try:
        await redis.setex(key, _SUMMARY_TTL, summary)
    except Exception as exc:
        logger.debug("compact_cache_save_error", extra={"error": str(exc)})


def _extract_summary_text(compacted: list[dict[str, Any]]) -> str | None:
    """Extract the last COMPACT SUMMARY message content from a compacted list."""
    for msg in reversed(compacted):
        content = msg.get("content", "")
        if "[COMPACT SUMMARY:" in content or "[MICRO SUMMARY:" in content:
            # Strip the marker prefix
            if "\n\n" in content:
                return content.split("\n\n", 1)[1].strip()
            return content.strip()
    return None


# ────────────────────────────────────────────────────────────────────────────
# LLM-based summarization (AUTO strategy)
# ────────────────────────────────────────────────────────────────────────────


async def _llm_summarize_compact(
    messages: list[dict[str, Any]],
    token_budget: int,
    llm_client: Any,
    prior_summary: str | None = None,
) -> list[dict[str, Any]]:
    """Use LLM to summarize old messages, keeping head and tail intact.

    Strategy:
    1. Keep head (system + first user) and tail (recent messages)
    2. Summarize the middle section via LLM
    3. Replace middle with a single summary message
    """
    if len(messages) <= KEEP_HEAD + 4:
        # Too few messages to meaningfully compress
        return messages

    # Determine how many tail messages to keep
    # Target: tail should be ~40% of budget
    tail_budget = int(token_budget * 0.4)
    tail_count = 0
    tail_tokens = 0
    for msg in reversed(messages[KEEP_HEAD:]):
        msg_tokens = len(msg.get("content", "")) / 3.8
        if tail_tokens + msg_tokens > tail_budget:
            break
        tail_tokens += msg_tokens
        tail_count += 1

    tail_count = max(4, min(tail_count, len(messages) - KEEP_HEAD))

    head = messages[:KEEP_HEAD]
    middle = messages[KEEP_HEAD : len(messages) - tail_count]
    tail = messages[len(messages) - tail_count :]

    if not middle:
        return messages

    # Format middle messages for summarization
    summary_input = _format_messages_for_summary(middle)
    if prior_summary:
        summary_input = (
            f"[Prior session summary]\n{prior_summary}\n\n"
            f"[Messages to summarize]\n{summary_input}"
        )

    # Ask LLM to summarize
    summary_messages = [
        {"role": "system", "content": SUMMARY_SYSTEM_PROMPT},
        {"role": "user", "content": summary_input},
    ]

    try:
        summary_text = await llm_client.planner_complete(summary_messages)
    except Exception as exc:
        logger.warning(
            "auto_compact_llm_failed",
            extra={"error": str(exc)},
        )
        # Fall back to SNIP
        return compact_history(messages, max_messages=KEEP_HEAD + tail_count + 1)

    summary_msg = {
        "role": "system",
        "content": (
            f"[COMPACT SUMMARY: {len(middle)} messages summarized]\n\n"
            f"{summary_text}"
        ),
    }

    compacted = head + [summary_msg] + tail

    logger.info(
        "auto_compact_completed",
        extra={
            "original_messages": len(messages),
            "summarized_count": len(middle),
            "final_messages": len(compacted),
            "original_tokens": estimate_messages_tokens(messages),
            "final_tokens": estimate_messages_tokens(compacted),
        },
    )

    return compacted


def _format_messages_for_summary(messages: list[dict[str, Any]]) -> str:
    """Format a list of messages into a text block for summarization."""
    parts: list[str] = []
    for msg in messages:
        role = msg.get("role", "unknown")
        content = msg.get("content", "")
        # Truncate very long individual messages
        if len(content) > 2000:
            content = content[:1900] + "... [truncated]"
        parts.append(f"[{role}]: {content}")
    return "\n\n".join(parts)


def _estimate_target_message_count(
    messages: list[dict[str, Any]],
    target_tokens: int,
) -> int:
    """Estimate how many messages fit in a target token count."""
    if not messages:
        return 0
    avg_tokens = estimate_messages_tokens(messages) / len(messages)
    if avg_tokens <= 0:
        return len(messages)
    return max(KEEP_HEAD + 4, int(target_tokens / avg_tokens))
