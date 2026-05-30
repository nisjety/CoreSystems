from __future__ import annotations

import json
from datetime import datetime, timezone
from typing import Any


DLQ_STREAM = "dataplane.worker.dlq"


def _truncate_fields(fields: dict[str, Any], max_length: int = 512) -> dict[str, Any]:
    truncated: dict[str, Any] = {}
    for key, value in fields.items():
        if isinstance(value, str) and len(value) > max_length:
            truncated[key] = f"{value[:max_length]}..."
        else:
            truncated[key] = value
    return truncated


async def move_to_dead_letter(
    redis_client: Any,
    *,
    source_stream: str,
    group: str,
    worker_name: str,
    message_id: str,
    fields: dict[str, Any],
    delivery_count: int,
    error: str,
) -> None:
    await redis_client.xadd(
        DLQ_STREAM,
        {
            "source_stream": source_stream,
            "group": group,
            "worker": worker_name,
            "original_message_id": message_id,
            "delivery_count": str(delivery_count),
            "last_error": error,
            "payload_json": json.dumps(_truncate_fields(fields), default=str, sort_keys=True),
            "moved_at": datetime.now(timezone.utc).isoformat(),
        },
    )