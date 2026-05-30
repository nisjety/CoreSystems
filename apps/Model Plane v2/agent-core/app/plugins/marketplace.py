"""Marketplace client — search and discover plugins from a remote catalog."""

from __future__ import annotations

import logging
from typing import Any

from pydantic import BaseModel, Field

logger = logging.getLogger(__name__)


class CatalogEntry(BaseModel):
    """One plugin in the marketplace catalog."""

    name: str
    version: str
    description: str = ""
    author: str = ""
    downloads: int = 0
    tags: list[str] = Field(default_factory=list)
    manifest_url: str = ""


class MarketplaceClient:
    """Fetch plugin catalog from a remote registry.

    Currently backed by an in-memory stub. Production implementation
    would call a REST API or Git-based registry.
    """

    def __init__(self, catalog_url: str = "") -> None:
        self._catalog_url = catalog_url
        self._cache: list[CatalogEntry] = []

    async def refresh_catalog(self) -> list[CatalogEntry]:
        """Refresh the local cache from the remote catalog.

        Stub: returns existing cache unchanged.
        """
        logger.info("Refreshing plugin catalog from %s", self._catalog_url)
        return list(self._cache)

    async def search(self, query: str) -> list[CatalogEntry]:
        """Search the cached catalog by name/description/tags."""
        q = query.lower()
        return [
            e for e in self._cache
            if q in e.name.lower()
            or q in e.description.lower()
            or any(q in t.lower() for t in e.tags)
        ]

    async def get_entry(self, name: str) -> CatalogEntry | None:
        """Look up a specific plugin by exact name."""
        for entry in self._cache:
            if entry.name == name:
                return entry
        return None

    def seed_catalog(self, entries: list[CatalogEntry]) -> None:
        """Seed the cache for testing."""
        self._cache = list(entries)

    @property
    def catalog_size(self) -> int:
        return len(self._cache)
