"""Tests for the Notion content connector.

These exercise the block-flattening logic and the end-to-end import against a
fake Notion client, proving the connector now stores the REAL page body (fetched
via blocks.children.list) instead of a stringified metadata dict.
"""

import asyncio

from app import notion_import


def test_flatten_rich_text_joins_plain_text():
    rich = [{"plain_text": "Hello "}, {"plain_text": "world"}, {"no": "plain"}]
    assert notion_import.flatten_rich_text(rich) == "Hello world"
    assert notion_import.flatten_rich_text(None) == ""
    assert notion_import.flatten_rich_text("nope") == ""


def test_block_to_line_prefixes_and_skips():
    para = {"type": "paragraph", "paragraph": {"rich_text": [{"plain_text": "a body line"}]}}
    h1 = {"type": "heading_1", "heading_1": {"rich_text": [{"plain_text": "Title"}]}}
    bullet = {"type": "bulleted_list_item", "bulleted_list_item": {"rich_text": [{"plain_text": "point"}]}}
    divider = {"type": "divider", "divider": {}}
    image = {"type": "image", "image": {"file": {"url": "x"}}}

    assert notion_import.block_to_line(para) == "a body line"
    assert notion_import.block_to_line(h1) == "# Title"
    assert notion_import.block_to_line(bullet) == "- point"
    assert notion_import.block_to_line(divider) == ""  # no rich_text -> dropped
    assert notion_import.block_to_line(image) == ""


def test_render_blocks_joins_nonempty_lines():
    blocks = [
        {"type": "heading_1", "heading_1": {"rich_text": [{"plain_text": "Doc"}]}},
        {"type": "divider", "divider": {}},
        {"type": "paragraph", "paragraph": {"rich_text": [{"plain_text": "First para."}]}},
    ]
    assert notion_import.render_blocks(blocks) == "# Doc\nFirst para."


class _FakeChildren:
    """Fake for notion.blocks.children — returns canned pages keyed by block_id.

    Shaped so the REAL collect_paginated_api helper drives it (results/has_more/
    next_cursor), exercising the pagination glue too.
    """

    def __init__(self, by_block):
        self._by_block = by_block

    def list(self, block_id, start_cursor=None, page_size=None, **_):
        return {
            "results": self._by_block.get(block_id, []),
            "has_more": False,
            "next_cursor": None,
        }


class _FakeBlocks:
    def __init__(self, by_block):
        self.children = _FakeChildren(by_block)


class _FakeNotion:
    def __init__(self, search_result, by_block):
        self._search_result = search_result
        self.blocks = _FakeBlocks(by_block)

    def search(self, **_):
        return self._search_result


def test_import_from_notion_fetches_real_page_body(monkeypatch):
    page = {
        "object": "page",
        "id": "page-123",
        "url": "https://notion.so/page-123",
        "properties": {
            "Name": {"type": "title", "title": [{"plain_text": "Runbook"}]},
        },
    }
    blocks = {
        "page-123": [
            {"type": "heading_1", "heading_1": {"rich_text": [{"plain_text": "Runbook"}]}},
            {"type": "paragraph", "paragraph": {"rich_text": [{"plain_text": "Step one: do the thing."}]}},
            {
                "type": "toggle",
                "id": "toggle-1",
                "has_children": True,
                "toggle": {"rich_text": [{"plain_text": "Details"}]},
            },
        ],
        "toggle-1": [
            {"type": "paragraph", "paragraph": {"rich_text": [{"plain_text": "Nested detail."}]}},
        ],
    }
    fake = _FakeNotion({"results": [page]}, blocks)
    monkeypatch.setattr(notion_import, "NotionClient", lambda auth=None: fake)

    docs = asyncio.run(notion_import.import_from_notion({"token": "t"}, {}))
    assert len(docs) == 1
    doc = docs[0]

    # Real body — NOT a stringified dict.
    assert "Step one: do the thing." in doc.text
    assert "Nested detail." in doc.text  # nested toggle child captured
    assert "object" not in doc.text  # would be present if we str(item)'d the dict
    assert doc.title == "Runbook"
    assert doc.metadata["has_body"] is True
    assert doc.metadata["source"] == "notion"
    assert doc.metadata["url"] == "https://notion.so/page-123"


def test_import_from_notion_database_object_stays_metadata(monkeypatch):
    # Databases have no block children; we must not attempt to fetch a body, and
    # should fall back to the title (never crash, never str(item)).
    db = {
        "object": "database",
        "id": "db-1",
        "properties": {"Name": {"type": "title", "title": [{"plain_text": "Tasks DB"}]}},
    }
    fake = _FakeNotion({"results": [db]}, {})
    monkeypatch.setattr(notion_import, "NotionClient", lambda auth=None: fake)

    docs = asyncio.run(notion_import.import_from_notion({"token": "t"}, {}))
    assert len(docs) == 1
    assert docs[0].text == "Tasks DB"
    assert docs[0].metadata["has_body"] is False


def test_import_from_notion_body_fetch_failure_is_best_effort(monkeypatch):
    page = {
        "object": "page",
        "id": "page-err",
        "properties": {"Name": {"type": "title", "title": [{"plain_text": "Broken"}]}},
    }

    class _BoomChildren:
        def list(self, **_):
            raise RuntimeError("notion 500")

    class _BoomBlocks:
        children = _BoomChildren()

    class _BoomNotion:
        blocks = _BoomBlocks()

        def search(self, **_):
            return {"results": [page]}

    monkeypatch.setattr(notion_import, "NotionClient", lambda auth=None: _BoomNotion())

    docs = asyncio.run(notion_import.import_from_notion({"token": "t"}, {}))
    # Body fetch failed -> falls back to the title, does not raise.
    assert docs[0].text == "Broken"
    assert docs[0].metadata["has_body"] is False
