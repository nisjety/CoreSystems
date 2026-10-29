"""Scored markdown report writer (docs/eval-reports/<date>.md)."""

from __future__ import annotations

from datetime import datetime, timezone
from pathlib import Path

from eval_lab.types import CaseResult


def render_report(results: list[CaseResult], *, when: datetime | None = None) -> str:
    when = when or datetime.now(timezone.utc)
    ran = [r for r in results if not r.skipped]
    passed = [r for r in ran if r.passed]
    skipped = [r for r in results if r.skipped]

    lines = [
        f"# Eval report — {when:%Y-%m-%d %H:%M} UTC",
        "",
        f"**{len(passed)}/{len(ran)} passed**, {len(skipped)} skipped "
        f"({len(results)} total cases)",
        "",
        "| Case | Result | accuracy | groundedness | cost | loop |",
        "|---|---|---|---|---|---|",
    ]
    for result in results:
        if result.skipped:
            lines.append(
                f"| {result.case_id} | ⏭ SKIP | — | — | — | — |"
            )
            continue
        cells = {m.metric: m for m in result.metrics}

        def cell(name: str) -> str:
            metric = cells.get(name)
            if metric is None or metric.skipped:
                return "—"
            return f"{'✅' if metric.passed else '❌'} {metric.score:.2f}"

        verdict = "✅ PASS" if result.passed else "❌ FAIL"
        lines.append(
            f"| {result.case_id} | {verdict} | {cell('accuracy')} | "
            f"{cell('groundedness')} | {cell('cost')} | {cell('loop_health')} |"
        )

    lines.append("")
    for result in results:
        if result.skipped:
            lines.append(f"- **{result.case_id}**: skipped — {result.skip_reason}")
            continue
        for metric in result.metrics:
            if not metric.passed and not metric.skipped:
                lines.append(f"- **{result.case_id}/{metric.metric}**: {metric.detail}")
    lines.append("")
    return "\n".join(lines)


def write_report(results: list[CaseResult], reports_dir: Path) -> Path:
    reports_dir.mkdir(parents=True, exist_ok=True)
    when = datetime.now(timezone.utc)
    path = reports_dir / f"{when:%Y-%m-%d}.md"
    path.write_text(render_report(results, when=when), encoding="utf-8")
    return path
