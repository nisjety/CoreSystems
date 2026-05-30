from __future__ import annotations

from time import perf_counter

from fastapi import FastAPI, Request
from prometheus_client import Counter, Histogram, make_asgi_app


HTTP_REQUESTS_TOTAL = Counter(
    "dataplane_http_requests_total",
    "Total HTTP requests handled by a data plane service.",
    ["service", "method", "route", "status"],
)

HTTP_REQUEST_DURATION_SECONDS = Histogram(
    "dataplane_http_request_duration_seconds",
    "HTTP request latency for a data plane service.",
    ["service", "method", "route"],
    buckets=(0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0),
)

DOCUMENT_EVENT_PUBLISH_TOTAL = Counter(
    "dataplane_document_event_publish_total",
    "Document event publish attempts and outcomes.",
    ["event", "result"],
)

DOCUMENT_CROSSPLANE_PUBLISH_TOTAL = Counter(
    "dataplane_document_crossplane_publish_total",
    "Cross-plane document publish attempts and outcomes.",
    ["event", "result"],
)


def instrument_app(app: FastAPI, *, service_name: str) -> None:
    """Attach Prometheus metrics middleware and /metrics to a FastAPI app."""
    app.mount("/metrics", make_asgi_app())

    @app.middleware("http")
    async def _metrics_middleware(request: Request, call_next):
        started_at = perf_counter()
        response = await call_next(request)
        route = request.scope.get("route")
        route_path = getattr(route, "path", request.url.path)
        duration = perf_counter() - started_at
        HTTP_REQUEST_DURATION_SECONDS.labels(
            service=service_name,
            method=request.method,
            route=route_path,
        ).observe(duration)
        HTTP_REQUESTS_TOTAL.labels(
            service=service_name,
            method=request.method,
            route=route_path,
            status=str(response.status_code),
        ).inc()
        return response