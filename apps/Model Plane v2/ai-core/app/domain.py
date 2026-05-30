"""ai-core domain types."""

from __future__ import annotations

from datetime import datetime
from enum import Enum
from typing import Any

from pydantic import BaseModel, Field, PrivateAttr

# Re-export reasoning_runtime types for convenience
from reasoning_runtime.domain import (  # noqa: F401
    ChunkType,
    CompletionChunk,
    CompletionEvent,
    CompletionRequest,
    CompletionResponse,
    Message,
    Provider,
    ToolCallBlock,
    ToolDefinition,
)


# ── AI-Core specific types ──────────────────────────────────────

class IntentType(str, Enum):
    """Classified intent of an incoming request."""
    CHAT = "chat"
    COMPLETION = "completion"
    AGENT_TASK = "agent_task"
    IMAGE_GENERATION = "image_generation"
    SPEECH_TTS = "speech_tts"
    SPEECH_STT = "speech_stt"
    TRANSLATION = "translation"
    CODE_GENERATION = "code_generation"
    SUMMARIZATION = "summarization"
    SEARCH = "search"
    UNKNOWN = "unknown"


class SafetyVerdict(str, Enum):
    """Content safety check result."""
    SAFE = "safe"
    FLAGGED = "flagged"
    BLOCKED = "blocked"


class Modality(str, Enum):
    """Detected modality of the incoming request (Layer 0)."""
    TEXT = "text"
    DOCUMENT = "document"
    IMAGE = "image"
    AUDIO = "audio"
    VIDEO = "video"
    REALTIME = "realtime"


class PipelineContext(BaseModel):
    """Context threaded through the 10-layer pipeline."""
    request_id: str
    org_id: str
    session_id: str = ""
    run_id: str = ""

    # Layer 0: Multimodal normalization
    modality: Modality = Modality.TEXT

    # Layer 3: Intent classification
    intent: IntentType = IntentType.UNKNOWN
    intent_confidence: float = 0.0

    # Layer 4: Capability resolution
    resolved_provider: Provider | None = None
    resolved_model: str = ""

    # Layer 5: Context enrichment
    system_prompt: str = ""
    context_documents: list[dict[str, Any]] = Field(default_factory=list)

    # Layer 7/9: Content safety
    pre_safety: SafetyVerdict = SafetyVerdict.SAFE
    post_safety: SafetyVerdict = SafetyVerdict.SAFE

    # Layer 8b: RAG reflection + synthesis
    rag_reflection_score: float = 0.0
    rag_reflection_verdict: str = ""  # "sufficient" | "insufficient" | "skipped"
    rag_synthesis_used: bool = False

    # Layer 10: Output formatting
    format_instructions: str = ""

    # Timing
    pipeline_start_ms: int = 0
    layer_timings: dict[str, int] = Field(default_factory=dict)

    # Internal scratchpad for layers (not serialised to callers)
    _raw: dict[str, Any] = PrivateAttr(default_factory=dict)


class ChatRequest(BaseModel):
    """Public chat API request (session-core → ai-core)."""
    message: str
    session_id: str = ""
    org_id: str = ""
    model: str = ""
    provider: str = ""
    temperature: float = 0.7
    max_tokens: int | None = None
    stream: bool = False
    tools: list[ToolDefinition] | None = None
    context: dict[str, Any] | None = None


class ChatResponse(BaseModel):
    """Public chat API response."""
    request_id: str
    content: str = ""
    tool_calls: list[ToolCallBlock] = Field(default_factory=list)
    model_used: str = ""
    provider: str = ""
    tokens_in: int = 0
    tokens_out: int = 0
    latency_ms: int = 0
    intent: str = ""
    finish_reason: str = "stop"
