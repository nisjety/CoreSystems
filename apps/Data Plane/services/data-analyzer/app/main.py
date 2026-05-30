"""data-analyzer FastAPI application entry point."""
import logging
from contextlib import asynccontextmanager

from fastapi import FastAPI

from app.config import settings
from app.events.publisher import close_shared_nats, get_shared_nats
from app.routers import analyze

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)


@asynccontextmanager
async def lifespan(app: FastAPI):
    logger.info("🚀 %s starting on port %s", settings.service_name, settings.service_port)
    shared = get_shared_nats()
    await shared.initialize()
    yield
    logger.info("🛑 %s shutting down", settings.service_name)
    await close_shared_nats()


app = FastAPI(
    title="data-analyzer",
    version="0.1.0",
    lifespan=lifespan,
)

app.include_router(analyze.router)


@app.get("/health")
async def health():
    return {"status": "ok", "service": settings.service_name}
