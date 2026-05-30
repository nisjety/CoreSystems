"""Plugin management routes — /v1/plugins."""

from __future__ import annotations

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel

from app.domain import PluginManifest
from app.plugins import manager as plugin_mgr

router = APIRouter(prefix="/v1/plugins", tags=["plugins"])


class EnableRequest(BaseModel):
    enabled: bool


@router.get("")
async def list_plugins(org_id: str | None = None) -> dict:
    plugins = await plugin_mgr.list_plugins(org_id=org_id)
    return {"plugins": [p.model_dump(mode="json") for p in plugins]}


@router.get("/{plugin_id}")
async def get_plugin(plugin_id: str) -> dict:
    plugin = await plugin_mgr.get_plugin(plugin_id)
    if plugin is None:
        raise HTTPException(404, "Plugin not found")
    return plugin.model_dump(mode="json")


@router.post("", status_code=201)
async def install_plugin(body: PluginManifest, actor: str = "system") -> dict:
    saved = await plugin_mgr.install(body, actor=actor)
    return saved.model_dump(mode="json")


@router.patch("/{plugin_id}/enabled")
async def set_enabled(
    plugin_id: str, body: EnableRequest, actor: str = "system"
) -> dict:
    updated = await plugin_mgr.set_enabled(
        plugin_id, body.enabled, actor=actor
    )
    if updated is None:
        raise HTTPException(404, "Plugin not found")
    return updated.model_dump(mode="json")


@router.delete("/{plugin_id}", status_code=204)
async def uninstall_plugin(
    plugin_id: str, actor: str = "system"
) -> None:
    ok = await plugin_mgr.uninstall(plugin_id, actor=actor)
    if not ok:
        raise HTTPException(404, "Plugin not found")
