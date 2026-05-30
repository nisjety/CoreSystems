from typing import Any

import httpx
from hubspot import HubSpot
from notion_client import Client as NotionClient
from odoorpc import ODOO
from simple_salesforce import Salesforce

from app.schemas import ImportDocument


async def import_from_notion(connection: dict[str, Any], options: dict[str, Any]) -> list[ImportDocument]:
    notion = NotionClient(auth=connection.get("token"))
    query: dict[str, Any] = {}
    if options.get("filter"):
        query["filter"] = options["filter"]
    if options.get("sort"):
        query["sort"] = options["sort"]

    result = notion.search(**query)
    documents: list[ImportDocument] = []
    for item in result.get("results", []):
        title = None
        properties = item.get("properties", {})
        for value in properties.values():
            if value.get("type") == "title":
                parts = value.get("title", [])
                title = "".join(part.get("plain_text", "") for part in parts)
                break
        documents.append(
            ImportDocument(
                source_id=item.get("id"),
                source_name=title,
                title=title,
                text=str(item),
                metadata={"source": "notion", "object": item.get("object")},
            )
        )
    return documents


async def import_from_hubspot(connection: dict[str, Any], options: dict[str, Any]) -> list[ImportDocument]:
    client = HubSpot(access_token=connection.get("access_token"))
    object_type = options.get("object_type", "contacts")
    limit = int(options.get("limit", 100))
    response = client.crm.objects.basic_api.get_page(object_type=object_type, limit=limit)
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
    result = sf.query_all(query)
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
    odoo = ODOO(connection.get("host"), port=int(connection.get("port", 8069)))
    odoo.login(connection.get("database"), connection.get("username"), connection.get("password"))
    model = options.get("model", "product.template")
    fields = options.get("fields", ["id", "name", "description"])
    domain = options.get("domain", [])
    limit = int(options.get("limit", 100))
    records = odoo.env[model].search_read(domain, fields, limit=limit)
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
    url = connection.get("url")
    headers = connection.get("headers", {})
    params = options.get("params", {})
    async with httpx.AsyncClient(timeout=30.0) as client:
        response = await client.get(url, headers=headers, params=params)
        response.raise_for_status()
    payload = response.json()
    items = payload if isinstance(payload, list) else payload.get("items", [payload])
    documents: list[ImportDocument] = []
    for item in items:
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
