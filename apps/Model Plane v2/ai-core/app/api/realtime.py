"""Realtime Conversation API.

Wraps the OpenAI Realtime API (and Azure OpenAI equivalent) for low-latency,
bidirectional audio + text conversations.

Two endpoints:

1. ``POST /api/v1/realtime/session``
   Creates a Realtime API session and returns an ephemeral client secret.
   Frontends use this token to open the WebSocket directly to OpenAI/Azure
   without exposing the primary API key.

   OpenAI:  POST /v1/realtime/sessions
   Azure:   POST /openai/realtime?api-version={version}  (WebSocket)
            However for session token creation we use the REST endpoint.

2. ``GET /api/v1/realtime/models``
   Lists available realtime-capable models.

WebSocket proxying is done client-side: the frontend connects directly to
``wss://api.openai.com/v1/realtime?model={model}`` using the ephemeral key.
Proxying that connection server-side adds latency and is therefore not
implemented here; the session endpoint pattern follows the recommendation
in the OpenAI Realtime docs.

Returns 501 if no OpenAI API key is configured.
"""

from __future__ import annotations

import logging
import uuid

import httpx
from fastapi import APIRouter, HTTPException
from pydantic import BaseModel, Field

from app.config import get_settings

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/api/v1/realtime", tags=["realtime"])

# ── HTTP client ───────────────────────────────────────────────────────────────

_http: httpx.AsyncClient | None = None


def _client() -> httpx.AsyncClient:
    global _http
    if _http is None:
        _http = httpx.AsyncClient(timeout=30.0)
    return _http


# ── Models ────────────────────────────────────────────────────────────────────


class RealtimeSessionRequest(BaseModel):
    model: str | None = Field(
        None,
        description="Realtime model override. Defaults to OPENAI_REALTIME_MODEL setting.",
    )
    voice: str = Field(
        "alloy",
        description="TTS voice for audio responses. One of: alloy, echo, fable, onyx, nova, shimmer.",
    )
    instructions: str = Field(
        "",
        description="System-level instructions for this session.",
    )
    input_audio_format: str = "pcm16"
    output_audio_format: str = "pcm16"
    turn_detection_type: str = Field(
        "server_vad",
        description="'server_vad' for automatic turn detection, 'none' for manual.",
    )
    org_id: str = ""


class RealtimeSessionResponse(BaseModel):
    session_id: str
    client_secret: str
    model: str
    websocket_url: str
    expires_at: int
    voice: str


class RealtimeModelInfo(BaseModel):
    id: str
    provider: str
    description: str
    modalities: list[str]


# ── Realtime model catalog ────────────────────────────────────────────────────

_REALTIME_MODELS: list[dict] = [
    {
        "id":          "gpt-4o-realtime-preview-2025-01-09",
        "provider":    "openai",
        "description": "GPT-4o Realtime — low-latency audio + text (Jan 2025)",
        "modalities":  ["audio", "text"],
    },
    {
        "id":          "gpt-4o-mini-realtime-preview-2024-12-17",
        "provider":    "openai",
        "description": "GPT-4o Mini Realtime — cost-efficient audio + text",
        "modalities":  ["audio", "text"],
    },
    {
        "id":          "gpt-4o-realtime-preview",
        "provider":    "azure_openai",
        "description": "GPT-4o Realtime via Azure OpenAI deployment",
        "modalities":  ["audio", "text"],
    },
]


# ── Helpers ───────────────────────────────────────────────────────────────────


def _require_openai_key() -> str:
    key = get_settings().openai_api_key
    if not key:
        raise HTTPException(status_code=501, detail="OPENAI_API_KEY not configured")
    return key


# ── Endpoints ─────────────────────────────────────────────────────────────────


@router.post("/session", response_model=RealtimeSessionResponse)
async def create_session(body: RealtimeSessionRequest):
    """Create an OpenAI Realtime session and return an ephemeral client secret.

    The returned ``client_secret`` is short-lived (~1 minute) and can be used
    by browser/mobile clients to open the realtime WebSocket without embedding
    the primary API key.
    """
    api_key = _require_openai_key()
    settings = get_settings()
    model = body.model or settings.openai_realtime_model

    payload: dict = {
        "model":                model,
        "voice":                body.voice,
        "input_audio_format":   body.input_audio_format,
        "output_audio_format":  body.output_audio_format,
        "turn_detection":       {"type": body.turn_detection_type},
    }
    if body.instructions:
        payload["instructions"] = body.instructions

    try:
        resp = await _client().post(
            "https://api.openai.com/v1/realtime/sessions",
            headers={
                "Authorization": f"Bearer {api_key}",
                "Content-Type":  "application/json",
            },
            json=payload,
        )
        if resp.status_code in (200, 201):
            data = resp.json()
            # OpenAI returns { id, object, model, expires_at, client_secret: { value, expires_at } }
            cs = data.get("client_secret") or {}
            return RealtimeSessionResponse(
                session_id=data.get("id") or str(uuid.uuid4()),
                client_secret=cs.get("value") or cs.get("token") or "",
                model=data.get("model", model),
                websocket_url=f"wss://api.openai.com/v1/realtime?model={model}",
                expires_at=cs.get("expires_at") or data.get("expires_at") or 0,
                voice=data.get("voice") or body.voice,
            )
        logger.error("realtime_session_error status=%d body=%s", resp.status_code, resp.text[:200])
        raise HTTPException(status_code=resp.status_code, detail=resp.text[:400])
    except HTTPException:
        raise
    except Exception as exc:
        logger.exception("realtime_session_exception")
        raise HTTPException(status_code=503, detail=str(exc))


@router.get("/models", response_model=list[RealtimeModelInfo])
async def list_realtime_models():
    """List realtime-capable models available through this service."""
    settings = get_settings()
    models = list(_REALTIME_MODELS)
    # Only surface azure_openai realtime if deployment is configured
    if not settings.azure_openai_realtime_deployment:
        models = [m for m in models if m["provider"] != "azure_openai"]
    return [RealtimeModelInfo(**m) for m in models]
