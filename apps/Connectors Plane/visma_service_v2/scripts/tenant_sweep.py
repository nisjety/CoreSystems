#!/usr/bin/env python3
"""Run every endpoint config for the tenants other than the pilot.

What it is for: a key or shape assumption that holds on the pilot tenant can break on another one
(`ModulePM` project transactions repeat their refNbr month after month, and the pilot has 9 of
them). Running everything once per tenant makes the sync's own validation reject such rows, and the
summary lists them.

For each (tenant, endpoint) it runs `vsv2 sync --tenant T --endpoint E` against one scratch Postgres
database that holds the rows of all tenants. It records status, rows, time and the lowest ERP rate
limit remaining, and it is safe to stop and start again:

  * one worker per tenant, at most --workers tenants at a time, endpoints in a tenant one by one, so
    a tenant's own Visma quota (15,000 requests per hour and tenant, separate counters) is spent by
    one process at a time, capped by --erp-rpm;
  * endpoints the pilot showed to be heavy run at most --heavy-slots at a time across tenants;
  * when a tenant's remaining quota falls under --floor the worker waits for the window to reset;
  * a (tenant, endpoint) already in the database, or already recorded as ok/empty, is skipped;
  * `touch <out>/STOP` makes the workers finish their current endpoint and quit.

The DataMart company of each tenant comes from config/tenants/*.json, written to the sweep
database with `vsv2 tenants-register` before the first sync. The environment's
VSV2_AQUATIQ_DATAMART_COMPANY_ID belongs to the pilot tenant alone and is ignored for the others.

The database must NOT be named vsv2_ingest_*: the single-tenant scripts (reconcile_sources.py,
v1_v2_key_diff.py ...) read every such database and would mix the tenants.

--sink bigquery writes to BigQuery instead of the database's local bronze table: raw staging
(one JSON table per endpoint) in --bq-project/--bq-dataset, via `vsv2-bq-stage`. --db is then the
real state database (`visma_v2`): its checkpoints must describe what is in BigQuery, so it has to be
empty of checkpoints when a first load starts. What is already done is read from its run history
(`sync_run`), which makes a restart safe. --include-pilot adds the pilot tenant.

Usage:
  scripts/tenant_sweep.py --db vsv2_alltenants_20260930 [--tenants id,id] [--endpoints a,b]
                          [--workers 4] [--erp-rpm 120] [--dm-rpm 120] [--out DIR]
  scripts/tenant_sweep.py --report DIR [RETRYDIR ...]   (latest status per pair, later dirs win)

Running again with the same --out retries what failed: only ok/empty pairs are skipped.
With --retry-failed DIR (and another --out) only the pairs that failed in DIR run, so a second
process can retry them while the first is still sweeping other tenants.
"""

from __future__ import annotations

import argparse
import csv
import json
import os
import re
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path

from _bq_target import DATASET, PROJECT

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))

import reconcile_sources as rs  # noqa: E402
import tenant_scan_compare as tenants  # noqa: E402

BINARY = ROOT / "target/debug/vsv2"
PG_CONTAINER = rs.CONTAINER
ANSI = re.compile(r"\x1b\[[0-9;]*m")
REMAINING = re.compile(r"remaining=Some\((\d+)\).*?reset=Some\((\d+)\)")
FETCHED = re.compile(r"sync: fetched .*?records=(\d+)")
WRITTEN = re.compile(r"rows_written=(\d+)")
HEAVY_PAGES = 300  # pilot pages above which an endpoint counts as heavy
FIELDS = [
    "tenant",
    "name",
    "endpoint",
    "source",
    "status",
    "records",
    "written",
    "seconds",
    "min_remaining",
    "error",
    "finished",
]


@dataclass
class Result:
    tenant: str
    name: str
    endpoint: str
    source: str
    status: str
    records: int
    written: int
    seconds: int
    min_remaining: int | None
    error: str


def load_endpoints() -> dict[str, str]:
    found = {}
    for path in sorted((ROOT / "config/endpoints").glob("*.json")):
        if path.name.startswith("_"):
            continue
        cfg = json.loads(path.read_text())
        if cfg.get("enabled", True):
            found[cfg["endpoint_name"]] = cfg["source"]
    return found


def pilot_pages() -> dict[str, int]:
    """Pages per endpoint in the pilot's probe runs: the best guide to cost and order."""
    best: dict[str, int] = {}
    for path in (ROOT / "docs/evidence").glob("ingest_probe_*/summary.csv"):
        for row in csv.DictReader(path.open()):
            if row["status"] == "succeeded" and row["pages"].isdigit():
                best[row["endpoint"]] = max(best.get(row["endpoint"], 0), int(row["pages"]))
    return best


def register_tenants(env: dict[str, str]) -> None:
    """Write config/tenants/*.json to the sweep database's tenant_company_map.

    A DataMart sync reads the tenant's company there. `vsv2 sync` only takes the environment's
    company id for the pilot tenant, so a tenant that is not registered cannot be synced from
    DataMart at all.
    """
    done = subprocess.run(
        [str(BINARY), "tenants-register"], cwd=ROOT, env=env, capture_output=True, text=True
    )
    if done.returncode != 0:
        raise RuntimeError(f"vsv2 tenants-register failed: {ANSI.sub('', done.stdout)[-400:]}")


def existing_rows(db: str, sink: str = "postgres") -> set[tuple[str, str]]:
    """(tenant, endpoint) pairs already written: rows in local bronze, or a succeeded sync run."""
    if sink == "bigquery":
        sql = "select distinct tenant_id, endpoint from vsv2.sync_run where status = 'succeeded'"
    else:
        sql = "select distinct tenant_id, endpoint from vsv2.local_bronze_record"
    return {tuple(line.split(rs.SEP)) for line in rs.psql(db, sql)}  # type: ignore[misc]


def failed_pairs(sweep: Path) -> set[tuple[str, str]]:
    """(tenant, endpoint) whose latest run in a sweep directory did not end ok or empty."""
    latest: dict[tuple[str, str], str] = {}
    for row in csv.DictReader((sweep / "summary.csv").open()):
        latest[(row["tenant"], row["endpoint"])] = row["status"]
    return {pair for pair, status in latest.items() if status not in ("ok", "empty")}


def recorded(summary: Path) -> set[tuple[str, str]]:
    if not summary.exists():
        return set()
    return {
        (r["tenant"], r["endpoint"])
        for r in csv.DictReader(summary.open())
        if r["status"] in ("ok", "empty")
    }


class Sweep:
    def __init__(self, args: argparse.Namespace, endpoints: dict[str, str]) -> None:
        self.args = args
        self.out = Path(args.out)
        self.out.mkdir(parents=True, exist_ok=True)
        (self.out / "logs").mkdir(exist_ok=True)
        self.summary = self.out / "summary.csv"
        self.lock = threading.Lock()
        self.heavy = threading.Semaphore(args.heavy_slots)
        pages = pilot_pages()
        self.pages = pages
        self.order = sorted(endpoints, key=lambda e: (pages.get(e, 0), e))
        self.sources = endpoints
        self.done = recorded(self.summary)
        self.in_db = set() if args.rerun else existing_rows(args.db, args.sink)
        self.only_pairs = failed_pairs(args.retry_failed) if args.retry_failed else None
        if not self.summary.exists():
            with self.summary.open("w", newline="") as fh:
                csv.writer(fh).writerow(FIELDS)

    def env(self) -> dict[str, str]:
        env = dict(os.environ)
        env.update(
            VSV2_DATABASE_URL=f"postgres://visma_v2:visma_v2@127.0.0.1:55433/{self.args.db}",
            VSV2_REDIS_URL="redis://127.0.0.1:56380/0",
            VSV2_BIGQUERY_PROJECT="",
            VSV2_BIGQUERY_DATASET_BRONZE="",
            VSV2_BIGQUERY_DATASET_SHADOW="",
            VSV2_BIGQUERY_DATASET_STAGING="",
            VSV2_LOCAL_BRONZE_POSTGRES_ENABLED="true",
            VSV2_RATELIMIT_ERP_REST_RPM=str(self.args.erp_rpm),
            VSV2_RATELIMIT_ERP_REST_RPD="100000",
            VSV2_RATELIMIT_DATAMART_RPM=str(self.args.dm_rpm),
            VSV2_RATELIMIT_DATAMART_RPD="100000",
            VSV2_RATELIMIT_EXPENSE_RPM="60",
            VSV2_RATELIMIT_EXPENSE_RPD="100000",
            RUST_LOG="info",
        )
        if self.args.sink == "bigquery":
            env.update(
                VSV2_BIGQUERY_PROJECT=self.args.bq_project,
                VSV2_BIGQUERY_DATASET_STAGING=self.args.bq_dataset,
                VSV2_LOCAL_BRONZE_POSTGRES_ENABLED="false",
                # the Rust writer starts `vsv2-bq-stage` from the tooling virtualenv
                PATH=f"{ROOT / '.venv/bin'}{os.pathsep}{env.get('PATH', '')}",
            )
        return env

    def run_one(self, tenant: str, name: str, endpoint: str) -> Result:
        source = self.sources[endpoint]
        started = time.time()
        lowest: int | None = None
        last_left, reset = None, 0
        kept: list[str] = []
        records = written = 0
        status, error = "ok", ""
        try:
            proc = subprocess.Popen(
                [str(BINARY), "sync", "--tenant", tenant, "--endpoint", endpoint],
                cwd=ROOT,
                env=self.env(),
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                errors="replace",
            )
            # monotonic: the wall clock jumps when the machine sleeps, which is no reason to kill
            deadline = time.monotonic() + self.args.timeout
            assert proc.stdout is not None
            for raw in proc.stdout:
                line = ANSI.sub("", raw.rstrip("\n"))
                if (m := REMAINING.search(line)) and "ratelimit" in line:
                    last_left, reset = int(m.group(1)), int(m.group(2))
                    lowest = last_left if lowest is None else min(lowest, last_left)
                    continue
                kept.append(line)
                if m := FETCHED.search(line):
                    records = int(m.group(1))
                if m := WRITTEN.search(line):
                    written = int(m.group(1))
                if time.monotonic() > deadline:
                    proc.kill()
                    status, error = "timeout", f"killed after {self.args.timeout}s"
                    break
            code = proc.wait()
            if status == "ok" and code != 0:
                status = "failed"
                errors = [ln for ln in kept if ln.startswith("Error") or " ERROR " in ln]
                error = (errors or kept[-1:] or [f"exit {code}"])[-1]
            elif status == "ok" and records == 0:
                status = "empty"
        except Exception as exc:  # a crashed launch must not stop the sweep
            status, error = "failed", f"{type(exc).__name__}: {exc}"
        if status not in ("ok", "empty"):
            (self.out / "logs" / f"{tenant[:8]}.{endpoint}.log").write_text("\n".join(kept[-200:]))
        result = Result(
            tenant, name, endpoint, source, status, records, written,
            int(time.time() - started), lowest, error[:400].replace("\n", " "),
        )  # fmt: skip
        if source == "erp_rest" and last_left is not None and last_left < self.args.floor:
            time.sleep(min(reset, 3600) + 5)  # this tenant's hourly window, then carry on
        return result

    def record(self, r: Result) -> None:
        with self.lock, self.summary.open("a", newline="") as fh:
            csv.writer(fh).writerow(
                [
                    r.tenant, r.name, r.endpoint, r.source, r.status, r.records, r.written,
                    r.seconds, "" if r.min_remaining is None else r.min_remaining, r.error,
                    datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%SZ"),
                ]
            )  # fmt: skip
        print(
            f"{datetime.now():%H:%M:%S} {r.name[:26]:26} {r.endpoint:34} {r.status:8} "
            f"rows={r.records:>8,} {r.seconds:>5}s {r.error[:90]}",
            flush=True,
        )

    def run_tenant(self, tenant: str, name: str) -> None:
        for endpoint in self.order:
            if (self.out / "STOP").exists():
                print(f"{name}: STOP file found, quitting", flush=True)
                return
            key = (tenant, endpoint)
            if key in self.done or key in self.in_db:
                continue
            if self.only_pairs is not None and key not in self.only_pairs:
                continue
            heavy = self.pages.get(endpoint, 0) >= HEAVY_PAGES
            if heavy:
                self.heavy.acquire()
            try:
                self.record(self.run_one(tenant, name, endpoint))
            finally:
                if heavy:
                    self.heavy.release()


def report(dirs: list[Path]) -> int:
    """Latest status per (tenant, endpoint) over sweep directories (later ones win)."""
    latest: dict[tuple[str, str], dict[str, str]] = {}
    for out in dirs:
        for row in csv.DictReader((out / "summary.csv").open()):
            latest[(row["tenant"], row["endpoint"])] = row
    counts: dict[str, int] = {}
    for row in latest.values():
        counts[row["status"]] = counts.get(row["status"], 0) + 1
    print(f"{len(latest)} tenant/endpoint pairs: {counts}")
    rows = sum(int(r["records"] or 0) for r in latest.values())
    print(f"{rows:,} rows fetched in the latest run of each pair")
    for (_tenant, endpoint), row in sorted(
        latest.items(), key=lambda kv: (kv[1]["name"], kv[0][1])
    ):
        if row["status"] not in ("ok", "empty"):
            print(f"  {row['name'][:28]:28} {endpoint:30} {row['status']:8} {row['error'][:110]}")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument(
        "--report",
        type=Path,
        nargs="+",
        help="print the latest status over sweep directories, a retry's directory after the first",
    )
    ap.add_argument("--db")
    ap.add_argument(
        "--tenants", default="", help="comma-separated tenant ids, run in that order (default: all)"
    )
    ap.add_argument("--endpoints", default="", help="comma-separated endpoint names")
    ap.add_argument("--workers", type=int, default=4)
    ap.add_argument("--heavy-slots", type=int, default=2)
    ap.add_argument("--erp-rpm", type=int, default=120)
    ap.add_argument("--dm-rpm", type=int, default=120)
    ap.add_argument("--floor", type=int, default=3000, help="wait for the reset under this many")
    ap.add_argument("--timeout", type=int, default=3 * 3600, help="seconds per sync")
    ap.add_argument("--sink", choices=("postgres", "bigquery"), default="postgres")
    ap.add_argument("--bq-project", default=PROJECT)
    ap.add_argument("--bq-dataset", default=DATASET)
    ap.add_argument("--include-pilot", action="store_true", help="also run the pilot tenant")
    ap.add_argument("--rerun", action="store_true", help="also run what the database already has")
    ap.add_argument(
        "--retry-failed",
        type=Path,
        help="run only the pairs whose latest run in this sweep directory failed",
    )
    ap.add_argument(
        "--out",
        default=str(ROOT / "docs/evidence" / f"tenant_sweep_{datetime.now():%Y%m%d_%H%M%S}"),
    )
    args = ap.parse_args()
    if args.report:
        return report(args.report)
    if not args.db:
        ap.error("--db is required")
    if args.db.startswith("vsv2_ingest_"):
        ap.error("the database holds several tenants: do not name it vsv2_ingest_*")

    endpoints = load_endpoints()
    if args.endpoints:
        wanted = {e for e in args.endpoints.split(",") if e}
        endpoints = {e: s for e, s in endpoints.items() if e in wanted}
    chosen = {t for t in args.tenants.split(",") if t}
    todo = [
        (t, n)
        for t, n, _ in tenants.TENANTS
        if (args.include_pilot or t != tenants.PILOT) and (not chosen or t in chosen)
    ]
    if chosen:  # the order given on the command line: put the heaviest tenants first
        order = {t: i for i, t in enumerate(args.tenants.split(","))}
        todo.sort(key=lambda item: order.get(item[0], len(order)))
    sweep = Sweep(args, endpoints)
    register_tenants(sweep.env())
    print(f"{len(todo)} tenants x {len(endpoints)} endpoints, out={sweep.out}", flush=True)
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        for future in [pool.submit(sweep.run_tenant, t, n) for t, n in todo]:
            future.result()
    print("sweep finished", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
