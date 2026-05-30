"""Per-tool micro-compaction — selectively compact tool results.

CC pattern: not all tool outputs are equally valuable long-term.
Large read-only results (file reads, grep results, web fetches) can be
summarised, while destructive tool results (bash, file edit) are kept
verbatim to preserve the audit trail.

Provides:
  - ``COMPACTABLE_TOOLS`` — set of tool names whose results can be shortened
  - ``micro_compact_messages()`` — replace large tool results with summaries
  - ``MicroCompactState`` — tracks which messages have been compacted
"""

from __future__ import annotations

import logging
from typing import Any

from pydantic import BaseModel, Field

from app.context.tokenizer import estimate_tokens

logger = logging.getLogger(__name__)

# Tools whose output can be safely summarised (read-only, large output)
COMPACTABLE_TOOLS = frozenset({
    "FileRead",
    "Grep",
    "Glob",
    "WebSearch",
    "WebFetch",
    "ToolSearch",
})

# Don't compact results shorter than this (token estimate)
MIN_TOKENS_TO_COMPACT = 200

# After compaction, truncate to this many tokens
COMPACT_TARGET_TOKENS = 80

# Maximum characters to keep in a truncated result
MAX_COMPACT_CHARS = 300


class MicroCompactState(BaseModel):
    """Tracks which messages in the history have been micro-compacted."""

    compacted_ids: set[int] = Field(default_factory=set)
    total_tokens_saved: int = 0

    def mark_compacted(self, msg_index: int, tokens_saved: int) -> None:
        self.compacted_ids.add(msg_index)
        self.total_tokens_saved += tokens_saved

    def is_compacted(self, msg_index: int) -> bool:
        return msg_index in self.compacted_ids

    def reset(self) -> None:
        self.compacted_ids.clear()
        self.total_tokens_saved = 0


def micro_compact_messages(
    messages: list[dict[str, Any]],
    state: MicroCompactState | None = None,
) -> tuple[list[dict[str, Any]], MicroCompactState]:
    """Selectively shorten large tool-result messages.

    Returns a new message list (immutable) and updated state.
    """
    if state is None:
        state = MicroCompactState()

    result: list[dict[str, Any]] = []

    for idx, msg in enumerate(messages):
        if state.is_compacted(idx):
            result.append(msg)
            continue

        tool_name = msg.get("name", "") or msg.get("tool_name", "")
        content = msg.get("content", "")

        if (
            msg.get("role") == "tool"
            and tool_name in COMPACTABLE_TOOLS
            and estimate_tokens(content) >= MIN_TOKENS_TO_COMPACT
        ):
            truncated = _truncate_tool_result(content, tool_name)
            tokens_saved = estimate_tokens(content) - estimate_tokens(truncated)
            new_msg = {**msg, "content": truncated}
            result.append(new_msg)
            state.mark_compacted(idx, tokens_saved)
            logger.debug(
                "micro_compacted",
                extra={"tool": tool_name, "idx": idx, "saved": tokens_saved},
            )
        else:
            result.append(msg)

    return result, state


def _truncate_tool_result(content: str, tool_name: str) -> str:
    """Truncate a tool result, keeping the head and a summary marker."""
    head = content[:MAX_COMPACT_CHARS].rstrip()
    line_count = content.count("\n")
    return (
        f"{head}\n\n"
        f"[... {tool_name} result truncated — "
        f"~{line_count} lines, ~{estimate_tokens(content)} tokens ...]"
    )


def strip_images_from_messages(
    messages: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    """Remove image content parts before summarization.

    CC strips base64 images before sending to the summarizer to
    reduce token usage and avoid confusing the summary.
    """
    result: list[dict[str, Any]] = []
    for msg in messages:
        content = msg.get("content")
        if isinstance(content, list):
            # Multi-part content — filter out image parts
            text_parts = [
                part for part in content
                if not (isinstance(part, dict) and part.get("type") == "image_url")
            ]
            if text_parts:
                result.append({**msg, "content": text_parts})
            # else: skip entirely image-only messages
        else:
            result.append(msg)
    return result
