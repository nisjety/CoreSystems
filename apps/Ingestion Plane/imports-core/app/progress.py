import asyncio
from collections import defaultdict
from collections.abc import AsyncGenerator
from typing import Any
from uuid import UUID


class ProgressHub:
    def __init__(self) -> None:
        self._subscribers: dict[UUID, set[asyncio.Queue[dict[str, Any]]]] = defaultdict(set)

    async def publish(self, job_id: UUID, event: dict[str, Any]) -> None:
        for queue in list(self._subscribers[job_id]):
            await queue.put(event)

    async def subscribe(self, job_id: UUID) -> AsyncGenerator[dict[str, Any], None]:
        queue: asyncio.Queue[dict[str, Any]] = asyncio.Queue()
        self._subscribers[job_id].add(queue)
        try:
            while True:
                payload = await queue.get()
                yield payload
                if payload.get("event") == "import.completed":
                    break
        finally:
            self._subscribers[job_id].discard(queue)
            if not self._subscribers[job_id]:
                self._subscribers.pop(job_id, None)


progress_hub = ProgressHub()
