"""NATS publisher — emits runner lifecycle events."""

from __future__ import annotations

import json
import logging
from typing import Any

from app.nats_client import NatsManager

logger = logging.getLogger(__name__)


class RunnerPublisher:
    """Publishes events on velion.runner.* subjects."""

    def __init__(self, nats_mgr: NatsManager) -> None:
        self._nats = nats_mgr

    async def publish_task_assigned(self, task_id: str, runner_id: str, payload: dict[str, Any]) -> None:
        """Notify that a task has been assigned to a runner."""
        subject = f"velion.runner.assigned.{task_id}"
        await self._publish(subject, {
            "task_id": task_id,
            "runner_id": runner_id,
            **payload,
        })

    async def publish_task_result(self, task_id: str, result: dict[str, Any]) -> None:
        """Publish the result of a completed task."""
        subject = f"velion.runner.result.{task_id}"
        await self._publish(subject, result)

    async def publish_runner_deregistered(self, runner_id: str, reason: str) -> None:
        """Publish runner removal event."""
        subject = f"velion.runner.deregistered.{runner_id}"
        await self._publish(subject, {"runner_id": runner_id, "reason": reason})

    async def publish_runner_dead(self, runner_id: str) -> None:
        """Publish that a runner has been marked dead (missed heartbeats)."""
        subject = f"velion.runner.dead.{runner_id}"
        await self._publish(subject, {"runner_id": runner_id})

    async def _publish(self, subject: str, data: dict[str, Any]) -> None:
        try:
            payload = json.dumps(data).encode()
            await self._nats.cross.publish(subject, payload)
            logger.debug("nats_published", extra={"subject": subject})
        except Exception:
            logger.exception("nats_publish_failed", extra={"subject": subject})
