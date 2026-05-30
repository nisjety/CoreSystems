"""Pipeline runner — threads a PipelineContext through all 10 layers."""

from __future__ import annotations

import asyncio
import logging
import time
from typing import Any, AsyncIterator

from app.domain import (
    ChatRequest,
    ChatResponse,
    CompletionChunk,
    CompletionRequest,
    Message,
    PipelineContext,
    Provider,
    SafetyVerdict,
)
from app.pipeline.layers import LAYERS
from app.services import usage_reporter
from app.services.inference_metrics import get_inference_metrics

logger = logging.getLogger(__name__)


async def run_pipeline(
    *,
    request_id: str,
    message: str,
    org_id: str = "",
    session_id: str = "",
    model: str = "",
    provider: str = "",
    temperature: float = 0.7,
    max_tokens: int | None = None,
    stream: bool = False,
    tools: list | None = None,
    context: dict[str, Any] | None = None,
) -> ChatResponse:
    """Run the full 10-layer pipeline synchronously (non-streaming)."""

    ctx = _build_context(
        request_id=request_id,
        message=message,
        org_id=org_id,
        session_id=session_id,
        model=model,
        provider=provider,
        temperature=temperature,
        max_tokens=max_tokens,
        tools=tools,
        context=context,
    )

    # Execute layers 1-10 in sequence
    for layer_name, layer_fn in LAYERS:
        t0 = time.monotonic_ns()
        ctx = await layer_fn(ctx)  # type: ignore[operator]
        elapsed_ms = (time.monotonic_ns() - t0) // 1_000_000
        ctx.layer_timings[layer_name] = elapsed_ms

        # Short-circuit if safety blocked
        if ctx.pre_safety == SafetyVerdict.BLOCKED:
            logger.warning("pipeline_blocked layer=%s request_id=%s", layer_name, request_id)
            return _blocked_response(ctx)
        if ctx.post_safety == SafetyVerdict.BLOCKED:
            logger.warning("pipeline_blocked_post layer=%s request_id=%s", layer_name, request_id)
            return _blocked_response(ctx)

    total_ms = sum(ctx.layer_timings.values())
    logger.info(
        "pipeline_complete request_id=%s intent=%s model=%s total_ms=%d",
        request_id,
        ctx.intent.value,
        ctx.resolved_model,
        total_ms,
    )

    response = _build_response(ctx)
    _fire_usage(ctx)
    return response


async def run_pipeline_stream(
    *,
    request_id: str,
    message: str,
    org_id: str = "",
    session_id: str = "",
    model: str = "",
    provider: str = "",
    temperature: float = 0.7,
    max_tokens: int | None = None,
    tools: list | None = None,
    context: dict[str, Any] | None = None,
) -> AsyncIterator[CompletionChunk]:
    """Run layers 1-7 (pre-execution), then stream layer 8, then run 9-10 post."""

    from reasoning_runtime import execute_stream

    ctx = _build_context(
        request_id=request_id,
        message=message,
        org_id=org_id,
        session_id=session_id,
        model=model,
        provider=provider,
        temperature=temperature,
        max_tokens=max_tokens,
        tools=tools,
        context=context,
    )

    # Run pre-execution layers (1-7)
    pre_layers = LAYERS[:7]
    for layer_name, layer_fn in pre_layers:
        t0 = time.monotonic_ns()
        ctx = await layer_fn(ctx)  # type: ignore[operator]
        elapsed_ms = (time.monotonic_ns() - t0) // 1_000_000
        ctx.layer_timings[layer_name] = elapsed_ms

        if ctx.pre_safety == SafetyVerdict.BLOCKED:
            from app.domain import ChunkType

            yield CompletionChunk(
                type=ChunkType.ERROR,
                content="Content blocked by safety filter",
                model=ctx.resolved_model,
            )
            return

    # Build CompletionRequest for streaming execution
    req = _build_completion_request(ctx, message)

    # Stream layer 8 directly
    async for chunk in execute_stream(req):
        yield chunk

    # Layers after rag_reflect run post-stream (best-effort on final content)
    _post_stream_names = {"safety_post", "format"}
    post_layers = [(n, fn) for n, fn in LAYERS if n in _post_stream_names]
    for layer_name, layer_fn in post_layers:
        t0 = time.monotonic_ns()
        ctx = await layer_fn(ctx)  # type: ignore[operator]
        elapsed_ms = (time.monotonic_ns() - t0) // 1_000_000
        ctx.layer_timings[layer_name] = elapsed_ms

    _fire_usage(ctx)


# ── Internal helpers ────────────────────────────────────────────


def _build_context(
    *,
    request_id: str,
    message: str,
    org_id: str,
    session_id: str,
    model: str,
    provider: str,
    temperature: float,
    max_tokens: int | None,
    tools: list | None,
    context: dict[str, Any] | None,
) -> PipelineContext:
    """Construct the initial PipelineContext from the incoming request."""
    # Stash raw values for layers to consume
    raw: dict[str, Any] = {
        "message": message,
        "temperature": temperature,
        "max_tokens": max_tokens,
        "tools": tools or [],
        "extra_context": context or {},
    }

    # If caller explicitly specified a provider, pre-resolve it
    resolved_provider: Provider | None = None
    if provider:
        try:
            resolved_provider = Provider(provider)
        except ValueError:
            pass

    ctx = PipelineContext(
        request_id=request_id,
        org_id=org_id,
        session_id=session_id,
        resolved_provider=resolved_provider,
        resolved_model=model,
        pipeline_start_ms=int(time.time() * 1000),
    )
    ctx._raw = raw  # PrivateAttr must be set post-construction
    return ctx


def _build_completion_request(ctx: PipelineContext, message: str) -> CompletionRequest:
    """Convert the enriched PipelineContext into a CompletionRequest for reasoning_runtime."""
    raw = ctx._raw  # type: ignore[attr-defined]

    messages = []
    if ctx.system_prompt:
        messages.append(Message(role="system", content=ctx.system_prompt))
    messages.append(Message(role="user", content=message))

    return CompletionRequest(
        request_id=ctx.request_id,
        provider=ctx.resolved_provider or Provider.OPENAI,
        model_id=ctx.resolved_model or "gpt-4o-mini",
        messages=messages,
        temperature=raw.get("temperature", 0.7),
        max_tokens=raw.get("max_tokens"),
        tools=raw.get("tools"),
        stream=False,
        org_id=ctx.org_id,
        run_id=ctx.run_id,
    )


def _build_response(ctx: PipelineContext) -> ChatResponse:
    """Build the final ChatResponse from completed PipelineContext."""
    raw = ctx._raw  # type: ignore[attr-defined]
    result = raw.get("result")

    content = ""
    tool_calls = []
    tokens_in = 0
    tokens_out = 0
    finish_reason = "stop"

    if result:
        content = result.content
        tool_calls = result.tool_calls or []
        tokens_in = result.tokens_in
        tokens_out = result.tokens_out
        finish_reason = result.finish_reason

    total_ms = sum(ctx.layer_timings.values())

    return ChatResponse(
        request_id=ctx.request_id,
        content=content,
        tool_calls=tool_calls,
        model_used=ctx.resolved_model,
        provider=ctx.resolved_provider.value if ctx.resolved_provider else "",
        tokens_in=tokens_in,
        tokens_out=tokens_out,
        latency_ms=total_ms,
        intent=ctx.intent.value,
        finish_reason=finish_reason,
    )


def _fire_usage(ctx: PipelineContext) -> None:
    """Fire-and-forget usage event publish.  Never raises."""
    raw = ctx._raw  # type: ignore[attr-defined]
    result = raw.get("result")
    tokens_in = result.tokens_in if result else 0
    tokens_out = result.tokens_out if result else 0
    total_tokens = tokens_in + tokens_out
    if total_tokens == 0:
        return  # nothing billable
    try:
        usage_reporter.report(
            org_id=ctx.org_id,
            metric="inference_tokens",
            quantity=total_tokens,
            source="ai-core",
            metadata={
                "request_id": ctx.request_id,
                "model": ctx.resolved_model,
                "provider": ctx.resolved_provider.value if ctx.resolved_provider else "",
                "tokens_in": tokens_in,
                "tokens_out": tokens_out,
                "intent": ctx.intent.value,
            },
        )
    except Exception:  # pragma: no cover
        pass

    try:
        get_inference_metrics().record(
            org_id=ctx.org_id,
            tokens_in=tokens_in,
            tokens_out=tokens_out,
            latency_ms=sum(ctx.layer_timings.values()),
        )
    except Exception:  # pragma: no cover
        pass


def _blocked_response(ctx: PipelineContext) -> ChatResponse:
    """Return a safe blocked response."""
    return ChatResponse(
        request_id=ctx.request_id,
        content="I'm unable to process this request due to content safety policies.",
        model_used=ctx.resolved_model,
        provider=ctx.resolved_provider.value if ctx.resolved_provider else "",
        latency_ms=sum(ctx.layer_timings.values()),
        intent=ctx.intent.value,
        finish_reason="content_filter",
    )
