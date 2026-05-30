"""OrgInsights — aggregates trajectory data into org-level intelligence.

Publishes a rolling summary event to NATS: ``aqencia.reasoning.org.insights``

Triggered:
- By the cron scheduler at a configurable interval (default: every 30 min)
- Optionally inline after each trajectory record (disabled by default)

Insight payload:
{
  "org_id": str,
  "window_hours": int,
  "total_runs": int,
  "success_rate": float,
  "top_patterns": [{"pattern": str, "count": int, "success_rate": float}, ...],
  "avg_cost_usd": float,
  "avg_duration_sec": float
}
"""

from __future__ import annotations

import asyncio
import json
import logging
from datetime import datetime, timezone
from typing import Any

from app.database import get_pool

logger = logging.getLogger(__name__)

# Aggregation window in hours
_DEFAULT_WINDOW_HOURS = 24


async def compute_org_insights(
    org_id: str,
    window_hours: int = _DEFAULT_WINDOW_HOURS,
) -> dict[str, Any]:
    """Query ``agent_trajectories`` and return an insights dict.

    Returns an empty dict on failure so callers can safely ignore it.
    """
    try:
        pool = await get_pool()
        async with pool.acquire() as conn:
            summary = await conn.fetchrow(
                """
                SELECT
                    COUNT(*)::INT                                    AS total_runs,
                    SUM(CASE WHEN outcome='success' THEN 1 ELSE 0 END)::INT AS successes,
                    AVG(cost_usd)::FLOAT                             AS avg_cost_usd,
                    AVG(duration_sec)::FLOAT                         AS avg_duration_sec
                FROM agent_trajectories
                WHERE org_id = $1
                  AND created_at > now() - ($2 || ' hours')::INTERVAL
                """,
                org_id,
                str(window_hours),
            )

            pattern_rows = await conn.fetch(
                """
                SELECT
                    task_pattern,
                    COUNT(*)::INT AS cnt,
                    SUM(CASE WHEN outcome='success' THEN 1 ELSE 0 END)::FLOAT / COUNT(*) AS sr
                FROM agent_trajectories
                WHERE org_id = $1
                  AND created_at > now() - ($2 || ' hours')::INTERVAL
                GROUP BY task_pattern
                ORDER BY cnt DESC
                LIMIT 10
                """,
                org_id,
                str(window_hours),
            )

        total = summary["total_runs"] or 0
        successes = summary["successes"] or 0
        success_rate = round(successes / total, 4) if total else 0.0

        return {
            "org_id": org_id,
            "window_hours": window_hours,
            "total_runs": total,
            "success_rate": success_rate,
            "top_patterns": [
                {
                    "pattern": r["task_pattern"],
                    "count": r["cnt"],
                    "success_rate": round(float(r["sr"] or 0), 4),
                }
                for r in pattern_rows
            ],
            "avg_cost_usd": round(float(summary["avg_cost_usd"] or 0), 8),
            "avg_duration_sec": round(float(summary["avg_duration_sec"] or 0), 3),
        }
    except Exception as exc:
        logger.warning(
            "org_insights_compute_failed", extra={"org_id": org_id, "error": str(exc)}
        )
        return {}


class OrgInsightsPublisher:
    """Computes org insights and publishes them to NATS."""

    def __init__(self, nats_mgr: Any) -> None:
        self._nats = nats_mgr

    async def publish_for_org(
        self,
        org_id: str,
        window_hours: int = _DEFAULT_WINDOW_HOURS,
    ) -> None:
        """Compute and publish one insights event for ``org_id``."""
        insights = await compute_org_insights(org_id, window_hours)
        if not insights:
            return

        try:
            await self._nats.publish_jetstream(
                "aqencia.reasoning.org.insights",
                {
                    **insights,
                    "published_at": datetime.now(timezone.utc).isoformat(),
                    "service": "agent-core-v2",
                },
                local=True,
            )
            logger.debug(
                "org_insights_published",
                extra={"org_id": org_id, "total_runs": insights.get("total_runs")},
            )
        except Exception as exc:
            logger.warning(
                "org_insights_publish_failed",
                extra={"org_id": org_id, "error": str(exc)},
            )

    async def publish_all_active_orgs(
        self,
        window_hours: int = _DEFAULT_WINDOW_HOURS,
    ) -> None:
        """Publish insights for every org that has runs in the given window.

        Intended to be called by the cron scheduler.
        """
        try:
            pool = await get_pool()
            async with pool.acquire() as conn:
                rows = await conn.fetch(
                    """
                    SELECT DISTINCT org_id
                    FROM agent_trajectories
                    WHERE created_at > now() - ($1 || ' hours')::INTERVAL
                    """,
                    str(window_hours),
                )
        except Exception as exc:
            logger.warning("org_insights_list_orgs_failed", extra={"error": str(exc)})
            return

        org_ids = [str(r["org_id"]) for r in rows]
        tasks = [self.publish_for_org(oid, window_hours) for oid in org_ids]
        results = await asyncio.gather(*tasks, return_exceptions=True)
        errors = [r for r in results if isinstance(r, Exception)]
        if errors:
            logger.warning("org_insights_partial_failures", extra={"count": len(errors)})
