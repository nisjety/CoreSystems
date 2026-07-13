import asyncio
import json
from typing import Any

import httpx
from hubspot import HubSpot
from odoorpc import ODOO
from simple_salesforce import Salesforce

from app.notion_import import import_from_notion
from app.network_policy import UnsafeOutboundTarget, validate_public_http_url
from app.schemas import ImportDocument


_MAX_HTTP_RESPONSE_BYTES = 10 * 1024 * 1024
_MAX_HTTP_RECORDS = 1_000
_FORBIDDEN_FORWARD_HEADERS = {
    "connection",
    "content-length",
    "host",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
}


def _connector_headers(value: Any) -> dict[str, str]:
    if not isinstance(value, dict):
        raise ValueError("connector headers must be an object")
    headers: dict[str, str] = {}
    for name, raw_value in value.items():
        normalized = str(name).strip().lower()
        if not normalized or normalized in _FORBIDDEN_FORWARD_HEADERS:
            raise ValueError(f"connector header is not permitted: {name}")
        if not isinstance(raw_value, str):
            raise ValueError(f"connector header value must be a string: {name}")
        headers[str(name)] = raw_value
    return headers

__all__ = [
    "import_from_notion",
    "import_from_hubspot",
    "import_from_salesforce",
    "import_from_odoo",
    "import_from_http_system",
    "import_from_source",
]


async def import_from_hubspot(connection: dict[str, Any], options: dict[str, Any]) -> list[ImportDocument]:
    client = HubSpot(access_token=connection.get("access_token"))
    object_type = options.get("object_type", "contacts")
    limit = max(1, min(int(options.get("limit", 100)), 1_000))
    response = await asyncio.to_thread(
        client.crm.objects.basic_api.get_page, object_type=object_type, limit=limit
    )
    documents: list[ImportDocument] = []
    for item in response.results:
        item_dict = item.to_dict()
        documents.append(
            ImportDocument(
                source_id=item_dict.get("id"),
                source_name=item_dict.get("id"),
                title=item_dict.get("id"),
                text=str(item_dict),
                metadata={"source": "hubspot", "object_type": object_type},
            )
        )
    return documents


async def import_from_salesforce(connection: dict[str, Any], options: dict[str, Any]) -> list[ImportDocument]:
    sf = Salesforce(
        username=connection.get("username"),
        password=connection.get("password"),
        security_token=connection.get("security_token"),
        domain=connection.get("domain", "login"),
    )
    query = options.get("query", "SELECT Id, Name FROM Account LIMIT 100")
    result = await asyncio.to_thread(sf.query_all, query)
    documents: list[ImportDocument] = []
    for record in result.get("records", []):
        source_id = record.get("Id")
        source_name = record.get("Name") or source_id
        documents.append(
            ImportDocument(
                source_id=source_id,
                source_name=source_name,
                title=source_name,
                text=str(record),
                metadata={"source": "salesforce", "query": query},
            )
        )
    return documents


async def import_from_odoo(connection: dict[str, Any], options: dict[str, Any]) -> list[ImportDocument]:
    host = connection.get("host")
    port = int(connection.get("port", 8069))
    if not isinstance(host, str) or not host:
        raise ValueError("Odoo host is required")
    if port < 1 or port > 65535:
        raise ValueError("Odoo port is invalid")
    host_url = host if "://" in host else f"http://{host}:{port}"
    validated = await validate_public_http_url(host_url)
    validated_host = httpx.URL(validated).host
    model = options.get("model", "product.template")
    fields = options.get("fields", ["id", "name", "description"])
    domain = options.get("domain", [])
    limit = max(1, min(int(options.get("limit", 100)), 1_000))

    def fetch_records() -> list[dict[str, Any]]:
        odoo = ODOO(validated_host, port=port)
        odoo.login(
            connection.get("database"), connection.get("username"), connection.get("password")
        )
        return odoo.env[model].search_read(domain, fields, limit=limit)

    records = await asyncio.to_thread(fetch_records)
    documents: list[ImportDocument] = []
    for record in records:
        source_id = str(record.get("id"))
        source_name = str(record.get("name") or source_id)
        documents.append(
            ImportDocument(
                source_id=source_id,
                source_name=source_name,
                title=source_name,
                text=str(record),
                metadata={"source": "odoo", "model": model},
            )
        )
    return documents


async def import_from_http_system(
    source_name: str,
    connection: dict[str, Any],
    options: dict[str, Any],
) -> list[ImportDocument]:
    url = await validate_public_http_url(connection.get("url"))
    headers = _connector_headers(connection.get("headers", {}))
    params = options.get("params", {})
    if not isinstance(params, dict):
        raise ValueError("connector params must be an object")
    async with httpx.AsyncClient(timeout=30.0, follow_redirects=False) as client:
        async with client.stream("GET", url, headers=headers, params=params) as response:
            if response.is_redirect:
                raise UnsafeOutboundTarget("connector redirects are not permitted")
            response.raise_for_status()
            chunks: list[bytes] = []
            total = 0
            async for chunk in response.aiter_bytes():
                total += len(chunk)
                if total > _MAX_HTTP_RESPONSE_BYTES:
                    raise ValueError("connector response exceeds 10 MiB")
                chunks.append(chunk)
    try:
        payload = json.loads(b"".join(chunks))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ValueError("connector response must be valid JSON") from exc
    if isinstance(payload, list):
        items = payload
    elif isinstance(payload, dict):
        items = payload.get("items", [payload])
    else:
        raise ValueError("connector response must be an object or an array")
    if not isinstance(items, list) or len(items) > _MAX_HTTP_RECORDS:
        raise ValueError("connector response contains an invalid number of items")
    documents: list[ImportDocument] = []
    for item in items:
        if not isinstance(item, dict):
            raise ValueError("connector response items must be objects")
        source_id = str(item.get("id") or item.get("uuid") or "")
        title = str(item.get("name") or item.get("title") or source_id or "record")
        documents.append(
            ImportDocument(
                source_id=source_id or None,
                source_name=title,
                title=title,
                text=str(item),
                metadata={"source": source_name, "url": url},
            )
        )
    return documents


async def import_from_source(
    source_type: str,
    connection: dict[str, Any],
    options: dict[str, Any],
) -> list[ImportDocument]:
    source_type_lower = source_type.lower()
    if source_type_lower == "notion":
        return await import_from_notion(connection, options)
    if source_type_lower in {"crm", "hubspot"}:
        return await import_from_hubspot(connection, options)
    if source_type_lower == "salesforce":
        return await import_from_salesforce(connection, options)
    if source_type_lower in {"erp", "pim", "odoo"}:
        return await import_from_odoo(connection, options)
    if source_type_lower == "cms":
        return await import_from_http_system("cms", connection, options)
    raise ValueError(f"Unsupported source type: {source_type}")
