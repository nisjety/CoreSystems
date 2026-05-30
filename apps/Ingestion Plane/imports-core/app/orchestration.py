import asyncio
import logging
from typing import Awaitable, Callable
from uuid import UUID

from temporalio.client import Client

from app.config import get_settings

logger = logging.getLogger(__name__)


class Orchestrator:
    def __init__(self) -> None:
        self._settings = get_settings()
        self._temporal_client: Client | None = None

    async def connect(self) -> None:
        if not self._settings.temporal_enabled or self._temporal_client is not None:
            return
        try:
            self._temporal_client = await Client.connect(
                self._settings.temporal_host_port,
                namespace=self._settings.temporal_namespace,
            )
            logger.info(
                "Connected to Temporal at %s (namespace: %s)",
                self._settings.temporal_host_port,
                self._settings.temporal_namespace,
            )
        except Exception as exc:
            logger.warning("Failed to connect to Temporal: %s", exc)
            self._temporal_client = None

    async def dispatch(
        self,
        job_id: UUID,
        runner: Callable[[UUID], Awaitable[None]],
    ) -> None:
        """Dispatch job to Temporal or execute locally"""
        if self._settings.temporal_enabled:
            await self.connect()
            if self._temporal_client:
                # Future: submit workflow to Temporal
                logger.debug("Dispatching job %s to Temporal", job_id)
                # Example: await self._temporal_client.start_workflow(...)
        
        # Execute as async task (always, or fallback if Temporal unavailable)
        asyncio.create_task(runner(job_id))

    async def close(self) -> None:
        if self._temporal_client:
            await self._temporal_client.close()
            self._temporal_client = None


orchestrator = Orchestrator()
