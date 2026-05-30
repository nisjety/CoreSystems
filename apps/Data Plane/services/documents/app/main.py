"""Documents Service entry point for HTTP and gRPC transports."""
from __future__ import annotations

import asyncio
import logging
import sys
from contextlib import asynccontextmanager
from pathlib import Path

import uvicorn
from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse
from sqlalchemy import text
from starlette.middleware.base import RequestResponseEndpoint
from starlette.responses import Response

from app.api.v1.documents import router as documents_router
from app.api.internal.documents import router as internal_router
from app.config import settings
from app.control_plane_subscriber import ControlPlaneSubscriber
from app.ingestion_subscriber import IngestionSubscriber
from app.db.postgres import engine
from app.quota_enforcement_handler import get_quota_enforcement_handler, get_quota_blocker
from app.events.publisher import close_shared_nats
from app.events.publisher import get_redis
from app.grpc_server import create_grpc_server
from app.observability import instrument_app

# Add app root to path so /app/shared is importable in containers.
_app_root = Path(__file__).resolve().parent.parent
if str(_app_root) not in sys.path:
    sys.path.insert(0, str(_app_root))

from shared.auth_middleware import create_auth_middleware, close_auth_channel
from shared.authz import check_org_access, check_route_permission, close_org_channel

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s %(levelname)s %(name)s %(message)s",
)
logger = logging.getLogger(__name__)

# Global references to event subscribers
control_plane_subscriber: ControlPlaneSubscriber | None = None
ingestion_subscriber: IngestionSubscriber | None = None
startup_complete = False


@asynccontextmanager
async def lifespan(app: FastAPI):
    global startup_complete
    logger.info("documents-service starting on port %d", settings.service_port)
    # Initialize shared NATS on startup (lazy init via get_shared_nats in publisher)
    from app.events.publisher import get_shared_nats
    await get_shared_nats()

    # Initialize Control Plane subscriber (Phase 6)
    global control_plane_subscriber
    control_plane_subscriber = ControlPlaneSubscriber(
        settings.nats_shared_url,
        settings.nats_shared_token,
        "documents-service",
    )

    # Wire up quota enforcement handler
    quota_handler = get_quota_enforcement_handler()
    quota_blocker = get_quota_blocker()

    # On plan upgrade: unblock org first, then update DB quotas
    async def _on_plan_changed(org_id: str, plan: str) -> bool:
        quota_blocker.unblock(org_id)
        return await quota_handler.handle(org_id, plan)

    control_plane_subscriber.on_org_plan_changed = _on_plan_changed
    # On quota_exceeded: block org in-memory so document uploads return 429
    control_plane_subscriber.on_billing_quota_exceeded = quota_blocker.block

    # Start control plane subscriber
    if await control_plane_subscriber.initialize():
        logger.info("✅ Control Plane Event Subscriber initialized")
    else:
        logger.warning(
            "⚠️  Control Plane Event Subscriber unavailable (graceful degradation)"
        )

    # Start ingestion subscriber (Quarry crawl lifecycle events)
    global ingestion_subscriber
    ingestion_subscriber = IngestionSubscriber(
        settings.nats_shared_url,
        settings.nats_shared_token,
        "documents-service",
    )
    if await ingestion_subscriber.initialize():
        logger.info("✅ Ingestion Event Subscriber initialized")
    else:
        logger.warning(
            "⚠️  Ingestion Event Subscriber unavailable (graceful degradation)"
        )

    startup_complete = True

    yield

    # Cleanup on shutdown
    startup_complete = False
    if ingestion_subscriber:
        await ingestion_subscriber.close()
    if control_plane_subscriber:
        await control_plane_subscriber.close()
    await close_auth_channel()
    await close_org_channel()
    await close_shared_nats()
    logger.info("documents-service shutting down")


app = FastAPI(
    title="Data Plane — Documents Service",
    description="Ground truth layer. Ingest, store, and version raw knowledge.",
    version="1.0.0",
    lifespan=lifespan,
)

instrument_app(app, service_name="documents")

# ── Auth middleware (Phase 1) ─────────────────────────────────────────────────
# Validates Bearer tokens via auth-core gRPC. Skips /health, /readyz, /metrics.
if settings.internal_api_key:
    _auth_mw = create_auth_middleware(
        auth_grpc_url=settings.auth_core_grpc_url,
        internal_api_key=settings.internal_api_key,
        redis_getter=get_redis,
        cache_ttl=settings.auth_cache_ttl,
    )
    app.middleware("http")(_auth_mw)

    # Authz middleware — runs after auth, checks org membership + permissions
    @app.middleware("http")
    async def authz_middleware(request: Request, call_next: RequestResponseEndpoint) -> Response:
        from shared.auth_middleware import _is_public
        if _is_public(request.url.path):
            return await call_next(request)

        auth_ctx = getattr(request.state, "auth", None)
        if auth_ctx is None:
            # Auth middleware already rejected or it's an internal endpoint
            return await call_next(request)

        if not auth_ctx.org_id:
            return JSONResponse(
                status_code=403,
                content={"detail": "No active organization in session. Switch to an org first."},
            )

        redis_client = None
        try:
            redis_client = await get_redis()
        except Exception:
            pass

        access = await check_org_access(
            auth_ctx=auth_ctx,
            org_grpc_url=settings.org_core_grpc_url,
            internal_api_key=settings.internal_api_key,
            redis_client=redis_client,
        )

        request.state.org_access = access

        # Check route-level permission
        denial = check_route_permission(access, request.method, request.url.path)
        if denial:
            return JSONResponse(status_code=403, content={"detail": denial})

        return await call_next(request)
else:
    logger.warning(
        "⚠️  INTERNAL_API_KEY not set — running WITHOUT authentication (dev mode only)"
    )

app.include_router(documents_router)
app.include_router(internal_router)


@app.get("/health")
async def health() -> dict:
    return {"status": "ok", "service": "documents"}


@app.get("/readyz")
async def readyz() -> JSONResponse:
    checks = {"startup": startup_complete, "postgres": False, "redis": False}
    status_code = 200

    if startup_complete:
        try:
            async with engine.connect() as connection:
                await connection.execute(text("SELECT 1"))
            checks["postgres"] = True
        except Exception as exc:
            logger.warning("documents readyz postgres check failed: %s", exc)
            status_code = 503

        try:
            redis_client = await get_redis()
            checks["redis"] = bool(await redis_client.ping())
        except Exception as exc:
            logger.warning("documents readyz redis check failed: %s", exc)
            status_code = 503
    else:
        status_code = 503

    return JSONResponse(
        status_code=status_code,
        content={
            "status": "ready" if status_code == 200 else "not_ready",
            "service": "documents",
            "checks": checks,
        },
    )


async def run_servers() -> None:
    grpc_server = await create_grpc_server()
    await grpc_server.start()
    logger.info("documents-service gRPC listening on port %d", settings.grpc_port)

    http_server = uvicorn.Server(
        uvicorn.Config(
            app,
            host="0.0.0.0",
            port=settings.service_port,
            reload=False,
            log_level="info",
        )
    )

    try:
        await http_server.serve()
    finally:
        await grpc_server.stop(grace=5)
        logger.info("documents-service gRPC server stopped")


def main() -> None:
    asyncio.run(run_servers())


if __name__ == "__main__":
    main()
