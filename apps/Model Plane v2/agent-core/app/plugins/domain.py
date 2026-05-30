"""Plugin domain models — manifest, scope, component descriptors."""

from __future__ import annotations

import enum
from datetime import datetime, timezone
from typing import Any

from pydantic import BaseModel, Field


class PluginScope(str, enum.Enum):
    """Where a plugin is installed."""

    USER = "user"
    PROJECT = "project"
    MANAGED = "managed"  # Read-only, pushed by org policy


class PluginState(str, enum.Enum):
    """Runtime state of an installed plugin."""

    ENABLED = "enabled"
    DISABLED = "disabled"
    ERROR = "error"
    UPDATING = "updating"


class ComponentType(str, enum.Enum):
    """Types of components a plugin may contribute."""

    TOOL = "tool"
    SKILL = "skill"
    HOOK = "hook"
    COMMAND = "command"
    MCP_SERVER = "mcp_server"


class PluginComponent(BaseModel):
    """One component contributed by a plugin."""

    type: ComponentType
    name: str
    config: dict[str, Any] = Field(default_factory=dict)


class PluginManifest(BaseModel):
    """Declarative plugin metadata — read from manifest file (e.g. plugin.json)."""

    name: str
    version: str
    description: str = ""
    author: str = ""
    license: str = ""
    homepage: str = ""
    min_agent_version: str = ""
    components: list[PluginComponent] = Field(default_factory=list)
    dependencies: list[str] = Field(default_factory=list)

    @property
    def id(self) -> str:
        return f"{self.name}@{self.version}"


class InstalledPlugin(BaseModel):
    """Runtime record for an installed plugin."""

    manifest: PluginManifest
    scope: PluginScope
    state: PluginState = PluginState.ENABLED
    installed_at: datetime = Field(
        default_factory=lambda: datetime.now(timezone.utc)
    )
    source: str = ""  # git url or local path
    error_message: str | None = None

    @property
    def id(self) -> str:
        return self.manifest.id
