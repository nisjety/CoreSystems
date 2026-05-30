"""cost-core v2 FastAPI entrypoint."""
from __future__ import annotations

import logging
from contextlib import asynccontextmanager

from fastapi import FastAPI

from app import nats_client, nats_subscriber
from app.api import analytics, costs, health
from app.config import get_settings
from app.database import close_pool, get_pool, run_migrations

logger = logging.getLogger(__name__)


@asynccontextmanager
async def lifespan(app: FastAPI):
    settings = get_settings()
    logging.basicConfig(level=settings.log_level)

    await get_pool()
    if settings.run_migrations_on_startup:
        await run_migrations("migrations")

    try:
        await nats_subscriber.start()
    except Exception:
        logger.exception("nats subscriber failed to start; continuing")

    yield

    try:
        await nats_client.close()
    finally:
        await close_pool()


app = FastAPI(title="cost-core v2", version="0.1.0", lifespan=lifespan)
app.include_router(health.router)
app.include_router(costs.router)
app.include_router(analytics.router)
