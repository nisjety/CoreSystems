"""Model discovery API — lists available inference providers and models."""

from __future__ import annotations

from fastapi import APIRouter
from pydantic import BaseModel

from reasoning_runtime.domain import Provider

logger = __import__("logging").getLogger(__name__)

router = APIRouter(prefix="/api/v1/models", tags=["models"])

# Static capability registry; runtime entries can be injected at startup.
_MODEL_CATALOG: list[dict] = [
    # ── Chat / text ──────────────────────────────────────────────────────
    {"id": "gpt-4o",           "provider": "openai",       "modality": "chat",        "streaming": True},
    {"id": "gpt-4o-mini",      "provider": "openai",       "modality": "chat",        "streaming": True},
    {"id": "o3-mini",          "provider": "openai",       "modality": "reasoning",   "streaming": True},
    # ── Anthropic Claude ──────────────────────────────────────────────────
    # Claude 4.x family (April 2026+, current generation)
    {"id": "claude-opus-4-6",           "provider": "anthropic", "modality": "chat",      "streaming": True},
    {"id": "claude-sonnet-4-6",         "provider": "anthropic", "modality": "chat",      "streaming": True},
    {"id": "claude-haiku-4-5-20251001", "provider": "anthropic", "modality": "chat",      "streaming": True},
    # Claude 4.x reasoning aliases (extended thinking)
    {"id": "claude-opus-4-6",           "provider": "anthropic", "modality": "reasoning", "streaming": True},
    {"id": "claude-sonnet-4-6",         "provider": "anthropic", "modality": "reasoning", "streaming": True},
    # Claude 3.7 / 3.5 (still available for backward-compat)
    {"id": "claude-3-7-sonnet-20250219","provider": "anthropic", "modality": "chat",      "streaming": True},
    {"id": "claude-3-5-sonnet-20241022","provider": "anthropic", "modality": "chat",      "streaming": True},
    {"id": "claude-3-5-haiku-20241022", "provider": "anthropic", "modality": "chat",      "streaming": True},
    # Vision
    {"id": "claude-sonnet-4-6",         "provider": "anthropic", "modality": "vision",    "streaming": False},
    {"id": "claude-opus-4-6",           "provider": "anthropic", "modality": "vision",    "streaming": False},
    {"id": "claude-3-7-sonnet-20250219","provider": "anthropic", "modality": "vision",    "streaming": False},
    # ── Other providers ───────────────────────────────────────────────────
    {"id": "gemini-2.0-flash-exp",       "provider": "gemini",    "modality": "chat", "streaming": True},
    {"id": "mistral-large-latest",       "provider": "mistral",   "modality": "chat", "streaming": True},
    {"id": "command-r-plus",             "provider": "cohere",    "modality": "chat", "streaming": True},
    # ── Image generation ─────────────────────────────────────────────────
    {"id": "dall-e-3",         "provider": "openai",       "modality": "image_gen",   "streaming": False},
    {"id": "imagen-3",         "provider": "gemini",       "modality": "image_gen",   "streaming": False},
    # ── Vision / analysis ────────────────────────────────────────────────
    {"id": "gpt-4o",           "provider": "openai",       "modality": "vision",      "streaming": False},
    {"id": "claude-3-5-sonnet-20241022", "provider": "anthropic", "modality": "vision", "streaming": False},
    # ── Speech / audio ───────────────────────────────────────────────────
    {"id": "tts-1",            "provider": "openai",       "modality": "tts",         "streaming": False},
    {"id": "tts-1-hd",         "provider": "openai",       "modality": "tts",         "streaming": False},
    {"id": "whisper-1",        "provider": "openai",       "modality": "asr",         "streaming": False},
    {"id": "azure-neural-tts", "provider": "azure_openai", "modality": "tts",         "streaming": True},
    {"id": "azure-whisper",    "provider": "azure_openai", "modality": "asr",         "streaming": False},
    # ── Translation ──────────────────────────────────────────────────────
    {"id": "azure-translator", "provider": "azure_openai", "modality": "translation", "streaming": False},
    # ── Document intelligence ────────────────────────────────────────────
    {"id": "prebuilt-layout",  "provider": "azure_openai", "modality": "document",    "streaming": False},
    {"id": "prebuilt-invoice", "provider": "azure_openai", "modality": "document",    "streaming": False},
]


class ModelEntry(BaseModel):
    id: str
    provider: str
    modality: str
    streaming: bool


class ModelsResponse(BaseModel):
    models: list[ModelEntry]
    total: int


@router.get("", response_model=ModelsResponse)
async def list_models(
    modality: str | None = None,
    provider: str | None = None,
):
    """Return all available models, optionally filtered by modality or provider."""
    entries = _MODEL_CATALOG

    if modality:
        entries = [e for e in entries if e["modality"] == modality]
    if provider:
        entries = [e for e in entries if e["provider"] == provider]

    # De-duplicate by (id, provider, modality) since vision+chat share model IDs
    seen: set[tuple] = set()
    deduped: list[dict] = []
    for e in entries:
        key = (e["id"], e["provider"], e["modality"])
        if key not in seen:
            seen.add(key)
            deduped.append(e)

    return ModelsResponse(models=[ModelEntry(**e) for e in deduped], total=len(deduped))


@router.get("/{model_id}", response_model=ModelEntry)
async def get_model(model_id: str):
    """Return details for a single model id (first match across providers)."""
    from fastapi import HTTPException

    for entry in _MODEL_CATALOG:
        if entry["id"] == model_id:
            return ModelEntry(**entry)
    raise HTTPException(status_code=404, detail=f"model '{model_id}' not found")
