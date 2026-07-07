"""Notion import connector.

Fetches the REAL page body for every Notion page (via ``blocks.children.list``)
instead of storing a stringified metadata dict as the document text. Kept in its
own module so it only depends on ``notion_client`` + the shared schema, and can be
unit-tested without pulling in the heavier connector SDKs (HubSpot, Odoo, …).
"""

from typing import Any

from notion_client import Client as NotionClient
from notion_client.helpers import collect_paginated_api

from app.schemas import ImportDocument

# Light-markdown prefixes per Notion block type. Blocks not listed contribute
# their rich_text with no prefix; blocks with no rich_text (dividers, images,
# …) produce no line at all.
_NOTION_BLOCK_PREFIX: dict[str, str] = {
    "heading_1": "# ",
    "heading_2": "## ",
    "heading_3": "### ",
    "bulleted_list_item": "- ",
    "numbered_list_item": "- ",
    "to_do": "- ",
    "quote": "> ",
    "callout": "> ",
}


def flatten_rich_text(rich_text: Any) -> str:
    """Join a Notion ``rich_text`` array into a single plain-text string."""
    if not isinstance(rich_text, list):
        return ""
    return "".join(
        part.get("plain_text", "")
        for part in rich_text
        if isinstance(part, dict)
    )


def block_to_line(block: dict[str, Any]) -> str:
    """Render one Notion block to a single plain-text line (light markdown).

    Returns "" for blocks carrying no rich_text (dividers, images, embeds, …),
    which the caller drops.
    """
    if not isinstance(block, dict):
        return ""
    block_type = block.get("type", "")
    payload = block.get(block_type)
    if not isinstance(payload, dict):
        return ""
    text = flatten_rich_text(payload.get("rich_text", []))
    if not text:
        return ""
    return _NOTION_BLOCK_PREFIX.get(block_type, "") + text


def render_blocks(blocks: list[dict[str, Any]]) -> str:
    """Flatten a list of Notion blocks into a plain-text document body."""
    lines = [block_to_line(block) for block in (blocks or [])]
    return "\n".join(line for line in lines if line)


def fetch_page_text(notion: NotionClient, page_id: str) -> str:
    """Fetch a page's block children (all pages) plus one level of nested
    children (toggles, list items, callouts), flattened to plain text.

    Best-effort: returns "" on any API error so one unreadable page never aborts
    the whole import run.
    """
    try:
        top_level = list(
            collect_paginated_api(notion.blocks.children.list, block_id=page_id)
        )
    except Exception:
        return ""

    collected: list[dict[str, Any]] = []
    for block in top_level:
        collected.append(block)
        if isinstance(block, dict) and block.get("has_children") and block.get("id"):
            try:
                children = collect_paginated_api(
                    notion.blocks.children.list, block_id=block["id"]
                )
            except Exception:
                children = []
            collected.extend(children)
    return render_blocks(collected)


def extract_title(item: dict[str, Any]) -> str | None:
    """Pull the page/database title out of its ``properties`` map."""
    properties = item.get("properties", {})
    if isinstance(properties, dict):
        for value in properties.values():
            if isinstance(value, dict) and value.get("type") == "title":
                return flatten_rich_text(value.get("title", []))
    return None


async def import_from_notion(
    connection: dict[str, Any], options: dict[str, Any]
) -> list[ImportDocument]:
    notion = NotionClient(auth=connection.get("token"))
    query: dict[str, Any] = {}
    if options.get("filter"):
        query["filter"] = options["filter"]
    if options.get("sort"):
        query["sort"] = options["sort"]

    result = notion.search(**query)
    documents: list[ImportDocument] = []
    for item in result.get("results", []):
        title = extract_title(item)
        obj = item.get("object")
        page_id = item.get("id")

        # Fetch the REAL page body via blocks.children.list. Only pages have
        # block children; database objects are indexed by metadata (their rows
        # surface as their own pages through search).
        body = ""
        if obj == "page" and page_id:
            body = fetch_page_text(notion, page_id)

        text = body or title or f"Untitled Notion {obj or 'object'}"
        documents.append(
            ImportDocument(
                source_id=page_id,
                source_name=title,
                title=title,
                text=text,
                metadata={
                    "source": "notion",
                    "object": obj,
                    "url": item.get("url"),
                    "has_body": bool(body),
                },
            )
        )
    return documents
