#!/usr/bin/env python3
"""Reconcile what ERP REST returns against what DataMart returns, entity by entity.

The warehouse must hold all Visma data, and neither API returns it all: ERP REST never returns
deleted rows and hides some live ones, DataMart has fewer fields. This compares the bronze rows
the ingest probes landed in the scratch `vsv2_ingest_*` databases, by business key, and says for
each entity whether the two sources agree:

  agree        same keys; any DataMart-only rows are tombstones (deletedDatabaseRecord = true)
  erp_hides    DataMart has live rows ERP REST does not return
  dm_lacks     ERP has rows DataMart does not (split into "newer than DataMart's newest row",
               which is only a timing gap, and the rest)

For the rows one side hides, it names the field values that separate them from the shared rows
(e.g. `contactType = Lead` on 24,000 of 25,000), which is where the explanation starts.

The keys of DataMart rows come from the registered normalizer (which reshapes them to the ERP
contract) or from an explicit field list. Nothing is fetched from Visma: run the ingest probes
first. Writes docs/reconciliation.md.

Usage:
  scripts/reconcile_sources.py
  scripts/reconcile_sources.py --only contacts,locations
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "docs/reconciliation.md"
sys.path.insert(0, str(ROOT / "scripts"))
sys.path.insert(0, str(ROOT / "tooling/src"))

from endpoint_matrix import live_status, load_configs  # noqa: E402

from vsv2_tooling.normalizers import REGISTRY  # noqa: E402

CONTAINER = "visma_service_v2-postgres-1"
SEP = "\x1f"
MAX_PAYLOAD_ROWS = 60_000  # above this DataMart payloads are not pulled for the explanation
DISTINCT_LIMIT = 20
SAMPLE = 5

Key = tuple[str | None, ...]


@dataclass(frozen=True)
class Pair:
    erp: str
    dm: str
    erp_key: list[str]
    dm_key: list[str] | None = None  # explicit DataMart key fields; None = use the normalizer
    normalizer: str | None = None
    dm_filter: dict[str, list[str]] = field(default_factory=dict)
    note: str = ""
    keyer: str | None = None  # named function that keys a DataMart row using lookups
    erp_sources: tuple[str, ...] = ()  # ERP endpoints whose union is compared; default (erp,)

    @property
    def sources(self) -> tuple[str, ...]:
        return self.erp_sources or (self.erp,)


# Pairs the endpoint configs do not declare, or whose DataMart side has no registered normalizer.
# The ERP list DTOs for invoices, credit notes and so on are all views of DataMart's
# `customerDocuments`, split by documentType.
EXPLICIT: list[Pair] = [
    Pair("payment_methods", "dm_payment_methods", ["paymentMethodID"], ["paymentMethodId"]),
    Pair("supplier_classes", "dm_supplier_classes", ["id"], ["supplierClassId"]),
    Pair(
        "supplier_documents",
        "dm_supplier_documents",
        ["documentType", "referenceNumber"],
        ["documentType", "referenceNumber"],
    ),
    Pair(
        "supplier_invoices",
        "dm_supplier_documents",
        ["documentType", "referenceNumber"],
        ["documentType", "referenceNumber"],
        note="`supplierInvoice` is the same documents as `supplierdocument` in another DTO",
    ),
    Pair("countries", "dm_countries", ["id"], ["countryId"]),
    Pair("sales_orders_v3", "dm_sales_orders", ["type", "orderId"], ["orderType", "orderNbr"]),
    Pair(
        "locations",
        "dm_locations",
        ["baccount.number", "locationId"],
        keyer="location",
        erp_sources=("locations", "supplier_locations"),
        note="DataMart `locations` holds customer and supplier locations; ERP splits them in two "
        "endpoints. The business-account number is looked up from DataMart customers and suppliers",
    ),
    Pair(
        "general_ledger_balances_v2",
        "dm_general_ledger_balances_v2",
        ["account.number", "subAccountCd", "financialPeriod", "ledger.id"],
        keyer="gl_balance",
        note="DataMart holds internal account, subaccount and ledger ids; account and subaccount "
        "codes are looked up from DataMart accounts and subaccounts, and the ledger is mapped "
        "because each side has exactly one",
    ),
    Pair(
        "general_ledger_transactions",
        "dm_journal_transactions",
        ["batchNumber", "lineNumber"],
        ["batchNbr", "lineNbr"],
        note="posted ledger lines; DataMart calls them journal transactions",
    ),
    *[
        Pair(
            erp,
            "dm_customer_documents",
            ["referenceNumber"],
            ["referenceNumber"],
            dm_filter={"documentType": [doc_type]},
            note=f"DataMart `customerDocuments` where documentType = {doc_type}",
        )
        for erp, doc_type in (
            ("customer_invoices", "Invoice"),
            ("customer_credit_notes", "CreditNote"),
            ("customer_debit_notes", "DebitNote"),
            ("customer_credit_write_offs", "SmallCreditWo"),
        )
    ],
]

# Config pairs to leave out: the ERP side is a different grain from DataMart's.
SKIP_CONFIG_PAIRS = {
    "sales_orders_v3",
    "locations",
    "general_ledger_balances_v2",
    "payment_methods",
    "supplier_classes",
    "supplier_documents",
    "supplier_invoices",
}


# What was measured about each discrepancy on 2026-09-30, kept next to the numbers the script
# prints so the report explains itself. Update when the data or the API behaviour changes.
MEASURED_2026_09_30: dict[str, str] = {
    "contacts": (
        "The extra rows are 'Business Account' contacts (the company contact every customer, "
        "supplier and lead has). ERP `/v1/contact` lists person contacts only; `active=true` and "
        "`active=false` together equal the default list, and `GET /v1/contact/{id}` answers 404 "
        "for the hidden ids. They carry data (full name on 25,043, phone on 12,320, email on "
        "6,496). DataMart is the only source."
    ),
    "customer_sales_prices": (
        "ERP lists the prices in force today (`priceType=All` changes nothing). Of the 2,048 "
        "hidden rows 1,356 have an expiration date in the past; `effectiveAsOf` set to a day "
        "inside such a price's window did not return it. The other 692 have no expiry and "
        "nothing in the data separates them from the shared rows; they are probably superseded "
        "prices but that is not verified. DataMart is the only source."
    ),
    "supplier_payments": (
        "All 31 are status Printed, released = false, never posted. Filtering ERP by each of "
        "the 12 documented statuses returns 9,135 rows in total and none of them, and "
        "`GET /v1/supplierPayment/{nr}` answers 404. DataMart is the only source."
    ),
    "payment_methods": (
        "ERP `/v1/paymentmethod` is the supplier (AP) list. The 5 hidden methods have "
        "useInAP = false, but `GET /v1/paymentmethod/{id}` returns them, so a by-id call per "
        "DataMart id (5 calls) recovers them through ERP too."
    ),
    "customer_payments": (
        "A page-boundary sort tie: `Payment 408853` and `VoidPayment 408853` share a reference "
        "number, and the walk returned one of them twice. A second walk recovers it. The "
        "completeness guard proposed for the executor would catch this."
    ),
    "locations": (
        "Once ERP's `locations` and `supplier_locations` are combined only 114 DataMart rows "
        "remain: 113 locations of business accounts that are neither customer nor supplier (112 "
        "named 'Hovedlokasjon'), which no ERP endpoint lists, and 1 live location (320428/HS) "
        "updated in DataMart on 2026-09-29 that ERP did not return."
    ),
}

SKIP_REASONS = {
    "budgets": "the tenant has no budget rows in either source",
}


def snake(name: str) -> str:
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


def psql(db: str, sql: str) -> list[str]:
    result = subprocess.run(
        [
            "docker",
            "exec",
            CONTAINER,
            "psql",
            "-U",
            "visma_v2",
            "-At",
            "-F",
            SEP,
            "-d",
            db,
            "-c",
            sql,
        ],
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout.splitlines()


def scratch_databases() -> list[str]:
    rows = psql("postgres", "select datname from pg_database where datname like 'vsv2_ingest_%'")
    return sorted(rows)


def locate(dbs: list[str]) -> dict[str, tuple[str, int]]:
    """endpoint -> (database holding the most rows for it, that row count)."""
    best: dict[str, tuple[str, int]] = {}
    for db in dbs:
        sql = "select endpoint, count(*) from vsv2.local_bronze_record group by 1"
        try:
            lines = psql(db, sql)
        except subprocess.CalledProcessError:
            continue  # a scratch database without the bronze table
        for line in lines:
            endpoint, count = line.split(SEP)
            if int(count) > best.get(endpoint, ("", 0))[1]:
                best[endpoint] = (db, int(count))
    return best


def json_path(path: str) -> str:
    return "payload #>> '{" + path.replace(".", ",") + "}'"


def clean(value: str | None) -> str | None:
    return value.strip() if value else None


def get_path(record: Any, path: str) -> Any:
    for part in path.split("."):
        if not isinstance(record, dict) or part not in record:
            return None
        record = record[part]
    return record


@dataclass
class Side:
    keys: dict[Key, dict[str, Any]] = field(default_factory=dict)
    keyless: int = 0
    rows: int = 0


class Lookups:
    """DataMart internal id -> ERP code maps, built once from the DataMart bronze rows."""

    def __init__(self, dbs: dict[str, tuple[str, int]]) -> None:
        self.dbs = dbs
        self._maps: dict[tuple[str, str, str], dict[str, str]] = {}
        self._single: dict[tuple[str, ...], dict[str, str]] = {}

    def map(self, endpoint: str, from_field: str, to_field: str) -> dict[str, str]:
        cache_key = (endpoint, from_field, to_field)
        if cache_key not in self._maps:
            found: dict[str, str] = {}
            if endpoint in self.dbs:
                sql = (
                    f"select {json_path(from_field)}, {json_path(to_field)} "
                    f"from vsv2.local_bronze_record where endpoint = '{endpoint}'"
                )
                for line in psql(self.dbs[endpoint][0], sql):
                    source, _, target = line.partition(SEP)
                    if source and target:
                        found[source] = target
            self._maps[cache_key] = found
        return self._maps[cache_key]

    def distinct(self, endpoint: str, path: str) -> set[str]:
        if endpoint not in self.dbs:
            return set()
        sql = f"select distinct {json_path(path)} from vsv2.local_bronze_record "
        return {v for v in psql(self.dbs[endpoint][0], sql + f"where endpoint = '{endpoint}'") if v}

    def single_value_map(self, dm: tuple[str, str], erp: tuple[str, str]) -> dict[str, str]:
        """{dm value: erp value} when each side has exactly one distinct value, else empty."""
        cache_key = (*dm, *erp)
        if cache_key not in self._single:
            left, right = self.distinct(*dm), self.distinct(*erp)
            one = len(left) == len(right) == 1
            self._single[cache_key] = {next(iter(left)): next(iter(right))} if one else {}
        return self._single[cache_key]


def key_location(raw: dict, lookups: Lookups) -> Key:
    owners = lookups.map("dm_customers", "customerId", "customerNumber") | lookups.map(
        "dm_suppliers", "supplierId", "supplierCd"
    )
    return (owners.get(_show(raw.get("bAccountId"))), raw.get("locationCd"))


def key_gl_balance(raw: dict, lookups: Lookups) -> Key:
    accounts = lookups.map("dm_accounts", "accountId", "accountCd")
    subaccounts = lookups.map("dm_subaccounts", "subaccountId", "subaccountCd")
    ledgers = lookups.single_value_map(
        ("dm_general_ledger_balances_v2", "ledgerId"), ("general_ledger_balances_v2", "ledger.id")
    )
    return (
        accounts.get(_show(raw.get("accountId"))),
        subaccounts.get(_show(raw.get("subaccountId"))),
        raw.get("finPeriodId"),
        ledgers.get(_show(raw.get("ledgerId"))),
    )


KEYERS = {"location": key_location, "gl_balance": key_gl_balance}


def filter_sql(dm_filter: dict[str, list[str]]) -> str:
    clauses = []
    for path, values in dm_filter.items():
        quoted = ",".join("'" + v.replace("'", "''") + "'" for v in values)
        clauses.append(f"{json_path(path)} in ({quoted})")
    return (" and " + " and ".join(clauses)) if clauses else ""


def load_erp(db: str, endpoint: str, pair: Pair, modified_field: str | None, side: Side) -> Side:
    columns = [json_path(k) for k in pair.erp_key]
    columns.append(json_path(modified_field) if modified_field else "null")
    sql = f"select {', '.join(columns)} from vsv2.local_bronze_record where endpoint = '{endpoint}'"
    for line in psql(db, sql):
        parts = line.split(SEP)
        key = tuple(clean(p) or None for p in parts[: len(pair.erp_key)])
        side.rows += 1
        if any(part is None for part in key):
            side.keyless += 1
            continue
        side.keys[key] = {"modified": clean(parts[-1])}
    return side


def load_dm(db: str, pair: Pair, erp_keys: list[str], total: int, lookups: Lookups) -> Side:
    """DataMart rows keyed like the ERP side; payloads kept when the entity is small enough."""
    deleted = json_path("deletedDatabaseRecord")
    modified = json_path("lastModifiedDateTime")
    where = f"endpoint = '{pair.dm}'{filter_sql(pair.dm_filter)}"
    normalizer = REGISTRY.get(pair.normalizer or "")
    side = Side()
    keep_payload = total <= MAX_PAYLOAD_ROWS
    if pair.dm_key is not None:
        columns = [json_path(k) for k in pair.dm_key] + [deleted, modified]
        if keep_payload:
            columns.append("payload::text")
        for line in psql(
            db, f"select {', '.join(columns)} from vsv2.local_bronze_record where {where}"
        ):
            parts = line.split(SEP, len(columns) - 1)
            n = len(pair.dm_key)
            key = tuple(clean(p) or None for p in parts[:n])
            side.rows += 1
            if any(part is None for part in key):
                side.keyless += 1
                continue
            side.keys[key] = {
                "deleted": parts[n] == "true",
                "modified": clean(parts[n + 1]),
                "payload": json.loads(parts[n + 2]) if keep_payload else None,
            }
        return side
    assert normalizer is not None or pair.keyer is not None
    for line in psql(db, f"select payload::text from vsv2.local_bronze_record where {where}"):
        raw = json.loads(line)
        side.rows += 1
        if pair.keyer is not None:
            key = tuple(clean(_text(part)) or None for part in KEYERS[pair.keyer](raw, lookups))
        else:
            try:
                shaped = normalizer(raw)
            except (KeyError, TypeError, ValueError):
                side.keyless += 1
                continue
            key = tuple(clean(_text(get_path(shaped, k))) or None for k in erp_keys)
        if any(part is None for part in key):
            side.keyless += 1
            continue
        side.keys[key] = {
            "deleted": bool(raw.get("deletedDatabaseRecord")),
            "modified": clean(raw.get("lastModifiedDateTime")),
            "payload": raw if keep_payload else None,
        }
    return side


def _text(value: Any) -> str | None:
    return None if value is None else str(value)


def discriminators(only: list[dict], common: list[dict]) -> list[str]:
    """Field values that separate `only` rows from `common` rows, strongest first."""
    payloads_only = [r["payload"] for r in only if r.get("payload")]
    payloads_common = [r["payload"] for r in common if r.get("payload")]
    if not payloads_only or not payloads_common:
        return []
    fields = [
        k
        for k, v in payloads_only[0].items()
        if not isinstance(v, (dict, list))
        and not k.startswith("_")
        and k not in ("id", "tenant_id")
    ]
    found: list[tuple[float, str]] = []
    for name in fields:
        count_only = Counter(_show(p.get(name)) for p in payloads_only)
        if len(count_only) > DISTINCT_LIMIT:
            continue
        count_common = Counter(_show(p.get(name)) for p in payloads_common)
        for value, n in count_only.items():
            share_only = n / len(payloads_only)
            share_common = count_common.get(value, 0) / len(payloads_common)
            if share_only >= 0.5 and share_only - share_common >= 0.3:
                found.append(
                    (
                        share_only - share_common,
                        f"`{name} = {value}` on {n:,} of {len(payloads_only):,}"
                        f" (shared rows: {share_common:.0%})",
                    )
                )
    return [text for _, text in sorted(found, reverse=True)[:4]]


def _show(value: Any) -> str:
    return "null" if value is None else str(value)[:40]


def newer_than(modified: str | None, cutoff: str | None) -> bool:
    if not modified or not cutoff:
        return False
    return modified.rstrip("Z") > cutoff.rstrip("Z")


@dataclass
class Result:
    pair: Pair
    erp_rows: int
    dm_rows: int
    dm_deleted: int
    common: int
    only_erp: list[Key]
    only_erp_fresh: int
    only_dm_live: list[Key]
    only_dm_deleted: int
    keyless_erp: int
    keyless_dm: int
    explain_dm: list[str]
    verdict: str


def compare(
    pair: Pair,
    dbs: dict[str, tuple[str, int]],
    modified_fields: dict[str, str | None],
    lookups: Lookups,
) -> Result | str:
    missing = [n for n in (*pair.sources, pair.dm) if n not in dbs]
    if missing:
        return "no bronze data for " + ", ".join(missing)
    erp = Side()
    for source in pair.sources:
        load_erp(dbs[source][0], source, pair, modified_fields.get(source), erp)
    dm_db, dm_total = dbs[pair.dm]
    dm = load_dm(dm_db, pair, pair.erp_key, dm_total, lookups)
    erp_keys, dm_keys = set(erp.keys), set(dm.keys)
    common = erp_keys & dm_keys
    only_dm = dm_keys - erp_keys
    only_dm_live = sorted(k for k in only_dm if not dm.keys[k]["deleted"])
    only_erp = sorted(erp_keys - dm_keys)
    dm_newest = max((v["modified"] for v in dm.keys.values() if v["modified"]), default=None)
    fresh = sum(1 for k in only_erp if newer_than(erp.keys[k]["modified"], dm_newest))
    explain = discriminators([dm.keys[k] for k in only_dm_live], [dm.keys[k] for k in common])
    real_only_erp = len(only_erp) - fresh
    if only_dm_live and real_only_erp:
        verdict = "erp_hides + dm_lacks"
    elif only_dm_live:
        verdict = "erp_hides"
    elif real_only_erp:
        verdict = "dm_lacks"
    else:
        verdict = "agree"
    return Result(
        pair, erp.rows, dm.rows, sum(1 for v in dm.keys.values() if v["deleted"]), len(common),
        only_erp, fresh, only_dm_live, len(only_dm) - len(only_dm_live), erp.keyless, dm.keyless,
        explain, verdict,
    )  # fmt: skip


def config_pairs(configs: dict[str, dict]) -> tuple[list[Pair], list[Pair]]:
    """(pairs the configs declare and a normalizer can key, declared pairs it cannot key)."""
    pairs, unmapped = [], []
    for name, cfg in configs.items():
        root = (cfg.get("datamart") or {}).get("root_field")
        if cfg["source"] != "erp_rest" or not root or name in SKIP_CONFIG_PAIRS:
            continue
        normalizer = cfg["datamart"].get("join_normalizer")
        pk = cfg["primary_key"]
        pair = Pair(
            name, "dm_" + snake(root), pk if isinstance(pk, list) else [pk], None, normalizer
        )
        (pairs if normalizer in REGISTRY else unmapped).append(pair)
    return pairs, unmapped


def shared_urls(configs: dict[str, dict]) -> dict[str, list[str]]:
    """api_endpoint -> configs reading it, when more than one does."""
    found: dict[str, list[str]] = {}
    for name, cfg in configs.items():
        url = (cfg.get("api_endpoint") or "").lower()
        if url:
            found.setdefault(url, []).append(name)
    return {url: names for url, names in found.items() if len(names) > 1}


def render(results: list[Result], skipped: list[tuple[Pair, str]], dbs: dict, configs: dict) -> str:
    status = live_status()
    lines = [
        "# Source reconciliation: ERP REST vs DataMart",
        "",
        "> Generated by `scripts/reconcile_sources.py` from the bronze rows in the scratch",
        "> `vsv2_ingest_*` databases. Do not edit by hand; re-run the script.",
        "",
        "Compared by business key. `DM deleted` are DataMart tombstones ERP REST never returns.",
        "`fresh` are ERP-only rows modified after DataMart's newest row: a timing gap, not a hole.",
        "",
        "| Entity (ERP) | DataMart | ERP rows | DM rows | DM deleted | Shared | ERP-only "
        "| fresh | DM-only live | Verdict |",
        "|---|---|---|---|---|---|---|---|---|---|",
    ]
    order = {"erp_hides + dm_lacks": 0, "erp_hides": 1, "dm_lacks": 2, "agree": 3}
    for r in sorted(results, key=lambda r: (order[r.verdict], -len(r.only_dm_live), r.pair.erp)):
        lines.append(
            f"| `{r.pair.erp}` | `{r.pair.dm}` | {r.erp_rows:,} | {r.dm_rows:,} "
            f"| {r.dm_deleted:,} | {r.common:,} | {len(r.only_erp):,} | {r.only_erp_fresh:,} "
            f"| {len(r.only_dm_live):,} | **{r.verdict}** |"
        )
    counts = Counter(r.verdict for r in results)
    lines += ["", "Verdicts: " + ", ".join(f"{n} {v}" for v, n in sorted(counts.items())), ""]

    for r in results:
        if r.verdict == "agree" and not r.pair.note and not (r.keyless_erp or r.keyless_dm):
            continue
        lines += [f"## `{r.pair.erp}` vs `{r.pair.dm}`: {r.verdict}", ""]
        if r.pair.note:
            lines += [r.pair.note + ".", ""]
        if r.keyless_erp or r.keyless_dm:
            lines += [
                f"Rows without a usable key: ERP {r.keyless_erp:,}, DataMart {r.keyless_dm:,}.",
                "",
            ]
        if r.only_dm_live:
            lines.append(
                f"- **{len(r.only_dm_live):,} live DataMart rows ERP REST does not return.** "
                f"Sample keys: {', '.join('/'.join(k) for k in r.only_dm_live[:SAMPLE])}"
            )
            lines += [f"  - separates them from the shared rows: {text}" for text in r.explain_dm]
        real = len(r.only_erp) - r.only_erp_fresh
        if real:
            lines.append(
                f"- **{real:,} ERP rows DataMart lacks** (not explained by timing). "
                f"Sample keys: {', '.join('/'.join(k) for k in r.only_erp[:SAMPLE])}"
            )
        if r.only_erp_fresh:
            lines.append(f"- {r.only_erp_fresh:,} ERP rows are newer than DataMart's newest row.")
        if r.only_dm_deleted:
            lines.append(f"- {r.only_dm_deleted:,} DataMart-only rows are tombstones.")
        if r.pair.erp in MEASURED_2026_09_30:
            lines += ["", f"**Measured 2026-09-30:** {MEASURED_2026_09_30[r.pair.erp]}"]
        lines.append("")

    paired_erp = {s for r in results for s in r.pair.sources} | {
        s for p, _ in skipped for s in p.sources
    }
    paired_dm = {r.pair.dm for r in results} | {p.dm for p, _ in skipped}
    lines += ["## Not comparable yet", ""]
    lines += [f"- `{p.erp}` vs `{p.dm}`: {SKIP_REASONS.get(p.erp, why)}" for p, why in skipped] or [
        "- none"
    ]
    duplicates = shared_urls(configs)
    lines += ["", "## Configs that read the same URL", ""]
    lines += [
        f"- `{url}`: {', '.join(f'`{n}`' for n in names)}" for url, names in duplicates.items()
    ] or ["- none"]
    lines += ["", "## Only one source", "", "ERP REST only (no DataMart counterpart):", ""]
    erp_only = sorted(
        n for n, c in configs.items() if c["source"] != "datamart_graphql" and n not in paired_erp
    )
    for n in erp_only:
        state = status.get(n, ("untested", 0))[0]
        rows = dbs.get(n, ("", 0))[1]
        lines.append(f"- `{n}`: {rows:,} rows in bronze ({state})")
    lines += ["", "DataMart only (no ERP counterpart):", ""]
    for n in sorted(
        x for x, c in configs.items() if c["source"] == "datamart_graphql" and x not in paired_dm
    ):
        rows = dbs.get(n, ("", 0))[1]
        lines.append(f"- `{n}`: {rows:,} rows in bronze ({status.get(n, ('untested', 0))[0]})")
    return "\n".join(lines) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--only", default="", help="comma-separated ERP endpoint names")
    args = ap.parse_args()
    only = {n for n in args.only.split(",") if n}

    configs = load_configs()
    dbs = locate(scratch_databases())
    keyed, unmapped = config_pairs(configs)
    pairs = [p for p in keyed + EXPLICIT if not only or p.erp in only]
    results: list[Result] = []
    skipped: list[tuple[Pair, str]] = []
    for pair in unmapped:
        if only and pair.erp not in only:
            continue
        dm_rows = dbs.get(pair.dm, ("", 0))[1]
        why = (
            "DataMart holds no rows for it (its ETL is not delivering), so ERP is the only source"
            if dm_rows == 0
            else "no normalizer maps DataMart rows to ERP keys"
        )
        skipped.append((pair, why))
    modified_fields = {n: c.get("modified_field") for n, c in configs.items()}
    lookups = Lookups(dbs)
    for pair in pairs:
        outcome = compare(pair, dbs, modified_fields, lookups)
        if isinstance(outcome, str):
            skipped.append((pair, outcome))
            print(f"  skip  {pair.erp:32} {outcome}")
        else:
            results.append(outcome)
            erp_n, dm_n = outcome.erp_rows, outcome.dm_rows
            print(f"  {outcome.verdict:22} {pair.erp:32} erp={erp_n:>7,} dm={dm_n:>7,}")
    if not only:
        OUT.write_text(render(results, skipped, dbs, configs))
        print(f"wrote {OUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
