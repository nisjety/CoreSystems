"""Letta memory bridge — connects persistent Letta memory to the turn loop.

This bridges the existing letta/org_memory.py + trajectory_sync.py into
the reactive turn loop and event stream so that:

1. At turn-loop START: relevant Letta memories are fetched and injected
   into the system prompt as additional context.
2. At turn-loop END: the run summary is queued for trajectory sync
   (batched compression → Letta archival push).
3. Memory search is exposed as a tool ("memory_recall") for the agent
   to query Letta mid-turn.

This is MP v2's equivalent of CC's memdir system, but backed by Letta's
persistent archival memory instead of filesystem markdown files.
"""

from __future__ import annotations

import asyncio
import logging
from typing import Any

logger = logging.getLogger(__name__)

# Max memory snippets injected into system prompt
_MAX_PROMPT_SNIPPETS = 5

# Max characters per snippet to avoid blowing up context
_MAX_SNIPPET_CHARS = 2000

# Memory prompt wrapper
_MEMORY_HEADER = (
    "\n\n<institutional_memory>\n"
    "The following are relevant memories from previous runs for this organisation. "
    "Use them to avoid repeating past mistakes and build on successful patterns.\n\n"
)
_MEMORY_FOOTER = "\n</institutional_memory>\n"


class LettaMemoryBridge:
    """Bridges Letta persistent memory into the agent turn loop."""

    def __init__(self) -> None:
        self._org_memory: Any | None = None
        self._trajectory_sync: Any | None = None
        self._initialized = False

    async def initialize(self) -> bool:
        """Lazy-initialize Letta clients. Returns False if Letta is disabled."""
        if self._initialized:
            return self._org_memory is not None

        from app.config import settings

        if not settings.letta_enabled:
            self._initialized = True
            return False

        try:
            from app.letta.org_memory import OrgMemory
            from app.letta.trajectory_sync import TrajectorySync

            self._org_memory = OrgMemory()
            self._trajectory_sync = TrajectorySync(
                org_memory=self._org_memory,
                llm_client=None,  # Will be set when available
            )
            self._initialized = True
            logger.info("letta_memory_bridge_initialized")
            return True
        except Exception as exc:
            logger.warning("letta_memory_bridge_init_failed", extra={"error": str(exc)})
            self._initialized = True
            return False

    # ------------------------------------------------------------------
    # Pre-turn: fetch relevant memories for prompt injection
    # ------------------------------------------------------------------

    async def fetch_context(
        self,
        org_id: str,
        query: str,
        *,
        max_snippets: int = _MAX_PROMPT_SNIPPETS,
    ) -> list[str]:
        """Fetch relevant Letta memories for an org + query.

        Returns a list of text snippets suitable for prompt injection.
        Returns empty list if Letta is disabled or errors.
        """
        if not await self.initialize():
            return []

        if not org_id or not self._org_memory:
            return []

        try:
            results = await self._org_memory.search_context(org_id, query)
            if not results:
                return []

            snippets: list[str] = []
            for r in results[:max_snippets]:
                text = r if isinstance(r, str) else str(r.get("text", r))
                if len(text) > _MAX_SNIPPET_CHARS:
                    text = text[:_MAX_SNIPPET_CHARS] + "..."
                snippets.append(text)

            logger.debug(
                "letta_context_fetched",
                extra={"org_id": org_id, "snippets": len(snippets)},
            )
            return snippets

        except Exception as exc:
            logger.warning(
                "letta_context_fetch_failed",
                extra={"org_id": org_id, "error": str(exc)},
            )
            return []

    def format_memory_prompt(self, snippets: list[str]) -> str:
        """Format memory snippets into a system prompt section."""
        if not snippets:
            return ""

        body = "\n---\n".join(f"Memory {i + 1}: {s}" for i, s in enumerate(snippets))
        return f"{_MEMORY_HEADER}{body}{_MEMORY_FOOTER}"

    # ------------------------------------------------------------------
    # Post-turn: queue trajectory for async Letta sync
    # ------------------------------------------------------------------

    async def queue_trajectory_sync(
        self,
        org_id: str,
        task_pattern: str,
        goal: str,
        final_output: str | None,
        success: bool,
    ) -> None:
        """Queue a completed run for trajectory sync to Letta.

        This is non-blocking. The actual batch compression and Letta push
        happens asynchronously when enough runs accumulate (see TrajectorySync).
        """
        if not await self.initialize():
            return

        if not org_id or not self._trajectory_sync:
            return

        try:
            # Fire and forget — trajectory sync is best-effort
            asyncio.ensure_future(
                self._trajectory_sync.maybe_sync(org_id, task_pattern)
            )
        except Exception as exc:
            logger.debug(
                "letta_trajectory_queue_failed",
                extra={"org_id": org_id, "error": str(exc)},
            )

    # ------------------------------------------------------------------
    # Memory recall tool (exposed to agent during turn loop)
    # ------------------------------------------------------------------

    async def memory_recall(
        self,
        org_id: str,
        query: str,
    ) -> dict[str, Any]:
        """Tool handler for "memory_recall" — search Letta archival memory.

        Returns a dict with "memories" list for the agent to consume.
        """
        snippets = await self.fetch_context(org_id, query, max_snippets=10)
        return {
            "memories": snippets,
            "count": len(snippets),
            "source": "letta_archival",
        }

    async def memory_store(
        self,
        org_id: str,
        content: str,
    ) -> dict[str, Any]:
        """Tool handler for "memory_store" — store a new memory in Letta.

        The agent can explicitly store important learnings mid-run.
        """
        if not await self.initialize():
            return {"stored": False, "reason": "letta_disabled"}

        if not org_id or not self._org_memory:
            return {"stored": False, "reason": "no_org_id"}

        try:
            result = await self._org_memory.update_profile(org_id, content)
            return {"stored": bool(result), "org_id": org_id}
        except Exception as exc:
            logger.warning(
                "letta_memory_store_failed",
                extra={"org_id": org_id, "error": str(exc)},
            )
            return {"stored": False, "reason": str(exc)}


# Module-level singleton
_bridge: LettaMemoryBridge | None = None


def get_memory_bridge() -> LettaMemoryBridge:
    """Return the module-level LettaMemoryBridge singleton."""
    global _bridge
    if _bridge is None:
        _bridge = LettaMemoryBridge()
    return _bridge
