"""Artifact routes — /v1/tasks/{task_id}/artifacts & /v1/artifacts."""

from __future__ import annotations

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel

from app.domain import ArtifactKind
from app.runner_service import RunnerService

router = APIRouter(tags=["artifacts"])

# Injected at startup
_service: RunnerService | None = None


def init(service: RunnerService) -> None:
    global _service
    _service = service


def _svc() -> RunnerService:
    if _service is None:
        raise HTTPException(503, "Service not ready")
    return _service


class UploadUrlRequest(BaseModel):
    workspace_id: str
    filename: str
    kind: ArtifactKind = ArtifactKind.OUTPUT
    content_type: str = "application/octet-stream"


@router.get("/v1/tasks/{task_id}/artifacts")
async def list_artifacts(task_id: str):
    """List artifacts attached to a task."""
    arts = await _svc().list_task_artifacts(task_id)
    return {"artifacts": [a.model_dump(mode="json") for a in arts]}


@router.post("/v1/tasks/{task_id}/artifacts/upload-url")
async def create_upload_url(task_id: str, body: UploadUrlRequest):
    """Generate a presigned upload URL for an artifact."""
    result = await _svc().create_upload_url(
        task_id=task_id,
        workspace_id=body.workspace_id,
        filename=body.filename,
        kind=body.kind,
        content_type=body.content_type,
    )
    return result


@router.get("/v1/artifacts/{artifact_id}/download-url")
async def get_download_url(artifact_id: str, storage_key: str):
    """Generate a presigned download URL for an artifact."""
    url = await _svc().create_download_url(storage_key)
    return {"download_url": url, "artifact_id": artifact_id}
