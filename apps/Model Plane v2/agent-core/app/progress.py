"""Streaming action progress — partial progress events via NATS.

Mirrors CC's BashProgress/TaskOutputProgress pattern:
current binary events (action.started / action.completed) become
a stream of partial progress updates during long-running actions.

Progress events are published on:
  velion.agent.run.{run_id}.progress

Event types:
  - action.progress: partial output (e.g. streaming bash output)
  - action.heartbeat: still alive, no new output
"""

from __future__ import annotations

import logging
import time
from dataclasses import dataclass, field
from typing import Any

from app.domain import AgentEvent

logger = logging.getLogger(__name__)


@dataclass
class ActionProgress:
    """A single progress update for a running action."""

    run_id: str
    action_id: str
    action_name: str
    progress_type: str = "output"  # output | heartbeat | status_change
    content: str = ""
    percentage: float | None = None  # 0.0 - 1.0 if deterministic
    bytes_processed: int = 0
    query_depth: int | None = None
    timestamp: float = field(default_factory=time.time)

    def to_event(self, session_id: str, sequence: int = 0) -> AgentEvent:
        """Convert to a publishable NATS AgentEvent."""
        return AgentEvent(
            event_type="action.progress",
            run_id=self.run_id,
            session_id=session_id,
            sequence=sequence,
            payload={
                "action_id": self.action_id,
                "action_name": self.action_name,
                "progress_type": self.progress_type,
                "content": self.content,
                "percentage": self.percentage,
                "bytes_processed": self.bytes_processed,
                "query_depth": self.query_depth,
            },
        )


class ProgressStream:
    """Streaming progress emitter for a single action.

    Usage:
        stream = ProgressStream(publisher, run_id, session_id, action_id, action_name)
        await stream.emit("Processing file 1/10...")
        await stream.emit("Processing file 2/10...", percentage=0.2)
        await stream.heartbeat()
        await stream.complete("All files processed")
    """

    def __init__(
        self,
        publisher: Any,
        run_id: str,
        session_id: str,
        action_id: str,
        action_name: str,
        query_depth: int | None = None,
    ) -> None:
        self._publisher = publisher
        self._run_id = run_id
        self._session_id = session_id
        self._action_id = action_id
        self._action_name = action_name
        self._query_depth = query_depth
        self._seq = 0
        self._started_at = time.time()
        self._bytes_processed = 0

    async def emit(
        self,
        content: str,
        percentage: float | None = None,
        extra_bytes: int = 0,
    ) -> None:
        """Emit a progress update with content."""
        self._bytes_processed += extra_bytes
        self._seq += 1

        progress = ActionProgress(
            run_id=self._run_id,
            action_id=self._action_id,
            action_name=self._action_name,
            progress_type="output",
            content=content,
            percentage=percentage,
            bytes_processed=self._bytes_processed,
            query_depth=self._query_depth,
        )

        event = progress.to_event(self._session_id, self._seq)
        await self._publisher.publish(event)

    async def heartbeat(self) -> None:
        """Emit a heartbeat (still alive, no new content)."""
        self._seq += 1
        elapsed = time.time() - self._started_at

        progress = ActionProgress(
            run_id=self._run_id,
            action_id=self._action_id,
            action_name=self._action_name,
            progress_type="heartbeat",
            content=f"Running for {elapsed:.1f}s",
            bytes_processed=self._bytes_processed,
            query_depth=self._query_depth,
        )

        event = progress.to_event(self._session_id, self._seq)
        await self._publisher.publish(event)

    async def status_change(self, status: str) -> None:
        """Emit a status change event (e.g. 'compiling', 'testing')."""
        self._seq += 1

        progress = ActionProgress(
            run_id=self._run_id,
            action_id=self._action_id,
            action_name=self._action_name,
            progress_type="status_change",
            content=status,
            bytes_processed=self._bytes_processed,
            query_depth=self._query_depth,
        )

        event = progress.to_event(self._session_id, self._seq)
        await self._publisher.publish(event)


def create_progress_stream(
    publisher: Any,
    run_id: str,
    session_id: str,
    action_id: str,
    action_name: str,
    query_depth: int | None = None,
) -> ProgressStream:
    """Factory for creating a progress stream for an action."""
    return ProgressStream(
        publisher=publisher,
        run_id=run_id,
        session_id=session_id,
        action_id=action_id,
        action_name=action_name,
        query_depth=query_depth,
    )
