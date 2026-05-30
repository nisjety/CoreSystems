"""TrajectorySync — pushes successful run summaries into Letta archival memory.

Strategy:
- Every 10 successful runs for the same (org_id, task_pattern) pair,
  fetch those runs' goals + outcomes, compress to 2–3 sentences via LLM,
  and push the summary to Letta via OrgMemory.update_profile().
- Marks rows as letta_stored=TRUE to avoid re-syncing.
- All operations are non-blocking: called inside asyncio.ensure_future().

Compression prompt is deliberately minimal to avoid hallucination.
"""

from __future__ import annotations

import logging
from typing import Any

from app.database import get_pool
from app.letta.org_memory import OrgMemory

logger = logging.getLogger(__name__)

# How many successful runs to batch before compressing
_BATCH_SIZE = 10

# Compression prompt template
_COMPRESS_PROMPT = (
    "You are a memory distillation engine. "
    "Given the following list of successful task summaries, "
    "write 2-3 sentences that capture the common pattern and what made them successful. "
    "Be concrete and action-oriented. Do not add new information.\n\n"
    "Tasks:\n{task_list}\n\n"
    "Summary (2-3 sentences):"
)


class TrajectorySync:
    """Checks for pending trajectories and syncs them to Letta."""

    def __init__(self, org_memory: OrgMemory, llm_client: Any | None = None) -> None:
        self._memory = org_memory
        self._llm = llm_client

    async def maybe_sync(self, org_id: str, task_pattern: str) -> None:
        """Check if there's a batch ready to sync; if so, compress + push.

        Safe to call after every successful run — exits quickly if no
        batch threshold reached.
        """
        from app.config import settings

        if not settings.letta_enabled:
            return

        try:
            pool = await get_pool()
        except Exception:
            return

        # Count unsync'd successes for this (org, pattern)
        try:
            async with pool.acquire() as conn:
                rows = await conn.fetch(
                    """
                    SELECT id, task_goal
                    FROM agent_trajectories
                    WHERE org_id = $1
                      AND task_pattern = $2
                      AND outcome = 'success'
                      AND letta_stored = FALSE
                    ORDER BY created_at
                    LIMIT $3
                    """,
                    org_id,
                    task_pattern,
                    _BATCH_SIZE,
                )

            if len(rows) < _BATCH_SIZE:
                return  # Not enough to compress yet

            trace_ids = [str(r["id"]) for r in rows]
            goals = [r["task_goal"] for r in rows]

            # Compress
            summary = await self._compress(goals)
            if not summary:
                return

            # Push to Letta
            pushed = await self._memory.update_profile(org_id, summary)
            if not pushed:
                return

            # Mark as stored
            async with pool.acquire() as conn:
                await conn.execute(
                    """
                    UPDATE agent_trajectories
                    SET letta_stored = TRUE
                    WHERE id = ANY($1::uuid[])
                    """,
                    trace_ids,
                )

            logger.info(
                "letta_sync_complete",
                extra={
                    "org_id": org_id,
                    "task_pattern": task_pattern,
                    "count": len(trace_ids),
                },
            )

        except Exception as exc:
            logger.warning(
                "letta_sync_failed",
                extra={"org_id": org_id, "task_pattern": task_pattern, "error": str(exc)},
            )

    async def _compress(self, goals: list[str]) -> str | None:
        """Compress a list of goal descriptions into a 2-3 sentence summary."""
        if self._llm is None:
            # No LLM available — produce a simple concatenation
            return "; ".join(goals[:5])

        task_list = "\n".join(f"- {g[:200]}" for g in goals)
        prompt = _COMPRESS_PROMPT.format(task_list=task_list)

        try:
            messages = [
                {"role": "system", "content": "You distill agent run patterns into concise memory entries."},
                {"role": "user", "content": prompt},
            ]
            raw = await self._llm.planner_complete(messages)
            if isinstance(raw, list) and raw:
                from app.domain import AgentAction
                return str(raw[0]) if not hasattr(raw[0], "output") else str(raw[0].output)
            return str(raw) if raw else None
        except Exception as exc:
            logger.warning("letta_compress_failed", extra={"error": str(exc)})
            return None
