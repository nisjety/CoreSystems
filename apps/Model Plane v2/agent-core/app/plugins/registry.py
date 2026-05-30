"""Plugin registry — install, uninstall, enable, disable, query."""

from __future__ import annotations

import logging
from typing import Any

from app.plugins.domain import (
    InstalledPlugin,
    PluginManifest,
    PluginScope,
    PluginState,
)

logger = logging.getLogger(__name__)


class PluginConflictError(Exception):
    """Raised when a plugin with the same name+version is already installed."""


class PluginNotFoundError(Exception):
    """Raised when the requested plugin is not installed."""


class PluginRegistry:
    """In-memory registry tracking installed plugins.

    Thread-safe for read operations; mutations create new state.
    """

    def __init__(self) -> None:
        self._plugins: dict[str, InstalledPlugin] = {}

    # ── Queries ─────────────────────────────────────────────

    def get(self, plugin_id: str) -> InstalledPlugin | None:
        return self._plugins.get(plugin_id)

    def get_by_name(self, name: str) -> list[InstalledPlugin]:
        return [
            p for p in self._plugins.values()
            if p.manifest.name == name
        ]

    def list_all(
        self,
        *,
        scope: PluginScope | None = None,
        state: PluginState | None = None,
    ) -> list[InstalledPlugin]:
        results = list(self._plugins.values())
        if scope is not None:
            results = [p for p in results if p.scope == scope]
        if state is not None:
            results = [p for p in results if p.state == state]
        return results

    def search(self, query: str) -> list[InstalledPlugin]:
        q = query.lower()
        return [
            p for p in self._plugins.values()
            if q in p.manifest.name.lower()
            or q in p.manifest.description.lower()
        ]

    @property
    def count(self) -> int:
        return len(self._plugins)

    # ── Mutations ───────────────────────────────────────────

    def install(
        self,
        manifest: PluginManifest,
        scope: PluginScope,
        source: str = "",
    ) -> InstalledPlugin:
        pid = manifest.id
        if pid in self._plugins:
            raise PluginConflictError(f"Plugin already installed: {pid}")

        plugin = InstalledPlugin(
            manifest=manifest,
            scope=scope,
            source=source,
        )
        self._plugins = {**self._plugins, pid: plugin}
        logger.info("Installed plugin %s (scope=%s)", pid, scope.value)
        return plugin

    def uninstall(self, plugin_id: str) -> InstalledPlugin:
        plugin = self._plugins.get(plugin_id)
        if plugin is None:
            raise PluginNotFoundError(plugin_id)
        if plugin.scope == PluginScope.MANAGED:
            raise PluginConflictError(
                f"Cannot uninstall managed plugin: {plugin_id}"
            )
        new_plugins = {
            k: v for k, v in self._plugins.items() if k != plugin_id
        }
        self._plugins = new_plugins
        logger.info("Uninstalled plugin %s", plugin_id)
        return plugin

    def enable(self, plugin_id: str) -> InstalledPlugin:
        return self._set_state(plugin_id, PluginState.ENABLED)

    def disable(self, plugin_id: str) -> InstalledPlugin:
        return self._set_state(plugin_id, PluginState.DISABLED)

    def set_error(self, plugin_id: str, message: str) -> InstalledPlugin:
        plugin = self._plugins.get(plugin_id)
        if plugin is None:
            raise PluginNotFoundError(plugin_id)
        updated = plugin.model_copy(
            update={"state": PluginState.ERROR, "error_message": message}
        )
        self._plugins = {**self._plugins, plugin_id: updated}
        return updated

    # ── Internals ───────────────────────────────────────────

    def _set_state(
        self, plugin_id: str, state: PluginState
    ) -> InstalledPlugin:
        plugin = self._plugins.get(plugin_id)
        if plugin is None:
            raise PluginNotFoundError(plugin_id)
        updated = plugin.model_copy(update={"state": state})
        self._plugins = {**self._plugins, plugin_id: updated}
        return updated
