"""Tests for the integration-corev2 actions gateway client (httpx MockTransport)."""

import asyncio
import json

import httpx
import pytest

from app.actions_gateway import ActionsGateway, ActionsGatewayError


def _client(handler) -> httpx.AsyncClient:
    return httpx.AsyncClient(transport=httpx.MockTransport(handler))


def test_configured():
    c = _client(lambda r: httpx.Response(200))
    assert ActionsGateway("http://icv2", "key", c).configured() is True
    assert ActionsGateway("", "key", c).configured() is False
    assert ActionsGateway("http://icv2", "", c).configured() is False


def test_list_connections_parses_envelope_and_sends_auth():
    seen = {}

    def handler(request: httpx.Request) -> httpx.Response:
        seen["path"] = request.url.path
        seen["query"] = dict(request.url.params)
        seen["api_key"] = request.headers.get("x-internal-api-key")
        seen["org"] = request.headers.get("x-org-id")
        return httpx.Response(
            200,
            json={"success": True, "data": {"connections": [{"id": "c1", "providerKey": "github"}]}},
        )

    async def run():
        gw = ActionsGateway("http://icv2", "secret", _client(handler))
        return await gw.list_connections("org-9", "github")

    conns = asyncio.run(run())
    assert conns == [{"id": "c1", "providerKey": "github"}]
    assert seen["path"] == "/api/v1/connections"
    assert seen["query"] == {"organizationId": "org-9", "providerKey": "github"}
    assert seen["api_key"] == "secret"
    assert seen["org"] == "org-9"


def test_execute_action_returns_result_and_posts_operation():
    seen = {}

    def handler(request: httpx.Request) -> httpx.Response:
        seen["path"] = request.url.path
        seen["body"] = json.loads(request.content)
        return httpx.Response(
            200,
            json={"success": True, "data": {"action": {"providerKey": "slack", "operation": "slack.messages.list", "result": {"messages": [{"text": "hi"}]}}}},
        )

    async def run():
        gw = ActionsGateway("http://icv2", "secret", _client(handler))
        return await gw.execute_action("conn-7", "slack.messages.list", params={"channel": "C1"}, org_id="org-9")

    result = asyncio.run(run())
    assert result == {"messages": [{"text": "hi"}]}
    assert seen["path"] == "/api/v1/connections/conn-7/actions"
    assert seen["body"]["operation"] == "slack.messages.list"
    assert seen["body"]["params"] == {"channel": "C1"}


def test_non_success_envelope_raises():
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(200, json={"success": False, "error": {"code": "capability_required", "message": "nope"}})

    async def run():
        gw = ActionsGateway("http://icv2", "secret", _client(handler))
        await gw.execute_action("c", "github.readme.get")

    with pytest.raises(ActionsGatewayError):
        asyncio.run(run())


def test_unconfigured_raises():
    async def run():
        gw = ActionsGateway("", "", _client(lambda r: httpx.Response(200)))
        await gw.list_connections("o", "github")

    with pytest.raises(ActionsGatewayError):
        asyncio.run(run())
