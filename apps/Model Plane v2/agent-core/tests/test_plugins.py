"""Tests for Phase C6: Plugin Marketplace."""

from __future__ import annotations

import json
import tempfile
from pathlib import Path

import pytest

from app.plugins.domain import (
    ComponentType,
    InstalledPlugin,
    PluginComponent,
    PluginManifest,
    PluginScope,
    PluginState,
)
from app.plugins.loader import PluginLoadError, PluginLoader, validate_manifest
from app.plugins.marketplace import CatalogEntry, MarketplaceClient
from app.plugins.reconciler import PluginReconciler
from app.plugins.registry import (
    PluginConflictError,
    PluginNotFoundError,
    PluginRegistry,
)


def _manifest(
    name: str = "test-plugin", version: str = "1.0.0", **kw
) -> PluginManifest:
    return PluginManifest(name=name, version=version, **kw)


# ── Domain ─────────────────────────────────────────────────────

class TestPluginDomain:
    def test_manifest_id(self):
        m = _manifest()
        assert m.id == "test-plugin@1.0.0"

    def test_installed_plugin_defaults(self):
        p = InstalledPlugin(manifest=_manifest(), scope=PluginScope.USER)
        assert p.state == PluginState.ENABLED
        assert p.id == "test-plugin@1.0.0"

    def test_component_model(self):
        c = PluginComponent(
            type=ComponentType.TOOL, name="my-tool", config={"k": "v"}
        )
        assert c.type == ComponentType.TOOL

    def test_scope_values(self):
        assert PluginScope.MANAGED.value == "managed"


# ── Registry ───────────────────────────────────────────────────

class TestPluginRegistry:
    def test_install_and_get(self):
        reg = PluginRegistry()
        p = reg.install(_manifest(), PluginScope.USER)
        assert reg.get(p.id) is not None
        assert reg.count == 1

    def test_install_duplicate_raises(self):
        reg = PluginRegistry()
        reg.install(_manifest(), PluginScope.USER)
        with pytest.raises(PluginConflictError):
            reg.install(_manifest(), PluginScope.USER)

    def test_uninstall(self):
        reg = PluginRegistry()
        p = reg.install(_manifest(), PluginScope.USER)
        reg.uninstall(p.id)
        assert reg.count == 0

    def test_uninstall_managed_raises(self):
        reg = PluginRegistry()
        p = reg.install(_manifest(), PluginScope.MANAGED)
        with pytest.raises(PluginConflictError):
            reg.uninstall(p.id)

    def test_uninstall_not_found_raises(self):
        reg = PluginRegistry()
        with pytest.raises(PluginNotFoundError):
            reg.uninstall("nope@0.0.0")

    def test_enable_disable(self):
        reg = PluginRegistry()
        p = reg.install(_manifest(), PluginScope.USER)
        reg.disable(p.id)
        assert reg.get(p.id).state == PluginState.DISABLED
        reg.enable(p.id)
        assert reg.get(p.id).state == PluginState.ENABLED

    def test_set_error(self):
        reg = PluginRegistry()
        p = reg.install(_manifest(), PluginScope.USER)
        updated = reg.set_error(p.id, "boom")
        assert updated.state == PluginState.ERROR
        assert updated.error_message == "boom"

    def test_list_all_filters(self):
        reg = PluginRegistry()
        reg.install(_manifest("a", "1.0.0"), PluginScope.USER)
        reg.install(_manifest("b", "1.0.0"), PluginScope.PROJECT)
        assert len(reg.list_all(scope=PluginScope.USER)) == 1
        assert len(reg.list_all()) == 2

    def test_search(self):
        reg = PluginRegistry()
        reg.install(
            _manifest("code-formatter", "1.0.0", description="Formats code"),
            PluginScope.USER,
        )
        reg.install(_manifest("linter", "1.0.0"), PluginScope.USER)
        assert len(reg.search("format")) == 1
        assert len(reg.search("lint")) == 1

    def test_get_by_name(self):
        reg = PluginRegistry()
        reg.install(_manifest("x", "1.0.0"), PluginScope.USER)
        reg.install(_manifest("x", "2.0.0"), PluginScope.USER)
        assert len(reg.get_by_name("x")) == 2


# ── Loader ─────────────────────────────────────────────────────

class TestPluginLoader:
    def test_validate_manifest_ok(self):
        m = validate_manifest({"name": "hello", "version": "1.0.0"})
        assert m.name == "hello"

    def test_validate_manifest_missing_name(self):
        with pytest.raises(PluginLoadError):
            validate_manifest({"version": "1.0.0"})

    def test_validate_manifest_bad_chars(self):
        with pytest.raises(PluginLoadError, match="alphanumeric"):
            validate_manifest({"name": "bad name!", "version": "1.0.0"})

    def test_validate_manifest_name_too_long(self):
        with pytest.raises(PluginLoadError, match="128"):
            validate_manifest({"name": "a" * 200, "version": "1.0.0"})

    def test_load_from_path(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            manifest = {"name": "from-file", "version": "0.1.0"}
            p = Path(tmpdir) / "plugin.json"
            p.write_text(json.dumps(manifest))

            reg = PluginRegistry()
            loader = PluginLoader(reg)
            installed = loader.install_from_path(p, PluginScope.PROJECT)
            assert installed.manifest.name == "from-file"
            assert reg.count == 1

    def test_load_from_missing_path(self):
        reg = PluginRegistry()
        loader = PluginLoader(reg)
        with pytest.raises(PluginLoadError, match="not found"):
            loader.install_from_path(Path("/nonexistent"), PluginScope.USER)

    def test_install_from_manifest_dict(self):
        reg = PluginRegistry()
        loader = PluginLoader(reg)
        p = loader.install_from_manifest(
            {"name": "inline", "version": "1.0.0"}, source="api"
        )
        assert p.source == "api"

    def test_extract_components(self):
        manifest = _manifest(
            components=[
                PluginComponent(type=ComponentType.TOOL, name="t1"),
                PluginComponent(type=ComponentType.HOOK, name="h1"),
                PluginComponent(type=ComponentType.TOOL, name="t2"),
            ]
        )
        reg = PluginRegistry()
        loader = PluginLoader(reg)
        installed = reg.install(manifest, PluginScope.USER)
        groups = loader.extract_components(installed)
        assert len(groups[ComponentType.TOOL]) == 2
        assert len(groups[ComponentType.HOOK]) == 1


# ── Marketplace ────────────────────────────────────────────────

class TestMarketplace:
    @pytest.mark.asyncio
    async def test_search_empty(self):
        client = MarketplaceClient()
        results = await client.search("anything")
        assert results == []

    @pytest.mark.asyncio
    async def test_seed_and_search(self):
        client = MarketplaceClient()
        client.seed_catalog([
            CatalogEntry(name="formatter", version="1.0", tags=["code"]),
            CatalogEntry(name="linter", version="2.0", description="Lint code"),
        ])
        results = await client.search("lint")
        assert len(results) == 1
        assert results[0].name == "linter"

    @pytest.mark.asyncio
    async def test_get_entry(self):
        client = MarketplaceClient()
        client.seed_catalog([CatalogEntry(name="x", version="1.0")])
        assert (await client.get_entry("x")).version == "1.0"
        assert await client.get_entry("nope") is None

    @pytest.mark.asyncio
    async def test_refresh_catalog(self):
        client = MarketplaceClient()
        result = await client.refresh_catalog()
        assert isinstance(result, list)


# ── Reconciler ─────────────────────────────────────────────────

class TestReconciler:
    @pytest.mark.asyncio
    async def test_reconcile_no_updates(self):
        reg = PluginRegistry()
        mkt = MarketplaceClient()
        rec = PluginReconciler(reg, mkt)
        result = await rec.reconcile()
        assert result.checked == 0
        assert result.updated == 0

    @pytest.mark.asyncio
    async def test_reconcile_detects_update(self):
        reg = PluginRegistry()
        reg.install(_manifest("foo", "1.0.0"), PluginScope.USER)
        mkt = MarketplaceClient()
        mkt.seed_catalog([CatalogEntry(name="foo", version="2.0.0")])
        rec = PluginReconciler(reg, mkt)
        result = await rec.reconcile()
        assert result.checked == 1
        assert result.updated == 1

    @pytest.mark.asyncio
    async def test_reconcile_installs_managed(self):
        reg = PluginRegistry()
        mkt = MarketplaceClient()
        mkt.seed_catalog([
            CatalogEntry(name="required-plugin", version="1.0.0"),
        ])
        rec = PluginReconciler(reg, mkt)
        result = await rec.reconcile(managed_names=["required-plugin"])
        assert result.installed == 1
        assert reg.count == 1
        assert reg.list_all()[0].scope == PluginScope.MANAGED

    @pytest.mark.asyncio
    async def test_reconcile_managed_not_in_catalog(self):
        reg = PluginRegistry()
        mkt = MarketplaceClient()
        rec = PluginReconciler(reg, mkt)
        result = await rec.reconcile(managed_names=["ghost"])
        assert len(result.errors) == 1
        assert "not in catalog" in result.errors[0]
