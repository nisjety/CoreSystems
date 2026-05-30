"""Retrieval Service entry point for HTTP and gRPC transports."""
from __future__ import annotations

import asyncio
import logging
import sys
from contextlib import asynccontextmanager
from pathlib import Path

import uvicorn
from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse
from starlette.middleware.base import RequestResponseEndpoint
from starlette.responses import Response

from app.api.v1.retrieval import router as retrieval_router
from app.config import settings
from app.control_plane_subscriber import ControlPlaneSubscriber
from app.quota_enforcement_handler import get_quota_enforcement_handler, get_quota_blocker
from app.events.publisher import close_shared_nats, get_redis
from app.grpc_server import create_grpc_server
from app.observability import instrument_app
from app.retrieval.rerank import close_rerank_http_client
from app.retrieval.vector_search import close_embedding_http_client
from app.retrieval.vector_search import get_qdrant

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

# Global reference to control plane subscriber
control_plane_subscriber: ControlPlaneSubscriber | None = None
startup_complete = False


@asynccontextmanager
async def lifespan(app: FastAPI):
    global control_plane_subscriber, startup_complete
    logger.info("retrieval-service starting on port %d", settings.service_port)
    # Initialize shared NATS on startup (lazy init via get_shared_nats in publisher)
    from app.events.publisher import get_shared_nats
    await get_shared_nats()

    # Initialize Control Plane subscriber (Phase 6)
    control_plane_subscriber = ControlPlaneSubscriber(
        settings.nats_shared_url,
        settings.nats_shared_token,
        "retrieval-service",
    )

    # Wire up quota enforcement handler
    quota_handler = get_quota_enforcement_handler()
    quota_blocker = get_quota_blocker()

    # On plan upgrade: unblock org first, then update DB quotas
    async def _on_plan_changed(org_id: str, plan: str) -> bool:
        quota_blocker.unblock(org_id)
        return await quota_handler.handle(org_id, plan)

    control_plane_subscriber.on_org_plan_changed = _on_plan_changed
    # On quota_exceeded: block org in-memory so /v1/retrieve returns 429
    control_plane_subscriber.on_billing_quota_exceeded = quota_blocker.block

    # Start subscriber
    if await control_plane_subscriber.initialize():
        logger.info("✅ Control Plane Event Subscriber initialized")
    else:
        logger.warning(
            "⚠️  Control Plane Event Subscriber unavailable (graceful degradation)"
        )

    startup_complete = True

    yield
    
    # Cleanup on shutdown
    startup_complete = False
    if control_plane_subscriber:
        await control_plane_subscriber.close()
    await close_embedding_http_client()
    await close_rerank_http_client()
    await close_auth_channel()
    await close_org_channel()
    await close_shared_nats()
    logger.info("retrieval-service shutting down")


app = FastAPI(
    title="Data Plane — Retrieval Service",
    description=(
        "The only interface AI-Core uses to access knowledge. "
        "Documents are never returned — only ranked facts."
    ),
    version="1.0.0",
    lifespan=lifespan,
)

instrument_app(app, service_name="retrieval")

# ── Auth middleware (Phase 1) ─────────────────────────────────────────────────
if settings.internal_api_key:
    _auth_mw = create_auth_middleware(
        auth_grpc_url=settings.auth_core_grpc_url,
        internal_api_key=settings.internal_api_key,
        redis_getter=get_redis,
        cache_ttl=settings.auth_cache_ttl,
    )
    app.middleware("http")(_auth_mw)

    @app.middleware("http")
    async def authz_middleware(request: Request, call_next: RequestResponseEndpoint) -> Response:
        from shared.auth_middleware import _is_public
        if _is_public(request.url.path):
            return await call_next(request)

        auth_ctx = getattr(request.state, "auth", None)
        if auth_ctx is None:
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

        denial = check_route_permission(access, request.method, request.url.path)
        if denial:
            return JSONResponse(status_code=403, content={"detail": denial})

        return await call_next(request)
else:
    logger.warning(
        "⚠️  INTERNAL_API_KEY not set — running WITHOUT authentication (dev mode only)"
    )

app.include_router(retrieval_router)

from app.api.v1.status import router as status_router
app.include_router(status_router)


@app.get("/health")
async def health() -> dict:
    return {"status": "ok", "service": "retrieval"}


@app.get("/readyz")
async def readyz() -> JSONResponse:
    checks = {"startup": startup_complete, "qdrant": False}
    status_code = 200

    if startup_complete:
        try:
            loop = asyncio.get_running_loop()
            await loop.run_in_executor(None, lambda: get_qdrant().get_collections())
            checks["qdrant"] = True
        except Exception as exc:
            logger.warning("retrieval readyz qdrant check failed: %s", exc)
            status_code = 503
    else:
        status_code = 503

    return JSONResponse(
        status_code=status_code,
        content={
            "status": "ready" if status_code == 200 else "not_ready",
            "service": "retrieval",
            "checks": checks,
        },
    )


async def run_servers() -> None:
    grpc_server = await create_grpc_server()
    await grpc_server.start()
    logger.info("retrieval-service gRPC listening on port %d", settings.grpc_port)

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
        logger.info("retrieval-service gRPC server stopped")


def main() -> None:
    asyncio.run(run_servers())


if __name__ == "__main__":
    main()
