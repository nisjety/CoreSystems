"""Quota checks must fail closed when Control Plane cannot make a decision."""

import asyncio
from types import SimpleNamespace

import httpx
import pytest

from app import service
from app.service import ImportService, QuotaCheckUnavailable


def _service_with_response(handler) -> ImportService:
    instance = ImportService()
    instance._settings = SimpleNamespace(
        org_service_url="http://org-core",
        org_service_quota_path="/api/v1/org/quota/check",
    )
    service.http_client = httpx.AsyncClient(transport=httpx.MockTransport(handler))
    service._quota_cache.clear()
    return instance


@pytest.mark.parametrize(
    "response",
    [
        httpx.Response(404, json={"error": {"code": "not_found"}}),
        httpx.Response(503, json={"error": {"code": "unavailable"}}),
        httpx.Response(200, json={}),
        httpx.Response(200, json={"allowed": "yes"}),
    ],
)
def test_quota_dependency_failures_never_allow_imports(response: httpx.Response) -> None:
    instance = _service_with_response(lambda _request: response)

    async def run() -> None:
        with pytest.raises(QuotaCheckUnavailable):
            await instance.check_quota("org-1", 2)
        await service.http_client.aclose()

    asyncio.run(run())


def test_quota_requires_an_explicit_boolean_decision_and_caches_it() -> None:
    calls = 0

    def handler(_request: httpx.Request) -> httpx.Response:
        nonlocal calls
        calls += 1
        return httpx.Response(200, json={"allowed": False})

    instance = _service_with_response(handler)

    async def run() -> None:
        assert await instance.check_quota("org-1", 2) is False
        assert await instance.check_quota("org-1", 2) is False
        await service.http_client.aclose()

    asyncio.run(run())
    assert calls == 1


def test_quota_transport_failure_is_dependency_unavailable() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        raise httpx.ConnectError("connection refused", request=request)

    instance = _service_with_response(handler)

    async def run() -> None:
        with pytest.raises(QuotaCheckUnavailable):
            await instance.check_quota("org-1", 1)
        await service.http_client.aclose()

    asyncio.run(run())
