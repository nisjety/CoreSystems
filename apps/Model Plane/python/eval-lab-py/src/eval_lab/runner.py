"""Case orchestration: fixtures → invoke → metrics → CaseResult.

`run_case` is the single entry the pytest suite parametrizes over. The
legacy exact-match runner this file used to hold was replaced by the MVP
harness (docs/EVAL_HARNESS_MVP.md).
"""

from __future__ import annotations

import os
import uuid

from eval_lab import metrics
from eval_lab.client import VerevonClient
from eval_lab.judge import make_judge
from eval_lab.types import CaseResult, CaseSpec, InvokeOutcome, MetricOutcome

# Fixture org registry: org_fixture key → (org_id, user_id). The eval orgs
# are dedicated, seeded tenants — never real customer orgs. Overridable via
# env so a shared fleet can namespace them.
FIXTURE_ORGS: dict[str, tuple[str, str]] = {
    "eval-org-a": (
        os.environ.get("EVAL_ORG_A", "org_eval_a"),
        os.environ.get("EVAL_USER_A", "user_eval_a"),
    ),
    "eval-org-b": (
        os.environ.get("EVAL_ORG_B", "org_eval_b"),
        os.environ.get("EVAL_USER_B", "user_eval_b"),
    ),
}

# Requirements satisfied by the current environment. Cases whose `requires`
# are not all present SKIP with a reason. Extend via EVAL_CAPABILITIES
# (comma-separated) as fixtures get seeded.
def available_capabilities() -> set[str]:
    raw = os.environ.get("EVAL_CAPABILITIES", "")
    return {item.strip() for item in raw.split(",") if item.strip()}


def _run_turns(client: VerevonClient, token: str, case: CaseSpec) -> InvokeOutcome:
    """Send `case.seed_turns` (if any) then `case.prompt`, all in one session,
    and return only the LAST turn's outcome -- that is the one every metric
    scores. A one-shot case (`seed_turns` empty) is exactly today's single
    call, byte-for-byte, so nothing about the existing 12 baseline cases
    changes.

    A fresh session key per RUN (not per case id) so repeated eval runs of
    the same compaction case do not collide onto one durable thread.
    """
    if not case.seed_turns:
        return client.invoke_stream(token, case, idempotency_key=f"eval:{case.id}")

    session_key = f"eval:{case.id}:{uuid.uuid4().hex[:12]}"
    for index, turn in enumerate(case.seed_turns):
        seed_outcome = client.invoke_stream(
            token,
            case,
            content=turn,
            session_key=session_key,
            idempotency_key=f"{session_key}:seed:{index}",
        )
        if seed_outcome.transport_error:
            # Fail fast and honestly: a seed turn that never reached the
            # gateway means the history this case depends on was never
            # built, so scoring the recall question would test nothing.
            return seed_outcome
    return client.invoke_stream(
        token,
        case,
        session_key=session_key,
        idempotency_key=f"{session_key}:recall",
    )


def run_case(client: VerevonClient, case: CaseSpec) -> CaseResult:
    missing = [r for r in case.requires if r not in available_capabilities()]
    if missing:
        return CaseResult(
            case_id=case.id,
            passed=False,
            skipped=True,
            skip_reason=f"unmet requirements: {', '.join(missing)} "
            "(set EVAL_CAPABILITIES once the fixture exists)",
        )
    org = FIXTURE_ORGS.get(case.org_fixture)
    if org is None:
        return CaseResult(
            case_id=case.id,
            passed=False,
            skipped=True,
            skip_reason=f"unknown org fixture {case.org_fixture!r}",
        )

    token = client.mint_token(*org)
    outcome = _run_turns(client, token, case)
    if outcome.transport_error:
        return CaseResult(
            case_id=case.id,
            passed=False,
            metrics=[
                MetricOutcome(
                    metric="accuracy",
                    passed=False,
                    score=0.0,
                    detail=f"transport error: {outcome.transport_error}",
                )
            ],
            outcome=outcome,
        )

    judge = None
    if case.checks.judge_rubric or case.groundedness.enabled:
        # Judge as org A regardless of case org: the judge sees only the
        # answer + rubric text, never org-scoped resources.
        judge_token = client.mint_token(*FIXTURE_ORGS["eval-org-a"])
        judge = make_judge(client, judge_token)

    results = [
        metrics.accuracy(case, outcome, judge),
        metrics.groundedness(case, outcome, judge),
        metrics.cost(case, outcome),
        metrics.loop_health(case, outcome),
    ]
    passed = all(metric.passed or metric.skipped for metric in results)
    return CaseResult(case_id=case.id, passed=passed, metrics=results, outcome=outcome)
