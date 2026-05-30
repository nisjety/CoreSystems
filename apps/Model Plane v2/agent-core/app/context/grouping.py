"""Message grouping — group by API round for PTL truncation.

CC pattern: when prompt-too-long (PTL) errors occur, the compactor
groups messages by "API round" (assistant response + tool calls + results)
and drops the oldest groups first, retrying up to MAX_RETRIES.
"""

from __future__ import annotations

from typing import Any


# Maximum retries when PTL error occurs
MAX_PTL_RETRIES = 3


def group_by_round(messages: list[dict[str, Any]]) -> list[list[dict[str, Any]]]:
    """Group messages into API rounds.

    A round starts with an assistant message and includes all subsequent
    tool calls and results until the next assistant or user message.

    The first system message(s) are kept in their own group (never dropped).
    User messages also form their own group.
    """
    groups: list[list[dict[str, Any]]] = []
    current: list[dict[str, Any]] = []

    for msg in messages:
        role = msg.get("role", "")

        if role == "system" and not current:
            # System messages at the start → own group
            current.append(msg)
            continue

        if role in ("assistant", "user") and current:
            groups.append(current)
            current = []

        current.append(msg)

    if current:
        groups.append(current)

    return groups


def drop_oldest_groups(
    groups: list[list[dict[str, Any]]],
    keep_first: int = 1,
    drop_count: int = 1,
) -> list[dict[str, Any]]:
    """Drop the oldest N non-protected groups and flatten.

    The first ``keep_first`` groups are never dropped (typically system
    prompt and initial user message).

    Returns a flat message list.
    """
    if drop_count <= 0 or len(groups) <= keep_first:
        return _flatten(groups)

    protected = groups[:keep_first]
    droppable = groups[keep_first:]

    remaining = droppable[drop_count:]
    result = protected + remaining
    return _flatten(result)


def ptl_retry_truncate(
    messages: list[dict[str, Any]],
    max_retries: int = MAX_PTL_RETRIES,
) -> list[list[dict[str, Any]]]:
    """Generate progressively shorter message lists for PTL retry.

    Returns a list of message lists, each shorter than the previous,
    for the caller to try in order until the LLM accepts the input.
    """
    groups = group_by_round(messages)
    attempts: list[list[dict[str, Any]]] = []

    for i in range(max_retries):
        attempt = drop_oldest_groups(groups, keep_first=1, drop_count=i + 1)
        if attempt == messages and i > 0:
            break  # no more groups to drop
        attempts.append(attempt)

    return attempts


def _flatten(groups: list[list[dict[str, Any]]]) -> list[dict[str, Any]]:
    result: list[dict[str, Any]] = []
    for g in groups:
        result.extend(g)
    return result
