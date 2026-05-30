"""In-memory mailbox inbox — asyncio.Queue per run_id."""

from __future__ import annotations

import asyncio
import logging
from typing import Any

from app.messaging.domain import InboxMessage

logger = logging.getLogger(__name__)

# run_id → asyncio.Queue[InboxMessage]
_inboxes: dict[str, asyncio.Queue[InboxMessage]] = {}


def get_inbox(run_id: str) -> asyncio.Queue[InboxMessage]:
    """Get or create the inbox queue for a run."""
    if run_id not in _inboxes:
        _inboxes[run_id] = asyncio.Queue(maxsize=1000)
    return _inboxes[run_id]


def drain_inbox(run_id: str) -> list[InboxMessage]:
    """Non-blocking drain of all messages currently in the inbox."""
    inbox = _inboxes.get(run_id)
    if inbox is None:
        return []
    messages: list[InboxMessage] = []
    while not inbox.empty():
        try:
            messages.append(inbox.get_nowait())
        except asyncio.QueueEmpty:
            break
    return messages


async def wait_for_message(
    run_id: str,
    timeout: float = 30.0,
) -> InboxMessage | None:
    """Block until a message arrives or timeout expires."""
    inbox = get_inbox(run_id)
    try:
        return await asyncio.wait_for(inbox.get(), timeout=timeout)
    except asyncio.TimeoutError:
        return None


def close_inbox(run_id: str) -> None:
    """Remove inbox for a completed run."""
    _inboxes.pop(run_id, None)
