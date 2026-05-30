"""Shared LLM provider runtime for Model Plane v2 services.

Both ai-core and agent-core import this package directly to execute
LLM completions, stream responses, and manage result storage — no
inter-service HTTP hop required.
"""

from reasoning_runtime.config import RuntimeConfig, configure, get_config
from reasoning_runtime.domain import (
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
from reasoning_runtime.executor import execute, execute_stream

__all__ = [
    "ChunkType",
    "CompletionChunk",
    "CompletionEvent",
    "CompletionRequest",
    "CompletionResponse",
    "Message",
    "Provider",
    "RuntimeConfig",
    "ToolCallBlock",
    "ToolDefinition",
    "configure",
    "execute",
    "execute_stream",
    "get_config",
]
