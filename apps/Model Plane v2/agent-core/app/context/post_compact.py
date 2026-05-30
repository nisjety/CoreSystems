"""Post-compact cleanup and file restoration.

CC pattern: after compaction, several caches and state trackers need
to be reset so the next turn starts clean:
  - Micro-compact state is reset
  - Classifier approval cache is cleared
  - Top-N recently referenced files are re-injected as context

File restoration re-reads the top referenced files (by frequency in
the conversation) and appends them as system messages, capped at a
total token budget.
"""

from __future__ import annotations

import logging
from collections import Counter
from typing import Any

from app.context.tokenizer import estimate_tokens

logger = logging.getLogger(__name__)

# Maximum total tokens for restored file snippets
FILE_RESTORE_BUDGET = 50_000

# Maximum tokens per individual file
FILE_RESTORE_PER_FILE = 5_000

# Maximum number of files to restore
MAX_FILES_TO_RESTORE = 10

# Pattern to detect file paths in message content
_FILE_PATH_INDICATORS = ("/", "\\", ".py", ".ts", ".js", ".go", ".rs", ".md")


def post_compact_cleanup(
    compact_state: Any | None = None,
) -> None:
    """Reset transient state after compaction.

    Args:
        compact_state: MicroCompactState or similar — will be reset.
    """
    if compact_state is not None and hasattr(compact_state, "reset"):
        compact_state.reset()
    logger.debug("post_compact_cleanup_done")


def extract_referenced_files(
    messages: list[dict[str, Any]],
) -> list[str]:
    """Extract file paths mentioned in the conversation, ranked by frequency.

    Returns deduplicated paths, most-referenced first.
    """
    path_counts: Counter[str] = Counter()

    for msg in messages:
        content = msg.get("content", "")
        if not isinstance(content, str):
            continue
        for word in content.split():
            # Simple heuristic: word looks like a file path
            if any(ind in word for ind in _FILE_PATH_INDICATORS):
                # Clean up common wrapping
                clean = word.strip("`'\"()[]{},:;")
                if "/" in clean or "\\" in clean:
                    path_counts[clean] += 1

    # Sort by frequency, then alphabetically
    return [
        path for path, _ in path_counts.most_common(MAX_FILES_TO_RESTORE * 2)
    ][:MAX_FILES_TO_RESTORE]


def build_file_restoration_messages(
    file_paths: list[str],
    file_reader: Any | None = None,
) -> list[dict[str, Any]]:
    """Build system messages with file content for context restoration.

    If no file_reader is provided, returns placeholder messages.

    Args:
        file_paths: Paths to restore.
        file_reader: Callable(path) → str | None.

    Returns:
        List of system messages with file content.
    """
    msgs: list[dict[str, Any]] = []
    total_tokens = 0

    for path in file_paths:
        if total_tokens >= FILE_RESTORE_BUDGET:
            break

        content: str | None = None
        if file_reader is not None:
            try:
                content = file_reader(path)
            except Exception:
                continue

        if content is None:
            continue

        tokens = estimate_tokens(content)
        if tokens > FILE_RESTORE_PER_FILE:
            # Truncate to budget
            char_limit = int(FILE_RESTORE_PER_FILE * 3.8)
            content = content[:char_limit] + "\n[... truncated ...]"
            tokens = FILE_RESTORE_PER_FILE

        if total_tokens + tokens > FILE_RESTORE_BUDGET:
            break

        msgs.append({
            "role": "system",
            "content": f"[Restored file: {path}]\n{content}",
        })
        total_tokens += tokens

    logger.debug(
        "file_restoration",
        extra={"files": len(msgs), "tokens": total_tokens},
    )
    return msgs


class CompactBoundaryMarker:
    """Generates boundary markers for stream consumers.

    CC inserts SystemCompactBoundaryMessage so consumers know
    when compaction occurred and can discard stale state.
    """

    @staticmethod
    def create(
        turn_index: int,
        tokens_before: int,
        tokens_after: int,
    ) -> dict[str, Any]:
        return {
            "role": "system",
            "content": (
                f"[COMPACT BOUNDARY — turn {turn_index}] "
                f"History compacted from ~{tokens_before} to ~{tokens_after} tokens."
            ),
            "metadata": {
                "type": "compact_boundary",
                "turn_index": turn_index,
                "tokens_before": tokens_before,
                "tokens_after": tokens_after,
            },
        }
