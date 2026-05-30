"""NATS subscriber for agent mailbox — delivers to in-memory inbox."""

from __future__ import annotations

import json
import logging
from typing import Any

from nats.aio.subscription import Subscription

from app.messaging.domain import AgentMessage, InboxMessage
from app.messaging.inbox import get_inbox
from app.nats_client import NatsManager

logger = logging.getLogger(__name__)

_SUBJECT_PREFIX = "velion.agent.mailbox"

_subscription: Subscription | None = None


async def start_mailbox_listener(
    nats_mgr: NatsManager,
    run_id: str,
) -> None:
    """Subscribe to velion.agent.mailbox.{run_id} and route to the inbox."""
    global _subscription

    subject = f"{_SUBJECT_PREFIX}.{run_id}"

    async def _handler(msg: Any) -> None:
        try:
            raw = json.loads(msg.data.decode())
            agent_msg = AgentMessage(**raw)
            inbox_msg = InboxMessage(
                id=agent_msg.id,
                kind=agent_msg.kind,
                from_agent_id=agent_msg.from_agent_id,
                from_agent_name=agent_msg.from_agent_name,
                text=agent_msg.text,
                payload=agent_msg.payload,
                timestamp=agent_msg.timestamp,
            )
            inbox = get_inbox(run_id)
            await inbox.put(inbox_msg)
            logger.debug(
                "mailbox_received",
                extra={"run_id": run_id, "kind": agent_msg.kind.value},
            )
        except Exception:
            logger.exception("mailbox_handler_error")

    _subscription = await nats_mgr.cross.subscribe(subject, cb=_handler)
    logger.info("mailbox_listener_started", extra={"run_id": run_id, "subject": subject})


async def stop_mailbox_listener() -> None:
    """Unsubscribe from the mailbox subject."""
    global _subscription
    if _subscription is not None:
        await _subscription.unsubscribe()
        _subscription = None
        logger.info("mailbox_listener_stopped")
