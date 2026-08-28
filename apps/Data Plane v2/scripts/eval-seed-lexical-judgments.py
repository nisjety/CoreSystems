#!/usr/bin/env python3
"""Seed the MINED LEXICAL golden set into `eval_golden_judgments`, durably and
labeled `query_class = 'lexical_identifier'`.

## Why this is a separate script from the miner

`eval-mine-lexical-queries.py` computes the judgments (tokens occurring in
exactly one — or, kept as a multi-answer judgment, exactly two — corpus
documents) and writes them to `golden-lexical*.json` in a scratch dir. That
JSON pair already drives the standalone retrieval-quality gate
(`eval-lexical-cell.sh` / `scripts/eval-gate.py`), which needs no database row
at all — it evaluates fixed files against a live `/v1/retrieve`.

The DURABLE, in-service eval (`data-quality-go`) works differently: it scores
whatever traffic recently ran through `/v1/retrieve`, joined against
`eval_golden_judgments` by normalized query text (see
`services/data-quality-go/internal/eval/runner.go`). For the lexical class to
be judged THERE too — so `GET /v1/evals/{id}`'s scorecard reports a
`lexical_identifier` entry in `by_class`, not just the offline gate — the
judgments have to be upserted into that table. Mining and seeding are kept
separate the same way `eval-build-golden-set.py` (mine) and `seed_golden_set.py`
(seed) already are for the natural-language set.

## Why this doesn't re-derive queries itself

Unlike `seed_golden_set.py` (which derives its own known-item queries from
chunk vocabulary), this reads the ALREADY-mined `golden-lexical*.json` files
verbatim. Re-deriving here would let the two mechanisms drift — the gate and
the in-service eval would judge different queries as "the lexical class" — and
the miner's single/two-document uniqueness check is exactly the source of
truth for what counts as ground truth here.

Read-only by default. `--apply` performs an idempotent upsert.

Usage:
  python3 scripts/eval-seed-lexical-judgments.py <work-dir> [--apply]

<work-dir> must contain golden-lexical.json (org-corpus-baseline) and
golden-lexical-ctx.json (org-corpus-contextual), from
scripts/eval-mine-lexical-queries.py.
"""
from __future__ import annotations

import argparse
import json
import os
import shlex
import subprocess
import sys

PG_CONTAINER = "data-plane-v2-postgres-1"
QUERY_CLASS = "lexical_identifier"

FILES = {
    "org-corpus-baseline": "golden-lexical.json",
    "org-corpus-contextual": "golden-lexical-ctx.json",
}


def psql(sql: str) -> str:
    """Run SQL inside the Postgres container — same convention as
    seed_golden_set.py's psql(): shlex.quote (not repr) so SQL containing both
    quote characters survives the shell hop, and the password stays inside the
    container's own environment.
    """
    proc = subprocess.run(
        [
            "docker", "exec", PG_CONTAINER, "sh", "-c",
            'PGPASSWORD="$POSTGRES_PASSWORD" psql -U "$POSTGRES_USER" '
            '-d "$POSTGRES_DB" -v ON_ERROR_STOP=1 -t -A -c ' + shlex.quote(sql),
        ],
        capture_output=True, text=True, encoding="utf-8",
    )
    if proc.returncode != 0:
        sys.exit(f"psql failed: {proc.stderr.strip()}")
    return proc.stdout


def load_entries(work_dir: str, org: str, filename: str) -> list[dict]:
    path = os.path.join(work_dir, filename)
    if not os.path.exists(path):
        sys.exit(f"missing {path} — run eval-mine-lexical-queries.py first")
    entries = json.load(open(path, encoding="utf-8"))
    for e in entries:
        e["org_id"] = org
    return entries


def sql_ids_array(ids: list[str]) -> str:
    # jsonb_build_array(...) rather than a JSON string literal: it keeps double
    # quotes out of the statement, so escaping is single-quote-only and survives
    # the shell hop the same way seed_golden_set.py's values() does. Supports
    # any number of ids, unlike a hand-built single-element array — the mined
    # set keeps BOTH ids for a two-document term as a legitimate multi-answer
    # judgment.
    args = ",".join("'{}'".format(i.replace("'", "''")) for i in ids)
    return f"jsonb_build_array({args})"


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("work_dir")
    ap.add_argument("--apply", action="store_true", help="upsert into eval_golden_judgments")
    args = ap.parse_args()

    rows: list[tuple[str, str, list[str]]] = []
    for org, filename in FILES.items():
        entries = load_entries(args.work_dir, org, filename)
        for e in entries:
            rows.append((org, e["query"], e["relevant_ids"]))

    if not rows:
        sys.exit("no lexical judgments found in either file")

    print(f"loaded {len(rows)} lexical judgments across {len(FILES)} orgs:")
    for org, query, ids in rows[:6]:
        print(f"  [{org}] {query:<40} -> {ids}")
    if len(rows) > 6:
        print(f"  ... {len(rows) - 6} more")

    if not args.apply:
        print("\nDRY RUN — re-run with --apply to upsert.")
        return

    values = ",".join(
        "('{}','{}',{},'{}')".format(
            org.replace("'", "''"),
            # query_norm mirrors NormalizeQuery in golden.go: lowercase, collapsed
            # whitespace. The mined queries are already single lowercase tokens
            # with no internal whitespace, so this is a no-op today — spelled out
            # so a future non-identifier lexical query stays correctly joinable.
            " ".join(query.lower().split()).replace("'", "''"),
            sql_ids_array(ids),
            QUERY_CLASS,
        )
        for org, query, ids in rows
    )
    sql = (
        "INSERT INTO eval_golden_judgments (org_id, query_norm, relevant_ids, query_class) VALUES "
        + values
        + " ON CONFLICT (org_id, query_norm) DO UPDATE SET "
        "relevant_ids = EXCLUDED.relevant_ids, query_class = EXCLUDED.query_class, updated_at = NOW();"
    )
    psql(sql)
    count = psql(
        f"SELECT count(*) FROM eval_golden_judgments WHERE query_class = '{QUERY_CLASS}';"
    ).strip()
    print(f"\napplied. eval_golden_judgments now holds {count} rows with query_class='{QUERY_CLASS}'.")


if __name__ == "__main__":
    main()
