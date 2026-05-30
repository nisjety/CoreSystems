"""Tests for Phase B6: MCP — Resources, normalization, channel permissions."""

from __future__ import annotations

import pytest

from app.mcp.resources import (
    McpResource,
    McpResourceContent,
    ResourceManager,
    ResourceProvider,
)
from app.mcp.normalization import (
    is_mcp_tool,
    normalize_tool_name,
    parse_normalized_name,
)
from app.mcp.channel_permissions import (
    clear_all,
    get_allowlist,
    is_server_allowed,
    remove_allowlist,
    set_allowlist,
)


# ── Normalization ──────────────────────────────────────────────

class TestNormalization:
    def test_basic(self):
        assert normalize_tool_name("github", "create_pr") == "mcp__github__create_pr"

    def test_special_chars_cleaned(self):
        result = normalize_tool_name("my-server", "do.thing")
        assert result == "mcp__my_server__do_thing"

    def test_parse_roundtrip(self):
        name = normalize_tool_name("srv", "tool")
        parsed = parse_normalized_name(name)
        assert parsed == ("srv", "tool")

    def test_parse_non_mcp_returns_none(self):
        assert parse_normalized_name("local_tool") is None

    def test_parse_missing_tool_returns_none(self):
        assert parse_normalized_name("mcp__serveronly") is None

    def test_is_mcp_tool_true(self):
        assert is_mcp_tool("mcp__github__search") is True

    def test_is_mcp_tool_false(self):
        assert is_mcp_tool("bash") is False


# ── Channel Permissions ────────────────────────────────────────

class TestChannelPermissions:
    def setup_method(self):
        clear_all()

    def teardown_method(self):
        clear_all()

    def test_permissive_default(self):
        assert is_server_allowed("org1", "anything") is True

    def test_allowlist_blocks(self):
        set_allowlist("org1", ["github"])
        assert is_server_allowed("org1", "github") is True
        assert is_server_allowed("org1", "slack") is False

    def test_wildcard_allows_all(self):
        set_allowlist("org1", ["*"])
        assert is_server_allowed("org1", "slack") is True

    def test_remove_reverts_to_permissive(self):
        set_allowlist("org1", ["github"])
        remove_allowlist("org1")
        assert is_server_allowed("org1", "slack") is True

    def test_get_allowlist(self):
        set_allowlist("org1", ["a", "b"])
        assert get_allowlist("org1") == {"a", "b"}

    def test_get_allowlist_unset(self):
        assert get_allowlist("org1") == set()


# ── Resource Models ────────────────────────────────────────────

class TestResourceModels:
    def test_mcp_resource_defaults(self):
        r = McpResource(uri="file:///a.txt", name="a")
        assert r.mime_type == "text/plain"
        assert r.description == ""

    def test_mcp_resource_content(self):
        c = McpResourceContent(uri="file:///a.txt", content="hello")
        assert c.content == "hello"


# ── ResourceManager ────────────────────────────────────────────

def _make_provider(
    resources: list[McpResource] | None = None,
    read_map: dict[str, McpResourceContent] | None = None,
) -> ResourceProvider:
    async def list_fn() -> list[McpResource]:
        return resources or []

    async def read_fn(uri: str) -> McpResourceContent | None:
        if read_map:
            return read_map.get(uri)
        return None

    return ResourceProvider(list_fn=list_fn, read_fn=read_fn)


class TestResourceManager:
    @pytest.mark.asyncio
    async def test_list_single_server(self):
        mgr = ResourceManager()
        res = McpResource(uri="f://a", name="a")
        mgr.register_provider("srv", _make_provider(resources=[res]))
        listed = await mgr.list_resources("srv")
        assert len(listed) == 1
        assert listed[0].name == "a"

    @pytest.mark.asyncio
    async def test_list_all_servers(self):
        mgr = ResourceManager()
        r1 = McpResource(uri="f://a", name="a")
        r2 = McpResource(uri="f://b", name="b")
        mgr.register_provider("s1", _make_provider(resources=[r1]))
        mgr.register_provider("s2", _make_provider(resources=[r2]))
        listed = await mgr.list_resources()
        assert len(listed) == 2

    @pytest.mark.asyncio
    async def test_list_unknown_server(self):
        mgr = ResourceManager()
        assert await mgr.list_resources("nope") == []

    @pytest.mark.asyncio
    async def test_read_resource(self):
        mgr = ResourceManager()
        content = McpResourceContent(uri="f://a", content="data")
        mgr.register_provider("srv", _make_provider(read_map={"f://a": content}))
        result = await mgr.read_resource("srv", "f://a")
        assert result is not None
        assert result.content == "data"

    @pytest.mark.asyncio
    async def test_read_unknown_server(self):
        mgr = ResourceManager()
        assert await mgr.read_resource("nope", "f://a") is None

    @pytest.mark.asyncio
    async def test_unregister_provider(self):
        mgr = ResourceManager()
        mgr.register_provider("srv", _make_provider(resources=[]))
        mgr.unregister_provider("srv")
        assert await mgr.list_resources("srv") == []

    @pytest.mark.asyncio
    async def test_list_survives_provider_error(self):
        async def bad_list():
            raise RuntimeError("boom")

        mgr = ResourceManager()
        mgr.register_provider("bad", ResourceProvider(list_fn=bad_list))
        mgr.register_provider(
            "good",
            _make_provider(resources=[McpResource(uri="f://x", name="x")]),
        )
        listed = await mgr.list_resources()
        assert len(listed) == 1
