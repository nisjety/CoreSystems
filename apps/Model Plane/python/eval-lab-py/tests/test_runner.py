"""Tests for eval_lab runner."""

from __future__ import annotations

import pytest

from eval_lab.runner import EvalRunner, run_suite
from eval_lab.types import EvalCase, EvalResult, EvalSuite


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

def _echo_fn(prompt: str) -> str:
    """Simple invoke_fn that echoes back the prompt."""
    return prompt


def _error_fn(prompt: str) -> str:
    """invoke_fn that always raises."""
    raise RuntimeError("boom")


def _make_suite(cases: list[EvalCase] | None = None) -> EvalSuite:
    if cases is None:
        cases = [
            EvalCase(
                id="c1",
                name="echo test",
                input_prompt="hello",
                expected_output="hello",
            ),
            EvalCase(
                id="c2",
                name="mismatch test",
                input_prompt="hello",
                expected_output="world",
            ),
        ]
    return EvalSuite(id="s1", name="Test Suite", cases=cases, description="A test suite")


# ---------------------------------------------------------------------------
# EvalRunner tests
# ---------------------------------------------------------------------------

class TestEvalRunner:
    def test_exact_match_pass(self) -> None:
        case = EvalCase(
            id="c1", name="match", input_prompt="hi", expected_output="hi"
        )
        runner = EvalRunner(_echo_fn)
        result = runner.run_case(case)
        assert result.passed is True
        assert result.score == 1.0
        assert result.error is None

    def test_exact_match_fail(self) -> None:
        case = EvalCase(
            id="c2", name="no-match", input_prompt="hi", expected_output="bye"
        )
        runner = EvalRunner(_echo_fn)
        result = runner.run_case(case)
        assert result.passed is False
        assert result.score == 0.0

    def test_no_expected_output_defaults_pass(self) -> None:
        case = EvalCase(id="c3", name="open", input_prompt="anything")
        runner = EvalRunner(_echo_fn)
        result = runner.run_case(case)
        assert result.passed is True
        assert result.score == 1.0

    def test_invoke_error_captured(self) -> None:
        case = EvalCase(
            id="c4", name="error", input_prompt="x", expected_output="x"
        )
        runner = EvalRunner(_error_fn)
        result = runner.run_case(case)
        assert result.passed is False
        assert result.error == "boom"
        assert result.score == 0.0

    def test_latency_recorded(self) -> None:
        case = EvalCase(id="c5", name="timing", input_prompt="fast")
        runner = EvalRunner(_echo_fn)
        result = runner.run_case(case)
        assert result.latency_ms >= 0.0

    def test_run_suite(self) -> None:
        suite = _make_suite()
        runner = EvalRunner(_echo_fn)
        results = runner.run_suite(suite)
        assert len(results) == 2
        assert results[0].passed is True
        assert results[1].passed is False


# ---------------------------------------------------------------------------
# Convenience function tests
# ---------------------------------------------------------------------------

class TestRunSuite:
    def test_convenience_wrapper(self) -> None:
        suite = _make_suite()
        results = run_suite(suite, _echo_fn)
        assert len(results) == 2
        assert all(isinstance(r, EvalResult) for r in results)

    def test_custom_threshold(self) -> None:
        case = EvalCase(
            id="c1", name="threshold", input_prompt="hi", expected_output="hi"
        )
        suite = _make_suite(cases=[case])
        results = run_suite(suite, _echo_fn, threshold=1.0)
        assert results[0].passed is True
