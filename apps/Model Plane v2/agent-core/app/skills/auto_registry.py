"""AutoSkillRegistry — org-level dynamic skills synthesised from trajectories.

Complements the static file-based ``app.skills.registry.match_skills()`` by
adding skills that were learned at runtime from trajectory clusters.

Workflow:
1. ``get_applicable()`` — merge static + dynamic skills for a run
2. ``record_usage()`` — tick usage/success counters on org_skills after a run
3. ``store()`` — upsert a newly synthesised skill into org_skills
"""

from __future__ import annotations

import json
import logging
from dataclasses import dataclass, field

from app.database import get_pool

logger = logging.getLogger(__name__)


@dataclass
class DynamicSkillMatch:
    """A synthesised skill returned from the database."""

    id: str
    name: str
    task_pattern: str
    skill_md: str
    success_rate: float


class AutoSkillRegistry:
    """Reads and writes org_skills rows."""

    # Minimum success_rate to be served to the LLM
    _MIN_SUCCESS_RATE = 0.3

    async def get_applicable(
        self,
        org_id: str,
        task_pattern: str,
        limit: int = 3,
    ) -> list[DynamicSkillMatch]:
        """Return active dynamic skills for this org + pattern.

        Only returns skills with success_rate >= 0.3 and not deprecated.
        """
        try:
            pool = await get_pool()
            async with pool.acquire() as conn:
                rows = await conn.fetch(
                    """
                    SELECT id, name, task_pattern, skill_md, success_rate
                    FROM org_skills
                    WHERE org_id = $1
                      AND task_pattern = $2
                      AND deprecated_at IS NULL
                      AND success_rate >= $3
                    ORDER BY success_rate DESC, usage_count DESC
                    LIMIT $4
                    """,
                    org_id,
                    task_pattern,
                    self._MIN_SUCCESS_RATE,
                    limit,
                )
            return [
                DynamicSkillMatch(
                    id=str(r["id"]),
                    name=r["name"],
                    task_pattern=r["task_pattern"],
                    skill_md=r["skill_md"],
                    success_rate=float(r["success_rate"]),
                )
                for r in rows
            ]
        except Exception as exc:
            logger.warning("auto_skill_get_failed", extra={"org_id": org_id, "error": str(exc)})
            return []

    async def record_usage(
        self,
        org_id: str,
        task_pattern: str,
        succeeded: bool,
    ) -> None:
        """Increment usage + success counters; recompute success_rate.

        Auto-deprecates skills with success_rate < 0.3 after 20+ uses.
        """
        try:
            pool = await get_pool()
            async with pool.acquire() as conn:
                await conn.execute(
                    """
                    UPDATE org_skills
                    SET
                        usage_count  = usage_count + 1,
                        success_count = success_count + $3,
                        success_rate  = (success_count + $3)::numeric / (usage_count + 1),
                        updated_at    = now()
                    WHERE org_id = $1
                      AND task_pattern = $2
                      AND deprecated_at IS NULL
                    """,
                    org_id,
                    task_pattern,
                    1 if succeeded else 0,
                )
                # Deprecate underperforming skills
                await conn.execute(
                    """
                    UPDATE org_skills
                    SET deprecated_at = now(), updated_at = now()
                    WHERE org_id = $1
                      AND task_pattern = $2
                      AND deprecated_at IS NULL
                      AND usage_count >= 20
                      AND success_rate < 0.3
                    """,
                    org_id,
                    task_pattern,
                )
        except Exception as exc:
            logger.warning(
                "auto_skill_record_usage_failed",
                extra={"org_id": org_id, "task_pattern": task_pattern, "error": str(exc)},
            )

    async def store(
        self,
        org_id: str,
        name: str,
        task_pattern: str,
        skill_md: str,
        source_trace_ids: list[str],
    ) -> str | None:
        """Upsert a synthesised skill; return its UUID or None on failure.

        On conflict with an existing active skill for the same (org, pattern),
        bumps the version and replaces the content.
        """
        try:
            pool = await get_pool()
            async with pool.acquire() as conn:
                row = await conn.fetchrow(
                    """
                    INSERT INTO org_skills (
                        org_id, name, task_pattern, skill_md, source_trace_ids
                    ) VALUES ($1, $2, $3, $4, $5::uuid[])
                    ON CONFLICT ON CONSTRAINT idx_org_skills_org_pattern
                    DO UPDATE SET
                        skill_md         = EXCLUDED.skill_md,
                        source_trace_ids = EXCLUDED.source_trace_ids,
                        version          = org_skills.version + 1,
                        updated_at       = now()
                    RETURNING id
                    """,
                    org_id,
                    name,
                    task_pattern,
                    skill_md,
                    source_trace_ids,
                )
            if row:
                skill_id = str(row["id"])
                logger.info(
                    "dynamic_skill_stored",
                    extra={"org_id": org_id, "task_pattern": task_pattern, "id": skill_id},
                )
                return skill_id
        except Exception as exc:
            logger.warning(
                "dynamic_skill_store_failed",
                extra={"org_id": org_id, "error": str(exc)},
            )
        return None
