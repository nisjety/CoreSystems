"""Harness self-tests — the CI path (no network, recorded fixtures only).

These keep the harness itself from rotting: case-schema validation over the
real checked-in suite, the SSE parser against a canned stream, and every
metric runner against recorded outcomes.
"""

from __future__ import annotations

from eval_lab import metrics
from eval_lab.cases import CASES_DIR, load_cases
from eval_lab.client import parse_sse_lines, _assemble_outcome
from eval_lab.report import render_report
from eval_lab.types import (
    CaseResult,
    CaseSpec,
    Checks,
    GroundednessSpec,
    InvokeOutcome,
    LoopBounds,
    MetricOutcome,
)


# ── case schema ──────────────────────────────────────────────────────────


def test_baseline_suite_loads_and_validates() -> None:
    cases = load_cases(CASES_DIR)
    assert len(cases) == 12, f"baseline suite must hold 12 cases, got {len(cases)}"
    assert len({case.id for case in cases}) == 12


def test_deployed_agent_cases_request_the_agentic_feature() -> None:
    # model-gateway only drives execution-core's tool catalog when the
    # request opts into the "agentic" feature (sse.rs: `features.iter().any(
    # |f| f == "agentic")`); profile=deployed_agent alone only affects
    # approval POSTURE once a run is already agentic. Missing this made
    # every deployed_agent case silently fall back to a plain completion
    # with zero tool calls (discovered 2026-07-08 calibrating case 07 — the
    # "multitool-loop-health" case had been passing vacuously on 0 tool
    # calls). This test makes that class of bug impossible to reintroduce.
    for case in load_cases(CASES_DIR):
        if case.profile == "deployed_agent":
            assert "agentic" in case.features, (
                f"{case.id}: profile=deployed_agent but features={case.features} "
                "is missing 'agentic' — the tool loop will never run"
            )


def test_vendor_dependent_cases_declare_requirements() -> None:
    # Cases that need seeded fixtures must say so — that is what turns a
    # missing fixture into an honest SKIP instead of a fake pass.
    cases = {case.id: case for case in load_cases(CASES_DIR)}
    assert "knowledge-fixtures" in cases["02-knowledge-crawled-fact"].requires
    assert "image-fixture" in cases["05-cross-modal-retrieval"].requires
    assert "zdr-fixture" in cases["12-zdr-restricted-grounding"].requires


# ── SSE parsing ──────────────────────────────────────────────────────────


CANNED_STREAM = [
    "event: connected",
    'data: {"request_id": "req_1", "ok": true}',
    "",
    "event: chunk",
    'data: {"delta": "Hei "}',
    "",
    "event: chunk",
    'data: {"delta": "verden"}',
    "",
    "event: tool_call",
    'data: {"name": "knowledge_search", "args": {}}',
    "",
    "event: usage",
    'data: {"input_tokens": 12, "output_tokens": 5, "cost_usd": 0.00042, "latency_ms": 900}',
    "",
    "event: done",
    'data: {"model_used": "verevon-budget"}',
    "",
]


def test_sse_parser_and_outcome_assembly() -> None:
    events = parse_sse_lines(iter(CANNED_STREAM))
    outcome = _assemble_outcome(events, fallback_latency_ms=1.0)
    assert outcome.text == "Hei verden"
    assert outcome.request_id == "req_1"
    assert outcome.cost_usd == 0.00042
    assert outcome.input_tokens == 12 and outcome.output_tokens == 5
    assert outcome.tool_calls == ["knowledge_search"]
    assert outcome.model_used == "verevon-budget"
    assert not outcome.paused_for_approval


def test_sse_parser_detects_approval_pause() -> None:
    stream = [
        "event: run.paused_for_approval",
        'data: {"run_id": "run_9", "approval_id": "appr_1"}',
        "",
        "event: done",
        "data: {}",
        "",
    ]
    outcome = _assemble_outcome(parse_sse_lines(iter(stream)), fallback_latency_ms=1.0)
    assert outcome.paused_for_approval
    assert outcome.run_id == "run_9"


# ── metric runners against recorded outcomes ─────────────────────────────


def spec(**overrides: object) -> CaseSpec:
    base: dict[str, object] = {"id": "t", "name": "t", "prompt": "p"}
    base.update(overrides)
    return CaseSpec.model_validate(base)


def test_accuracy_deterministic_checks() -> None:
    case = spec(
        checks=Checks(
            contains=["pong"], not_contains=["feil"], regex=[r"PO\w+"]
        ).model_dump()
    )
    good = metrics.accuracy(case, InvokeOutcome(text="PONG"))
    assert good.passed and good.score == 1.0
    bad = metrics.accuracy(case, InvokeOutcome(text="feil svar"))
    assert not bad.passed and bad.score < 1.0
    assert "missing expected text" in bad.detail


def test_accuracy_judge_rubric_skips_without_judge() -> None:
    case = spec(checks=Checks(judge_rubric="good answer").model_dump())
    outcome = metrics.accuracy(case, InvokeOutcome(text="whatever"), judge=None)
    assert "SKIPPED" in outcome.detail  # honest, visible skip


def test_accuracy_judge_floor_enforced() -> None:
    case = spec(checks=Checks(judge_rubric="rubric", judge_floor=4.0).model_dump())
    low_judge = lambda answer, rubric: (2.0, "weak")  # noqa: E731
    outcome = metrics.accuracy(case, InvokeOutcome(text="x"), judge=low_judge)
    assert not outcome.passed


def test_pause_assertions() -> None:
    must_pause = spec(checks=Checks(must_pause=True).model_dump())
    paused = InvokeOutcome(paused_for_approval=True)
    unpaused = InvokeOutcome()
    assert metrics.accuracy(must_pause, paused).passed
    assert not metrics.accuracy(must_pause, unpaused).passed


def test_groundedness_refusal_paths() -> None:
    case = spec(
        groundedness=GroundednessSpec(enabled=True, expect_refusal=True).model_dump()
    )
    refused = metrics.groundedness(
        case, InvokeOutcome(text="Jeg finner ikke dette i kildene.")
    )
    assert refused.passed
    fabricated = metrics.groundedness(
        case, InvokeOutcome(text="Dørkoden er 4471 og den gjelder hele bygget.")
    )
    assert not fabricated.passed


def test_groundedness_requires_citations() -> None:
    case = spec(groundedness=GroundednessSpec(enabled=True).model_dump())
    no_sources = metrics.groundedness(case, InvokeOutcome(text="Påstand uten kilde."))
    assert not no_sources.passed
    assert "unattributable" in no_sources.detail


def test_cost_ceiling() -> None:
    case = spec(cost_ceiling_usd=0.01)
    under = metrics.cost(case, InvokeOutcome(cost_usd=0.004))
    assert under.passed
    over = metrics.cost(case, InvokeOutcome(cost_usd=0.02))
    assert not over.passed
    missing = metrics.cost(case, InvokeOutcome())
    assert not missing.passed  # unprovable ceiling = fail, not silent pass


def test_loop_health_bounds() -> None:
    case = spec(loop=LoopBounds(max_tool_calls=2, max_errors=0).model_dump())
    ok = metrics.loop_health(case, InvokeOutcome(tool_calls=["a", "b"]))
    assert ok.passed
    too_many = metrics.loop_health(
        case, InvokeOutcome(tool_calls=["a", "b", "c"], tool_errors=1)
    )
    assert not too_many.passed


# ── report ───────────────────────────────────────────────────────────────


def test_report_renders_pass_fail_and_skip() -> None:
    results = [
        CaseResult(
            case_id="ok",
            passed=True,
            metrics=[MetricOutcome(metric="accuracy", passed=True, score=1.0)],
        ),
        CaseResult(
            case_id="bad",
            passed=False,
            metrics=[
                MetricOutcome(
                    metric="cost", passed=False, score=0.0, detail="over ceiling"
                )
            ],
        ),
        CaseResult(case_id="later", passed=False, skipped=True, skip_reason="no fixture"),
    ]
    report = render_report(results)
    assert "1/2 passed" in report
    assert "⏭ SKIP" in report
    assert "over ceiling" in report
