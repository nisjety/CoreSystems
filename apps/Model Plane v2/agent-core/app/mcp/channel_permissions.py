"""MCP channel permissions — per-org allowlist for MCP servers.

CC pattern: organisations can restrict which MCP servers are
available to their agents via an allowlist. If a channel is not
in the allowlist, connection is blocked.
"""

from __future__ import annotations

import logging
from typing import Any

logger = logging.getLogger(__name__)

# In-memory store — in production, backed by Postgres/Redis
_org_allowlists: dict[str, set[str]] = {}

# Special wildcard means all servers are allowed
ALLOW_ALL = "*"


def set_allowlist(org_id: str, servers: list[str]) -> None:
    """Set the MCP server allowlist for an org."""
    _org_allowlists[org_id] = set(servers)


def get_allowlist(org_id: str) -> set[str]:
    """Get the MCP server allowlist for an org (empty = nothing allowed)."""
    return _org_allowlists.get(org_id, set())


def is_server_allowed(org_id: str, server_name: str) -> bool:
    """Check if an MCP server is allowed for the given org.

    - If no allowlist is set for the org, all servers are allowed (permissive default).
    - If the allowlist contains ``*``, all servers are allowed.
    - Otherwise, the server must be in the allowlist.
    """
    allowlist = _org_allowlists.get(org_id)
    if allowlist is None:
        return True  # No restriction
    if ALLOW_ALL in allowlist:
        return True
    return server_name in allowlist


def remove_allowlist(org_id: str) -> None:
    """Remove allowlist for an org (reverts to permissive default)."""
    _org_allowlists.pop(org_id, None)


def clear_all() -> None:
    """Clear all allowlists (for testing)."""
    _org_allowlists.clear()
