"""Core data types for the eval framework."""

from __future__ import annotations

from pydantic import BaseModel, Field


class EvalCase(BaseModel, frozen=True):
    """A single evaluation test case."""

    id: str
    name: str
    input_prompt: str
    expected_output: str | None = None
    expected_model: str | None = None
    tags: list[str] = Field(default_factory=list)
    metadata: dict[str, object] = Field(default_factory=dict)


class EvalResult(BaseModel, frozen=True):
    """Result of running a single eval case."""

    case_id: str
    passed: bool
    actual_output: str
    score: float = Field(ge=0.0, le=1.0)
    latency_ms: float
    error: str | None = None


class EvalSuite(BaseModel, frozen=True):
    """A collection of eval cases to run together."""

    id: str
    name: str
    cases: list[EvalCase]
    description: str = ""
