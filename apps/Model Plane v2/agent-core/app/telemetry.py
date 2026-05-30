"""OpenTelemetry instrumentation for agent-core v2.

Provides distributed tracing across:
- HTTP requests (FastAPI auto-instrumentation)
- NATS message handling
- LLM calls (custom spans)
- Tool execution (custom spans)

Exports traces to OTEL_EXPORTER_OTLP_ENDPOINT (default: http://localhost:4317).
Falls back to no-op if OTel dependencies are not installed.
"""

from __future__ import annotations

import logging
from contextlib import contextmanager
from typing import Any, Generator

from app.config import settings

logger = logging.getLogger(__name__)

# Module-level state
_tracer: Any = None
_initialized = False


def init_telemetry() -> None:
    """Initialize OpenTelemetry tracing if dependencies are available."""
    global _tracer, _initialized

    if _initialized:
        return

    try:
        from opentelemetry import trace
        from opentelemetry.sdk.trace import TracerProvider
        from opentelemetry.sdk.trace.export import BatchSpanProcessor
        from opentelemetry.sdk.resources import Resource, SERVICE_NAME, SERVICE_VERSION
        from opentelemetry.exporter.otlp.proto.grpc.trace_exporter import OTLPSpanExporter

        resource = Resource.create({
            SERVICE_NAME: settings.service_name,
            SERVICE_VERSION: settings.service_version,
            "deployment.environment": settings.environment,
        })

        provider = TracerProvider(resource=resource)

        otlp_endpoint = settings.otel_exporter_endpoint
        if otlp_endpoint:
            exporter = OTLPSpanExporter(endpoint=otlp_endpoint, insecure=True)
            provider.add_span_processor(BatchSpanProcessor(exporter))
            logger.info("otel_traces_enabled", extra={"endpoint": otlp_endpoint})

        trace.set_tracer_provider(provider)
        _tracer = trace.get_tracer(settings.service_name, settings.service_version)
        _initialized = True

        # Auto-instrument FastAPI if available
        try:
            from opentelemetry.instrumentation.fastapi import FastAPIInstrumentor
            FastAPIInstrumentor.instrument()
        except ImportError:
            pass

        # Auto-instrument httpx if available
        try:
            from opentelemetry.instrumentation.httpx import HTTPXClientInstrumentor
            HTTPXClientInstrumentor.instrument()
        except ImportError:
            pass

        logger.info("opentelemetry_initialized")

    except ImportError:
        logger.info("opentelemetry_not_installed", extra={"info": "Running without tracing"})
        _initialized = True  # Don't retry


def get_tracer() -> Any:
    """Get the OpenTelemetry tracer (or a no-op stub)."""
    if _tracer is not None:
        return _tracer

    # Return a no-op tracer if OTel not available
    return _NoOpTracer()


@contextmanager
def trace_span(
    name: str,
    attributes: dict[str, Any] | None = None,
) -> Generator[Any, None, None]:
    """Context manager to create a traced span.

    Usage:
        with trace_span("llm.complete", {"model": "claude-haiku"}) as span:
            result = await llm.complete(...)
            span.set_attribute("tokens", result.tokens)
    """
    tracer = get_tracer()
    with tracer.start_as_current_span(name) as span:
        if attributes:
            for k, v in attributes.items():
                span.set_attribute(k, str(v) if not isinstance(v, (int, float, bool)) else v)
        yield span


def trace_agent_run(run_id: str, org_id: str, goal: str) -> Any:
    """Start a top-level span for an agent run."""
    tracer = get_tracer()
    return tracer.start_as_current_span(
        "agent.run",
        attributes={
            "agent.run_id": run_id,
            "agent.org_id": org_id,
            "agent.goal": goal[:200],
        },
    )


def trace_tool_execution(tool_name: str, run_id: str) -> Any:
    """Start a span for tool execution."""
    tracer = get_tracer()
    return tracer.start_as_current_span(
        f"tool.{tool_name}",
        attributes={
            "tool.name": tool_name,
            "agent.run_id": run_id,
        },
    )


def trace_llm_call(model: str, provider: str, run_id: str) -> Any:
    """Start a span for an LLM call."""
    tracer = get_tracer()
    return tracer.start_as_current_span(
        "llm.complete",
        attributes={
            "llm.model": model,
            "llm.provider": provider,
            "agent.run_id": run_id,
        },
    )


class _NoOpSpan:
    """No-op span for when OTel is not installed."""

    def set_attribute(self, key: str, value: Any) -> None:
        pass

    def set_status(self, status: Any) -> None:
        pass

    def record_exception(self, exc: Exception) -> None:
        pass

    def __enter__(self) -> "_NoOpSpan":
        return self

    def __exit__(self, *args: Any) -> None:
        pass


class _NoOpTracer:
    """No-op tracer for when OTel is not installed."""

    def start_as_current_span(self, name: str, **kwargs: Any) -> _NoOpSpan:
        return _NoOpSpan()
