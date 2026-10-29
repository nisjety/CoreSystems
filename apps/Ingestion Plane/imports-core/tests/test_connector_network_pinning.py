"""Customer-controlled connectors must carry their DNS preflight to connection time."""

import asyncio
from types import SimpleNamespace

import httpx
import pytest

from app import connectors
from app.network_policy import PublicHttpTarget


def test_http_connector_uses_the_pinned_transport(monkeypatch: pytest.MonkeyPatch) -> None:
    target = PublicHttpTarget(
        url="https://customer.example/records",
        hostname="customer.example",
        port=443,
        addresses=("93.184.216.34",),
    )
    seen: list[PublicHttpTarget] = []

    async def resolve(_raw_url: str) -> PublicHttpTarget:
        return target

    def create_client(received_target: PublicHttpTarget, *, timeout: float) -> httpx.AsyncClient:
        assert timeout == 30.0
        seen.append(received_target)
        return httpx.AsyncClient(
            transport=httpx.MockTransport(
                lambda _request: httpx.Response(200, json={"items": [{"id": "1", "name": "Record"}]})
            )
        )

    monkeypatch.setattr(connectors, "resolve_public_http_target", resolve)
    monkeypatch.setattr(connectors, "create_pinned_async_http_client", create_client)

    documents = asyncio.run(
        connectors.import_from_http_system(
            "cms",
            {"url": "https://customer.example/records", "headers": {}},
            {"params": {}},
        )
    )

    assert seen == [target]
    assert [(document.source_id, document.title) for document in documents] == [("1", "Record")]


def test_odoo_connector_uses_a_pinned_opener_and_original_tls_hostname(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    target = PublicHttpTarget(
        url="https://odoo.customer.example/jsonrpc",
        hostname="odoo.customer.example",
        port=443,
        addresses=("93.184.216.34",),
    )
    seen: dict[str, object] = {}

    async def resolve(_raw_url: str) -> PublicHttpTarget:
        return target

    def opener(received_target: PublicHttpTarget) -> object:
        seen["opener_target"] = received_target
        seen["opener"] = object()
        return seen["opener"]

    class FakeOdoo:
        def __init__(self, host: str, **kwargs: object) -> None:
            seen["host"] = host
            seen["kwargs"] = kwargs
            self.env = {"product.template": SimpleNamespace(search_read=lambda *_args, **_kwargs: [])}

        def login(self, *_args: object) -> None:
            return None

    monkeypatch.setattr(connectors, "resolve_public_http_target", resolve)
    monkeypatch.setattr(connectors, "create_pinned_urllib_opener", opener)
    monkeypatch.setattr(connectors, "ODOO", FakeOdoo)

    documents = asyncio.run(
        connectors.import_from_odoo(
            {"host": "https://odoo.customer.example", "port": 8069},
            {},
        )
    )

    assert documents == []
    assert seen["host"] == "odoo.customer.example"
    assert seen["opener_target"] == target
    assert seen["kwargs"] == {"protocol": "jsonrpc+ssl", "port": 443, "opener": seen["opener"]}
