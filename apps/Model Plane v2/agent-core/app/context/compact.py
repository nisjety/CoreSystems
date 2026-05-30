"""History compaction — SNIP-based message trimming.

Implements the CC pattern of trimming conversation history while preserving
edges (system prompt + first messages and most recent messages).

When message count exceeds the budget, middle messages are removed and
replaced with a SNIP boundary marker.
"""

from __future__ import annotations

import logging
from typing import Any

logger = logging.getLogger(__name__)

SNIP_MARKER = "[SNIP: {count} messages removed to stay within context budget]"

# Default: keep first 2 messages (system + initial user) and last N
KEEP_HEAD = 2
KEEP_TAIL_DEFAULT = 8


def compact_history(
    messages: list[dict[str, Any]],
    max_messages: int = 30,
    keep_tail: int = KEEP_TAIL_DEFAULT,
) -> list[dict[str, Any]]:
    """Trim middle messages if history exceeds max_messages.

    Preserves:
      - First KEEP_HEAD messages (system prompt + initial user message)
      - Last keep_tail messages (most recent context)

    Inserts a SNIP marker between head and tail sections.
    """
    if len(messages) <= max_messages:
        return messages

    # Calculate how many to remove
    head = messages[:KEEP_HEAD]
    tail = messages[-keep_tail:]
    removed_count = len(messages) - KEEP_HEAD - keep_tail

    if removed_count <= 0:
        return messages

    snip = {
        "role": "system",
        "content": SNIP_MARKER.format(count=removed_count),
    }

    compacted = head + [snip] + tail

    logger.debug(
        "history_compacted",
        extra={
            "original": len(messages),
            "compacted": len(compacted),
            "removed": removed_count,
        },
    )

    return compacted
