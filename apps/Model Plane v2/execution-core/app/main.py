"""FastAPI application for execution-core.

Startup:  connect Postgres → run migrations → connect Redis → connect NATS
          → wire RunnerService → start NATS loop → start dead-runner reaper
Shutdown: stop reaper → stop loop → close NATS → close Redis → close Postgres
"""

from __future__ import annotations

import asyncio
import logging
from contextlib import asynccontextmanager
from typing import AsyncGenerator

import uvicorn
from fastapi import FastAPI

from app.api.artifacts import init as init_artifacts
from app.api.artifacts import router as artifacts_router
from app.api.health import router as health_router
from app.api.runners import init as init_runners
from app.api.runners import router as runners_router
from app.api.tasks import init as init_tasks
from app.api.tasks import router as tasks_router
from app.artifact_store import ensure_bucket
from app.config import settings
from app.database import close_pool, get_pool, run_migrations
from app.middleware import InternalAuthMiddleware
from app.nats_client import NatsManager
from app.nats_loop import RunnerLoop
from app.nats_publisher import RunnerPublisher
from app.redis_client import close_redis, get_redis
from app.runner_service import RunnerService

logging.basicConfig(
    level=logging.DEBUG if settings.debug else logging.INFO,
    format="%(asctime)s %(levelname)s %(name)s %(message)s",
)
logger = logging.getLogger(__name__)

# Module-level singletons (populated in lifespan)
nats_mgr: NatsManager | None = None
runner_service: RunnerService | None = None
runner_loop: RunnerLoop | None = None
_reaper_task: asyncio.Task | None = None


async def _dead_runner_reaper(service: RunnerService) -> None:
    """Periodic background task to mark dead runners."""
    interval = max(settings.runner_heartbeat_interval, 30)
    while True:
        try:
            await asyncio.sleep(interval)
            count = await service.sweep_dead_runners(stale_seconds=settings.runner_lease_ttl)
            if count:
                logger.info("reaped_dead_runners", extra={"count": count})
        except asyncio.CancelledError:
            break
        except Exception:
            logger.exception("dead_runner_reaper_error")


@asynccontextmanager
async def lifespan(app: FastAPI) -> AsyncGenerator[None, None]:
    """Application lifecycle: startup -> yield -> shutdown."""
    global nats_mgr, runner_service, runner_loop, _reaper_task

    logger.info("starting execution-core", extra={"port": settings.port})

    # 1. Postgres
    await get_pool()
    await run_migrations()

    # 2. Redis
    await get_redis()

    # 3. NATS
    nats_mgr = NatsManager()
    await nats_mgr.connect()

    # 4. Ensure object-storage bucket
    try:
        ensure_bucket()
    except Exception:
        logger.warning("object_storage_bucket_init_failed — presigned URLs may not work")

    # 5. Wire up RunnerService
    publisher = RunnerPublisher(nats_mgr)
    runner_service = RunnerService(publisher=publisher)

    # Inject service into route modules
    init_runners(runner_service)
    init_tasks(runner_service)
    init_artifacts(runner_service)

    # 6. Start NATS runner loop
    runner_loop = RunnerLoop(
        nats_mgr=nats_mgr,
        on_register=runner_service.handle_register,
        on_heartbeat=runner_service.handle_heartbeat,
        on_claim=runner_service.handle_claim,
        on_complete=runner_service.handle_complete,
        on_cancel=runner_service.handle_cancel,
    )
    await runner_loop.start()

    # 7. Start dead-runner reaper
    _reaper_task = asyncio.create_task(_dead_runner_reaper(runner_service))

    logger.info("execution-core ready")
    yield

    # ---- Shutdown ----
    logger.info("shutting down execution-core")

    if _reaper_task:
        _reaper_task.cancel()
        try:
            await _reaper_task
        except asyncio.CancelledError:
            pass

    if runner_loop:
        await runner_loop.stop()
    if nats_mgr:
        await nats_mgr.close()
    await close_redis()
    await close_pool()

    logger.info("execution-core stopped")


def create_app() -> FastAPI:
    """Factory for the FastAPI application."""
    app = FastAPI(
        title="Execution Core",
        version=settings.service_version,
        lifespan=lifespan,
    )
    app.add_middleware(InternalAuthMiddleware)
    app.include_router(health_router)
    app.include_router(runners_router)
    app.include_router(tasks_router)
    app.include_router(artifacts_router)
    return app


app = create_app()


if __name__ == "__main__":
    uvicorn.run(
        "app.main:app",
        host=settings.host,
        port=settings.port,
        log_level="debug" if settings.debug else "info",
    )
