"""Core data types for the eval harness.

The 2026-07 MVP (docs/EVAL_HARNESS_MVP.md) replaced the original
exact-string-match scaffold with a live-stack harness: declarative YAML cases
driven against model-gateway /v1/invoke, scored on four metrics (accuracy,
groundedness, cost, loop health).
"""

from __future__ import annotations

from pydantic import BaseModel, Field


class Checks(BaseModel, frozen=True):
    """Deterministic accuracy assertions for one case."""

    contains: list[str] = Field(default_factory=list)
    not_contains: list[str] = Field(default_factory=list)
    regex: list[str] = Field(default_factory=list)
    tools_called: list[str] = Field(default_factory=list)
    # HITL posture assertions: the run must (or must not) pause for approval.
    must_pause: bool = False
    must_not_pause: bool = False
    # Optional LLM-judge rubric applied to the final answer (0-5 scale; pass
    # floor is judge_floor). Skipped-with-reason when no judge is available.
    judge_rubric: str | None = None
    judge_floor: float = 3.5


class LoopBounds(BaseModel, frozen=True):
    """Retry/loop-health ceilings for one case (from the event stream)."""

    max_tool_calls: int = 12
    max_errors: int = 0
    max_pauses: int = 1


class GroundednessSpec(BaseModel, frozen=True):
    """Groundedness config: claims in the answer must be supported by the
    sources captured from the run's citation/grounding events."""

    enabled: bool = False
    # When the case expects an honest refusal (answer not in sources → say
    # so), the metric passes when the model declines instead of fabricating.
    expect_refusal: bool = False
    judge_floor: float = 3.5


class CaseSpec(BaseModel, frozen=True):
    """One declarative eval case (loaded from cases/**/*.yaml)."""

    id: str
    name: str
    prompt: str
    model: str | None = None  # velion-budget | velion-balance | velion-genius
    profile: str = "chat"  # "chat" | "deployed_agent"
    features: list[str] = Field(
        default_factory=lambda: ["usage", "tools", "citations"]
    )
    zdr: bool = False
    org_fixture: str = "eval-org-a"  # key into the fixture org registry
    max_cost_usd: float | None = None  # sent to the gateway (budget guard)
    cost_ceiling_usd: float | None = None  # metric ceiling (fail if exceeded)
    checks: Checks = Field(default_factory=Checks)
    loop: LoopBounds = Field(default_factory=LoopBounds)
    groundedness: GroundednessSpec = Field(default_factory=GroundednessSpec)
    # Preconditions (e.g. "knowledge-fixtures", "zdr-fixture",
    # "image-fixture"). Unmet requirements SKIP the case with a reason —
    # never a silent fake pass.
    requires: list[str] = Field(default_factory=list)
    tags: list[str] = Field(default_factory=list)
    timeout_s: float = 120.0


class StreamEvent(BaseModel, frozen=True):
    """One parsed SSE event from /v1/invoke/stream."""

    event: str
    data: dict[str, object] = Field(default_factory=dict)


class InvokeOutcome(BaseModel, frozen=True):
    """Everything captured from driving one case against the live stack."""

    text: str = ""
    events: list[StreamEvent] = Field(default_factory=list)
    request_id: str | None = None
    run_id: str | None = None
    model_used: str | None = None
    input_tokens: int | None = None
    output_tokens: int | None = None
    cost_usd: float | None = None
    latency_ms: float | None = None
    paused_for_approval: bool = False
    tool_calls: list[str] = Field(default_factory=list)
    tool_errors: int = 0
    error_events: int = 0
    citations: list[dict[str, object]] = Field(default_factory=list)
    transport_error: str | None = None


class MetricOutcome(BaseModel, frozen=True):
    """One metric's verdict for one case."""

    metric: str  # accuracy | groundedness | cost | loop_health
    passed: bool
    score: float = Field(ge=0.0, le=1.0)
    detail: str = ""
    skipped: bool = False
    skip_reason: str = ""


class CaseResult(BaseModel, frozen=True):
    """Aggregate result of one case run."""

    case_id: str
    passed: bool
    skipped: bool = False
    skip_reason: str = ""
    metrics: list[MetricOutcome] = Field(default_factory=list)
    outcome: InvokeOutcome | None = None


# ── Legacy scaffold types (kept importable; the exact-match runner they fed
# was replaced by the live harness above) ────────────────────────────────────


class EvalCase(BaseModel, frozen=True):
    """Legacy scaffold case (exact-match era)."""

    id: str
    name: str
    input_prompt: str
    expected_output: str | None = None
    expected_model: str | None = None
    tags: list[str] = Field(default_factory=list)
    metadata: dict[str, object] = Field(default_factory=dict)


class EvalResult(BaseModel, frozen=True):
    """Legacy scaffold result (exact-match era)."""

    case_id: str
    passed: bool
    actual_output: str
    score: float = Field(ge=0.0, le=1.0)
    latency_ms: float
    error: str | None = None


class EvalSuite(BaseModel, frozen=True):
    """Legacy scaffold suite (exact-match era)."""

    id: str
    name: str
    cases: list[EvalCase]
    description: str = ""
