"""Declarative case loading: cases/**/*.yaml → validated CaseSpec objects."""

from __future__ import annotations

from pathlib import Path

import yaml
from pydantic import ValidationError

from eval_lab.types import CaseSpec

CASES_DIR = Path(__file__).resolve().parents[2] / "cases"


class CaseLoadError(ValueError):
    """A YAML case file failed schema validation."""


def load_case_file(path: Path) -> CaseSpec:
    raw = yaml.safe_load(path.read_text(encoding="utf-8"))
    if not isinstance(raw, dict):
        raise CaseLoadError(f"{path}: case file must be a mapping")
    try:
        return CaseSpec.model_validate(raw)
    except ValidationError as error:
        raise CaseLoadError(f"{path}: {error}") from error


def load_cases(directory: Path | None = None) -> list[CaseSpec]:
    """Load every case under the directory, sorted by id; duplicate ids are
    an error (silently shadowed cases are how suites rot)."""
    base = directory or CASES_DIR
    specs: list[CaseSpec] = []
    for path in sorted(base.rglob("*.yaml")):
        specs.append(load_case_file(path))
    seen: dict[str, int] = {}
    for spec in specs:
        seen[spec.id] = seen.get(spec.id, 0) + 1
    duplicates = [case_id for case_id, count in seen.items() if count > 1]
    if duplicates:
        raise CaseLoadError(f"duplicate case ids: {duplicates}")
    return sorted(specs, key=lambda spec: spec.id)
