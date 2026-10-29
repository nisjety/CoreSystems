#!/usr/bin/env python3
"""Which rows does v1 hold that v2 did not fetch, and the other way round?

For every v1 table with a v2 counterpart (matched by source URL, see v1_v2_row_counts.py) this
compares the DISTINCT keys of the pilot tenant: v1's not-deleted rows in `dw_1_bronze_visma`
(read-only `bq` queries) against the rows v2 landed in the scratch `vsv2_ingest_*` databases, both
keyed by v1's primary key columns.

  v1 only   v1 keeps the row, v2 did not fetch it. Either the row is gone from Visma and v1 never
            removed it (v1 deletes only after it stays missing for several runs), or v2 misses it.
  v2 only   v2 fetched the row, v1 has none: new since v1's last sync, or v1 loses it.

It then asks DataMart about each v1-only key (for the entities where an ERP<->DataMart pair is
known): still live there (v2 gets it from the DataMart side), tombstoned there (deleted in Visma, v1
kept a dead row) or absent from both (unexplained; samples are listed for a check by hand).
Tables whose key is not a plain column of the v1 table, or with more than 150,000 distinct keys, are
skipped. Nothing is written except docs/v1_v2_key_diff.md.

Usage: scripts/v1_v2_key_diff.py [--tenant ID] [--project prj-dw-dev] [--only a,b] [--db NAME]
  --db   read v2 rows from this one scratch database instead of the fullest one per endpoint
         (a fresh fetch, so "new since v2 was fetched" is not mistaken for "v2 misses it");
         with --db or --only nothing is written to docs/v1_v2_key_diff.md
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "docs/v1_v2_key_diff.md"
sys.path.insert(0, str(ROOT / "scripts"))

import reconcile_sources as rs  # noqa: E402
import v1_v2_row_counts as counts  # noqa: E402

MAX_KEYS = 150_000
SAMPLE = 5


def key_expr(fields: list[str], as_json: bool = False) -> str:
    """One key string from the fields; `as_json` reads dotted paths out of a JSON text column."""

    def column(field: str) -> str:
        if as_json and "." in field:
            head, _, rest = field.partition(".")
            return f"JSON_VALUE({head}, '$.{rest}')"
        return field

    return ", '|', ".join(
        f"IFNULL(CAST(TRIM(CAST({column(f)} AS STRING)) AS STRING), '')" for f in fields
    )


def _bq_keys(project: str, table: str, tenant: str, fields: list[str], as_json: bool):
    sql = (
        f"SELECT DISTINCT CONCAT({key_expr(fields, as_json)}) AS k "
        f"FROM {counts.table_ref(project, table)} "
        "WHERE tenantId = @tenant AND NOT COALESCE(isDeleted, FALSE)"
    )
    return subprocess.run(
        [
            "bq",
            "query",
            "--use_legacy_sql=false",
            "--format=json",
            f"--max_rows={MAX_KEYS + 1}",
            f"--parameter=tenant::{tenant}",
            sql,
        ],
        capture_output=True,
        text=True,
    )


def v1_keys(project: str, table: str, tenant: str, fields: list[str]) -> set[str] | str:
    result = _bq_keys(project, table, tenant, fields, as_json=False)
    if result.returncode != 0 and "Cannot access field" in result.stdout:
        # v1 keeps nested objects as JSON text in some tables
        result = _bq_keys(project, table, tenant, fields, as_json=True)
    if result.returncode != 0:
        # bq prints query errors on stdout and transport errors on stderr
        message = (result.stdout.strip() + "\n" + result.stderr.strip()).strip().splitlines()
        return " ".join(message)[-160:] or "bq failed"
    rows = json.loads(result.stdout)
    if len(rows) > MAX_KEYS:
        return f"more than {MAX_KEYS:,} keys"
    return {r["k"] for r in rows}


def v2_keys(bronze: dict, endpoint: str, fields: list[str]) -> set[str] | str:
    if endpoint not in bronze:
        return "no v2 bronze rows"
    columns = ", ".join(rs.json_path(f) for f in fields)
    keys, missing = set(), 0
    for line in rs.psql(
        bronze[endpoint][0],
        f"select {columns} from vsv2.local_bronze_record where endpoint = '{endpoint}'",
    ):
        parts = [p.strip() for p in line.split(rs.SEP)]
        if not any(parts):
            missing += 1
            continue
        keys.add("|".join(parts))
    return keys if keys or not missing else f"v2 rows lack {', '.join(fields)}"


def datamart_verdicts(
    bronze: dict, lookups: rs.Lookups, pairs: dict[str, rs.Pair], name: str, fields: list[str], keys
) -> tuple[int, int, int] | str:
    """(live in DataMart, tombstoned in DataMart, absent) for a set of v1 keys, or why not known."""
    pair = pairs.get(name)
    if pair is None:
        return "no DataMart counterpart"
    if [f.lower() for f in fields] != [f.lower() for f in pair.erp_key]:
        return "v1 and DataMart key differently"
    if pair.dm not in bronze:
        return "no DataMart rows"
    db, total = bronze[pair.dm]
    side = rs.load_dm(db, pair, pair.erp_key, total, lookups)
    live = tombstoned = 0
    for key in keys:
        found = side.keys.get(tuple(key.split("|")))
        if found is None:
            continue
        if found["deleted"]:
            tombstoned += 1
        else:
            live += 1
    return live, tombstoned, len(keys) - live - tombstoned


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--tenant", default=counts.PILOT_TENANT)
    ap.add_argument("--project", default="prj-dw-dev")
    ap.add_argument("--only", default="")
    ap.add_argument("--db", default="")
    args = ap.parse_args()
    only = {n for n in args.only.split(",") if n}

    v1 = counts.load_v1()
    v2_configs = rs.load_configs()
    bronze = rs.locate([args.db] if args.db else rs.scratch_databases())
    names = [
        n
        for n in sorted(v1)
        if n not in counts.DERIVED
        and counts.V1_TO_V2.get(n, n) in v2_configs
        and (not only or n in only)
    ]

    def job(name: str):
        fields = counts.as_list(v1[name]["primary_key"])
        table = v1[name].get("bigquery_table", f"visma_{name}")
        return name, fields, v1_keys(args.project, table, args.tenant, fields)

    with ThreadPoolExecutor(max_workers=6) as pool:
        first = list(pool.map(job, names))

    rows, skipped = [], []
    for name, fields, keys1 in first:
        target = counts.V1_TO_V2.get(name, name)
        keys2 = v2_keys(bronze, target, fields) if isinstance(keys1, set) else keys1
        if isinstance(keys1, str) or isinstance(keys2, str):
            skipped.append((name, keys1 if isinstance(keys1, str) else keys2))
            continue
        rows.append((name, target, fields, keys1, keys2))

    keyed, _ = rs.config_pairs(v2_configs)
    pairs = {p.erp: p for p in keyed + rs.EXPLICIT}
    lookups = rs.Lookups(bronze)
    verdicts = {
        name: datamart_verdicts(bronze, lookups, pairs, name, fields, k1 - k2)
        for name, target, fields, k1, k2 in rows
        if k1 - k2
    }

    lines = [
        "# v1 vs v2: rows one side has and the other lacks (pilot tenant)",
        "",
        "> Generated by `scripts/v1_v2_key_diff.py`. Distinct keys under v1's primary key;",
        "> v1 = not-deleted rows of `dw_1_bronze_visma`, v2 = one full fetch.",
        "> The two were fetched at different times.",
        "",
        "`v1 only` is split by what DataMart says about those keys: live there (v2 covers them "
        "through the DataMart side), tombstoned (deleted in Visma, v1 kept the row) or in neither.",
        "",
        "| Endpoint | v2 config | key | v1 keys | v2 keys | v1 only | live in DM | tombstoned "
        "| in neither | v2 only | v1-only sample |",
        "|---|---|---|---|---|---|---|---|---|---|---|",
    ]
    for name, target, fields, k1, k2 in sorted(rows, key=lambda r: -len(r[3] - r[4])):
        only1, only2 = k1 - k2, k2 - k1
        sample = ", ".join(sorted(only1)[:SAMPLE])
        verdict = verdicts.get(name)
        if isinstance(verdict, tuple):
            split = " | ".join(f"{n:,}" for n in verdict)
        else:
            split = f"{verdict or ''} | |" if only1 else " | |"
        lines.append(
            f"| `{name}` | `{target}` | {', '.join(fields)} | {len(k1):,} | {len(k2):,} "
            f"| {len(only1):,} | {split} | {len(only2):,} | {sample} |"
        )
    lines += ["", "## Not compared", ""]
    lines += [f"- `{n}`: {why}" for n, why in skipped] or ["- none"]
    if args.db or only:
        print(f"partial run, report not written: {len(rows)} compared, {len(skipped)} skipped")
        for name, why in skipped:
            print(f"  skipped {name}: {why}")
    else:
        OUT.write_text("\n".join(lines) + "\n")
        print(f"wrote {OUT.relative_to(ROOT)}: {len(rows)} compared, {len(skipped)} skipped")
    print(f"{'endpoint':30} {'v1 keys':>8} {'v2 keys':>8} {'v1 only':>8} {'v2 only':>8}")
    for name, _target, _fields, k1, k2 in sorted(rows, key=lambda r: -len(r[3] - r[4])):
        verdict = verdicts.get(name)
        split = (
            "  live/tomb/neither " + "/".join(map(str, verdict))
            if isinstance(verdict, tuple)
            else (f"  ({verdict})" if verdict else "")
        )
        print(f"{name:30} {len(k1):>8,} {len(k2):>8,} {len(k1 - k2):>8,} {len(k2 - k1):>8,}{split}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
