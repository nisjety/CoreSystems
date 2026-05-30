"""LLM Worker — FastAPI application entry point.

DEPRECATED: llm-worker is now a thin wrapper around shared/reasoning_runtime.
New consumers should use:
  - ai-core (HTTP gateway + 10-layer pipeline) for external access
  - reasoning_runtime.execute() directly for internal service-to-service calls
  - velion.ai.chat.request NATS subject for async AI requests

llm-worker will be retired once all dependent services migrate.
"""

from __future__ import annotations

import logging
from contextlib import asynccontextmanager

from fastapi import FastAPI

from app import nats_client, redis_client, result_store
from app.api import complete, health, images, speech, translate
from app.config import get_settings
from app.providers import image_provider, speech_provider
from app.middleware import InternalAuthMiddleware

from reasoning_runtime import configure
from reasoning_runtime.config import RuntimeConfig

logger = logging.getLogger(__name__)


def _build_runtime_config() -> RuntimeConfig:
    """Bridge llm-worker Settings → shared RuntimeConfig."""
    s = get_settings()
    return RuntimeConfig(
        openai_api_key=s.openai_api_key,
        anthropic_api_key=s.anthropic_api_key,
        google_api_key=s.google_api_key,
        cohere_api_key=s.cohere_api_key,
        mistral_api_key=s.mistral_api_key,
        ollama_base_url=s.ollama_base_url,
        azure_openai_endpoint=s.azure_openai_endpoint,
        azure_openai_api_key=s.azure_openai_api_key,
        azure_openai_api_version=s.azure_openai_api_version,
        redis_url=s.redis_url,
        object_storage_endpoint=s.object_storage_endpoint,
        object_storage_access_key=s.object_storage_access_key,
        object_storage_secret_key=s.object_storage_secret_key,
        object_storage_bucket=s.object_storage_bucket,
        object_storage_region=s.object_storage_region,
        rate_limit_rpm=s.rate_limit_rpm,
        result_store_threshold_bytes=s.result_store_threshold_bytes,
    )


@asynccontextmanager
async def lifespan(app: FastAPI):
    settings = get_settings()

    # Startup
    logging.basicConfig(
        level=logging.DEBUG if settings.debug else logging.INFO,
        format="%(asctime)s %(levelname)s %(name)s %(message)s",
    )
    logger.info("llm_worker_starting port=8005")

    # Configure shared runtime
    configure(_build_runtime_config())

    await redis_client.init()
    result_store.init()
    await nats_client.connect()

    logger.info("llm_worker_ready")
    yield

    # Shutdown
    await speech_provider.close()
    await image_provider.close()
    await nats_client.close()
    await redis_client.close()
    logger.info("llm_worker_stopped")


def create_app() -> FastAPI:
    app = FastAPI(
        title="llm-worker",
        version="2.0.0",
        lifespan=lifespan,
    )

    # Auth middleware
    settings = get_settings()
    if settings.internal_api_key:
        app.add_middleware(InternalAuthMiddleware)

    # Routes
    app.include_router(health.router)
    app.include_router(complete.router)
    app.include_router(speech.router)
    app.include_router(images.router)
    app.include_router(translate.router)

    return app


app = create_app()
