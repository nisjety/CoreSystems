"""Rich message protocol — CC-compatible message types for agent runs.

Provides a typed RunEvent envelope and MessageType taxonomy that mirrors
Claude Code's 15+ message types while staying backend/API-native.
"""

from app.messages.types import (
    MessageRole,
    MessageType,
    RunEvent,
    RunEventBatch,
    build_event,
)

__all__ = [
    "MessageRole",
    "MessageType",
    "RunEvent",
    "RunEventBatch",
    "build_event",
    "EventStream",
    "get_or_create_stream",
]
