#!/usr/bin/env python3
"""Which ERP fields does DataMart also carry, and which fields does only DataMart have?

For the rows both sources return (see reconcile_sources.py) this matches fields by VALUE, not by
name and not through a normalizer: an ERP field "is in DataMart" when some DataMart field holds
the same value on (nearly) every shared row. That finds renames (`referenceNbr` =
`referenceNumber`), and it cannot be fooled by a normalizer that quietly drops or invents a field.

  in_dm      a DataMart field equals it on >= 98% of the rows where the ERP field is filled
  tz_shift   equal apart from a whole-hour offset (ERP local time vs DataMart's mislabelled Z)
  partly     a DataMart field equals it on 50-98% of those rows (empty or different on the rest)
  erp_only   no DataMart field matches: the warehouse must take it from ERP
  constant   the ERP field has one value on every row; a value match would be ambiguous
  nested     a list of objects; DataMart keeps such data in separate tables

DataMart fields no ERP field matched, and that carry data, are reported as DataMart-only.
DataMart keeps addresses, contacts, terms, payment methods and so on in their own tables, reached
through foreign keys (`mainAddressId`). Those are joined in first and appear as
`join:mainAddress.city`, so "in DataMart" means "DataMart can deliver it", not "it sits in the
same table".

Nothing is fetched from Visma. Writes docs/field_parity.md and docs/field_parity.json.

Usage:
  scripts/field_parity.py
  scripts/field_parity.py --only customers,suppliers
"""

from __future__ import annotations

import argparse
import difflib
import json
import re
import sys
from collections import Counter
from datetime import UTC, datetime
from decimal import Decimal, InvalidOperation
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
OUT_MD = ROOT / "docs/field_parity.md"
OUT_JSON = ROOT / "docs/field_parity.json"
sys.path.insert(0, str(ROOT / "scripts"))

import reconcile_sources as rs  # noqa: E402

SAMPLE_ROWS = 2500
MATCH = 0.98
PARTLY = 0.50
QUICK_ROWS = 60  # a candidate must agree on half of the first rows to be scored in full
ERP_SYSTEM = {"_source", "_synced_at", "tenant_id", "_partition_key"}
DM_TECHNICAL = {
    "_source",
    "_synced_at",
    "tenant_id",
    "id",
    "companyId",
    "deletedDatabaseRecord",
    "createdInDataMartDateTime",
    "updatedInDataMartDateTime",
}
DATETIME = re.compile(r"^(\d{4}-\d\d-\d\d)(?:[T ](\d\d:\d\d:\d\d)(\.\d+)?)?(Z|[+-]\d\d:?\d\d)?$")
PLAIN_NUMBER = re.compile(r"^-?(0|[1-9]\d*)(\.\d+)?$")
LOW_CARDINALITY = 6  # up to this many distinct values a value match can be coincidence
NAME_MATCH = 0.5  # last path segments must look alike to trust a low-cardinality match
CONSTANT_NAME_MATCH = 0.7
LOW_CONFIDENCE_ROWS = 30


def canon(value: Any) -> str | None:
    """One comparable string per value; None when the value is empty."""
    if value is None or value == "" or value == [] or value == {}:
        return None
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return _number(value)
    if isinstance(value, str):
        text = value.strip()
        if not text:
            return None
        if text[:1] in ("{", "["):
            try:
                return canon(json.loads(text))
            except ValueError:
                return text
        moment = _moment(text)
        if moment is not None:
            return moment.strftime("%Y-%m-%dT%H:%M:%S.%f").rstrip("0").rstrip(".")
        if PLAIN_NUMBER.match(text):
            return _number(text)
        return text
    if isinstance(value, dict):
        inner = {k: c for k, v in value.items() if (c := canon(v)) is not None}
        return json.dumps(inner, sort_keys=True) if inner else None
    if isinstance(value, list):
        inner = [c for v in value if (c := canon(v)) is not None]
        return json.dumps(inner, sort_keys=True) if inner else None
    return str(value)


def _number(value: Any) -> str | None:
    try:
        number = Decimal(str(value)).normalize()
    except InvalidOperation:
        return None
    if not number.is_finite():
        return None
    return format(number, "f") if number != number.to_integral() else str(int(number))


def _moment(text: str) -> datetime | None:
    """Naive datetime; an explicit offset is converted to UTC, Z and no zone are kept as is."""
    match = DATETIME.match(text)
    if not match:
        return None
    day, clock, fraction, zone = match.groups()
    try:
        moment = datetime.fromisoformat(f"{day}T{clock or '00:00:00'}{fraction or ''}")
    except ValueError:
        return None
    if zone and zone != "Z":
        sign = 1 if zone[0] == "+" else -1
        digits = zone[1:].replace(":", "")
        offset = sign * (int(digits[:2]) * 60 + int(digits[2:]))
        moment = (moment.replace(tzinfo=UTC) - _minutes(offset)).replace(tzinfo=None)
    return moment


def _minutes(n: int):
    from datetime import timedelta

    return timedelta(minutes=n)


def leaves(record: Any, prefix: str = "") -> dict[str, Any]:
    """Dotted path -> value for scalars; a list becomes one opaque `path[]` entry."""
    found: dict[str, Any] = {}
    if isinstance(record, dict):
        for key, value in record.items():
            found.update(leaves(value, f"{prefix}.{key}" if prefix else key))
    elif isinstance(record, list):
        found[prefix + "[]"] = record
    else:
        found[prefix] = record
    return found


def column_values(rows: list[dict], system: set[str]) -> dict[str, list[str | None]]:
    per_row = [leaves(r) for r in rows]
    paths = {p for row in per_row for p in row if p.split(".")[0] not in system}
    paths = {p for p in paths if not p.startswith("metadata.")}
    return {p: [canon(row.get(p)) for row in per_row] for p in sorted(paths)}


def instant(text: str | None) -> datetime | None:
    return _moment(text) if text else None


def score(erp: list[str | None], dm: list[str | None], early: bool = True) -> tuple[float, float]:
    """(agreement, dm-empty share) over the rows where the ERP value is filled."""
    filled = [i for i, v in enumerate(erp) if v is not None]
    if not filled:
        return 0.0, 0.0
    if early:
        probe = filled[:QUICK_ROWS]
        if sum(1 for i in probe if erp[i] == dm[i]) / len(probe) < PARTLY:
            return 0.0, 0.0
    equal = sum(1 for i in filled if erp[i] == dm[i])
    empty = sum(1 for i in filled if dm[i] is None)
    return equal / len(filled), empty / len(filled)


def shifted(erp: list[str | None], dm: list[str | None]) -> float:
    """Share of filled ERP rows where the DataMart value is the same time up to whole hours."""
    filled = [i for i, v in enumerate(erp) if v is not None]
    hits = 0
    for i in filled:
        a, b = instant(erp[i]), instant(dm[i])
        if a is not None and b is not None:
            delta = abs((a - b).total_seconds())
            hits += delta % 3600 == 0 and delta <= 3 * 3600
    return hits / len(filled) if filled else 0.0


def similarity(a: str, b: str) -> float:
    left, right = a.split(".")[-1].lower(), b.split(".")[-1].lower()
    return difflib.SequenceMatcher(None, left, right).ratio()


def path_similarity(a: str, b: str) -> float:
    return difflib.SequenceMatcher(None, a.lower(), b.replace("join:", "").lower()).ratio()


def _constant_twin(path: str, value: str, dm_cols: dict) -> str | None:
    """A DataMart field with the same constant value and a similar name, if there is one."""
    best: tuple[float, str] | None = None
    for dm_path, dm in dm_cols.items():
        filled = {v for v in dm if v is not None}
        if filled == {value} and similarity(path, dm_path) >= CONSTANT_NAME_MATCH:
            candidate = (similarity(path, dm_path), dm_path)
            best = max(best, candidate) if best else candidate
    return best[1] if best else None


def match_fields(erp_cols: dict, dm_cols: dict) -> tuple[list[dict], list[str]]:
    results: list[dict] = []
    claimed: set[str] = set()
    for path, erp in erp_cols.items():
        filled = [v for v in erp if v is not None]
        entry: dict[str, Any] = {"erp": path, "filled": len(filled), "rows": len(erp)}
        if not filled:
            entry["status"] = "always_empty"
            results.append(entry)
            continue
        if path.endswith("[]"):
            entry["status"] = "nested"
            results.append(entry)
            continue
        distinct = len(set(filled))
        if distinct == 1:
            twin = _constant_twin(path, filled[0], dm_cols)
            if twin:
                claimed.add(twin)
                entry.update(status="in_dm", dm=twin, agreement=1.0, dm_empty=0.0, constant=True)
            else:
                entry.update(status="constant", value=filled[0][:40])
            results.append(entry)
            continue
        best: tuple[float, float, float, str] | None = None
        best_empty = 0.0
        for dm_path, dm in dm_cols.items():
            if distinct <= LOW_CARDINALITY and similarity(path, dm_path) < NAME_MATCH:
                continue
            agree, empty = score(erp, dm)
            if agree < PARTLY:
                continue
            candidate = (agree, similarity(path, dm_path), path_similarity(path, dm_path), dm_path)
            if best is None or candidate[:3] > best[:3]:
                best, best_empty = candidate, empty
        if best is not None and best[0] >= PARTLY:
            agree, _, _, dm_path = best
            claimed.add(dm_path)
            entry.update(
                status="in_dm" if agree >= MATCH else "partly",
                dm=dm_path,
                agreement=round(agree, 3),
                dm_empty=round(best_empty, 3),
            )
        else:
            best_shift = max(
                ((shifted(erp, dm), dm_path) for dm_path, dm in dm_cols.items()),
                default=(0.0, ""),
            )
            if best_shift[0] >= MATCH:
                claimed.add(best_shift[1])
                entry.update(status="tz_shift", dm=best_shift[1], agreement=round(best_shift[0], 3))
            else:
                entry["status"] = "erp_only"
        results.append(entry)
    dm_only = [
        p
        for p, col in dm_cols.items()
        if p not in claimed
        and any(v is not None for v in col)
        and not p.endswith("[]")
        and not p.startswith("join:")
    ]
    return results, dm_only


# Foreign key column (regex) -> DataMart table and its key column. The alias is the column name
# without `Id`, so `mainAddressId` becomes the path prefix `join:mainAddress`.
JOINS = [
    (re.compile(r"^(.*)addressid$", re.I), "dm_addresses", "addressId"),
    (re.compile(r"^(.*)contactid$", re.I), "dm_contacts", "contactId"),
    (re.compile(r"^(termsId|creditTermsId)$"), "dm_terms", "termsId"),
    (re.compile(r"^paymentMethodId$"), "dm_payment_methods", "paymentMethodId"),
    (re.compile(r"^supplierClassId$"), "dm_supplier_classes", "supplierClassId"),
    (re.compile(r"^vatZoneId$"), "dm_vat_zones", "vatZoneId"),
    (re.compile(r"^accountId$"), "dm_accounts", "accountId"),
    (re.compile(r"^subaccountId$"), "dm_subaccounts", "subaccountId"),
    (re.compile(r"^locationId$"), "dm_locations", "locationId"),
    (re.compile(r"^countryId$"), "dm_countries", "countryId"),
    (re.compile(r"^customerId$"), "dm_customers", "customerId"),
    (re.compile(r"^supplierId$"), "dm_suppliers", "supplierId"),
    (re.compile(r"^branchId$"), "dm_branches", "branchId"),
    (re.compile(r"^salesPersonId$"), "dm_sales_persons", "salesPersonId"),
]


class Joiner:
    """Adds the rows a DataMart row points at, under `join:<alias>`."""

    def __init__(self, dbs: dict) -> None:
        self.dbs = dbs
        self._tables: dict[str, dict[str, dict]] = {}

    def table(self, endpoint: str, key: str) -> dict[str, dict]:
        if endpoint not in self._tables:
            rows: dict[str, dict] = {}
            if endpoint in self.dbs:
                sql = "select payload::text from vsv2.local_bronze_record "
                for line in rs.psql(self.dbs[endpoint][0], sql + f"where endpoint = '{endpoint}'"):
                    row = json.loads(line)
                    if row.get("deletedDatabaseRecord") or row.get(key) is None:
                        continue
                    rows[str(row[key])] = {
                        k: v
                        for k, v in row.items()
                        if not isinstance(v, (dict, list)) and k not in DM_TECHNICAL
                    }
            self._tables[endpoint] = rows
        return self._tables[endpoint]

    def augment(self, raw: dict, own_table: str) -> dict:
        joined = {}
        for column, value in raw.items():
            if value is None or isinstance(value, (dict, list)):
                continue
            for pattern, endpoint, key in JOINS:
                if endpoint == own_table:
                    continue  # a table does not join onto itself
                if pattern.match(column) and (target := self.table(endpoint, key).get(str(value))):
                    alias = column[:-2] if column.endswith("Id") else column
                    joined[f"join:{alias}"] = target
                    break
        return {**raw, **joined}


def erp_sample(pair: rs.Pair, dbs: dict, limit: int) -> dict[tuple, dict]:
    rows: dict[tuple, dict] = {}
    width = len(pair.erp_key)
    columns = [rs.json_path(k) for k in pair.erp_key] + ["payload::text"]
    for source in pair.sources:
        sql = (
            f"select {', '.join(columns)} from vsv2.local_bronze_record "
            f"where endpoint = '{source}' order by md5(record_key) limit {limit}"
        )
        for line in rs.psql(dbs[source][0], sql):
            parts = line.split(rs.SEP, width)
            key = tuple(rs.clean(p) or None for p in parts[:width])
            if all(key):
                rows[key] = json.loads(parts[width])
    return rows


def dm_rows_for(
    pair: rs.Pair, dbs: dict, wanted: set[tuple], lookups: rs.Lookups, joiner: Joiner
) -> dict[tuple, dict]:
    db = dbs[pair.dm][0]
    found: dict[tuple, dict] = {}
    if pair.dm_key is not None:
        first = pair.dm_key[0]
        values = sorted({key[0] for key in wanted})
        for start in range(0, len(values), 1500):
            quoted = ",".join(
                "'" + v.replace("'", "''") + "'" for v in values[start : start + 1500]
            )
            width = len(pair.dm_key)
            sql = (
                f"select {', '.join(rs.json_path(k) for k in pair.dm_key)}, payload::text "
                f"from vsv2.local_bronze_record where endpoint = '{pair.dm}' "
                f"and {rs.json_path(first)} in ({quoted}){rs.filter_sql(pair.dm_filter)}"
            )
            for line in rs.psql(db, sql):
                parts = line.split(rs.SEP, width)
                key = tuple(rs.clean(p) or None for p in parts[:width])
                if key in wanted:
                    found[key] = joiner.augment(json.loads(parts[width]), pair.dm)
        return found
    normalizer = rs.REGISTRY.get(pair.normalizer or "")
    for line in rs.psql(
        db, f"select payload::text from vsv2.local_bronze_record where endpoint = '{pair.dm}'"
    ):
        raw = json.loads(line)
        if raw.get("deletedDatabaseRecord"):
            continue
        if pair.keyer is not None:
            key = tuple(rs.clean(rs._text(p)) or None for p in rs.KEYERS[pair.keyer](raw, lookups))
        elif normalizer is not None:
            try:
                shaped = normalizer(raw)
            except (KeyError, TypeError, ValueError):
                continue
            key = tuple(rs.clean(rs._text(rs.get_path(shaped, k))) or None for k in pair.erp_key)
        else:
            continue
        if key in wanted:
            found[key] = joiner.augment(raw, pair.dm)
    return found


def compare_pair(pair: rs.Pair, dbs: dict, lookups: rs.Lookups, joiner: Joiner) -> dict | str:
    missing = [n for n in (*pair.sources, pair.dm) if n not in dbs]
    if missing:
        return "no bronze data for " + ", ".join(missing)
    erp_rows = erp_sample(pair, dbs, SAMPLE_ROWS)
    dm_rows = dm_rows_for(pair, dbs, set(erp_rows), lookups, joiner)
    keys = sorted(set(erp_rows) & set(dm_rows))
    if len(keys) < 5:
        return f"only {len(keys)} shared rows in the sample"
    erp_cols = column_values([erp_rows[k] for k in keys], ERP_SYSTEM)
    dm_cols = column_values([dm_rows[k] for k in keys], DM_TECHNICAL)
    fields, dm_only = match_fields(erp_cols, dm_cols)
    return {"erp": pair.erp, "dm": pair.dm, "rows": len(keys), "fields": fields, "dm_only": dm_only}


def summarize(result: dict) -> Counter:
    return Counter(f["status"] for f in result["fields"])


def render(results: list[dict], skipped: list[tuple[str, str]]) -> str:
    lines = [
        "# Field parity: which ERP fields DataMart also carries",
        "",
        "> Generated by `scripts/field_parity.py` from the bronze rows in the scratch",
        "> `vsv2_ingest_*` databases. Do not edit by hand; re-run the script.",
        "",
        "Fields are matched by value on the rows both sources return (sample of up to "
        f"{SAMPLE_ROWS:,}). `in DM` = a DataMart field equals it on >= 98% of the filled rows; "
        "`tz` = equal up to a whole-hour offset; `partly` = 50-98%; `ERP only` = no DataMart "
        "field matches, so the warehouse must take it from ERP; `constant`/`nested` cannot be "
        "matched by value.",
        "",
        "| Entity | Shared rows | ERP fields filled | in DM | tz | partly | ERP only | constant | "
        "nested | DM-only fields |",
        "|---|---|---|---|---|---|---|---|---|---|",
    ]
    for r in sorted(results, key=lambda r: r["erp"]):
        c = summarize(r)
        filled = sum(n for s, n in c.items() if s != "always_empty")
        rows = f"{r['rows']:,}" + (" (low)" if r["rows"] < LOW_CONFIDENCE_ROWS else "")
        lines.append(
            f"| `{r['erp']}` | {rows} | {filled} | {c['in_dm']} | {c['tz_shift']} | "
            f"{c['partly']} | {c['erp_only']} | {c['constant']} | {c['nested']} | "
            f"{len(r['dm_only'])} |"
        )
    for r in sorted(results, key=lambda r: r["erp"]):
        lines += ["", f"## `{r['erp']}` vs `{r['dm']}` ({r['rows']:,} shared rows)", ""]
        groups: dict[str, list[str]] = {}
        for f in r["fields"]:
            status = f["status"]
            if status in ("in_dm", "tz_shift", "partly"):
                note = f"`{f['erp']}` = `{f['dm']}`"
                if status != "in_dm":
                    empty = f.get("dm_empty")
                    extra = f", DM empty on {empty:.0%}" if empty else ""
                    note += f" ({f['agreement']:.0%}{extra})"
                groups.setdefault(status, []).append(note)
            elif status == "constant":
                groups.setdefault("constant", []).append(f"`{f['erp']}` (always {f['value']!r})")
            elif status in ("erp_only", "nested"):
                groups.setdefault(status, []).append(f"`{f['erp']}`")
        titles = {
            "erp_only": "ERP only (take from ERP)",
            "partly": "Partly in DataMart",
            "tz_shift": "In DataMart up to a timezone shift",
            "in_dm": "In DataMart (equal)",
            "constant": "Constant in ERP",
            "nested": "Nested lists (ERP only, or separate DataMart tables)",
        }
        for status in ("erp_only", "partly", "tz_shift", "nested", "constant", "in_dm"):
            if groups.get(status):
                lines += [
                    f"**{titles[status]}** ({len(groups[status])}): " + "; ".join(groups[status]),
                    "",
                ]
        if r["dm_only"]:
            lines += [
                f"**DataMart only** ({len(r['dm_only'])}): "
                + ", ".join(f"`{p}`" for p in r["dm_only"]),
                "",
            ]
    lines += ["", "## Not compared", ""]
    lines += [f"- `{name}`: {why}" for name, why in skipped] or ["- none"]
    return "\n".join(lines) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--only", default="", help="comma-separated ERP endpoint names")
    args = ap.parse_args()
    only = {n for n in args.only.split(",") if n}

    configs = rs.load_configs()
    dbs = rs.locate(rs.scratch_databases())
    keyed, _ = rs.config_pairs(configs)
    pairs = [p for p in keyed + rs.EXPLICIT if not only or p.erp in only]
    lookups = rs.Lookups(dbs)
    joiner = Joiner(dbs)
    results, skipped = [], []
    seen: set[tuple[str, str]] = set()
    for pair in pairs:
        if (pair.erp, pair.dm) in seen:
            continue
        seen.add((pair.erp, pair.dm))
        outcome = compare_pair(pair, dbs, lookups, joiner)
        if isinstance(outcome, str):
            skipped.append((pair.erp, outcome))
            print(f"  skip  {pair.erp:32} {outcome}")
            continue
        results.append(outcome)
        c = summarize(outcome)
        dm_only = len(outcome["dm_only"])
        print(
            f"  {pair.erp:32} rows={outcome['rows']:>5} in_dm={c['in_dm']:>3} "
            f"partly={c['partly']:>3} erp_only={c['erp_only']:>3} dm_only={dm_only:>3}"
        )
    if not only:
        OUT_MD.write_text(render(results, skipped))
        OUT_JSON.write_text(json.dumps(results, indent=1, ensure_ascii=False) + "\n")
        print(f"wrote {OUT_MD.relative_to(ROOT)} and {OUT_JSON.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
