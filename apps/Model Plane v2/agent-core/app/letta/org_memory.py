"""OrgMemory — manages one Letta agent per organisation.

Each org gets a single persistent Letta agent that accumulates archival
memory across all of that org's runs.  Agent IDs are cached in Redis so
we avoid redundant API calls on every run.

Design:
- ``get_or_create_agent(org_id)``: returns Letta agent_id, creating it once.
- ``search_context(org_id, query)``: returns up to 10 archival snippets.
- ``update_profile(org_id, summary)``: appends a new archival memory entry
  (called every N successful runs by TrajectorySync).

Falls back gracefully when Letta is disabled or unreachable.
"""

from __future__ import annotations

import json
import logging
from typing import Any

logger = logging.getLogger(__name__)

# Redis key: letta:org_agent_id:{org_id}   value: Letta agent_id string
_REDIS_KEY_PREFIX = "letta:org_agent_id:"
_REDIS_TTL = 86400 * 30  # 30 days — agent IDs are stable

# Letta memory block defaults
_PERSONA_TEXT = (
    "I am Velion's institutional memory for this organisation. "
    "I accumulate patterns from successful runs to help future agents work better."
)
_HUMAN_LIMIT = 2000
_PERSONA_LIMIT = 1000


def _make_client() -> Any | None:
    """Return a letta_client.Letta instance or None if disabled/unavailable."""
    from app.config import settings

    if not settings.letta_enabled:
        return None
    try:
        from letta_client import Letta  # type: ignore[import-untyped]

        return Letta(base_url=settings.letta_api_url)
    except Exception as exc:
        logger.warning("letta_client_init_failed", extra={"error": str(exc)})
        return None


class OrgMemory:
    """Wraps the Letta API for per-org persistent memory."""

    def __init__(self) -> None:
        self._client: Any | None = None
        self._client_checked = False

    def _get_client(self) -> Any | None:
        if not self._client_checked:
            self._client = _make_client()
            self._client_checked = True
        return self._client

    # ------------------------------------------------------------------
    # Agent lifecycle
    # ------------------------------------------------------------------

    async def get_or_create_agent(self, org_id: str) -> str | None:
        """Return the Letta agent_id for ``org_id``, creating if absent.

        Returns None when Letta is disabled or an error occurs.
        """
        client = self._get_client()
        if client is None:
            return None

        # Check Redis cache first
        try:
            from app.redis_client import get_redis

            redis = await get_redis()
            cached = await redis.get(f"{_REDIS_KEY_PREFIX}{org_id}")
            if cached:
                return cached.decode() if isinstance(cached, bytes) else cached
        except Exception as exc:
            logger.debug("letta_redis_cache_miss", extra={"org_id": org_id, "error": str(exc)})

        # Create a new agent
        agent_id = await self._create_agent(client, org_id)
        if agent_id:
            try:
                from app.redis_client import get_redis

                redis = await get_redis()
                await redis.set(f"{_REDIS_KEY_PREFIX}{org_id}", agent_id, ex=_REDIS_TTL)
            except Exception:
                pass  # Cache failure is non-fatal

        return agent_id

    async def _create_agent(self, client: Any, org_id: str) -> str | None:
        """Create one Letta agent for the org and return its ID."""
        try:
            from letta_client import CreateBlock  # type: ignore[import-untyped]

            agent = client.agents.create(
                name=f"velion-org-{org_id}",
                model="gpt-4o-mini",
                embedding="text-embedding-3-small",
                memory_blocks=[
                    CreateBlock(
                        label="human",
                        value=f"This agent serves organisation {org_id}. "
                              "It remembers patterns from past successful runs.",
                        limit=_HUMAN_LIMIT,
                    ),
                    CreateBlock(
                        label="persona",
                        value=_PERSONA_TEXT,
                        limit=_PERSONA_LIMIT,
                    ),
                ],
            )
            logger.info(
                "letta_org_agent_created",
                extra={"org_id": org_id, "agent_id": agent.id},
            )
            return agent.id
        except Exception as exc:
            logger.warning(
                "letta_agent_create_failed",
                extra={"org_id": org_id, "error": str(exc)},
            )
            return None

    # ------------------------------------------------------------------
    # Memory operations
    # ------------------------------------------------------------------

    async def search_context(self, org_id: str, query: str) -> list[str]:
        """Return up to 10 relevant archival memory snippets for a query.

        Returns empty list when Letta is disabled or the agent is missing.
        """
        client = self._get_client()
        if client is None:
            return []

        agent_id = await self.get_or_create_agent(org_id)
        if not agent_id:
            return []

        try:
            passages = client.agents.archival_memory.list(agent_id=agent_id)
            query_lower = query.lower()
            results = [
                p.text
                for p in passages
                if p.text and query_lower in p.text.lower()
            ]
            return results[:10]
        except Exception as exc:
            logger.warning(
                "letta_search_failed",
                extra={"org_id": org_id, "error": str(exc)},
            )
            return []

    async def update_profile(self, org_id: str, summary: str) -> bool:
        """Append ``summary`` to archival memory for the org's agent.

        ``summary`` should be a 2–3 sentence LLM-compressed trace.
        Returns True on success.
        """
        client = self._get_client()
        if client is None:
            return False

        agent_id = await self.get_or_create_agent(org_id)
        if not agent_id:
            return False

        try:
            client.agents.archival_memory.create(
                agent_id=agent_id,
                text=summary,
            )
            logger.debug(
                "letta_profile_updated",
                extra={"org_id": org_id, "agent_id": agent_id},
            )
            return True
        except Exception as exc:
            logger.warning(
                "letta_update_profile_failed",
                extra={"org_id": org_id, "error": str(exc)},
            )
            return False
