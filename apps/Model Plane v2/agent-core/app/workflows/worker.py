"""Temporal worker — connects to the Temporal server and polls for tasks.

Started as a background asyncio task in the FastAPI lifespan.
Uses the ``pydantic_data_converter`` so Pydantic models are natively
serialised/deserialised as Temporal payloads.
"""

from __future__ import annotations

import asyncio
import logging
from typing import Any

from app.config import settings

logger = logging.getLogger(__name__)

_worker_task: asyncio.Task[Any] | None = None
_worker_instance: Any = None  # temporalio.worker.Worker


async def start_temporal_worker() -> None:
    """Connect to Temporal and start the worker in a background task.

    No-op if ``settings.temporal_enabled`` is False.
    """
    global _worker_task, _worker_instance

    if not settings.temporal_enabled:
        logger.info("temporal_disabled — skipping worker startup")
        return

    try:
        from temporalio.client import Client
        from temporalio.contrib.pydantic import pydantic_data_converter
        from temporalio.worker import Worker

        from app.workflows.activities import (
            checkpoint_run,
            execute_action,
            finalize_run,
            plan_actions,
        )
        from app.workflows.agent_workflow import AgentRunWorkflow

        client = await Client.connect(
            settings.temporal_host,
            namespace=settings.temporal_namespace,
            data_converter=pydantic_data_converter,
        )

        _worker_instance = Worker(
            client,
            task_queue=settings.temporal_task_queue,
            workflows=[AgentRunWorkflow],
            activities=[
                plan_actions,
                execute_action,
                checkpoint_run,
                finalize_run,
            ],
        )

        _worker_task = asyncio.create_task(
            _worker_instance.run(),
            name="temporal-worker",
        )
        logger.info(
            "temporal_worker_started",
            extra={
                "host": settings.temporal_host,
                "namespace": settings.temporal_namespace,
                "task_queue": settings.temporal_task_queue,
            },
        )

    except ImportError:
        logger.warning(
            "temporalio not installed — Temporal worker disabled. "
            "Install with: pip install temporalio"
        )
    except Exception as exc:
        logger.error(
            "temporal_worker_start_failed",
            extra={"error": str(exc)},
            exc_info=True,
        )


async def stop_temporal_worker() -> None:
    """Gracefully shut down the Temporal worker."""
    global _worker_task, _worker_instance

    if _worker_instance is not None:
        try:
            await _worker_instance.shutdown()
            logger.info("temporal_worker_shutdown_requested")
        except Exception as exc:
            logger.warning(
                "temporal_worker_shutdown_error",
                extra={"error": str(exc)},
            )

    if _worker_task is not None and not _worker_task.done():
        _worker_task.cancel()
        try:
            await _worker_task
        except asyncio.CancelledError:
            pass
        logger.info("temporal_worker_stopped")

    _worker_task = None
    _worker_instance = None


async def get_temporal_client() -> Any:
    """Get a Temporal client for starting workflows from HTTP handlers.

    Returns None if Temporal is disabled.
    """
    if not settings.temporal_enabled:
        return None

    try:
        from temporalio.client import Client
        from temporalio.contrib.pydantic import pydantic_data_converter

        return await Client.connect(
            settings.temporal_host,
            namespace=settings.temporal_namespace,
            data_converter=pydantic_data_converter,
        )
    except Exception as exc:
        logger.error("temporal_client_connect_failed", extra={"error": str(exc)})
        return None
