"""Image generation and vision API."""

from __future__ import annotations

import base64
import uuid
from typing import Any

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel, Field

from reasoning_runtime.domain import CompletionRequest, Message, Provider
from reasoning_runtime.providers import image_provider

logger = __import__("logging").getLogger(__name__)

router = APIRouter(prefix="/api/v1/images", tags=["images"])


# ── Generation ───────────────────────────────────────────────────────────────


class ImageGenerateRequest(BaseModel):
    prompt: str = Field(..., min_length=1, max_length=4000)
    provider: str = "openai"
    model: str | None = None
    org_id: str = ""


class ImageGenerateResponse(BaseModel):
    request_id: str
    content: str
    model_used: str
    provider: str
    metadata: dict | None = None


@router.post("/generate", response_model=ImageGenerateResponse)
async def generate_image(body: ImageGenerateRequest):
    """Generate an image from a text prompt."""
    request_id = str(uuid.uuid4())

    provider_enum = Provider.GEMINI if body.provider == "google" else Provider.OPENAI

    req = CompletionRequest(
        request_id=request_id,
        org_id=body.org_id,
        model_id=body.model or "default",
        provider=provider_enum,
        messages=[Message(role="user", content=body.prompt)],
    )

    try:
        resp = await image_provider.complete(req)
        return ImageGenerateResponse(
            request_id=request_id,
            content=resp.content,
            model_used=resp.model_used,
            provider=resp.provider,
            metadata=resp.metadata,
        )
    except Exception as exc:
        logger.exception("image_generation_error request_id=%s", request_id)
        raise HTTPException(status_code=503, detail=str(exc))


# ── Vision / analysis ────────────────────────────────────────────────────────


class ImageAnalyzeRequest(BaseModel):
    """Accepts either a public URL or base64-encoded image bytes."""

    url: str | None = Field(None, description="Publicly reachable image URL")
    content_base64: str | None = Field(None, description="Base64-encoded image bytes")
    prompt: str = "Describe this image in detail."
    provider: str = "openai"
    model: str | None = None
    org_id: str = ""


class ImageAnalyzeResponse(BaseModel):
    request_id: str
    description: str
    model_used: str
    metadata: dict[str, Any] | None = None


def _image_message(url: str | None, content_base64: str | None, prompt: str) -> Message:
    """Build a multimodal Message with an image attachment."""
    if url:
        content: list[dict[str, Any]] = [
            {"type": "image_url", "image_url": {"url": url}},
            {"type": "text", "text": prompt},
        ]
    elif content_base64:
        content = [
            {"type": "image_url", "image_url": {"url": f"data:image/jpeg;base64,{content_base64}"}},
            {"type": "text", "text": prompt},
        ]
    else:
        raise HTTPException(status_code=422, detail="Provide url or content_base64")

    return Message(role="user", content=content)  # type: ignore[arg-type]


@router.post("/analyze", response_model=ImageAnalyzeResponse)
async def analyze_image(body: ImageAnalyzeRequest):
    """Describe or answer questions about an image using a vision model."""
    request_id = str(uuid.uuid4())
    provider_enum = Provider.ANTHROPIC if body.provider == "anthropic" else Provider.OPENAI

    req = CompletionRequest(
        request_id=request_id,
        org_id=body.org_id,
        model_id=body.model or "gpt-4o",
        provider=provider_enum,
        messages=[_image_message(body.url, body.content_base64, body.prompt)],
    )

    try:
        resp = await image_provider.complete(req)
        return ImageAnalyzeResponse(
            request_id=request_id,
            description=resp.content,
            model_used=resp.model_used,
            metadata=resp.metadata,
        )
    except HTTPException:
        raise
    except Exception as exc:
        logger.exception("image_analyze_error request_id=%s", request_id)
        raise HTTPException(status_code=503, detail=str(exc))


# ── OCR ──────────────────────────────────────────────────────────────────────


class OCRRequest(BaseModel):
    url: str | None = None
    content_base64: str | None = None
    provider: str = "openai"
    org_id: str = ""


class OCRResponse(BaseModel):
    request_id: str
    text: str
    model_used: str


@router.post("/ocr", response_model=OCRResponse)
async def ocr(body: OCRRequest):
    """Extract all text from an image (OCR via vision model)."""
    request_id = str(uuid.uuid4())
    provider_enum = Provider.ANTHROPIC if body.provider == "anthropic" else Provider.OPENAI

    req = CompletionRequest(
        request_id=request_id,
        org_id=body.org_id,
        model_id="gpt-4o",
        provider=provider_enum,
        messages=[
            _image_message(
                body.url,
                body.content_base64,
                "Extract all text from this image verbatim. Return only the text, no commentary.",
            )
        ],
    )

    try:
        resp = await image_provider.complete(req)
        return OCRResponse(request_id=request_id, text=resp.content, model_used=resp.model_used)
    except HTTPException:
        raise
    except Exception as exc:
        logger.exception("ocr_error request_id=%s", request_id)
        raise HTTPException(status_code=503, detail=str(exc))


# ── Object detection ─────────────────────────────────────────────────────────


class ObjectDetectionRequest(BaseModel):
    url: str | None = None
    content_base64: str | None = None
    provider: str = "openai"
    org_id: str = ""


class DetectedObject(BaseModel):
    label: str
    confidence: float | None = None


class ObjectDetectionResponse(BaseModel):
    request_id: str
    objects: list[DetectedObject]
    model_used: str


@router.post("/objects", response_model=ObjectDetectionResponse)
async def detect_objects(body: ObjectDetectionRequest):
    """Detect and list objects present in an image."""
    request_id = str(uuid.uuid4())
    provider_enum = Provider.ANTHROPIC if body.provider == "anthropic" else Provider.OPENAI

    req = CompletionRequest(
        request_id=request_id,
        org_id=body.org_id,
        model_id="gpt-4o",
        provider=provider_enum,
        messages=[
            _image_message(
                body.url,
                body.content_base64,
                "List all objects you can identify in this image. "
                "Respond as a JSON array of strings: [\"object1\", \"object2\", ...]",
            )
        ],
    )

    try:
        resp = await image_provider.complete(req)
        import json as _json
        try:
            raw_list = _json.loads(resp.content)
            if not isinstance(raw_list, list):
                raw_list = [resp.content]
        except _json.JSONDecodeError:
            raw_list = [t.strip() for t in resp.content.split(",") if t.strip()]

        objects = [DetectedObject(label=item) for item in raw_list if isinstance(item, str)]
        return ObjectDetectionResponse(request_id=request_id, objects=objects, model_used=resp.model_used)
    except HTTPException:
        raise
    except Exception as exc:
        logger.exception("object_detection_error request_id=%s", request_id)
        raise HTTPException(status_code=503, detail=str(exc))
