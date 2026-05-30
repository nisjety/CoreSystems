"""Image generation API — text-to-image endpoint. Phase 4.2."""
from __future__ import annotations

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel, Field

from app.providers import image_provider

router = APIRouter(prefix="/v1/images", tags=["images"])


class ImageGenerateRequest(BaseModel):
    prompt: str = Field(..., min_length=1, max_length=4000)
    provider: str = "openai"
    model: str | None = None
    size: str = "1024x1024"
    quality: str = "standard"
    n: int = Field(1, ge=1, le=4)


class ImageResult(BaseModel):
    b64_json: str
    revised_prompt: str


class ImageGenerateResponse(BaseModel):
    images: list[ImageResult]


@router.post("/generate", response_model=ImageGenerateResponse)
async def generate_image_endpoint(body: ImageGenerateRequest):
    """Generate images from a text prompt."""
    try:
        results = await image_provider.generate_image(
            body.prompt,
            provider=body.provider,
            model=body.model,
            size=body.size,
            quality=body.quality,
            n=body.n,
        )
        return ImageGenerateResponse(
            images=[ImageResult(**r) for r in results],
        )
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc))
    except RuntimeError as exc:
        raise HTTPException(status_code=503, detail=str(exc))
