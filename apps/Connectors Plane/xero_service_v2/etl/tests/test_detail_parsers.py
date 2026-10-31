from datetime import date, datetime, timezone

from etl.xero import budgets, contact_groups


def _record(payload, record_id):
    synced_at = datetime(2026, 1, 1, tzinfo=timezone.utc)
    return {
        "tenant_id": "tenant-1",
        "record_id": record_id,
        "payload": payload,
        "last_seen_at": synced_at,
        "first_seen_at": synced_at,
    }


def test_budget_detail_yields_period_balances():
    record = _record({
        "BudgetID": "budget-1", "Description": "FY26", "Type": "OVERALL",
        "UpdatedDateUTC": "2026-01-05T12:30:00",
        "BudgetLines": [{
            "AccountID": "account-1", "AccountCode": "200",
            "BudgetBalances": [
                {"Period": "2026-01", "Amount": "1000.25", "UnitAmount": "1.25", "Notes": "Plan"},
                {"Period": "2026-02", "Amount": "1200"},
            ],
        }],
    }, "budget-1")
    header = budgets.parse_header(record)
    assert header["budget_line_count"] == 1
    assert header["updated_at"] == datetime(2026, 1, 5, 12, 30, tzinfo=timezone.utc)
    rows = budgets.parse_balances(record)
    assert len(rows) == 2
    assert rows[0]["account_id"] == "account-1"
    assert rows[0]["period"] == "2026-01"
    assert rows[0]["amount"] == "1000.25"
    assert rows[0]["unit_amount"] == "1.25"
    assert rows[0]["period_date_local"] == date(2026, 1, 1)
    assert rows[1]["period_date_local"] == date(2026, 2, 1)


def test_contact_group_detail_yields_members():
    record = _record({
        "ContactGroupID": "group-1", "Name": "Suppliers", "Status": "ACTIVE",
        "Contacts": [{"ContactID": "contact-1", "Name": "Vendor"}],
    }, "group-1")
    assert contact_groups.parse_header(record)["member_count"] == 1
    rows = contact_groups.parse_members(record)
    assert len(rows) == 1
    assert rows[0]["contact_id"] == "contact-1"
    assert rows[0]["contact_name"] == "Vendor"


def test_detail_parsers_route_headers_and_snapshot_rows():
    class Reader:
        def __init__(self, record):
            self.record = record

        def iter_records(self, *_args, **_kwargs):
            yield self.record

    class Writer:
        def __init__(self):
            self.calls = []

        def merge(self, table, rows, key_columns=("tenant_id", "record_id")):
            self.calls.append((table, rows, key_columns))

    budget = _record({
        "BudgetID": "budget-1", "BudgetLines": [{
            "AccountID": "account-1", "BudgetBalances": [{"Period": "2026-01", "Amount": "1"}],
        }],
    }, "budget-1")
    group = _record({
        "ContactGroupID": "group-1", "Contacts": [{"ContactID": "contact-1"}],
    }, "group-1")

    writer = Writer()
    assert budgets.run(Reader(budget), writer) == {"headers": 1, "balances": 1}
    assert [call[0] for call in writer.calls] == ["budgets", "budget_balance_snapshots"]
    assert "synced_at" in writer.calls[1][2]

    writer = Writer()
    assert contact_groups.run(Reader(group), writer) == {"headers": 1, "members": 1}
    assert [call[0] for call in writer.calls] == ["contact_groups", "contact_group_member_snapshots"]
    assert "synced_at" in writer.calls[1][2]
