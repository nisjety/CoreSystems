"""Shared by the four v1 comparison tools: the --out, --json and --cutoff options, the finding shape,
and the like-for-like rules (the cutoff, the historical tenants, the primary keys).

A finding is one difference from v1 that a comparison tool reports. --json writes them as a list,
and parity_gate.py matches each one against parity_allowlist.json.

Fields of a finding:
  check     the tool that made it: compare_with_v1, compare_columns, compare_values, compare_bronze_rows
  dataset   the dataset in this project (v2). The same table name can exist in two datasets
            (silver and gold), so the allow-list can pin one.
  table     the table or view name
  metric    row_count | column_fill:<col> | sum:<col> | bronze_field:<col>, and these extra forms:
            column_presence:<col>  only one side has the column (note says which)
            not_compared           a bronze table the tool could not compare (note says why)
            not_comparable         a silver/gold table the tool could not compare (note says why)
            records_in_both        a bronze table with no record in common, so no field was checked
  v1, v2    the two numbers the tool compared; null where the tool has none
  diff_pct  signed change from v1 to v2 in percent of |v1|: (v2 - v1) / |v1| * 100.
            null when v1 is null or 0.
  note      optional: what the two numbers mean for this metric

Like-for-like cutoff. Where a table has a last-modified column on both sides (lastModifiedDateTime in
bronze, last_modified_at in silver and gold) and that column is set on every row on both sides, a
comparison counts only the rows last modified before the cutoff, on both sides. A table where a side
has no date, or some rows have none, is compared on all rows and its note says so. The cutoff is the
day after the staging snapshot's last load: v1 keeps syncing, so rows changed after it differ by timing.
The cutoff drops rows changed in v1 after the cutoff from v1's side only; a row changed in v1 after the
cutoff is still in v2 in its older state, so it is still counted in v2 (see docs/dataform_v1_comparison.md).

Historical tenants. Four tenants (HISTORICAL) stopped syncing in v1 in 2025 and v2 holds their whole
history. compare_values and compare_columns leave them out. compare_with_v1 and compare_bronze_rows
count them, so a v2 surplus on those tables is explained in parity_allowlist.json as a historical-tenant
difference.

Primary keys come from config/endpoints/<name>.json (primary_key). tenant_id is dropped, because bronze
keys on tenantId. A dotted key such as 'account.number' is a field inside the bronze column 'account',
which holds a JSON object; the key is JSON_VALUE(account, '$.number').
"""

from __future__ import annotations

import argparse
import json
from functools import lru_cache
from pathlib import Path

V2_ROOT = Path(__file__).resolve().parents[2]
ENDPOINTS = V2_ROOT / "config" / "endpoints"
HISTORICAL = (
    "f630d6b0-5566-11ee-960c-025417856183",
    "61320c0a-102d-11ee-a0e0-0693d8a7c3dd",
    "bb6c024e-5567-11ee-960c-025417856183",
    "9b1fa69b-5568-11ee-960c-025417856183",
)
HISTORICAL_SQL = ", ".join(f"'{t}'" for t in HISTORICAL)
DEFAULT_CUTOFF = "2026-10-01"
MODIFIED_COLUMNS = ("lastModifiedDateTime", "last_modified_at")


def add_cutoff_option(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--cutoff",
        default=DEFAULT_CUTOFF,
        metavar="DATE",
        help="like-for-like cutoff: compare rows last modified before DATE on both sides "
        "(default %(default)s; an empty string turns it off)",
    )


def modified_column(cols_new: dict, cols_old: dict) -> str | None:
    """The last-modified column both sides have, or None. Which one is set by MODIFIED_COLUMNS."""
    for name in MODIFIED_COLUMNS:
        if name in cols_new and name in cols_old:
            return name
    return None


def before_cutoff(column: str, cutoff: str) -> str:
    return f"SAFE_CAST(`{column}` AS TIMESTAMP) < TIMESTAMP '{cutoff}'"


def unset(column: str) -> str:
    return f"SAFE_CAST(`{column}` AS TIMESTAMP) IS NULL"


@lru_cache(maxsize=1)
def _keys_by_table() -> dict[str, list[str]]:
    keys: dict[str, list[str]] = {}
    for path in sorted(ENDPOINTS.glob("*.json")):
        if path.name.startswith("_"):
            continue
        config = json.loads(path.read_text())
        raw = config.get("primary_key")
        names = raw if isinstance(raw, list) else [k.strip() for k in str(raw or "").split(",")]
        names = [n for n in names if n and n != "tenant_id"]
        if names and config.get("bigquery_table"):
            keys[config["bigquery_table"]] = names
    return keys


def primary_key(table: str) -> list[str] | None:
    """The bronze primary key of a table (without tenantId), or None when the config has none.
    A dotted name is a field inside a JSON column: see key_expr."""
    return _keys_by_table().get(table)


def key_column(name: str) -> str:
    """The bronze column a key name lives in: 'account.number' -> 'account'."""
    return name.split(".", 1)[0]


def key_expr(name: str, alias: str | None = None) -> str:
    """SQL for one key part: a plain column, or the field of a JSON column ('account.number' ->
    JSON_VALUE(account, '$.number')). alias, if given, qualifies the column (n.account)."""
    prefix = f"{alias}." if alias else ""
    if "." not in name:
        return f"{prefix}`{name}`"
    column, field = name.split(".", 1)
    return f"JSON_VALUE({prefix}`{column}`, '$.{field}')"


def key_sql(keys: list[str], alias: str | None = None) -> str:
    """One string per record: tenant and key. Two records with the same string are the same record."""
    prefix = f"{alias}." if alias else ""
    parts = ", ".join(key_expr(k, alias) for k in keys)
    return f"TO_JSON_STRING(STRUCT({prefix}tenantId, {parts}))"


def keys_present(keys: list[str], cols_a: dict, cols_b: dict) -> bool:
    """Every key column exists on both sides, and tenantId too."""
    return (
        "tenantId" in cols_a
        and "tenantId" in cols_b
        and all(key_column(k) in cols_a and key_column(k) in cols_b for k in keys)
    )


LOCATION = "europe-north2"
HISTORICAL_TENANT_CONDITION = f"tenant_id NOT IN ({HISTORICAL_SQL})"


def where_sql(conditions: list[str | None]) -> str:
    live = [c for c in conditions if c]
    return f" WHERE {' AND '.join(live)}" if live else ""


def columns_by_table(client, project: str, dataset: str) -> dict[str, dict[str, str]]:
    """Column name -> data type, for every table and view of one dataset, in column order."""
    sql = (
        f"SELECT table_name, column_name, data_type FROM `{project}.{dataset}.INFORMATION_SCHEMA.COLUMNS` "
        "ORDER BY table_name, ordinal_position"
    )
    out: dict[str, dict[str, str]] = {}
    for row in client.query(sql, location=LOCATION).result():
        out.setdefault(row.table_name, {})[row.column_name] = row.data_type
    return out


def scope_counts(
    client,
    project: str,
    dataset: str,
    table: str,
    modified: str | None,
    cutoff: str,
    tenant: str | None,
) -> dict | None:
    """Rows of a table in the population (tenant filter only), how many have no date, how many are
    before the cutoff. None when the table cannot be read. Used to decide whether the cutoff applies."""
    parts = ["COUNT(*) AS n"]
    if modified:
        parts += [
            f"COUNTIF({unset(modified)}) AS unset",
            f"COUNTIF({before_cutoff(modified, cutoff)}) AS before",
        ]
    sql = f"SELECT {', '.join(parts)} FROM `{project}.{dataset}.{table}`{where_sql([tenant])}"
    try:
        row = next(iter(client.query(sql, location=LOCATION).result()))
    except Exception:
        return None
    return dict(row.items())


def cutoff_applies(modified: str | None, cutoff: str, *stats: dict | None) -> bool:
    """The cutoff is used only when both sides have the date column and every row on both sides has
    a date. Otherwise the comparison covers all rows and the report says why."""
    return bool(modified and cutoff and all(s is not None and s.get("unset") == 0 for s in stats))


def add_output_options(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--out",
        metavar="DIR",
        help="write the markdown report into DIR instead of docs/ (the default)",
    )
    parser.add_argument(
        "--json",
        metavar="PATH",
        help="also write the differences from v1 as a JSON list of findings",
    )


def report_path(out_dir: str | None, default: Path) -> Path:
    """Where the markdown report goes: the default under docs/, or DIR/<same name>."""
    if not out_dir:
        return default
    folder = Path(out_dir)
    folder.mkdir(parents=True, exist_ok=True)
    return folder / default.name


def pct_change(v1, v2) -> float | None:
    if v1 in (None, 0) or v2 is None:
        return None
    return 100 * (v2 - v1) / abs(v1)


def finding(check, dataset, table, metric, v1, v2, note=None) -> dict:
    return {
        "check": check,
        "dataset": dataset,
        "table": table,
        "metric": metric,
        "v1": v1,
        "v2": v2,
        "diff_pct": pct_change(v1, v2),
        "note": note,
    }


def write_findings(path: str | None, findings: list[dict]) -> None:
    if not path:
        return
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(findings, indent=2, ensure_ascii=False) + "\n")
