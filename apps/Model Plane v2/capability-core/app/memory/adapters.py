"""Memory adapter catalog — lists available adapter types and their schemas."""

from __future__ import annotations

from app import repository
from app.domain import MemoryAdapterEntry


async def list_adapters() -> list[MemoryAdapterEntry]:
    return await repository.list_memory_adapters()
