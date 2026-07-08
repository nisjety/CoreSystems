"""Live-stack eval harness for Model Plane (docs/EVAL_HARNESS_MVP.md)."""

from eval_lab.cases import load_cases
from eval_lab.client import EvalEnv, VelionClient
from eval_lab.runner import run_case
from eval_lab.types import CaseResult, CaseSpec, InvokeOutcome, MetricOutcome

__all__ = [
    "CaseResult",
    "CaseSpec",
    "EvalEnv",
    "InvokeOutcome",
    "MetricOutcome",
    "VelionClient",
    "load_cases",
    "run_case",
]
