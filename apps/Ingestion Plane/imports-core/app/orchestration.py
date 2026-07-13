import asyncio
import logging
from typing import Awaitable, Callable
from uuid import UUID

from app.config import get_settings

logger = logging.getLogger(__name__)


class Orchestrator:
    def __init__(self) -> None:
        self._settings = get_settings()
        self._warned_legacy_temporal = False

    async def connect(self) -> None:
        """Compatibility no-op; durable execution is backed by Postgres leases."""

    async def dispatch(
        self,
        job_id: UUID,
        runner: Callable[[UUID], Awaitable[None]],
    ) -> None:
        """Run a durable Postgres-backed job lease in this process."""
        if self._settings.temporal_enabled and not self._warned_legacy_temporal:
            logger.warning(
                "TEMPORAL_ENABLED is deprecated for imports-core; using durable Postgres leases"
            )
            self._warned_legacy_temporal = True
        asyncio.create_task(runner(job_id))

    async def close(self) -> None:
        return None


orchestrator = Orchestrator()
