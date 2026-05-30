"""Video generation API — Sora via Azure OpenAI.

Sora is accessed through the Azure OpenAI service using a dedicated
deployment (default: ``sora-turbo``).  The generation is asynchronous:
1. ``POST /api/v1/video/generate``  — submit a job, get a job_id immediately.
2. ``GET  /api/v1/video/{job_id}``  — poll for status; once "succeeded",
   returns a signed URL for the generated video.

Azure OpenAI Sora API reference:
  POST  /openai/deployments/{deployment}/videos/generations?api-version={version}
  GET   /openai/deployments/{deployment}/videos/generations/{job_id}?api-version={version}

Returns 501 if ``AZURE_OPENAI_VIDEO_DEPLOYMENT`` is not configured or the
Azure OpenAI endpoint is absent.
"""

from __future__ import annotations

import logging
import uuid
from typing import Literal

import httpx
from fastapi import APIRouter, HTTPException
from pydantic import BaseModel, Field

from app.config import get_settings

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/api/v1/video", tags=["video"])

# ── HTTP client ───────────────────────────────────────────────────────────────

_http: httpx.AsyncClient | None = None


def _client() -> httpx.AsyncClient:
    global _http
    if _http is None:
        _http = httpx.AsyncClient(timeout=60.0)
    return _http


def _require_sora() -> tuple[str, str, str, str]:
    """Return (base_url, api_key, deployment, api_version) or raise 501."""
    s = get_settings()
    if not s.azure_openai_endpoint or not s.azure_openai_api_key:
        raise HTTPException(status_code=501, detail="Azure OpenAI endpoint not configured")
    if not s.azure_openai_video_deployment:
        raise HTTPException(status_code=501, detail="AZURE_OPENAI_VIDEO_DEPLOYMENT not configured")
    return (
        s.azure_openai_endpoint.rstrip("/"),
        s.azure_openai_api_key,
        s.azure_openai_video_deployment,
        s.azure_openai_video_api_version,
    )


def _sora_headers(api_key: str) -> dict[str, str]:
    return {"api-key": api_key, "Content-Type": "application/json"}


# ── Models ────────────────────────────────────────────────────────────────────


class VideoGenerateRequest(BaseModel):
    prompt: str = Field(..., min_length=1, max_length=4000)
    width: int = Field(1280, description="Video width in pixels (must be divisible by 8)")
    height: int = Field(720, description="Video height in pixels (must be divisible by 8)")
    duration_seconds: int = Field(
        5, ge=1, le=20, description="Target video duration (1-20 seconds)"
    )
    fps: int = Field(25, description="Frames per second")
    quality: Literal["standard", "hd"] = "standard"
    org_id: str = ""


class VideoGenerateResponse(BaseModel):
    job_id: str
    status: Literal["queued", "running", "succeeded", "failed"]
    request_id: str
    message: str = ""


class VideoStatusResponse(BaseModel):
    job_id: str
    status: Literal["queued", "running", "succeeded", "failed"]
    video_url: str | None = None
    thumbnail_url: str | None = None
    duration_seconds: float | None = None
    error: str | None = None
    metadata: dict | None = None


# ── Endpoints ─────────────────────────────────────────────────────────────────


@router.post("/generate", response_model=VideoGenerateResponse, status_code=202)
async def generate_video(body: VideoGenerateRequest):
    """Submit a Sora video generation job.

    Returns *202 Accepted* immediately with a ``job_id``.
    Poll ``GET /api/v1/video/{job_id}`` to check completion.
    """
    base_url, api_key, deployment, api_version = _require_sora()
    request_id = str(uuid.uuid4())

    url = (
        f"{base_url}/openai/deployments/{deployment}"
        f"/videos/generations?api-version={api_version}"
    )

    payload = {
        "prompt":   body.prompt,
        "size":     f"{body.width}x{body.height}",
        "duration": body.duration_seconds,
        "fps":      body.fps,
        "quality":  body.quality,
    }

    try:
        resp = await _client().post(url, headers=_sora_headers(api_key), json=payload)
        if resp.status_code == 202:
            data = resp.json()
            job_id = data.get("id") or data.get("job_id") or f"sora-{request_id}"
            return VideoGenerateResponse(
                job_id=job_id,
                status=data.get("status", "queued"),
                request_id=request_id,
                message=data.get("message", ""),
            )
        logger.error("sora_generate_error status=%d body=%s", resp.status_code, resp.text[:200])
        raise HTTPException(status_code=resp.status_code, detail=resp.text[:400])
    except HTTPException:
        raise
    except Exception as exc:
        logger.exception("sora_generate_exception request_id=%s", request_id)
        raise HTTPException(status_code=503, detail=str(exc))


@router.get("/{job_id}", response_model=VideoStatusResponse)
async def get_video_status(job_id: str):
    """Poll a Sora generation job for status and retrieve the video URL."""
    base_url, api_key, deployment, api_version = _require_sora()

    url = (
        f"{base_url}/openai/deployments/{deployment}"
        f"/videos/generations/{job_id}?api-version={api_version}"
    )

    try:
        resp = await _client().get(url, headers=_sora_headers(api_key))
        if resp.status_code == 200:
            data = resp.json()
            # Azure returns the video in data.result.url or data.video_url
            result = data.get("result") or {}
            video_url = result.get("url") or data.get("video_url")
            thumb_url = result.get("thumbnail_url") or data.get("thumbnail_url")
            return VideoStatusResponse(
                job_id=job_id,
                status=data.get("status", "running"),
                video_url=video_url,
                thumbnail_url=thumb_url,
                duration_seconds=result.get("duration"),
                error=data.get("error"),
                metadata=data.get("metadata"),
            )
        if resp.status_code == 404:
            raise HTTPException(status_code=404, detail=f"Job {job_id} not found")
        raise HTTPException(status_code=resp.status_code, detail=resp.text[:400])
    except HTTPException:
        raise
    except Exception as exc:
        logger.exception("sora_status_exception job_id=%s", job_id)
        raise HTTPException(status_code=503, detail=str(exc))
