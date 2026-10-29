#!/usr/bin/env python3
"""Carry the v1 Visma Dataform definitions (silver, gold, Excel inputs) over to this project.

A copy, not a move: the v1 repository (apps/visma_service/Dataform, deployed as `Datawarehouse` in
prj-dw-dev) is left as it is. What changes on the way is the dataset names (v1 `dw_1_<layer>_<x>`
become `dw_<layer>_<x>`, see SCHEMAS) and nothing else, so the models stay comparable with v1's.
Bronze is not copied: it is generated from staging (generate_bronze.py).

Not carried: the Xero models, credentials, helper scripts, the `finance docs` folder and data files
that are not definitions.

Usage: dataform/tools/port_v1_definitions.py [--source ../Dataform/definitions]
"""

from __future__ import annotations

import argparse
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_SOURCE = ROOT.parent.parent / "Dataform" / "definitions"

SCHEMAS = {
    "dw_1_bronze_visma": "visma_bronze",
    "dw_1_bronze_excel": "visma_bronze_excel",
    "dw_1_silver_visma": "visma_silver",
    "dw_1_silver_common": "visma_silver_common",
    "dw_1_silver_excel": "visma_silver_excel",
    "dw_1_gold_common": "visma_gold_common",
    "dw_1_gold_finance": "visma_gold_finance",
    "dw_1_gold_sales": "visma_gold_sales",
    "dw_1_gold_bi": "visma_datamart",
    "dw_1_staging_currency_n2": "dw_staging_currency",
}
PATTERN = re.compile("|".join(sorted(map(re.escape, SCHEMAS), key=len, reverse=True)))

# Folders copied as they are (relative to the v1 definitions folder).
FOLDERS = ["silver", "gold", "bronze/excel"]
# Single files copied to another place: v1 path -> new path.
FILES = {"bronze/visma/history_currency_rates.sqlx": "sources/staging_currency.sqlx"}
SUFFIXES = {".sqlx", ".js"}


def remap(text: str) -> str:
    return PATTERN.sub(lambda m: SCHEMAS[m.group(0)], text)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    args = ap.parse_args()
    target = ROOT / "definitions"
    copied = 0
    for folder in FOLDERS:
        for path in sorted((args.source / folder).rglob("*")):
            if path.suffix not in SUFFIXES or path.name.startswith("."):
                continue
            out = target / path.relative_to(args.source)
            out.parent.mkdir(parents=True, exist_ok=True)
            out.write_text(remap(path.read_text()))
            copied += 1
    for src, dst in FILES.items():
        out = target / dst
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(remap((args.source / src).read_text()))
        copied += 1
    left = sorted(
        {
            str(p.relative_to(target))
            for p in target.rglob("*")
            if p.suffix in SUFFIXES and "dw_1_" in p.read_text()
        }
    )
    print(f"{copied} definitions copied; files still mentioning dw_1_: {len(left)}")
    for name in left[:20]:
        print("  ", name)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
