from etl.xero import reports


def _record(contact_id: str) -> dict:
    return {
        "tenant_id": "tenant-1",
        "first_seen_at": "2026-10-18T08:00:00Z",
        "last_seen_at": "2026-10-18T08:00:00Z",
        "meta": {
            "x-report": "report_aged_receivables_by_contact",
            "x-report-params": f"contactId={contact_id}&date=2026-10-18",
            "x-run-id": "run-1",
        },
        "payload": {
            "ReportID": "AgedReceivablesByContact",
            "ReportName": "Aged Receivables By Contact",
            "Rows": [{"RowType": "Row", "Cells": [{"Value": "Customer"}]}],
        },
    }


def test_contact_scoped_report_snapshot_identity_includes_parameter_signature():
    first = _record("contact-a")
    second = _record("contact-b")

    first_header = reports.parse_header(first)
    second_header = reports.parse_header(second)
    assert first_header["record_id"] != second_header["record_id"]
    assert first_header["contact_id"] == "contact-a"
    assert second_header["contact_id"] == "contact-b"
    assert first_header["report_params"] == "contactId=contact-a&date=2026-10-18"
    assert first_header["report_parameter_hash"] != second_header["report_parameter_hash"]

    first_rows = reports.parse_rows(first)
    second_rows = reports.parse_rows(second)
    assert first_rows[0]["record_id"] == first_header["record_id"]
    assert second_rows[0]["record_id"] == second_header["record_id"]
    assert first_rows[0]["contact_id"] == "contact-a"
    assert second_rows[0]["contact_id"] == "contact-b"
