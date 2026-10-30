"""The endpoint-count gate's rule and request logic. Pure: no Visma, no BigQuery."""

from __future__ import annotations

import json
import re
import sys
import urllib.error
from dataclasses import asdict
from datetime import UTC, datetime, timedelta, timezone
from pathlib import Path
from typing import Any

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))

import verify_endpoint_counts as gate
from verify_endpoint_counts import (
    BASIS_FILTER,
    BASIS_NO_FILTER,
    BASIS_NO_WRITE,
    BASIS_NOT_NEEDED,
    NOT_COUNTABLE,
    WINDOWED,
    Cell,
    EndpointResult,
    FilterGuard,
    Staged,
    Visma,
    VismaError,
    check_endpoint,
    derive_filters,
    extract_total,
    guarded_check,
    judge,
    last_write,
    load_configs,
    modified_query,
    one_row_query,
    plan_for,
    render,
    row_count,
)

TENANT = "33f319b9-4d57-11ee-960c-025417856183"
OTHER = "ab0e5d06-fa25-11ed-a0e0-0693d8a7c3dd"
LAST_WRITE = datetime(2026, 10, 1, 13, 50, 35, 570527, tzinfo=UTC)


# The rule


@pytest.mark.unit
def test_missing_within_the_changes_since_the_last_write_passes() -> None:
    verdict = judge(visma_total=22_360, changed=387, staging_total=22_105)

    assert verdict.word == "PASS"
    assert "255 missing, within the 387" in verdict.reason


@pytest.mark.unit
def test_more_missing_than_could_have_changed_fails() -> None:
    verdict = judge(visma_total=22_360, changed=100, staging_total=22_105)

    assert verdict.word == "FAIL"
    assert "255" in verdict.reason
    assert "100" in verdict.reason


@pytest.mark.unit
def test_exactly_as_many_missing_as_changed_passes() -> None:
    assert judge(visma_total=617, changed=3, staging_total=614).word == "PASS"


@pytest.mark.unit
def test_staging_ahead_of_visma_is_a_warning_not_a_failure() -> None:
    verdict = judge(visma_total=100, changed=None, staging_total=103)

    assert verdict.word == "WARN"
    assert "3 more than Visma" in verdict.reason


@pytest.mark.unit
def test_equal_counts_pass_with_nothing_missing() -> None:
    verdict = judge(visma_total=50, changed=None, staging_total=50)

    assert verdict.word == "PASS"
    assert verdict.reason == "none missing"


@pytest.mark.unit
def test_without_a_changed_count_the_tolerance_is_zero() -> None:
    verdict = judge(visma_total=6_370, changed=None, staging_total=6_360)

    assert verdict.word == "FAIL"
    assert "10" in verdict.reason
    assert "tolerance 0" in verdict.reason


@pytest.mark.unit
def test_a_changed_count_of_zero_is_a_real_tolerance_of_zero() -> None:
    assert judge(visma_total=11, changed=0, staging_total=10).word == "FAIL"


# Reading Visma's answer


@pytest.mark.unit
@pytest.mark.parametrize(
    ("body", "expected"),
    [
        pytest.param(
            [{"metadata": {"totalCount": 23_511, "maxPageSize": 1000}}], 23_511, id="list"
        ),
        pytest.param([], 0, id="empty list"),
        pytest.param([{"metadata": {"totalCount": 1}}] * 617, 1, id="whole list, total in row 1"),
        pytest.param([{"cashAccount": "1"}], None, id="row without metadata"),
        pytest.param([{"metadata": {"maxPageSize": 100}}], None, id="metadata without total"),
        pytest.param({"totalCount": 22_360, "nextPage": None, "value": [{}]}, 22_360, id="so"),
        pytest.param({"totalCount": 22, "records": [{}]}, 22, id="records"),
        pytest.param({"data": [{}], "metadata": {"totalCount": 30, "pageSize": 1}}, 30, id="meta"),
        pytest.param({"totalCount": 0, "value": []}, 0, id="object, total 0"),
        pytest.param({"value": []}, 0, id="object, empty page"),
        pytest.param({"records": [{"metadata": {"totalCount": 258}}]}, 258, id="records row meta"),
        pytest.param({"value": [{"id": 1}]}, None, id="object rows without total"),
    ],
)
def test_total_is_read_from_where_the_answer_carries_it(body: Any, expected: int | None) -> None:
    assert extract_total(body) == expected


@pytest.mark.unit
@pytest.mark.parametrize("body", [None, "x", 7, {"status": "ok"}, {}])
def test_a_shape_that_is_not_understood_is_an_error_not_no_total(body: Any) -> None:
    with pytest.raises(ValueError, match="unexpected answer"):
        extract_total(body)


@pytest.mark.unit
def test_booleans_and_negative_numbers_are_not_totals() -> None:
    assert extract_total([{"metadata": {"totalCount": True}}]) is None
    assert extract_total({"totalCount": -1, "value": [{}]}) is None


@pytest.mark.unit
def test_row_count_sees_an_endpoint_that_ignores_page_size() -> None:
    assert row_count([{}] * 54) == 54
    assert row_count({"value": [{}], "totalCount": 9}) == 1
    assert row_count({"totalCount": 9}) == 0
    assert row_count(None) == 0


# The request


def config(name: str, path: str, **extra: Any) -> dict[str, Any]:
    return {"endpoint_name": name, "api_endpoint": path, **extra}


@pytest.mark.unit
def test_expand_flags_are_left_out_and_filters_are_kept() -> None:
    query = {
        "expandApplications": "true",
        "expand": ["All"],
        "ledger": "ACTUAL",
        "fromFinPeriod": "200001",
        "includeTransactionBalance": True,
        "empty": None,
    }

    assert derive_filters(query) == {
        "ledger": "ACTUAL",
        "fromFinPeriod": "200001",
        "includeTransactionBalance": "true",
    }


@pytest.mark.unit
def test_finance_one_row_query_pages_from_one() -> None:
    plan = plan_for(config("customer_invoices", "/v1/customerinvoice"))

    assert plan.url == "https://api.finance.visma.net/v1/customerinvoice"
    assert plan.scope == "erp"
    assert one_row_query(plan) == {"pageNumber": "1", "pageSize": "1"}


@pytest.mark.unit
def test_sales_order_one_row_query_pages_from_zero_and_uses_its_own_scope() -> None:
    plan = plan_for(
        config(
            "sales_orders_v3",
            "https://salesorder.visma.net/api/v3/SalesOrders",
            default_query={"expand": ["All", "LineFees"]},
        )
    )

    assert plan.sales_order
    assert plan.scope == "sales-order"
    assert one_row_query(plan) == {"pageIndex": "0", "pageSize": "1"}


@pytest.mark.unit
def test_general_ledger_transactions_are_counted_over_every_period_the_fetch_walks() -> None:
    plan = plan_for(
        config(
            "general_ledger_transactions",
            "/v1/GeneralLedgerTransactions",
            default_query={"ledger": "1", "lastModifiedDateTime": "2020-01-01 00:00:00"},
            period_scan={"first_period": "202001", "query_params": ["fromPeriod", "toPeriod"]},
        )
    )

    assert dict(one_row_query(plan)) == {
        "ledger": "1",
        "fromPeriod": "202001",
        "toPeriod": "203012",
        "pageNumber": "1",
        "pageSize": "1",
    }


@pytest.mark.unit
def test_finance_modified_filter_uses_utc_to_the_second_with_the_greater_or_equal_condition() -> (
    None
):
    oslo = timezone(timedelta(hours=2))
    since = datetime(2026, 10, 1, 15, 50, 35, 570527, tzinfo=oslo)

    assert modified_query(False, since) == {
        "lastModifiedDateTime": "2026-10-01 13:50:35",
        "lastModifiedDateTimeCondition": ">=",
    }


@pytest.mark.unit
def test_sales_order_modified_filter_is_modified_since() -> None:
    assert modified_query(True, LAST_WRITE) == {"modifiedSince": "2026-10-01T13:50:35Z"}


@pytest.mark.unit
def test_modified_filter_is_added_to_the_one_row_query() -> None:
    plan = plan_for(config("contacts", "/v1/contact"))

    assert one_row_query(plan, LAST_WRITE) == {
        "pageNumber": "1",
        "pageSize": "1",
        "lastModifiedDateTime": "2026-10-01 13:50:35",
        "lastModifiedDateTimeCondition": ">=",
    }


# The last write


@pytest.mark.unit
def test_a_heartbeat_wins_over_the_last_staging_write() -> None:
    beat = LAST_WRITE + timedelta(days=3)

    assert last_write(Staged(5, LAST_WRITE), beat) == (beat, "heartbeat")


@pytest.mark.unit
def test_without_a_heartbeat_the_last_staging_write_stands_in_and_says_so() -> None:
    when, source = last_write(Staged(5, LAST_WRITE), None)

    assert when == LAST_WRITE
    assert "no heartbeat" in source


@pytest.mark.unit
def test_no_rows_and_no_heartbeat_is_no_write_on_record() -> None:
    assert last_write(None, None) == (None, "none")
    assert last_write(Staged(0, None), None) == (None, "none")


# The client: retries, token refresh, and that nothing from a request reaches an error


def http_error(code: int) -> urllib.error.HTTPError:
    return urllib.error.HTTPError("https://api.finance.visma.net/v1/x?q=1", code, "m", {}, None)  # type: ignore[arg-type]


class Script:
    """A send() that plays back a list of results; an exception in the list is raised."""

    def __init__(self, *steps: Any) -> None:
        self.steps = list(steps)
        self.sent: list[tuple[str, dict[str, str]]] = []

    def __call__(self, url: str, headers: dict[str, str]) -> Any:
        self.sent.append((url, headers))
        step = self.steps.pop(0)
        if isinstance(step, BaseException):
            raise step
        return step


def client_for(send: Script) -> tuple[Visma, list[float], list[str]]:
    sleeps: list[float] = []
    issued: list[str] = []

    def get_token(scope: str, tenant: str) -> str:
        issued.append(f"secret-token-{len(issued) + 1}")
        return issued[-1]

    visma = Visma(
        TENANT, get_token=get_token, send=send, sleep=sleeps.append, pace=lambda _k, act: act()
    )
    return visma, sleeps, issued


@pytest.mark.unit
def test_request_carries_the_headers_the_fetch_sends() -> None:
    send = Script([{"ok": True}])
    visma, _sleeps, _issued = client_for(send)

    body = visma.get_json("https://api.finance.visma.net/v1/x", {"pageSize": "1"}, "erp")

    url, headers = send.sent[0]
    assert body == [{"ok": True}]
    assert url == "https://api.finance.visma.net/v1/x?pageSize=1"
    assert headers["ipp-company-id"] == TENANT
    assert headers["ipp-application-type"] == "Visma.net Financials"
    assert headers["Authorization"] == "Bearer secret-token-1"


@pytest.mark.unit
def test_a_transient_status_is_retried_after_the_waits() -> None:
    send = Script(http_error(503), http_error(429), [1])
    visma, sleeps, _issued = client_for(send)

    assert visma.get_json("https://h/x", {}, "erp") == [1]
    assert sleeps == [5, 15]


@pytest.mark.unit
def test_a_persistent_transient_status_ends_in_an_error_with_the_status_only() -> None:
    send = Script(*[http_error(500)] * 4)
    visma, sleeps, issued = client_for(send)

    with pytest.raises(VismaError) as caught:
        visma.get_json("https://api.finance.visma.net/v1/x", {"q": "1"}, "erp")

    assert sleeps == [5, 15, 45]
    assert caught.value.status == 500
    assert str(caught.value) == "HTTP 500"
    assert issued[0] not in repr(caught.value)


@pytest.mark.unit
def test_a_401_is_retried_once_with_a_new_token() -> None:
    send = Script(http_error(401), [1])
    visma, _sleeps, issued = client_for(send)

    assert visma.get_json("https://h/x", {}, "erp") == [1]
    assert issued == ["secret-token-1", "secret-token-2"]
    assert send.sent[1][1]["Authorization"] == "Bearer secret-token-2"


@pytest.mark.unit
def test_a_second_401_is_an_error() -> None:
    send = Script(http_error(401), http_error(401))
    visma, _sleeps, _issued = client_for(send)

    with pytest.raises(VismaError, match="HTTP 401"):
        visma.get_json("https://h/x", {}, "erp")


@pytest.mark.unit
def test_a_400_or_404_is_not_retried() -> None:
    for code in (400, 404):
        send = Script(http_error(code))
        visma, sleeps, _issued = client_for(send)

        with pytest.raises(VismaError) as caught:
            visma.get_json("https://h/x", {}, "erp")

        assert caught.value.status == code
        assert sleeps == []
        assert len(send.sent) == 1


@pytest.mark.unit
def test_timeouts_are_retried_and_reported_by_type_only() -> None:
    send = Script(*[TimeoutError("read timed out: https://h/x?secret=1")] * 4)
    visma, sleeps, _issued = client_for(send)

    with pytest.raises(VismaError) as caught:
        visma.get_json("https://h/x", {}, "erp")

    assert sleeps == [5, 15, 45]
    assert str(caught.value) == "request failed: TimeoutError"
    assert "secret" not in repr(caught.value)


@pytest.mark.unit
def test_an_answer_that_is_not_json_is_an_error_without_a_retry() -> None:
    send = Script(json.JSONDecodeError("Expecting value: <html>", "<html>", 0))
    visma, sleeps, _issued = client_for(send)

    with pytest.raises(VismaError, match="not JSON") as caught:
        visma.get_json("https://h/x", {}, "erp")

    assert sleeps == []
    assert "html" not in str(caught.value)


@pytest.mark.unit
def test_tokens_are_kept_per_scope() -> None:
    send = Script([1], [2], [3])
    visma, _sleeps, issued = client_for(send)

    visma.get_json("https://h/a", {}, "erp")
    visma.get_json("https://h/b", {}, "sales-order")
    visma.get_json("https://h/c", {}, "erp")

    assert issued == ["secret-token-1", "secret-token-2"]


# The modified-filter guard and one endpoint across tenants


class FakeVisma:
    """Answers by what is asked: a plain request gets the total, a modified request the count."""

    def __init__(self, total: Any, changed: Any = None, future: Any = None) -> None:
        self.total, self.changed, self.future = total, changed, future
        self.queries: list[dict[str, str]] = []

    def get_json(self, url: str, query: dict[str, str], scope: str) -> Any:
        self.queries.append(query)
        stamp = query.get("lastModifiedDateTime") or query.get("modifiedSince") or ""
        if stamp.startswith("2999"):
            return self.future
        if stamp:
            return self.changed
        return self.total


def listing(total: int) -> list[dict[str, Any]]:
    return [{"metadata": {"totalCount": total}}]


PLAN = plan_for(config("customer_invoices", "/v1/customerinvoice"))


def run_one(
    fake: FakeVisma,
    staged: Staged | None,
    beat: datetime | None = None,
    guard: FilterGuard | None = None,
) -> EndpointResult:
    targets = [(TENANT, "Aquatiq AS (pilot)")]
    return check_endpoint(
        PLAN,
        targets,
        {TENANT: staged} if staged is not None else {},
        {(TENANT, PLAN.endpoint): beat} if beat is not None else {},
        lambda _tenant: fake,  # type: ignore[arg-type,return-value]
        guard or FilterGuard(),
    )


@pytest.mark.unit
def test_guard_trusts_a_filter_that_answers_zero_for_the_year_2999_and_asks_once() -> None:
    fake = FakeVisma(listing(10), future=[])
    guard = FilterGuard()

    assert guard.works(fake, PLAN)  # type: ignore[arg-type]
    assert guard.works(fake, PLAN)  # type: ignore[arg-type]
    assert len(fake.queries) == 1
    assert guard.state(PLAN.endpoint) == "works"


@pytest.mark.unit
def test_guard_rejects_a_filter_that_is_ignored() -> None:
    fake = FakeVisma(listing(10), future=listing(10))
    guard = FilterGuard()

    assert not guard.works(fake, PLAN)  # type: ignore[arg-type]
    assert guard.state(PLAN.endpoint) == "ignored"


class Failing(FakeVisma):
    """Every request fails with the given status (None: a network failure)."""

    def __init__(self, status: int | None) -> None:
        super().__init__(None)
        self.status = status

    def get_json(self, url: str, query: dict[str, str], scope: str) -> Any:
        self.queries.append(query)
        raise VismaError(f"HTTP {self.status}" if self.status else "request failed", self.status)


@pytest.mark.unit
def test_guard_takes_a_refused_filter_as_no_filter_and_remembers_it() -> None:
    fake = Failing(400)
    guard = FilterGuard()

    assert not guard.works(fake, PLAN)  # type: ignore[arg-type]
    assert not guard.works(fake, PLAN)  # type: ignore[arg-type]
    assert len(fake.queries) == 1
    assert guard.state(PLAN.endpoint) == "ignored"


@pytest.mark.unit
@pytest.mark.parametrize("status", [401, 403, 429, 500, 503, None])
def test_guard_does_not_remember_a_probe_that_failed_for_another_reason(status: int | None) -> None:
    fake = Failing(status)
    guard = FilterGuard()

    with pytest.raises(VismaError):
        guard.works(fake, PLAN)  # type: ignore[arg-type]
    with pytest.raises(VismaError):
        guard.works(fake, PLAN)  # type: ignore[arg-type]

    assert len(fake.queries) == 2  # asked again, not cached
    assert guard.state(PLAN.endpoint) == "not needed"


@pytest.mark.unit
def test_a_failed_probe_is_an_error_cell_and_the_next_tenant_asks_again() -> None:

    class ProbeFails(FakeVisma):
        """Answers the total, but every request with the modified filter fails with HTTP 503."""

        def get_json(self, url: str, query: dict[str, str], scope: str) -> Any:
            if "lastModifiedDateTime" in query:
                raise VismaError("HTTP 503", 503)
            return super().get_json(url, query, scope)

    answers = {
        TENANT: ProbeFails(listing(100)),
        OTHER: FakeVisma(listing(100), changed=listing(9), future=[]),
    }
    guard = FilterGuard()
    result = check_endpoint(
        PLAN,
        [(TENANT, "A"), (OTHER, "B")],
        {TENANT: Staged(95, LAST_WRITE), OTHER: Staged(95, LAST_WRITE)},
        {},
        lambda tenant: answers[tenant],  # type: ignore[arg-type,return-value]
        guard,
    )

    first, second = result.cells
    assert (first.verdict, first.reason) == ("ERROR", "HTTP 503")
    assert (second.verdict, second.changed, second.basis) == ("PASS", 9, BASIS_FILTER)
    assert guard.state(PLAN.endpoint) == "works"


@pytest.mark.unit
def test_a_gap_within_the_changes_since_the_last_write_passes() -> None:
    fake = FakeVisma(listing(100), changed=listing(7), future=[])

    result = run_one(fake, Staged(95, LAST_WRITE))

    (cell,) = result.cells
    assert (cell.verdict, cell.missing, cell.changed, cell.basis) == ("PASS", 5, 7, BASIS_FILTER)
    assert cell.last_write == "2026-10-01T13:50:35+00:00"
    assert "no heartbeat" in cell.last_write_source
    assert result.modified_filter == "works"
    assert len(fake.queries) == 3  # the total, the filter probe, the changed count


@pytest.mark.unit
def test_a_gap_beyond_the_changes_since_the_last_write_fails() -> None:
    fake = FakeVisma(listing(100), changed=listing(5), future=[])

    (cell,) = run_one(fake, Staged(80, LAST_WRITE)).cells

    assert (cell.verdict, cell.missing, cell.changed) == ("FAIL", 20, 5)


@pytest.mark.unit
def test_a_changed_count_of_zero_is_an_empty_page_and_leaves_the_gap_a_failure() -> None:
    fake = FakeVisma(listing(100), changed=[], future=[])

    (cell,) = run_one(fake, Staged(99, LAST_WRITE)).cells

    assert (cell.verdict, cell.changed) == ("FAIL", 0)


@pytest.mark.unit
def test_an_ignored_filter_gives_tolerance_zero_so_any_gap_fails() -> None:
    fake = FakeVisma(listing(6_370), changed=listing(6_370), future=listing(6_370))

    result = run_one(fake, Staged(6_360, LAST_WRITE))

    (cell,) = result.cells
    assert (cell.verdict, cell.changed, cell.basis) == ("FAIL", None, BASIS_NO_FILTER)
    assert result.modified_filter == "ignored"
    assert len(fake.queries) == 2  # the total and the probe; the changed count is not asked


@pytest.mark.unit
def test_nothing_missing_asks_nothing_more() -> None:
    fake = FakeVisma(listing(50))

    result = run_one(fake, Staged(50, LAST_WRITE))

    (cell,) = result.cells
    assert (cell.verdict, cell.basis) == ("PASS", BASIS_NOT_NEEDED)
    assert len(fake.queries) == 1
    assert result.modified_filter == "not needed"


@pytest.mark.unit
def test_staging_ahead_of_visma_warns_and_asks_nothing_more() -> None:
    fake = FakeVisma(listing(100))

    result = run_one(fake, Staged(103, LAST_WRITE))

    assert [c.verdict for c in result.cells] == ["WARN"]
    assert len(fake.queries) == 1


@pytest.mark.unit
def test_a_tenant_with_nothing_in_staging_and_records_in_visma_fails_never_fetched() -> None:
    fake = FakeVisma(listing(40))

    (cell,) = run_one(fake, None).cells

    assert (cell.verdict, cell.missing, cell.basis) == ("FAIL", 40, BASIS_NO_WRITE)
    assert cell.last_write is None
    assert len(fake.queries) == 1


@pytest.mark.unit
def test_an_empty_tenant_in_both_places_passes() -> None:
    (cell,) = run_one(FakeVisma([]), None).cells

    assert (cell.verdict, cell.visma_total, cell.staging_total) == ("PASS", 0, 0)


@pytest.mark.unit
def test_a_heartbeat_sets_the_last_write_the_changed_count_is_asked_from() -> None:
    beat = datetime(2026, 10, 9, 17, 10, 53, tzinfo=UTC)
    fake = FakeVisma(listing(100), changed=listing(9), future=[])

    (cell,) = run_one(fake, Staged(95, LAST_WRITE), beat=beat).cells

    assert cell.last_write_source == "heartbeat"
    assert fake.queries[-1]["lastModifiedDateTime"] == "2026-10-09 17:10:53"


@pytest.mark.unit
def test_an_answer_with_rows_and_no_total_makes_the_endpoint_not_countable() -> None:
    fake = FakeVisma([{"departmentId": "1"}] * 54)

    result = run_one(fake, Staged(54, LAST_WRITE))

    assert result.cells == ()
    assert result.not_countable is not None
    assert "ignores pageSize" in result.not_countable
    assert "54 rows" in result.not_countable


@pytest.mark.unit
def test_cells_of_tenants_before_the_first_no_total_answer_are_dropped() -> None:
    """An empty answer is a real 0, but an endpoint that has no total reports no cells at all."""
    answers = {OTHER: FakeVisma([]), TENANT: FakeVisma([{"departmentId": "1"}])}

    result = check_endpoint(
        PLAN,
        [(OTHER, "B"), (TENANT, "A")],
        {},
        {},
        lambda tenant: answers[tenant],  # type: ignore[arg-type,return-value]
        FilterGuard(),
    )

    assert result.cells == ()
    assert result.not_countable is not None


@pytest.mark.unit
def test_a_bug_inside_an_endpoint_becomes_error_cells_that_name_only_the_exception_type() -> None:
    def broken(_tenant: str) -> Visma:
        raise KeyError("https://h/x?token=secret")

    result = guarded_check(
        PLAN,
        [(TENANT, "A"), (OTHER, "B")],
        {TENANT: Staged(4, LAST_WRITE)},
        {},
        broken,
        FilterGuard(),
    )

    assert [(c.tenant, c.verdict, c.reason) for c in result.cells] == [
        (TENANT, "ERROR", "gate error: KeyError"),
        (OTHER, "ERROR", "gate error: KeyError"),
    ]
    assert result.cells[0].staging_total == 4


@pytest.mark.unit
def test_a_paged_answer_without_a_total_is_not_countable_either() -> None:
    result = run_one(FakeVisma([{"referenceNbr": "1"}]), Staged(490, LAST_WRITE))

    assert result.not_countable == "no total: the one-row answer has a row but no totalCount"


@pytest.mark.unit
def test_an_http_error_is_an_error_cell_that_keeps_the_status_and_fails_nothing_else() -> None:
    class Refusing(FakeVisma):
        def get_json(self, url: str, query: dict[str, str], scope: str) -> Any:
            raise VismaError("HTTP 403", 403)

    (cell,) = run_one(Refusing(None), Staged(3, LAST_WRITE)).cells

    assert (cell.verdict, cell.reason, cell.visma_total, cell.staging_total) == (
        "ERROR",
        "HTTP 403",
        None,
        3,
    )


@pytest.mark.unit
def test_an_unreadable_shape_is_an_error_cell() -> None:
    (cell,) = run_one(FakeVisma("<html>"), Staged(3, LAST_WRITE)).cells

    assert cell.verdict == "ERROR"
    assert "unexpected answer shape str" in cell.reason


@pytest.mark.unit
def test_each_tenant_gets_its_own_cell_and_one_failure_does_not_stop_the_rest() -> None:
    answers = {TENANT: FakeVisma(listing(10)), OTHER: FakeVisma([])}
    targets = [(TENANT, "A"), (OTHER, "B")]

    result = check_endpoint(
        PLAN,
        targets,
        {TENANT: Staged(10, LAST_WRITE)},
        {},
        lambda tenant: answers[tenant],  # type: ignore[arg-type,return-value]
        FilterGuard(),
    )

    assert [(c.tenant, c.verdict) for c in result.cells] == [(TENANT, "PASS"), (OTHER, "PASS")]


# The configs and the report


@pytest.mark.unit
def test_every_not_countable_endpoint_and_count_filter_is_a_real_erp_endpoint() -> None:
    configs = load_configs()

    assert set(NOT_COUNTABLE) <= set(configs)
    assert set(WINDOWED) <= set(configs)


@pytest.mark.unit
def test_every_other_erp_endpoint_gets_a_plan_with_a_usable_url_and_table_name() -> None:
    configs = load_configs()

    for name, cfg in configs.items():
        assert re.fullmatch(r"[a-z0-9_]+", name), name
        if name in NOT_COUNTABLE:
            continue
        plan = plan_for(cfg)
        assert plan.url.startswith("https://"), name
        assert "{" not in plan.url, name


@pytest.mark.unit
def test_a_report_lists_counts_flagged_cells_and_the_not_countable_endpoints() -> None:
    ok = Cell("a", TENANT, "Pilot", 10, 10, 0, None, BASIS_NOT_NEEDED, None, "none", "PASS", "none")
    near = Cell("a", TENANT, "Near", 10, 8, 2, 5, BASIS_FILTER, None, "none", "PASS", "2 missing")
    bad = Cell(
        "a", OTHER, "Other", 12, 9, 3, None, BASIS_NO_FILTER, None, "none", "FAIL", "3 missing"
    )
    results = [
        EndpointResult("a", "finance list, one row", (ok, near, bad), None, "ignored"),
        EndpointResult("b", "-", (), "needs a parent key", "-"),
    ]
    info = {"day": "2026-10-12", "write_source": "the last staging write", "calls": {"GET": 3}}

    text = render(results, ["Pilot", "Near", "Other"], info)

    assert "| a | 2 | 0 | 1 | 0 | 32 | 27 | 5 | 5 | ignored | finance list, one row |" in text
    assert "| a | Other | 12 | 9 | 3 | - |" in text
    assert "| b | needs a parent key |" in text
    assert "1 endpoints counted for 3 tenants (3 cells); 1 NOT COUNTABLE" in text
    assert "PASS 2 (1 exact, 1 within the tolerance), WARN 0, FAIL 1, ERROR 0" in text
    assert json.dumps([asdict(ok), asdict(near), asdict(bad)])


@pytest.mark.unit
def test_a_report_without_flagged_cells_says_so() -> None:
    ok = Cell("a", TENANT, "Pilot", 10, 10, 0, None, BASIS_NOT_NEEDED, None, "none", "PASS", "none")
    info = {"day": "2026-10-12", "write_source": "x", "calls": {}}

    text = render([EndpointResult("a", "m", (ok,), None, "not needed")], ["Pilot"], info)

    assert "## Cells that are not PASS\n\nNone.\n" in text


@pytest.mark.unit
def test_gate_exposes_the_staging_dataset_it_reads() -> None:
    assert gate.STAGING == "aquatiq-datavarehus.dw_staging_visma"
    assert gate.HEARTBEAT.endswith("._sync_heartbeat")
