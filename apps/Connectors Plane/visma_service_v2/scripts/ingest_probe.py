#!/usr/bin/env python3
"""Bounded live ingestion probe: sync endpoints twice into a throwaway local bronze.

For every endpoint it runs `vsv2 sync` two times and checks the properties a
DataMart cutover depends on, failing loudly instead of trusting exit codes:

  * the run succeeded AND fetched rows (an empty fetch proves nothing),
  * every fetched row landed in bronze (no silent drops),
  * pagination looks complete (all pages HTTP 200, last page short),
  * a second pass loses nothing and creates no duplicates,
  * the checkpoint exists and never moves backwards.

It only reads from Visma. The sink is Postgres; BigQuery is force-disabled and
the target database must be named `vsv2_ingest_*` so it cannot hit real state.

Usage:
  scripts/ingest_probe.py --db vsv2_ingest_20260929 \
      branches currencies accounts customer_invoices:14
      (name[:window_days]; a window adds --date-from/--date-to to both passes)

  scripts/ingest_probe.py --db vsv2_ingest_20260929 --crash-restart accounts
"""

from __future__ import annotations

import argparse
import csv
import json
import os
import signal
import subprocess
import sys
import time
from dataclasses import dataclass, field
from datetime import UTC, datetime, timedelta
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PG_CONTAINER = os.environ.get("VSV2_PG_CONTAINER", "visma_service_v2-postgres-1")
DEFAULT_TENANT = "33f319b9-4d57-11ee-960c-025417856183"
# Endpoints the API paginates and counts by nested *line*, not by returned row:
# its totalCount is then a line count, so completeness is summed over these arrays.
LINE_FIELDS = {"journal_transactions": "journalTransactionLines"}


def period_scanned(endpoint: str) -> bool:
    """Endpoints fetched one financial period at a time (config `period_scan`)."""
    cfg = json.loads((ROOT / "config/endpoints" / f"{endpoint}.json").read_text())
    return bool(cfg.get("period_scan"))


def tracks_modified_field(endpoint: str) -> bool:
    cfg = json.loads((ROOT / "config/endpoints" / f"{endpoint}.json").read_text())
    return bool(cfg.get("modified_field"))


def psql(db: str, sql: str) -> list[list[str]]:
    out = subprocess.run(
        [
            "docker",
            "exec",
            PG_CONTAINER,
            "psql",
            "-U",
            "visma_v2",
            "-d",
            db,
            "-At",
            "-F",
            "\x1f",
            "-c",
            sql,
        ],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return [line.split("\x1f") for line in out.splitlines() if line]


def vsv2_env(db: str) -> dict[str, str]:
    env = dict(os.environ)
    env.update(
        VSV2_DATABASE_URL=f"postgres://visma_v2:visma_v2@127.0.0.1:55433/{db}",
        VSV2_REDIS_URL="redis://127.0.0.1:56380/0",
        VSV2_BIGQUERY_PROJECT="",
        VSV2_BIGQUERY_DATASET_BRONZE="",
        VSV2_BIGQUERY_DATASET_SHADOW="",
        VSV2_LOCAL_BRONZE_POSTGRES_ENABLED="true",
        RUST_LOG="info",
    )
    return env


def sync_cmd(tenant: str, endpoint: str, window_days: int | None) -> list[str]:
    cmd = [str(ROOT / "target/debug/vsv2"), "sync", "--tenant", tenant, "--endpoint", endpoint]
    if window_days:
        now = datetime.now(UTC).replace(microsecond=0)
        cmd += [
            "--date-from",
            (now - timedelta(days=window_days)).isoformat().replace("+00:00", "Z"),
            "--date-to",
            now.isoformat().replace("+00:00", "Z"),
        ]
    return cmd


@dataclass
class Pass:
    exit_code: int
    run_id: str = ""
    status: str = ""
    records_in: int = 0
    records_written: int = 0
    error: str = ""
    pages: int = 0
    non_200: int = 0
    page_records: int = 0
    last_page_records: int = 0
    page_size: int = 0
    total_count: int | None = None
    line_count: int | None = None
    min_rate_remaining: int | None = None
    max_elapsed_ms: int = 0
    bronze_count: int = 0
    keys: frozenset[str] = frozenset()
    watermark: str = ""


@dataclass
class Report:
    endpoint: str
    window_days: int | None
    passes: list[Pass] = field(default_factory=list)
    failures: list[str] = field(default_factory=list)
    notes: list[str] = field(default_factory=list)

    def check(self, ok: bool, message: str) -> None:
        if not ok:
            self.failures.append(message)


def observe(db: str, tenant: str, endpoint: str, exit_code: int) -> Pass:
    p = Pass(exit_code=exit_code)
    runs = psql(
        db,
        f"SELECT run_id, status, records_in, records_written, coalesce(error,'') "
        f"FROM vsv2.sync_run WHERE tenant_id='{tenant}' AND endpoint='{endpoint}' "
        f"ORDER BY started_at DESC, created_at DESC LIMIT 1",
    )
    if runs:
        p.run_id, p.status = runs[0][0], runs[0][1]
        p.records_in, p.records_written, p.error = int(runs[0][2]), int(runs[0][3]), runs[0][4]
        pages = psql(
            db,
            f"SELECT status, records, coalesce(page_size,0), "
            f"coalesce(rate_limit_remaining,-1), elapsed_ms, coalesce(total_count,-1) "
            f"FROM vsv2.sync_page_metric "
            f"WHERE run_id='{p.run_id}' ORDER BY metric_id",
        )
        p.pages = len(pages)
        p.non_200 = sum(1 for r in pages if r[0] != "200")
        p.page_records = sum(int(r[1]) for r in pages)
        p.last_page_records = int(pages[-1][1]) if pages else 0
        p.page_size = int(pages[-1][2]) if pages else 0
        remaining = [int(r[3]) for r in pages if int(r[3]) >= 0]
        p.min_rate_remaining = min(remaining) if remaining else None
        p.max_elapsed_ms = max((int(r[4]) for r in pages), default=0)
        totals = [int(r[5]) for r in pages if int(r[5]) >= 0]
        p.total_count = max(totals) if totals else None
    keys = psql(
        db,
        f"SELECT record_key FROM vsv2.local_bronze_record "
        f"WHERE tenant_id='{tenant}' AND endpoint='{endpoint}'",
    )
    p.keys = frozenset(k[0] for k in keys)
    p.bronze_count = len(keys)
    if endpoint in LINE_FIELDS:
        lines = psql(
            db,
            f"SELECT coalesce(sum(jsonb_array_length(payload->'{LINE_FIELDS[endpoint]}')),0) "
            f"FROM vsv2.local_bronze_record WHERE tenant_id='{tenant}' AND endpoint='{endpoint}'",
        )
        p.line_count = int(lines[0][0])
    cp = psql(
        db,
        f"SELECT coalesce(max(coalesce(last_modified_watermark, last_updated_in_dm))::text,'') "
        f"FROM vsv2.sync_checkpoint "
        f"WHERE tenant_id='{tenant}' AND endpoint='{endpoint}'",
    )
    p.watermark = cp[0][0] if cp else ""
    return p


DEFAULT_SYNC_TIMEOUT_SECS = 1200
sync_timeout_secs = DEFAULT_SYNC_TIMEOUT_SECS  # set from --sync-timeout in main()


def run_sync(db: str, tenant: str, endpoint: str, window_days: int | None, log: Path) -> int:
    with log.open("ab") as fh:
        try:
            return subprocess.run(
                sync_cmd(tenant, endpoint, window_days),
                cwd=ROOT,
                env=vsv2_env(db),
                stdout=fh,
                stderr=subprocess.STDOUT,
                timeout=sync_timeout_secs,
            ).returncode
        except subprocess.TimeoutExpired:
            fh.write(f"\nprobe: killed after {sync_timeout_secs}s timeout\n".encode())
            return 124


def probe_endpoint(
    db: str, tenant: str, spec: str, out: Path, allow_empty: bool, passes: int = 2
) -> Report:
    name, _, win = spec.partition(":")
    window = int(win) if win else None
    rep = Report(name, window)
    log = out / f"{name}.log"
    for index in range(passes):
        # Only pass 1 gets the window. An explicit --date-from is an exact range and overrides
        # the checkpoint, so pass 2 runs without it to test what scheduled runs do: resume from
        # the checkpoint pass 1 left behind.
        pass_window = window if index == 0 else None
        rep.passes.append(observe(db, tenant, name, run_sync(db, tenant, name, pass_window, log)))
        if rep.passes[-1].exit_code != 0:
            break
    first = rep.passes[0]
    rep.check(
        first.exit_code == 0 and first.status == "succeeded",
        f"pass 1 did not succeed (exit={first.exit_code}, status={first.status!r}, "
        f"error={first.error[:120]!r})",
    )
    if first.status == "succeeded":
        if first.records_in == 0:
            (rep.notes if allow_empty else rep.failures).append(
                "pass 1 fetched 0 rows - nothing was proven "
                "(widen the window or pick another endpoint)"
            )
        rep.check(
            first.bronze_count == first.records_in,
            f"rows lost between fetch and bronze: fetched {first.records_in}, "
            f"bronze holds {first.bronze_count}",
        )
        rep.check(
            first.records_written == first.records_in,
            f"records_written {first.records_written} != records_in {first.records_in}",
        )
        rep.check(first.non_200 == 0, f"{first.non_200} page(s) were not HTTP 200")
        if name in LINE_FIELDS and period_scanned(name):
            rep.notes.append(
                f"fetched one financial period at a time; {first.line_count} lines in bronze, each "
                "period's walk checked against its own API totalCount (in lines) by the walk guard"
            )
        elif name in LINE_FIELDS:
            # totalCount and pageSize are in lines; rows are batches merged across pages.
            if first.total_count is not None and first.line_count is not None:
                rep.check(
                    first.line_count >= first.total_count,
                    f"API reports {first.total_count} lines but bronze holds {first.line_count}: "
                    f"pagination stopped after {first.pages} page(s)",
                )
                rep.notes.append(
                    f"completeness proven by lines: {first.line_count} of "
                    f"API totalCount={first.total_count}"
                )
        else:
            rep.check(
                first.page_records == first.records_in,
                f"page metrics sum to {first.page_records} rows but run reports {first.records_in}",
            )
            if period_scanned(name):
                # One walk per month: the page metrics hold many totalCounts, so the largest of
                # them proves nothing about the whole. The walk guard checks each month's walk
                # against that month's own total.
                rep.notes.append(
                    "fetched one financial period at a time; each period's walk is checked "
                    "against its own API totalCount by the walk guard"
                )
            elif first.total_count is not None:
                rep.check(
                    first.page_records >= first.total_count,
                    f"API reports totalCount={first.total_count} but only "
                    f"{first.page_records} rows were fetched",
                )
                rep.notes.append(f"completeness proven by API totalCount={first.total_count}")
            elif first.pages and first.page_size:
                rep.check(
                    first.last_page_records < first.page_size or first.records_in == 0,
                    f"last page holds {first.last_page_records} of {first.page_size} rows and "
                    "the API gave no totalCount: pagination may have stopped early",
                )
        if tracks_modified_field(name):
            rep.check(
                first.watermark != "" or first.records_in == 0,
                "endpoint has a modified_field but no checkpoint watermark was stored",
            )
        else:
            rep.notes.append(
                "no modified_field: every sync is a full re-fetch (no incremental watermark)"
            )
    if len(rep.passes) == 2:
        second = rep.passes[1]
        rep.check(
            second.status == "succeeded",
            f"pass 2 did not succeed (status={second.status!r}, error={second.error[:120]!r})",
        )
        rep.check(
            first.keys <= second.keys,
            f"pass 2 lost {len(first.keys - second.keys)} key(s) present after pass 1",
        )
        grown = len(second.keys - first.keys)
        if grown:
            rep.notes.append(f"{grown} new key(s) appeared between passes (upstream changed)")
        rep.check(
            second.watermark >= first.watermark,
            f"checkpoint moved backwards: {first.watermark} -> {second.watermark}",
        )
    return rep


def crash_restart(db: str, tenant: str, endpoint: str, out: Path, kill_after: float) -> Report:
    """Kill the sync mid-flight, then prove a restart converges with no duplicates."""
    rep = Report(f"{endpoint} (crash-restart)", None)
    log = out / f"{endpoint}.crash.log"
    with log.open("ab") as fh:
        proc = subprocess.Popen(
            sync_cmd(tenant, endpoint, None),
            cwd=ROOT,
            env=vsv2_env(db),
            stdout=fh,
            stderr=subprocess.STDOUT,
        )
        time.sleep(kill_after)
        if proc.poll() is None:
            proc.send_signal(signal.SIGKILL)
            killed = True
        else:
            killed = False
        proc.wait()
    crashed = observe(db, tenant, endpoint, proc.returncode)
    rep.passes.append(crashed)
    if not killed:
        rep.notes.append(
            f"process finished before the {kill_after}s kill; crash was NOT exercised "
            "(use a larger endpoint or a shorter delay)"
        )
    else:
        rep.check(
            crashed.bronze_count == 0 and crashed.watermark == "",
            f"killed run left state behind: bronze={crashed.bronze_count}, "
            f"watermark={crashed.watermark!r}",
        )
        if crashed.status == "running":
            rep.notes.append("killed run stays 'running' in sync_run forever (no stale-run reaper)")
    recovered = observe(db, tenant, endpoint, run_sync(db, tenant, endpoint, None, log))
    rep.passes.append(recovered)
    rep.check(
        recovered.status == "succeeded" and recovered.records_in > 0,
        f"restart did not succeed with rows (status={recovered.status!r}, "
        f"rows={recovered.records_in})",
    )
    rep.check(
        recovered.bronze_count == recovered.records_in,
        f"after restart bronze holds {recovered.bronze_count} rows, fetched {recovered.records_in}",
    )
    return rep


def write_evidence(out: Path, reports: list[Report]) -> None:
    with (out / "summary.csv").open("w", newline="") as fh:
        w = csv.writer(fh)
        w.writerow(
            [
                "endpoint",
                "window_days",
                "pass",
                "exit",
                "status",
                "records_in",
                "written",
                "bronze",
                "pages",
                "non_200",
                "last_page",
                "min_rate_remaining",
                "max_page_ms",
                "watermark",
            ]
        )
        for r in reports:
            for i, p in enumerate(r.passes, 1):
                w.writerow(
                    [
                        r.endpoint,
                        r.window_days or "",
                        i,
                        p.exit_code,
                        p.status,
                        p.records_in,
                        p.records_written,
                        p.bronze_count,
                        p.pages,
                        p.non_200,
                        p.last_page_records,
                        p.min_rate_remaining,
                        p.max_elapsed_ms,
                        p.watermark,
                    ]
                )
    lines = [f"# Ingestion probe {out.name}", ""]
    for r in reports:
        verdict = "FAIL" if r.failures else "PASS"
        lines.append(f"## {verdict} - {r.endpoint}")
        lines += [f"- FAIL: {m}" for m in r.failures] + [f"- note: {m}" for m in r.notes]
        for i, p in enumerate(r.passes, 1):
            lines.append(
                f"- pass {i}: {p.status or 'n/a'} in={p.records_in} bronze={p.bronze_count} "
                f"pages={p.pages} last_page={p.last_page_records}/{p.page_size} "
                f"rate_remaining>={p.min_rate_remaining} slowest_page={p.max_elapsed_ms}ms"
            )
        lines.append("")
    (out / "report.md").write_text("\n".join(lines))


def sweep_specs(
    include_partitioned: bool, window_days: int | None, only: set[str] | None = None
) -> tuple[list[str], list[str]]:
    """Every enabled non-DataMart endpoint as `name[:window]`; returns (specs, skipped)."""
    specs, skipped = [], []
    for path in sorted((ROOT / "config/endpoints").glob("*.json")):
        if path.name.startswith("_"):
            continue
        cfg = json.loads(path.read_text())
        name = cfg["endpoint_name"]
        if only is not None and name not in only:
            continue
        if not cfg.get("enabled", True):
            skipped.append(f"{name} (disabled)")
        elif cfg.get("partitioning") and not include_partitioned:
            skipped.append(f"{name} (partitioned)")
        else:
            windowed = window_days and cfg.get("modified_field")
            specs.append(f"{name}:{window_days}" if windowed else name)
    return specs, skipped


def last_rate_remaining(reports: list[Report]) -> int | None:
    seen = [p.min_rate_remaining for r in reports for p in r.passes if p.min_rate_remaining]
    return seen[-1] if seen else None


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("endpoints", nargs="*", help="name[:window_days]")
    ap.add_argument("--db", required=True, help="throwaway database, must start with vsv2_ingest_")
    ap.add_argument("--tenant", default=DEFAULT_TENANT)
    ap.add_argument("--allow-empty", action="store_true", help="do not fail on 0 fetched rows")
    ap.add_argument("--crash-restart", metavar="ENDPOINT")
    ap.add_argument("--kill-after", type=float, default=1.5)
    ap.add_argument("--all", action="store_true", help="sweep every enabled endpoint, one pass")
    ap.add_argument("--window-days", type=int, default=2, help="--all: window where modified_field")
    ap.add_argument("--include-partitioned", action="store_true")
    ap.add_argument("--no-window", action="store_true", help="--all: full fetch, no date window")
    ap.add_argument("--only", help="--all: comma-separated endpoint names to include")
    ap.add_argument("--min-rate-remaining", type=int, default=6000, help="--all: stop below this")
    ap.add_argument(
        "--sync-timeout",
        type=int,
        default=DEFAULT_SYNC_TIMEOUT_SECS,
        help="seconds one sync may run before it is killed (large tables need hours: the local "
        "bronze insert is row by row, ~600 rows/s)",
    )
    args = ap.parse_args()
    global sync_timeout_secs
    sync_timeout_secs = args.sync_timeout

    if not args.db.startswith("vsv2_ingest_"):
        sys.exit(f"refusing to run against '{args.db}': database must be named vsv2_ingest_*")
    if not (ROOT / "target/debug/vsv2").exists():
        sys.exit("build first: cargo build --bin vsv2")

    out = ROOT / "docs/evidence" / f"ingest_probe_{datetime.now(UTC):%Y%m%d_%H%M%S}"
    out.mkdir(parents=True)
    reports: list[Report] = []
    if args.all:
        only = set(args.only.split(",")) if args.only else None
        specs, skipped = sweep_specs(
            args.include_partitioned, None if args.no_window else args.window_days, only
        )
        print(
            f"sweep: {len(specs)} endpoints, skipped {len(skipped)}: {', '.join(skipped)}",
            flush=True,
        )
        for spec in specs:
            remaining = last_rate_remaining(reports)
            if remaining is not None and remaining < args.min_rate_remaining:
                print(
                    f"sweep: stopping, rate limit remaining {remaining} < {args.min_rate_remaining}"
                )
                break
            reports.append(probe_endpoint(args.db, args.tenant, spec, out, True, passes=1))
            write_evidence(out, reports)
            verdict = "FAIL" if reports[-1].failures else "ok"
            p = reports[-1].passes[0]
            print(f"  [{verdict:4}] {spec:40} in={p.records_in} pages={p.pages}", flush=True)
    reports += [
        probe_endpoint(args.db, args.tenant, spec, out, args.allow_empty) for spec in args.endpoints
    ]
    if args.crash_restart:
        reports.append(
            crash_restart(args.db, args.tenant, args.crash_restart, out, args.kill_after)
        )
    write_evidence(out, reports)

    print((out / "report.md").read_text())
    print(f"evidence: {out.relative_to(ROOT)}")
    return 1 if any(r.failures for r in reports) else 0


if __name__ == "__main__":
    raise SystemExit(main())
