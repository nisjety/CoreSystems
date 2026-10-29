"""Model Plane v2 parity catalogue.

The current Model Plane is the target runtime. Model Plane v2 is only a donor
for capability contracts and behaviours that still need to be migrated.
"""

from __future__ import annotations

from enum import Enum
from typing import Literal

from pydantic import BaseModel, Field


class ParityStatus(str, Enum):
    """Implementation status for a v2 donor capability in current Model Plane."""

    yes = "yes"
    partial = "partial"
    no = "no"


class ParityCapability(BaseModel, frozen=True):
    """A single v2-to-current Model Plane parity item."""

    capability_id: str
    name: str
    source: Literal["model-plane-v2"]
    owner: Literal["rust", "go", "python", "data-plane", "cross-plane"]
    status: ParityStatus
    v2_surface: str = Field(min_length=1)
    current_surface: str = Field(min_length=1)
    next_step: str = Field(min_length=1)


MODEL_PLANE_V2_PARITY: tuple[ParityCapability, ...] = (
    ParityCapability(
        capability_id="ai.embeddings",
        name="Provider-backed embeddings",
        source="model-plane-v2",
        owner="rust",
        status=ParityStatus.yes,
        v2_surface="ai-core ChatService.CreateEmbedding",
        current_surface="InferenceCore.CreateEmbedding plus /v1/ai/embeddings",
        next_step="Add production smoke coverage with real Azure OpenAI credentials.",
    ),
    ParityCapability(
        capability_id="ai.model_catalog",
        name="Model catalogue",
        source="model-plane-v2",
        owner="rust",
        status=ParityStatus.partial,
        v2_surface="ai-core ChatService.ListModels and /api/v1/models",
        current_surface="InferenceCore.ListModels plus /v1/ai/models",
        next_step="Back model catalogue with capability-core provider registry instead of startup env only.",
    ),
    ParityCapability(
        capability_id="ai.images",
        name="Image generation, analysis, and OCR",
        source="model-plane-v2",
        owner="rust",
        status=ParityStatus.partial,
        v2_surface="ai-core ImageService.GenerateImage/AnalyzeImage/ExtractText",
        current_surface="InferenceCore.GenerateImage/AnalyzeImage/ExtractImageText plus /v1/ai/images, /v1/ai/images/analyze, and /v1/ai/images/ocr",
        next_step="Add Azure Document Intelligence read/OCR fallback and real provider smoke coverage.",
    ),
    ParityCapability(
        capability_id="ai.speech",
        name="Speech synthesis and transcription",
        source="model-plane-v2",
        owner="rust",
        status=ParityStatus.partial,
        v2_surface="ai-core SpeechService TTS/transcribe/stream/detect-language/list-voices",
        current_surface="InferenceCore.SynthesizeSpeech/TranscribeSpeech/ListSpeechVoices plus /v1/ai/speech and /v1/ai/speech/voices",
        next_step="Add streaming transcription, spoken-language detection, and realtime voice session ownership.",
    ),
    ParityCapability(
        capability_id="ai.translation",
        name="Translation and language detection",
        source="model-plane-v2",
        owner="rust",
        status=ParityStatus.partial,
        v2_surface="ai-core TranslationService translate/batch/detect/list-languages",
        current_surface="InferenceCore.TranslateText/BatchTranslateText/DetectTextLanguage/ListTranslationLanguages plus /v1/ai/translate, /v1/ai/translate/detect, and /v1/ai/translate/languages",
        next_step="Add transliteration only if product needs it, then run production Azure Translator smoke coverage.",
    ),
    ParityCapability(
        capability_id="ai.document_intelligence",
        name="Document intelligence extraction",
        source="model-plane-v2",
        owner="rust",
        status=ParityStatus.partial,
        v2_surface="ai-core DocumentService analyze/layout/forms/receipts/invoices",
        current_surface="InferenceCore.AnalyzeDocument plus /v1/ai/documents/analyze, /layout, /forms, /receipts, and /invoices",
        next_step="Add real Azure Document Intelligence smoke coverage and richer typed invoice/receipt field normalization.",
    ),
    ParityCapability(
        capability_id="ai.language_analytics",
        name="Language analytics",
        source="model-plane-v2",
        owner="rust",
        status=ParityStatus.partial,
        v2_surface="ai-core language sentiment/entities/key-phrases/PII/summary",
        current_surface="InferenceCore.AnalyzeLanguage plus /v1/ai/language, /sentiment, /entities, /key-phrases, /pii, /detect, and /summary/text",
        next_step="Add production Azure AI Language smoke coverage and typed result normalization per operation.",
    ),
    ParityCapability(
        capability_id="ai.realtime",
        name="Realtime sessions",
        source="model-plane-v2",
        owner="rust",
        status=ParityStatus.partial,
        v2_surface="ai-core realtime session/model APIs",
        current_surface="InferenceCore.CreateRealtimeSession plus /v1/ai/realtime, /v1/ai/realtime/session, and /v1/ai/realtime/models",
        next_step="Add real OpenAI realtime smoke coverage and browser/WebRTC client integration; add Azure realtime only after confirming product need.",
    ),
    ParityCapability(
        capability_id="ai.video",
        name="Video generation jobs",
        source="model-plane-v2",
        owner="rust",
        status=ParityStatus.partial,
        v2_surface="ai-core video generate/status APIs",
        current_surface="InferenceCore.CreateVideoGenerationJob/GetVideoGenerationJob/StreamVideoGenerationContent plus /v1/ai/video/generate, /v1/ai/video/jobs/:job_id, and /v1/ai/video/generations/:generation_id/content",
        next_step="Add real Azure OpenAI Sora smoke coverage and durable artifact handoff if generated media must be retained.",
    ),
    ParityCapability(
        capability_id="providers.extended",
        name="Extended provider registry",
        source="model-plane-v2",
        owner="cross-plane",
        status=ParityStatus.no,
        v2_surface="OpenAI/Azure, Anthropic, Gemini, Mistral, Cohere, Ollama providers",
        current_surface="Anthropic and OpenAI/Azure chat; OpenAI/Azure embeddings",
        next_step="Port provider adapters behind the current Rust trait boundaries.",
    ),
    ParityCapability(
        capability_id="agent.runtime_controls",
        name="Agent runtime controls",
        source="model-plane-v2",
        owner="go",
        status=ParityStatus.partial,
        v2_surface="agent-core snapshots, event history, teams, control-actions, subagents",
        current_surface="orchestrator/session/execution split with partial gateway methods",
        next_step="Map v2 runtime APIs to current services and persist runtime state.",
    ),
    ParityCapability(
        capability_id="runtime.hooks_mcp_plugins",
        name="Hooks, MCP, plugins, and command runtime",
        source="model-plane-v2",
        owner="go",
        status=ParityStatus.partial,
        v2_surface="agent-core hooks/MCP/skills CRUD plus tool proxy modules",
        current_surface="capability-core schemas plus model-gateway registries; several paths are in-memory/ack-only",
        next_step="Move gateway registries into capability-core persistence and implement real command/hook execution.",
    ),
)


def parity_matrix(status: ParityStatus | str | None = None) -> list[dict[str, str]]:
    """Return the parity catalogue as serializable dictionaries.

    Args:
        status: Optional status filter.
    """

    wanted = ParityStatus(status) if status is not None else None
    return [
        capability.model_dump(mode="json")
        for capability in MODEL_PLANE_V2_PARITY
        if wanted is None or capability.status == wanted
    ]


def missing_capabilities() -> list[ParityCapability]:
    """Return capabilities that still need implementation work."""

    return [
        capability
        for capability in MODEL_PLANE_V2_PARITY
        if capability.status != ParityStatus.yes
    ]
