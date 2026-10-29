#!/usr/bin/env python3
"""Field-by-field parity between ERP REST and a normalized DataMart endpoint.

ERP rows come from a local bronze database that `ingest_probe.py` filled; DataMart
rows are pulled live with `vsv2 dm-pull`. Both sides are keyed by the endpoint's
primary key (DataMart after the normalizer has reshaped it to the ERP shape), then
every ERP field is classified per shared key:

  equal        same value
  both_empty   null / "" / [] / {} on both sides
  dm_missing   ERP has a value, the normalized DataMart record does not
  erp_empty    DataMart has a value the ERP record lacks
  trim_only    equal only after trimming whitespace: the ERP pads some strings with trailing
               spaces (e.g. taskId "2         "), DataMart does not. Harmless for display, but
               a MERGE on such a key would see two different rows.
  partial      DataMart object is a strict subset of the ERP object and every key it has
               matches (typically the id without the joined description)
  tz_shift     equal timestamps except a whole-hour offset (timezone mismatch)
  differ       both populated, values differ

It measures; it does not decide. A field is only "safe" when it is equal (or empty
on both sides) for every shared key.

Usage:
  scripts/normalizer_parity.py --db vsv2_ingest_parity vat_zones
  scripts/normalizer_parity.py --db vsv2_ingest_parity suppliers --examples 2
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from collections import Counter, defaultdict
from datetime import datetime
from decimal import Decimal, InvalidOperation
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))
import ingest_probe as ip  # shared psql/env helpers

from vsv2_tooling.normalizers import REGISTRY

SYSTEM_FIELDS = {"_source", "_synced_at", "tenant_id", "_updated_in_dm", "metadata"}
COMPANY_ID = "4839725"


def empty(value: Any) -> bool:
    return value is None or value == "" or value == [] or value == {}


def get_path(record: Any, path: str) -> Any:
    for part in path.split("."):
        if not isinstance(record, dict) or part not in record:
            return None
        record = record[part]
    return record


def _clean(value: Any) -> Any:
    if isinstance(value, dict):
        cleaned = {k: c for k, v in value.items() if not empty(c := _clean(v))}
        return cleaned or None
    if isinstance(value, list):
        return [c for v in value if not empty(c := _clean(v))]
    if isinstance(value, str):
        text = value.strip()
        if text[:1] in ("{", "["):
            try:
                return _clean(json.loads(text))
            except ValueError:
                pass
        return text
    return value


def _as_number(value: Any) -> Decimal | None:
    if isinstance(value, bool):
        return None
    try:
        return Decimal(str(value))
    except (InvalidOperation, ValueError):
        return None


def _as_time(value: Any) -> datetime | None:
    if not isinstance(value, str) or not re.match(r"^\d{4}-\d\d-\d\d[T ]\d\d:\d\d", value):
        return None
    naive = re.sub(r"(Z|[+-]\d\d:?\d\d)$", "", value).replace(" ", "T")
    try:
        return datetime.fromisoformat(naive)
    except ValueError:
        return None


def _exact(value: Any) -> Any:
    """Like `_clean` but keeps string whitespace, so padding differences stay visible."""
    if isinstance(value, dict):
        return {k: e for k, v in value.items() if not empty(e := _exact(v))}
    if isinstance(value, list):
        return [e for v in value if not empty(e := _exact(v))]
    if isinstance(value, str) and value[:1] in ("{", "["):
        try:
            return _exact(json.loads(value))
        except ValueError:
            return value
    return value


def classify(erp: Any, dm: Any) -> str:
    verdict = _classify(erp, dm)
    if verdict == "equal" and _exact(erp) != _exact(dm):
        return "trim_only"
    return verdict


def _is_partial(erp: Any, dm: Any) -> bool:
    """DataMart's object (or list of objects) is a strict subset of the ERP's, values equal."""
    if isinstance(erp, dict) and isinstance(dm, dict):
        return set(dm) < set(erp) and all(_classify(erp[k], dm[k]) == "equal" for k in dm)
    if isinstance(erp, list) and isinstance(dm, list) and erp and len(erp) == len(dm):
        return all(
            _is_partial(a, b) or _classify(a, b) == "equal" for a, b in zip(erp, dm, strict=True)
        ) and any(_is_partial(a, b) for a, b in zip(erp, dm, strict=True))
    return False


def _classify(erp: Any, dm: Any) -> str:
    erp, dm = _clean(erp), _clean(dm)
    if empty(erp) and empty(dm):
        return "both_empty"
    if empty(dm):
        return "dm_missing"
    if empty(erp):
        return "erp_empty"
    if erp == dm:
        return "equal"
    if _is_partial(erp, dm):
        return "partial"
    if isinstance(erp, bool) or isinstance(dm, bool):
        as_bool = lambda v: str(v).lower() in ("true", "1")  # noqa: E731
        return "equal" if as_bool(erp) == as_bool(dm) else "differ"
    a, b = _as_number(erp), _as_number(dm)
    if a is not None and b is not None:
        return "equal" if a == b else "differ"
    ta, tb = _as_time(erp), _as_time(dm)
    if ta is not None and tb is not None:
        delta = abs((ta - tb).total_seconds())
        if delta < 1:
            return "equal"
        if delta % 3600 == 0 and delta <= 3 * 3600:
            return "tz_shift"
        return "differ"
    return "differ"


def dm_records(cfg: dict[str, Any], cache: Path | None) -> list[dict[str, Any]]:
    if cache and cache.exists():
        text = cache.read_text()
    else:
        dm = cfg["datamart"]
        env = ip.vsv2_env("vsv2_ingest_parity")
        proc = subprocess.run(
            [
                str(ip.ROOT / "target/debug/vsv2"),
                "dm-pull",
                "--tenant",
                ip.DEFAULT_TENANT,
                "--company-id",
                COMPANY_ID,
                "--query-file",
                dm["query_file"],
                "--root-field",
                dm["root_field"],
                "--page-size",
                "500",
            ],
            cwd=ip.ROOT,
            env=env,
            capture_output=True,
            text=True,
            timeout=1800,
        )
        if proc.returncode != 0:
            sys.exit(f"dm-pull failed: {(proc.stdout + proc.stderr)[-300:]}")
        text = proc.stdout
        if cache:
            cache.write_text(text)
    return json.loads(text[re.search(r"^\[", text, re.M).start() :])


def key_of(record: dict[str, Any], pk: list[str]) -> str | None:
    parts = [get_path(record, p) for p in pk]
    return None if any(empty(p) for p in parts) else "|".join(str(p).strip() for p in parts)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("endpoint")
    ap.add_argument("--db", required=True)
    ap.add_argument("--normalizer", help="override datamart.join_normalizer")
    ap.add_argument("--dm-cache", type=Path, help="reuse/save the raw dm-pull output")
    ap.add_argument("--examples", type=int, default=0, help="show N differing values per field")
    args = ap.parse_args()
    if not args.db.startswith("vsv2_ingest_"):
        sys.exit("database must be named vsv2_ingest_*")

    cfg = json.loads((ip.ROOT / "config/endpoints" / f"{args.endpoint}.json").read_text())
    name = args.normalizer or cfg["datamart"].get("join_normalizer")
    if not name or name not in REGISTRY:
        sys.exit(f"no registered normalizer (got {name!r}); registered: {sorted(REGISTRY)}")
    pk = cfg["primary_key"] if isinstance(cfg["primary_key"], list) else [cfg["primary_key"]]
    normalize = REGISTRY[name]

    erp_rows = [
        json.loads(r[0])
        for r in ip.psql(
            args.db,
            f"SELECT payload::text FROM vsv2.local_bronze_record WHERE endpoint='{args.endpoint}'",
        )
    ]
    if not erp_rows:
        sys.exit(f"no ERP rows for {args.endpoint} in {args.db}; run ingest_probe.py first")
    raw = dm_records(cfg, args.dm_cache)
    live = [r for r in raw if not r.get("deletedDatabaseRecord")]
    dm_norm = [normalize(r) for r in live]

    erp_by = {k: r for r in erp_rows if (k := key_of(r, pk))}
    dm_by = {k: r for r in dm_norm if (k := key_of(r, pk))}
    both = sorted(set(erp_by) & set(dm_by))
    print(f"\n## {args.endpoint}  (normalizer: {name})")
    print(
        f"ERP {len(erp_by)} | DataMart live {len(dm_by)} of {len(raw)} rows "
        f"({len(raw) - len(live)} tombstones) | shared keys {len(both)} | only ERP "
        f"{len(set(erp_by) - set(dm_by))} | only DataMart {len(set(dm_by) - set(erp_by))}"
    )
    if not both:
        print("no shared keys: check that the normalizer emits the primary key fields")
        return 1

    fields = sorted({f for r in erp_rows for f in r} - SYSTEM_FIELDS)
    stats: dict[str, Counter] = {f: Counter() for f in fields}
    examples: dict[str, list[tuple[Any, Any]]] = defaultdict(list)
    for k in both:
        for f in fields:
            verdict = classify(erp_by[k].get(f), dm_by[k].get(f))
            stats[f][verdict] += 1
            if verdict in ("differ", "tz_shift") and len(examples[f]) < args.examples:
                examples[f].append((erp_by[k].get(f), dm_by[k].get(f)))

    order = [
        "equal",
        "both_empty",
        "trim_only",
        "partial",
        "tz_shift",
        "differ",
        "dm_missing",
        "erp_empty",
    ]
    print(f"\n{'field':30}" + "".join(f"{c:>12}" for c in order) + "   verdict")
    safe = 0
    for f in sorted(fields, key=lambda f: -(sum(stats[f][c] for c in order[4:]))):
        s = stats[f]
        ok = s["equal"] + s["both_empty"] == len(both)
        safe += ok
        print(f"{f:30}" + "".join(f"{s[c]:>12}" for c in order) + f"   {'OK' if ok else 'GAP'}")
        for erp, dm in examples[f]:
            print(f"    e.g. ERP={json.dumps(erp)[:70]}  DM={json.dumps(dm)[:70]}")
    print(f"\nfields with full parity: {safe}/{len(fields)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
