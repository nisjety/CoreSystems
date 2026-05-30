"""ai-core — FastAPI application entry point."""

from __future__ import annotations

import logging
from contextlib import asynccontextmanager

from fastapi import FastAPI

from app.config import get_settings
from app.middleware.auth import InternalAuthMiddleware

from reasoning_runtime import configure
from reasoning_runtime.config import RuntimeConfig

logger = logging.getLogger(__name__)


def _build_runtime_config() -> RuntimeConfig:
    """Bridge ai-core Settings → shared RuntimeConfig."""
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
        azure_speech_key=s.azure_speech_key,
        azure_speech_region=s.azure_speech_region,
        azure_translator_key=s.azure_translator_key,
        azure_translator_region=s.azure_translator_region,
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
    logger.info("ai_core_starting port=%d", settings.port)

    # Configure shared runtime
    configure(_build_runtime_config())

    # Connect NATS handler
    from app import nats_handler

    try:
        await nats_handler.connect()
        # Wire usage reporter to NATS connection
        from app.services import usage_reporter
        usage_reporter.init(nats_handler._nc)
        # Initialise inference metrics (optional Redis persistence)
        from app.services.inference_metrics import get_inference_metrics
        get_inference_metrics().init(settings.redis_url)
    except Exception:
        logger.warning("nats_connect_failed — running without NATS subscriptions")

    # Start gRPC server (non-fatal if grpc/stubs not installed)
    from app.grpc_server import create_grpc_server
    await create_grpc_server(port=settings.grpc_port)

    logger.info("ai_core_ready")
    yield

    # Shutdown
    from app.grpc_server import stop_grpc_server
    await stop_grpc_server()

    try:
        from app.services import usage_reporter
        await usage_reporter.close()
    except Exception:
        pass
    try:
        await nats_handler.close()
    except Exception:
        pass
    logger.info("ai_core_stopped")


def create_app() -> FastAPI:
    app = FastAPI(
        title="ai-core",
        version="2.0.0",
        lifespan=lifespan,
    )

    # Auth middleware
    settings = get_settings()
    if settings.internal_api_key:
        app.add_middleware(InternalAuthMiddleware)

    # Import and register routes
    from app.api import (
        analyze, chat, completions, documents, health, images, language,
        metrics, models, realtime, speech, translate, video,
    )

    app.include_router(health.router)
    app.include_router(chat.router)
    app.include_router(completions.router)
    app.include_router(images.router)
    app.include_router(speech.router)
    app.include_router(translate.router)
    app.include_router(documents.router)
    app.include_router(analyze.router)
    app.include_router(models.router)
    app.include_router(video.router)
    app.include_router(realtime.router)
    app.include_router(language.router)
    app.include_router(metrics.router)

    return app


app = create_app()
