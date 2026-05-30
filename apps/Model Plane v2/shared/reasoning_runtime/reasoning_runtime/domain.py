"""Domain types for the reasoning runtime.

Covers: completion requests/responses, streaming chunks, tool call
normalization, result store references.
"""

from __future__ import annotations

from datetime import datetime
from enum import Enum
from typing import Any

from pydantic import BaseModel, Field


# ── Provider enum ───────────────────────────────────────────────

class Provider(str, Enum):
    OPENAI = "openai"
    AZURE_OPENAI = "azure_openai"
    ANTHROPIC = "anthropic"
    GEMINI = "gemini"
    COHERE = "cohere"
    MISTRAL = "mistral"
    OLLAMA = "ollama"


# ── Completion request ──────────────────────────────────────────

class Message(BaseModel):
    role: str  # system | user | assistant | tool
    content: str | list[dict[str, Any]] = ""
    name: str | None = None
    tool_call_id: str | None = None
    tool_calls: list[dict[str, Any]] | None = None


class ToolDefinition(BaseModel):
    """OpenAI-compatible tool definition."""
    type: str = "function"
    function: dict[str, Any] = Field(default_factory=dict)


class CompletionRequest(BaseModel):
    """Fully-resolved request — model already selected upstream."""
    request_id: str
    org_id: str
    model_id: str
    provider: Provider
    api_endpoint: str = ""

    messages: list[Message]
    tools: list[ToolDefinition] | None = None
    tool_choice: str | dict | None = None

    temperature: float = 0.7
    max_tokens: int | None = None
    top_p: float | None = None
    stop: list[str] | None = None

    stream: bool = False

    # Metadata (pass-through, not sent to provider)
    session_id: str = ""
    run_id: str = ""


# ── Normalized tool call ────────────────────────────────────────

class ToolCallBlock(BaseModel):
    """Unified tool call across all providers."""
    id: str
    name: str
    arguments: str  # JSON string


# ── Completion response ─────────────────────────────────────────

class CompletionResponse(BaseModel):
    """Non-streaming completion response."""
    request_id: str
    content: str = ""
    tool_calls: list[ToolCallBlock] = Field(default_factory=list)
    finish_reason: str = "stop"

    model_used: str = ""
    provider: str = ""
    tokens_in: int = 0
    tokens_out: int = 0
    cost_nok: float = 0.0
    latency_ms: int = 0

    result_id: str | None = None  # set if content stored in MinIO
    metadata: dict[str, Any] | None = None  # provider-specific metadata


# ── Streaming chunk ─────────────────────────────────────────────

class ChunkType(str, Enum):
    METADATA = "metadata"
    CONTENT = "content"
    TOOL_CALL = "tool_call"
    ERROR = "error"
    DONE = "done"


class CompletionChunk(BaseModel):
    type: ChunkType
    content: str = ""
    tool_call: ToolCallBlock | None = None
    metadata: dict[str, Any] | None = None
    error: str | None = None


class StreamChunk:
    """Lightweight chunk yielded by chat_provider.stream() — consumed by the gRPC servicer."""

    __slots__ = ("delta", "done", "model_used")

    def __init__(self, delta: str = "", done: bool = False, model_used: str = "") -> None:
        self.delta = delta
        self.done = done
        self.model_used = model_used


# ── NATS completion event ───────────────────────────────────────

class CompletionEvent(BaseModel):
    request_id: str
    org_id: str
    session_id: str = ""
    run_id: str = ""
    model_used: str = ""
    provider: str = ""
    tokens_in: int = 0
    tokens_out: int = 0
    cost_nok: float = 0.0
    latency_ms: int = 0
    finish_reason: str = "stop"
    timestamp: datetime = Field(default_factory=datetime.utcnow)
