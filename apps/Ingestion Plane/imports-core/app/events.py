import json
from typing import Any

from nats.aio.client import Client as NATS

from app.config import get_settings


class EventPublisher:
    def __init__(self) -> None:
        self._client = NATS()
        self._connected = False
        self._settings = get_settings()

    async def connect(self) -> None:
        if self._connected:
            return
        options: dict[str, Any] = {"servers": [self._settings.nats_url]}
        if self._settings.nats_token:
            options["token"] = self._settings.nats_token
        await self._client.connect(**options)
        self._connected = True

    async def close(self) -> None:
        if self._connected:
            await self._client.close()
            self._connected = False

    async def publish(self, subject: str, payload: dict[str, Any]) -> None:
        if not self._connected:
            await self.connect()
        await self._client.publish(subject, json.dumps(payload).encode("utf-8"))


event_publisher = EventPublisher()
