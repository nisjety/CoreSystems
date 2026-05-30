"""Tests for Phase B6: MCP — Elicitation handler."""

from __future__ import annotations

import asyncio

import pytest

from app.mcp.elicitation import (
    ElicitationHandler,
    ElicitationRequest,
    ElicitationResponse,
)


def _make_request(**overrides) -> ElicitationRequest:
    defaults = {
        "request_id": "r1",
        "server_name": "github",
        "tool_name": "create_pr",
        "message": "Which branch?",
    }
    defaults.update(overrides)
    return ElicitationRequest(**defaults)


class TestElicitationModels:
    def test_request_fields(self):
        req = _make_request()
        assert req.request_id == "r1"
        assert req.server_name == "github"
        assert req.timeout == 30.0

    def test_response_defaults(self):
        resp = ElicitationResponse(request_id="r1")
        assert resp.action == "provide"
        assert resp.content == {}


class TestElicitationHandler:
    @pytest.mark.asyncio
    async def test_no_callback_cancels(self):
        handler = ElicitationHandler()
        resp = await handler.handle(_make_request())
        assert resp.action == "cancel"

    @pytest.mark.asyncio
    async def test_callback_provides(self):
        async def cb(req: ElicitationRequest) -> ElicitationResponse:
            return ElicitationResponse(
                request_id=req.request_id,
                action="provide",
                content={"branch": "main"},
            )

        handler = ElicitationHandler()
        handler.set_callback(cb)
        resp = await handler.handle(_make_request())
        assert resp.action == "provide"
        assert resp.content["branch"] == "main"

    @pytest.mark.asyncio
    async def test_history_recorded(self):
        handler = ElicitationHandler()
        await handler.handle(_make_request())
        assert len(handler.history) == 1
        req, resp = handler.history[0]
        assert req.request_id == "r1"
        assert resp.action == "cancel"

    @pytest.mark.asyncio
    async def test_clear_history(self):
        handler = ElicitationHandler()
        await handler.handle(_make_request())
        handler.clear_history()
        assert len(handler.history) == 0

    @pytest.mark.asyncio
    async def test_timeout_returns_cancel(self):
        async def slow_cb(req: ElicitationRequest) -> ElicitationResponse:
            await asyncio.sleep(10)
            return ElicitationResponse(request_id=req.request_id)

        handler = ElicitationHandler()
        handler.set_callback(slow_cb)
        resp = await handler.handle(_make_request(timeout=0.05))
        assert resp.action == "cancel"
