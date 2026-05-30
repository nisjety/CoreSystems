"""NATS subscriber loop for runner lifecycle subjects.

Listens on:
  velion.runner.register       — new runner registration
  velion.runner.heartbeat.>    — periodic runner heartbeat
  velion.runner.claim          — agent-core assigns a task to the pool
  velion.runner.complete.>     — runner reports task completion
  velion.runner.cancel.>       — agent-core or user cancels a task
"""

from __future__ import annotations

import json
import logging
from typing import Any, Awaitable, Callable

from nats.aio.msg import Msg

from app.nats_client import NatsManager

logger = logging.getLogger(__name__)


class RunnerLoop:
    """NATS subscription loop for the runner lifecycle."""

    def __init__(
        self,
        nats_mgr: NatsManager,
        on_register: Callable[[dict[str, Any]], Awaitable[None]],
        on_heartbeat: Callable[[str, dict[str, Any]], Awaitable[None]],
        on_claim: Callable[[dict[str, Any]], Awaitable[None]],
        on_complete: Callable[[str, dict[str, Any]], Awaitable[None]],
        on_cancel: Callable[[str, dict[str, Any]], Awaitable[None]],
    ) -> None:
        self._nats = nats_mgr
        self._on_register = on_register
        self._on_heartbeat = on_heartbeat
        self._on_claim = on_claim
        self._on_complete = on_complete
        self._on_cancel = on_cancel
        self._subs: list[Any] = []

    async def start(self) -> None:
        nc = self._nats.cross

        sub = await nc.subscribe("velion.runner.register", cb=self._handle_register)
        self._subs.append(sub)

        sub = await nc.subscribe("velion.runner.heartbeat.>", cb=self._handle_heartbeat)
        self._subs.append(sub)

        sub = await nc.subscribe(
            "velion.runner.claim",
            queue="execution-core",
            cb=self._handle_claim,
        )
        self._subs.append(sub)

        sub = await nc.subscribe("velion.runner.complete.>", cb=self._handle_complete)
        self._subs.append(sub)

        sub = await nc.subscribe("velion.runner.cancel.>", cb=self._handle_cancel)
        self._subs.append(sub)

        logger.info("runner_loop_started", extra={"subs": 5})

    async def stop(self) -> None:
        for sub in self._subs:
            await sub.unsubscribe()
        self._subs.clear()
        logger.info("runner_loop_stopped")

    # -- handlers --

    async def _handle_register(self, msg: Msg) -> None:
        try:
            data = json.loads(msg.data.decode())
            await self._on_register(data)
        except Exception:
            logger.exception("register_handler_error")

    async def _handle_heartbeat(self, msg: Msg) -> None:
        try:
            subject = msg.subject
            parts = subject.split(".")
            runner_id = parts[3] if len(parts) > 3 else "unknown"
            data = json.loads(msg.data.decode())
            await self._on_heartbeat(runner_id, data)
        except Exception:
            logger.exception("heartbeat_handler_error")

    async def _handle_claim(self, msg: Msg) -> None:
        try:
            data = json.loads(msg.data.decode())
            await self._on_claim(data)
        except Exception:
            logger.exception("claim_handler_error")

    async def _handle_complete(self, msg: Msg) -> None:
        try:
            subject = msg.subject
            parts = subject.split(".")
            task_id = parts[3] if len(parts) > 3 else "unknown"
            data = json.loads(msg.data.decode())
            await self._on_complete(task_id, data)
        except Exception:
            logger.exception("complete_handler_error")

    async def _handle_cancel(self, msg: Msg) -> None:
        try:
            subject = msg.subject
            parts = subject.split(".")
            task_id = parts[3] if len(parts) > 3 else "unknown"
            data = json.loads(msg.data.decode())
            await self._on_cancel(task_id, data)
        except Exception:
            logger.exception("cancel_handler_error")
