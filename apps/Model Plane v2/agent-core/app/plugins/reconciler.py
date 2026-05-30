"""Background reconciler — check for updates and enforce managed plugins."""

from __future__ import annotations

import logging
from datetime import datetime, timezone

from app.plugins.domain import InstalledPlugin, PluginScope, PluginState
from app.plugins.marketplace import CatalogEntry, MarketplaceClient
from app.plugins.registry import PluginRegistry

logger = logging.getLogger(__name__)


class ReconcileResult:
    """Summary of a single reconcile run."""

    __slots__ = ("checked", "updated", "installed", "errors")

    def __init__(self) -> None:
        self.checked: int = 0
        self.updated: int = 0
        self.installed: int = 0
        self.errors: list[str] = []


class PluginReconciler:
    """Ensure managed plugins stay up to date.

    Runs periodically in the background to:
    1. Check if installed plugins have newer versions in the catalog.
    2. Auto-install any managed plugins defined by org policy.
    """

    def __init__(
        self,
        registry: PluginRegistry,
        marketplace: MarketplaceClient,
    ) -> None:
        self._registry = registry
        self._marketplace = marketplace

    async def reconcile(
        self,
        managed_names: list[str] | None = None,
    ) -> ReconcileResult:
        """Run a single reconcile pass.

        Args:
            managed_names: Plugin names required by org policy.
                          If a name isn't installed, it will be installed
                          as MANAGED scope.
        """
        result = ReconcileResult()
        await self._marketplace.refresh_catalog()

        # 1. Check installed plugins for updates
        for plugin in self._registry.list_all():
            result.checked += 1
            entry = await self._marketplace.get_entry(plugin.manifest.name)
            if entry is None:
                continue
            if entry.version != plugin.manifest.version:
                logger.info(
                    "Plugin %s has update: %s -> %s",
                    plugin.manifest.name,
                    plugin.manifest.version,
                    entry.version,
                )
                result.updated += 1

        # 2. Ensure managed plugins are installed
        if managed_names:
            for name in managed_names:
                existing = self._registry.get_by_name(name)
                if not existing:
                    entry = await self._marketplace.get_entry(name)
                    if entry is None:
                        result.errors.append(
                            f"Managed plugin not in catalog: {name}"
                        )
                        continue
                    # Auto-install
                    from app.plugins.domain import PluginManifest

                    manifest = PluginManifest(
                        name=entry.name,
                        version=entry.version,
                        description=entry.description,
                        author=entry.author,
                    )
                    try:
                        self._registry.install(
                            manifest, PluginScope.MANAGED, source="marketplace"
                        )
                        result.installed += 1
                    except Exception as exc:
                        result.errors.append(f"Failed to install {name}: {exc}")

        return result
