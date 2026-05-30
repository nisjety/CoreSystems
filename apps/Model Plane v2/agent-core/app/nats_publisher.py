"""NATS event publisher — publishes agent events to session-core.

Two subject families:
- velion.agent.run.{run_id}.event  (v2 canonical)
- aqencia.reasoning.*              (v1 backward compat, on local NATS)

All published payloads conform to the typed contract in
``app.events.contract``.  Callers should import this module only;
do not import nats_publisher from app.events (circular).
"""

from __future__ import annotations

import logging
import time
from datetime import datetime, timezone
from typing import Any

from app.domain import AgentEvent, RunRecord, get_query_depth
from app.events.contract import make_subject
from app.nats_client import NatsManager

logger = logging.getLogger(__name__)


def _iso_now() -> str:
    return datetime.now(timezone.utc).isoformat()


class EventPublisher:
    """Publishes agent run events to NATS JetStream."""

    def __init__(self, nats_mgr: NatsManager) -> None:
        self._nats = nats_mgr
        self._seq: dict[str, int] = {}  # run_id → last sequence

    def _next_seq(self, run_id: str) -> int:
        seq = self._seq.get(run_id, 0) + 1
        self._seq[run_id] = seq
        return seq

    async def publish(self, event: AgentEvent) -> None:
        """Publish an event on the v2 subject and optionally on compat."""
        subject = f"velion.agent.run.{event.run_id}.event"
        event.sequence = self._next_seq(event.run_id)
        payload = event.model_dump(mode="json")

        await self._nats.publish_jetstream(subject, payload)
        logger.debug("event_published", extra={"subject": subject, "type": event.event_type})

    def _with_run_lineage(self, run: RunRecord, payload: dict[str, Any]) -> dict[str, Any]:
        enriched = dict(payload)
        enriched["query_depth"] = get_query_depth(run.metadata)
        if run.parent_run_id:
            enriched["parent_run_id"] = run.parent_run_id
        return enriched

    # ---- convenience methods ----

    async def run_started(self, run: RunRecord) -> None:
        await self.publish(AgentEvent(
            event_type="run.started",
            run_id=run.id,
            session_id=run.session_id,
            payload=self._with_run_lineage(run, {
                "agent_type": run.agent_type.value,
                "mode": run.mode.value,
                "goal": run.goal,
            }),
        ))
        # v1 compat
        await self._compat_reasoning_started(run)

    async def run_completed(self, run: RunRecord) -> None:
        await self.publish(AgentEvent(
            event_type="run.completed",
            run_id=run.id,
            session_id=run.session_id,
            payload=self._with_run_lineage(run, {
                "status": run.status.value,
                "final_output": run.final_output,
                "checkpoint_index": run.checkpoint_index,
            }),
        ))
        await self._compat_reasoning_completed(run)

    async def run_failed(self, run: RunRecord) -> None:
        await self.publish(AgentEvent(
            event_type="run.failed",
            run_id=run.id,
            session_id=run.session_id,
            payload=self._with_run_lineage(run, {"error": run.error}),
        ))
        await self._compat_reasoning_failed(run)

    async def action_started(
        self,
        run_id: str,
        session_id: str,
        action_id: str,
        action_name: str,
        query_depth: int | None = None,
    ) -> None:
        await self.publish(AgentEvent(
            event_type="action.started",
            run_id=run_id,
            session_id=session_id,
            payload={
                "action_id": action_id,
                "action_name": action_name,
                "query_depth": query_depth,
            },
        ))

    async def action_completed(
        self,
        run_id: str,
        session_id: str,
        action_id: str,
        output: Any = None,
        error: str | None = None,
        query_depth: int | None = None,
    ) -> None:
        await self.publish(AgentEvent(
            event_type="action.completed",
            run_id=run_id,
            session_id=session_id,
            payload={
                "action_id": action_id,
                "output": output,
                "error": error,
                "query_depth": query_depth,
            },
        ))

    async def approval_requested(
        self,
        run_id: str,
        session_id: str,
        approval_id: str,
        action_name: str,
        reason: str,
        query_depth: int | None = None,
    ) -> None:
        await self.publish(AgentEvent(
            event_type="approval.requested",
            run_id=run_id,
            session_id=session_id,
            payload={
                "approval_id": approval_id,
                "action_name": action_name,
                "reason": reason,
                "query_depth": query_depth,
            },
        ))

    async def todo_updated(
        self,
        run_id: str,
        session_id: str,
        todo_id: str,
        content: str,
        status: str,
        query_depth: int | None = None,
    ) -> None:
        await self.publish(AgentEvent(
            event_type="todo.updated",
            run_id=run_id,
            session_id=session_id,
            payload={
                "todo_id": todo_id,
                "content": content,
                "status": status,
                "query_depth": query_depth,
            },
        ))

    # ---- plan-mode events -----------------------------------------------

    async def plan_created(
        self,
        run_id: str,
        session_id: str,
        approval_id: str,
        plan_markdown: str,
        query_depth: int | None = None,
    ) -> None:
        """Emitted when the agent proposes a plan and waits for user approval."""
        await self.publish(AgentEvent(
            event_type="plan.created",
            run_id=run_id,
            session_id=session_id,
            payload={
                "approval_id": approval_id,
                "plan_markdown": plan_markdown,
                "query_depth": query_depth,
            },
        ))

    # ---- subagent spawn -------------------------------------------------

    async def subagent_spawned(
        self,
        run: RunRecord,
        child_run_id: str,
        child_agent_id: str,
        delegation_reason: str = "",
    ) -> None:
        """Emitted by parent run when it delegates work to a subagent."""
        await self.publish(AgentEvent(
            event_type="subagent.spawned",
            run_id=run.id,
            session_id=run.session_id,
            payload=self._with_run_lineage(run, {
                "child_run_id": child_run_id,
                "child_agent_id": child_agent_id,
                "delegation_reason": delegation_reason,
            }),
        ))

    # ---- recovery -------------------------------------------------------

    async def recovery_attempted(
        self,
        run: RunRecord,
        attempt: int,
        strategy: str,
        reason: str,
    ) -> None:
        """Emitted each time the recovery loop is triggered for this run."""
        await self.publish(AgentEvent(
            event_type="recovery.attempted",
            run_id=run.id,
            session_id=run.session_id,
            payload=self._with_run_lineage(run, {
                "attempt": attempt,
                "strategy": strategy,
                "reason": reason,
            }),
        ))

    # ---- v1 backward compatibility — usage / decision / quota ----

    async def usage_recorded(
        self,
        run: RunRecord,
        metric: str,
        units: int,
        metadata: dict[str, Any] | None = None,
    ) -> None:
        """Emit aqencia.reasoning.usage.recorded (v1 compat)."""
        await self._nats.publish_jetstream(
            "aqencia.reasoning.usage.recorded",
            {
                "org_id": run.org_id or "",
                "reasoning_id": run.id,
                "metric": metric,
                "units": units,
                "metadata": metadata or {},
                "service": "agent-core-v2",
                "timestamp": _iso_now(),
            },
            local=True,
        )

    async def decision_made(
        self,
        run: RunRecord,
        decision_type: str,
        decision_value: str = "",
        confidence: float = 0.0,
    ) -> None:
        """Emit aqencia.reasoning.decision.made (v1 compat)."""
        await self._nats.publish_jetstream(
            "aqencia.reasoning.decision.made",
            {
                "org_id": run.org_id or "",
                "reasoning_id": run.id,
                "decision_type": decision_type,
                "decision_value": decision_value,
                "confidence": confidence,
                "timestamp": _iso_now(),
            },
            local=True,
        )

    async def quota_exceeded(
        self,
        run: RunRecord,
        metric: str,
        limit: int,
        current: int,
    ) -> None:
        """Emit aqencia.reasoning.quota.exceeded (v1 compat)."""
        await self._nats.publish_jetstream(
            "aqencia.reasoning.quota.exceeded",
            {
                "org_id": run.org_id or "",
                "reasoning_id": run.id,
                "metric": metric,
                "limit": limit,
                "current": current,
                "service": "agent-core-v2",
                "timestamp": _iso_now(),
            },
            local=True,
        )

    # ---- v1 backward compatibility (aqencia.reasoning.*) ----

    async def _compat_reasoning_started(self, run: RunRecord) -> None:
        await self._nats.publish_jetstream(
            "aqencia.reasoning.reasoning.started",
            {
                "org_id": run.org_id or "",
                "reasoning_id": run.id,
                "prompt": run.goal,
                "timestamp": datetime.now(timezone.utc).isoformat(),
            },
            local=True,
        )

    async def _compat_reasoning_completed(self, run: RunRecord) -> None:
        await self._nats.publish_jetstream(
            "aqencia.reasoning.reasoning.completed",
            {
                "org_id": run.org_id or "",
                "reasoning_id": run.id,
                "prompt": run.goal,
                "conclusion": run.final_output or "",
                "timestamp": datetime.now(timezone.utc).isoformat(),
            },
            local=True,
        )

    async def _compat_reasoning_failed(self, run: RunRecord) -> None:
        await self._nats.publish_jetstream(
            "aqencia.reasoning.reasoning.completed",
            {
                "org_id": run.org_id or "",
                "reasoning_id": run.id,
                "prompt": run.goal,
                "conclusion": f"error: {run.error or 'unknown'}",
                "timestamp": datetime.now(timezone.utc).isoformat(),
            },
            local=True,
        )
