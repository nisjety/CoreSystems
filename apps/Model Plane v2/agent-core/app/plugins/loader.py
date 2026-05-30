"""Plugin loader — fetch, validate, and extract plugin components."""

from __future__ import annotations

import json
import logging
from pathlib import Path
from typing import Any

from app.plugins.domain import (
    ComponentType,
    InstalledPlugin,
    PluginComponent,
    PluginManifest,
    PluginScope,
)
from app.plugins.registry import PluginRegistry

logger = logging.getLogger(__name__)


class PluginLoadError(Exception):
    """Raised when a plugin cannot be loaded."""


REQUIRED_MANIFEST_FIELDS = {"name", "version"}


def validate_manifest(data: dict[str, Any]) -> PluginManifest:
    """Parse and validate a raw manifest dict.

    Raises PluginLoadError on invalid input.
    """
    missing = REQUIRED_MANIFEST_FIELDS - set(data.keys())
    if missing:
        raise PluginLoadError(f"Manifest missing required fields: {missing}")

    name = data["name"]
    if not name or not isinstance(name, str):
        raise PluginLoadError("Plugin name must be a non-empty string")
    if len(name) > 128:
        raise PluginLoadError("Plugin name exceeds 128 characters")
    # Sanitise name: alphanumeric, hyphens, underscores only
    if not all(c.isalnum() or c in "-_" for c in name):
        raise PluginLoadError(
            "Plugin name may only contain alphanumeric, -, _"
        )

    return PluginManifest(**data)


def load_manifest_from_path(manifest_path: Path) -> PluginManifest:
    """Load and validate a plugin manifest from a local JSON file."""
    if not manifest_path.is_file():
        raise PluginLoadError(f"Manifest not found: {manifest_path}")

    try:
        raw = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError) as exc:
        raise PluginLoadError(f"Failed to read manifest: {exc}") from exc

    return validate_manifest(raw)


class PluginLoader:
    """High-level loader that fetches, validates, and registers plugins."""

    def __init__(self, registry: PluginRegistry) -> None:
        self._registry = registry

    def install_from_path(
        self,
        manifest_path: Path,
        scope: PluginScope = PluginScope.USER,
    ) -> InstalledPlugin:
        """Load a plugin from a local manifest file and install it."""
        manifest = load_manifest_from_path(manifest_path)
        return self._registry.install(
            manifest, scope, source=str(manifest_path.parent)
        )

    def install_from_manifest(
        self,
        data: dict[str, Any],
        scope: PluginScope = PluginScope.USER,
        source: str = "",
    ) -> InstalledPlugin:
        """Install from an already-parsed manifest dict."""
        manifest = validate_manifest(data)
        return self._registry.install(manifest, scope, source=source)

    def extract_components(
        self, plugin: InstalledPlugin
    ) -> dict[ComponentType, list[PluginComponent]]:
        """Group a plugin's components by type for registration."""
        groups: dict[ComponentType, list[PluginComponent]] = {}
        for comp in plugin.manifest.components:
            groups.setdefault(comp.type, []).append(comp)
        return groups
