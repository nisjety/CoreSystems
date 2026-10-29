#!/usr/bin/env python3
"""Drop from `validation_fields` the fields that real rows do not always have.

`validation_fields` must hold what every row carries, because one row without them fails the whole
sync. Lists written from the pilot tenant's rows often require fields another tenant simply leaves
out (the API omits empty values: a cash transaction without a description, a warehouse without a
replenishment rule). The sync's error names each such field with the number of rows lacking it:

    ... rows per reason: missing validation field descriptionText: 35; ...

This reads those errors from the logs of failed runs (`scripts/tenant_sweep.py` keeps them under
<sweep dir>/logs), and lists per endpoint which fields to remove. With --apply it edits the endpoint
configs. A key field is never removed: a row without its key is a real problem, not an optional
field. The errors show the twelve most common reasons, so run the failed endpoints again afterwards
and repeat until nothing is left.

Usage: scripts/relax_validation_fields.py <sweep dir> [--apply]
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MISSING = re.compile(r"missing validation field ([^:;]+?): (\d+)")
LOG_NAME = re.compile(r"^(?P<tenant>[0-9a-f]{8})\.(?P<endpoint>.+)\.log$")


def key_fields(cfg: dict) -> set[str]:
    key = cfg["primary_key"]
    return set(key if isinstance(key, list) else [key])


def missing_by_endpoint(sweep: Path) -> dict[str, dict[str, int]]:
    """endpoint -> field -> most rows lacking it in any one failed run."""
    found: dict[str, dict[str, int]] = defaultdict(dict)
    for log in sorted((sweep / "logs").glob("*.log")):
        match = LOG_NAME.match(log.name)
        if not match:
            continue
        for line in log.read_text().splitlines():
            if not line.startswith("Error") or "missing validation field" not in line:
                continue
            for field, count in MISSING.findall(line.split("first rejection:")[0]):
                fields = found[match["endpoint"]]
                fields[field.strip()] = max(fields.get(field.strip(), 0), int(count))
    return found


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("sweep", type=Path)
    ap.add_argument("--apply", action="store_true")
    args = ap.parse_args()

    changed = 0
    for endpoint, fields in sorted(missing_by_endpoint(args.sweep).items()):
        path = ROOT / "config/endpoints" / f"{endpoint}.json"
        if not path.exists():
            continue
        cfg = json.loads(path.read_text())
        current = cfg.get("validation_fields") or []
        protected = key_fields(cfg)
        drop = [f for f in current if f in fields and f not in protected]
        kept_key = [f for f in fields if f in protected]
        summary = ", ".join(f"{f} ({fields[f]:,})" for f in drop)
        print(f"{endpoint:32} remove: {summary or '-'}")
        if kept_key:
            print(f"{'':32} KEY FIELD missing, not touched: {', '.join(kept_key)}")
        if args.apply and drop:
            cfg["validation_fields"] = [f for f in current if f not in drop]
            tmp = path.with_suffix(".json.tmp")
            tmp.write_text(json.dumps(cfg, indent=2, ensure_ascii=False) + "\n")
            tmp.replace(path)
            changed += 1
    if args.apply:
        print(f"{changed} config(s) changed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
