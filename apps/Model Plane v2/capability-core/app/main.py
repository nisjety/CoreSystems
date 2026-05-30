"""capability-core — FastAPI application entry point."""

from __future__ import annotations

import logging
from contextlib import asynccontextmanager

from fastapi import FastAPI

from app.config import get_settings
from app.database import close_pool, init_pool, run_migrations
from app.middleware import InternalAuthMiddleware
from app.nats_client import close_nats, connect_nats
from app.redis_client import close_redis, init_redis

from app.api.health import router as health_router
from app.api.catalog import router as catalog_router
from app.api.plugins import router as plugins_router
from app.api.mcp import router as mcp_router
from app.api.routing import router as routing_router
from app.api.models import router as models_router
from app.api.memory import router as memory_router
from app.api.safety import router as safety_router

logger = logging.getLogger(__name__)


@asynccontextmanager
async def lifespan(app: FastAPI):
    settings = get_settings()

    logging.basicConfig(
        level=logging.DEBUG if settings.debug else logging.INFO,
        format="%(asctime)s %(levelname)-8s %(name)s  %(message)s",
    )

    logger.info("capability-core starting  port=%d", settings.port)

    # ── startup ──
    await init_pool()
    await run_migrations()
    await init_redis()
    await connect_nats()

    logger.info("capability-core ready")

    yield

    # ── shutdown ──
    from app.safety import close as close_safety
    await close_safety()
    await close_nats()
    await close_redis()
    await close_pool()
    logger.info("capability-core stopped")


def create_app() -> FastAPI:
    settings = get_settings()

    app = FastAPI(
        title="Capability Core",
        version=settings.service_version,
        lifespan=lifespan,
    )

    app.add_middleware(InternalAuthMiddleware)

    app.include_router(health_router)
    app.include_router(catalog_router)
    app.include_router(plugins_router)
    app.include_router(mcp_router)
    app.include_router(routing_router)
    app.include_router(models_router)
    app.include_router(memory_router)
    app.include_router(safety_router)

    return app


app = create_app()
