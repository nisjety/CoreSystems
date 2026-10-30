#!/usr/bin/env python3
"""Parity gate: every difference from v1 is either explained by the allow-list, or the gate fails.

Runs the four comparison tools with --json into --out (or reads the findings JSON already there
with --reuse), matches every finding against parity_allowlist.json, and prints the findings no
entry explains, grouped by table. For each such table it says whether docs/dataform_validation.md
names it, and under which heading. Allow-list entries that matched nothing are warnings only.

Exit codes: 0 every finding is matched; 1 at least one finding is not; 2 the gate could not run
(bad allow-list, a tool failed, findings JSON missing).

The tools read v1 (prj-dw-dev) and v2 (aquatiq-datavarehus) with SELECT only. Their markdown
reports go to --out, never to docs/. Stdlib only, so it runs under any Python.

Usage: dataform/tools/parity_gate.py [--allowlist PATH] [--reuse] [--out DIR]
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, datetime
from fnmatch import fnmatchcase
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
DOC = ROOT / "docs" / "dataform_validation.md"
DOC_REF = "docs/dataform_validation.md"
DEFAULT_ALLOWLIST = HERE / "parity_allowlist.json"
DEFAULT_OUT = Path.home() / ".cache" / "aquatiq-parity-gate"
TOOLS = ("compare_with_v1", "compare_columns", "compare_values", "compare_bronze_rows")
REQUIRED = ("id", "check", "table", "metric", "reason", "evidence")
OPTIONAL = ("diff_pct_range", "abs_diff_range")
FINDING_FIELDS = ("check", "dataset", "table", "metric", "v1", "v2", "diff_pct")
MIN_REASON = 40


class GateError(Exception):
    """The gate could not run. The caller exits with 2."""


def slugify(heading: str) -> str:
    """GitHub-style anchor: lower case, punctuation dropped, spaces turned into hyphens."""
    return re.sub(r"[^\w\- ]", "", heading.strip().lower()).replace(" ", "-")


def doc_headings() -> list[tuple[str, str]]:
    text = DOC.read_text()
    return [(m.group(2), slugify(m.group(2))) for m in re.finditer(r"^(#{1,6}) (.+)$", text, re.M)]


def check_range(entry_id: str, key: str, bounds: object) -> None:
    if not isinstance(bounds, list) or len(bounds) != 2:
        raise GateError(f"{entry_id}: {key} must be [low, high], either end may be null")
    for end in bounds:
        if end is not None and (isinstance(end, bool) or not isinstance(end, (int, float))):
            raise GateError(f"{entry_id}: {key} ends must be numbers or null")
    low, high = bounds
    if low is not None and high is not None and low > high:
        raise GateError(f"{entry_id}: {key} is empty (low > high)")


def load_allowlist(path: Path) -> list[dict]:
    try:
        data = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise GateError(f"cannot read allow-list {path}: {error}") from error
    entries = data.get("entries") if isinstance(data, dict) else None
    if not isinstance(entries, list) or not entries:
        raise GateError(f"{path} needs a non-empty 'entries' list")
    anchors = {anchor for _, anchor in doc_headings()}
    seen: set[str] = set()
    for entry in entries:
        if not isinstance(entry, dict):
            raise GateError("every allow-list entry must be an object")
        ident = str(entry.get("id", "<no id>"))
        unknown = set(entry) - set(REQUIRED) - set(OPTIONAL)
        if unknown:
            raise GateError(f"{ident}: unknown keys {sorted(unknown)}")
        missing = [key for key in REQUIRED if key not in entry]
        if missing:
            raise GateError(f"{ident}: missing keys {missing}")
        if ident in seen:
            raise GateError(f"duplicate id {ident}")
        seen.add(ident)
        if entry["check"] != "*" and entry["check"] not in TOOLS:
            raise GateError(f"{ident}: check must be '*' or one of {', '.join(TOOLS)}")
        for key in ("table", "metric"):
            if not isinstance(entry[key], str) or not entry[key]:
                raise GateError(f"{ident}: {key} must be a non-empty string")
        if not isinstance(entry["reason"], str) or len(entry["reason"]) < MIN_REASON:
            raise GateError(f"{ident}: reason must say why, at least {MIN_REASON} characters")
        evidence = entry["evidence"]
        if not isinstance(evidence, str) or not evidence.startswith(DOC_REF + "#"):
            raise GateError(f"{ident}: evidence must be {DOC_REF}#<heading anchor>")
        if evidence.split("#", 1)[1] not in anchors:
            raise GateError(
                f"{ident}: no heading in {DOC_REF} has anchor {evidence.split('#', 1)[1]}"
            )
        for key in OPTIONAL:
            if key in entry:
                check_range(ident, key, entry[key])
    return entries


def load_findings(tool: str, path: Path) -> list[dict]:
    try:
        items = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise GateError(f"cannot read findings of {tool} at {path}: {error}") from error
    if not isinstance(items, list):
        raise GateError(f"{path} must hold a JSON list")
    for item in items:
        absent = [key for key in FINDING_FIELDS if not isinstance(item, dict) or key not in item]
        if absent:
            raise GateError(f"{path}: a finding lacks {absent}")
    return items


def run_tools(out: Path) -> None:
    for tool in TOOLS:
        (out / f"{tool}.json").unlink(missing_ok=True)

    def run(tool: str) -> tuple[str, int, float]:
        cmd = [
            sys.executable,
            str(HERE / f"{tool}.py"),
            "--out",
            str(out),
            "--json",
            str(out / f"{tool}.json"),
        ]
        started = time.monotonic()
        with (out / f"{tool}.log").open("w") as log:
            code = subprocess.run(
                cmd, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=False
            ).returncode
        return tool, code, time.monotonic() - started

    print(f"running {len(TOOLS)} tools in parallel, logs in {out}", flush=True)
    with ThreadPoolExecutor(max_workers=len(TOOLS)) as pool:
        results = list(pool.map(run, TOOLS))
    for tool, code, seconds in results:
        print(f"  {tool}: exit {code} after {seconds:,.0f} s", flush=True)
    failed = [
        f"{tool} exit {code} (log: {out / (tool + '.log')})" for tool, code, _ in results if code
    ]
    if failed:
        raise GateError("tool(s) failed: " + "; ".join(failed))


def table_matches(pattern: str, item: dict) -> bool:
    """A pattern with a dot pins the dataset too: 'visma_silver.dim_currency'."""
    if "." in pattern:
        return fnmatchcase(f"{item['dataset']}.{item['table']}", pattern)
    return fnmatchcase(item["table"], pattern)


def in_range(value: float, bounds: list) -> bool:
    low, high = bounds
    return (low is None or value >= low) and (high is None or value <= high)


def covers(entry: dict, item: dict) -> bool:
    if entry["check"] not in ("*", item["check"]):
        return False
    if not table_matches(entry["table"], item) or not fnmatchcase(item["metric"], entry["metric"]):
        return False
    bounds = entry.get("diff_pct_range")
    if bounds is not None and (item["diff_pct"] is None or not in_range(item["diff_pct"], bounds)):
        return False
    bounds = entry.get("abs_diff_range")
    if bounds is not None:
        if item["v1"] is None or item["v2"] is None:
            return False
        if not in_range(item["v2"] - item["v1"], bounds):
            return False
    return True


def match_all(findings: list[dict], entries: list[dict]) -> tuple[list, list, set]:
    matched, unmatched, used = [], [], set()
    for item in findings:
        ids = [entry["id"] for entry in entries if covers(entry, item)]
        if ids:
            matched.append({"finding": item, "entries": ids})
            used.update(ids)
        else:
            unmatched.append(item)
    return matched, unmatched, used


def doc_sections_naming(table: str) -> list[str]:
    pattern = re.compile(rf"(?<!\w){re.escape(table)}(?!\w)")
    section, hits = "(before the first heading)", []
    for line in DOC.read_text().splitlines():
        heading = re.match(r"^#{1,6} (.+)$", line)
        if heading:
            section = heading.group(1)
        elif pattern.search(line) and section not in hits:
            hits.append(section)
    return hits


def fmt_value(value: object) -> str:
    if value is None:
        return "null"
    if isinstance(value, float):
        return f"{value:,.2f}"
    if isinstance(value, int):
        return f"{value:,}"
    return str(value)


def describe(item: dict) -> str:
    pct = "null" if item["diff_pct"] is None else f"{item['diff_pct']:+,.1f} %"
    note = f"  [{item['note']}]" if item.get("note") else ""
    return (
        f"    {item['check']:<19} {item['metric']:<38} v1={fmt_value(item['v1']):>18}"
        f"  v2={fmt_value(item['v2']):>18}  diff={pct:>11}{note}"
    )


def print_unmatched(unmatched: list[dict]) -> None:
    groups: dict[tuple[str, str], list[dict]] = {}
    for item in unmatched:
        groups.setdefault((item["dataset"], item["table"]), []).append(item)
    print(f"\nUNMATCHED: {len(unmatched)} finding(s) in {len(groups)} table(s), grouped by table")
    for (dataset, table), items in sorted(groups.items()):
        named = doc_sections_naming(table)
        where = (
            f"named in {DOC_REF} under: " + "; ".join(named) if named else f"not named in {DOC_REF}"
        )
        print(f"\n{dataset}.{table}  ({len(items)})  {where}")
        for item in items:
            print(describe(item))


def write_result(out: Path, args: argparse.Namespace, counts: dict, matched, unmatched, unused):
    digest = hashlib.sha256(args.allowlist.read_bytes()).hexdigest()
    result = {
        "generated_utc": datetime.now(UTC).isoformat(timespec="seconds"),
        "allowlist": str(args.allowlist),
        "allowlist_sha256": digest,
        "reused_findings": args.reuse,
        "findings_per_tool": counts,
        "matched": matched,
        "unmatched": unmatched,
        "unused_entries": unused,
    }
    path = out / "gate_result.json"
    path.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n")
    return path


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument(
        "--allowlist",
        type=Path,
        default=DEFAULT_ALLOWLIST,
        help="allow-list JSON (default: parity_allowlist.json next to this script)",
    )
    ap.add_argument(
        "--reuse",
        action="store_true",
        help="read the findings JSON already in --out instead of running the four tools",
    )
    ap.add_argument(
        "--out",
        type=Path,
        default=DEFAULT_OUT,
        help="directory for the tools' JSON, markdown and logs, and gate_result.json",
    )
    args = ap.parse_args(argv)
    try:
        entries = load_allowlist(args.allowlist)
        args.out.mkdir(parents=True, exist_ok=True)
        if args.reuse:
            print(f"reusing the findings JSON in {args.out}", flush=True)
        else:
            run_tools(args.out)
        by_tool = {tool: load_findings(tool, args.out / f"{tool}.json") for tool in TOOLS}
    except GateError as error:
        print(f"GATE COULD NOT RUN: {error}", file=sys.stderr)
        return 2

    findings = [item for tool in TOOLS for item in by_tool[tool]]
    matched, unmatched, used = match_all(findings, entries)
    counts = {tool: len(by_tool[tool]) for tool in TOOLS}
    unused = [entry for entry in entries if entry["id"] not in used]

    print(f"\nallow-list: {args.allowlist} ({len(entries)} entries)")
    print(f"findings: {len(findings)}  " + "  ".join(f"{t}={n}" for t, n in counts.items()))
    print(f"matched: {len(matched)}  unmatched: {len(unmatched)}")
    if unmatched:
        print_unmatched(unmatched)
    for entry in unused:
        print(
            f"WARNING: allow-list entry '{entry['id']}' matched no finding "
            f"({entry['check']} {entry['table']} {entry['metric']})"
        )
    path = write_result(args.out, args, counts, matched, unmatched, [e["id"] for e in unused])
    print(f"\nresult written to {path}")
    if unmatched:
        print(f"RESULT: FAIL, {len(unmatched)} of {len(findings)} findings are not explained")
        return 1
    print(f"RESULT: PASS, all {len(findings)} findings are explained by the allow-list")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
