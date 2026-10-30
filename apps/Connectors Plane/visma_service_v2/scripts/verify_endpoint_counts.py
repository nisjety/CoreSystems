#!/usr/bin/env python3
"""Gate: does staging hold as many records per endpoint and company as Visma reports? Read-only.

Why this exists: every other check compares v2 with something v2 fetched itself or with v1, so a
fetch gap that both share is invisible to them (silver once dropped 967 sales orders that way).
Visma can say how many records it holds. gate-s asks that of the sales orders; this asks it of every
ERP REST endpoint that has a total, for every company, against the raw landing tables in
aquatiq-datavarehus.dw_staging_visma.

Rule, per endpoint and tenant (as in verify_sales_order_counts.py):
  missing  = Visma's total - staging's live rows (deleted_at IS NULL)
  changed  = Visma's count of records modified since the last write, asked only when something is
             missing (lastModifiedDateTime with the >= condition; modifiedSince for Sales Order)
  FAIL  when missing > changed. With no modified filter that works, or no write on record, nothing
        can be pending and the tolerance is 0; the report says so for the cell.
  WARN  when staging has more than Visma: records deleted in Visma since the last full pass stay
        until that pass reconciles them. Not a failure; the number is reported.
  PASS  otherwise.
  ERROR when Visma's answer cannot be read. A cell that could not be measured is not a pass, so an
        ERROR fails the gate.

Visma's total comes from one request of one row (pageSize=1): metadata.totalCount in the first row
of a finance list, totalCount at the top of an object; an empty page is 0. An answer with rows and
no total makes the endpoint NOT COUNTABLE (nothing is guessed), as do the endpoints in
NOT_COUNTABLE (a parent key, a period or the background API). The modified filter is trusted only
after Visma answered 0 to a request dated 2999 (once per endpoint): several endpoints ignore it and
answer their full total, which would make the tolerance equal the total. See docs/gates/README.md.

Last write: MAX(finished_at) of the heartbeat table where a row exists, else the last staging write
(MAX(updated_at)); the report says which. Visma: GET only, one request at a time, paced by the
population gate's limiter (0.3 s after each response). Tokens come from `vsv2 token` and are never
printed; an HTTP error is reported as its status code only.

Usage: scripts/verify_endpoint_counts.py [--tenants id,id] [--endpoints a,b] [--date YYYY-MM-DD]
Writes docs/gates/endpoint_counts_<date>.md and .json (`_subset` with --tenants or --endpoints).
Exit 0 only if no cell is FAIL or ERROR.
"""

from __future__ import annotations

import argparse
import http.client
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
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict, dataclass
from datetime import UTC, date, datetime
from pathlib import Path
from typing import Any

from google.api_core.exceptions import NotFound
from google.cloud import bigquery

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))

import tenant_scan_compare as tenants  # noqa: E402
from _bq_target import DATASET, PROJECT  # noqa: E402
from verify_population_vs_visma import PACER  # noqa: E402

LOCATION = "europe-north2"
# The project and staging dataset the pipeline writes to (VSV2_BIGQUERY_PROJECT and
# VSV2_BIGQUERY_DATASET_STAGING, as in every script), so a rehearsal dataset can be checked too.
STAGING = f"{PROJECT}.{DATASET}"
HEARTBEAT = f"{STAGING}._sync_heartbeat"
CONFIGS = ROOT / "config" / "endpoints"
GATES = ROOT / "docs" / "gates"
FINANCE = "https://api.finance.visma.net"
SALES_ORDER_HOST = "salesorder.visma.net"
APPLICATION_TYPE = "Visma.net Financials"  # the header vsv2-erp-rest sends (client.rs)
REQUEST_TIMEOUT_S = 180
RETRY_WAITS = (5, 15, 45)
TRANSIENT = frozenset({429, 500, 502, 503, 504})
ROW_KEYS = ("value", "items", "data", "results", "records")
FAR_FUTURE = datetime(2999, 1, 1, tzinfo=UTC)
STAGING_WORKERS = 6
HEARTBEAT_SOURCE = "heartbeat"
WRITE_SOURCE = "last staging write (no heartbeat yet)"
BASIS_FILTER = "modified filter"
BASIS_NO_FILTER = "no working modified filter, tolerance 0"
BASIS_NO_WRITE = "no write on record, tolerance 0"
BASIS_NOT_NEEDED = "not needed"

# General ledger transactions are counted over every period the fetch walks: from the config's
# first_period to one far beyond the 12 months past today where the walk stops.
WINDOWED = frozenset({"general_ledger_transactions"})
LAST_PERIOD = "203012"

# Endpoints that cannot be counted by a total, with the reason. Statuses were read on the pilot
# (Aquatiq AS) on 2026-10-12 with one GET each.
NOT_COUNTABLE: dict[str, str] = {
    "budgets": (
        "needs branch, ledger and financialYear (a partitioned fan-out): HTTP 400 without them. "
        "A count is one request per combination, not one total."
    ),
    "time_cards": (
        "needs employeeCd (one request per employee): HTTP 400 without it, so no single total."
    ),
    "project_tasks": (
        "needs projectId (one request per project): HTTP 400 without it, so no single total."
    ),
    "customer_locations": (
        "child of the Sales Order API (/Customers/{customerId}/locations): an unpaged list per "
        "customer with no total, and it needs the parent key."
    ),
    "expense_receipts": (
        "no total: the answer is the whole unpaged list, no totalCount, about 100 s (pilot: "
        "14,548 rows, HTTP 200)."
    ),
    "general_ledger_balances_v2": (
        "period-scanned: periodId is required (HTTP 400 without it) and the total is per period, "
        "so a count is one request per period, not one total."
    ),
    "journal_transactions": (
        "period-scanned and line-counted: periodId is required (HTTP 400 without it) and the "
        "total counts journal lines, while staging holds one row per batch."
    ),
    "project_transactions": (
        "period-scanned: fromPeriod and toPeriod are required (HTTP 400 without them) and the "
        "fetch walks one month at a time (staging keeps a row per document and _scan_period), so "
        "a count is one request per month, not one total."
    ),
}


@dataclass(frozen=True)
class Verdict:
    word: str  # PASS | WARN | FAIL
    reason: str


def judge(visma_total: int, changed: int | None, staging_total: int) -> Verdict:
    """The rule from the module docstring, on the counts of one endpoint and tenant.

    `changed` is Visma's count of records modified since the last write, or None when nothing can
    be pending (no working modified filter, no write on record): the tolerance is then 0.
    """
    missing = visma_total - staging_total
    tolerance = changed or 0
    if missing > tolerance:
        if changed is None:
            return Verdict(
                "FAIL",
                f"{missing:,} records missing from staging and none can be pending (tolerance 0)",
            )
        return Verdict(
            "FAIL",
            f"{missing:,} records missing from staging, more than the {changed:,} modified in "
            "Visma since the last write",
        )
    if missing < 0:
        return Verdict(
            "WARN",
            f"staging has {-missing:,} more than Visma (deleted in Visma since the last full pass)",
        )
    if missing == 0:
        return Verdict("PASS", "none missing")
    return Verdict(
        "PASS", f"{missing:,} missing, within the {tolerance:,} modified since the last write"
    )


def _as_count(value: object) -> int | None:
    """A total as Visma reports it: a non-negative integer. Booleans are not counts."""
    if isinstance(value, int) and not isinstance(value, bool) and value >= 0:
        return value
    return None


def _total_in_rows(rows: list[Any]) -> int | None:
    """0 for an empty page; else the first row's metadata.totalCount, or None when it has none."""
    if not rows:
        return 0
    first = rows[0]
    meta = first.get("metadata") if isinstance(first, dict) else None
    return _as_count(meta.get("totalCount")) if isinstance(meta, dict) else None


def extract_total(body: Any) -> int | None:
    """Visma's total from the answer to a one-row request.

    0 for an empty page. None when the answer has rows and no total. Raises ValueError for a shape
    that is not understood, so that it becomes an ERROR and is not mistaken for "no total".
    """
    if isinstance(body, list):
        return _total_in_rows(body)
    if not isinstance(body, dict):
        raise ValueError(f"unexpected answer shape {type(body).__name__}")
    direct = _as_count(body.get("totalCount"))
    if direct is not None:
        return direct
    meta = body.get("metadata")
    nested = _as_count(meta.get("totalCount")) if isinstance(meta, dict) else None
    if nested is not None:
        return nested
    for key in ROW_KEYS:
        if isinstance(body.get(key), list):
            return _total_in_rows(body[key])
    raise ValueError("unexpected answer object: no total and no rows")


def row_count(body: Any) -> int:
    """Rows in a one-row answer: more than one means the endpoint ignored pageSize."""
    if isinstance(body, list):
        return len(body)
    if isinstance(body, dict):
        for key in ROW_KEYS:
            if isinstance(body.get(key), list):
                return len(body[key])
    return 0


def derive_filters(default_query: dict[str, Any]) -> dict[str, str]:
    """The scalar default_query parameters other than expand flags, as query strings."""
    filters: dict[str, str] = {}
    for key, value in default_query.items():
        if key.lower().startswith("expand") or isinstance(value, list | dict) or value is None:
            continue
        filters[key] = str(value).lower() if isinstance(value, bool) else str(value)
    return filters


def count_filters(config: dict[str, Any]) -> dict[str, str]:
    """The query parameters that choose which records an endpoint's fetch covers. The expand flags
    in a config only add fields and are left out of a count."""
    filters = derive_filters(config.get("default_query") or {})
    if config["endpoint_name"] in WINDOWED:
        window = {"fromPeriod": config["period_scan"]["first_period"], "toPeriod": LAST_PERIOD}
        return {"ledger": filters["ledger"], **window}
    return filters


@dataclass(frozen=True)
class Plan:
    """How to count one endpoint."""

    endpoint: str
    url: str
    sales_order: bool  # Sales Order API: pageIndex, modifiedSince and its own token scope
    filters: tuple[tuple[str, str], ...]
    method: str

    @property
    def scope(self) -> str:
        return "sales-order" if self.sales_order else "erp"


def plan_for(config: dict[str, Any]) -> Plan:
    name = config["endpoint_name"]
    raw = config["api_endpoint"]
    url = raw if raw.startswith("http") else FINANCE + raw
    sales_order = urllib.parse.urlparse(url).hostname == SALES_ORDER_HOST
    filters = count_filters(config)
    api = "Sales Order API" if sales_order else "finance list"
    note = ", ".join(f"{k}={v}" for k, v in filters.items())
    method = f"{api}, one row" + (f" ({note})" if note else "")
    return Plan(name, url, sales_order, tuple(filters.items()), method)


def modified_query(sales_order: bool, since: datetime) -> dict[str, str]:
    """The parameters that ask for records modified at or after `since` (UTC, to the second)."""
    utc = since.astimezone(UTC)
    if sales_order:
        return {"modifiedSince": utc.strftime("%Y-%m-%dT%H:%M:%SZ")}
    return {
        "lastModifiedDateTime": utc.strftime("%Y-%m-%d %H:%M:%S"),
        "lastModifiedDateTimeCondition": ">=",
    }


def one_row_query(plan: Plan, since: datetime | None = None) -> dict[str, str]:
    """The query of a one-row request: the endpoint's filters, page 1 of size 1, and optionally
    the modified filter."""
    # The Sales Order API counts pages from 0 (pageIndex), the finance API from 1 (pageNumber).
    paging = (
        {"pageIndex": "0", "pageSize": "1"}
        if plan.sales_order
        else {"pageNumber": "1", "pageSize": "1"}
    )
    modified = modified_query(plan.sales_order, since) if since is not None else {}
    return {**dict(plan.filters), **paging, **modified}


class VismaError(RuntimeError):
    """A Visma answer the gate cannot read. The message carries a status or a reason, nothing
    that came from a request."""

    def __init__(self, message: str, status: int | None = None) -> None:
        super().__init__(message)
        self.status = status


def token(scope: str, tenant: str) -> str:
    """A bearer token for the scope. The CLI reads its .env from the v2 root, hence cwd."""
    try:
        out = subprocess.run(
            [str(ROOT / "target/debug/vsv2"), "token", "--scope", scope, "--tenant", tenant],
            capture_output=True,
            text=True,
            check=True,
            cwd=ROOT,
        ).stdout
    except subprocess.CalledProcessError as error:
        raise VismaError(f"token for {scope}: vsv2 exited {error.returncode}") from None
    lines = out.strip().splitlines()
    if not lines:
        raise VismaError(f"token for {scope}: vsv2 printed nothing")
    return lines[-1]


def _send(url: str, headers: dict[str, str]) -> Any:
    request = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(request, timeout=REQUEST_TIMEOUT_S) as response:
        return json.loads(response.read())


Pace = Callable[[str, Callable[[], Any]], Any]


class Visma:
    """GET-only client for one tenant. Every request goes through the shared pacer."""

    def __init__(
        self,
        tenant: str,
        *,
        get_token: Callable[[str, str], str] = token,
        send: Callable[[str, dict[str, str]], Any] = _send,
        sleep: Callable[[float], None] = time.sleep,
        pace: Pace = PACER.run,
    ) -> None:
        self.tenant = tenant
        self._get_token = get_token
        self._send = send
        self._sleep = sleep
        self._pace = pace
        self._tokens: dict[str, str] = {}

    def _headers(self, scope: str) -> dict[str, str]:
        if scope not in self._tokens:
            self._tokens[scope] = self._pace("token", lambda: self._get_token(scope, self.tenant))
        return {
            "Authorization": f"Bearer {self._tokens[scope]}",
            "Accept": "application/json",
            "ipp-company-id": self.tenant,
            "ipp-application-type": APPLICATION_TYPE,
        }

    def get_json(self, url: str, query: dict[str, str], scope: str) -> Any:
        """One GET, parsed. Transient failures are retried after 5, 15 and 45 s, a 401 once with a
        new token. Whatever fails is raised as a VismaError with a status or a reason only."""
        full = f"{url}?{urllib.parse.urlencode(query)}" if query else url
        refreshed = False
        for attempt in range(len(RETRY_WAITS) + 1):
            headers = self._headers(scope)
            try:
                return self._pace("GET", lambda sent=headers: self._send(full, sent))
            except urllib.error.HTTPError as error:
                code = error.code
                if code == 401 and not refreshed:
                    refreshed = True
                    self._tokens.pop(scope, None)
                    continue
                if code in TRANSIENT and attempt < len(RETRY_WAITS):
                    self._sleep(RETRY_WAITS[attempt])
                    continue
                raise VismaError(f"HTTP {code}", code) from None
            except ValueError:
                raise VismaError("answer is not JSON") from None
            except (OSError, http.client.HTTPException) as error:
                if attempt < len(RETRY_WAITS):
                    self._sleep(RETRY_WAITS[attempt])
                    continue
                raise VismaError(f"request failed: {type(error).__name__}") from None
        raise VismaError("no answer")


@dataclass(frozen=True)
class Answer:
    total: int | None  # None: the answer has rows and no total
    rows: int  # rows in the answer: more than 1 when the endpoint ignores pageSize


def ask(visma: Visma, plan: Plan, since: datetime | None = None) -> Answer:
    body = visma.get_json(plan.url, one_row_query(plan, since), plan.scope)
    try:
        return Answer(extract_total(body), row_count(body))
    except ValueError as error:
        raise VismaError(str(error)) from None


class FilterGuard:
    """Whether an endpoint's modified filter takes effect, learned once per endpoint.

    Asked for a record modified in the year 2999, a filter that works answers 0. One that is
    ignored answers the full total, and its count of "modified since" would excuse any gap.
    """

    def __init__(self) -> None:
        self._works: dict[str, bool] = {}

    def works(self, visma: Visma, plan: Plan) -> bool:
        if plan.endpoint not in self._works:
            self._works[plan.endpoint] = self._probe(visma, plan)
        return self._works[plan.endpoint]

    def state(self, endpoint: str) -> str:
        known = self._works.get(endpoint)
        return "not needed" if known is None else ("works" if known else "ignored")

    @staticmethod
    def _probe(visma: Visma, plan: Plan) -> bool:
        """An endpoint that refuses the filter (HTTP 400) has none. Any other failure is no answer:
        it is raised, so that it is not remembered for the endpoint's other tenants."""
        try:
            return ask(visma, plan, FAR_FUTURE).total == 0
        except VismaError as error:
            if error.status == 400:
                return False
            raise


@dataclass(frozen=True)
class Staged:
    live: int  # rows with deleted_at IS NULL
    last_write: datetime | None  # MAX(updated_at) over every row


def last_write(staged: Staged | None, beat: datetime | None) -> tuple[datetime | None, str]:
    """The time staging was last brought up to date for a tenant and endpoint, and its source."""
    if beat is not None:
        return beat, HEARTBEAT_SOURCE
    if staged is not None and staged.last_write is not None:
        return staged.last_write, WRITE_SOURCE
    return None, "none"


@dataclass(frozen=True)
class Cell:
    endpoint: str
    tenant: str
    name: str
    visma_total: int | None
    staging_total: int
    missing: int | None
    changed: int | None
    basis: str
    last_write: str | None
    last_write_source: str
    verdict: str
    reason: str


@dataclass(frozen=True)
class EndpointResult:
    endpoint: str
    method: str
    cells: tuple[Cell, ...]
    not_countable: str | None
    modified_filter: str  # works | ignored | not needed

    def count(self, word: str) -> int:
        return sum(cell.verdict == word for cell in self.cells)


def measure(
    visma: Visma,
    plan: Plan,
    target: tuple[str, str],
    total: int,
    staged: Staged | None,
    beat: datetime | None,
    guard: FilterGuard,
) -> Cell:
    """Compare one tenant's Visma total with staging; ask for the changed count when it matters."""
    tenant, name = target
    live = staged.live if staged is not None else 0
    since, source = last_write(staged, beat)
    missing = total - live
    changed: int | None = None
    basis = BASIS_NOT_NEEDED
    if missing > 0:
        if since is None:
            basis = BASIS_NO_WRITE
        elif guard.works(visma, plan):
            changed = ask(visma, plan, since).total
            basis = BASIS_FILTER if changed is not None else BASIS_NO_FILTER
        else:
            basis = BASIS_NO_FILTER
    verdict = judge(total, changed, live)
    return Cell(
        plan.endpoint,
        tenant,
        name,
        total,
        live,
        missing,
        changed,
        basis,
        since.isoformat(timespec="seconds") if since else None,
        source,
        verdict.word,
        verdict.reason,
    )


def error_cell(plan: Plan, target: tuple[str, str], staged: Staged | None, reason: str) -> Cell:
    tenant, name = target
    live = staged.live if staged is not None else 0
    return Cell(
        plan.endpoint, tenant, name, None, live, None, None, "-", None, "-", "ERROR", reason
    )


def no_total_reason(rows: int) -> str:
    if rows > 1:
        return (
            f"no total: the endpoint ignores pageSize and answers its whole list ({rows:,} rows "
            "for a one-row request) without a totalCount"
        )
    return "no total: the one-row answer has a row but no totalCount"


def check_endpoint(
    plan: Plan,
    targets: list[tuple[str, str]],
    staged: dict[str, Staged] | None,
    beats: dict[tuple[str, str], datetime] | None,
    visma_for: Callable[[str], Visma],
    guard: FilterGuard,
) -> EndpointResult:
    """Every tenant of one endpoint. The first answer with rows and no total makes the endpoint NOT
    COUNTABLE and drops the cells so far (which can only have been empty answers)."""
    cells: list[Cell] = []
    for target in targets:
        tenant, _name = target
        held = (staged or {}).get(tenant)
        try:
            visma = visma_for(tenant)
            answer = ask(visma, plan)
            if answer.total is None:
                return EndpointResult(
                    plan.endpoint, plan.method, (), no_total_reason(answer.rows), "-"
                )
            beat = (beats or {}).get((tenant, plan.endpoint))
            cells.append(measure(visma, plan, target, answer.total, held, beat, guard))
        except VismaError as error:
            cells.append(error_cell(plan, target, held, str(error)))
    return EndpointResult(
        plan.endpoint, plan.method, tuple(cells), None, guard.state(plan.endpoint)
    )


def guarded_check(
    plan: Plan,
    targets: list[tuple[str, str]],
    staged: dict[str, Staged] | None,
    beats: dict[tuple[str, str], datetime] | None,
    visma_for: Callable[[str], Visma],
    guard: FilterGuard,
) -> EndpointResult:
    """check_endpoint, but a bug inside it makes that endpoint's cells ERROR (its type name only)
    and does not end a run that has already spent its Visma calls."""
    try:
        return check_endpoint(plan, targets, staged, beats, visma_for, guard)
    except Exception as error:
        reason = f"gate error: {type(error).__name__}"
        cells = tuple(error_cell(plan, t, (staged or {}).get(t[0]), reason) for t in targets)
        return EndpointResult(plan.endpoint, plan.method, cells, None, guard.state(plan.endpoint))


def load_configs() -> dict[str, dict[str, Any]]:
    """The erp_rest endpoint configs by endpoint name (the name of the staging table)."""
    configs: dict[str, dict[str, Any]] = {}
    for path in sorted(CONFIGS.glob("*.json")):
        if path.name.startswith("_"):
            continue
        config = json.loads(path.read_text(encoding="utf-8"))
        if config.get("source") == "erp_rest":
            configs[config["endpoint_name"]] = config
    return configs


def run_query(client: bigquery.Client, sql: str) -> list[dict[str, Any]]:
    return [dict(row.items()) for row in client.query(sql, location=LOCATION).result()]


def staged_by_tenant(client: bigquery.Client, endpoint: str) -> dict[str, Staged] | None:
    """Live rows and last write per tenant for one staging table; None when it does not exist."""
    if not re.fullmatch(r"[a-z0-9_]+", endpoint):
        raise ValueError(f"not an endpoint name: {endpoint!r}")
    sql = (
        "SELECT tenant_id, COUNTIF(deleted_at IS NULL) AS live, MAX(updated_at) AS last_write "
        f"FROM `{STAGING}.{endpoint}` GROUP BY tenant_id"
    )
    try:
        rows = run_query(client, sql)
    except NotFound:
        return None
    return {r["tenant_id"]: Staged(int(r["live"]), r["last_write"]) for r in rows}


def heartbeats(client: bigquery.Client) -> dict[tuple[str, str], datetime] | None:
    """The last successful sync per (tenant, endpoint); None when the table does not exist yet."""
    sql = (
        "SELECT tenant_id, endpoint, MAX(finished_at) AS last_at "
        f"FROM `{HEARTBEAT}` WHERE status = 'succeeded' GROUP BY tenant_id, endpoint"
    )
    try:
        rows = run_query(client, sql)
    except NotFound:
        return None
    return {(r["tenant_id"], r["endpoint"]): r["last_at"] for r in rows}


def staged_all(
    client: bigquery.Client, endpoints: list[str]
) -> dict[str, dict[str, Staged] | None]:
    with ThreadPoolExecutor(STAGING_WORKERS) as pool:
        found = list(pool.map(lambda name: staged_by_tenant(client, name), endpoints))
    return dict(zip(endpoints, found, strict=True))


def fmt(value: int | None) -> str:
    return "-" if value is None else f"{value:,}"


def _verdict_line(cells: list[Cell]) -> str:
    tally = Counter(cell.verdict for cell in cells)
    exact = sum(cell.verdict == "PASS" and cell.missing == 0 for cell in cells)
    rest = ", ".join(f"{w} {tally.get(w, 0)}" for w in ("WARN", "FAIL", "ERROR"))
    return (
        f"- Cells: PASS {tally.get('PASS', 0)} ({exact} exact, {tally.get('PASS', 0) - exact} "
        f"within the tolerance), {rest}"
    )


def _endpoint_table(counted: list[EndpointResult]) -> list[str]:
    """One row per endpoint. Missing is summed over the tenants where something is missing and
    Allowed is what excuses it (the modified-since count; 0 where no filter works)."""
    lines = [
        "| Endpoint | PASS | WARN | FAIL | ERROR | Visma | Staging | Missing | Allowed "
        "| Modified filter | Method |",
        "|---|---:|---:|---:|---:|---:|---:|---:|---:|---|---|",
    ]
    for r in counted:
        gaps = [c for c in r.cells if (c.missing or 0) > 0]
        lines.append(
            f"| {r.endpoint} | {r.count('PASS')} | {r.count('WARN')} | {r.count('FAIL')} "
            f"| {r.count('ERROR')} | {sum(c.visma_total or 0 for c in r.cells):,} "
            f"| {sum(c.staging_total for c in r.cells):,} | {sum(c.missing or 0 for c in gaps):,} "
            f"| {sum(c.changed or 0 for c in gaps):,} | {r.modified_filter} | {r.method} |"
        )
    return lines


def _flagged_table(cells: list[Cell]) -> list[str]:
    flagged = [c for c in cells if c.verdict != "PASS"]
    if not flagged:
        return ["None."]
    lines = [
        "| Endpoint | Tenant | Visma | Staging | Missing | Changed since last write "
        "| Tolerance basis | Last write (source) | Verdict | Why |",
        "|---|---|---:|---:|---:|---:|---|---|---|---|",
    ]
    for c in flagged:
        lines.append(
            f"| {c.endpoint} | {c.name} | {fmt(c.visma_total)} | {c.staging_total:,} "
            f"| {fmt(c.missing)} | {fmt(c.changed)} | {c.basis} "
            f"| {c.last_write or 'none'} ({c.last_write_source}) | {c.verdict} | {c.reason} |"
        )
    return lines


def render(results: list[EndpointResult], tenant_names: list[str], info: dict[str, Any]) -> str:
    counted = [r for r in results if r.not_countable is None]
    cells = [cell for r in counted for cell in r.cells]
    lines = [
        f"# Endpoint counts: Visma against staging ({info['day']})",
        "",
        "Rule: FAIL when more records are missing from staging than Visma reports as modified",
        "since the last write. Where an endpoint has no modified filter that works, or a tenant",
        "has no write on record, the tolerance is 0. See scripts/verify_endpoint_counts.py.",
        "",
        f"- Last write: {info['write_source']}.",
        f"- Scope: {len(counted)} endpoints counted for {len(tenant_names)} tenants "
        f"({len(cells)} cells); {len(results) - len(counted)} NOT COUNTABLE.",
        _verdict_line(cells),
        f"- Visma calls: {info['calls']}.",
        "",
        "## Per endpoint",
        "",
        "Missing is summed over the tenants where records are missing; Allowed is what excuses",
        "it (the modified-since count, 0 where no filter works).",
        "",
        *_endpoint_table(counted),
        "",
        "## Cells that are not PASS",
        "",
        *_flagged_table(cells),
        "",
        "## NOT COUNTABLE",
        "",
        "| Endpoint | Reason |",
        "|---|---|",
        *[f"| {r.endpoint} | {r.not_countable} |" for r in results if r.not_countable],
    ]
    return "\n".join(lines) + "\n"


def report(
    results: list[EndpointResult], tenant_names: list[str], info: dict[str, Any], subset: bool
) -> Path:
    GATES.mkdir(parents=True, exist_ok=True)
    md = GATES / f"endpoint_counts_{info['day']}{'_subset' if subset else ''}.md"
    md.write_text(render(results, tenant_names, info), encoding="utf-8")
    payload = {
        **info,
        "subset": subset,
        "tenants": tenant_names,
        "endpoints": [asdict(r) for r in results],
    }
    md.with_suffix(".json").write_text(json.dumps(payload, indent=1), encoding="utf-8")
    return md


def print_endpoint(result: EndpointResult) -> None:
    if result.not_countable is not None:
        print(f"{result.endpoint:34} NOT COUNTABLE: {result.not_countable}", flush=True)
        return
    tally = " ".join(f"{w} {result.count(w):>2}" for w in ("PASS", "WARN", "FAIL", "ERROR"))
    print(f"{result.endpoint:34} {tally}", flush=True)
    for c in result.cells:
        if c.verdict != "PASS":
            print(
                f"    {c.name:30} visma {fmt(c.visma_total):>9} staging {c.staging_total:>9,} "
                f"changed {fmt(c.changed):>7}  {c.verdict}: {c.reason}",
                flush=True,
            )


def select(
    ap: argparse.ArgumentParser, args: argparse.Namespace, configs: dict[str, dict[str, Any]]
) -> tuple[list[str], list[tuple[str, str]], bool]:
    """The endpoints and (tenant, name) pairs to check, and whether that is a subset."""
    wanted = {e for e in args.endpoints.split(",") if e}
    if wanted - configs.keys():
        ap.error(f"unknown endpoints: {sorted(wanted - configs.keys())}")
    chosen = {t for t in args.tenants.split(",") if t}
    targets = [(t, name) for t, name, _kind in tenants.TENANTS if not chosen or t in chosen]
    if not targets:
        ap.error("no tenants selected")
    return sorted(wanted or configs), targets, bool(wanted or chosen)


def run_all(
    names: list[str], targets: list[tuple[str, str]], configs: dict[str, dict[str, Any]]
) -> tuple[list[EndpointResult], bool]:
    """Every endpoint in order, printing each. Also whether a heartbeat table exists."""
    client = bigquery.Client(project=PROJECT)
    beats = heartbeats(client)
    staged = staged_all(client, [n for n in names if n not in NOT_COUNTABLE])
    guard = FilterGuard()
    clients: dict[str, Visma] = {}

    def visma_for(tenant: str) -> Visma:
        return clients.setdefault(tenant, Visma(tenant))

    results: list[EndpointResult] = []
    for name in names:
        if name in NOT_COUNTABLE:
            result = EndpointResult(name, "-", (), NOT_COUNTABLE[name], "-")
        else:
            plan = plan_for(configs[name])
            result = guarded_check(plan, targets, staged[name], beats, visma_for, guard)
        results.append(result)
        print_endpoint(result)
    return results, beats is not None


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--tenants", default="", help="comma-separated tenant ids (default: all)")
    ap.add_argument("--endpoints", default="", help="comma-separated endpoints (default: all)")
    ap.add_argument("--date", default=date.today().isoformat(), help="the date in the report name")
    args = ap.parse_args()
    configs = load_configs()
    names, targets, subset = select(ap, args, configs)

    results, has_heartbeat = run_all(names, targets, configs)

    info = {
        "day": args.date,
        "write_source": (
            "the heartbeat table where a row exists, else the last staging write"
            if has_heartbeat
            else WRITE_SOURCE
        ),
        "heartbeat_table": has_heartbeat,
        "calls": dict(PACER.calls),
    }
    md = report(results, [name for _t, name in targets], info, subset)
    cells = [cell for r in results for cell in r.cells]
    code = 1 if any(c.verdict in ("FAIL", "ERROR") for c in cells) else 0
    print(f"cells: {dict(Counter(c.verdict for c in cells))}")
    print(f"Visma calls: {dict(PACER.calls)}")
    print(f"wrote {md} (exit {code})")
    return code


if __name__ == "__main__":
    sys.exit(main())
