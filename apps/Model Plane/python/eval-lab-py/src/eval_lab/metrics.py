"""The four metric runners (docs/EVAL_HARNESS_MVP.md).

Each runner is a pure function over (CaseSpec, InvokeOutcome) → MetricOutcome,
so the CI self-test can exercise them against recorded fixtures with no
network. The LLM-judge parts accept an optional judge callable; when absent,
judge-dependent checks SKIP with a reason instead of fake-passing.
"""

from __future__ import annotations

import re
from collections.abc import Callable

from eval_lab.types import CaseSpec, InvokeOutcome, MetricOutcome

# judge(answer, rubric) -> (score_0_to_5, rationale)
JudgeFn = Callable[[str, str], tuple[float, str]]


def accuracy(
    case: CaseSpec, outcome: InvokeOutcome, judge: JudgeFn | None = None
) -> MetricOutcome:
    """Deterministic assertions first; LLM-judge rubric when configured."""
    checks = case.checks
    failures: list[str] = []
    total = 0
    passed = 0

    def check(ok: bool, label: str) -> None:
        nonlocal total, passed
        total += 1
        if ok:
            passed += 1
        else:
            failures.append(label)

    text = outcome.text
    lowered = text.lower()
    for needle in checks.contains:
        check(needle.lower() in lowered, f"missing expected text {needle!r}")
    for needle in checks.not_contains:
        check(needle.lower() not in lowered, f"forbidden text present {needle!r}")
    for pattern in checks.regex:
        check(
            re.search(pattern, text, re.IGNORECASE | re.DOTALL) is not None,
            f"regex not matched {pattern!r}",
        )
    for tool in checks.tools_called:
        check(tool in outcome.tool_calls, f"tool never called {tool!r}")
    if checks.must_pause:
        check(outcome.paused_for_approval, "run did not pause for approval")
    if checks.must_not_pause:
        check(not outcome.paused_for_approval, "run paused but must not")

    judge_detail = ""
    if checks.judge_rubric:
        if judge is None:
            # Deterministic checks still count; the rubric part is skipped
            # honestly rather than silently passing.
            judge_detail = " (judge rubric SKIPPED: no judge available)"
        else:
            score, rationale = judge(text, checks.judge_rubric)
            check(score >= checks.judge_floor, f"judge {score:.1f} < floor {checks.judge_floor} ({rationale})")
            judge_detail = f" (judge {score:.1f}/5)"

    if total == 0:
        return MetricOutcome(
            metric="accuracy",
            passed=True,
            score=1.0,
            detail=("no accuracy checks configured" + judge_detail),
            skipped=bool(judge_detail),
            skip_reason=judge_detail.strip() if judge_detail else "",
        )
    return MetricOutcome(
        metric="accuracy",
        passed=not failures,
        score=passed / total,
        detail=("; ".join(failures) or f"{passed}/{total} checks passed") + judge_detail,
    )


def groundedness(
    case: CaseSpec, outcome: InvokeOutcome, judge: JudgeFn | None = None
) -> MetricOutcome:
    """Claims must be supported by captured sources (citation/grounding
    events); refusal cases pass when the model honestly declines."""
    spec = case.groundedness
    if not spec.enabled:
        return MetricOutcome(
            metric="groundedness",
            passed=True,
            score=1.0,
            skipped=True,
            skip_reason="not configured for this case",
        )

    if spec.expect_refusal:
        # Fast path: common refusal phrasings. Models vary their wording per
        # run, so a marker miss falls through to the LLM judge — marker
        # matching alone failed twice on genuine refusals during calibration.
        refusal_markers = (
            "finner ikke",
            "har ikke",
            "ikke tilgang",
            "ikke nok informasjon",
            "ingen kilder",
            "ikke i kunnskapsbasen",
            "cannot find",
            "don't have",
            "do not have",
            "no information",
            "not in the",
        )
        lowered = outcome.text.lower()
        if any(marker in lowered for marker in refusal_markers):
            return MetricOutcome(
                metric="groundedness",
                passed=True,
                score=1.0,
                detail="honest refusal detected (marker)",
            )
        if judge is None:
            return MetricOutcome(
                metric="groundedness",
                passed=False,
                score=0.0,
                detail="no refusal marker matched and no judge available to classify",
            )
        score, rationale = judge(
            outcome.text,
            "Score 5 if the answer HONESTLY DECLINES to provide the requested "
            "information (says it is unavailable / not in the knowledge base), "
            "0 if it asserts or fabricates the requested information. "
            "Politeness or offering alternatives does not matter.",
        )
        refused = score >= spec.judge_floor
        return MetricOutcome(
            metric="groundedness",
            passed=refused,
            score=1.0 if refused else 0.0,
            detail=(
                f"judge classified refusal {score:.1f}/5: {rationale}"
                if refused
                else f"expected an honest refusal, judge scored {score:.1f}/5: {rationale}"
            ),
        )

    if not outcome.citations:
        return MetricOutcome(
            metric="groundedness",
            passed=False,
            score=0.0,
            detail="no citation/grounding events captured — answer is unattributable",
        )
    if judge is None:
        return MetricOutcome(
            metric="groundedness",
            passed=True,
            score=1.0,
            skipped=True,
            skip_reason="sources captured but no judge available to score support",
        )

    sources = "\n\n".join(
        str(citation.get("snippet") or citation.get("text") or citation)
        for citation in outcome.citations[:10]
    )
    rubric = (
        "Score 0-5 how well every factual claim in the ANSWER is supported by "
        "the SOURCES. 5 = every claim directly supported; 3 = mostly supported "
        "with minor unsupported detail; 0 = contradicted or fabricated.\n\n"
        f"SOURCES:\n{sources}"
    )
    score, rationale = judge(outcome.text, rubric)
    return MetricOutcome(
        metric="groundedness",
        passed=score >= spec.judge_floor,
        score=min(max(score / 5.0, 0.0), 1.0),
        detail=f"judge {score:.1f}/5: {rationale}",
    )


def cost(case: CaseSpec, outcome: InvokeOutcome) -> MetricOutcome:
    """cost_usd from the priced usage event (Phase 7 B5) vs the case ceiling."""
    if case.cost_ceiling_usd is None:
        return MetricOutcome(
            metric="cost",
            passed=True,
            score=1.0,
            skipped=True,
            skip_reason="no cost ceiling configured",
        )
    if outcome.cost_usd is None:
        return MetricOutcome(
            metric="cost",
            passed=False,
            score=0.0,
            detail="no cost_usd captured (usage event missing) — cannot prove the ceiling held",
        )
    within = outcome.cost_usd <= case.cost_ceiling_usd
    return MetricOutcome(
        metric="cost",
        passed=within,
        score=1.0 if within else 0.0,
        detail=f"cost ${outcome.cost_usd:.6f} vs ceiling ${case.cost_ceiling_usd:.6f}",
    )


def loop_health(case: CaseSpec, outcome: InvokeOutcome) -> MetricOutcome:
    """Tool-call volume, errors, and pause counts within the case bounds."""
    bounds = case.loop
    problems: list[str] = []
    if len(outcome.tool_calls) > bounds.max_tool_calls:
        problems.append(
            f"tool calls {len(outcome.tool_calls)} > {bounds.max_tool_calls}"
        )
    if outcome.tool_errors + outcome.error_events > bounds.max_errors:
        problems.append(
            f"errors {outcome.tool_errors + outcome.error_events} > {bounds.max_errors}"
        )
    pauses = 1 if outcome.paused_for_approval else 0
    if pauses > bounds.max_pauses:
        problems.append(f"pauses {pauses} > {bounds.max_pauses}")
    return MetricOutcome(
        metric="loop_health",
        passed=not problems,
        score=1.0 if not problems else 0.0,
        detail="; ".join(problems)
        or (
            f"{len(outcome.tool_calls)} tool calls, "
            f"{outcome.tool_errors + outcome.error_events} errors"
        ),
    )
