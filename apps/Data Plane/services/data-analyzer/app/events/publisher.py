"""Lazy singleton accessor for the shared cross-plane NATS publisher."""
from typing import Optional

from app.config import settings
from app.shared_nats import SharedNatsPublisher

_shared_nats: Optional[SharedNatsPublisher] = None


def get_shared_nats() -> SharedNatsPublisher:
    global _shared_nats
    if _shared_nats is None:
        _shared_nats = SharedNatsPublisher(
            nats_url=settings.nats_shared_url,
            nats_token=settings.nats_shared_token,
            service_name=settings.service_name,
        )
    return _shared_nats


async def close_shared_nats() -> None:
    global _shared_nats
    if _shared_nats:
        await _shared_nats.close()
        _shared_nats = None
