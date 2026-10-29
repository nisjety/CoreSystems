"""vsv2-bq-open-keys: the keys of a tenant's open documents in staging, without BigQuery.

The client is a fake that can only `query`: a tool that is meant to read cannot write through it.
"""

from __future__ import annotations

import json
import re
from typing import Any
from unittest.mock import patch

import pytest
from google.api_core.exceptions import Forbidden, NotFound
from google.auth.exceptions import DefaultCredentialsError
from google.cloud.bigquery.table import Row
from typer.testing import CliRunner

from vsv2_tooling.bigquery.open_keys import (
    BALANCE_OUTPUT_FIELD,
    QUERY_TIMEOUT_S,
    app,
    build_query,
    dry_run_bytes,
    open_keys,
    parse_key_fields,
    table_id,
)

TENANT = "33f319b9-4d57-11ee-960c-025417856183"
COMPOSITE = ["documentType", "referenceNumber"]


def row(*values: Any, balance: Any = None) -> Row:
    """A result row the way BigQuery hands it out: columns k0, k1, ... and `balance`."""
    fields = {f"k{index}": index for index in range(len(values))}
    fields["balance"] = len(values)
    return Row((*values, balance), fields)


class FakeJob:
    def __init__(
        self,
        rows: list[Row],
        processed: int | None,
        billed: int | None,
        result_error: Exception | None = None,
    ) -> None:
        self._rows = rows
        self._result_error = result_error
        self.total_bytes_processed = processed
        self.total_bytes_billed = billed
        self.waited_for: float | None = None
        self.read = False

    def result(self, timeout: float | None = None) -> Any:
        self.read = True
        self.waited_for = timeout
        if self._result_error:
            raise self._result_error
        return iter(self._rows)


class FakeClient:
    """Answers `query` and nothing else."""

    def __init__(
        self,
        rows: list[Row] | None = None,
        *,
        processed: int | None = 4_096,
        billed: int | None = 10_485_760,
        error: Exception | None = None,
        result_error: Exception | None = None,
    ) -> None:
        self._rows = rows or []
        self._processed = processed
        self._billed = billed
        self._error = error
        self._result_error = result_error
        self.queries: list[dict[str, Any]] = []
        self.jobs: list[FakeJob] = []

    def query(self, sql: str, job_config: Any = None, location: str | None = None) -> FakeJob:
        self.queries.append({"sql": sql, "config": job_config, "location": location})
        if self._error:
            raise self._error
        job = FakeJob(self._rows, self._processed, self._billed, self._result_error)
        self.jobs.append(job)
        return job


def parameters(call: dict[str, Any]) -> dict[str, Any]:
    return {p.name: p.value for p in call["config"].query_parameters}


def run_open_keys(client: FakeClient, key_fields: list[str] | None = None) -> Any:
    return open_keys(
        project="aquatiq-datavarehus",
        dataset="dw_staging_visma",
        table="supplier_invoices",
        tenant=TENANT,
        key_fields=key_fields or COMPOSITE,
        client=client,  # type: ignore[arg-type]
    )


# ─── the query ────────────────────────────────────────────────────────────────


@pytest.mark.unit
def test_the_query_reads_the_tenants_live_open_rows_and_only_their_key_fields_and_balance() -> None:
    client = FakeClient([])

    run_open_keys(client)

    sql = client.queries[0]["sql"]
    assert sql.lstrip().upper().startswith("SELECT"), "a read, never a write"
    assert "`aquatiq-datavarehus.dw_staging_visma.supplier_invoices`" in sql
    assert "tenant_id = @tenant" in sql
    assert "deleted_at IS NULL" in sql
    assert "JSON_VALUE(payload, '$.status') = @open_status" in sql
    assert "JSON_VALUE(payload, '$.documentType') AS k0" in sql
    assert "JSON_VALUE(payload, '$.referenceNumber') AS k1" in sql
    select_list = sql.split(" FROM ")[0]
    assert select_list == (
        "SELECT JSON_VALUE(payload, '$.documentType') AS k0, "
        "JSON_VALUE(payload, '$.referenceNumber') AS k1, "
        "JSON_VALUE(payload, '$.balance') AS balance"
    ), "the key fields and the one balance value: not the payload, not a *"


@pytest.mark.unit
def test_the_balance_is_the_only_json_path_the_query_reads_besides_status_and_the_keys() -> None:
    """The cost property: `payload` is only ever named through a JSON_VALUE path, and the paths
    are the status, the key fields and the balance. Nothing else of the payload is read."""
    client = FakeClient([])

    run_open_keys(client)

    sql = client.queries[0]["sql"]
    assert re.findall(r"payload(?!, '\$\.)", sql) == [], "payload is not named bare"
    assert sorted(re.findall(r"JSON_VALUE\(payload, '\$\.(\w+)'\)", sql)) == [
        "balance",
        "documentType",
        "referenceNumber",
        "status",
    ]
    assert "*" not in sql
    assert "JSON_QUERY" not in sql
    assert "TO_JSON" not in sql


@pytest.mark.unit
def test_the_balance_field_can_be_named_and_is_still_printed_as_balance() -> None:
    client = FakeClient([row("Invoice", "1", balance="75.5")])

    found = open_keys(
        project="p",
        dataset="d",
        table="t",
        tenant=TENANT,
        key_fields=COMPOSITE,
        balance_field="balanceInCurrency",
        client=client,  # type: ignore[arg-type]
    )

    assert "JSON_VALUE(payload, '$.balanceInCurrency') AS balance" in client.queries[0]["sql"]
    assert "JSON_VALUE(payload, '$.balance')" not in client.queries[0]["sql"]
    assert found.keys[0]["balance"] == "75.5"


@pytest.mark.unit
def test_the_balance_field_is_checked_like_a_key_field() -> None:
    with pytest.raises(ValueError):
        build_query("p.d.t", ["id"], "status", "balance') OR TRUE --")


@pytest.mark.unit
def test_a_key_field_cannot_be_called_balance_it_would_be_overwritten_in_the_output() -> None:
    client = FakeClient([])

    with pytest.raises(ValueError, match="balance"):
        run_open_keys(client, ["documentType", BALANCE_OUTPUT_FIELD])

    assert client.queries == []


@pytest.mark.unit
def test_tenant_and_open_status_are_parameters_not_text() -> None:
    client = FakeClient([])

    run_open_keys(client)

    call = client.queries[0]
    assert parameters(call) == {"tenant": TENANT, "open_status": "Open"}
    assert TENANT not in call["sql"]
    assert call["config"].dry_run is False


@pytest.mark.unit
def test_a_real_run_may_use_the_query_cache_and_waits_for_the_result_with_a_timeout() -> None:
    client = FakeClient([])

    run_open_keys(client)

    assert client.queries[0]["config"].use_query_cache is True
    assert client.jobs[0].waited_for == QUERY_TIMEOUT_S


@pytest.mark.unit
def test_the_query_runs_in_the_staging_location(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("VSV2_BIGQUERY_LOCATION", raising=False)
    client = FakeClient([])
    run_open_keys(client)
    assert client.queries[0]["location"] == "europe-north2"

    monkeypatch.setenv("VSV2_BIGQUERY_LOCATION", "EU")
    other = FakeClient([])
    run_open_keys(other)
    assert other.queries[0]["location"] == "EU"


@pytest.mark.unit
def test_the_status_field_and_value_can_be_named() -> None:
    client = FakeClient([])

    open_keys(
        project="p",
        dataset="d",
        table="t",
        tenant=TENANT,
        key_fields=["id"],
        status_field="state",
        open_status="Active",
        client=client,  # type: ignore[arg-type]
    )

    assert "JSON_VALUE(payload, '$.state') = @open_status" in client.queries[0]["sql"]
    assert parameters(client.queries[0])["open_status"] == "Active"


# ─── the keys ─────────────────────────────────────────────────────────────────


@pytest.mark.unit
def test_a_composite_key_comes_out_as_one_object_with_every_key_field_and_the_balance() -> None:
    client = FakeClient(
        [
            row("Invoice", "210507", balance="1450.5"),
            row("DebitAdj", "200089", balance="20"),
        ]
    )

    found = run_open_keys(client)

    assert found.keys == [
        {"documentType": "Invoice", "referenceNumber": "210507", "balance": "1450.5"},
        {"documentType": "DebitAdj", "referenceNumber": "200089", "balance": "20"},
    ]


@pytest.mark.unit
def test_a_single_key_field_is_one_entry_next_to_the_balance() -> None:
    client = FakeClient([row("023672", balance="10.25"), row("100196", balance="0")])

    found = run_open_keys(client, ["referenceNumber"])

    assert found.keys == [
        {"referenceNumber": "023672", "balance": "10.25"},
        {"referenceNumber": "100196", "balance": "0"},
    ]


@pytest.mark.unit
def test_no_open_rows_is_no_keys() -> None:
    found = run_open_keys(FakeClient([]))

    assert found.keys == []


@pytest.mark.unit
def test_a_leading_zero_survives() -> None:
    found = run_open_keys(FakeClient([row("000196")]), ["referenceNumber"])

    assert found.keys[0]["referenceNumber"] == "000196"


@pytest.mark.unit
@pytest.mark.parametrize("held", ["1450.5", "1450.50", "0", "-12.34", "1e-05", "123456789.123456"])
def test_the_balance_is_the_text_the_payload_holds_not_a_rounded_number(held: str) -> None:
    found = run_open_keys(FakeClient([row("023672", balance=held)]), ["referenceNumber"])

    assert found.keys[0]["balance"] == held
    assert isinstance(found.keys[0]["balance"], str)


@pytest.mark.unit
def test_a_document_without_a_balance_gets_a_null_balance_and_is_not_an_error() -> None:
    found = run_open_keys(FakeClient([row("023672", balance=None)]), ["referenceNumber"])

    assert found.keys == [{"referenceNumber": "023672", "balance": None}]


@pytest.mark.unit
@pytest.mark.parametrize("missing", [None, "", "   "])
def test_a_staged_open_document_without_its_key_is_an_error_not_a_skipped_row(
    missing: str | None,
) -> None:
    client = FakeClient([row("Invoice", "1"), row("Invoice", missing)])

    with pytest.raises(ValueError, match="referenceNumber"):
        run_open_keys(client)


@pytest.mark.unit
def test_what_the_query_cost_is_reported_with_the_keys() -> None:
    found = run_open_keys(FakeClient([row("Invoice", "1")], processed=123_456, billed=10_485_760))

    assert (found.bytes_processed, found.bytes_billed) == (123_456, 10_485_760)


@pytest.mark.unit
def test_a_job_without_byte_counts_is_reported_as_unknown_not_as_zero() -> None:
    found = run_open_keys(FakeClient([row("Invoice", "1")], processed=None, billed=None))

    assert (found.bytes_processed, found.bytes_billed) == (None, None)


# ─── the dry run ──────────────────────────────────────────────────────────────


@pytest.mark.unit
def test_a_dry_run_asks_for_the_bytes_and_reads_nothing() -> None:
    client = FakeClient([row("Invoice", "1")], processed=987_654)

    processed = dry_run_bytes(
        project="aquatiq-datavarehus",
        dataset="dw_staging_visma",
        table="customer_invoices",
        tenant=TENANT,
        key_fields=["referenceNumber"],
        client=client,  # type: ignore[arg-type]
    )

    assert processed == 987_654
    assert client.queries[0]["config"].dry_run is True
    assert client.queries[0]["config"].use_query_cache is False, "a cache hit would say 0 bytes"
    assert client.jobs[0].read is False, "a dry run has no rows to read"


@pytest.mark.unit
def test_a_dry_run_prices_the_same_query_a_real_run_sends() -> None:
    """The bytes a dry run reports must be those of the query that is really run: the balance
    included."""
    dry, real = FakeClient([]), FakeClient([])
    where = {
        "project": "aquatiq-datavarehus",
        "dataset": "dw_staging_visma",
        "table": "customer_invoices",
        "tenant": TENANT,
        "key_fields": ["referenceNumber"],
    }

    dry_run_bytes(**where, client=dry)  # type: ignore[arg-type]
    open_keys(**where, client=real)  # type: ignore[arg-type]

    assert dry.queries[0]["sql"] == real.queries[0]["sql"]
    assert "'$.balance'" in dry.queries[0]["sql"]


# ─── what is allowed into the SQL ─────────────────────────────────────────────


@pytest.mark.unit
def test_key_fields_are_split_on_commas_and_trimmed() -> None:
    assert parse_key_fields("documentType, referenceNumber ") == COMPOSITE
    assert parse_key_fields("referenceNumber") == ["referenceNumber"]
    assert parse_key_fields("customer.number") == ["customer.number"]


@pytest.mark.unit
@pytest.mark.parametrize(
    "bad",
    ["", " , ", "a b", "1a", "a..b", ".a", "a.", "a'); DROP TABLE x; --", "a-b", "a$b", "$.a"],
)
def test_a_key_field_that_is_not_a_plain_name_is_refused(bad: str) -> None:
    with pytest.raises(ValueError):
        parse_key_fields(bad)


@pytest.mark.unit
def test_the_status_field_is_checked_like_a_key_field() -> None:
    with pytest.raises(ValueError):
        build_query("p.d.t", ["id"], "status') OR TRUE --")


@pytest.mark.unit
@pytest.mark.parametrize("fields", [["a') OR TRUE --"], ["id", "x y"], ["id", "1a"]])
def test_every_key_field_is_checked_where_the_sql_is_built(fields: list[str]) -> None:
    with pytest.raises(ValueError):
        build_query("p.d.t", fields, "status")


@pytest.mark.unit
def test_a_query_without_key_fields_is_refused_not_built_as_select_from() -> None:
    with pytest.raises(ValueError, match="at least one key field"):
        build_query("p.d.t", [], "status")


@pytest.mark.unit
def test_open_keys_refuses_a_bad_key_field_before_any_query() -> None:
    client = FakeClient([])

    with pytest.raises(ValueError):
        run_open_keys(client, ["a') OR TRUE --"])

    assert client.queries == []


@pytest.mark.unit
@pytest.mark.parametrize(
    ("project", "dataset", "table"),
    [
        ("p`; DROP TABLE x; --", "d", "t"),
        ("p", "d d", "t"),
        ("p", "d", "t`"),
        ("p", "d", ""),
        ("p", "d", "a/b"),
    ],
)
def test_a_table_id_that_could_leave_its_backticks_is_refused(
    project: str, dataset: str, table: str
) -> None:
    with pytest.raises(ValueError):
        table_id(project, dataset, table)


@pytest.mark.unit
def test_the_real_ids_pass() -> None:
    assert (
        table_id("aquatiq-datavarehus", "dw_staging_visma", "customer_invoices")
        == "aquatiq-datavarehus.dw_staging_visma.customer_invoices"
    )


@pytest.mark.unit
def test_an_empty_tenant_is_refused_before_any_query() -> None:
    client = FakeClient([])

    with pytest.raises(ValueError, match="tenant"):
        open_keys(
            project="p",
            dataset="d",
            table="t",
            tenant="  ",
            key_fields=["id"],
            client=client,  # type: ignore[arg-type]
        )

    assert client.queries == []


# ─── the command ──────────────────────────────────────────────────────────────

ARGS = [
    "--project",
    "aquatiq-datavarehus",
    "--dataset",
    "dw_staging_visma",
    "--table",
    "supplier_invoices",
    "--tenant",
    TENANT,
    "--pk",
    "documentType,referenceNumber",
]


def invoke(client: FakeClient, *extra: str) -> Any:
    with patch("vsv2_tooling.bigquery.open_keys.bigquery.Client", return_value=client):
        return CliRunner().invoke(app, [*ARGS, *extra])


@pytest.mark.unit
def test_stdout_is_one_json_object_per_key_and_stderr_ends_with_the_cost() -> None:
    client = FakeClient(
        [
            row("Invoice", "210507", balance="1450.5"),
            row("DebitAdj", "200089", balance="20"),
        ],
        processed=2_048,
        billed=10_485_760,
    )

    result = invoke(client)

    assert result.exit_code == 0, result.output
    keys = [json.loads(line) for line in result.stdout.splitlines()]
    assert keys == [
        {"documentType": "Invoice", "referenceNumber": "210507", "balance": "1450.5"},
        {"documentType": "DebitAdj", "referenceNumber": "200089", "balance": "20"},
    ]
    summary = json.loads(result.stderr.strip().splitlines()[-1])
    assert summary == {"keys": 2, "bytes_processed": 2_048, "bytes_billed": 10_485_760}


@pytest.mark.unit
def test_a_document_without_a_balance_prints_null_for_it() -> None:
    result = invoke(FakeClient([row("Invoice", "1", balance=None)]))

    assert result.exit_code == 0, result.output
    assert json.loads(result.stdout) == {
        "documentType": "Invoice",
        "referenceNumber": "1",
        "balance": None,
    }


@pytest.mark.unit
def test_the_balance_field_option_changes_the_path_and_not_the_printed_name() -> None:
    client = FakeClient([row("Invoice", "1", balance="5")])

    result = invoke(client, "--balance-field", "balanceInCurrency")

    assert result.exit_code == 0, result.output
    assert "JSON_VALUE(payload, '$.balanceInCurrency') AS balance" in client.queries[0]["sql"]
    assert json.loads(result.stdout)["balance"] == "5"


@pytest.mark.unit
def test_a_key_field_called_balance_is_refused_with_a_one_line_error() -> None:
    client = FakeClient([])
    with patch("vsv2_tooling.bigquery.open_keys.bigquery.Client", return_value=client):
        result = CliRunner().invoke(app, [*ARGS[:-1], "documentType,balance"])

    assert result.exit_code == 2
    assert result.stderr.startswith("error:")
    assert "balance" in result.stderr
    assert client.queries == []


@pytest.mark.unit
def test_no_open_documents_print_nothing_on_stdout_and_still_report_the_cost() -> None:
    result = invoke(FakeClient([]))

    assert result.exit_code == 0, result.output
    assert result.stdout == ""
    assert json.loads(result.stderr.strip().splitlines()[-1])["keys"] == 0


@pytest.mark.unit
def test_non_ascii_keys_are_escaped_on_stdout_and_read_back_as_the_same_text() -> None:
    client = FakeClient([row("Invoice", "Æ-12", balance="3")])

    result = invoke(client)

    assert result.stdout.isascii(), "no dependence on the locale of the process"
    assert json.loads(result.stdout) == {
        "documentType": "Invoice",
        "referenceNumber": "Æ-12",
        "balance": "3",
    }


@pytest.mark.unit
def test_dry_run_prints_the_bytes_and_reads_nothing() -> None:
    client = FakeClient([row("Invoice", "1")], processed=555)

    result = invoke(client, "--dry-run")

    assert result.exit_code == 0, result.output
    assert json.loads(result.stdout) == {"dry_run": True, "bytes_processed": 555}
    assert client.queries[0]["config"].dry_run is True
    assert client.jobs[0].read is False


@pytest.mark.unit
def test_a_table_that_does_not_exist_is_a_one_line_error_and_a_non_zero_exit() -> None:
    result = invoke(FakeClient(error=NotFound("Not found: Table x")))

    assert result.exit_code == 2
    assert result.stdout == ""
    assert result.stderr.startswith("error: aquatiq-datavarehus.dw_staging_visma.supplier_invoices")
    assert len(result.stderr.strip().splitlines()) == 1


@pytest.mark.unit
def test_an_error_with_the_job_footer_bigquery_adds_is_still_one_line() -> None:
    footer = "Not found: Table x\n\nLocation: europe-north2\nJob ID: 0a1b2c\n"

    result = invoke(FakeClient(error=NotFound(footer)))

    assert result.exit_code == 2
    assert len(result.stderr.strip().splitlines()) == 1, result.stderr
    assert "Job ID: 0a1b2c" in result.stderr


@pytest.mark.unit
def test_a_query_that_does_not_finish_in_time_is_one_line_and_not_a_traceback() -> None:
    result = invoke(
        FakeClient(result_error=TimeoutError("Operation timed out after 300.0 seconds"))
    )

    assert result.exit_code == 1
    assert result.stdout == ""
    assert result.stderr.startswith("error: TimeoutError: Operation timed out")
    assert len(result.stderr.strip().splitlines()) == 1, result.stderr


@pytest.mark.unit
def test_missing_credentials_are_one_line_and_not_a_traceback() -> None:
    failing = patch(
        "vsv2_tooling.bigquery.open_keys.bigquery.Client",
        side_effect=DefaultCredentialsError("Your default credentials were not found."),
    )
    with failing:
        result = CliRunner().invoke(app, ARGS)

    assert result.exit_code == 1
    assert result.stdout == ""
    assert result.stderr.startswith("error: DefaultCredentialsError: Your default credentials")
    assert len(result.stderr.strip().splitlines()) == 1, result.stderr


@pytest.mark.unit
def test_a_refused_query_is_a_one_line_error_and_a_non_zero_exit() -> None:
    result = invoke(FakeClient(error=Forbidden("no bigquery.jobs.create")))

    assert result.exit_code == 1
    assert result.stderr.startswith("error: BigQuery refused the query")


@pytest.mark.unit
def test_a_bad_key_field_never_reaches_bigquery() -> None:
    client = FakeClient([])
    with patch("vsv2_tooling.bigquery.open_keys.bigquery.Client", return_value=client):
        args = [*ARGS[:-1], "a; DROP TABLE x"]
        result = CliRunner().invoke(app, args)

    assert result.exit_code == 2
    assert result.stderr.startswith("error:")
    assert client.queries == []


@pytest.mark.unit
def test_a_staged_document_without_a_key_fails_the_command_and_nothing_is_printed() -> None:
    result = invoke(FakeClient([row("Invoice", "1"), row("Invoice", None)]))

    assert result.exit_code == 2
    assert "referenceNumber" in result.stderr
    assert result.stdout == "", "not even the keys read before the bad row: the set is untrusted"


@pytest.mark.unit
def test_a_dry_run_whose_job_has_no_byte_count_says_zero() -> None:
    client = FakeClient(processed=None)

    result = invoke(client, "--dry-run")

    assert json.loads(result.stdout) == {"dry_run": True, "bytes_processed": 0}


@pytest.mark.unit
def test_a_run_without_byte_counts_prints_null_in_the_summary() -> None:
    result = invoke(FakeClient([row("Invoice", "1")], processed=None, billed=None))

    summary = json.loads(result.stderr.strip().splitlines()[-1])
    assert summary == {"keys": 1, "bytes_processed": None, "bytes_billed": None}
