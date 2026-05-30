"""Internal auth middleware — same pattern as capability-core / agent-core."""

from __future__ import annotations

from starlette.middleware.base import BaseHTTPMiddleware
from starlette.requests import Request
from starlette.responses import JSONResponse

from app.config import get_settings


class InternalAuthMiddleware(BaseHTTPMiddleware):
    async def dispatch(self, request: Request, call_next):
        if request.url.path in ("/health", "/docs", "/openapi.json"):
            return await call_next(request)

        settings = get_settings()
        if not settings.internal_api_key:
            return await call_next(request)

        token = request.headers.get("x-internal-api-key", "")
        if token != settings.internal_api_key:
            return JSONResponse({"error": "unauthorized"}, status_code=401)

        return await call_next(request)
