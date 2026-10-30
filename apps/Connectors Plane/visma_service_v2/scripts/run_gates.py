#!/usr/bin/env python3
"""One runner for the five v2 gates. It reads each gate's verdict and changes no gate rule.

gate-c  every assertion file in dataform/definitions/assertions/ must exist as a view in
        aquatiq-datavarehus.visma_assertions and return zero rows (read-only BigQuery).
gate-b  scripts/verify_population_vs_visma.py --source v2 must exit 0. It makes live Visma GET
        calls for every tenant (no --tenants filter) and writes
        docs/gates/population_v2_<date>.md/.json.
gate-s  scripts/verify_sales_order_counts.py must exit 0: per tenant, silver holds as many sales
        orders as Visma reports, less at most those changed since the last sync.
gate-e  scripts/verify_endpoint_counts.py must exit 0: for every ERP REST endpoint that Visma can
        count and every tenant, staging holds as many records as Visma reports, less at most
        those modified since the last write. It writes docs/gates/endpoint_counts_<date>.md/.json.
gate-a  dataform/tools/parity_gate.py must exit 0. Its tools read v1 (prj-dw-dev) and v2 with
        SELECT.

A missing gate file fails that gate. All five gates run even when one fails. The exit code is 0
only when all five pass. docs/gates/RESULTS.md is written on every run.

Usage: ROOT/.venv/bin/python scripts/run_gates.py [--log-dir DIR]
  --log-dir DIR  keep each gate's full output in DIR and send gate-a's --out there. Without it,
                 gate-a uses its own default output directory and the other output is read for
                 the summary and then dropped.
"""

from __future__ import annotations

import argparse
import json
import re
import shlex
import subprocess
import tempfile
import time
from collections import Counter
from collections.abc import Callable
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PYTHON = ROOT / ".venv" / "bin" / "python"
GATES_DIR = ROOT / "docs" / "gates"
RESULTS = GATES_DIR / "RESULTS.md"
ASSERTIONS = ROOT / "dataform" / "definitions" / "assertions"
POPULATION = ROOT / "scripts" / "verify_population_vs_visma.py"
SALES_COUNTS = ROOT / "scripts" / "verify_sales_order_counts.py"
ENDPOINT_COUNTS = ROOT / "scripts" / "verify_endpoint_counts.py"
PARITY = ROOT / "dataform" / "tools" / "parity_gate.py"
PROJECT = "aquatiq-datavarehus"
DATASET = "visma_assertions"
LOCATION = "europe-north2"
GATE_B_TIMEOUT_S = 3 * 60 * 60
GATE_S_TIMEOUT_S = 30 * 60
GATE_E_TIMEOUT_S = 60 * 60
WROTE = re.compile(r"^wrote (.+) \(exit (\d+)\)$", re.M)
GATE_A_RESULT = re.compile(r"^RESULT: (PASS|FAIL), (.+)$", re.M)
GATE_A_WARNING = re.compile(r"^WARNING: allow-list entry", re.M)
VERDICTS = ("PASS", "PENDING", "FAIL", "ERROR")

TITLE = {
    "gate-c": "gate-c: Dataform assertions in BigQuery",
    "gate-b": "gate-b: population against Visma (v2)",
    "gate-s": "gate-s: sales orders per company against Visma",
    "gate-e": "gate-e: endpoint counts against Visma (staging)",
    "gate-a": "gate-a: parity with v1",
}

PROVES = {
    "gate-c": (
        "The assertion views in dataform/definitions/assertions/ exist in "
        "aquatiq-datavarehus.visma_assertions and return zero violating rows: every GL batch and "
        "every company period nets to zero within 0.005, journal headers are unique, and every "
        "posted journal header matches the sum of its lines."
    ),
    "gate-b": (
        "For each of the 12 tenants and both invoice endpoints (customer, supplier), Visma's live "
        "open list agrees with the warehouse's open population by key. Each difference is PENDING "
        "(warehouse behind Visma) or DEFECT (the warehouse should have seen it at its last sync). "
        "A tenant whose last sync is older than the max age is FAIL."
    ),
    "gate-s": (
        "For each of the 12 tenants, silver holds as many sales orders as Visma's SalesOrder list "
        "reports, less at most the orders created or changed in Visma since the last sync. On the "
        "old silver rule (one order per number) it fails: Aquatiq AS 770 missing, Hygiene Systems "
        "404, Food Factory 2."
    ),
    "gate-e": (
        "For each of the 12 tenants and each ERP REST endpoint that reports a total (63 of 96: the "
        "finance lists, the Sales Order lists and general ledger transactions), the live rows in "
        "dw_staging_visma hold as many records as Visma reports, less at most those Visma shows "
        "as modified since the last write. Where Visma ignores an endpoint's modified filter the "
        "tolerance is 0. A gap beyond the tolerance fails: 650 invoices or 10,000 GL lines taken "
        "off the pilot's count do."
    ),
    "gate-a": (
        "Each difference between v1 (prj-dw-dev) and v2 (aquatiq-datavarehus) in row counts, "
        "column "
        "sets, values and bronze rows is matched by an allow-list entry that cites a heading in "
        "docs/dataform_validation.md. An unmatched difference fails the gate."
    ),
}

CANNOT_SEE = {
    "gate-c": (
        "Whether the deployed view SQL is the repo SQL (only existence and row count are read). "
        "Whether Dataform runs these on a schedule. Why Visma's own header totals disagree with "
        "its lines in the 49 journal batches. The detail of any violation (only a count). The "
        "uniqueKey assertions declared on models (e.g. fact_sales_order): the Dataform run checks "
        "those, this gate does not."
    ),
    "gate-b": (
        "Freshness comes from the heartbeat table, which stays empty until a scheduled sync writes "
        "it, so every tenant is stale until then. Visma-only documents are not confirmed by key. "
        "The 401, 429 and 5xx retry paths were not exercised."
    ),
    "gate-s": (
        "Order types: the list API cannot filter by type, so a wrong type, or one missing order "
        "offset by one extra, passes. Orders deleted in Visma that silver still holds are a WARN, "
        "not a FAIL. Status, amounts and lines are not compared. Until heartbeats exist, the last "
        "staging write stands in for the last sync."
    ),
    "gate-e": (
        "33 of the 96 ERP endpoints cannot be counted by a total (NOT COUNTABLE): journals, GL "
        "balances and project transactions (per period); budgets, time cards, project tasks and "
        "customer locations (parent key); cash transactions, balances, expense receipts and 21 "
        "more with no total. A gap no larger than the modified-since count passes, and with "
        "staging last written 2026-10-01/02 that count is wide (pilot customer documents: 674). "
        "A missing record offset by an extra one passes. Values and fields are not compared. "
        "Until heartbeats exist, the last staging write stands in for the last sync."
    ),
    "gate-a": (
        "Whether each explanation is true. Several entries cite bounds or sections that "
        "docs/dataform_validation.md does not state (verifier, 2026-10-10). Bronze JSON entries "
        "accept any size of change. Values inside explained differences. v1 is the reference "
        "here, not a correct answer."
    ),
}

BUILDER_STATE = {
    "gate-c": (
        "Deployed to the Dev workspace (deploy.py now sends the schema). The GL batch, company "
        "period and header-unique views return 0 rows. The journal control-totals view returns 49 "
        "batches whose Visma header totals disagree with Visma's own lines; no v2 line loss was "
        "found in the 39 period walks checked."
    ),
    "gate-b": (
        "Pacing (0.3 s after each response, shared across tenants), complete verdicts and the sync "
        "heartbeat were added 2026-10-10. It fails as stale until a scheduled sync writes "
        "heartbeats."
    ),
    "gate-s": (
        "Added 2026-10-10 with the silver sales-order grain fix. Rule tested offline (4 tests); "
        "live run: 12 of 12 tenants PASS; negative control on the old rule: 3 tenants FAIL."
    ),
    "gate-e": (
        "Added 2026-10-12. Rule and request logic tested offline (76 tests; 13 mutations of the "
        "logic each fail one). Live run on all 12 tenants: 913 Visma GETs in 14 min, 756 cells: "
        "PASS 752 (656 exact), WARN 1, FAIL 3. The 3 FAILs are sales_order_customers, whose "
        "modified filter Visma ignores; by key they are customers created, re-activated or "
        "inactivated after the fetch, so no fetch gap was found. Exits 1 until staging is "
        "refreshed or the tolerance for that endpoint is settled."
    ),
    "gate-a": (
        "Tolerances moved to the doc's bounds and a like-for-like cutoff added. The last run, "
        "before the sales-order fix, left 505 findings unexplained, and several entries cite "
        "bounds the doc does not state. Its 399 v1-only sales-order lines are explained: 83 "
        "orders Visma no longer "
        "has (v1 stale) and 7 restored by the silver fix."
    ),
}

NOT_COVERED = (
    "open-set (the open-invoice population check in vsv2): not part of this runner. Its builder is "
    "blocked, so there is no gate to run.",
    "Dataform deploy, releases and schedules: this runner deploys, compiles and runs nothing in "
    "Dataform itself. gate-a runs `dataform compile` through its tools. gate-c reads only the "
    "views "
    "that exist now.",
    "The gate rules: the runner reads each gate's verdict and changes none of them.",
    "Anything the gate scripts do not check. Each gate's own 'cannot see' list applies on top of "
    "this one.",
    "History: RESULTS.md is overwritten on each run. Nothing compares runs over time.",
)


@dataclass(frozen=True)
class GateRun:
    gate: str
    passed: bool
    summary: str  # the one line printed for this gate
    numbers: str  # markdown for the key-numbers section of RESULTS.md
    commands: tuple[str, ...]


@dataclass(frozen=True)
class Child:
    code: int | None  # None: the timeout expired and the process was killed
    output: str
    seconds: float


@dataclass(frozen=True)
class ViewCheck:
    name: str
    state: str  # pass | missing | not a view | has rows | schema mismatch | error
    rows: int | None
    detail: str = ""

    def describe(self) -> str:
        if self.state == "pass":
            return "exists as a view, 0 rows"
        if self.state == "has rows":
            return f"exists as a view, {self.rows:,} rows"
        if self.state == "missing":
            return "missing"
        return f"{self.state} ({self.detail})"


def verdict_word(passed: bool) -> str:
    return "PASS" if passed else "FAIL"


def relative(path: Path) -> str:
    try:
        return str(path.relative_to(ROOT))
    except ValueError:
        return str(path)


def cell(text: str) -> str:
    return text.replace("|", "/").replace("\n", " ").strip()


def md_table(header: list[str], rows: list[list[str]]) -> str:
    lines = ["| " + " | ".join(cell(h) for h in header) + " |", "|" + "---|" * len(header)]
    lines += ["| " + " | ".join(cell(c) for c in row) + " |" for row in rows]
    return "\n".join(lines)


def first_line(prefix: str, output: str) -> str:
    for line in output.splitlines():
        if line.startswith(prefix):
            return line
    return f"(no line starting with '{prefix}' in the output)"


def fail_run(gate: str, reason: str, commands: tuple[str, ...]) -> GateRun:
    return GateRun(gate, False, f"{gate} FAIL: {reason}", f"Not measured: {reason}.", commands)


def guarded(gate: str, step: Callable[[], GateRun]) -> GateRun:
    """An error inside a gate fails that gate and its text is kept. It never passes silently."""
    try:
        return step()
    except Exception as error:
        return fail_run(gate, f"runner error {type(error).__name__}: {error}", ())


def run_child(name: str, cmd: list[str], log_dir: Path | None, timeout: float | None) -> Child:
    """Run one gate tool from the repo root. Its output goes to log_dir/<name>.log when given."""
    started = time.monotonic()
    if log_dir is None:
        # closed by the `with sink:` block below
        sink = tempfile.TemporaryFile("w+", encoding="utf-8", errors="replace")  # noqa: SIM115
    else:
        sink = (log_dir / f"{name}.log").open("w+", encoding="utf-8", errors="replace")
    with sink:
        try:
            code: int | None = subprocess.run(
                cmd,
                cwd=ROOT,
                stdout=sink,
                stderr=subprocess.STDOUT,
                timeout=timeout,
                check=False,
            ).returncode
        except subprocess.TimeoutExpired:
            code = None
        sink.seek(0)
        output = sink.read()
    return Child(code, output, time.monotonic() - started)


# gate-c: Dataform assertion views in BigQuery


def assertion_files() -> list[tuple[str, str | None]]:
    """(view name, declared schema) per assertion file, read from its config block.

    The name is the config's name, else the file stem, as Dataform defaults it.
    """
    found: list[tuple[str, str | None]] = []
    for path in sorted(ASSERTIONS.glob("*.sqlx")):
        block = re.search(r"^config\s*\{(.*?)^\}", path.read_text(encoding="utf-8"), re.S | re.M)
        body = block.group(1) if block else ""
        name = re.search(r'\bname\s*:\s*"([^"]+)"', body)
        schema = re.search(r'\bschema\s*:\s*"([^"]+)"', body)
        found.append((name.group(1) if name else path.stem, schema.group(1) if schema else None))
    return found


def check_view(
    client: object, not_found: type[Exception], name: str, declared: str | None
) -> ViewCheck:
    if declared not in (None, DATASET):
        return ViewCheck(name, "schema mismatch", None, f"config declares schema {declared}")
    ref = f"{PROJECT}.{DATASET}.{name}"
    try:
        table = client.get_table(ref)  # type: ignore[attr-defined]
    except not_found:
        return ViewCheck(name, "missing", None)
    except Exception as error:
        return ViewCheck(name, "error", None, f"get_table {type(error).__name__}: {error}")
    if table.table_type != "VIEW":
        return ViewCheck(name, "not a view", None, f"table type {table.table_type}")
    try:
        query = f"SELECT COUNT(*) AS n FROM `{ref}`"
        job = client.query(query, location=LOCATION)  # type: ignore[attr-defined]
        rows = int(next(iter(job.result())).n)
    except Exception as error:
        return ViewCheck(name, "error", None, f"row count {type(error).__name__}: {error}")
    return ViewCheck(name, "pass" if rows == 0 else "has rows", rows)


def gate_c() -> GateRun:
    commands = (
        f"bigquery.Client(project={PROJECT}, location={LOCATION}): get_table and "
        "SELECT COUNT(*) on each assertion view",
    )
    try:
        from google.api_core.exceptions import NotFound
        from google.cloud import bigquery
    except ImportError as error:
        return fail_run("gate-c", f"BigQuery client not importable ({error})", commands)
    files = assertion_files()
    if not files:
        return fail_run("gate-c", f"no assertion files in {relative(ASSERTIONS)}", commands)
    client = bigquery.Client(project=PROJECT, location=LOCATION)
    checks = [check_view(client, NotFound, name, declared) for name, declared in files]
    passed = all(check.state == "pass" for check in checks)
    ok = sum(check.state == "pass" for check in checks)
    summary = (
        f"gate-c {verdict_word(passed)}: {ok} of {len(checks)} assertion views exist and return "
        "zero rows. " + "; ".join(f"{c.name}: {c.describe()}" for c in checks)
    )
    table = md_table(
        ["View", "State", "Rows"],
        [
            [f"`{DATASET}.{c.name}`", c.describe(), "-" if c.rows is None else f"{c.rows:,}"]
            for c in checks
        ],
    )
    numbers = (
        f"{len(files)} assertion files read from `{relative(ASSERTIONS)}`. Each is checked as "
        f"`{PROJECT}.{DATASET}.<name>`.\n\n{table}"
    )
    return GateRun("gate-c", passed, summary, numbers, commands)


# gate-b: population against Visma


def summarise_population(md_path: Path) -> tuple[str, Counter[str], int]:
    """Key numbers from the population report the script wrote.

    Returns (markdown, verdict counts, DEFECT rows).
    """
    report = json.loads(md_path.with_suffix(".json").read_text(encoding="utf-8"))
    tenants = report["tenants"]
    kinds: Counter[str] = Counter()
    rows: list[list[str]] = []
    pending_total = 0
    lookups = 0
    pages = 0
    for tenant in tenants:
        endpoints = list((tenant.get("endpoints") or {}).values())
        for ep in endpoints:
            kinds.update(ep["defects"])
            pending_total += sum(ep["pending"].values())
            lookups += int(ep.get("lookups", 0))
            pages += int(ep["visma"].get("pages", 0))
        ages = [
            ep["warehouse"]["age_hours"]
            for ep in endpoints
            if ep["warehouse"]["age_hours"] is not None
        ]
        errors = len(tenant["errors"]) + sum(len(ep["errors"]) for ep in endpoints)
        rows.append(
            [
                tenant["name"],
                tenant["verdict"],
                str(sum(sum(ep["defects"].values()) for ep in endpoints)),
                str(sum(sum(ep["pending"].values()) for ep in endpoints)),
                "yes" if any(ep["warehouse"]["stale"] for ep in endpoints) else "no",
                f"{max(ages):.1f}" if ages else "-",
                str(errors),
            ]
        )
    counts = Counter(t["verdict"] for t in tenants)
    kind_text = ", ".join(f"{k} {v}" for k, v in sorted(kinds.items())) or "none"
    head = [
        f"- Exit code {report['exit_code']}. Verdicts: "
        + ", ".join(f"{v} {counts.get(v, 0)}" for v in VERDICTS)
        + f" of {len(tenants)} tenants. Stale threshold {report['max_age_hours']} h.",
        f"- DEFECT rows by kind: {kind_text}. PENDING rows: {pending_total}.",
        f"- Recorded by the report: {lookups} by-key lookups and {pages} list pages.",
        "",
    ]
    table = md_table(
        ["Tenant", "Verdict", "DEFECT", "PENDING", "Stale", "Max age h", "Errors"], rows
    )
    return "\n".join(head) + table, counts, sum(kinds.values())


def gate_b(log_dir: Path | None) -> GateRun:
    cmd = [str(PYTHON), str(POPULATION), "--source", "v2"]
    commands = (shlex.join(cmd),)
    if not POPULATION.is_file():
        return fail_run("gate-b", f"{POPULATION.name} does not exist", commands)
    child = run_child("gate-b", cmd, log_dir, GATE_B_TIMEOUT_S)
    if child.code is None:
        return fail_run(
            "gate-b", f"killed after the {GATE_B_TIMEOUT_S // 3600} h timeout", commands
        )
    wrote = WROTE.search(child.output)
    if wrote is None:
        return fail_run(
            "gate-b", f"exit {child.code} and no report written (see the log)", commands
        )
    md_path = Path(wrote.group(1))
    passed = child.code == 0
    try:
        numbers, counts, defects = summarise_population(md_path)
        tally = ", ".join(f"{v} {counts.get(v, 0)}" for v in VERDICTS)
        detail = f"tenants {tally} (of {sum(counts.values())}); {defects} DEFECT row(s)"
    except Exception as error:
        numbers = (
            f"Report could not be summarised: {type(error).__name__}: {error}. "
            f"The verdict is the script's exit code."
        )
        detail = "summary unavailable"
    summary = (
        f"gate-b {verdict_word(passed)}: exit {child.code}; {detail}; report {relative(md_path)}"
    )
    return GateRun("gate-b", passed, summary, numbers, commands)


# gate-s: sales orders per company against Visma


def gate_s(log_dir: Path | None) -> GateRun:
    cmd = [str(PYTHON), str(SALES_COUNTS)]
    commands = (shlex.join(cmd),)
    if not SALES_COUNTS.is_file():
        return fail_run("gate-s", f"{SALES_COUNTS.name} does not exist", commands)
    child = run_child("gate-s", cmd, log_dir, GATE_S_TIMEOUT_S)
    if child.code is None:
        return fail_run(
            "gate-s", f"killed after the {GATE_S_TIMEOUT_S // 60} min timeout", commands
        )
    wrote = WROTE.search(child.output)
    if wrote is None:
        return fail_run(
            "gate-s", f"exit {child.code} and no report written (see the log)", commands
        )
    md_path = Path(wrote.group(1))
    passed = child.code == 0
    verdicts = Counter(re.findall(r"  (PASS|WARN|FAIL)$", child.output, re.M))
    detail = ", ".join(f"{v} {verdicts.get(v, 0)}" for v in ("PASS", "WARN", "FAIL"))
    report = md_path.read_text(encoding="utf-8").split("\n", 1)[-1] if md_path.is_file() else ""
    numbers = report.strip() or "The report is missing; the verdict is the script's exit code."
    summary = (
        f"gate-s {verdict_word(passed)}: exit {child.code}; tenants {detail}; "
        f"report {relative(md_path)}"
    )
    return GateRun("gate-s", passed, summary, numbers, commands)


# gate-e: endpoint counts against Visma

CELL_VERDICTS = ("PASS", "WARN", "FAIL", "ERROR")


def summarise_endpoint_counts(md_path: Path) -> tuple[str, Counter[str], int]:
    """Key numbers from the endpoint-count report the script wrote.

    Returns (markdown, cell verdict counts, endpoints that are NOT COUNTABLE).
    """
    report = json.loads(md_path.with_suffix(".json").read_text(encoding="utf-8"))
    endpoints = report["endpoints"]
    counts = Counter(c["verdict"] for ep in endpoints for c in ep["cells"])
    skipped = [ep["endpoint"] for ep in endpoints if ep["not_countable"]]
    counted = len(endpoints) - len(skipped)
    head = [
        "- Cells: "
        + ", ".join(f"{v} {counts.get(v, 0)}" for v in CELL_VERDICTS)
        + f" ({counted} endpoints counted for {len(report['tenants'])} tenants).",
        f"- Last write: {report['write_source']}.",
        f"- Visma calls: {report['calls']}.",
        f"- NOT COUNTABLE ({len(skipped)}): {', '.join(skipped) or 'none'}. The reason for each "
        f"is in `{relative(md_path)}`.",
        "",
    ]
    rows = [
        [
            ep["endpoint"],
            c["name"],
            "-" if c["visma_total"] is None else f"{c['visma_total']:,}",
            f"{c['staging_total']:,}",
            "-" if c["missing"] is None else f"{c['missing']:,}",
            "-" if c["changed"] is None else f"{c['changed']:,}",
            c["verdict"],
            c["reason"],
        ]
        for ep in endpoints
        for c in ep["cells"]
        if c["verdict"] in ("FAIL", "ERROR")
    ]
    table = (
        md_table(
            ["Endpoint", "Tenant", "Visma", "Staging", "Missing", "Changed", "Verdict", "Why"], rows
        )
        if rows
        else "No FAIL or ERROR cell."
    )
    return "\n".join(head) + table, counts, len(skipped)


def gate_e(log_dir: Path | None) -> GateRun:
    cmd = [str(PYTHON), str(ENDPOINT_COUNTS)]
    commands = (shlex.join(cmd),)
    if not ENDPOINT_COUNTS.is_file():
        return fail_run("gate-e", f"{ENDPOINT_COUNTS.name} does not exist", commands)
    child = run_child("gate-e", cmd, log_dir, GATE_E_TIMEOUT_S)
    if child.code is None:
        return fail_run(
            "gate-e", f"killed after the {GATE_E_TIMEOUT_S // 60} min timeout", commands
        )
    wrote = WROTE.search(child.output)
    if wrote is None:
        return fail_run(
            "gate-e", f"exit {child.code} and no report written (see the log)", commands
        )
    md_path = Path(wrote.group(1))
    passed = child.code == 0
    try:
        numbers, counts, skipped = summarise_endpoint_counts(md_path)
        tally = ", ".join(f"{v} {counts.get(v, 0)}" for v in CELL_VERDICTS)
        detail = f"cells {tally}; {skipped} endpoints not countable"
    except Exception as error:
        numbers = (
            f"Report could not be summarised: {type(error).__name__}: {error}. "
            f"The verdict is the script's exit code."
        )
        detail = "summary unavailable"
    summary = (
        f"gate-e {verdict_word(passed)}: exit {child.code}; {detail}; report {relative(md_path)}"
    )
    return GateRun("gate-e", passed, summary, numbers, commands)


# gate-a: parity with v1


def gate_a(log_dir: Path | None) -> GateRun:
    cmd = [str(PYTHON), str(PARITY)]
    result_json = "(the tool's default output directory, ~/.cache/aquatiq-parity-gate)"
    if log_dir is not None:
        cmd += ["--out", str(log_dir / "gate-a")]
        result_json = str(log_dir / "gate-a" / "gate_result.json")
    commands = (shlex.join(cmd),)
    if not PARITY.is_file():
        return fail_run("gate-a", f"{PARITY.name} does not exist", commands)
    child = run_child("gate-a", cmd, log_dir, None)
    passed = child.code == 0
    result = GATE_A_RESULT.search(child.output)
    if child.code == 2:
        detail = "the gate could not run (exit 2)"
    elif result is not None:
        detail = result.group(2)
    else:
        detail = "no RESULT line in the output (see the log)"
    summary = f"gate-a {verdict_word(passed)}: exit {child.code}; {detail}"
    warnings = len(GATE_A_WARNING.findall(child.output))
    numbers = "\n".join(
        [
            f"- Exit code {child.code}. {detail[0].upper() + detail[1:]}.",
            f"- `{first_line('findings: ', child.output)}`",
            f"- `{first_line('matched: ', child.output)}`",
            f"- `{first_line('UNMATCHED: ', child.output)}`"
            if "UNMATCHED:" in child.output
            else "- No unmatched findings printed.",
            f"- Allow-list entries that matched nothing (warnings, not failures): {warnings}.",
            f"- Full findings and matches: `{result_json}`.",
        ]
    )
    return GateRun("gate-a", passed, summary, numbers, commands)


# RESULTS.md


def render(runs: list[GateRun], started: datetime, passed: bool, log_dir: Path | None) -> str:
    local_date = started.astimezone().date().isoformat()
    lines = [
        "# v2 gates: results of one run",
        "",
        f"Run date {local_date} (local), started {started.isoformat(timespec='seconds')} UTC. "
        f"Overall: **{verdict_word(passed)}**, exit {0 if passed else 1}.",
        "",
        "Written by `scripts/run_gates.py`. Each gate keeps its own rules. This file records each "
        "gate's exit verdict and the numbers its own output reported.",
        "",
    ]
    if log_dir is not None:
        lines += [f"Full output of each gate: `{log_dir}`.", ""]
    lines += [
        md_table(
            [
                "Gate",
                "Status",
                "What it proves",
                "What it cannot see",
                "Run date",
                "Builder and verifier state (as handed to this run)",
            ],
            [
                [
                    TITLE[r.gate],
                    f"**{verdict_word(r.passed)}**",
                    PROVES[r.gate],
                    CANNOT_SEE[r.gate],
                    local_date,
                    BUILDER_STATE[r.gate],
                ]
                for r in runs
            ],
        ),
        "",
        "## Key numbers",
        "",
    ]
    for r in runs:
        lines += [f"### {r.gate}", "", r.numbers, ""]
    lines += ["## Commands run", ""]
    lines += [f"- `{c}`" for r in runs for c in r.commands]
    lines += ["", "## Not covered by this runner", ""]
    lines += [f"- {item}" for item in NOT_COVERED]
    return "\n".join(lines) + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "--log-dir", type=Path, default=None, help="keep each gate's full output in this directory"
    )
    args = parser.parse_args(argv)
    log_dir = args.log_dir.resolve() if args.log_dir else None
    if log_dir is not None:
        log_dir.mkdir(parents=True, exist_ok=True)
    started = datetime.now(UTC)
    steps: tuple[tuple[str, Callable[[], GateRun]], ...] = (
        ("gate-c", gate_c),
        ("gate-b", lambda: gate_b(log_dir)),
        ("gate-s", lambda: gate_s(log_dir)),
        ("gate-e", lambda: gate_e(log_dir)),
        ("gate-a", lambda: gate_a(log_dir)),
    )
    runs: list[GateRun] = []
    for gate, step in steps:
        run = guarded(gate, step)
        runs.append(run)
        print(run.summary, flush=True)
    passed = all(r.passed for r in runs)
    RESULTS.parent.mkdir(parents=True, exist_ok=True)
    RESULTS.write_text(render(runs, started, passed, log_dir), encoding="utf-8")
    print(
        f"overall {verdict_word(passed)}, exit {0 if passed else 1}; "
        f"results in {relative(RESULTS)}",
        flush=True,
    )
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
