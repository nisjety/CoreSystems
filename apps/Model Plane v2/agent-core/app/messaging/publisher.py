"""NATS publisher for inter-agent messages."""

from __future__ import annotations

import json
import logging

from app.messaging.domain import AgentMessage
from app.nats_client import NatsManager

logger = logging.getLogger(__name__)

# NATS subject pattern: velion.agent.mailbox.{target_run_id}
_SUBJECT_PREFIX = "velion.agent.mailbox"


async def publish_message(
    nats: NatsManager,
    message: AgentMessage,
) -> None:
    """Publish a message to the target run's mailbox subject."""
    subject = f"{_SUBJECT_PREFIX}.{message.to_run_id}"
    payload = message.model_dump()
    # Serialize datetimes for JSON
    payload["timestamp"] = payload["timestamp"].isoformat() if hasattr(payload["timestamp"], "isoformat") else str(payload["timestamp"])
    try:
        await nats.publish_jetstream(subject, payload)
        logger.info(
            "message_published",
            extra={
                "kind": message.kind.value,
                "to_run_id": message.to_run_id,
                "from_agent_id": message.from_agent_id,
            },
        )
    except Exception:
        logger.exception("message_publish_failed", extra={"to_run_id": message.to_run_id})
        raise
