"""Eval suite runner with configurable invoke function."""

from __future__ import annotations

import time
from collections.abc import Callable

from eval_lab.types import EvalCase, EvalResult, EvalSuite


class EvalRunner:
    """Runs all cases in an eval suite against a user-supplied callable.

    Args:
        invoke_fn: A callable that takes a prompt string and returns a string.
        threshold: Score threshold (0-1) for pass/fail when no exact match is
            used. Defaults to 0.8.
    """

    def __init__(
        self,
        invoke_fn: Callable[[str], str],
        *,
        threshold: float = 0.8,
    ) -> None:
        self._invoke_fn = invoke_fn
        self._threshold = threshold

    def run_case(self, case: EvalCase) -> EvalResult:
        """Run a single eval case and return the result."""
        start = time.perf_counter()
        error: str | None = None
        actual_output = ""

        try:
            actual_output = self._invoke_fn(case.input_prompt)
        except Exception as exc:  # noqa: BLE001
            error = str(exc)

        latency_ms = (time.perf_counter() - start) * 1000.0

        if error is not None:
            return EvalResult(
                case_id=case.id,
                passed=False,
                actual_output=actual_output,
                score=0.0,
                latency_ms=latency_ms,
                error=error,
            )

        score = self._compute_score(case, actual_output)
        passed = score >= self._threshold

        return EvalResult(
            case_id=case.id,
            passed=passed,
            actual_output=actual_output,
            score=score,
            latency_ms=latency_ms,
            error=None,
        )

    def run_suite(self, suite: EvalSuite) -> list[EvalResult]:
        """Run every case in *suite* and return the results."""
        return [self.run_case(case) for case in suite.cases]

    # ------------------------------------------------------------------
    # Scoring
    # ------------------------------------------------------------------

    @staticmethod
    def _compute_score(case: EvalCase, actual_output: str) -> float:
        """Compute a 0-1 score for *actual_output* against *case*.

        Uses exact-match when ``expected_output`` is set, returning 1.0 on
        match and 0.0 otherwise.  When no expected output is provided the
        score defaults to 1.0 (assume pass).
        """
        if case.expected_output is None:
            return 1.0
        return 1.0 if actual_output.strip() == case.expected_output.strip() else 0.0


def run_suite(
    suite: EvalSuite,
    invoke_fn: Callable[[str], str],
    *,
    threshold: float = 0.8,
) -> list[EvalResult]:
    """Convenience wrapper: run an eval suite and return results.

    Args:
        suite: The eval suite to execute.
        invoke_fn: A callable ``(prompt) -> response``.
        threshold: Score threshold for pass/fail (default 0.8).

    Returns:
        List of :class:`EvalResult`, one per case.
    """
    runner = EvalRunner(invoke_fn, threshold=threshold)
    return runner.run_suite(suite)
