"""Typed coordination result — structured output from coordinator synthesis.

Replaces the plain dict returned by aggregate_worker_results() with
a Pydantic model that provides schema, serialization, and type safety.
"""

from __future__ import annotations

from pydantic import BaseModel, Field


class ChildRunSummary(BaseModel):
    """Summary of a single child worker run."""

    run_id: str
    agent_id: str | None = None
    status: str
    final_output: str | None = None
    error: str | None = None


class CoordinationResult(BaseModel):
    """Typed result from coordinator worker aggregation.

    Used by synthesize() and aggregate_worker_results() to return
    structured data instead of ad-hoc dicts.
    """

    child_count: int = 0
    status_summary: dict[str, int] = Field(default_factory=dict)
    all_done: bool = False
    children: list[ChildRunSummary] = Field(default_factory=list)
    outputs: list[str] = Field(default_factory=list)
    synthesis: str | None = None

    @classmethod
    def from_children(
        cls,
        children: list,  # list[RunRecord]
        synthesis: str | None = None,
    ) -> CoordinationResult:
        """Build from a list of RunRecord objects."""
        summary: dict[str, int] = {}
        outputs: list[str] = []
        child_summaries: list[ChildRunSummary] = []

        for child in children:
            status_val = child.status.value if hasattr(child.status, "value") else str(child.status)
            summary[status_val] = summary.get(status_val, 0) + 1

            if child.final_output:
                outputs.append(child.final_output)

            agent_id = (child.metadata or {}).get("agent_id") if hasattr(child, "metadata") else None
            child_summaries.append(ChildRunSummary(
                run_id=child.id,
                agent_id=agent_id,
                status=status_val,
                final_output=child.final_output,
                error=child.error if hasattr(child, "error") else None,
            ))

        from app.domain import RunStatus

        all_done = all(
            child.status in (RunStatus.COMPLETED, RunStatus.FAILED, RunStatus.CANCELLED)
            for child in children
        )

        return cls(
            child_count=len(children),
            status_summary=summary,
            all_done=all_done,
            children=child_summaries,
            outputs=outputs,
            synthesis=synthesis,
        )
