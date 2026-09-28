import asyncio
import json
import logging
from collections.abc import AsyncGenerator
from contextlib import asynccontextmanager
from pathlib import Path
from uuid import UUID

from fastapi import Depends, FastAPI, File, Form, Header, HTTPException, Request, UploadFile
from fastapi.responses import JSONResponse
from pydantic import BaseModel, Field
from sse_starlette.sse import EventSourceResponse
from sqlalchemy import text

from app.actions_gateway import ActionsGateway
from app.config import get_settings
from app.control_plane_subscriber import ControlPlaneSubscriber
from app.db import engine, run_sql_migrations
from app.events import event_publisher
from app.auth_middleware import AuthContext, authentication_ready, require_internal_auth
from app.knowledge_sync import KnowledgeSyncer
from app.m365_provider_handler import get_m365_handler
from app.orchestration import orchestrator
from app.parsers import UnsupportedFileTypeError, parse_uploaded_file
from app.chat_extract import ChatExtractRequest, extract_chat_document
from starlette.concurrency import run_in_threadpool
from app.progress import progress_hub
from app.schemas import JobDetailResponse, JobItemResponse, JobResponse, SourceImportRequest
from app.space_import_authority import SpaceImportIngressDenied, verify_space_import_ingress_decision
from app.space_deletion_auth import is_space_deletion_principal
from app.service import (
    QuotaCheckUnavailable,
    close_http_client,
    get_http_client,
    import_service,
    init_http_client,
)
from app.shared_nats import SharedNatsPublisher


settings = get_settings()
logging.basicConfig(level=getattr(logging, settings.log_level.upper(), logging.INFO))
logger = logging.getLogger(__name__)

# Global instances
shared_nats_publisher: SharedNatsPublisher | None = None
control_plane_subscriber: ControlPlaneSubscriber | None = None


@asynccontextmanager
async def lifespan(_: FastAPI):
    global shared_nats_publisher, control_plane_subscriber

    # Verify DB connectivity
    async with engine.begin() as conn:
        await conn.execute(text("SELECT 1"))

    await run_sql_migrations()

    # Initialise shared HTTP client (avoids per-request TCP handshakes)
    init_http_client()
    await event_publisher.connect()
    
    # Initialize shared NATS publisher for cross-plane events
    shared_nats_publisher = SharedNatsPublisher(
        settings.nats_shared_url,
        settings.nats_shared_token,
        "imports-api",
    )
    await shared_nats_publisher.initialize()
    
    # Initialize Control Plane subscriber (Phase 6)
    control_plane_subscriber = ControlPlaneSubscriber(
        settings.nats_shared_url,
        settings.nats_shared_token,
        "imports-api",
    )
    
    # Wire up event handlers
    m365_handler = get_m365_handler()
    control_plane_subscriber.on_user_provider_linked = m365_handler.handle
    
    # Start subscriber
    if await control_plane_subscriber.initialize():
        logger.info("✅ Control Plane Event Subscriber initialized")
    else:
        logger.warning("⚠️  Control Plane Event Subscriber unavailable (graceful degradation)")

    for job_id in await import_service.recoverable_job_ids():
        await orchestrator.dispatch(job_id, import_service.run_job)
    
    yield
    
    await event_publisher.close()
    if shared_nats_publisher:
        await shared_nats_publisher.close()
    if control_plane_subscriber:
        await control_plane_subscriber.close()
    await close_http_client()
    await engine.dispose()



app = FastAPI(title=settings.import_service_name, lifespan=lifespan)


class SpaceDeletionCancelRequest(BaseModel):
    deletion_request_id: str = Field(min_length=1, max_length=256)
    space_ref: str = Field(min_length=1, max_length=256)
    reason: str = Field(min_length=8, max_length=500)


@app.exception_handler(QuotaCheckUnavailable)
async def quota_dependency_unavailable(
    _request: Request, _exc: QuotaCheckUnavailable
) -> JSONResponse:
    return JSONResponse(
        status_code=503,
        content={
            "error": {
                "code": "quota_dependency_unavailable",
                "message": "Import quota could not be verified; retry later",
                "details": {},
            }
        },
    )


def _job_to_response(job) -> JobResponse:
    return JobResponse(
        id=job.id,
        org_id=job.org_id,
        user_id=job.user_id,
        source_type=job.source_type,
        status=job.status,
        total_items=job.total_items,
        processed_items=job.processed_items,
        failed_items=job.failed_items,
        error_message=job.error_message,
        metadata=job.metadata_json,
        created_at=job.created_at,
        started_at=job.started_at,
        completed_at=job.completed_at,
    )


def _job_item_to_response(item) -> JobItemResponse:
    return JobItemResponse(
        id=item.id,
        job_id=item.job_id,
        source_id=item.source_id,
        source_name=item.source_name,
        status=item.status,
        error_message=item.error_message,
        metadata=item.metadata_json,
        created_at=item.created_at,
        completed_at=item.completed_at,
    )


@app.get("/health")
async def health() -> dict[str, str]:
    return {"status": "ok", "service": settings.import_service_name}


@app.get("/ready")
async def ready() -> JSONResponse:
    checks: dict[str, bool] = {"database": False, "auth_core": False, "data_plane": False}
    try:
        async with engine.connect() as conn:
            await conn.execute(text("SELECT 1"))
        checks["database"] = True
    except Exception:
        logger.exception("Readiness database check failed")

    checks["auth_core"] = await authentication_ready()
    try:
        response = await get_http_client().get(
            f"{settings.document_service_url.rstrip('/')}/readyz", timeout=3.0
        )
        checks["data_plane"] = response.is_success
    except Exception:
        logger.exception("Readiness Data Plane check failed")

    required_ready = all(checks.values())
    optional = {
        "local_nats": event_publisher.is_connected(),
        "shared_nats": bool(shared_nats_publisher and shared_nats_publisher.nc),
        "control_plane_events": bool(control_plane_subscriber and control_plane_subscriber.nc),
    }
    status_code = 200 if required_ready else 503
    return JSONResponse(
        status_code=status_code,
        content={
            "data": {
                "status": "ready" if required_ready else "not_ready",
                "required": checks,
                "optional": optional,
            }
        },
    )


@app.post("/api/v1/import/extract")
async def extract_chat_attachment(
    request: ChatExtractRequest,
    auth: AuthContext = Depends(require_internal_auth),
) -> JSONResponse:
    # This operation is ephemeral even for persistent sessions. No storage,
    # embedding, model call, or Tika/third-party egress is involved.
    result = await run_in_threadpool(extract_chat_document, request)
    return JSONResponse(content=result, headers={"Cache-Control": "no-store"})


@app.post("/api/v1/import/jobs/upload", response_model=JobResponse)
async def create_upload_job(
    files: list[UploadFile] = File(...),
    zdr: bool = Form(False),
    auth: AuthContext = Depends(require_internal_auth),
) -> JobResponse:
    if zdr:
        raise HTTPException(
            status_code=409,
            detail="zdr_persistence_forbidden: imports create durable Data Plane documents",
        )
    if not files:
        raise HTTPException(status_code=400, detail="No files provided")
    if len(files) > settings.max_upload_files:
        raise HTTPException(status_code=400, detail="Too many files in request")

    org_id = auth.org_id
    user_id = auth.user_id

    documents = []
    max_bytes = settings.max_file_size_mb * 1024 * 1024
    for upload in files:
        extension = Path(upload.filename).suffix.lower().lstrip(".")
        if extension not in settings.allowed_file_types:
            raise HTTPException(status_code=400, detail=f"Unsupported file type: {extension}")

        content = await upload.read()
        if len(content) > max_bytes:
            raise HTTPException(
                status_code=400,
                detail=f"File exceeds max size of {settings.max_file_size_mb}MB: {upload.filename}",
            )
        try:
            documents.append(parse_uploaded_file(upload.filename, content, upload.content_type))
        except UnsupportedFileTypeError as exc:
            raise HTTPException(status_code=400, detail=str(exc)) from exc

    allowed = await import_service.check_quota(org_id=org_id, items=len(documents))
    if not allowed:
        raise HTTPException(status_code=403, detail="Import quota exceeded")

    job_id = await import_service.create_job(
        org_id=org_id,
        user_id=user_id,
        source_type="upload",
        documents=documents,
        metadata={"upload_count": len(documents)},
    )

    await orchestrator.dispatch(job_id, import_service.run_job)
    job = await import_service.get_job(job_id)
    if not job:
        raise HTTPException(status_code=500, detail="Failed to create import job")
    return _job_to_response(job)


@app.post("/api/v1/import/jobs/source", response_model=JobResponse)
async def create_source_job(
    request: SourceImportRequest,
    auth: AuthContext = Depends(require_internal_auth),
    space_import_ingress_decision: str | None = Header(
        default=None, alias="X-Space-Import-Ingress-Decision"
    ),
) -> JobResponse:
    if request.zdr:
        raise HTTPException(
            status_code=409,
            detail="zdr_persistence_forbidden: imports create durable Data Plane documents",
        )
    org_id = auth.org_id
    user_id = auth.user_id
    space_import_intent = None
    if space_import_ingress_decision is not None:
        try:
            space_import_intent = verify_space_import_ingress_decision(
                space_import_ingress_decision, auth
            )
        except SpaceImportIngressDenied as exc:
            raise HTTPException(status_code=403, detail="Space import authority denied") from exc
        if space_import_intent.source_type != request.source_type:
            raise HTTPException(status_code=403, detail="Space import authority denied")

    try:
        documents = await import_service.create_source_documents(
            source_type=request.source_type,
            connection=request.connection,
            options=request.options,
        )
    except Exception as exc:
        logger.warning(
            "Source import failed org=%s source_type=%s error_type=%s",
            org_id,
            request.source_type,
            type(exc).__name__,
        )
        raise HTTPException(status_code=400, detail="Source import failed") from exc

    allowed = await import_service.check_quota(org_id=org_id, items=len(documents))
    if not allowed:
        raise HTTPException(status_code=403, detail="Import quota exceeded")

    job_id = await import_service.create_job(
        org_id=org_id,
        user_id=user_id,
        source_type=request.source_type,
        documents=documents,
        metadata={"source": request.source_type, "count": len(documents)},
        space_import_intent=space_import_intent,
    )

    await orchestrator.dispatch(job_id, import_service.run_job)
    job = await import_service.get_job(job_id)
    if not job:
        raise HTTPException(status_code=500, detail="Failed to create import job")
    return _job_to_response(job)


@app.post("/api/v1/internal/space-deletion/cancel-imports")
async def cancel_space_imports_for_deletion(
    request: SpaceDeletionCancelRequest,
    auth: AuthContext = Depends(require_internal_auth),
) -> JSONResponse:
    """Ingestion's bounded deletion adapter.

    It cancels queued Space-scoped jobs and erases pending payloads. It does
    not delete documents already handed to Data or connector credentials, so
    the owner outcome is explicitly partial and cannot become a completion
    receipt for the Space deletion coordinator.
    """
    if not is_space_deletion_principal(auth):
        raise HTTPException(status_code=403, detail="Dedicated Space deletion service scope required")
    result = await import_service.cancel_queued_space_jobs(
        auth.org_id, request.space_ref.strip(), request.reason.strip()
    )
    return JSONResponse(
        {
            "request_id": request.deletion_request_id.strip(),
            "owner_plane": "ingestion",
            "org_id": auth.org_id,
            "space_ref": request.space_ref.strip(),
            **result,
            "owner_outcome": "partial",
            "remaining_work": "Data document and connector resource owners must report separately",
        }
    )


class _NatsKnowledgeSyncAudit:
    """Best-effort audit sink that publishes knowledge-sync run outcomes to NATS."""

    _SUBJECT = "imports.knowledge_sync.run"

    async def publish_sync(self, audit: dict) -> None:
        await event_publisher.publish(self._SUBJECT, audit)


@app.post("/api/v1/import/jobs/knowledge-sync")
async def knowledge_sync(
    auth: AuthContext = Depends(require_internal_auth),
    zdr: bool = Header(False, alias="X-ZDR"),
) -> JSONResponse:
    """Pull GitHub/Slack content for the caller's org through the integration
    actions gateway and forward it into the import pipeline (which persists to
    Data Plane v2). Honest 503 when the gateway is not configured."""
    if zdr:
        raise HTTPException(
            status_code=409,
            detail="zdr_persistence_forbidden: knowledge sync creates durable documents",
        )
    org_id = auth.org_id
    gateway = ActionsGateway(
        settings.integration_core_url,
        auth.bearer_token or "",
        get_http_client(),
    )
    if not gateway.configured():
        raise HTTPException(
            status_code=503,
            detail="knowledge-sync unavailable: integration URL or caller credential missing",
        )

    syncer = KnowledgeSyncer(gateway, audit=_NatsKnowledgeSyncAudit())
    result = await syncer.sync(org_id)

    job_id = None
    if result.documents:
        allowed = await import_service.check_quota(org_id=org_id, items=len(result.documents))
        if not allowed:
            raise HTTPException(status_code=403, detail="Import quota exceeded")
        documents = result.documents
        job_id = await import_service.create_job(
            org_id=org_id,
            user_id=auth.user_id,
            source_type="knowledge-sync",
            documents=documents,
            metadata={"source": "knowledge-sync", "count": len(documents)},
        )
        await orchestrator.dispatch(job_id, import_service.run_job)

    return JSONResponse(
        {
            "outcome": result.outcome,
            "connections": result.connections,
            "documents": len(result.documents),
            "skipped": result.skipped,
            "job_id": str(job_id) if job_id else None,
        }
    )


@app.get("/api/v1/import/jobs/{job_id}", response_model=JobDetailResponse)
async def get_job(
    job_id: UUID,
    auth: AuthContext = Depends(require_internal_auth),
) -> JobDetailResponse:
    job, items = await import_service.get_job_with_items(job_id, org_id=auth.org_id)
    if not job:
        raise HTTPException(status_code=404, detail="Job not found")
    return JobDetailResponse(
        **_job_to_response(job).model_dump(),
        items=[_job_item_to_response(item) for item in items],
    )


@app.get("/api/v1/import/jobs/{job_id}/events")
async def stream_job_events(
    job_id: UUID,
    auth: AuthContext = Depends(require_internal_auth),
) -> EventSourceResponse:
    job = await import_service.get_job(job_id, org_id=auth.org_id)
    if not job:
        raise HTTPException(status_code=404, detail="Job not found")

    async def stream() -> AsyncGenerator[dict[str, str], None]:
        initial = {
            "event": "import.snapshot",
            "job_id": str(job.id),
            "status": job.status,
            "processed_items": job.processed_items,
            "total_items": job.total_items,
            "failed_items": job.failed_items,
        }
        yield {"event": "import.snapshot", "data": json.dumps(initial)}

        if job.status in {"completed", "completed_with_errors", "failed"}:
            return

        async for payload in progress_hub.subscribe(job_id):
            yield {"event": payload.get("event", "import.progress"), "data": json.dumps(payload)}

    return EventSourceResponse(stream(), ping=15)


@app.get("/")
async def root() -> JSONResponse:
    return JSONResponse(
        {
            "service": settings.import_service_name,
            "version": "1.0.0",
            "endpoints": [
                "/api/v1/import/jobs/upload",
                "/api/v1/import/jobs/source",
                "/api/v1/import/jobs/{job_id}",
                "/api/v1/import/jobs/{job_id}/events",
            ],
        }
    )


if __name__ == "__main__":
    import uvicorn

    uvicorn.run("app.main:app", host="0.0.0.0", port=settings.import_service_port, reload=True)
