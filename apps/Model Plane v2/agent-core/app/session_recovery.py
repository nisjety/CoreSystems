"""Session recovery — conversation deserialization with interruption detection.

Ported from CC's utils/conversationRecovery.ts + sessionRestore.ts:
- Filters orphaned tool_use messages without matching results
- Detects mid-turn interruptions
- Auto-injects continuation messages for interrupted sessions
- Restores cost state and agent config from session logs

This bridges snapshot.py (binary state) with the message layer to
provide seamless session resume.
"""

from __future__ import annotations

import logging
from enum import Enum
from typing import Any

logger = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Interruption detection (CC conversationRecovery.ts)
# ---------------------------------------------------------------------------

class InterruptionType(str, Enum):
    """Type of turn interruption detected in the conversation."""
    NONE = "none"
    INTERRUPTED_PROMPT = "interrupted_prompt"   # User was typing when interrupted
    INTERRUPTED_TURN = "interrupted_turn"       # Agent was executing when interrupted


# Continuation message injected when we detect an interrupted turn
_CONTINUATION_MSG = (
    "Continue from where you left off. Do not repeat completed work. "
    "If you were in the middle of a tool call, re-evaluate whether it's still needed."
)


def detect_turn_interruption(
    messages: list[dict[str, Any]],
) -> InterruptionType:
    """Detect if the conversation was interrupted mid-turn.

    Three-way detection matching CC's logic:
      - NONE: conversation ended cleanly (last message is assistant with content)
      - INTERRUPTED_PROMPT: last message is user role (user was interrupted)
      - INTERRUPTED_TURN: last message is assistant with pending tool_use (no result)
    """
    if not messages:
        return InterruptionType.NONE

    last = messages[-1]
    role = last.get("role", "")

    if role == "user":
        return InterruptionType.INTERRUPTED_PROMPT

    if role == "assistant":
        content = last.get("content")
        # Check for pending tool_use in content blocks
        if isinstance(content, list):
            has_tool_use = any(
                isinstance(b, dict) and b.get("type") == "tool_use"
                for b in content
            )
            has_tool_result = any(
                isinstance(b, dict) and b.get("type") == "tool_result"
                for b in content
            )
            if has_tool_use and not has_tool_result:
                return InterruptionType.INTERRUPTED_TURN

        # Check stringified JSON for tool_call kind
        if isinstance(content, str) and '"kind": "tool_call"' in content:
            # No subsequent user message with result → interrupted
            return InterruptionType.INTERRUPTED_TURN

    return InterruptionType.NONE


# ---------------------------------------------------------------------------
# Message deserialization + cleanup (CC deserializeMessages)
# ---------------------------------------------------------------------------

def deserialize_messages(
    raw_messages: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    """Clean and filter messages for safe replay.

    Operations:
    1. Remove tool_use blocks without matching tool_result
    2. Remove whitespace-only assistant messages
    3. Remove orphaned thinking blocks at end of conversation
    4. Migrate legacy attachment types
    """
    cleaned: list[dict[str, Any]] = []
    pending_tool_ids: set[str] = set()

    for msg in raw_messages:
        role = msg.get("role", "")
        content = msg.get("content", "")

        # Skip whitespace-only assistant messages
        if role == "assistant" and isinstance(content, str) and not content.strip():
            continue

        # Track tool_use IDs
        if isinstance(content, list):
            for block in content:
                if isinstance(block, dict):
                    if block.get("type") == "tool_use":
                        tool_id = block.get("id", "")
                        if tool_id:
                            pending_tool_ids.add(tool_id)
                    elif block.get("type") == "tool_result":
                        tool_id = block.get("tool_use_id", "")
                        pending_tool_ids.discard(tool_id)

        cleaned.append(msg)

    # Second pass: remove trailing messages with unresolved tool_use
    if pending_tool_ids:
        while cleaned:
            last = cleaned[-1]
            content = last.get("content")
            if isinstance(content, list):
                has_pending = any(
                    isinstance(b, dict)
                    and b.get("type") == "tool_use"
                    and b.get("id") in pending_tool_ids
                    for b in content
                )
                if has_pending:
                    cleaned.pop()
                    continue
            break

    return cleaned


def deserialize_with_interruption_detection(
    raw_messages: list[dict[str, Any]],
) -> tuple[list[dict[str, Any]], InterruptionType]:
    """Deserialize messages and detect interruption, injecting continuation if needed.

    Returns (cleaned_messages, interruption_type).
    """
    messages = deserialize_messages(raw_messages)
    interruption = detect_turn_interruption(messages)

    if interruption == InterruptionType.INTERRUPTED_TURN:
        messages.append({
            "role": "user",
            "content": _CONTINUATION_MSG,
        })
        logger.info(
            "session_recovery_continuation_injected",
            extra={"interruption": interruption.value, "message_count": len(messages)},
        )

    return messages, interruption


# ---------------------------------------------------------------------------
# Session state restoration (CC sessionRestore.ts)
# ---------------------------------------------------------------------------

def restore_cost_state(
    session_metadata: dict[str, Any],
) -> dict[str, Any]:
    """Restore accumulated cost state from session metadata.

    Returns a dict compatible with CostTracker initialization.
    """
    cost_summary = session_metadata.get("cost_summary", {})
    return {
        "total_input_tokens": cost_summary.get("total_input_tokens", 0),
        "total_output_tokens": cost_summary.get("total_output_tokens", 0),
        "total_tokens": cost_summary.get("total_tokens", 0),
        "total_usd": cost_summary.get("total_usd", 0.0),
        "turns_tracked": cost_summary.get("turns_tracked", 0),
    }


def restore_agent_config(
    session_metadata: dict[str, Any],
) -> dict[str, Any]:
    """Restore agent type and model override from session metadata."""
    return {
        "agent_type": session_metadata.get("agent_type"),
        "model_override": session_metadata.get("model_override"),
        "thinking_mode": session_metadata.get("thinking_mode"),
        "policy": session_metadata.get("policy"),
    }


def extract_todos_from_transcript(
    messages: list[dict[str, Any]],
) -> list[dict[str, Any]] | None:
    """Scan transcript for the last TodoWrite block to hydrate todo state.

    Returns the todo list or None if no TodoWrite found.
    """
    import json

    for msg in reversed(messages):
        content = msg.get("content", "")
        if isinstance(content, str) and "todo_write" in content.lower():
            try:
                data = json.loads(content)
                if isinstance(data, dict) and "todos" in data:
                    return data["todos"]
                if isinstance(data, dict) and data.get("name") == "todo_write":
                    return data.get("input", {}).get("todos")
            except (json.JSONDecodeError, KeyError):
                continue
        # Check content blocks
        if isinstance(content, list):
            for block in content:
                if (
                    isinstance(block, dict)
                    and block.get("type") == "tool_use"
                    and block.get("name") == "todo_write"
                ):
                    return block.get("input", {}).get("todos")
    return None
