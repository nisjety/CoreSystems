"""NATS command loop — subscribes to velion.session.{id}.command.

This is the primary command ingestion path for agent-core v2.
Session-core publishes commands; this loop receives and dispatches them.
"""

from __future__ import annotations

import asyncio
import json
import logging
import re
from typing import TYPE_CHECKING, Any, Callable, Coroutine

import nats
from nats.aio.msg import Msg

_UUID_RE = re.compile(
    r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
    re.IGNORECASE,
)

if TYPE_CHECKING:
    from app.nats_client import NatsManager

logger = logging.getLogger(__name__)

# Subject pattern: velion.session.*.command
COMMAND_SUBJECT = "velion.session.*.command"
DURABLE_NAME = "agent-core-v2-cmd"
STREAM_NAME = "VELION_SESSION"


class CommandLoop:
    """Subscribes to session commands and dispatches to handler callbacks.

    The loop uses a JetStream pull subscription with explicit ack.
    On failure the message is nak'd for redelivery.
    """

    def __init__(
        self,
        nats_mgr: NatsManager,
        handler: Callable[[str, dict[str, Any]], Coroutine[Any, Any, None]],
    ) -> None:
        self._nats = nats_mgr
        self._handler = handler
        self._task: asyncio.Task[None] | None = None
        self._stop = asyncio.Event()

    async def start(self) -> None:
        """Start the background pull loop."""
        self._stop.clear()
        self._task = asyncio.create_task(self._run(), name="nats-command-loop")
        logger.info("command_loop_started", extra={"subject": COMMAND_SUBJECT})

    async def stop(self) -> None:
        """Signal stop and wait for the loop to drain."""
        self._stop.set()
        if self._task:
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass
            self._task = None
        logger.info("command_loop_stopped")

    async def _run(self) -> None:
        sub = await self._nats.subscribe_jetstream(
            COMMAND_SUBJECT,
            durable=DURABLE_NAME,
            stream=STREAM_NAME,
        )

        while not self._stop.is_set():
            try:
                msgs: list[Msg] = await sub.fetch(batch=10, timeout=5)
            except nats.errors.TimeoutError:
                continue
            except Exception as exc:
                logger.error("command_loop_fetch_error", extra={"error": str(exc)})
                await asyncio.sleep(1)
                continue

            for msg in msgs:
                try:
                    payload = json.loads(msg.data.decode())
                    # Extract session_id from subject: velion.session.<id>.command
                    parts = msg.subject.split(".")
                    session_id = parts[2] if len(parts) >= 4 else "unknown"

                    # Reject messages with a malformed session_id — these cannot
                    # be corrected by redelivery so nak immediately (no delay).
                    if not _UUID_RE.match(session_id):
                        logger.warning(
                            "command_loop_invalid_session_id",
                            extra={"subject": msg.subject, "session_id": session_id},
                        )
                        await msg.nak()
                        continue

                    await self._handler(session_id, payload)
                    await msg.ack()
                except Exception as exc:
                    logger.error(
                        "command_handler_error",
                        extra={"subject": msg.subject, "error": str(exc)},
                    )
                    await msg.nak(delay=2)
