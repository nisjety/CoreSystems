#!/usr/bin/env python3
"""Measure how each configured field of an endpoint is really populated in live data.

`validation_fields` that the API never returns, or leaves empty on some rows,
make the executor reject rows - and one rejected row fails the whole endpoint.
This tool loads the endpoint with a scratch config that validates nothing but
the primary key, then reports per configured field how many rows lack it, using
the executor's own notion of "present" (null, blank string, [] and {} are missing).

Reads from Visma; writes only to the throwaway `vsv2_ingest_*` database.

Usage:
  scripts/validation_coverage.py --db vsv2_ingest_20260929b earning_types employees
  scripts/validation_coverage.py --db vsv2_ingest_20260929b stocktakes_v2 \
      --pk stocktakes_v2=referenceNbr
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import ingest_probe as ip

Json = object


def is_present(value: Json) -> bool:
    """Mirror of `validation::is_present` in vsv2-orchestrator."""
    if value is None:
        return False
    if isinstance(value, str):
        return value.strip() != ""
    if isinstance(value, (list, dict)):
        return len(value) > 0
    return True


def get_path(record: Json, path: str) -> Json:
    current = record
    for part in path.split("."):
        if not isinstance(current, dict) or part not in current:
            return None
        current = current[part]
    return current


def load_with_pk_only(db: str, tenant: str, endpoint: str, pk_override: str | None) -> bool:
    cfg = json.loads((ip.ROOT / "config/endpoints" / f"{endpoint}.json").read_text())
    if pk_override:
        cfg["primary_key"] = pk_override.split(",") if "," in pk_override else pk_override
    pk = cfg["primary_key"]
    cfg["validation_fields"] = [pk] if isinstance(pk, str) else list(pk)
    ip.psql(db, f"DELETE FROM vsv2.local_bronze_record WHERE endpoint='{endpoint}'")
    ip.psql(db, f"DELETE FROM vsv2.sync_checkpoint WHERE endpoint='{endpoint}'")
    with tempfile.TemporaryDirectory() as scratch:
        (Path(scratch) / f"{endpoint}.json").write_text(json.dumps(cfg))
        proc = subprocess.run(
            [
                str(ip.ROOT / "target/debug/vsv2"),
                "sync",
                "--tenant",
                tenant,
                "--endpoint",
                endpoint,
                "--config-dir",
                scratch,
            ],
            cwd=ip.ROOT,
            env=ip.vsv2_env(db),
            capture_output=True,
            text=True,
            timeout=1200,
        )
    if proc.returncode != 0:
        tail = (proc.stdout + proc.stderr).strip().splitlines()[-1][:300]
        print(f"  load failed: {tail}")
    return proc.returncode == 0


def report(db: str, endpoint: str) -> None:
    cfg = json.loads((ip.ROOT / "config/endpoints" / f"{endpoint}.json").read_text())
    rows = [
        json.loads(r[0])
        for r in ip.psql(
            db, f"SELECT payload::text FROM vsv2.local_bronze_record WHERE endpoint='{endpoint}'"
        )
    ]
    n = len(rows)
    print(
        f"\n## {endpoint}: {n} rows | pk={cfg['primary_key']} "
        f"| modified_field={cfg.get('modified_field')}"
    )
    if not n:
        return
    fields = list(dict.fromkeys(cfg.get("validation_fields") or []))
    if cfg.get("modified_field") and cfg["modified_field"] not in fields:
        fields.append(cfg["modified_field"])
    problems = []
    for field in fields:
        missing = sum(1 for r in rows if not is_present(get_path(r, field)))
        tag = "ABSENT (field never present)" if missing == n else ("PARTIAL" if missing else "ok")
        if missing:
            problems.append((field, missing, tag))
        print(f"  {field:34} missing {missing:>6}/{n}  {tag if missing else ''}")
    top_level = sorted({k for r in rows for k in r if not k.startswith("_") and k != "tenant_id"})
    unused = [k for k in top_level if k not in {f.split(".")[0] for f in fields}]
    print(f"  payload fields not referenced by config: {', '.join(unused)}")
    if problems:
        print(f"  -> remove/fix: {', '.join(f for f, _, _ in problems)}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("endpoints", nargs="+")
    ap.add_argument("--db", required=True)
    ap.add_argument("--tenant", default=ip.DEFAULT_TENANT)
    ap.add_argument(
        "--pk",
        action="append",
        default=[],
        metavar="endpoint=field[,field]",
        help="override the primary key when the configured one is wrong",
    )
    args = ap.parse_args()
    if not args.db.startswith("vsv2_ingest_"):
        sys.exit("database must be named vsv2_ingest_*")
    overrides = dict(item.split("=", 1) for item in args.pk)
    for endpoint in args.endpoints:
        if load_with_pk_only(args.db, args.tenant, endpoint, overrides.get(endpoint)):
            report(args.db, endpoint)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
