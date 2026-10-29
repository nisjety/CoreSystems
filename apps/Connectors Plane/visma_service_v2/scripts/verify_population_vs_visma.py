#!/usr/bin/env python3
"""Population gate: the warehouse's open invoices against Visma's own list of open invoices.

A watermark sync (documents modified since the last run) cannot see a document that stopped being
open without its timestamp moving: a payment that closes an invoice does not touch
lastModifiedDateTime. A key-level spot check cannot see a missing document at all. So this compares
the whole open population of each tenant, customer and supplier invoices, and classifies each
difference against the last sync of that tenant and endpoint. docs/gates/README.md has the rules in
full; this is the short form.

Verdict per tenant. Every reason the tenant failed is listed (error, stale, defect):
  ERROR    error: the gate cannot give a reliable answer (ERROR guards, no token, a bad response)
  FAIL     stale and/or defect, and no error
  PENDING  no reason to fail, but differences newer than the last sync (the warehouse is behind)
  PASS     no differences
Exit code 0 only when no tenant has a reason, that is every tenant is PASS or PENDING.

Differences, against the last sync (Visma's lastModifiedDateTime compared with it):
  PENDING  open_at_visma_pending, closed_at_visma_pending, balance_pending: modified after the sync
  DEFECT   missed_or_stale, closed_without_timestamp, balance_stale: modified at or before the sync
  DEFECT   gone_at_visma: open in the warehouse, 404 in Visma. A deletion has no timestamp that a
           later sync could catch up with, so it is a DEFECT whatever the time of the sync.
  A balance off by exactly 0.01 is cent drift: counted, never a verdict.

ERROR guards (the tenant gets reason error): a warehouse-open key that Visma returns Open by key
but that is missing from the open list; a by-key response without status or lastModifiedDateTime;
a duplicate key in the open list or in the warehouse open set; a null balance or key, or a missing
required field, from Visma; a null balance or key in the warehouse; a list page that is not a list
or holds a row that is not Open; no token for the tenant.

Last sync: MAX(finished_at) per tenant and endpoint of the heartbeat table `_sync_heartbeat` in the
staging dataset, status 'succeeded'. vsv2-bq-stage writes it after each successful endpoint write
(bigquery/heartbeat.py). A last sync older than --max-age-hours, or none on record, makes the
endpoint stale. With no sync on record every difference with a timestamp is PENDING, because
nothing is measured against a sync that is not on record. Source v1 writes no heartbeat, so it is
stale for every tenant.

Live side, GET only: /v1/customerinvoice and /v1/supplierInvoice with status=Open, pageNumber and
pageSize=1000, read until an empty page; one GET by key of the first open row; one GET by key for
each warehouse-open document that is not open in Visma. One Limiter (PACER) paces every request of
the run, across all tenants: the next request starts PACE_SECONDS after the previous response
returned. Requests are counted and written to the report.

Warehouse side, read only: fact_customer_invoice and fact_supplier_invoice with status 'Open'.

--source v1 reads prj-dw-dev (dw_1_silver_visma) read-only: the negative control, which is known to
hold invoices closed in Visma. It writes no heartbeat, so it is stale, and its timestamped
differences are PENDING until a heartbeat exists for it.

Usage: scripts/verify_population_vs_visma.py [--source v2|v1] [--max-age-hours 26]
           [--date YYYY-MM-DD] [--tenants id,id]
Writes docs/gates/population_<source>_<date>[_subset].md and the same data as .json next to it.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import Counter
from collections.abc import Callable
from dataclasses import dataclass, field
from datetime import UTC, date, datetime
from decimal import ROUND_HALF_UP, Decimal
from pathlib import Path
from typing import Any, TypeVar

from google.api_core.exceptions import NotFound

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))

import tenant_scan_compare as tenants  # noqa: E402
from google.cloud import bigquery  # noqa: E402
from verify_bq_vs_visma import BASE, token  # noqa: E402

T = TypeVar("T")
GATES = ROOT / "docs/gates"
QUERY_PROJECT = "aquatiq-datavarehus"  # query jobs run here; prj-dw-dev tables are only read
LOCATION = "europe-north2"
APPLICATION_TYPE = "Visma.net Financials"
PACE_SECONDS = 0.3
PAGE_SIZE = 1000
MAX_PAGES = 200
RETRY_WAITS = (5, 15, 45)
TRANSIENT_CODES = {429, 500, 502, 503, 504}
REQUIRED_FIELDS = (
    "referenceNumber",
    "documentType",
    "balance",
    "status",
    "lastModifiedDateTime",
)
CENT = Decimal("0.01")
EPOCH = datetime.min.replace(tzinfo=UTC)
SAMPLE_KEYS = 20
HEARTBEAT_STATUS = "succeeded"

Key = tuple[str, ...]


class Limiter:
    """The one pace for every Visma request in a run, across all tenants.

    The next request waits until `pace_seconds` have passed since the previous response returned
    (or failed). Requests run one at a time, so no tenant has a timer of its own. `calls` counts
    requests by kind ("GET", "token") for the report.
    """

    def __init__(
        self,
        pace_seconds: float = PACE_SECONDS,
        *,
        clock: Callable[[], float] = time.monotonic,
        sleep: Callable[[float], None] = time.sleep,
    ) -> None:
        self.pace_seconds = pace_seconds
        self.calls: Counter[str] = Counter()
        self._clock = clock
        self._sleep = sleep
        self._ready_at = 0.0

    def run(self, kind: str, action: Callable[[], T]) -> T:
        wait = self._ready_at - self._clock()
        if wait > 0:
            self._sleep(wait)
        self.calls[kind] += 1
        try:
            return action()
        finally:
            self._ready_at = self._clock() + self.pace_seconds


PACER = Limiter()


def fetch_token(tenant: str) -> str:
    """A bearer token from `vsv2 token`, paced like every other Visma request. Never printed."""
    return PACER.run("token", lambda: token(tenant))


@dataclass(frozen=True)
class Source:
    name: str
    customer_table: str  # status, reference_number, balance
    supplier_table: str  # status, document_type, reference_number, balance
    heartbeat_table: str | None  # tenant_id, endpoint, finished_at, status; None: no heartbeat


SOURCES = {
    "v2": Source(
        "v2",
        "aquatiq-datavarehus.visma_silver.fact_customer_invoice",
        "aquatiq-datavarehus.visma_silver.fact_supplier_invoice",
        "aquatiq-datavarehus.dw_staging_visma._sync_heartbeat",
    ),
    "v1": Source(
        "v1",
        "prj-dw-dev.dw_1_silver_visma.fact_customer_invoice",
        "prj-dw-dev.dw_1_silver_visma.fact_supplier_invoice",
        None,
    ),
}


def _quote(text: str) -> str:
    return urllib.parse.quote(text, safe="")


def _customer_path(key: Key) -> str:
    return f"/v1/customerinvoice/{_quote(key[0])}"


def _supplier_path(key: Key) -> str:
    return f"/v1/supplierInvoice/{_quote(key[0])}/{_quote(key[1])}"


@dataclass(frozen=True)
class Endpoint:
    name: str  # also the endpoint name in the heartbeat table
    list_path: str
    key_fields: tuple[str, ...]  # Visma field names
    key_columns: tuple[str, ...]  # warehouse column names of the same fields
    by_key: Callable[[Key], str]


ENDPOINTS = (
    Endpoint(
        "customer_invoices",
        "/v1/customerinvoice",
        ("referenceNumber",),
        ("reference_number",),
        _customer_path,
    ),
    Endpoint(
        "supplier_invoices",
        "/v1/supplierInvoice",
        ("documentType", "referenceNumber"),
        ("document_type", "reference_number"),
        _supplier_path,
    ),
)


class VismaError(RuntimeError):
    """An answer from Visma the gate cannot rely on. The tenant gets reason error."""


class Visma:
    """GET-only client for one tenant. Every request goes through PACER."""

    def __init__(self, tenant: str) -> None:
        self.tenant = tenant
        self.bearer = fetch_token(tenant)

    def get(self, path: str, params: dict[str, Any] | None = None) -> tuple[int, Any]:
        query = ("?" + urllib.parse.urlencode(params)) if params else ""
        for attempt in range(len(RETRY_WAITS) + 1):
            try:
                return PACER.run("GET", lambda: self._read(BASE + path + query))
            except urllib.error.HTTPError as error:
                if error.code == 404:
                    return 404, None
                if error.code == 401 and attempt == 0:
                    self.bearer = self._refresh_token()
                    continue
                if error.code in TRANSIENT_CODES and attempt < len(RETRY_WAITS):
                    time.sleep(RETRY_WAITS[attempt])
                    continue
                raise VismaError(f"GET {path} answered HTTP {error.code}") from None
            except OSError as error:  # timeouts, resets: transient, retried like 429/5xx
                if attempt < len(RETRY_WAITS):
                    print(f"  retry after {type(error).__name__}: GET {path}", file=sys.stderr)
                    time.sleep(RETRY_WAITS[attempt])
                    continue
                raise VismaError(f"GET {path} failed: {type(error).__name__}") from None
        raise VismaError(f"GET {path} gave no answer")

    def _refresh_token(self) -> str:
        try:
            return fetch_token(self.tenant)
        except subprocess.CalledProcessError as error:
            code = error.returncode
            raise VismaError(f"token refresh failed: vsv2 token exited {code}") from None

    def _read(self, url: str) -> tuple[int, Any]:
        # The headers vsv2-erp-rest sends (client.rs build_get_request). Floats parse as Decimal.
        request = urllib.request.Request(
            url,
            headers={
                "Authorization": f"Bearer {self.bearer}",
                "Accept": "application/json",
                "ipp-company-id": self.tenant,
                "ipp-application-type": APPLICATION_TYPE,
            },
        )
        with urllib.request.urlopen(request, timeout=90) as response:
            return response.status, json.loads(response.read(), parse_float=Decimal)


@dataclass(frozen=True)
class Doc:
    balance: Decimal
    modified: datetime


@dataclass(frozen=True)
class Lookup:
    code: int
    status: str | None = None
    modified: datetime | None = None


@dataclass(frozen=True)
class Finding:
    kind: str
    key: Key
    detail: str


@dataclass
class Outcome:
    matched: int = 0
    cent_drift: int = 0
    pending: Counter[str] = field(default_factory=Counter)
    defects: list[Finding] = field(default_factory=list)
    errors: list[str] = field(default_factory=list)


@dataclass
class WarehouseOpen:
    balances: dict[Key, Decimal] = field(default_factory=dict)
    duplicates: int = 0
    null_balances: int = 0
    null_keys: int = 0


@dataclass
class Warehouse:
    open: dict[str, dict[str, WarehouseOpen]]  # endpoint name -> tenant -> open documents
    synced: dict[str, dict[str, datetime]]  # endpoint name -> tenant -> last successful sync


def cents(value: Any) -> Decimal:
    return Decimal(str(value if value is not None else 0)).quantize(CENT, ROUND_HALF_UP)


def parse_visma_time(text: str) -> datetime:
    """Visma writes lastModifiedDateTime without an offset; it is UTC (calibrated 2026-10-09)."""
    moment = datetime.fromisoformat(re.sub(r"(\.\d{6})\d+", r"\1", text))
    if moment.tzinfo is None:
        moment = moment.replace(tzinfo=UTC)
    return moment.astimezone(UTC)


def key_of(endpoint: Endpoint, row: dict[str, Any]) -> Key:
    return tuple(str(row[name]).strip() for name in endpoint.key_fields)


def live_docs(endpoint: Endpoint, rows: list[dict[str, Any]]) -> dict[Key, Doc]:
    for row in rows:
        missing = [name for name in REQUIRED_FIELDS if name not in row]
        if missing:
            raise VismaError(f"{endpoint.list_path}: fields {missing} missing, not as confirmed")
        if row["balance"] is None or row["lastModifiedDateTime"] is None:
            raise VismaError(f"{endpoint.list_path}: null balance or lastModifiedDateTime")
        if any(row[name] is None for name in endpoint.key_fields):
            raise VismaError(f"{endpoint.list_path}: null key field {endpoint.key_fields}")
    docs = {
        key_of(endpoint, row): Doc(
            cents(row["balance"]), parse_visma_time(row["lastModifiedDateTime"])
        )
        for row in rows
    }
    if len(docs) != len(rows):
        raise VismaError(f"{len(rows) - len(docs)} duplicate key(s) in the open list")
    return docs


def list_open(visma: Visma, endpoint: Endpoint) -> tuple[list[dict[str, Any]], int]:
    """Every open row, page by page, until an empty page. Every page must hold only Open rows."""
    rows: list[dict[str, Any]] = []
    page = 0
    while True:
        page += 1
        if page > MAX_PAGES:
            raise VismaError(f"{endpoint.list_path}: more than {MAX_PAGES} pages")
        params = {"status": "Open", "pageNumber": page, "pageSize": PAGE_SIZE}
        code, body = visma.get(endpoint.list_path, params)
        if code != 200 or not isinstance(body, list):
            raise VismaError(f"{endpoint.list_path} page {page}: HTTP {code}, not a list")
        if not body:
            return rows, page - 1
        not_open = sum(1 for row in body if row.get("status") != "Open")
        if not_open:
            raise VismaError(f"{endpoint.list_path} page {page}: {not_open} row(s) not Open")
        rows.extend(body)


def confirm_by_key(visma: Visma, endpoint: Endpoint, rows: list[dict[str, Any]]) -> str:
    if not rows:
        return "not run: no open rows"
    path = endpoint.by_key(key_of(endpoint, rows[0]))
    code, body = visma.get(path)
    if code != 200 or not isinstance(body, dict) or body.get("status") != "Open":
        raise VismaError(f"by-key path did not return the open row (HTTP {code})")
    return f"200 and status Open for {path}"


def lookup(visma: Visma, endpoint: Endpoint, key: Key) -> Lookup:
    code, body = visma.get(endpoint.by_key(key))
    if code == 404:
        return Lookup(404)
    if code != 200 or not isinstance(body, dict):
        raise VismaError(f"by-key lookup answered HTTP {code}, not an object")
    if not body.get("status"):
        raise VismaError(f"by-key response for {'|'.join(key)} has no status")
    modified = body.get("lastModifiedDateTime")
    return Lookup(200, body["status"], parse_visma_time(modified) if modified else None)


def classify(
    live: dict[Key, Doc],
    wh: dict[Key, Decimal],
    lookups: dict[Key, Lookup],
    threshold: datetime,
) -> Outcome:
    """Compare the two populations. Pure: every Visma answer arrives as an argument."""
    out = Outcome()
    for key in sorted(live.keys() - wh.keys()):
        modified = live[key].modified
        text = f"open in Visma, lastModified {modified:%Y-%m-%d %H:%M}Z"
        if modified > threshold:
            out.pending["open_at_visma_pending"] += 1
        else:
            out.defects.append(Finding("missed_or_stale", key, text))
    for key in sorted(wh.keys() - live.keys()):
        look = lookups[key]
        if look.code == 404:
            out.defects.append(Finding("gone_at_visma", key, "404 in Visma, open in warehouse"))
        elif look.status == "Open":
            out.errors.append(f"{'|'.join(key)} is Open by key but missing from the open list")
        elif look.modified is None:
            out.errors.append(f"{'|'.join(key)} has no lastModifiedDateTime in Visma")
        elif look.modified > threshold:
            out.pending["closed_at_visma_pending"] += 1
        else:
            text = f"{look.status} in Visma, lastModified {look.modified:%Y-%m-%d %H:%M}Z"
            out.defects.append(Finding("closed_without_timestamp", key, text))
    for key in sorted(live.keys() & wh.keys()):
        out.matched += 1
        gap = abs(live[key].balance - wh[key])
        if gap == CENT:
            out.cent_drift += 1
        elif gap > CENT:
            text = f"balance Visma {live[key].balance} vs warehouse {wh[key]}"
            if live[key].modified > threshold:
                out.pending["balance_pending"] += 1
            else:
                out.defects.append(Finding("balance_stale", key, text))
    return out


def freshness(
    last_sync: datetime | None, now: datetime, max_age_hours: float
) -> tuple[float | None, bool]:
    if last_sync is None:
        return None, True
    age = (now - last_sync).total_seconds() / 3600
    return round(age, 1), age > max_age_hours


def compare_endpoint(
    visma: Visma, endpoint: Endpoint, wh: WarehouseOpen, threshold: datetime
) -> tuple[Outcome, dict[str, Any], int]:
    """The Visma side of one endpoint and its comparison. Raises VismaError on a bad answer."""
    rows, pages = list_open(visma, endpoint)
    live = live_docs(endpoint, rows)
    by_key = confirm_by_key(visma, endpoint, rows)
    lookups = {
        key: lookup(visma, endpoint, key) for key in sorted(wh.balances.keys() - live.keys())
    }
    outcome = classify(live, wh.balances, lookups, threshold)
    block = {
        "open": len(rows),
        "balance": str(sum((doc.balance for doc in live.values()), Decimal("0"))),
        "pages": pages,
        "fields_checked": list(REQUIRED_FIELDS),
        "by_key_check": by_key,
    }
    return outcome, block, len(lookups)


def _unread_visma() -> dict[str, Any]:
    return {
        "open": None,
        "balance": None,
        "pages": 0,
        "fields_checked": [],
        "by_key_check": "not run",
    }


def check_endpoint(
    visma: Visma | None,
    endpoint: Endpoint,
    warehouse: Warehouse,
    tenant: str,
    now: datetime,
    max_age_hours: float,
) -> dict[str, Any]:
    """One endpoint. Freshness comes from the warehouse: judged even when Visma is not read."""
    wh = warehouse.open[endpoint.name].get(tenant, WarehouseOpen())
    last_sync = warehouse.synced[endpoint.name].get(tenant)
    age, stale = freshness(last_sync, now, max_age_hours)
    errors = _warehouse_errors(wh)
    outcome = Outcome()
    block = _unread_visma()
    lookups = 0
    if visma is None:
        errors.append("Visma not read: no token for this tenant")
    else:
        try:
            # No sync on record: the threshold is the epoch, so every difference with a timestamp
            # is PENDING and nothing is measured as a DEFECT.
            outcome, block, lookups = compare_endpoint(visma, endpoint, wh, last_sync or EPOCH)
        except (VismaError, OSError, ValueError, subprocess.SubprocessError) as error:
            errors.append(str(error))
    return {
        "visma": block,
        "warehouse": {
            "open": len(wh.balances),
            "balance": str(sum(wh.balances.values(), Decimal("0"))),
            "last_sync": last_sync.isoformat(timespec="seconds") if last_sync else None,
            "age_hours": age,
            "stale": stale,
        },
        "matched": outcome.matched,
        "cent_drift": outcome.cent_drift,
        "pending": dict(sorted(outcome.pending.items())),
        "defects": dict(sorted(Counter(f.kind for f in outcome.defects).items())),
        "defect_keys": [
            {"kind": f.kind, "key": "|".join(f.key), "detail": f.detail} for f in outcome.defects
        ],
        "lookups": lookups,
        "errors": outcome.errors + errors,
    }


def _warehouse_errors(wh: WarehouseOpen) -> list[str]:
    found = []
    if wh.duplicates:
        found.append(f"warehouse holds {wh.duplicates} duplicate open key(s)")
    if wh.null_balances:
        found.append(f"warehouse holds {wh.null_balances} open row(s) with a null balance")
    if wh.null_keys:
        found.append(f"warehouse holds {wh.null_keys} open row(s) with a null key")
    return found


def judge(record: dict[str, Any]) -> None:
    """Set record['reasons'] to every reason the tenant failed, and record['verdict'] to the worst.

    Reasons are error, stale and defect, and all of them are listed. The verdict is ERROR when any
    error applies, else FAIL when stale or defect applies, else PENDING, else PASS.
    """
    endpoints = list(record["endpoints"].values())
    errors = bool(record["errors"]) or any(ep["errors"] for ep in endpoints)
    stale = any(ep["warehouse"]["stale"] for ep in endpoints)
    defect = any(sum(ep["defects"].values()) for ep in endpoints)
    pending = any(ep["pending"] for ep in endpoints)
    record["reasons"] = [
        name for name, hit in (("error", errors), ("stale", stale), ("defect", defect)) if hit
    ]
    if errors:
        record["verdict"] = "ERROR"
    elif stale or defect:
        record["verdict"] = "FAIL"
    elif pending:
        record["verdict"] = "PENDING"
    else:
        record["verdict"] = "PASS"


def check_tenant(
    tenant: str,
    name: str,
    kind: str,
    warehouse: Warehouse,
    now: datetime,
    max_age_hours: float,
) -> dict[str, Any]:
    record: dict[str, Any] = {"tenant_id": tenant, "name": name, "kind": kind, "endpoints": {}}
    record["errors"] = []
    visma: Visma | None = None
    try:
        visma = Visma(tenant)
    except subprocess.CalledProcessError as error:
        record["errors"].append(f"no Visma token: vsv2 token exited {error.returncode}")
    except OSError as error:
        record["errors"].append(f"no Visma token: {type(error).__name__}")
    for endpoint in ENDPOINTS:
        record["endpoints"][endpoint.name] = check_endpoint(
            visma, endpoint, warehouse, tenant, now, max_age_hours
        )
    judge(record)
    return record


def load_open(
    client: bigquery.Client, table: str, key_columns: tuple[str, ...]
) -> dict[str, WarehouseOpen]:
    columns = ", ".join(("tenant_id", *key_columns, "balance"))
    result = client.query(
        f"SELECT {columns} FROM `{table}` WHERE status = 'Open'", location=LOCATION
    )
    found: dict[str, WarehouseOpen] = {}
    for row in result.result():
        bucket = found.setdefault(str(row["tenant_id"]).strip(), WarehouseOpen())
        if any(row[column] is None for column in key_columns):
            bucket.null_keys += 1
            continue
        key = tuple(str(row[column]).strip() for column in key_columns)
        if row["balance"] is None:
            bucket.null_balances += 1
        if key in bucket.balances:
            bucket.duplicates += 1
        bucket.balances[key] = cents(row["balance"])
    return found


def load_heartbeats(
    client: bigquery.Client, table: str | None, endpoint: str
) -> dict[str, datetime]:
    """MAX(finished_at) per tenant of the endpoint's heartbeats. Empty: no sync on record.

    No table yet (NotFound) and a source with no table are both 'no sync on record'.
    """
    if table is None:
        return {}
    query = (
        "SELECT tenant_id, MAX(finished_at) AS last_sync "
        f"FROM `{table}` WHERE endpoint = @endpoint AND status = @status GROUP BY tenant_id"
    )
    config = bigquery.QueryJobConfig(
        query_parameters=[
            bigquery.ScalarQueryParameter("endpoint", "STRING", endpoint),
            bigquery.ScalarQueryParameter("status", "STRING", HEARTBEAT_STATUS),
        ]
    )
    try:
        rows = client.query(query, job_config=config, location=LOCATION).result()
        return {str(row[0]).strip(): row[1] for row in rows if row[1] is not None}
    except NotFound:
        return {}


def load_warehouse(client: bigquery.Client, source: Source) -> Warehouse:
    customer, supplier = ENDPOINTS
    return Warehouse(
        open={
            customer.name: load_open(client, source.customer_table, customer.key_columns),
            supplier.name: load_open(client, source.supplier_table, supplier.key_columns),
        },
        synced={
            endpoint.name: load_heartbeats(client, source.heartbeat_table, endpoint.name)
            for endpoint in ENDPOINTS
        },
    )


def _count(value: int | None) -> str:
    return "-" if value is None else f"{value:,}"


def _money(value: str | None) -> str:
    return "-" if value is None else f"{Decimal(value):,.2f}"


def _kinds(counts: dict[str, int]) -> str:
    return ", ".join(f"{kind} {n}" for kind, n in counts.items()) or "none"


def _sync_text(endpoint: dict[str, Any]) -> str:
    wh = endpoint["warehouse"]
    if wh["last_sync"] is None:
        return "none on record (stale)"
    stamp = wh["last_sync"][:16].replace("T", " ")
    return f"{stamp}Z, {wh['age_hours']} h{' STALE' if wh['stale'] else ''}"


def _per_endpoint(record: dict[str, Any], read: Callable[[dict[str, Any]], str]) -> str:
    """Customer / supplier values for the summary table."""
    return " / ".join(read(record["endpoints"][e.name]) for e in ENDPOINTS)


def _verdict_cell(record: dict[str, Any]) -> str:
    reasons = f" ({', '.join(record['reasons'])})" if record["reasons"] else ""
    return f"**{record['verdict']}**{reasons}"


def _summary_row(record: dict[str, Any]) -> str:
    eps = record["endpoints"]
    defects = sum(sum(ep["defects"].values()) for ep in eps.values())
    pending = sum(sum(ep["pending"].values()) for ep in eps.values())
    visma = _per_endpoint(record, lambda ep: _count(ep["visma"]["open"]))
    warehouse = _per_endpoint(record, lambda ep: _count(ep["warehouse"]["open"]))
    syncs = _per_endpoint(record, _sync_text)
    return (
        f"| {record['name']} | {record['kind']} | {_verdict_cell(record)} "
        f"| {visma} | {warehouse} | {defects} | {pending} | {syncs} |"
    )


def _tenant_section(record: dict[str, Any]) -> list[str]:
    reasons = ", ".join(record["reasons"]) or "none"
    lines = [
        f"## {record['name']} ({record['kind']}), `{record['tenant_id']}`: {record['verdict']}",
        "",
        f"- Reasons: {reasons}",
    ]
    if record["errors"]:
        lines += [f"- ERROR: {error}" for error in record["errors"]]
    lines.append("")
    lines += ["| | Customer invoices | Supplier invoices |", "|---|---|---|"]
    rows = [
        (
            "Visma open: count / balance",
            lambda e: f"{_count(e['visma']['open'])} / {_money(e['visma']['balance'])}",
        ),
        (
            "Warehouse open: count / balance",
            lambda e: f"{_count(e['warehouse']['open'])} / {_money(e['warehouse']['balance'])}",
        ),
        ("Matched (open in both)", lambda e: _count(e["matched"])),
        ("Last sync (age)", _sync_text),
        ("PENDING", lambda e: _kinds(e["pending"])),
        ("DEFECT", lambda e: _kinds(e["defects"])),
        ("Cent drift (1 cent, not counted)", lambda e: _count(e["cent_drift"])),
        ("Lookups by key (warehouse only)", lambda e: _count(e["lookups"])),
    ]
    for label, cell in rows:
        values = [cell(record["endpoints"][e.name]) for e in ENDPOINTS]
        lines.append(f"| {label} | {values[0]} | {values[1]} |")
    lines.append("")
    for endpoint in ENDPOINTS:
        ep = record["endpoints"][endpoint.name]
        if ep["errors"]:
            lines += [f"- {endpoint.name} errors: " + "; ".join(ep["errors"]), ""]
    samples = [
        f"- {name} `{item['key']}`: {item['kind']}, {item['detail']}"
        for name, ep in record["endpoints"].items()
        for item in ep["defect_keys"]
    ][:SAMPLE_KEYS]
    if samples:
        lines += [f"First {len(samples)} DEFECT keys:", "", *samples, ""]
    return lines


METHOD = """## Method and confirmed facts

- Live list, confirmed on every run per tenant: `GET /v1/customerinvoice` and
  `GET /v1/supplierInvoice` with `status=Open`, `pageNumber` and `pageSize=1000`, read until an
  empty page. Every row must have `status` Open and the fields `referenceNumber`, `documentType`,
  `balance`, `status` and `lastModifiedDateTime`. A duplicate key or a missing field is an ERROR.
  The by-key path is checked with one GET of the first open row.
- Responses are JSON lists with no total in the headers. Customer keys are `referenceNumber`;
  supplier keys are `(documentType, referenceNumber)`.
- Warehouse documents that are open in the warehouse but not in the live list get one GET by key
  each: `/v1/customerinvoice/{referenceNumber}` or
  `/v1/supplierInvoice/{documentType}/{referenceNumber}`. A 404 is gone_at_visma (DEFECT). A 200
  needs `status` and `lastModifiedDateTime`, and decides PENDING or DEFECT.
- Requests: one limiter for all tenants. The next request starts at least 0.3 s after the previous
  response returned. Token calls (`vsv2 token`) are paced the same way.
- `lastModifiedDateTime` from Visma has no offset and is read as UTC. Calibrated on 2026-10-09 on
  the pilot tenant: 630 of 706 matched customer invoices equal the warehouse `last_modified_at` to
  the minute on that reading.
- Last sync: MAX(finished_at) per tenant and endpoint of the heartbeat table `_sync_heartbeat`,
  status `succeeded`, written by `vsv2-bq-stage` after each successful endpoint write. It replaces
  the write-timestamp proxies, which move on a rewrite and not on a sync.
- Balances are compared at cent precision. A difference counts when it is more than 0.01. A
  difference of exactly one cent is reported as cent drift and not counted.
- Freshness is checked per endpoint, even when Visma is not read: a stale endpoint is a reason
  `stale`.
- Verdicts and the exit code: see docs/gates/README.md.
"""

SUMMARY_HEADER = (
    "| Tenant | Kind | Verdict | Visma open C / S | Warehouse open C / S "
    "| DEFECT | PENDING | Last sync C / S |"
)


WAREHOUSE_LABEL = {
    "v2": "v2 (aquatiq-datavarehus)",
    "v1": "v1 (prj-dw-dev), negative control",
}


def render(report: dict[str, Any]) -> str:
    counts = Counter(t["verdict"] for t in report["tenants"])
    requests = report["requests"]
    heartbeat = report["heartbeat_table"] or "none (this source writes no heartbeat)"
    head = (
        f"# Population gate: {report['source']} against Visma, {report['date']}\n\n"
        f"> Generated by `scripts/verify_population_vs_visma.py` at {report['generated_utc']}. "
        f"Max age {report['max_age_hours']} h. Live side: Visma GET only, "
        f"{requests.get('GET', 0)} GET and {requests.get('token', 0)} token requests, one shared "
        f"pace of {PACE_SECONDS} s after each response. Warehouse side: "
        f"{WAREHOUSE_LABEL[report['source']]}. Heartbeat: `{heartbeat}`.\n"
    )
    if report["subset"]:
        head += "\n> SUBSET RUN: only the tenants named with `--tenants`. Not a full gate.\n"
    verdict_counts = ", ".join(
        f"{verdict} {counts.get(verdict, 0)}" for verdict in ("PASS", "PENDING", "FAIL", "ERROR")
    )
    summary = [
        "",
        f"**Overall: exit code {report['exit_code']}.** Verdicts: {verdict_counts}",
        "",
        SUMMARY_HEADER,
        "|---|---|---|---|---|---|---|---|",
    ]
    summary += [_summary_row(record) for record in report["tenants"]]
    body = [""]
    for record in report["tenants"]:
        body += _tenant_section(record)
    return "\n".join([head, *summary, *body, METHOD]) + "\n"


def build_report(
    source: str,
    date_text: str,
    max_age_hours: float,
    now: datetime,
    results: list[dict[str, Any]],
    subset: bool,
    requests: dict[str, int],
) -> dict[str, Any]:
    ok = bool(results) and not any(r["reasons"] for r in results)
    return {
        "source": source,
        "date": date_text,
        "generated_utc": now.isoformat(timespec="seconds"),
        "max_age_hours": max_age_hours,
        "subset": subset,
        "heartbeat_table": SOURCES[source].heartbeat_table,
        "requests": dict(requests),
        "exit_code": 0 if ok else 1,
        "tenants": results,
    }


def write_outputs(report: dict[str, Any]) -> Path:
    GATES.mkdir(parents=True, exist_ok=True)
    stem = f"population_{report['source']}_{report['date']}" + (
        "_subset" if report["subset"] else ""
    )
    markdown = GATES / f"{stem}.md"
    markdown.write_text(render(report))
    (GATES / f"{stem}.json").write_text(
        json.dumps(report, indent=2, ensure_ascii=False, default=str) + "\n"
    )
    return markdown


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--source", choices=sorted(SOURCES), default="v2")
    parser.add_argument("--max-age-hours", type=float, default=26.0)
    parser.add_argument(
        "--date", default=date.today().isoformat(), help="YYYY-MM-DD, names the report"
    )
    parser.add_argument("--tenants", default="", help="comma-separated tenant ids (default: all)")
    args = parser.parse_args()
    chosen = {t for t in args.tenants.split(",") if t}
    unknown = sorted(chosen - {tenant for tenant, _, _ in tenants.TENANTS})
    if unknown:
        parser.error(f"not in tenant_scan_compare.TENANTS: {', '.join(unknown)}")
    try:
        date.fromisoformat(args.date)
    except ValueError:
        parser.error("--date must be YYYY-MM-DD")

    now = datetime.now(UTC)
    selected = [t for t in tenants.TENANTS if not chosen or t[0] in chosen]
    source = SOURCES[args.source]
    warehouse = load_warehouse(bigquery.Client(project=QUERY_PROJECT, location=LOCATION), source)
    results = []
    for number, (tenant, name, kind) in enumerate(selected, 1):
        record = check_tenant(tenant, name, kind, warehouse, now, args.max_age_hours)
        results.append(record)
        reasons = f" [{', '.join(record['reasons'])}]" if record["reasons"] else ""
        print(f"[{number}/{len(selected)}] {name}: {record['verdict']}{reasons}", flush=True)
    requests = dict(PACER.calls)
    print(
        f"requests: GET {requests.get('GET', 0)}, token {requests.get('token', 0)}",
        flush=True,
    )
    report = build_report(
        args.source, args.date, args.max_age_hours, now, results, bool(chosen), requests
    )
    path = write_outputs(report)
    print(f"wrote {path} (exit {report['exit_code']})", flush=True)
    return report["exit_code"]


if __name__ == "__main__":
    raise SystemExit(main())
