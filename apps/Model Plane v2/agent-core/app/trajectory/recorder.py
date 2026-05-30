"""TrajectoryRecorder — persists a run trace to ``agent_trajectories``.

Called from ``agent_service.py`` after each run completes (success, partial, or failure).
Design: fire-and-forget from the hot path, never blocks the caller.
"""

from __future__ import annotations

import asyncio
import logging
import time
from typing import Any

from app.database import get_pool
from app.domain import AgentAction, ActionStatus, RunRecord, RunStatus
from app.trajectory.patterns import normalize_goal

logger = logging.getLogger(__name__)


def _derive_outcome(run: RunRecord) -> str:
    """Map ``RunStatus`` to the three-value outcome enum."""
    if run.status == RunStatus.COMPLETED:
        if run.final_output:
            return "success"
        return "partial"
    if run.status in {RunStatus.FAILED, RunStatus.CANCELLED}:
        return "failure"
    # Any other terminal state is treated as partial
    return "partial"


def _extract_skills_used(actions: list[AgentAction]) -> list[str]:
    """Collect tool names from completed tool-call actions."""
    names: list[str] = []
    for action in actions:
        if action.status == ActionStatus.COMPLETED and action.name:
            if action.name not in names:
                names.append(action.name)
    return names


class TrajectoryRecorder:
    """Records run traces to ``agent_trajectories`` in Postgres.

    Instantiated once at startup and shared with ``AgentService``.
    ``record()`` is deliberately async but intended to be wrapped in
    ``asyncio.ensure_future()`` so the caller returns immediately.
    """

    async def record(
        self,
        run: RunRecord,
        *,
        tokens_in: int = 0,
        tokens_out: int = 0,
        started_at: float | None = None,
    ) -> None:
        """Insert one row into ``agent_trajectories``.

        Args:
            run: The completed (or failed) ``RunRecord``.
            tokens_in: LLM planning tokens consumed (from llm_client.last_response_data).
            tokens_out: LLM output tokens from planning phase.
            started_at: ``time.monotonic()`` captured just before run execution.
        """
        if not run.org_id:
            return  # Skip anonymous/system runs — no org to improve

        try:
            pool = await get_pool()
        except Exception as exc:
            logger.warning("trajectory_recorder_no_pool", extra={"error": str(exc)})
            return

        outcome = _derive_outcome(run)
        task_pattern = normalize_goal(run.goal)
        skills_used = _extract_skills_used(run.actions or [])
        duration_sec: float | None = None
        if started_at is not None:
            duration_sec = round(time.monotonic() - started_at, 3)

        # Serialise actions as dicts (already Pydantic models)
        planned_actions_json: list[dict[str, Any]] = []
        executed_actions_json: list[dict[str, Any]] = []
        for action in run.actions or []:
            d = action.model_dump(mode="json", exclude={"output"})
            planned_actions_json.append(d)
            if action.status in {ActionStatus.COMPLETED, ActionStatus.FAILED}:
                executed_actions_json.append(d)

        import json

        try:
            async with pool.acquire() as conn:
                await conn.execute(
                    """
                    INSERT INTO agent_trajectories (
                        run_id, org_id, session_id,
                        task_goal, task_pattern,
                        model, tokens_in, tokens_out, cost_usd,
                        planned_actions, executed_actions,
                        outcome, duration_sec, skills_used
                    ) VALUES (
                        $1, $2, $3,
                        $4, $5,
                        $6, $7, $8, $9,
                        $10::jsonb, $11::jsonb,
                        $12, $13, $14
                    )
                    """,
                    run.id,
                    run.org_id,
                    run.session_id,
                    run.goal[:4096],
                    task_pattern,
                    None,           # model — not tracked on RunRecord yet
                    tokens_in,
                    tokens_out,
                    run.total_cost_usd or 0.0,
                    json.dumps(planned_actions_json),
                    json.dumps(executed_actions_json),
                    outcome,
                    duration_sec,
                    skills_used,
                )
            logger.debug(
                "trajectory_recorded",
                extra={
                    "run_id": run.id,
                    "org_id": run.org_id,
                    "outcome": outcome,
                    "pattern": task_pattern,
                },
            )
        except Exception as exc:
            logger.warning(
                "trajectory_insert_failed",
                extra={"run_id": run.id, "error": str(exc)},
            )

    def record_nowait(
        self,
        run: RunRecord,
        *,
        tokens_in: int = 0,
        tokens_out: int = 0,
        started_at: float | None = None,
    ) -> None:
        """Schedule recording as a background task — non-blocking."""
        asyncio.ensure_future(
            self.record(
                run,
                tokens_in=tokens_in,
                tokens_out=tokens_out,
                started_at=started_at,
            )
        )
