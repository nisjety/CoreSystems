"""LLM provider adapters."""

from __future__ import annotations

from typing import Any, AsyncIterator, Protocol

from reasoning_runtime.domain import CompletionRequest, CompletionResponse


class LLMProvider(Protocol):
    """Common interface every provider must implement."""

    async def complete(self, req: CompletionRequest) -> CompletionResponse: ...

    async def stream(
        self, req: CompletionRequest
    ) -> AsyncIterator[dict[str, Any]]: ...
