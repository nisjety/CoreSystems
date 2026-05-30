"""Plugin lifecycle — install / enable / disable / uninstall."""

from __future__ import annotations

import logging
from datetime import datetime, timezone

from app import nats_client, repository
from app.domain import PluginManifest

logger = logging.getLogger(__name__)


async def install(manifest: PluginManifest, *, actor: str) -> PluginManifest:
    """Register a plugin in the catalog and publish event."""

    manifest = manifest.model_copy(
        update={
            "enabled": True,
            "installed_at": datetime.now(timezone.utc),
        }
    )

    saved = await repository.upsert_plugin(manifest)

    await nats_client.publish_plugin_installed(saved)
    await repository.write_audit(
        "plugin.installed",
        entity_id=saved.plugin_id,
        actor_id=actor,
        payload={"version": saved.version},
    )

    logger.info("plugin installed: %s v%s", saved.plugin_id, saved.version)
    return saved


async def set_enabled(
    plugin_id: str, enabled: bool, *, actor: str
) -> PluginManifest | None:
    """Enable or disable a plugin."""

    plugin = await repository.get_plugin(plugin_id)
    if plugin is None:
        return None

    ok = await repository.set_plugin_enabled(plugin_id, enabled)
    if not ok:
        return None

    updated = plugin.model_copy(update={"enabled": enabled})
    await nats_client.publish_plugin_enabled(updated)
    await repository.write_audit(
        "plugin.enabled" if enabled else "plugin.disabled",
        entity_id=plugin_id,
        actor_id=actor,
    )

    logger.info("plugin %s: %s", "enabled" if enabled else "disabled", plugin_id)
    return updated


async def uninstall(plugin_id: str, *, actor: str) -> bool:
    """Remove a plugin from the catalog."""

    plugin = await repository.get_plugin(plugin_id)
    if plugin is None:
        return False

    await repository.delete_plugin(plugin_id)
    await repository.write_audit(
        "plugin.uninstalled",
        entity_id=plugin_id,
        actor_id=actor,
    )

    logger.info("plugin uninstalled: %s", plugin_id)
    return True


async def list_plugins(
    *, org_id: str | None = None
) -> list[PluginManifest]:
    return await repository.list_plugins(org_id=org_id)


async def get_plugin(plugin_id: str) -> PluginManifest | None:
    return await repository.get_plugin(plugin_id)
