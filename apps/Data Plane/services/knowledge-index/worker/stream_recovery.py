from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True)
class ClaimedMessage:
    message_id: str
    fields: dict[str, Any]
    delivery_count: int
    previous_consumer: str | None
    idle_ms: int


async def claim_pending_messages(
    redis_client: Any,
    stream: str,
    group: str,
    consumer: str,
    min_idle_ms: int = 30_000,
    count: int = 100,
) -> list[ClaimedMessage]:
    """Claim idle pending messages and return them in XREADGROUP-compatible shape."""
    pending_info = await redis_client.xpending_range(
        name=stream,
        groupname=group,
        min="-",
        max="+",
        count=count,
    )
    if not pending_info:
        return []

    pending_metadata: dict[str, dict[str, Any]] = {}
    normalized_ids: list[str] = []
    for item in pending_info:
        idle_ms = int(item.get("time_since_delivered", 0))
        if idle_ms < min_idle_ms:
            continue
        raw_message_id = item["message_id"]
        message_id = (
            raw_message_id.decode() if isinstance(raw_message_id, bytes) else raw_message_id
        )
        raw_consumer = item.get("consumer")
        consumer = raw_consumer.decode() if isinstance(raw_consumer, bytes) else raw_consumer
        pending_metadata[message_id] = {
            "delivery_count": int(item.get("times_delivered", 1)),
            "previous_consumer": consumer,
            "idle_ms": idle_ms,
        }
        normalized_ids.append(message_id)

    if not normalized_ids:
        return []

    claimed = await redis_client.xclaim(
        name=stream,
        groupname=group,
        consumername=consumer,
        min_idle_time=min_idle_ms,
        message_ids=normalized_ids,
    )
    claimed_messages: list[ClaimedMessage] = []
    for message_id, fields in claimed or []:
        normalized_id = message_id.decode() if isinstance(message_id, bytes) else message_id
        metadata = pending_metadata.get(normalized_id, {})
        claimed_messages.append(
            ClaimedMessage(
                message_id=normalized_id,
                fields=fields,
                delivery_count=int(metadata.get("delivery_count", 1)),
                previous_consumer=metadata.get("previous_consumer"),
                idle_ms=int(metadata.get("idle_ms", min_idle_ms)),
            )
        )
    return claimed_messages