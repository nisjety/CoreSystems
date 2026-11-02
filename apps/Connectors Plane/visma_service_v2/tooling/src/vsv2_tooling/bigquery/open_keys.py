"""vsv2-bq-open-keys: the keys of a tenant's open documents in an endpoint's staging table.

The Rust orchestrator starts this for open-set reconciliation
(`core/crates/vsv2-orchestrator/src/open_set.rs`). A sync that follows `lastModifiedDateTime`
never sees an invoice closed by a payment, so once a day it compares the invoices staging holds as
open with Visma's own list of open invoices. This is the staging half: it reads, and writes
nothing.

One SELECT on `<project>.<dataset>.<table>` for the tenant's live rows (`deleted_at IS NULL`)
whose payload says the document is open:

    JSON_VALUE(payload, '$.status') = 'Open'

stdout is one JSON object per document: the endpoint's key fields as they are in the payload, and
the balance staging holds for it,

    {"documentType": "Invoice", "referenceNumber": "210507", "balance": "1450.5"}

A dotted key field such as `customer.number` is read from the nested object and printed flat,
under its dotted name. Lines come in no particular order. Non-ASCII text is escaped (`\\u00c6`), so
the output does not depend on the locale of the process; any JSON reader gives the text back.

`balance` is what lets the orchestrator see a part payment, which leaves an invoice `Open` with
the same `lastModifiedDateTime` and moves only its balance. It is the text `JSON_VALUE` returns
for `$.balance` (a JSON number comes back as its text, `1450.5`; a string as it is), neither
rounded nor reformatted, and `null` when the payload has no such value. `balance` is the
remaining balance in base currency of both invoice endpoints (`balanceInCurrency` is the same in
the document's currency); the orchestrator compares it with the `balance` of Visma's list row, so
the two sides have to name the same field (`BALANCE_FIELD` in `open_set.rs`).

What the query reads: `tenant_id`, `deleted_at` and three kinds of JSON path of `payload` (the
status, the key fields and the balance), never the rest of the payload. The last line of stderr is
a summary, `{"keys": N, "bytes_processed": B, "bytes_billed": C}`, so a log shows what a run
cost. With `--dry-run` nothing is read: BigQuery says what the query would process and stdout is
`{"dry_run": true, "bytes_processed": B}`. A failure is one line, `error: ...`, on stderr and a
non-zero exit, so that the orchestrator's error text is the reason and not a traceback.

A key field that is NULL in a matching row is an error, not a skipped row: a staged document
without its key would look like one that is not open, and the diff against Visma would be wrong.
A NULL balance is not an error: it is printed as `null` and the orchestrator treats it as unknown.
"""

from __future__ import annotations

import json
import re
from collections.abc import Sequence
from dataclasses import dataclass
from typing import Annotated

import typer
from google.api_core.exceptions import GoogleAPICallError, NotFound
from google.cloud import bigquery

from vsv2_tooling.bigquery.staging import default_location

DEFAULT_STATUS_FIELD = "status"
DEFAULT_OPEN_STATUS = "Open"
# The payload field that holds what is still owed on a document: `balance` on customer and
# supplier invoices alike (base currency; `balanceInCurrency` is the same in the document's
# currency). The orchestrator reads the same field from Visma's list rows (`BALANCE_FIELD` in
# `open_set.rs`), so the two have to stay in step.
DEFAULT_BALANCE_FIELD = "balance"
# The name the staged balance is printed under, next to the key fields (and the name of the
# column it is selected as). A key field cannot have it: the two would be one entry.
BALANCE_OUTPUT_FIELD = "balance"
# Seconds to wait for the query. It reads one tenant's open rows of one table.
QUERY_TIMEOUT_S = 300

# A key field, the status field or the balance field is written into the SQL as a JSON path, so it
# has to be plain names (dotted for a nested field) and nothing else.
_FIELD_PATH = re.compile(r"[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)*")
# Project, dataset and table ids sit inside backticks.
_TABLE_PART = re.compile(r"[A-Za-z0-9_.:-]+")


@dataclass(frozen=True)
class OpenKeys:
    """What the query found and what it cost.

    One entry of `keys` per open document: its key fields, and its staged balance under
    `BALANCE_OUTPUT_FIELD` (`None` when the payload has none). That is the line that is printed.
    """

    keys: list[dict[str, str | None]]
    bytes_processed: int | None
    bytes_billed: int | None


def parse_key_fields(pk: str) -> list[str]:
    """The key fields of `--pk`, comma-separated, each a plain (dotted) field name."""
    fields = [part.strip() for part in pk.split(",") if part.strip()]
    if not fields:
        raise ValueError("--pk names no key field")
    for field in fields:
        _check_field(field)
    return fields


def _check_field(field: str) -> None:
    if not _FIELD_PATH.fullmatch(field):
        raise ValueError(f"{field!r} is not a plain field name (letters, digits, _ and dots)")


def table_id(project: str, dataset: str, table: str) -> str:
    for part in (project, dataset, table):
        if not _TABLE_PART.fullmatch(part):
            raise ValueError(f"{part!r} is not a valid project, dataset or table id")
    return f"{project}.{dataset}.{table}"


def build_query(
    target_id: str,
    key_fields: Sequence[str],
    status_field: str,
    balance_field: str = DEFAULT_BALANCE_FIELD,
) -> str:
    """The SELECT. Tenant and open status are parameters; the field paths are checked names.

    It selects the key fields and the balance, each as one JSON path of `payload`, and filters on
    the status path. The balance is the one JSON value more than the key fields that spotting a
    part payment needs; the rest of the payload is never named, so it is never read.
    """
    if not key_fields:
        raise ValueError("a query needs at least one key field")
    for field in (*key_fields, status_field, balance_field):
        _check_field(field)
    if BALANCE_OUTPUT_FIELD in key_fields:
        raise ValueError(
            f"{BALANCE_OUTPUT_FIELD!r} cannot be a key field: the output prints the staged "
            "balance under that name"
        )
    columns = ", ".join(
        f"JSON_VALUE(payload, '$.{field}') AS k{index}" for index, field in enumerate(key_fields)
    )
    return (
        f"SELECT {columns}, JSON_VALUE(payload, '$.{balance_field}') AS {BALANCE_OUTPUT_FIELD} "
        f"FROM `{target_id}` "
        "WHERE tenant_id = @tenant AND deleted_at IS NULL "
        f"AND JSON_VALUE(payload, '$.{status_field}') = @open_status"
    )


def _prepared_sql(
    *,
    project: str,
    dataset: str,
    table: str,
    tenant: str,
    key_fields: Sequence[str],
    status_field: str,
    balance_field: str,
) -> str:
    if not tenant.strip():
        raise ValueError("--tenant is empty")
    return build_query(table_id(project, dataset, table), key_fields, status_field, balance_field)


def _job_config(tenant: str, open_status: str, *, dry_run: bool) -> bigquery.QueryJobConfig:
    return bigquery.QueryJobConfig(
        query_parameters=[
            bigquery.ScalarQueryParameter("tenant", "STRING", tenant),
            bigquery.ScalarQueryParameter("open_status", "STRING", open_status),
        ],
        dry_run=dry_run,
        use_query_cache=not dry_run,
    )


def open_keys(
    *,
    project: str,
    dataset: str,
    table: str,
    tenant: str,
    key_fields: Sequence[str],
    status_field: str = DEFAULT_STATUS_FIELD,
    open_status: str = DEFAULT_OPEN_STATUS,
    balance_field: str = DEFAULT_BALANCE_FIELD,
    location: str | None = None,
    client: bigquery.Client | None = None,
) -> OpenKeys:
    """The keys, and staged balances, of `tenant`'s open documents in `project.dataset.table`."""
    sql = _prepared_sql(
        project=project,
        dataset=dataset,
        table=table,
        tenant=tenant,
        key_fields=key_fields,
        status_field=status_field,
        balance_field=balance_field,
    )
    client = client or bigquery.Client(project=project)
    job = client.query(
        sql,
        job_config=_job_config(tenant, open_status, dry_run=False),
        location=location or default_location(),
    )
    keys: list[dict[str, str | None]] = []
    for row in job.result(timeout=QUERY_TIMEOUT_S):
        key: dict[str, str | None] = {}
        for index, field in enumerate(key_fields):
            value = row[f"k{index}"]
            if value is None or not str(value).strip():
                raise ValueError(
                    f"a staged open document of tenant {tenant} has no value for key field "
                    f"{field!r}; it cannot be compared with Visma"
                )
            key[field] = str(value)
        # The text the payload holds, as BigQuery gives it: not parsed, not rounded. A document
        # without a balance is printed with `null`; the orchestrator reads that as unknown.
        balance = row[BALANCE_OUTPUT_FIELD]
        key[BALANCE_OUTPUT_FIELD] = None if balance is None else str(balance)
        keys.append(key)
    return OpenKeys(
        keys=keys,
        bytes_processed=job.total_bytes_processed,
        bytes_billed=job.total_bytes_billed,
    )


def dry_run_bytes(
    *,
    project: str,
    dataset: str,
    table: str,
    tenant: str,
    key_fields: Sequence[str],
    status_field: str = DEFAULT_STATUS_FIELD,
    open_status: str = DEFAULT_OPEN_STATUS,
    balance_field: str = DEFAULT_BALANCE_FIELD,
    location: str | None = None,
    client: bigquery.Client | None = None,
) -> int:
    """Bytes BigQuery says the query would process. Nothing is read and nothing is billed."""
    sql = _prepared_sql(
        project=project,
        dataset=dataset,
        table=table,
        tenant=tenant,
        key_fields=key_fields,
        status_field=status_field,
        balance_field=balance_field,
    )
    client = client or bigquery.Client(project=project)
    job = client.query(
        sql,
        job_config=_job_config(tenant, open_status, dry_run=True),
        location=location or default_location(),
    )
    return int(job.total_bytes_processed or 0)


app = typer.Typer(add_completion=False)


def _fail(message: str, code: int) -> typer.Exit:
    """Print `message` as one line on stderr, the line the orchestrator reads, and exit `code`.

    BigQuery errors carry a footer (`Location: ...`, `Job ID: ...`) on lines of their own.
    """
    typer.echo(f"error: {' '.join(message.split())}", err=True)
    return typer.Exit(code)


@app.command()
def run(
    project: Annotated[str, typer.Option(help="GCP project id")],
    dataset: Annotated[str, typer.Option(help="Staging dataset")],
    table: Annotated[str, typer.Option(help="Staging table (the endpoint name)")],
    tenant: Annotated[str, typer.Option(help="Tenant whose open documents are listed")],
    pk: Annotated[
        str,
        typer.Option(help="Key fields of a document, comma-separated, as named in the payload"),
    ],
    status_field: Annotated[
        str, typer.Option("--status-field", help="Payload field that says whether it is open")
    ] = DEFAULT_STATUS_FIELD,
    open_status: Annotated[
        str, typer.Option("--open-status", help="Value of the status field of an open document")
    ] = DEFAULT_OPEN_STATUS,
    balance_field: Annotated[
        str,
        typer.Option(
            "--balance-field", help="Payload field with the document's balance, printed as balance"
        ),
    ] = DEFAULT_BALANCE_FIELD,
    dry_run: Annotated[
        bool,
        typer.Option("--dry-run", help="Only ask BigQuery what the query would process"),
    ] = False,
) -> None:
    """Print the tenant's open documents, one JSON object per line: key fields and balance.

    Read only.
    """
    try:
        fields = parse_key_fields(pk)
        if dry_run:
            processed = dry_run_bytes(
                project=project,
                dataset=dataset,
                table=table,
                tenant=tenant,
                key_fields=fields,
                status_field=status_field,
                open_status=open_status,
                balance_field=balance_field,
            )
            typer.echo(json.dumps({"dry_run": True, "bytes_processed": processed}))
            return
        found = open_keys(
            project=project,
            dataset=dataset,
            table=table,
            tenant=tenant,
            key_fields=fields,
            status_field=status_field,
            open_status=open_status,
            balance_field=balance_field,
        )
    except ValueError as error:
        raise _fail(str(error), 2) from error
    except NotFound as error:
        raise _fail(f"{project}.{dataset}.{table} was not found: {error}", 2) from error
    except GoogleAPICallError as error:
        raise _fail(f"BigQuery refused the query: {error}", 1) from error
    except Exception as error:  # a timeout, no credentials, a retry that gave up: still one line
        raise _fail(f"{type(error).__name__}: {error}", 1) from error
    for key in found.keys:
        typer.echo(json.dumps(key))
    summary = {
        "keys": len(found.keys),
        "bytes_processed": found.bytes_processed,
        "bytes_billed": found.bytes_billed,
    }
    typer.echo(json.dumps(summary), err=True)


if __name__ == "__main__":
    app()
